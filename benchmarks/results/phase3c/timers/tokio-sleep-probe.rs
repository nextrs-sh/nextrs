use std::time::{Duration, Instant};
fn main() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        for ms in [0u64, 1, 5] {
            let mut v = Vec::new();
            for _ in 0..400 {
                let t = Instant::now();
                tokio::time::sleep(Duration::from_millis(ms)).await;
                v.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!("tokio sleep({ms}ms): p50 {:.3} p90 {:.3} p99 {:.3} ms", v[200], v[360], v[396]);
        }
    });
}
