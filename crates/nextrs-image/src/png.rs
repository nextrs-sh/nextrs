//! PNG output, lossless: an indexed PNG (at the smallest bit depth) when
//! the output has at most 256 distinct colours, greyscale when every pixel
//! is grey, full colour otherwise. sharp's `png({ quality })` quantizes to
//! a palette (lossy, visibly so on gradients and alpha); NextRS does not.

use std::borrow::Cow;
use std::collections::HashMap;

/// `pixels` is RGB or RGBA (`channels` 3 or 4).
pub(crate) fn encode(
    pixels: &[u8],
    width: u32,
    height: u32,
    channels: usize,
) -> Result<Vec<u8>, String> {
    if pixels.len() != width as usize * height as usize * channels {
        return Err("pixel buffer size".into());
    }
    let mut out = Vec::with_capacity(pixels.len() / 2);
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_compression(png::Compression::Balanced);
        encoder.set_filter(png::Filter::Adaptive);
        let data = if let Some((palette, indices)) = palette(pixels, channels) {
            let depth = match palette.len() {
                0..=2 => png::BitDepth::One,
                3..=4 => png::BitDepth::Two,
                5..=16 => png::BitDepth::Four,
                _ => png::BitDepth::Eight,
            };
            encoder.set_color(png::ColorType::Indexed);
            encoder.set_depth(depth);
            // Indexed images are not filtered (libpng's advice, and smaller).
            encoder.set_filter(png::Filter::NoFilter);
            encoder.set_palette(
                palette
                    .iter()
                    .flat_map(|c| [c[0], c[1], c[2]])
                    .collect::<Vec<u8>>(),
            );
            if palette.iter().any(|c| c[3] != 255) {
                // Entries after the last translucent one default to opaque.
                let last = palette.iter().rposition(|c| c[3] != 255).unwrap_or(0);
                encoder.set_trns(palette[..=last].iter().map(|c| c[3]).collect::<Vec<u8>>());
            }
            Cow::Owned(pack(&indices, width as usize, depth))
        } else if grey(pixels, channels) {
            encoder.set_depth(png::BitDepth::Eight);
            if channels == 4 {
                encoder.set_color(png::ColorType::GrayscaleAlpha);
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| [p[0], p[3]])
                    .collect()
            } else {
                encoder.set_color(png::ColorType::Grayscale);
                Cow::Owned(pixels.as_chunks::<3>().0.iter().map(|p| p[0]).collect())
            }
        } else {
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_color(if channels == 4 {
                png::ColorType::Rgba
            } else {
                png::ColorType::Rgb
            });
            Cow::Borrowed(pixels)
        };
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&data).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

type Palette = Vec<[u8; 4]>;

/// The image's distinct colours and each pixel's index, if there are at most
/// 256 (stops at the 257th: photos bail out after a few pixels).
fn palette(pixels: &[u8], channels: usize) -> Option<(Palette, Vec<u8>)> {
    let mut colours: Palette = Vec::new();
    let mut index: HashMap<u32, u8> = HashMap::new();
    let mut indices = Vec::with_capacity(pixels.len() / channels);
    let mut last: Option<(u32, u8)> = None;
    for p in pixels.chunks_exact(channels) {
        let c = [p[0], p[1], p[2], if channels == 4 { p[3] } else { 255 }];
        let key = u32::from_le_bytes(c);
        let i = match last {
            Some((k, i)) if k == key => i,
            _ => {
                let i = match index.get(&key) {
                    Some(&i) => i,
                    None => {
                        if colours.len() == 256 {
                            return None;
                        }
                        colours.push(c);
                        let i = (colours.len() - 1) as u8;
                        index.insert(key, i);
                        i
                    }
                };
                last = Some((key, i));
                i
            }
        };
        indices.push(i);
    }
    Some((colours, indices))
}

fn grey(pixels: &[u8], channels: usize) -> bool {
    pixels
        .chunks_exact(channels)
        .all(|p| p[0] == p[1] && p[1] == p[2])
}

/// Packs 8-bit indices into rows of `depth` bits, each row byte-aligned.
fn pack(indices: &[u8], width: usize, depth: png::BitDepth) -> Vec<u8> {
    let bits = depth as usize;
    if bits == 8 {
        return indices.to_vec();
    }
    let per_byte = 8 / bits;
    let row_bytes = width.div_ceil(per_byte);
    let mut out = Vec::with_capacity(row_bytes * indices.len() / width.max(1));
    for row in indices.chunks(width) {
        for group in row.chunks(per_byte) {
            let mut byte = 0u8;
            for (k, &i) in group.iter().enumerate() {
                byte |= i << (8 - bits * (k + 1));
            }
            out.push(byte);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    fn roundtrip(
        pixels: &[u8],
        w: u32,
        h: u32,
        channels: usize,
    ) -> (png::ColorType, png::BitDepth) {
        let bytes = super::encode(pixels, w, h, channels).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        let info = decoder.read_info().unwrap().info().clone();
        let decoded = image::load_from_memory(&bytes).unwrap();
        let expected: Vec<u8> = if channels == 4 {
            pixels.to_vec()
        } else {
            pixels
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect()
        };
        assert_eq!(decoded.to_rgba8().into_raw(), expected, "lossless");
        (info.color_type, info.bit_depth)
    }

    #[test]
    fn few_colours_become_an_indexed_png_losslessly() {
        // 3 colours, 7 px wide (rows not byte-aligned at 2 bits).
        let px: Vec<u8> = (0..7 * 5)
            .flat_map(|i| match i % 3 {
                0 => [255, 0, 0, 255],
                1 => [0, 0, 255, 128],
                _ => [10, 200, 10, 0],
            })
            .collect();
        assert_eq!(
            roundtrip(&px, 7, 5, 4),
            (png::ColorType::Indexed, png::BitDepth::Two)
        );
        let rgb: Vec<u8> = (0..33 * 3).flat_map(|i| [(i % 200) as u8, 7, 9]).collect();
        assert_eq!(
            roundtrip(&rgb, 33, 3, 3),
            (png::ColorType::Indexed, png::BitDepth::Eight)
        );
    }

    #[test]
    fn grey_and_full_colour_images_stay_lossless() {
        let grey: Vec<u8> = (0..64 * 64)
            .flat_map(|i| {
                let v = (i * 7 % 256) as u8;
                [v, v, v]
            })
            .collect();
        // 256 greys fit a palette; more than that are written as greyscale.
        assert_eq!(roundtrip(&grey, 64, 64, 3).0, png::ColorType::Indexed);
        let grey_alpha: Vec<u8> = (0..64 * 64)
            .flat_map(|i| {
                let v = (i % 256) as u8;
                [v, v, v, (i / 256 * 16) as u8]
            })
            .collect();
        assert_eq!(
            roundtrip(&grey_alpha, 64, 64, 4).0,
            png::ColorType::GrayscaleAlpha
        );
        let colour: Vec<u8> = (0..64 * 64)
            .flat_map(|i| [(i % 256) as u8, (i / 64) as u8, 3])
            .collect();
        assert_eq!(roundtrip(&colour, 64, 64, 3).0, png::ColorType::Rgb);
    }
}
