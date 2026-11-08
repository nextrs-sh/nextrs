//! The image transformation behind NextRS's `/_next/image`: what Next.js's
//! `optimizeImage()` asks sharp to do (`rotate()`, `resize(width, { withoutEnlargement })`,
//! then one encoder), with Rust decoders and explicit resource limits.
//!
//! Policy (which types are optimized, bypassed or refused, which format is
//! negotiated) lives in the server; this crate only transforms pixels.

mod sniff;
mod webp;

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use fast_image_resize as fr;
use image::{DynamicImage, ImageDecoder, ImageReader};

pub use crate::sniff::{SniffError, detect_content_type, is_animated};

/// The types Next.js's optimizer distinguishes (`image-type.js`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageType {
    Avif,
    Webp,
    Png,
    Jpeg,
    Jxl,
    Jp2,
    Heic,
    Gif,
    Svg,
    Ico,
    Icns,
    Tiff,
    Bmp,
    Pdf,
}

impl ImageType {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Avif => "image/avif",
            Self::Webp => "image/webp",
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Jxl => "image/jxl",
            Self::Jp2 => "image/jp2",
            Self::Heic => "image/heic",
            Self::Gif => "image/gif",
            Self::Svg => "image/svg+xml",
            Self::Ico => "image/x-icon",
            Self::Icns => "image/x-icns",
            Self::Tiff => "image/tiff",
            Self::Bmp => "image/bmp",
            Self::Pdf => "application/pdf",
        }
    }

    pub fn from_mime(mime: &str) -> Option<Self> {
        [
            Self::Avif,
            Self::Webp,
            Self::Png,
            Self::Jpeg,
            Self::Jxl,
            Self::Jp2,
            Self::Heic,
            Self::Gif,
            Self::Svg,
            Self::Ico,
            Self::Icns,
            Self::Tiff,
            Self::Bmp,
            Self::Pdf,
        ]
        .into_iter()
        .find(|t| t.mime() == mime)
    }

    /// The extension next start's `getExtension` (send's mime 1.6 plus
    /// Next's additions) gives the type.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Avif => "avif",
            Self::Webp => "webp",
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Jxl => "jxl",
            Self::Jp2 => "jp2",
            Self::Heic => "heic",
            Self::Gif => "gif",
            Self::Svg => "svg",
            Self::Ico => "ico",
            Self::Icns => "icns",
            Self::Tiff => "tiff",
            Self::Bmp => "bmp",
            Self::Pdf => "pdf",
        }
    }

    /// Animated images of these types are served unchanged.
    pub fn animatable(self) -> bool {
        matches!(self, Self::Webp | Self::Png | Self::Gif)
    }

    /// Served unchanged (`BYPASS_TYPES`).
    pub fn bypassed(self) -> bool {
        matches!(
            self,
            Self::Svg | Self::Ico | Self::Icns | Self::Bmp | Self::Jxl | Self::Heic
        )
    }
}

/// Bounds on one transformation, checked before memory is committed.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Source width × height (after reading the header, before decoding).
    pub max_input_pixels: u64,
    /// Bytes the decoder may allocate.
    pub max_alloc_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_pixels: 64_000_000,
            max_alloc_bytes: 512 * 1024 * 1024,
        }
    }
}

pub struct Job<'a> {
    pub input: &'a [u8],
    /// The encoder to use. Types without an encoder here (the bypassed
    /// ones) are refused with [`Error::Unsupported`].
    pub output: ImageType,
    pub width: u32,
    /// 1-100, as Next passes it to sharp.
    pub quality: u8,
    pub limits: Limits,
    /// Checked between stages; an encoder already running finishes.
    pub deadline: Option<Instant>,
    /// Set when nobody waits for the result any more.
    pub cancelled: Option<&'a AtomicBool>,
}

#[derive(Debug, Clone, Default)]
pub struct Timings {
    pub decode: Duration,
    pub resize: Duration,
    pub encode: Duration,
}

