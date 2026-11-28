//! The ArrayBuffer memory limit (RE-4D): a refused allocation is a
//! RangeError in JavaScript and the isolate keeps serving; dead buffers are
//! collected before a refusal; small allocations are never refused; the
//! budget is shared by the isolates and returns to zero when an isolate is
//! replaced; and no common buffer-allocating API aborts the process while
//! the budget is exhausted (in a child process).

mod common;

use std::time::Duration;

use common::*;
use nextrs_runtime::{Executor, Limits};
use serde_json::Value;

const MIB: usize = 1024 * 1024;

fn limited(mib: usize, isolates: usize) -> std::sync::Arc<Executor> {
    executor(Limits {
        isolates,
        external_memory_limit_bytes: Some(mib * MIB),
        // Generous: allocation-heavy turns in a debug build.
        stall_timeout: None,
        ..small_limits()
    })
}

async fn call(executor: &Executor, headers: &[(&str, &str)]) -> Value {
    let response = collect(executor, request("external.js", headers)).await;
    assert_eq!(
        response.status,
        200,
        "{} {:?} ended={} delivered={} chunks={} {:?}",
        response.text(),
        response.error,
        response.ended,
        response.delivered,
        response.chunks,
        executor.metrics().executors[0].last_failure
    );
    serde_json::from_str(&response.text()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_limit_nothing_is_counted() {
    let executor = executor(small_limits());
    let out = call(&executor, &[("x-mode", "alloc"), ("x-mib", "64")]).await;
    assert_eq!(out["ok"], true);
    assert!(executor.metrics().external_memory.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_allocation_past_the_limit_is_a_range_error_and_the_isolate_survives() {
    let executor = limited(32, 1);
    let out = call(&executor, &[("x-mode", "alloc"), ("x-mib", "64")]).await;
    assert_eq!(out["ok"], false);
    assert_eq!(out["error"], "RangeError", "{out}");
    let snapshot = executor.metrics();
    let external = snapshot.external_memory.unwrap();
    assert!(external.refused >= 1);
    assert_eq!(snapshot.executors[0].restarts, 0, "isolate not replaced");
    // The same isolate serves the next request.
    let out = call(&executor, &[("x-mode", "alloc"), ("x-mib", "8")]).await;
    assert_eq!(out["ok"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn dead_buffers_are_collected_before_anything_is_refused() {
    // 40 × 16 MiB = 640 MiB allocated in one turn under a 48 MiB limit:
    // only possible because V8 frees unreachable buffers and retries.
    let executor = limited(48, 1);
    let out = call(
        &executor,
        &[("x-mode", "churn"), ("x-mib", "16"), ("x-count", "40")],
    )
    .await;
    assert_eq!(out["ok"], true, "{out}");
    let external = executor.metrics().external_memory.unwrap();
    assert!(external.peak_bytes <= 48 * MIB as u64, "{external:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn retained_buffers_fill_the_budget_and_released_ones_free_it() {
    let executor = limited(64, 1);
    let out = call(&executor, &[("x-mode", "fill"), ("x-mib", "8")]).await;
    assert_eq!(out["error"], "RangeError", "{out}");
    let held = out["held"].as_u64().unwrap();
    assert!(
        (48..=64).contains(&held),
        "held {held} MiB in 8 MiB buffers under 64 MiB"
    );
    let external = executor.metrics().external_memory.unwrap();
    assert!(external.used_bytes <= 64 * MIB as u64);
    call(&executor, &[("x-mode", "release")]).await;
    // Freed once V8 collects them, which a refused allocation forces.
    let out = call(&executor, &[("x-mode", "alloc"), ("x-mib", "32")]).await;
    assert_eq!(out["ok"], true, "{out}");
}

#[tokio::test(flavor = "multi_thread")]
async fn small_allocations_are_never_refused() {
    let executor = limited(16, 1);
    call(&executor, &[("x-mode", "fill"), ("x-mib", "1")]).await;
    let out = call(&executor, &[("x-mode", "apis")]).await;
    for case in [
        "small_typed_array_buffer",
        "small_buffer_alloc",
        "buffer_from_string",
        "buffer_concat_small",
        "structured_clone_small",
        "array_buffer_slice",
    ] {
        assert_eq!(out[case]["ok"], true, "{case}: {out}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_budget_is_one_for_all_isolates_and_released_with_a_replaced_isolate() {
    let executor = limited(64, 2);
    // Isolate 0 holds 40 MiB; isolate 1 (asked while 0 is busy) cannot
    // take another 40: the limit is for all of them together.
    let a = call(
        &executor,
        &[("x-mode", "alloc"), ("x-mib", "40"), ("x-retain", "1")],
    )
    .await;
    assert_eq!(a["ok"], true);
    let used = executor.metrics().external_memory.unwrap().used_bytes;
    assert!(used >= 40 * MIB as u64);
    let blocker = executor
        .render(request(
            "external.js",
            &[("x-mode", "churn"), ("x-mib", "1"), ("x-count", "1")],
        ))
        .await
        .unwrap();
    let b = call(
        &executor,
        &[("x-mode", "alloc"), ("x-mib", "40"), ("x-retain", "1")],
    )
    .await;
    assert_eq!(b["error"], "RangeError", "{b}");
    drain(blocker).await;
    // Replace every isolate: their buffers go with them.
    for slot in 0..2 {
        executor.fault_terminate(slot);
    }
    assert!(
        eventually(Duration::from_secs(10), async || {
            let m = executor.metrics();
            m.ready_executors() == 2
                && m.executors.iter().all(|e| e.restarts >= 1)
                && m.external_memory.unwrap().used_bytes < 8 * MIB as u64
        })
        .await,
        "{:?}",
        executor.metrics().external_memory
    );
    let out = call(&executor, &[("x-mode", "alloc"), ("x-mib", "48")]).await;
    assert_eq!(out["ok"], true, "{out}");
}

/// Child: exhausts the budget, then runs every API case. The parent checks
/// that the process survived (no V8 fatal OOM) and that each case either
/// worked or threw.
#[tokio::test(flavor = "multi_thread")]
async fn child_apis_with_the_budget_exhausted() {
    if !is_child("external-apis") {
        return;
    }
    let executor = limited(16, 1);
    call(&executor, &[("x-mode", "fill"), ("x-mib", "1")]).await;
    let out = call(&executor, &[("x-mode", "apis")]).await;
    println!("APIS {out}");
    let after = call(&executor, &[("x-mode", "alloc"), ("x-mib", "1")]).await;
    println!("AFTER {after}");
}

#[test]
fn no_buffer_api_aborts_the_process_when_the_budget_is_exhausted() {
    let output = run_in_child("child_apis_with_the_budget_exhausted", "external-apis", &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "child failed ({:?}):\n{stdout}\n{stderr}",
        output.status
    );
    let line = stdout
        .lines()
        .find_map(|l| l.split_once("APIS ").map(|(_, report)| report))
        .unwrap_or_else(|| panic!("no report:\n{stdout}"));
    let out: Value = serde_json::from_str(line).unwrap();
    // Recorded for RE-4-EXTERNAL-MEMORY-ANALYSIS.
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
    for (name, result) in out.as_object().unwrap() {
        let thrown = result["error"].as_str();
        assert!(
            result["ok"] == true
                || thrown == Some("RangeError")
                || thrown == Some("TypeError")
                || thrown == Some("DOMException")
                || thrown == Some("Error"),
            "{name}: {result}"
        );
    }
}
