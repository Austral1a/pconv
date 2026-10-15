use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::Path;

use base64::Engine;
use image::{DynamicImage, ImageFormat, RgbImage, RgbaImage};
use zune_core::bit_depth::BitDepth;
use zune_core::colorspace::ColorSpace;
use zune_core::options::EncoderOptions;
use zune_jpegxl::JxlSimpleEncoder;

use crate::format::TargetFormat;

pub fn decode_image(path: &Path) -> Result<DynamicImage, String> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match TargetFormat::from_ext(&ext) {
        Some(TargetFormat::Heic) => decode_heic(path),
        Some(TargetFormat::Jxl) => decode_jxl(path),
        Some(TargetFormat::Svg) => decode_svg(path),
        Some(TargetFormat::Icns) => decode_icns(path),
        Some(TargetFormat::Jpeg2000) => decode_jp2(path),
        Some(TargetFormat::Psd) => decode_psd(path),
        Some(TargetFormat::Raw) => decode_raw(path),
        _ => image::ImageReader::open(path)
            .map_err(|error| error.to_string())?
            .with_guessed_format()
            .map_err(|error| error.to_string())?
            .decode()
            .map_err(|error| error.to_string()),
    }
}

pub fn encode_image(
    image: &DynamicImage,
    dest: &Path,
    target: TargetFormat,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
    }

    match target {
        TargetFormat::Image(ImageFormat::Jpeg) => image
            .to_rgb8()
            .save_with_format(dest, ImageFormat::Jpeg)
            .map_err(|error| error.to_string()),
        TargetFormat::Image(ImageFormat::Avif) => image
            .to_rgb8()
            .save_with_format(dest, ImageFormat::Avif)
            .map_err(|error| error.to_string()),
        TargetFormat::Image(ImageFormat::Farbfeld) => image
            .to_rgba16()
            .save_with_format(dest, ImageFormat::Farbfeld)
            .map_err(|error| error.to_string()),
        TargetFormat::Image(ImageFormat::Hdr) => image
            .to_rgb32f()
            .save_with_format(dest, ImageFormat::Hdr)
            .map_err(|error| error.to_string()),
        TargetFormat::Image(ImageFormat::OpenExr) => image
            .to_rgb32f()
            .save_with_format(dest, ImageFormat::OpenExr)
            .map_err(|error| error.to_string()),
        TargetFormat::Image(format) => image
            .save_with_format(dest, format)
            .map_err(|error| error.to_string()),
        TargetFormat::Heic => encode_heic(image, dest),
        TargetFormat::Jxl => encode_jxl(image, dest),
        TargetFormat::Svg => encode_svg(image, dest),
        TargetFormat::Icns => encode_icns(image, dest),
        TargetFormat::Jpeg2000 | TargetFormat::Psd | TargetFormat::Raw => Err(format!(
            "{} can be decoded but not encoded; pick a writable format such as png",
            target.name()
        )),
    }
}

fn decode_jxl(path: &Path) -> Result<DynamicImage, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let decoder = jxl_oxide::integration::JxlDecoder::new(file).map_err(|error| error.to_string())?;
    DynamicImage::from_decoder(decoder).map_err(|error| error.to_string())
}

fn encode_jxl(image: &DynamicImage, dest: &Path) -> Result<(), String> {
    let rgba = image.to_rgba8();
    let options = EncoderOptions::new(
        rgba.width() as usize,
        rgba.height() as usize,
        ColorSpace::RGBA,
        BitDepth::Eight,
    );
    let encoder = JxlSimpleEncoder::new(rgba.as_raw(), options);
    let mut encoded = Vec::new();
    encoder
        .encode(&mut encoded)
        .map_err(|error| error.to_string())?;
    fs::write(dest, encoded).map_err(|error| error.to_string())
}

fn decode_svg(path: &Path) -> Result<DynamicImage, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let svg = if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svgz"))
        || bytes.starts_with(&[0x1f, 0x8b])
    {
        let mut decoder = flate2::read::GzDecoder::new(bytes.as_slice());
        let mut inflated = Vec::new();
        decoder
            .read_to_end(&mut inflated)
            .map_err(|error| error.to_string())?;
        inflated
    } else {
        bytes
    };

    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())
        .map_err(|error| error.to_string())?;
    let size = tree.size().to_int_size();
    let width = size.width().max(1);
    let height = size.height().max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| "failed to allocate SVG pixmap".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    RgbaImage::from_raw(width, height, pixmap.data().to_vec())
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| "SVG raster produced invalid dimensions".to_string())
}

fn encode_svg(image: &DynamicImage, dest: &Path) -> Result<(), String> {
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(png.into_inner());
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><image href="data:image/png;base64,{encoded}" width="{width}" height="{height}"/></svg>"#,
        width = image.width(),
        height = image.height(),
    );
    fs::write(dest, svg).map_err(|error| error.to_string())
}

fn decode_icns(path: &Path) -> Result<DynamicImage, String> {
    let family = icns::IconFamily::read(File::open(path).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let icon_type = family
        .available_icons()
        .into_iter()
        .max_by_key(|icon_type| {
            u32::from(icon_type.pixel_width()) * u32::from(icon_type.pixel_height())
        })
        .ok_or_else(|| "icns file contains no icons".to_string())?;
    let icon = family
        .get_icon_with_type(icon_type)
        .map_err(|error| error.to_string())?
        .convert_to(icns::PixelFormat::RGBA);
    RgbaImage::from_raw(icon.width(), icon.height(), icon.data().to_vec())
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| "icns decode produced invalid dimensions".to_string())
}

