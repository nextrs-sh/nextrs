//! One transformation through `nextrs_image::optimize`, measured: wall time
//! per stage, CPU time, peak heap bytes (a counting global allocator) and,
//! with `--ssim`, the fidelity of the output against an unencoded Lanczos3
//! reference made from the full-size decode.
//!
//!   cargo run --release -p nextrs-image --example measure -- \
//!       INPUT FORMAT WIDTH QUALITY [ITERATIONS] [--ssim] [--out FILE]
//!
//! Prints one JSON line. FORMAT: jpeg, png, webp, avif, gif.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

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
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size > layout.size() {
                let now = CURRENT.fetch_add(new_size - layout.size(), Ordering::Relaxed) + new_size
                    - layout.size();
                PEAK.fetch_max(now, Ordering::Relaxed);
            } else {
                CURRENT.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        p
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn cpu_ms() -> f64 {
    let mut ts = libc_timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // CLOCK_THREAD_CPUTIME_ID = 3 on Linux.
    unsafe { clock_gettime(3, &mut ts) };
    ts.tv_sec as f64 * 1e3 + ts.tv_nsec as f64 / 1e6
}

#[repr(C)]
struct libc_timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

unsafe extern "C" {
    fn clock_gettime(clock: i32, ts: *mut libc_timespec) -> i32;
}

/// The process's RSS or peak RSS (C allocations included: libwebp allocates
/// with malloc, past the counting allocator).
fn rss_kib(field: &str) -> i64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|l| l.strip_prefix(field))
        .and_then(|v| v.trim().trim_end_matches(" kB").trim().parse().ok())
        .unwrap_or(0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let positional: Vec<&String> = {
        let mut skip = false;
        args.iter()
            .filter(|a| {
                if skip {
                    skip = false;
                    return false;
                }
                if *a == "--out" {
                    skip = true;
                    return false;
                }
                !a.starts_with("--")
            })
            .collect()
    };
    let input = std::fs::read(positional[0]).expect("input");
    let output = match positional[1].as_str() {
        "jpeg" => nextrs_image::ImageType::Jpeg,
        "png" => nextrs_image::ImageType::Png,
        "webp" => nextrs_image::ImageType::Webp,
        "avif" => nextrs_image::ImageType::Avif,
        "gif" => nextrs_image::ImageType::Gif,
        other => panic!("format {other}"),
    };
    let width: u32 = positional[2].parse().expect("width");
    let quality: u8 = positional[3].parse().expect("quality");
    let iterations: usize = positional
        .get(4)
        .map_or(3, |s| s.parse().expect("iterations"));

    let rss_before = rss_kib("VmRSS:");
    let mut best_ms = f64::MAX;
    let mut best_cpu = f64::MAX;
    let mut peak = 0usize;
    let mut last = None;
    for _ in 0..iterations {
        let base = CURRENT.load(Ordering::Relaxed);
        PEAK.store(base, Ordering::Relaxed);
        let (t0, c0) = (Instant::now(), cpu_ms());
        let result = nextrs_image::optimize(&nextrs_image::Job {
            input: &input,
            output,
            width,
            quality,
            limits: nextrs_image::Limits::default(),
            deadline: None,
            cancelled: None,
        })
        .expect("optimize");
        let (ms, cpu) = (t0.elapsed().as_secs_f64() * 1e3, cpu_ms() - c0);
        peak = peak.max(PEAK.load(Ordering::Relaxed) - base);
        best_ms = best_ms.min(ms);
        best_cpu = best_cpu.min(cpu);
        last = Some(result);
    }
    let result = last.expect("one iteration");
    let peak_rss_delta = rss_kib("VmHWM:") - rss_before;
    let estimate = nextrs_image::estimate(&input, output, width).ok();
    let ssim = flag("--ssim").then(|| fidelity(&input, &result.bytes, result.width, result.height));
    if let Some(path) = value("--out") {
        std::fs::write(path, &result.bytes).expect("write output");
    }
    let t = &result.timings;
    println!(
        "{{\"input\":{:?},\"format\":{:?},\"width\":{width},\"quality\":{quality},\"out\":[{},{}],\"bytes\":{},\"ms\":{:.2},\"cpu_ms\":{:.2},\"decode_ms\":{:.2},\"resize_ms\":{:.2},\"encode_ms\":{:.2},\"peak_heap_mib\":{:.2},\"estimate_mib\":{},\"ssim\":{},\"psnr\":{},\"peak_rss_delta_mib\":{:.1}}}",
        positional[0],
        positional[1],
        result.width,
        result.height,
        result.bytes.len(),
        best_ms,
        best_cpu,
        t.decode.as_secs_f64() * 1e3,
        t.resize.as_secs_f64() * 1e3,
        t.encode.as_secs_f64() * 1e3,
        peak as f64 / 1048576.0,
        estimate.map_or("null".into(), |e| format!("{:.2}", e as f64 / 1048576.0)),
        ssim.map_or("null".into(), |s| format!("{:.4}", s.0)),
        ssim.map_or("null".into(), |s| format!("{:.2}", s.1)),
        peak_rss_delta as f64 / 1024.0,
    );
}

