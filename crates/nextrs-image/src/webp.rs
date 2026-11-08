//! Lossy WebP encoding through libwebp's simple API (`WebPEncodeRGB(A)`:
//! method 4, alpha quality 100), the settings sharp's `webp({ quality })`
//! uses.

use std::os::raw::c_int;

pub(crate) fn encode(
    pixels: &[u8],
    width: u32,
    height: u32,
    alpha: bool,
    quality: u8,
) -> Option<Vec<u8>> {
    let channels: usize = if alpha { 4 } else { 3 };
    let stride = (width as usize).checked_mul(channels)?;
    if pixels.len() != stride.checked_mul(height as usize)?
        || width == 0
        || height == 0
        // libwebp's own limit (WEBP_MAX_DIMENSION).
        || width > 16383
        || height > 16383
    {
        return None;
    }
    let mut out: *mut u8 = std::ptr::null_mut();
    // SAFETY: `pixels` holds `height` rows of `stride` bytes (checked
    // above) and outlives the call; libwebp writes a buffer it allocated
    // to `out` and returns its length (0 on failure).
    let len = unsafe {
        let encode = if alpha {
            libwebp_sys::WebPEncodeRGBA
        } else {
            libwebp_sys::WebPEncodeRGB
        };
        encode(
            pixels.as_ptr(),
            width as c_int,
            height as c_int,
            stride as c_int,
            f32::from(quality),
            &mut out,
        )
    };
    if out.is_null() {
        return None;
    }
    // SAFETY: on success `out` points to `len` initialized bytes owned by
    // libwebp; they are copied before `WebPFree` releases them.
    unsafe {
        let bytes = (len > 0).then(|| std::slice::from_raw_parts(out, len).to_vec());
        libwebp_sys::WebPFree(out.cast());
        bytes
    }
}
