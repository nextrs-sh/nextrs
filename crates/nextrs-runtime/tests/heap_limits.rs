//! Heap-limit behaviour (RE-4E), one case per child process, because a
//! fatal V8 out-of-memory error aborts the whole process:
//!
//! * a large allocation V8 can still recover from replaces the isolate,
//!   and the callback calls and limits of the episode are recorded;
//! * an allocation past the fatal boundary aborts its process (documented
//!   behaviour, not a goal: the embedder cannot catch it);
//! * a heap that reaches its limit right after the ArrayBuffer budget
//!   refused an allocation is still replaced, not left to abort.
//!
//! `child_heap_fault` is also the driver of the RE-4E overshoot sweep
//! (see RE-4-V8-HEAP-OVERSHOOT for the command).

mod common;

use std::time::{Duration, Instant};

use common::*;
use nextrs_runtime::Limits;
use serde_json::{Value, json};

const MIB: usize = 1024 * 1024;

fn env_number(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Peak resident memory of this process, KiB.
fn vm_hwm_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

/// Child: one fault case from the environment (`FAULT_MODE`, `FAULT_MIB`,
/// `FAULT_HEAP_MIB`, optional `FAULT_EXTERNAL_MIB`). Prints `CASE {json}`.
#[tokio::test(flavor = "multi_thread")]
async fn child_heap_fault() {
    if !is_child("heap-fault") {
        return;
    }
    let mode = std::env::var("FAULT_MODE").unwrap_or_else(|_| "whoami".into());
    let mib = env_number("FAULT_MIB", 0).to_string();
    let heap = env_number("FAULT_HEAP_MIB", 64);
    let external = env_number("FAULT_EXTERNAL_MIB", 0);
    let retain = env_number("FAULT_RETAIN_MIB", 0).to_string();
    let executor = executor(Limits {
        heap_limit_bytes: Some(heap * MIB),
        external_memory_limit_bytes: (external > 0).then_some(external * MIB),
        stall_timeout: None,
        ..small_limits()
    });
    let entry = "heap-faults.js";
    assert_eq!(
        collect(&executor, request(entry, &[("x-mode", "whoami")]))
            .await
            .status,
        200
    );
    let started = Instant::now();
    let response = collect(
        &executor,
        request(
            entry,
            &[
                ("x-mode", mode.as_str()),
                ("x-mib", mib.as_str()),
                ("x-retain-mib", retain.as_str()),
            ],
        ),
    )
    .await;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let replaced = eventually(Duration::from_secs(20), async || {
        let m = executor.metrics();
        m.ready_executors() == 1
            && (m.executors[0].restarts > 0 || m.executors[0].last_heap_limit.is_none())
            && m.executors[0]
                .last_heap_limit
                .is_none_or(|e| e.replacement_ms > 0)
    })
    .await;
    let after = collect(&executor, request(entry, &[("x-mode", "whoami")])).await;
    let m = executor.metrics();
    let slot = &m.executors[0];
    let case = json!({
        "mode": mode,
        "mib": mib.parse::<u64>().unwrap(),
        "heap_mib": heap,
        "external_mib": external,
        "status": response.status,
        "body": response.text().chars().take(160).collect::<String>(),
        "ms": elapsed_ms,
        "restarts": slot.restarts,
        "heap_limit_failures": slot.failures.iter().find(|(r, _)| *r == "heap_limit").map_or(0, |(_, n)| *n),
        "near_limit_calls": slot.near_heap_limit_calls,
        "near_limit_declined": slot.near_heap_limit_declined,
        "episode": slot.last_heap_limit,
        "ready_after": replaced,
        "after_status": after.status,
        "vmhwm_mib": vm_hwm_kib() / 1024,
    });
    println!("CASE {case}");
}

struct Outcome {
    success: bool,
    case: Option<Value>,
    stderr: String,
}

fn run(mode: &str, mib: usize, heap_mib: usize, external_mib: usize) -> Outcome {
    run_retaining(mode, mib, heap_mib, external_mib, 0)
}

fn run_retaining(
    mode: &str,
    mib: usize,
    heap_mib: usize,
    external_mib: usize,
    retain_mib: usize,
) -> Outcome {
    let (mib, heap, external, retain) = (
        mib.to_string(),
        heap_mib.to_string(),
        external_mib.to_string(),
        retain_mib.to_string(),
    );
    let output = run_in_child(
        "child_heap_fault",
        "heap-fault",
        &[
            ("FAULT_MODE", mode),
            ("FAULT_MIB", &mib),
            ("FAULT_HEAP_MIB", &heap),
            ("FAULT_EXTERNAL_MIB", &external),
            ("FAULT_RETAIN_MIB", &retain),
        ],
    );
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let case = stdout
        .lines()
        .find_map(|l| l.split_once("CASE ").map(|(_, c)| c))
        .map(|c| serde_json::from_str(c).unwrap());
    Outcome {
        success: output.status.success(),
        case,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn a_recoverable_large_allocation_replaces_the_isolate_and_is_recorded() {
    let outcome = run("heap-string", 96, 64, 0);
    assert!(outcome.success, "{}", outcome.stderr);
    let case = outcome.case.expect("case report");
    assert_eq!(case["restarts"], 1, "{case}");
    assert_eq!(case["heap_limit_failures"], 1, "{case}");
    assert_eq!(case["after_status"], 200, "{case}");
    let episode = &case["episode"];
    assert!(episode["callback_calls"].as_u64().unwrap() >= 1, "{case}");
    assert!(episode["replacement_ms"].as_u64().unwrap() > 0, "{case}");
    assert!(
        episode["last_limit_bytes"].as_u64() >= episode["first_limit_bytes"].as_u64(),
        "{case}"
    );
    println!("{case}");
}

#[test]
fn an_allocation_past_the_fatal_boundary_aborts_its_process() {
    // Documents V8's behaviour (RE-23B: 192 MiB at a 64 MiB limit): the
    // embedder cannot recover it. Must stay in a child process.
    let outcome = run("heap-string", 256, 64, 0);
    assert!(
        !outcome.success,
        "expected a fatal abort; got {:?}",
        outcome.case
    );
    assert!(outcome.case.is_none());
    assert!(
        outcome.stderr.contains("out of memory"),
        "V8's fatal message: {}",
        outcome.stderr
    );
}

#[test]
fn a_full_heap_right_after_an_external_refusal_is_still_replaced() {
    let outcome = run("refuse-then-grow", 64, 64, 16);
    assert!(outcome.success, "must not abort: {}", outcome.stderr);
    let case = outcome.case.expect("case report");
    assert_eq!(case["heap_limit_failures"], 1, "{case}");
    assert_eq!(case["restarts"], 1, "{case}");
    assert_eq!(case["after_status"], 200, "{case}");
    println!("{case}");
}

#[test]
fn an_external_refusal_alone_declines_the_callback_and_keeps_the_isolate() {
    let outcome = run("external", 128, 64, 64);
    assert!(outcome.success, "{}", outcome.stderr);
    let case = outcome.case.expect("case report");
    assert_eq!(case["status"], 200, "{case}");
    assert!(
        case["body"].as_str().unwrap().contains("RangeError"),
        "{case}"
    );
    assert_eq!(case["restarts"], 0, "{case}");
    assert!(case["near_limit_declined"].as_u64().unwrap() >= 1, "{case}");
    println!("{case}");
}

/// Independent review, finding 1: a refusal with the heap more than half
/// full (35 MiB of small objects at a 64 MiB limit) replaced the isolate.
/// It must be a RangeError.
#[test]
fn a_refusal_with_a_half_full_heap_is_a_range_error_not_a_restart() {
    for retain in [10, 25, 35] {
        let outcome = run_retaining("retain-then-refuse", 64, 64, 16, retain);
        assert!(outcome.success, "{}", outcome.stderr);
        let case = outcome.case.expect("case report");
        assert_eq!(case["status"], 200, "{retain} MiB: {case}");
        assert!(
            case["body"].as_str().unwrap().contains("RangeError"),
            "{retain} MiB: {case}"
        );
        assert_eq!(case["restarts"], 0, "{retain} MiB: {case}");
        println!("{case}");
    }
}
