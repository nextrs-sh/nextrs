//! JPEG output through mozjpeg (libjpeg-turbo with trellis quantization),
//! as sharp's `jpeg({ quality, mozjpeg: true })` writes it: mozjpeg's
//! defaults (trellis quantization, overshoot deringing, quantization table
//! 3), progressive with optimized scans, 4:2:0. C code that only encodes
//! pixels NextRS decoded itself; its decoder is never used.

/// libjpeg's JPEG_MAX_DIMENSION.
const MAX_DIMENSION: u32 = 65_500;

pub(crate) fn encode(rgb: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(format!(
            "JPEG of {width}x{height} is outside libjpeg's limits"
        ));
    }
    if rgb.len() != width as usize * height as usize * 3 {
        return Err("pixel buffer size".into());
    }
    // libjpeg reports errors by unwinding out of its error handler
    // (mozjpeg-sys is built with -fexceptions); they end here.
    std::panic::catch_unwind(|| -> std::io::Result<Vec<u8>> {
        let mut compress = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
        compress.set_size(width as usize, height as usize);
        compress.set_quality(f32::from(quality.clamp(1, 100)));
        compress.set_chroma_sampling_pixel_sizes((2, 2), (2, 2));
        compress.set_progressive_mode();
        compress.set_optimize_scans(true);
        compress.set_optimize_coding(true);
        let mut started = compress.start_compress(Vec::with_capacity(rgb.len() / 8))?;
        started.write_scanlines(rgb)?;
        started.finish()
    })
    .map_err(|_| "libjpeg failed".to_owned())?
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn encodes_and_refuses_impossible_sizes() {
        let rgb: Vec<u8> = (0..64 * 48 * 3).map(|i| (i % 251) as u8).collect();
        let out = super::encode(&rgb, 64, 48, 75).unwrap();
        let decoded = image::load_from_memory(&out).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (64, 48));
        assert!(super::encode(&rgb, 64, 47, 75).is_err());
        assert!(super::encode(&[], 70_000, 1, 75).is_err());
    }
}