#[derive(Debug)]
pub struct Optimized {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timings: Timings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The source could not be decoded (corrupt, truncated, unsupported codec).
    Decode(String),
    /// Width × height above [`Limits::max_input_pixels`], or the decoder
    /// would allocate more than [`Limits::max_alloc_bytes`].
    TooLarge {
        width: u32,
        height: u32,
    },
    Unsupported(&'static str),
    TimedOut,
    Cancelled,
    Encode(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode(e) => write!(f, "decode failed: {e}"),
            Self::TooLarge { width, height } => {
                write!(f, "source of {width}x{height} exceeds the input limits")
            }
            Self::Unsupported(what) => write!(f, "unsupported: {what}"),
            Self::TimedOut => f.write_str("transformation timed out"),
            Self::Cancelled => f.write_str("transformation cancelled"),
            Self::Encode(e) => write!(f, "encode failed: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// Output size for `resize(width, undefined, { withoutEnlargement: true })`
/// as sharp 0.35 computes it (`pipeline.cc`): never larger than the source;
/// the height follows from the width, after the shrink-on-load sharp uses
/// for JPEG (an integer factor, libvips rounding the shrunk size down) and
/// WebP (a scale, rounded half to even), skipped when the image must be
/// rotated first. Checked against sharp on 285 size combinations.
pub fn target_size(src_width: u32, src_height: u32, width: u32, source: SourceKind) -> (u32, u32) {
    if width >= src_width || src_width == 0 || width == 0 {
        return (src_width, src_height);
    }
    let (mut w, mut h) = (f64::from(src_width), f64::from(src_height));
    let target = f64::from(width);
    match source {
        SourceKind::Jpeg => {
            let shrink = w / target;
            let mut s = if shrink >= 8.0 {
                8.0
            } else if shrink >= 4.0 {
                4.0
            } else if shrink >= 2.0 {
                2.0
            } else {
                1.0
            };
            // "Lower shrink-on-load for known libjpeg rounding errors".
            if s > 1.0 && shrink.trunc() == s {
                s /= 2.0;
            }
            if s > 1.0 {
                w = (w / s).floor();
                h = (h / s).floor();
            }
        }
        SourceKind::Webp => {
            let scale = 1.0 / (w / target);
            w = (w * scale).round_ties_even();
            h = (h * scale).round_ties_even();
        }
        SourceKind::Other => {}
    }
    let shrink = (w / target).max(1.0);
    let height = (h * (1.0 / shrink)).round().max(1.0);
    (width, height as u32)
}

/// Which shrink-on-load sharp would apply to the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Jpeg,
    Webp,
    Other,
}

/// Header dimensions without decoding pixels (after EXIF orientation).
pub fn dimensions(input: &[u8]) -> Result<(u32, u32), Error> {
    let mut decoder = ImageReader::new(Cursor::new(input))
        .with_guessed_format()
        .map_err(|e| Error::Decode(e.to_string()))?
        .into_decoder()
        .map_err(|e| Error::Decode(e.to_string()))?;
    let (w, h) = decoder.dimensions();
    let swap = matches!(
        decoder.orientation(),
        Ok(image::metadata::Orientation::Rotate90
            | image::metadata::Orientation::Rotate270
            | image::metadata::Orientation::Rotate90FlipH
            | image::metadata::Orientation::Rotate270FlipH)
    );
    Ok(if swap { (h, w) } else { (w, h) })
}

pub fn optimize(job: &Job<'_>) -> Result<Optimized, Error> {
    let checkpoint = || {
        if job.cancelled.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(Error::Cancelled);
        }
        if job.deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(Error::TimedOut);
        }
        Ok(())
    };
    if !matches!(
        job.output,
        ImageType::Avif
            | ImageType::Webp
            | ImageType::Png
            | ImageType::Jpeg
            | ImageType::Gif
            | ImageType::Tiff
    ) {
        return Err(Error::Unsupported("no encoder for this output type"));
    }
    let mut timings = Timings::default();
    let started = Instant::now();

    // Decode: the header first, so limits apply before any pixel buffer.
    let mut reader = ImageReader::new(Cursor::new(job.input))
        .with_guessed_format()
        .map_err(|e| Error::Decode(e.to_string()))?;
    let format = reader.format();
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(job.limits.max_alloc_bytes);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| Error::Decode(e.to_string()))?;
    let (w, h) = decoder.dimensions();
    if u64::from(w) * u64::from(h) > job.limits.max_input_pixels {
        return Err(Error::TooLarge {
            width: w,
            height: h,
        });
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    // sharp shrinks on load only when no rotation must happen first.
    let kind = match (format, orientation) {
        (_, o) if o != image::metadata::Orientation::NoTransforms => SourceKind::Other,
        (Some(image::ImageFormat::Jpeg), _) => SourceKind::Jpeg,
        (Some(image::ImageFormat::WebP), _) => SourceKind::Webp,
        _ => SourceKind::Other,
    };
    let icc = decoder.icc_profile().ok().flatten();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| match e {
        image::ImageError::Limits(_) => Error::TooLarge {
            width: w,
            height: h,
        },
        e => Error::Decode(e.to_string()),
    })?;
    img.apply_orientation(orientation);
    let (src_w, src_h) = (img.width(), img.height());
    let alpha = img.color().has_alpha();
    let (pixels, pixel_type) = if alpha {
        (img.into_rgba8().into_raw(), fr::PixelType::U8x4)
    } else {
        (img.into_rgb8().into_raw(), fr::PixelType::U8x3)
    };
    timings.decode = started.elapsed();
    checkpoint()?;

