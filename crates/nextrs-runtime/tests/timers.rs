//! Timer behaviour inside an isolate (Phase 3C timer analysis). The probe
//! (fixtures/handlers/dist/timer-probe.js) also runs under Node.js, so the
//! latencies can be compared; see benchmarks/phase3c/timers.sh.

mod common;

use common::*;

fn probe(iterations: &str) -> serde_json::Value {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let executor = executor(small_limits());
        let response = collect(
            &executor,
            request("timers.js", &[("x-iterations", iterations)]),
        )
        .await;
        assert_eq!(response.status, 200, "{}", response.text());
        serde_json::from_str(&response.text()).expect("probe JSON")
    })
}

/// Cleared timers never fire; the others all do; no timer fires before
/// its delay.
#[test]
fn timers_fire_in_order_and_cleared_ones_never_fire() {
    let result = probe("20");
    let cancellation = &result["cancellation"];
    assert_eq!(cancellation["fired"], 100, "{result}");
    assert_eq!(cancellation["firedAfterCancel"], 0, "{result}");
    // A 5 ms timer is never early (1 ms of clock granularity allowed).
    let min_5 = result["setTimeout5"]["p50"].as_f64().unwrap();
    assert!(min_5 >= 4.0, "{result}");
    // Behind 3 ms of synchronous work, a 1 ms timer fires after the work.
    assert!(
        result["behindCpu3ms"]["p50"].as_f64().unwrap() >= 3.0,
        "{result}"
    );
}

/// The full latency report (prints JSON). Run with
/// `cargo test -p nextrs-runtime --release --test timers -- --ignored --nocapture`.
#[test]
#[ignore = "measurement, not a check"]
fn timer_latency_report() {
    let result = probe("400");
    println!("TIMER-REPORT {result}");
}