/// SSIM (luma, 8x8 blocks, stride 4) and PSNR of the decoded output against
/// the full-size decode resized with Lanczos3 (unencoded), both flattened on
/// white: the same metric as the Phase 2D engine benchmark.
fn fidelity(input: &[u8], output: &[u8], w: u32, h: u32) -> (f64, f64) {
    use fast_image_resize as fr;
    let mut src = image::load_from_memory(input).expect("reference decode");
    if let Ok(mut d) = image::ImageReader::new(std::io::Cursor::new(input))
        .with_guessed_format()
        .unwrap()
        .into_decoder()
    {
        use image::ImageDecoder;
        if let Ok(o) = d.orientation() {
            src.apply_orientation(o);
        }
    }
    let src = src.to_rgba8();
    let (sw, sh) = src.dimensions();
    let reference = if (sw, sh) == (w, h) {
        src.into_raw()
    } else {
        let s =
            fr::images::Image::from_vec_u8(sw, sh, src.into_raw(), fr::PixelType::U8x4).unwrap();
        let mut d = fr::images::Image::new(w, h, fr::PixelType::U8x4);
        fr::Resizer::new()
            .resize(
                &s,
                &mut d,
                &fr::ResizeOptions::new()
                    .resize_alg(fr::ResizeAlg::Convolution(fr::FilterType::Lanczos3))
                    .use_alpha(true),
            )
            .unwrap();
        d.into_vec()
    };
    let out = nextrs_image_decode(output);
    assert_eq!(out.len(), reference.len(), "output dimensions");
    let flat = |px: &[u8]| -> [f64; 3] {
        let a = f64::from(px[3]) / 255.0;
        [0, 1, 2].map(|c| f64::from(px[c]) * a + 255.0 * (1.0 - a))
    };
    let (mut se, mut n) = (0.0, 0usize);
    let mut la = vec![0f64; (w * h) as usize];
    let mut lb = vec![0f64; (w * h) as usize];
    for i in 0..(w * h) as usize {
        let (a, b) = (flat(&reference[i * 4..]), flat(&out[i * 4..]));
        for c in 0..3 {
            se += (a[c] - b[c]).powi(2);
            n += 1;
        }
        la[i] = 0.299 * a[0] + 0.587 * a[1] + 0.114 * a[2];
        lb[i] = 0.299 * b[0] + 0.587 * b[1] + 0.114 * b[2];
    }
    let psnr = 10.0 * (255.0f64 * 255.0 / (se / n as f64).max(1e-9)).log10();
    let (c1, c2) = ((0.01f64 * 255.0).powi(2), (0.03f64 * 255.0).powi(2));
    let (mut total, mut blocks) = (0.0, 0usize);
    let (w, h) = (w as usize, h as usize);
    let mut y = 0;
    while y + 8 <= h {
        let mut x = 0;
        while x + 8 <= w {
            let (mut ma, mut mb) = (0.0, 0.0);
            for j in 0..8 {
                for k in 0..8 {
                    ma += la[(y + j) * w + x + k];
                    mb += lb[(y + j) * w + x + k];
                }
            }
            ma /= 64.0;
            mb /= 64.0;
            let (mut va, mut vb, mut cv) = (0.0, 0.0, 0.0);
            for j in 0..8 {
                for k in 0..8 {
                    let (a, b) = (la[(y + j) * w + x + k] - ma, lb[(y + j) * w + x + k] - mb);
                    va += a * a;
                    vb += b * b;
                    cv += a * b;
                }
            }
            va /= 63.0;
            vb /= 63.0;
            cv /= 63.0;
            total += ((2.0 * ma * mb + c1) * (2.0 * cv + c2))
                / ((ma * ma + mb * mb + c1) * (va + vb + c2));
            blocks += 1;
            x += 4;
        }
        y += 4;
    }
    (
        if blocks == 0 {
            1.0
        } else {
            total / blocks as f64
        },
        psnr,
    )
}

/// Decodes an output (AVIF through the crate's own decoder when built with it).
fn nextrs_image_decode(bytes: &[u8]) -> Vec<u8> {
    match nextrs_image::decode_rgba(bytes) {
        Ok((_, _, px)) => px,
        Err(e) => panic!("decoding the output: {e}"),
    }
}
