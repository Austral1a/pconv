use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;

use crate::format::{is_readable_extension, INPUT_ONLY_EXTENSIONS, OUTPUT_EXTENSIONS, TargetFormat};

#[derive(Debug)]
pub enum ConvertError {
    NoMatches(String),
    NotAnImage(PathBuf),
    InvalidPattern(String),
    Collisions(Vec<(PathBuf, Vec<PathBuf>)>),
    GlobPattern(glob::PatternError),
    Glob(glob::GlobError),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMatches(pattern) => {
                write!(f, "no image files matched `{pattern}`")
            }
            Self::NotAnImage(path) => {
                write!(f, "{} is not a supported image format", path.display())
            }
            Self::InvalidPattern(message) => write!(f, "{message}"),
            Self::Collisions(collisions) => {
                write!(f, "output path collision")?;
                for (dest, sources) in collisions {
                    let sources = sources
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    write!(f, "\n  {} would be written by {sources}", dest.display())?;
                }
                Ok(())
            }
            Self::GlobPattern(error) => write!(f, "invalid upload glob: {error}"),
            Self::Glob(error) => write!(f, "failed to expand upload glob: {error}"),
        }
    }
}

impl std::error::Error for ConvertError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffloadPattern {
    prefix: String,
    suffix: String,
    extension: String,
    target: TargetFormat,
    /// A `*` pattern uses the filename stem only. A directory keeps the source layout.
    flatten: bool,
}

impl OffloadPattern {
    pub fn parse(pattern: &str, output_ext: &str) -> Result<Self, ConvertError> {
        let star_count = pattern.chars().filter(|ch| *ch == '*').count();
        if star_count > 1 {
            return Err(ConvertError::InvalidPattern(
                "offload pattern must contain at most one `*`".to_string(),
            ));
        }

        let (prefix, suffix, flatten) = if let Some(star) = pattern.find('*') {
            (
                pattern[..star].to_string(),
                pattern[star + 1..].to_string(),
                true,
            )
        } else {
            (ensure_trailing_slash(pattern), String::new(), false)
        };
        let extension = normalize_ext(output_ext);
        if extension.is_empty() {
            return Err(ConvertError::InvalidPattern(
                "output extension must not be empty".to_string(),
            ));
        }
        let target = target_from_output_ext(&extension)?;

        Ok(Self {
            prefix,
            suffix,
            extension,
            target,
            flatten,
        })
    }

    pub fn map(&self, source: &Path, root: &Path) -> PathBuf {
        let stem = if self.flatten {
            source
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            let mut relative = source
                .strip_prefix(root)
                .unwrap_or(source)
                .to_path_buf();
            relative.set_extension("");
            relative.to_string_lossy().into_owned()
        };
        let mut dest = PathBuf::from(format!("{}{stem}{}", self.prefix, self.suffix));
        dest.set_extension(&self.extension);
        dest
    }

    pub fn target(&self) -> TargetFormat {
        self.target
    }
}

#[derive(Debug, Default)]
pub struct RunResult {
    pub successes: Vec<PathBuf>,
    pub failures: Vec<(PathBuf, String)>,
}

pub fn run(
    upload_from: &str,
    offload_to: &str,
    output_ext: &str,
    workers: usize,
) -> Result<RunResult, ConvertError> {
    let pattern = OffloadPattern::parse(offload_to, output_ext)?;
    let root = upload_root(upload_from);
    let sources = collect_sources(upload_from)?;
    let planned = plan_outputs(&sources, &pattern, &root)?;
    Ok(convert_planned(&planned, pattern.target(), workers))
}

enum Outcome {
    Done(PathBuf),
    Failed(PathBuf, String),
}