    // Resize (Lanczos3, alpha premultiplied while filtering).
    let resized_at = Instant::now();
    let (out_w, out_h) = target_size(src_w, src_h, job.width, kind);
    let mut pixels = if (out_w, out_h) == (src_w, src_h) {
        pixels
    } else {
        let src = fr::images::Image::from_vec_u8(src_w, src_h, pixels, pixel_type)
            .map_err(|e| Error::Decode(e.to_string()))?;
        let mut dst = fr::images::Image::new(out_w, out_h, pixel_type);
        let options = fr::ResizeOptions::new()
            .resize_alg(fr::ResizeAlg::Convolution(fr::FilterType::Lanczos3))
            .use_alpha(alpha);
        fr::Resizer::new()
            .resize(&src, &mut dst, &options)
            .map_err(|e| Error::Decode(e.to_string()))?;
        dst.into_vec()
    };
    // Embedded profiles: output is sRGB without a profile, as sharp writes it.
    if let Some(icc) = icc {
        to_srgb(&icc, &mut pixels, alpha);
    }
    timings.resize = resized_at.elapsed();
    checkpoint()?;

    let encoded_at = Instant::now();
    let bytes = encode(job.output, pixels, out_w, out_h, alpha, job.quality)?;
    timings.encode = encoded_at.elapsed();
    Ok(Optimized {
        bytes,
        width: out_w,
        height: out_h,
        timings,
    })
}

/// Converts pixels from an embedded ICC profile to sRGB. A profile that
/// cannot be parsed or applied (a grey or CMYK profile on pixels already
/// expanded to RGB) leaves the pixels as decoded.
fn to_srgb(icc: &[u8], pixels: &mut [u8], alpha: bool) {
    let Ok(source) = moxcms::ColorProfile::new_from_slice(icc) else {
        return;
    };
    if source.color_space != moxcms::DataColorSpace::Rgb {
        return;
    }
    let srgb = moxcms::ColorProfile::new_srgb();
    let layout = if alpha {
        moxcms::Layout::Rgba
    } else {
        moxcms::Layout::Rgb
    };
    let Ok(transform) =
        source.create_transform_8bit(layout, &srgb, layout, moxcms::TransformOptions::default())
    else {
        return;
    };
    let mut out = vec![0u8; pixels.len()];
    if transform.transform(pixels, &mut out).is_ok() {
        pixels.copy_from_slice(&out);
    }
}

