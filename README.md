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
pconv --upload-from <PATH> --offload-to <PATH> --output-ext <EXT>
```

All three flags are required.

| Flag | Short | Meaning |
| --- | --- | --- |
| `--upload-from` | `-uf` | Source file, directory, or glob |
| `--offload-to` | `-ot` | Output directory, or a path pattern with one `*` |
| `--output-ext` | `-oe` | Target extension, with or without a leading dot |

Short flags work with or without `=`:

```sh
pconv -uf photos -ot out -oe webp
pconv -uf='photos/**/*.png' -ot='out/*' -oe=jpg
```

Successful output paths are printed to stdout, one per line. Per-file errors go to stderr. The process exits with status `1` if nothing matched, the arguments are invalid, or any file failed to convert.

## Paths

`--upload-from` accepts a relative or absolute path:

- A file is converted as-is.
- A directory is scanned recursively.
- A glob may use `*`, `?`, and `**`.

`--offload-to` is a relative or absolute output directory, or a pattern with at most one `*`. That `*` is replaced by each source filename stem (the name without its extension). `--output-ext` sets the written extension and replaces any extension written in the offload pattern.

```sh
pconv --upload-from './photos' --offload-to './out' --output-ext webp
pconv --upload-from '/tmp/in/*.png' --offload-to '/tmp/out' --output-ext jpg
pconv -uf='photos/**/*.png' -ot='converted_*' -oe=jpg
```

The first command writes `./out/<stem>.webp` for every image under `./photos`. The third writes `converted_<stem>.jpg` in the current directory.

Output directories are created when they do not exist. If two sources would write the same path (for example `a/cat.png` and `b/cat.jpg` both mapping to `out/cat.webp`), nothing is written and the command reports the collision.

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