fn convert_planned(
    planned: &[(PathBuf, PathBuf)],
    target: TargetFormat,
    workers: usize,
) -> RunResult {
    if planned.is_empty() {
        return RunResult::default();
    }

    let workers = workers.clamp(1, planned.len());
    let slots: Mutex<Vec<Option<Outcome>>> =
        Mutex::new((0..planned.len()).map(|_| None).collect());
    let next = AtomicUsize::new(0);
    let started = AtomicUsize::new(0);
    let loader = Mutex::new(Loader::new(planned.len()));

    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= planned.len() {
                        break;
                    }
                    let (source, dest) = &planned[index];
                    let shown = started.fetch_add(1, Ordering::Relaxed) + 1;
                    loader.lock().unwrap().tick(shown, source);
                    let outcome = match convert_one(source, dest, target) {
                        Ok(()) => Outcome::Done(dest.clone()),
                        Err(error) => Outcome::Failed(source.clone(), error),
                    };
                    slots.lock().unwrap()[index] = Some(outcome);
                }
            });
        }
    });

    loader.lock().unwrap().clear();

    let mut result = RunResult::default();
    for outcome in slots.into_inner().unwrap() {
        match outcome {
            Some(Outcome::Done(dest)) => result.successes.push(dest),
            Some(Outcome::Failed(source, error)) => result.failures.push((source, error)),
            None => unreachable!("worker left a conversion unfinished"),
        }
    }
    result
}

pub fn collect_sources(pattern: &str) -> Result<Vec<PathBuf>, ConvertError> {
    let path = Path::new(pattern);
    if path.is_file() {
        if !is_image_file(path) {
            return Err(ConvertError::NotAnImage(path.to_path_buf()));
        }
        return Ok(vec![path.to_path_buf()]);
    }

    let glob_pattern = if path.is_dir() {
        let dir = ensure_trailing_slash(pattern);
        format!("{dir}**/*")
    } else {
        pattern.to_string()
    };
    let entries = glob::glob(&glob_pattern).map_err(ConvertError::GlobPattern)?;
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(ConvertError::Glob)?;
        if path.is_file() && is_image_file(&path) {
            files.push(path);
        }
    }
    if files.is_empty() {
        return Err(ConvertError::NoMatches(pattern.to_string()));
    }
    files.sort();
    Ok(files)
}

fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(is_readable_extension)
}

pub fn plan_outputs(
    sources: &[PathBuf],
    pattern: &OffloadPattern,
    root: &Path,
) -> Result<Vec<(PathBuf, PathBuf)>, ConvertError> {
    let pairs: Vec<(PathBuf, PathBuf)> = sources
        .iter()
        .map(|source| (source.clone(), pattern.map(source, root)))
        .collect();

    let mut by_dest: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for (source, dest) in &pairs {
        by_dest.entry(dest.clone()).or_default().push(source.clone());
    }

    let collisions: Vec<(PathBuf, Vec<PathBuf>)> = by_dest
        .into_iter()
        .filter(|(_, sources)| sources.len() > 1)
        .collect();
    if !collisions.is_empty() {
        return Err(ConvertError::Collisions(collisions));
    }

    Ok(pairs)
}

fn upload_root(pattern: &str) -> PathBuf {
    let path = Path::new(pattern);
    if path.is_file() {
        return path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
    }
    if path.is_dir() {
        return path.to_path_buf();
    }
    let Some(meta) = pattern.find(['*', '?', '[']) else {
        return path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
    };
    let prefix = &pattern[..meta];
    if prefix.is_empty() {
        return PathBuf::new();
    }
    let prefix_path = Path::new(prefix);
    if prefix.ends_with('/') || prefix.ends_with('\\') {
        prefix_path.to_path_buf()
    } else {
        prefix_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf()
    }
}

fn ensure_trailing_slash(path: &str) -> String {
    if path.ends_with('/') || path.ends_with('\\') {
        path.to_string()
    } else {
        format!("{path}/")
    }
}

fn normalize_ext(ext: &str) -> String {
    ext.trim().trim_start_matches('.').to_ascii_lowercase()
}

fn target_from_output_ext(ext: &str) -> Result<TargetFormat, ConvertError> {
    let ext = normalize_ext(ext);
    match TargetFormat::from_ext(&ext) {
        Some(target) if target.can_encode() => Ok(target),
        Some(target) => Err(ConvertError::InvalidPattern(format!(
            "`{}` can be decoded but not encoded; writable: {}",
            target.name(),
            OUTPUT_EXTENSIONS.join(", ")
        ))),
        None => Err(ConvertError::InvalidPattern(format!(
            "unknown output extension `{ext}`; writable: {}; read-only: {}",
            OUTPUT_EXTENSIONS.join(", "),
            INPUT_ONLY_EXTENSIONS.join(", ")
        ))),
    }
}

