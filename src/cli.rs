use clap::Parser;

const AFTER_HELP: &str = "\
Pattern rules:
  --upload-from is a relative or absolute path: a file, a directory (scanned recursively), or a glob (`*`, `?`, `**`).
  --offload-to is a relative or absolute output directory, or a pattern with one `*` for the source filename stem.
  --output-ext is the target format (with or without a leading dot).

Examples:
  pconv --upload-from './photos' --offload-to './out' --output-ext webp
  pconv --upload-from '/tmp/in/*.png' --offload-to '/tmp/out' --output-ext jpg
  pconv -uf='photos/**/*.png' -ot='out/*' -oe=jpg

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

    /// Output directory or path pattern. Relative and absolute paths are accepted. A `*` is replaced by each source filename stem.
    #[arg(long = "offload-to", value_name = "PATH")]
    pub offload_to: String,

    /// Target file extension for converted images (for example `webp` or `.jpg`).
    #[arg(long = "output-ext", value_name = "EXT")]
    pub output_ext: String,
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
    use super::normalize_argv;

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
}
