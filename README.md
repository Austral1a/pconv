# pconv

`pconv` is a command-line image converter. Point it at a file, a directory, or a glob, choose an output location, and it writes every match in the format you ask for.

## Requirements

- [Rust](https://www.rust-lang.org/tools/install) (edition 2021)
- For HEIC and HEIF: `libheif` and `pkg-config` on the machine that builds the binary. HEIC support is on by default.

## Install

From this repository:

```sh
cargo install --path .
```

Build a release binary without installing it:

```sh
cargo build --release
```

The binary is `target/release/pconv`.

To build without HEIC and HEIF (no `libheif` required):

```sh
cargo build --release --no-default-features
```

## Usage

```sh
pconv --upload-from <PATH> --offload-to <PATH> --output-ext <EXT> [options]
```

`--upload-from`, `--offload-to`, and `--output-ext` are required.

| Flag | Short | Meaning |
| --- | --- | --- |
| `--upload-from` | `-uf` | Source file, directory, or glob |
| `--offload-to` | `-ot` | Output directory, or a path pattern with one `*` |
| `--output-ext` | `-oe` | Target extension, with or without a leading dot |
| `--workers` | `-w` | Images to convert at the same time (default: CPU cores) |
| `--speed` | `-s` | AVIF speed, `1` slowest to `10` fastest (default `10`) |
| `--quality` | `-q` | Lossy quality for JPEG, AVIF, HEIC/HEIF, `1`–`100` (default `80`) |
| `--skip-existing` |  | Leave existing output files untouched |

Short flags work with or without `=`:

```sh
pconv -uf photos -ot out -oe avif -s 10 -q 70
pconv -uf='photos/**/*.png' -ot='out/*' -oe=jpg -w=4 --skip-existing
```

Each worker converts one image at a time. The worker count is limited to the number of CPU cores: AVIF encoding already uses the cores, so extra workers make the run slower. AVIF is much slower than JPEG or WebP; `--speed 10` is the fastest setting, and `--skip-existing` lets you resume a long batch. `--speed` is AVIF-only. `--quality` applies to JPEG, AVIF, and HEIC/HEIF; WebP here is lossless, so quality does not change it.

While images are converting, a progress line on the terminal shows the current file and how many have started. Successful output paths are printed to stdout, one per line. Per-file errors go to stderr. The process exits with status `1` if nothing matched, the arguments are invalid, or any file failed to convert.

## Paths

`--upload-from` accepts a relative or absolute path:

- A file is converted as-is. A file whose extension is not a supported image format is an error.
- A directory is scanned recursively. Non-image files are skipped.
- A glob may use `*`, `?`, and `**`. Matches that are not supported image formats are skipped.

`--offload-to` is a relative or absolute output directory, or a pattern with at most one `*`. A directory keeps each file's path relative to the upload root, so `public/images/products/gallery/01.jpg` becomes `public/out/products/gallery/01.avif`. A `*` is replaced by the filename stem only, which flattens every match into one directory. `--output-ext` sets the written extension and replaces any extension written in the offload pattern.

```sh
pconv --upload-from './photos' --offload-to './out' --output-ext webp
pconv --upload-from '/tmp/in/*.png' --offload-to '/tmp/out' --output-ext jpg
pconv -uf='photos/**/*.png' -ot='converted_*' -oe=jpg
```

The first command writes `./out/a/cat.webp` for `./photos/a/cat.png`. The third writes `converted_<stem>.jpg` in the current directory.

Output directories are created when they do not exist. A `*` pattern that would write two sources to the same path (for example `a/cat.png` and `b/cat.jpg` both mapping to `out/cat.webp`) writes nothing and reports the collision.

## Formats

Writable extensions:

`png`, `jpg`, `jpeg`, `gif`, `webp`, `bmp`, `tif`, `tiff`, `tga`, `ico`, `pnm`, `pbm`, `pgm`, `ppm`, `qoi`, `avif`, `exr`, `hdr`, `ff`, `jxl`, `svg`, `icns`, `heic`, `heif`

These extensions can be read and cannot be written:

`dds`, `psd`, `jp2`, `j2k`, `j2c`, `jpx`, `dng`, `cr2`, `nef`, `arw`, `raf`, `orf`, `rw2`, `pef`, `srw`, `raw`

Input is chosen from the file extension. Common camera raw files and JPEG 2000, PSD, DDS, SVG, ICNS, JPEG XL, and HEIC/HEIF are decoded by dedicated readers. SVG output embeds the image as a PNG. ICNS output picks a standard icon size large enough for the source.

HEIC and HEIF need the default `heic` feature and a build-time `libheif`. A binary built with `--no-default-features` rejects those formats.

## Help

```sh
pconv --help
```