struct Loader {
    enabled: bool,
    total: usize,
    last_width: usize,
}

impl Loader {
    fn new(total: usize) -> Self {
        Self {
            enabled: io::stderr().is_terminal(),
            total,
            last_width: 0,
        }
    }

    fn tick(&mut self, index: usize, source: &Path) {
        if !self.enabled {
            return;
        }
        const FRAMES: [&str; 4] = ["|", "/", "-", "\\"];
        let frame = FRAMES[(index - 1) % FRAMES.len()];
        let name = shorten_path(source, 60);
        let line = format!("{frame} converting {index}/{total}  {name}", total = self.total);
        self.draw(&line);
    }

    fn clear(&mut self) {
        if !self.enabled || self.last_width == 0 {
            return;
        }
        let pad = " ".repeat(self.last_width);
        let mut stderr = io::stderr().lock();
        let _ = write!(stderr, "\r{pad}\r");
        let _ = stderr.flush();
        self.last_width = 0;
    }

    fn draw(&mut self, line: &str) {
        let width = line.chars().count();
        let pad = self.last_width.saturating_sub(width);
        let mut stderr = io::stderr().lock();
        let _ = write!(stderr, "\r{line}{}", " ".repeat(pad));
        let _ = stderr.flush();
        self.last_width = width;
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.clear();
    }
}

fn shorten_path(path: &Path, max_chars: usize) -> String {
    let text = path.display().to_string();
    let count = text.chars().count();
    if count <= max_chars {
        return text;
    }
    let keep = max_chars.saturating_sub(1);
    let tail: String = text.chars().skip(count - keep).collect();
    format!("…{tail}")
}

