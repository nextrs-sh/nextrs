//! `estimate` is an upper bound of what `optimize` allocates (Rust heap,
//! counted by this test binary's allocator; libwebp's own C allocations are
//! checked by the release-mode calibration, benchmarks/estimate-calibration.sh).
//! One test: the counter is process-wide.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};

use image::DynamicImage;
use nextrs_image::{ImageType, Job, Limits, estimate, optimize};

struct Counting;
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new) };
        if !p.is_null() {
            if new > layout.size() {
                let now =
                    CURRENT.fetch_add(new - layout.size(), Ordering::Relaxed) + new - layout.size();
                PEAK.fetch_max(now, Ordering::Relaxed);
            } else {
                CURRENT.fetch_sub(layout.size() - new, Ordering::Relaxed);
            }
        }
        p
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn encode(img: &DynamicImage, format: image::ImageFormat) -> Vec<u8> {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), format).unwrap();
    out
}

fn noise(width: u32, height: u32, alpha: bool) -> DynamicImage {
    let mut s = 12345u32;
    let mut next = move || {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (s >> 24) as u8
    };
    if alpha {
        DynamicImage::ImageRgba8(image::RgbaImage::from_fn(width, height, |_, _| {
            image::Rgba([next(), next(), next(), next()])
        }))
    } else {
        DynamicImage::ImageRgb8(image::RgbImage::from_fn(width, height, |_, _| {
            image::Rgb([next(), next(), next()])
        }))
    }
}

fn photo(width: u32, height: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    }))
}

#[test]
fn estimates_bound_measured_peaks() {
    let sources = [
        (
            "jpeg 2400x1600",
            encode(&photo(2400, 1600), image::ImageFormat::Jpeg),
        ),
        (
            "png 1200x900",
            encode(&noise(1200, 900, false), image::ImageFormat::Png),
        ),
        (
            "png rgba 800x600",
            encode(&noise(800, 600, true), image::ImageFormat::Png),
        ),
        (
            "webp lossless 640x480",
            encode(&noise(640, 480, false), image::ImageFormat::WebP),
        ),
        (
            "gif 400x300",
            encode(&photo(400, 300), image::ImageFormat::Gif),
        ),
        (
            "tiff 600x400",
            encode(&photo(600, 400), image::ImageFormat::Tiff),
        ),
    ];
    let mut checked = 0;
    for (name, input) in &sources {
        for output in [
            ImageType::Webp,
            ImageType::Jpeg,
            ImageType::Png,
            ImageType::Avif,
        ] {
            // rav1e is slow unoptimized: AVIF at thumbnail sizes only.
            let widths: &[u32] = if output == ImageType::Avif {
                &[32, 64]
            } else {
                &[64, 640, 3840]
            };
            for &width in widths {
                let expected = estimate(input, output, width).unwrap();
                let base = CURRENT.load(Ordering::Relaxed);
                PEAK.store(base, Ordering::Relaxed);
                let result = optimize(&Job {
                    input,
                    output,
                    width,
                    quality: 75,
                    limits: Limits::default(),
                    deadline: None,
                    cancelled: None,
                });
                let peak = (PEAK.load(Ordering::Relaxed) - base) as u64;
                drop(result.unwrap());
                assert!(
                    peak <= expected,
                    "{name} -> {output:?} at {width}: peak {peak} above the estimate {expected}"
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, sources.len() * 11);
}
