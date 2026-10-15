use image::ImageFormat;

pub const OUTPUT_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "tga", "ico", "pnm", "pbm", "pgm",
    "ppm", "qoi", "avif", "exr", "hdr", "ff", "jxl", "svg", "icns", "heic", "heif",
];

pub const INPUT_ONLY_EXTENSIONS: &[&str] = &[
    "dds", "psd", "jp2", "j2k", "j2c", "jpx", "dng", "cr2", "nef", "arw", "raf", "orf", "rw2", "pef",
    "srw", "raw",
];

/// True when `ext` is a format this tool can decode, with or without a leading dot.
pub fn is_readable_extension(ext: &str) -> bool {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    !ext.is_empty() && TargetFormat::from_ext(&ext).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFormat {
    Image(ImageFormat),
    Heic,
    Jxl,
    Svg,
    Icns,
    Jpeg2000,
    Psd,
    Raw,
}

impl TargetFormat {
    pub fn from_ext(ext: &str) -> Option<Self> {
        match ext {
            "heic" | "heif" | "hif" => Some(Self::Heic),
            "jxl" => Some(Self::Jxl),
            "svg" | "svgz" => Some(Self::Svg),
            "icns" => Some(Self::Icns),
            "jp2" | "j2k" | "j2c" | "jpx" => Some(Self::Jpeg2000),
            "psd" => Some(Self::Psd),
            "dng" | "cr2" | "cr3" | "nef" | "arw" | "raf" | "orf" | "rw2" | "pef" | "srw" | "raw"
            | "erf" | "kdc" | "mrw" | "nrw" | "sr2" => Some(Self::Raw),
            "ff" | "farbfeld" => Some(Self::Image(ImageFormat::Farbfeld)),
            "exr" => Some(Self::Image(ImageFormat::OpenExr)),
            "hdr" => Some(Self::Image(ImageFormat::Hdr)),
            "dds" => Some(Self::Image(ImageFormat::Dds)),
            "avif" => Some(Self::Image(ImageFormat::Avif)),
            other => ImageFormat::from_extension(other).map(Self::Image),
        }
    }

    pub fn can_encode(self) -> bool {
        match self {
            Self::Image(format) => format.can_write(),
            Self::Heic | Self::Jxl | Self::Svg | Self::Icns => true,
            Self::Jpeg2000 | Self::Psd | Self::Raw => false,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Image(ImageFormat::Png) => "png",
            Self::Image(ImageFormat::Jpeg) => "jpeg",
            Self::Image(ImageFormat::Gif) => "gif",
            Self::Image(ImageFormat::WebP) => "webp",
            Self::Image(ImageFormat::Bmp) => "bmp",
            Self::Image(ImageFormat::Tiff) => "tiff",
            Self::Image(ImageFormat::Tga) => "tga",
            Self::Image(ImageFormat::Ico) => "ico",
            Self::Image(ImageFormat::Pnm) => "pnm",
            Self::Image(ImageFormat::Qoi) => "qoi",
            Self::Image(ImageFormat::Avif) => "avif",
            Self::Image(ImageFormat::OpenExr) => "exr",
            Self::Image(ImageFormat::Hdr) => "hdr",
            Self::Image(ImageFormat::Farbfeld) => "ff",
            Self::Image(ImageFormat::Dds) => "dds",
            Self::Image(_) => "image",
            Self::Heic => "heic",
            Self::Jxl => "jxl",
            Self::Svg => "svg",
            Self::Icns => "icns",
            Self::Jpeg2000 => "jp2",
            Self::Psd => "psd",
            Self::Raw => "raw",
        }
    }
}