fn convert_one(source: &Path, dest: &Path, target: TargetFormat) -> Result<(), String> {
    let image = crate::codecs::decode_image(source)?;
    crate::codecs::encode_image(&image, dest, target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn shortens_long_paths_from_the_end() {
        let path = Path::new("public/images/products/wall-formwork/gallery/01.jpg");
        let short = shorten_path(path, 20);
        assert!(short.starts_with('…'));
        assert!(short.ends_with("gallery/01.jpg"));
        assert!(short.chars().count() <= 20);
        assert_eq!(shorten_path(Path::new("01.jpg"), 20), "01.jpg");
    }

    #[test]
    fn maps_out_star_webp() {
        let pattern = OffloadPattern::parse("out/*", "webp").unwrap();
        assert_eq!(
            pattern.map(Path::new("photos/a/cat.png"), Path::new("photos")),
            PathBuf::from("out/cat.webp")
        );
    }

    #[test]
    fn maps_prefixed_star() {
        let pattern = OffloadPattern::parse("converted_*", "jpg").unwrap();
        assert_eq!(
            pattern.map(Path::new("cat.png"), Path::new("")),
            PathBuf::from("converted_cat.jpg")
        );
    }

    #[test]
    fn output_ext_replaces_pattern_extension() {
        let pattern = OffloadPattern::parse("out/*.png", "webp").unwrap();
        assert_eq!(
            pattern.map(Path::new("photos/a/cat.png"), Path::new("photos")),
            PathBuf::from("out/cat.webp")
        );
    }

    #[test]
    fn directory_offload_is_relative_or_absolute() {
        let relative = OffloadPattern::parse("out", "webp").unwrap();
        assert_eq!(
            relative.map(Path::new("photos/a/cat.png"), Path::new("photos")),
            PathBuf::from("out/a/cat.webp")
        );

        let dotted = OffloadPattern::parse("./converted", "jpg").unwrap();
        assert_eq!(
            dotted.map(Path::new("cat.png"), Path::new("")),
            PathBuf::from("./converted/cat.jpg")
        );

        let absolute = OffloadPattern::parse("/tmp/out", "png").unwrap();
        assert_eq!(
            absolute.map(
                Path::new("/tmp/in/photos/a/cat.png"),
                Path::new("/tmp/in")
            ),
            PathBuf::from("/tmp/out/photos/a/cat.png")
        );
    }

    #[test]
    fn directory_offload_keeps_same_filenames_apart() {
        let pattern = OffloadPattern::parse("public/out/", "avif").unwrap();
        let root = Path::new("public/images");
        let sources = vec![
            PathBuf::from("public/images/products/variflex/gallery/01.jpg"),
            PathBuf::from("public/images/products/toptower-40/gallery/01.jpg"),
        ];
        let planned = plan_outputs(&sources, &pattern, root).unwrap();
        assert_eq!(
            planned[0].1,
            PathBuf::from("public/out/products/variflex/gallery/01.avif")
        );
        assert_eq!(
            planned[1].1,
            PathBuf::from("public/out/products/toptower-40/gallery/01.avif")
        );
    }

    #[test]
    fn two_stars_fail() {
        let error = OffloadPattern::parse("out/*/*.webp", "webp").unwrap_err();
        assert!(matches!(error, ConvertError::InvalidPattern(_)));
    }

    #[test]
    fn extension_parsing() {
        let pattern = OffloadPattern::parse("out/*", ".JPEG").unwrap();
        assert_eq!(
            pattern.map(Path::new("cat.png"), Path::new("")),
            PathBuf::from("out/cat.jpeg")
        );
        assert_eq!(
            pattern.target(),
            TargetFormat::Image(image::ImageFormat::Jpeg)
        );
    }

    #[test]
    fn empty_extension_fails() {
        let error = OffloadPattern::parse("out/*", "").unwrap_err();
        assert!(matches!(error, ConvertError::InvalidPattern(_)));
    }

    #[test]
    fn unknown_extension_lists_supported_formats() {
        let error = OffloadPattern::parse("out/*", "xyz").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("xyz"));
        assert!(message.contains("png"));
    }

    #[test]
    fn read_only_extension_is_rejected_as_output() {
        let error = OffloadPattern::parse("out/*", "psd").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("decoded but not encoded"));
    }

    #[test]
    fn detects_output_path_collisions() {
        let pattern = OffloadPattern::parse("out/*", "webp").unwrap();
        let sources = vec![
            PathBuf::from("photos/a/cat.png"),
            PathBuf::from("photos/b/cat.jpg"),
        ];
        let error = plan_outputs(&sources, &pattern, Path::new("photos")).unwrap_err();
        match error {
            ConvertError::Collisions(collisions) => {
                assert_eq!(collisions.len(), 1);
                assert_eq!(collisions[0].0, PathBuf::from("out/cat.webp"));
                assert_eq!(collisions[0].1.len(), 2);
            }
            other => panic!("expected collision, got {other}"),
        }
    }

    #[test]
    fn converts_png_to_jpeg() {
        let dir = std::env::temp_dir().join(format!(
            "pconv-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("out")).unwrap();
        let source = dir.join("red.png");
        RgbImage::from_pixel(1, 1, Rgb([255, 0, 0]))
            .save_with_format(&source, image::ImageFormat::Png)
            .unwrap();

        let upload = format!("{}/red.png", dir.display());
        let offload = format!("{}/out/*", dir.display());
        let result = run(&upload, &offload, "jpg", 1).unwrap();
        assert!(result.failures.is_empty(), "{:?}", result.failures);
        assert_eq!(result.successes.len(), 1);

        let dest = &result.successes[0];
        assert!(dest.exists());
        let decoded = image::open(dest).unwrap();
        assert_eq!(decoded.width(), 1);
        assert_eq!(decoded.height(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn converts_with_multiple_workers() {
        let dir = std::env::temp_dir().join(format!(
            "pconv-workers-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        for name in ["a", "b", "c", "d"] {
            RgbImage::from_pixel(1, 1, Rgb([255, 0, 0]))
                .save_with_format(dir.join(format!("{name}.png")), image::ImageFormat::Png)
                .unwrap();
        }

        let out = dir.join("out");
        let result = run(dir.to_str().unwrap(), out.to_str().unwrap(), "jpg", 4).unwrap();
        assert!(result.failures.is_empty(), "{:?}", result.failures);
        assert_eq!(
            result.successes,
            vec![
                out.join("a.jpg"),
                out.join("b.jpg"),
                out.join("c.jpg"),
                out.join("d.jpg"),
            ]
        );
        for dest in &result.successes {
            assert!(dest.is_file());
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn accepts_file_and_directory_paths() {
        let dir = std::env::temp_dir().join(format!(
            "pconv-paths-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let nested = dir.join("photos");
        let out = dir.join("out");
        fs::create_dir_all(&nested).unwrap();
        let source = nested.join("red.png");
        RgbImage::from_pixel(1, 1, Rgb([255, 0, 0]))
            .save_with_format(&source, image::ImageFormat::Png)
            .unwrap();

        let from_file = collect_sources(source.to_str().unwrap()).unwrap();
        assert_eq!(from_file, vec![source.clone()]);

        let from_dir = collect_sources(dir.to_str().unwrap()).unwrap();
        assert!(from_dir.contains(&source));

        let result = run(dir.to_str().unwrap(), out.to_str().unwrap(), "jpg", 1).unwrap();
        assert!(result.failures.is_empty(), "{:?}", result.failures);
        assert_eq!(result.successes, vec![out.join("photos").join("red.jpg")]);
        assert!(out.join("photos").join("red.jpg").is_file());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_non_image_files_when_scanning() {
        let dir = std::env::temp_dir().join(format!(
            "pconv-skip-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let nested = dir.join("src");
        let out = dir.join("out");
        fs::create_dir_all(&nested).unwrap();
        let source = dir.join("red.png");
        RgbImage::from_pixel(1, 1, Rgb([255, 0, 0]))
            .save_with_format(&source, image::ImageFormat::Png)
            .unwrap();
        fs::write(dir.join("README.md"), "# notes").unwrap();
        fs::write(nested.join("main.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("notes"), "no extension").unwrap();

        let sources = collect_sources(dir.to_str().unwrap()).unwrap();
        assert_eq!(sources, vec![source]);

        let result = run(dir.to_str().unwrap(), out.to_str().unwrap(), "jpg", 1).unwrap();
        assert!(result.failures.is_empty(), "{:?}", result.failures);
        assert_eq!(result.successes, vec![out.join("red.jpg")]);

        let explicit = dir.join("README.md");
        let error = collect_sources(explicit.to_str().unwrap()).unwrap_err();
        assert!(matches!(error, ConvertError::NotAnImage(_)));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn converts_png_to_farbfeld_and_svg() {
        let dir = std::env::temp_dir().join(format!(
            "pconv-formats-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("red.png");
        RgbImage::from_pixel(2, 2, Rgb([255, 0, 0]))
            .save_with_format(&source, image::ImageFormat::Png)
            .unwrap();

        let upload = format!("{}", source.display());
        let offload = format!("{}/*", dir.display());

        let ff = run(&upload, &offload, "ff", 1).unwrap();
        assert!(ff.failures.is_empty(), "{:?}", ff.failures);
        assert!(ff.successes[0].exists());

        let svg = run(&upload, &offload, "svg", 1).unwrap();
        assert!(svg.failures.is_empty(), "{:?}", svg.failures);
        let svg_text = fs::read_to_string(&svg.successes[0]).unwrap();
        assert!(svg_text.contains("<svg"));

        let jxl = run(&upload, &offload, "jxl", 1).unwrap();
        assert!(jxl.failures.is_empty(), "{:?}", jxl.failures);
        let decoded = crate::codecs::decode_image(&jxl.successes[0]).unwrap();
        assert_eq!(decoded.width(), 2);

        let svg_src = dir.join("box.svg");
        fs::write(
            &svg_src,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="#00ff00"/></svg>"##,
        )
        .unwrap();
        let from_svg = run(&format!("{}", svg_src.display()), &offload, "png", 1).unwrap();
        assert!(from_svg.failures.is_empty(), "{:?}", from_svg.failures);

        let icns = run(&upload, &offload, "icns", 1).unwrap();
        assert!(icns.failures.is_empty(), "{:?}", icns.failures);
        let decoded_icns = crate::codecs::decode_image(&icns.successes[0]).unwrap();
        assert!(decoded_icns.width() >= 16);

        let _ = fs::remove_dir_all(&dir);
    }
}
