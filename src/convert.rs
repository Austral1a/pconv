use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::format::{INPUT_ONLY_EXTENSIONS, OUTPUT_EXTENSIONS, TargetFormat};

#[derive(Debug)]
pub enum ConvertError {
    NoMatches(String),
    InvalidPattern(String),
    Collisions(Vec<(PathBuf, Vec<PathBuf>)>),
    GlobPattern(glob::PatternError),
    Glob(glob::GlobError),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMatches(pattern) => {
                write!(f, "no files matched upload glob `{pattern}`")
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
}

impl OffloadPattern {
    pub fn parse(pattern: &str, output_ext: &str) -> Result<Self, ConvertError> {
        let star_count = pattern.chars().filter(|ch| *ch == '*').count();
        if star_count > 1 {
            return Err(ConvertError::InvalidPattern(
                "offload pattern must contain at most one `*`".to_string(),
            ));
        }

        let (prefix, suffix) = if let Some(star) = pattern.find('*') {
            (
                pattern[..star].to_string(),
                pattern[star + 1..].to_string(),
            )
        } else {
            (ensure_trailing_slash(pattern), String::new())
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
        })
    }

    pub fn map(&self, source: &Path) -> PathBuf {
        let stem = source
            .file_stem()
            .map(|stem| stem.to_string_lossy())
            .unwrap_or_default();
        let mut dest = PathBuf::from(format!("{}{stem}{}", self.prefix, self.suffix));
        dest.set_extension(&self.extension);
        dest
    }

    pub fn extension(&self) -> &str {
        &self.extension
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
) -> Result<RunResult, ConvertError> {
    let pattern = OffloadPattern::parse(offload_to, output_ext)?;
    let sources = collect_sources(upload_from)?;
    let planned = plan_outputs(&sources, &pattern)?;

    let mut result = RunResult::default();
    for (source, dest) in planned {
        match convert_one(&source, &dest, pattern.target()) {
            Ok(()) => result.successes.push(dest),
            Err(error) => result.failures.push((source, error)),
        }
    }
    Ok(result)
}

pub fn collect_sources(pattern: &str) -> Result<Vec<PathBuf>, ConvertError> {
    let path = Path::new(pattern);
    if path.is_file() {
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
        if path.is_file() {
            files.push(path);
        }
    }
    if files.is_empty() {
        return Err(ConvertError::NoMatches(pattern.to_string()));
    }
    files.sort();
    Ok(files)
}

pub fn plan_outputs(
    sources: &[PathBuf],
    pattern: &OffloadPattern,
) -> Result<Vec<(PathBuf, PathBuf)>, ConvertError> {
    let pairs: Vec<(PathBuf, PathBuf)> = sources
        .iter()
        .map(|source| (source.clone(), pattern.map(source)))
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
    fn maps_out_star_webp() {
        let pattern = OffloadPattern::parse("out/*", "webp").unwrap();
        assert_eq!(
            pattern.map(Path::new("photos/a/cat.png")),
            PathBuf::from("out/cat.webp")
        );
    }

    #[test]
    fn maps_prefixed_star() {
        let pattern = OffloadPattern::parse("converted_*", "jpg").unwrap();
        assert_eq!(
            pattern.map(Path::new("cat.png")),
            PathBuf::from("converted_cat.jpg")
        );
    }

    #[test]
    fn output_ext_replaces_pattern_extension() {
        let pattern = OffloadPattern::parse("out/*.png", "webp").unwrap();
        assert_eq!(
            pattern.map(Path::new("photos/a/cat.png")),
            PathBuf::from("out/cat.webp")
        );
    }

    #[test]
    fn directory_offload_is_relative_or_absolute() {
        let relative = OffloadPattern::parse("out", "webp").unwrap();
        assert_eq!(
            relative.map(Path::new("photos/a/cat.png")),
            PathBuf::from("out/cat.webp")
        );

        let dotted = OffloadPattern::parse("./converted", "jpg").unwrap();
        assert_eq!(
            dotted.map(Path::new("cat.png")),
            PathBuf::from("./converted/cat.jpg")
        );

        let absolute = OffloadPattern::parse("/tmp/out", "png").unwrap();
        assert_eq!(
            absolute.map(Path::new("photos/a/cat.png")),
            PathBuf::from("/tmp/out/cat.png")
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
        assert_eq!(pattern.extension(), "jpeg");
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
        let error = plan_outputs(&sources, &pattern).unwrap_err();
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
        let result = run(&upload, &offload, "jpg").unwrap();
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

        let result = run(
            dir.to_str().unwrap(),
            out.to_str().unwrap(),
            "jpg",
        )
        .unwrap();
        assert!(result.failures.is_empty(), "{:?}", result.failures);
        assert_eq!(result.successes, vec![out.join("red.jpg")]);
        assert!(out.join("red.jpg").is_file());

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

        let ff = run(&upload, &offload, "ff").unwrap();
        assert!(ff.failures.is_empty(), "{:?}", ff.failures);
        assert!(ff.successes[0].exists());

        let svg = run(&upload, &offload, "svg").unwrap();
        assert!(svg.failures.is_empty(), "{:?}", svg.failures);
        let svg_text = fs::read_to_string(&svg.successes[0]).unwrap();
        assert!(svg_text.contains("<svg"));

        let jxl = run(&upload, &offload, "jxl").unwrap();
        assert!(jxl.failures.is_empty(), "{:?}", jxl.failures);
        let decoded = crate::codecs::decode_image(&jxl.successes[0]).unwrap();
        assert_eq!(decoded.width(), 2);

        let svg_src = dir.join("box.svg");
        fs::write(
            &svg_src,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="#00ff00"/></svg>"##,
        )
        .unwrap();
        let from_svg = run(&format!("{}", svg_src.display()), &offload, "png").unwrap();
        assert!(from_svg.failures.is_empty(), "{:?}", from_svg.failures);

        let icns = run(&upload, &offload, "icns").unwrap();
        assert!(icns.failures.is_empty(), "{:?}", icns.failures);
        let decoded_icns = crate::codecs::decode_image(&icns.successes[0]).unwrap();
        assert!(decoded_icns.width() >= 16);

        let _ = fs::remove_dir_all(&dir);
    }
}
