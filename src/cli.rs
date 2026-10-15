use clap::Parser;

const AFTER_HELP: &str = "\
Pattern rules:
  --upload-from is a relative or absolute path: a file, a directory (scanned recursively), or a glob (`*`, `?`, `**`).
  Files whose extension is not a supported image format are skipped. A single non-image file is an error.
  --offload-to is a relative or absolute output directory, or a pattern with one `*`.
  A directory keeps each file's path relative to the upload root. A `*` is replaced by the filename stem only.
  --output-ext is the target format (with or without a leading dot).
  --workers is how many images to convert at the same time. It defaults to the number of CPU cores, and higher values are limited to that count.
  --speed is the AVIF encoder speed, from 1 (smallest files) to 10 (fastest). Default is 10. Other formats ignore it.
  --quality is lossy quality from 1 to 100 (default 80). Applies to jpg/jpeg, avif, and heic/heif.
  --skip-existing leaves output files that already exist untouched.

Examples:
  pconv --upload-from './photos' --offload-to './out' --output-ext webp
  pconv --upload-from '/tmp/in/*.png' --offload-to '/tmp/out' --output-ext avif --speed 10
  pconv -uf='photos/**/*.png' -ot='out/*' -oe=jpg -w=4 --skip-existing

Writable extensions:
  png, jpg, jpeg, gif, webp, bmp, tif, tiff, tga, ico, pnm, pbm, pgm, ppm, qoi,
  avif, exr, hdr, ff, jxl, svg, icns, heic, heif

Read-only extensions:
  dds, psd, jp2, j2k, j2c, jpx, dng, cr2, nef, arw, raf, orf, rw2, pef, srw, raw

HEIC/HEIF requires libheif (and pkg-config) at build time.";

/// Convert images matched by a glob into another format.
#[derive(Parser, Debug)]
#[command(
    name = "pconv",
    version,
    about = "Convert images from one format to another.",
    after_help = AFTER_HELP
)]
pub struct Cli {
    /// Source file, directory, or glob. Relative and absolute paths are accepted.
    #[arg(long = "upload-from", value_name = "PATH")]
    pub upload_from: String,

    /// Output directory or path pattern. A directory keeps the source layout. A `*` is replaced by each source filename stem.
    #[arg(long = "offload-to", value_name = "PATH")]
    pub offload_to: String,

    /// Target file extension for converted images (for example `webp` or `.jpg`).
    #[arg(long = "output-ext", value_name = "EXT")]
    pub output_ext: String,

    /// How many images to convert at the same time. Defaults to the number of CPU cores.
    #[arg(long = "workers", short = 'w', value_name = "N", value_parser = parse_workers)]
    pub workers: Option<usize>,

    /// AVIF encoder speed from 1 (smallest files, slowest) to 10 (fastest).
    #[arg(
        long = "speed",
        short = 's',
        value_name = "1-10",
        default_value_t = 10,
        value_parser = parse_speed
    )]
    pub speed: u8,

    /// Lossy quality from 1 (worst) to 100 (best). Applies to JPEG, AVIF, and HEIC/HEIF.
    #[arg(
        long = "quality",
        short = 'q',
        value_name = "1-100",
        default_value_t = 80,
        value_parser = parse_quality
    )]
    pub quality: u8,

    /// Skip conversions whose output file already exists.
    #[arg(long = "skip-existing")]
    pub skip_existing: bool,
}

fn parse_speed(raw: &str) -> Result<u8, String> {
    let speed: u8 = raw
        .parse()
        .map_err(|_| format!("`{raw}` is not a whole number"))?;
    if !(1..=10).contains(&speed) {
        return Err("must be from 1 to 10".to_string());
    }
    Ok(speed)
}

fn parse_quality(raw: &str) -> Result<u8, String> {
    let quality: u8 = raw
        .parse()
        .map_err(|_| format!("`{raw}` is not a whole number"))?;
    if !(1..=100).contains(&quality) {
        return Err("must be from 1 to 100".to_string());
    }
    Ok(quality)
}