fn encode(
    output: ImageType,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    alpha: bool,
    quality: u8,
) -> Result<Vec<u8>, Error> {
    let encode_error = |e: &dyn std::fmt::Display| Error::Encode(e.to_string());
    let mut out = Vec::new();
    match output {
        ImageType::Jpeg => {
            // JPEG has no alpha: flatten onto black, as libvips's jpegsave
            // does with its default background.
            let rgb = if alpha { flatten(&pixels) } else { pixels };
            let (w, h) = (
                u16::try_from(width).map_err(|_| Error::Unsupported("JPEG wider than 65535"))?,
                u16::try_from(height).map_err(|_| Error::Unsupported("JPEG taller than 65535"))?,
            );
            let mut encoder = jpeg_encoder::Encoder::new(&mut out, quality.clamp(1, 100));
            encoder.set_progressive(true);
            encoder.set_optimized_huffman_tables(true);
            encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_0);
            encoder
                .encode(&rgb, w, h, jpeg_encoder::ColorType::Rgb)
                .map_err(|e| encode_error(&e))?;
        }
        ImageType::Webp => {
            out = webp::encode(&pixels, width, height, alpha, quality)
                .ok_or_else(|| Error::Encode("libwebp failed".into()))?;
        }
        ImageType::Avif => {
            // Next scales quality for AVIF: max(round(q * 50 / 80), 1).
            let q = (f32::from(quality) * 50.0 / 80.0).round().max(1.0);
            let encoder = ravif::Encoder::new()
                .with_quality(q)
                .with_alpha_quality(q)
                .with_speed(AVIF_SPEED)
                .with_num_threads(Some(1));
            let (w, h) = (width as usize, height as usize);
            let encoded = if alpha {
                let px: &[rgb::RGBA8] = rgb_slice(&pixels);
                encoder.encode_rgba(ravif::Img::new(px, w, h))
            } else {
                let px: &[rgb::RGB8] = rgb_slice(&pixels);
                encoder.encode_rgb(ravif::Img::new(px, w, h))
            }
            .map_err(|e| encode_error(&e))?;
            out = encoded.avif_file;
        }
        ImageType::Png | ImageType::Gif | ImageType::Tiff => {
            let img = if alpha {
                image::RgbaImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgba8)
            } else {
                image::RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
            }
            .ok_or_else(|| Error::Encode("pixel buffer size".into()))?;
            let mut cursor = Cursor::new(&mut out);
            match output {
                ImageType::Png => {
                    img.write_with_encoder(image::codecs::png::PngEncoder::new_with_quality(
                        &mut cursor,
                        image::codecs::png::CompressionType::Default,
                        image::codecs::png::FilterType::Adaptive,
                    ))
                }
                ImageType::Gif => {
                    // One frame; a transparent index only when the image has alpha.
                    let (w, h) = (
                        u16::try_from(width)
                            .map_err(|_| Error::Unsupported("GIF wider than 65535"))?,
                        u16::try_from(height)
                            .map_err(|_| Error::Unsupported("GIF taller than 65535"))?,
                    );
                    let mut frame = if alpha {
                        let mut rgba = img.to_rgba8().into_raw();
                        gif::Frame::from_rgba_speed(w, h, &mut rgba, 10)
                    } else {
                        gif::Frame::from_rgb_speed(w, h, img.as_bytes(), 10)
                    };
                    frame.dispose = gif::DisposalMethod::Keep;
                    let mut encoder =
                        gif::Encoder::new(&mut cursor, w, h, &[]).map_err(|e| encode_error(&e))?;
                    encoder.write_frame(&frame).map_err(|e| encode_error(&e))?;
                    Ok(())
                }
                _ => img.write_with_encoder(image::codecs::tiff::TiffEncoder::new(&mut cursor)),
            }
            .map_err(|e| encode_error(&e))?;
        }
        _ => return Err(Error::Unsupported("no encoder for this output type")),
    }
    if out.is_empty() {
        return Err(Error::Encode("empty output".into()));
    }
    Ok(out)
}

/// rav1e speed preset (0 slowest .. 10 fastest). sharp uses libaom at
/// effort 3 of 9; see the engine ADR for how this was chosen.
const AVIF_SPEED: u8 = 8;

fn rgb_slice<T: rgb::Pod>(bytes: &[u8]) -> &[T] {
    rgb::bytemuck::cast_slice(bytes)
}