fn encode_icns(image: &DynamicImage, dest: &Path) -> Result<(), String> {
    const SIZES: [u32; 6] = [16, 32, 128, 256, 512, 1024];
    let longest = image.width().max(image.height()).max(1);
    let side = SIZES
        .iter()
        .copied()
        .find(|size| *size >= longest)
        .unwrap_or(1024);
    let resized = image.resize_exact(side, side, image::imageops::FilterType::Lanczos3);
    let rgba = resized.to_rgba8();
    let icon = icns::Image::from_data(icns::PixelFormat::RGBA, side, side, rgba.into_raw())
        .map_err(|error| error.to_string())?;
    let mut family = icns::IconFamily::new();
    family
        .add_icon(&icon)
        .map_err(|error| error.to_string())?;
    family
        .write(File::create(dest).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

fn decode_psd(path: &Path) -> Result<DynamicImage, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let psd = psd::Psd::from_bytes(&bytes).map_err(|error| format!("{error:?}"))?;
    RgbaImage::from_raw(psd.width(), psd.height(), psd.rgba())
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| "psd decode produced invalid dimensions".to_string())
}

fn decode_raw(path: &Path) -> Result<DynamicImage, String> {
    let decoded = imagepipe::simple_decode_8bit(path, 16384, 16384)?;
    RgbImage::from_raw(
        u32::try_from(decoded.width).map_err(|error| error.to_string())?,
        u32::try_from(decoded.height).map_err(|error| error.to_string())?,
        decoded.data,
    )
    .map(DynamicImage::ImageRgb8)
    .ok_or_else(|| "raw decode produced invalid dimensions".to_string())
}

fn decode_jp2(path: &Path) -> Result<DynamicImage, String> {
    let image = jpeg2k::Image::from_file(path).map_err(|error| error.to_string())?;
    DynamicImage::try_from(&image).map_err(|error| error.to_string())
}

#[cfg(feature = "heic")]
fn decode_heic(path: &Path) -> Result<DynamicImage, String> {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

    let path = path
        .to_str()
        .ok_or_else(|| "HEIC path must be valid UTF-8".to_string())?;
    let lib = LibHeif::new();
    let ctx = HeifContext::read_from_file(path).map_err(|error| error.to_string())?;
    let handle = ctx
        .primary_image_handle()
        .map_err(|error| error.to_string())?;
    let decoded = lib
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), None)
        .map_err(|error| error.to_string())?;
    let width = decoded.width();
    let height = decoded.height();
    let planes = decoded.planes();
    let plane = planes
        .interleaved
        .ok_or_else(|| "HEIC decode did not provide interleaved RGBA".to_string())?;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let start = (row as usize) * plane.stride;
        let end = start + (width as usize) * 4;
        pixels.extend_from_slice(&plane.data[start..end]);
    }
    RgbaImage::from_raw(width, height, pixels)
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| "HEIC decode produced invalid dimensions".to_string())
}

#[cfg(not(feature = "heic"))]
fn decode_heic(_: &Path) -> Result<DynamicImage, String> {
    Err("HEIC/HEIF support requires libheif (build pconv with the `heic` feature)".to_string())
}

#[cfg(feature = "heic")]
fn encode_heic(image: &DynamicImage, dest: &Path) -> Result<(), String> {
    use libheif_rs::{
        Channel, ColorSpace, CompressionFormat, EncoderQuality, HeifContext, LibHeif, RgbChroma,
    };

    let rgb = image.to_rgb8();
    let width = rgb.width();
    let height = rgb.height();
    let lib = LibHeif::new();
    let mut heif = libheif_rs::Image::new(width, height, ColorSpace::Rgb(RgbChroma::C444))
        .map_err(|error| error.to_string())?;
    heif.create_plane(Channel::R, width, height, 8)
        .map_err(|error| error.to_string())?;
    heif.create_plane(Channel::G, width, height, 8)
        .map_err(|error| error.to_string())?;
    heif.create_plane(Channel::B, width, height, 8)
        .map_err(|error| error.to_string())?;
    {
        let mut planes = heif.planes_mut();
        let r = planes
            .r
            .as_mut()
            .ok_or_else(|| "HEIC encoder did not provide an R plane".to_string())?;
        let g = planes
            .g
            .as_mut()
            .ok_or_else(|| "HEIC encoder did not provide a G plane".to_string())?;
        let b = planes
            .b
            .as_mut()
            .ok_or_else(|| "HEIC encoder did not provide a B plane".to_string())?;
        let r_stride = r.stride;
        let g_stride = g.stride;
        let b_stride = b.stride;
        let src = rgb.as_raw();
        for y in 0..height as usize {
            for x in 0..width as usize {
                let pixel = (y * width as usize + x) * 3;
                r.data[y * r_stride + x] = src[pixel];
                g.data[y * g_stride + x] = src[pixel + 1];
                b.data[y * b_stride + x] = src[pixel + 2];
            }
        }
    }

    let mut context = HeifContext::new().map_err(|error| error.to_string())?;
    let mut encoder = lib
        .encoder_for_format(CompressionFormat::Hevc)
        .or_else(|_| lib.encoder_for_format(CompressionFormat::Av1))
        .map_err(|error| error.to_string())?;
    encoder
        .set_quality(EncoderQuality::Lossy(80))
        .map_err(|error| error.to_string())?;
    context
        .encode_image(&heif, &mut encoder, None)
        .map_err(|error| error.to_string())?;
    let dest = dest
        .to_str()
        .ok_or_else(|| "HEIC path must be valid UTF-8".to_string())?;
    context
        .write_to_file(dest)
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "heic"))]
fn encode_heic(_: &DynamicImage, _: &Path) -> Result<(), String> {
    Err("HEIC/HEIF support requires libheif (build pconv with the `heic` feature)".to_string())
}