fn parse_workers(raw: &str) -> Result<usize, String> {
    let workers: usize = raw
        .parse()
        .map_err(|_| format!("`{raw}` is not a whole number"))?;
    if workers == 0 {
        return Err("must be at least 1".to_string());
    }
    Ok(workers)
}

/// Rewrite `-uf` / `-ot` / `-oe` into clap long flags.
pub fn normalize_argv<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = Vec::new();
    let mut passthrough = false;
    for arg in args {
        let arg = arg.as_ref();
        if passthrough {
            out.push(arg.to_string());
            continue;
        }
        if arg == "--" {
            passthrough = true;
            out.push(arg.to_string());
            continue;
        }
        if let Some(value) = arg.strip_prefix("-uf=") {
            out.push(format!("--upload-from={value}"));
        } else if arg == "-uf" {
            out.push("--upload-from".to_string());
        } else if let Some(value) = arg.strip_prefix("-ot=") {
            out.push(format!("--offload-to={value}"));
        } else if arg == "-ot" {
            out.push("--offload-to".to_string());
        } else if let Some(value) = arg.strip_prefix("-oe=") {
            out.push(format!("--output-ext={value}"));
        } else if arg == "-oe" {
            out.push("--output-ext".to_string());
        } else {
            out.push(arg.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{normalize_argv, Cli};
    use clap::Parser;

    #[test]
    fn rewrites_short_flags_with_equals() {
        let args = normalize_argv(["pconv", "-uf=photos/**/*.png", "-ot=out/*", "-oe=webp"]);
        assert_eq!(
            args,
            [
                "pconv",
                "--upload-from=photos/**/*.png",
                "--offload-to=out/*",
                "--output-ext=webp"
            ]
        );
    }

    #[test]
    fn rewrites_short_flags_without_equals() {
        let args = normalize_argv(["pconv", "-uf", "in/*.png", "-ot", "out/*", "-oe", "jpg"]);
        assert_eq!(
            args,
            [
                "pconv",
                "--upload-from",
                "in/*.png",
                "--offload-to",
                "out/*",
                "--output-ext",
                "jpg"
            ]
        );
    }

    #[test]
    fn leaves_long_flags_unchanged() {
        let args = normalize_argv([
            "pconv",
            "--upload-from",
            "a",
            "--offload-to=b/*",
            "--output-ext=png",
        ]);
        assert_eq!(
            args,
            [
                "pconv",
                "--upload-from",
                "a",
                "--offload-to=b/*",
                "--output-ext=png"
            ]
        );
    }

    #[test]
    fn stops_rewriting_after_double_dash() {
        let args = normalize_argv(["pconv", "--", "-uf=still-a-path"]);
        assert_eq!(args, ["pconv", "--", "-uf=still-a-path"]);
    }

    #[test]
    fn parses_worker_count() {
        let cli = Cli::try_parse_from([
            "pconv",
            "--upload-from",
            "in",
            "--offload-to",
            "out",
            "--output-ext",
            "webp",
            "-w",
            "4",
        ])
        .unwrap();
        assert_eq!(cli.workers, Some(4));
        assert_eq!(cli.speed, 10);
        assert_eq!(cli.quality, 80);
        assert!(!cli.skip_existing);

        let default = Cli::try_parse_from([
            "pconv",
            "--upload-from",
            "in",
            "--offload-to",
            "out",
            "--output-ext",
            "webp",
            "--quality",
            "60",
            "--skip-existing",
        ])
        .unwrap();
        assert_eq!(default.workers, None);
        assert_eq!(default.speed, 10);
        assert_eq!(default.quality, 60);
        assert!(default.skip_existing);

        let rejected = Cli::try_parse_from([
            "pconv",
            "--upload-from",
            "in",
            "--offload-to",
            "out",
            "--output-ext",
            "webp",
            "--workers",
            "0",
        ]);
        assert!(rejected.is_err());
    }
}