fn flatten(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for px in rgba.as_chunks::<4>().0 {
        let a = u32::from(px[3]);
        for &c in &px[..3] {
            rgb.push(((u32::from(c) * a + 127) / 255) as u8);
        }
    }
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, alpha: bool) -> Vec<u8> {
        let img = if alpha {
            DynamicImage::ImageRgba8(image::RgbaImage::from_fn(width, height, |x, y| {
                image::Rgba([
                    (x % 256) as u8,
                    (y % 256) as u8,
                    128,
                    if x < width / 2 { 255 } else { 0 },
                ])
            }))
        } else {
            DynamicImage::ImageRgb8(image::RgbImage::from_fn(width, height, |x, y| {
                image::Rgb([(x % 256) as u8, (y % 256) as u8, 64])
            }))
        };
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn job(input: &[u8], output: ImageType, width: u32) -> Job<'_> {
        Job {
            input,
            output,
            width,
            quality: 75,
            limits: Limits::default(),
            deadline: None,
            cancelled: None,
        }
    }

    #[test]
    fn resizes_to_width_keeping_aspect() {
        // sharp 0.35 (scratch measurement, docs: image-format-compatibility).
        let other = SourceKind::Other;
        assert_eq!(target_size(3300, 2400, 1080, other), (1080, 785));
        assert_eq!(target_size(100, 50, 640, other), (100, 50));
        assert_eq!(target_size(1000, 1, 10, other), (10, 1));
        assert_eq!(target_size(1600, 1200, 750, other), (750, 563));
        assert_eq!(target_size(1600, 1200, 750, SourceKind::Jpeg), (750, 563));
        assert_eq!(target_size(1600, 1200, 750, SourceKind::Webp), (750, 562));
        assert_eq!(target_size(1001, 667, 256, SourceKind::Jpeg), (256, 170));
        assert_eq!(target_size(1001, 667, 256, other), (256, 171));
        assert_eq!(target_size(1001, 667, 64, SourceKind::Jpeg), (64, 42));
        assert_eq!(target_size(300, 200, 32, SourceKind::Jpeg), (32, 22));
        assert_eq!(target_size(333, 1000, 16, SourceKind::Jpeg), (16, 49));
        assert_eq!(target_size(999, 777, 16, SourceKind::Jpeg), (16, 13));
        let src = png(400, 300, false);
        for out in [
            ImageType::Webp,
            ImageType::Jpeg,
            ImageType::Png,
            ImageType::Avif,
        ] {
            let result = optimize(&job(&src, out, 128)).unwrap();
            assert_eq!((result.width, result.height), (128, 96), "{out:?}");
            assert_eq!(detect_content_type(&result.bytes).unwrap(), Some(out));
            let decoded = image::load_from_memory(&result.bytes);
            if out != ImageType::Avif {
                let decoded = decoded.unwrap();
                assert_eq!((decoded.width(), decoded.height()), (128, 96));
            }
        }
    }

    #[test]
    fn keeps_alpha() {
        let src = png(64, 64, true);
        let result = optimize(&job(&src, ImageType::Webp, 32)).unwrap();
        let decoded = image::load_from_memory(&result.bytes).unwrap().to_rgba8();
        assert!(decoded.get_pixel(2, 16)[3] > 250);
        assert!(decoded.get_pixel(29, 16)[3] < 5);
    }

    #[test]
    fn refuses_before_decoding_too_many_pixels() {
        let src = png(1000, 1000, false);
        let mut j = job(&src, ImageType::Webp, 64);
        j.limits.max_input_pixels = 999_999;
        assert_eq!(
            optimize(&j).unwrap_err(),
            Error::TooLarge {
                width: 1000,
                height: 1000
            }
        );
    }

    #[test]
    fn corrupt_and_truncated_inputs_fail() {
        let src = png(64, 64, false);
        assert!(matches!(
            optimize(&job(&src[..src.len() / 2], ImageType::Webp, 32)),
            Err(Error::Decode(_))
        ));
        assert!(matches!(
            optimize(&job(b"\xff\xd8\xffnot a jpeg", ImageType::Webp, 32)),
            Err(Error::Decode(_))
        ));
    }

    #[test]
    fn cancelled_and_expired_jobs_stop() {
        let src = png(64, 64, false);
        let cancelled = AtomicBool::new(true);
        let mut j = job(&src, ImageType::Webp, 32);
        j.cancelled = Some(&cancelled);
        assert_eq!(optimize(&j).unwrap_err(), Error::Cancelled);
        let mut j = job(&src, ImageType::Webp, 32);
        j.deadline = Some(Instant::now());
        assert_eq!(optimize(&j).unwrap_err(), Error::TimedOut);
    }
}
