//! The image transformation behind NextRS's `/_next/image`: what Next.js's
//! `optimizeImage()` asks sharp to do (`rotate()`, `resize(width, { withoutEnlargement })`,
//! then one encoder), with Rust decoders and explicit resource limits.
//!
//! Policy (which types are optimized, bypassed or refused, which format is
//! negotiated) lives in the server; this crate only transforms pixels.

mod jpeg;
mod png;
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
            let s = f64::from(jpeg_load_shrink(src_width, width));
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

/// The shrink-on-load factor (1, 2, 4 or 8) libvips applies to a JPEG
/// `src_width` wide resized to `width` (sharp 0.35 `pipeline.cc`).
pub fn jpeg_load_shrink(src_width: u32, width: u32) -> u32 {
    if width == 0 || width >= src_width {
        return 1;
    }
    let shrink = f64::from(src_width) / f64::from(width);
    let mut s = if shrink >= 8.0 {
        8
    } else if shrink >= 4.0 {
        4
    } else if shrink >= 2.0 {
        2
    } else {
        1
    };
    // "Lower shrink-on-load for known libjpeg rounding errors".
    if s > 1 && shrink.trunc() == f64::from(s) {
        s /= 2;
    }
    s
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

/// Bytes a transformation of `input` to `output` at `width` is expected to
/// allocate at its peak: the decoder's buffers (including a progressive
/// JPEG's coefficients), the decoded pixels, orientation, resize work and
/// destination buffers, colour conversion and the encoder's own memory.
/// Read from the header alone (no pixel is decoded), so work can be admitted
/// against a memory budget before it starts. Calibrated against measured
/// peaks (`examples/measure.rs`, `tests/estimate.rs`): an upper bound for
/// the decoders and encoders NextRS uses, not an exact figure.
pub fn estimate(input: &[u8], output: ImageType, width: u32) -> Result<u64, Error> {
    let header = Header::read(input)?;
    let (w, h) = (header.width, header.height);
    let format = Some(header.format);
    let transforms = header.orientation != image::metadata::Orientation::NoTransforms;
    let decoded_bpp = u64::from(header.color.bytes_per_pixel());
    let alpha = header.color.has_alpha();
    let channels: u64 = if alpha { 4 } else { 3 };
    let has_icc = header.icc.is_some();
    let (src_w, src_h) = header.oriented();
    let (out_w, out_h) = target_size(src_w, src_h, width, header.kind());
    let out = u64::from(out_w) * u64::from(out_h);
    let full = u64::from(w) * u64::from(h);

    // Decoder: its own buffers plus the pixels it returns.
    let (dec_w, dec_h, decode) = match format {
        Some(image::ImageFormat::Jpeg) => {
            let frame = sniff::jpeg_frame(input);
            let components = frame.map_or(3, |f| u64::from(f.components));
            let shrink = header.jpeg_shrink(width);
            let (dw, dh) = (u64::from(w.div_ceil(shrink)), u64::from(h.div_ceil(shrink)));
            // Progressive scans keep every coefficient (2 bytes) of every
            // component at full resolution, whatever the decode scale.
            let coefficients = match frame {
                Some(f) if f.progressive => 2 * f.coefficient_samples(w, h),
                Some(_) => 0,
                None => 2 * components * full,
            };
            let pixels = dw * dh;
            let work = if shrink > 1 {
                // jpeg-decoder: component planes at the decode scale.
                pixels * components
            } else {
                // zune-jpeg (through image, which copies the input first):
                // MCU rows only; CMYK/YCCK through a 4-byte copy.
                input.len() as u64 + pixels * 4 * u64::from(components == 4)
            };
            (dw, dh, coefficients + work + pixels * 3)
        }
        // image-webp keeps the YUV planes (1.5 bytes) beside the output.
        Some(image::ImageFormat::WebP) => (u64::from(w), u64::from(h), full * (decoded_bpp + 2)),
        // Row buffers and the inflate window beside the pixels.
        Some(image::ImageFormat::Png) => (u64::from(w), u64::from(h), full * decoded_bpp * 5 / 4),
        _ => (u64::from(w), u64::from(h), full * decoded_bpp * 2),
    };
    let decoded = dec_w * dec_h;
    // into_rgb8/into_rgba8 copies unless the decoder already produced 8-bit
    // RGB(A); orientation other than a flip writes a rotated copy.
    let convert = if decoded_bpp == channels {
        0
    } else {
        decoded * channels
    };
    let orient = if transforms { decoded * channels } else { 0 };
    // Resize: destination, the two-pass intermediate, and a premultiplied
    // copy of the source when alpha is filtered.
    let resize = if (u64::from(out_w), u64::from(out_h)) == (dec_w, dec_h) {
        0
    } else {
        let intermediate = (u64::from(out_w) * dec_h).max(dec_w * u64::from(out_h));
        channels * (out + intermediate) + if alpha { decoded * 4 } else { 0 }
    };
    let icc = if has_icc { out * channels } else { 0 };
    // Measured per output pixel (examples/measure.rs: Rust heap, and RSS
    // for libwebp, which allocates with malloc), worst case of the corpus
    // rounded up: photos, graphics, text, gradients, noise, alpha.
    let encode = match output {
        // libwebp: YUV planes, per-macroblock state, the alpha plane.
        ImageType::Webp => out * if alpha { 28 } else { 16 },
        // Progressive + optimized Huffman keep every block's coefficients;
        // alpha is flattened through an RGB copy first.
        ImageType::Jpeg => out * if alpha { 12 } else { 9 },
        // rav1e: frames, references, analysis; noise is its worst case.
        ImageType::Avif => out * 96 + 8 * 1024 * 1024,
        ImageType::Png | ImageType::Tiff => out * channels * 3,
        ImageType::Gif => out * 10,
        _ => out * 4,
    };
    // The pixels stay alive through every later stage; the decoder's work
    // buffers are gone by then.
    let peak = (decode + convert + orient)
        .max(decoded * channels + resize + icc)
        .max(out * channels + icc + encode);
    // A margin over the measured worst cases.
    Ok(peak + peak / 10 + PER_JOB_BYTES)
}

/// Bookkeeping per transformation beyond its pixel buffers.
const PER_JOB_BYTES: u64 = 1024 * 1024;

/// Decodes any supported image to RGBA8 (tests and measurements).
pub fn decode_rgba(input: &[u8]) -> Result<(u32, u32, Vec<u8>), Error> {
    let img = image::load_from_memory(input).map_err(|e| Error::Decode(e.to_string()))?;
    let img = img.to_rgba8();
    Ok((img.width(), img.height(), img.into_raw()))
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
    let header = Header::read(job.input)?;
    let (w, h) = (header.width, header.height);
    if u64::from(w) * u64::from(h) > job.limits.max_input_pixels {
        return Err(Error::TooLarge {
            width: w,
            height: h,
        });
    }
    let orientation = header.orientation;
    let kind = header.kind();
    let icc = header.icc.clone();
    // Geometry follows the source's own (oriented) size, whatever scale
    // the decoder works at.
    let (src_w, src_h) = header.oriented();
    let (out_w, out_h) = target_size(src_w, src_h, job.width, kind);
    let shrink = header.jpeg_shrink(job.width);
    let scaled = (shrink > 1)
        .then(|| decode_jpeg_scaled(job.input, (w, h), shrink, job.limits.max_alloc_bytes))
        .flatten();
    let mut img = match scaled {
        Some(img) => img?,
        None => {
            let decoder = match header.decoder {
                Some(decoder) => decoder,
                None => Header::image_decoder(job.input)?,
            };
            decode_full(decoder, job.limits.max_alloc_bytes, (w, h))?
        }
    };
    img.apply_orientation(orientation);
    let (dec_w, dec_h) = (img.width(), img.height());
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
    let mut pixels = if (out_w, out_h) == (dec_w, dec_h) {
        pixels
    } else {
        let src = fr::images::Image::from_vec_u8(dec_w, dec_h, pixels, pixel_type)
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

/// What the transformation needs from a source before any pixel is decoded.
struct Header<'a> {
    format: image::ImageFormat,
    width: u32,
    height: u32,
    orientation: image::metadata::Orientation,
    icc: Option<Vec<u8>>,
    /// The pixel layout the decoder produces.
    color: image::ColorType,
    /// `image`'s decoder when the header was read with it (not for the
    /// JPEGs the scaled decoder can take).
    decoder: Option<Box<dyn ImageDecoder + 'a>>,
    /// The scaled JPEG decoder handles this source (8-bit grey or YCbCr).
    jpeg_scalable: bool,
}

impl<'a> Header<'a> {
    fn read(input: &'a [u8]) -> Result<Self, Error> {
        if input.starts_with(&[0xFF, 0xD8, 0xFF])
            && let Some(header) = Self::read_jpeg(input)
        {
            return Ok(header);
        }
        let decoder = Self::image_decoder(input)?;
        let format = ImageReader::new(Cursor::new(input))
            .with_guessed_format()
            .ok()
            .and_then(|r| r.format())
            .ok_or_else(|| Error::Decode("unknown image format".into()))?;
        Self::from_decoder(format, decoder)
    }

    /// A JPEG's header through jpeg-decoder, which reads it in place (image's
    /// zune-jpeg wrapper copies the whole input first). `None`: a header it
    /// does not take; image's decoder reads it instead.
    fn read_jpeg(input: &'a [u8]) -> Option<Self> {
        let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(input));
        decoder.read_info().ok()?;
        let info = decoder.info()?;
        let (color, jpeg_scalable) = match info.pixel_format {
            jpeg_decoder::PixelFormat::L8 => (image::ColorType::L8, true),
            jpeg_decoder::PixelFormat::RGB24 => (image::ColorType::Rgb8, true),
            // CMYK and 16-bit: decoded at full size by zune-jpeg, to RGB.
            _ => return None,
        };
        let orientation = decoder
            .exif_data()
            .and_then(image::metadata::Orientation::from_exif_chunk)
            .unwrap_or(image::metadata::Orientation::NoTransforms);
        Some(Self {
            format: image::ImageFormat::Jpeg,
            width: u32::from(info.width),
            height: u32::from(info.height),
            orientation,
            icc: decoder.icc_profile(),
            color,
            decoder: None,
            jpeg_scalable,
        })
    }

    fn image_decoder(input: &'a [u8]) -> Result<Box<dyn ImageDecoder + 'a>, Error> {
        let reader = ImageReader::new(Cursor::new(input))
            .with_guessed_format()
            .map_err(|e| Error::Decode(e.to_string()))?;
        Ok(Box::new(
            reader
                .into_decoder()
                .map_err(|e| Error::Decode(e.to_string()))?,
        ))
    }

    fn from_decoder(
        format: image::ImageFormat,
        mut decoder: Box<dyn ImageDecoder + 'a>,
    ) -> Result<Self, Error> {
        let (width, height) = decoder.dimensions();
        let orientation = decoder
            .orientation()
            .unwrap_or(image::metadata::Orientation::NoTransforms);
        let icc = decoder.icc_profile().ok().flatten();
        let color = decoder.color_type();
        Ok(Self {
            format,
            width,
            height,
            orientation,
            icc,
            color,
            decoder: Some(decoder),
            jpeg_scalable: false,
        })
    }

    fn rotated(&self) -> bool {
        matches!(
            self.orientation,
            image::metadata::Orientation::Rotate90
                | image::metadata::Orientation::Rotate270
                | image::metadata::Orientation::Rotate90FlipH
                | image::metadata::Orientation::Rotate270FlipH
        )
    }

    /// Width and height after EXIF orientation.
    fn oriented(&self) -> (u32, u32) {
        if self.rotated() {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }

    /// The shrink-on-load sharp's geometry assumes: none when the image
    /// must be rotated first.
    fn kind(&self) -> SourceKind {
        match self.format {
            _ if self.orientation != image::metadata::Orientation::NoTransforms => {
                SourceKind::Other
            }
            image::ImageFormat::Jpeg => SourceKind::Jpeg,
            image::ImageFormat::WebP => SourceKind::Webp,
            _ => SourceKind::Other,
        }
    }

    /// The scale the JPEG is decoded at: libvips's shrink-on-load factor
    /// (also for rotated sources, whose geometry sharp computes without
    /// it: the decoded image is still at least the output's size).
    fn jpeg_shrink(&self, width: u32) -> u32 {
        if self.jpeg_scalable {
            jpeg_load_shrink(self.oriented().0, width)
        } else {
            1
        }
    }
}

/// The whole image at full size, within the allocation limit.
fn decode_full(
    decoder: Box<dyn ImageDecoder + '_>,
    max_alloc: u64,
    (width, height): (u32, u32),
) -> Result<DynamicImage, Error> {
    let mut decoder = decoder;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(max_alloc);
    decoder
        .set_limits(limits)
        .map_err(|_| Error::TooLarge { width, height })?;
    DynamicImage::from_decoder(decoder).map_err(|e| match e {
        image::ImageError::Limits(_) => Error::TooLarge { width, height },
        e => Error::Decode(e.to_string()),
    })
}

/// Decodes a JPEG at 1/`shrink` of its size with a scaled IDCT (what libjpeg
/// does for libvips's shrink-on-load), so a large photo resized to a small
/// width never exists at full resolution. `None`: a JPEG this path does not
/// handle (CMYK/YCCK, 12-bit, a size the decoder would not scale exactly);
/// the caller decodes it at full size instead.
fn decode_jpeg_scaled(
    input: &[u8],
    (width, height): (u32, u32),
    shrink: u32,
    max_alloc: u64,
) -> Option<Result<DynamicImage, Error>> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(input));
    decoder.set_max_decoding_buffer_size(usize::try_from(max_alloc).unwrap_or(usize::MAX));
    let want = (width.div_ceil(shrink), height.div_ceil(shrink));
    let (Ok(rw), Ok(rh)) = (u16::try_from(want.0), u16::try_from(want.1)) else {
        return None;
    };
    match decoder.scale(rw, rh) {
        Ok((sw, sh)) if (u32::from(sw), u32::from(sh)) == want => {}
        _ => return None,
    }
    let format = decoder.info()?.pixel_format;
    if !matches!(
        format,
        jpeg_decoder::PixelFormat::L8 | jpeg_decoder::PixelFormat::RGB24
    ) {
        return None;
    }
    Some(match decoder.decode() {
        Ok(pixels) => {
            let (w, h) = want;
            match format {
                jpeg_decoder::PixelFormat::L8 => {
                    image::GrayImage::from_raw(w, h, pixels).map(DynamicImage::ImageLuma8)
                }
                _ => image::RgbImage::from_raw(w, h, pixels).map(DynamicImage::ImageRgb8),
            }
            .ok_or_else(|| Error::Decode("scaled JPEG decode: pixel buffer size".into()))
        }
        Err(jpeg_decoder::Error::Format(e)) => Err(Error::Decode(e)),
        Err(jpeg_decoder::Error::Unsupported(_)) => return None,
        Err(e) => Err(Error::Decode(e.to_string())),
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
            out = jpeg::encode(&rgb, width, height, quality).map_err(Error::Encode)?;
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
        ImageType::Png => {
            let channels = if alpha { 4 } else { 3 };
            out = png::encode(&pixels, width, height, channels).map_err(Error::Encode)?;
        }
        ImageType::Gif | ImageType::Tiff => {
            let img = if alpha {
                image::RgbaImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgba8)
            } else {
                image::RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
            }
            .ok_or_else(|| Error::Encode("pixel buffer size".into()))?;
            let mut cursor = Cursor::new(&mut out);
            match output {
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

    fn jpeg(width: u32, height: u32, grey: bool) -> Vec<u8> {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(width, height, |x, y| {
            let v = ((x / 7 + y / 5) % 2) as u8 * 120;
            image::Rgb([(x % 256) as u8 / 2 + v, (y % 256) as u8 / 2 + v / 2, 90])
        }));
        let img = if grey {
            DynamicImage::ImageLuma8(img.to_luma8())
        } else {
            img
        };
        let mut out = Vec::new();
        img.write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut out, 92,
        ))
        .unwrap();
        out
    }

    /// Splices an EXIF APP1 segment with the given orientation after SOI.
    fn with_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        let mut tiff = b"MM\0\x2a\0\0\0\x08".to_vec();
        tiff.extend_from_slice(&1u16.to_be_bytes()); // one entry
        tiff.extend_from_slice(&0x0112u16.to_be_bytes()); // Orientation
        tiff.extend_from_slice(&3u16.to_be_bytes()); // SHORT
        tiff.extend_from_slice(&1u32.to_be_bytes());
        tiff.extend_from_slice(&orientation.to_be_bytes());
        tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let mut out = jpeg[..2].to_vec();
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&(app1.len() as u16 + 2).to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    /// Mean absolute difference per channel between two RGB images.
    fn mean_diff(a: &image::RgbImage, b: &image::RgbImage) -> f64 {
        assert_eq!(a.dimensions(), b.dimensions());
        let sum: u64 = a
            .as_raw()
            .iter()
            .zip(b.as_raw())
            .map(|(x, y)| u64::from(x.abs_diff(*y)))
            .sum();
        sum as f64 / a.as_raw().len() as f64
    }

    /// The full-size decode resized with the same filter: the reference a
    /// scaled decode is compared with.
    fn reference(input: &[u8], width: u32, height: u32) -> image::RgbImage {
        let mut img = image::load_from_memory(input).unwrap();
        let mut decoder = ImageReader::new(Cursor::new(input))
            .with_guessed_format()
            .unwrap()
            .into_decoder()
            .unwrap();
        img.apply_orientation(decoder.orientation().unwrap());
        let src = img.to_rgb8();
        let (w, h) = src.dimensions();
        let src =
            fr::images::Image::from_vec_u8(w, h, src.into_raw(), fr::PixelType::U8x3).unwrap();
        let mut dst = fr::images::Image::new(width, height, fr::PixelType::U8x3);
        fr::Resizer::new()
            .resize(
                &src,
                &mut dst,
                &fr::ResizeOptions::new()
                    .resize_alg(fr::ResizeAlg::Convolution(fr::FilterType::Lanczos3)),
            )
            .unwrap();
        image::RgbImage::from_raw(width, height, dst.into_vec()).unwrap()
    }

    #[test]
    fn large_jpegs_decode_scaled_with_sharps_geometry_and_close_pixels() {
        let src = jpeg(1600, 1200, false);
        // 2.5 and 4.0 hit sharp's "libjpeg rounding" exception (one step less).
        for (width, shrink) in [
            (800, 1),
            (640, 1),
            (500, 2),
            (400, 2),
            (300, 4),
            (128, 8),
            (32, 8),
        ] {
            assert_eq!(jpeg_load_shrink(1600, width), shrink, "{width}");
            let result = optimize(&job(&src, ImageType::Png, width)).unwrap();
            assert_eq!(
                (result.width, result.height),
                target_size(1600, 1200, width, SourceKind::Jpeg),
                "{width}"
            );
            let out = image::load_from_memory(&result.bytes).unwrap().to_rgb8();
            let diff = mean_diff(&out, &reference(&src, result.width, result.height));
            assert!(diff < 4.0, "width {width}: mean difference {diff}");
        }
        // Estimated without decoding; the scaled decode is far below it.
        assert!(estimate(&src, ImageType::Webp, 128).unwrap() < 4 * 1024 * 1024);
        assert!(estimate(&src, ImageType::Webp, 1600).unwrap() > 1600 * 1200 * 3);
    }

    #[test]
    fn scaled_jpegs_keep_orientation_and_greyscale() {
        let src = with_orientation(&jpeg(1200, 800, false), 6);
        // Rotated: sharp computes the size without shrink-on-load.
        let result = optimize(&job(&src, ImageType::Png, 100)).unwrap();
        assert_eq!((result.width, result.height), (100, 150));
        assert_eq!(dimensions(&src).unwrap(), (800, 1200));
        let out = image::load_from_memory(&result.bytes).unwrap().to_rgb8();
        let diff = mean_diff(&out, &reference(&src, 100, 150));
        assert!(diff < 4.0, "rotated: mean difference {diff}");
        let grey = jpeg(1200, 800, true);
        let result = optimize(&job(&grey, ImageType::Png, 150)).unwrap();
        assert_eq!((result.width, result.height), (150, 100));
        let out = image::load_from_memory(&result.bytes).unwrap().to_rgb8();
        assert!(out.pixels().all(|p| p[0] == p[1] && p[1] == p[2]));
        let diff = mean_diff(&out, &reference(&grey, 150, 100));
        assert!(diff < 4.0, "grey: mean difference {diff}");
    }

    #[test]
    fn corrupt_scaled_jpegs_fail_cleanly() {
        let src = jpeg(1600, 1200, false);
        // Header intact, entropy-coded data cut short.
        let truncated = &src[..src.len() / 3];
        assert!(optimize(&job(truncated, ImageType::Webp, 128)).is_err());
        let mut garbage = src.clone();
        let half = garbage.len() / 2;
        for b in &mut garbage[half..half + 512] {
            *b = 0xFF;
        }
        let _ = optimize(&job(&garbage, ImageType::Webp, 128));
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
