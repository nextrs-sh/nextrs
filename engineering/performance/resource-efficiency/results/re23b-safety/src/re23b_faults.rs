//! RE-23B.7 scratch experiment (not part of the repository). One case per
//! process: RE23B_MODE, RE23B_MIB, RE23B_HEAP_MIB. Prints one JSON line.
mod common;
use std::time::{Duration, Instant};
use common::*;
use nextrs_runtime::Limits;

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status").ok().and_then(|s| {
        s.lines().find(|l| l.starts_with("VmHWM:")).and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
    }).unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread")]
async fn re23b_fault_case() {
    let mode = std::env::var("RE23B_MODE").unwrap_or_else(|_| "whoami".into());
    let mib = std::env::var("RE23B_MIB").unwrap_or_else(|_| "0".into());
    let heap: usize = std::env::var("RE23B_HEAP_MIB").ok().and_then(|v| v.parse().ok()).unwrap_or(64);
    let limits = Limits { heap_limit_bytes: Some(heap * 1024 * 1024), stall_timeout: None, ..small_limits() };
    let executor = executor(limits);
    let entry = "re23b-faults.js";
    let warm = collect(&executor, request(entry, &[("x-mode", "whoami")])).await;
    assert_eq!(warm.status, 200);
    let t0 = Instant::now();
    let r = collect(&executor, request(entry, &[("x-mode", mode.as_str()), ("x-mib", mib.as_str())])).await;
    let elapsed = t0.elapsed().as_millis();
    let ok_after = eventually(Duration::from_secs(10), async || executor.metrics().ready_executors() == 1).await;
    let after = collect(&executor, request(entry, &[("x-mode", "whoami")])).await;
    let m = executor.metrics();
    println!(
        "RE23B {{\"mode\":\"{mode}\",\"mib\":{mib},\"heap_mib\":{heap},\"status\":{},\"ended\":{},\"body\":{:?},\"ms\":{elapsed},\"restarts\":{},\"failures\":{:?},\"ready_after\":{ok_after},\"after_status\":{},\"vmhwm_kib\":{}}}",
        r.status, r.ended, r.text().chars().take(160).collect::<String>(), m.executors[0].restarts,
        m.executors[0].failures.iter().filter(|(_, n)| *n > 0).collect::<Vec<_>>(), after.status, rss_kib()
    );
}
