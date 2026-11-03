//! JavaScriptCore spike: embeds WebKitGTK's JavaScriptCore through its public
//! C API and runs the same engine-agnostic drivers as the Phase 0 V8/QuickJS
//! experiment (`experiments/js-runtime/js`). Prints one JSON line per run.
//!
//!   jsc-experiment hello | async | react | rsc
//!   jsc-experiment next <project-dir> <entry> <level 0-4>
//!   jsc-experiment probe            # async-context and V8-only API probes
//!
//! Experimental code: the `next` test reads arbitrary files below the
//! project directory. `unsafe` is confined to the FFI module.

mod jsc;

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
        .map_or(0, |pages| pages * 4)
}

const PROBE: &str = r#"
(() => {
  const out = {};
  out.asyncContextProposal = typeof AsyncContext !== "undefined";
  out.errorCaptureStackTrace = typeof Error.captureStackTrace === "function";
  out.prepareStackTraceCallSites = (() => {
    const original = Error.prepareStackTrace;
    let callSites = false;
    Error.prepareStackTrace = (_e, frames) => { callSites = Array.isArray(frames); return "x"; };
    String(new Error("probe").stack);
    Error.prepareStackTrace = original;
    return callSites;
  })();
  out.queueMicrotask = typeof queueMicrotask === "function";
  out.weakRef = typeof WeakRef === "function";
  out.finalizationRegistry = typeof FinalizationRegistry === "function";
  out.structuredClone = typeof structuredClone === "function";
  return JSON.stringify(out);
})()
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let test = args.first().map(String::as_str).unwrap_or("hello");
    let js = Path::new(env!("CARGO_MANIFEST_DIR")).join("../js-runtime/js");

    let rss_before = rss_kib();
    let started = Instant::now();
    let mut engine = jsc::Engine::new();
    let init = started.elapsed();
    engine.eval("[nextrs:prelude]", jsc::PRELUDE)?;

    let mut scripts: Vec<PathBuf> = Vec::new();
    let call = match test {
        "hello" | "async" => {
            scripts.push(js.join("drivers/tests.js"));
            format!("__runTest({test:?}, [])")
        }
        "react" => {
            scripts.extend([js.join("dist/react-ssr.js"), js.join("drivers/tests.js")]);
            "__runTest(\"react\", [])".to_owned()
        }
        "rsc" => {
            scripts.extend([js.join("dist/rsc-flight.js"), js.join("drivers/tests.js")]);
            "__runTest(\"rsc\", [])".to_owned()
        }
        "next" => {
            let [_, project, entry, level] = args.as_slice() else {
                return Err("usage: next <project-dir> <entry> <level>".into());
            };
            let project = std::fs::canonicalize(project)?;
            scripts.extend([js.join("drivers/next-host.js"), js.join("drivers/tests.js")]);
            format!(
                "__runTest(\"next\", {})",
                json!([project.to_string_lossy(), entry, level.parse::<u8>()?])
            )
        }
        "probe" => {
            let value = engine.eval("[nextrs:probe]", PROBE)?;
            jsc::report(value);
            String::new()
        }
        other => return Err(format!("unknown test {other}").into()),
    };
    let outcome = (|| -> Result<(), String> {
        for script in &scripts {
            let source = std::fs::read_to_string(script).map_err(|e| e.to_string())?;
            engine.eval(&script.to_string_lossy(), &source)?;
        }
        if !call.is_empty() {
            engine.eval("[nextrs:call]", &call)?;
        }
        engine.run_event_loop()
    })();
    let total = started.elapsed();
    let reports = jsc::take_reports();
    let result: Value = match (&outcome, reports.last()) {
        (Ok(()), Some(last)) => serde_json::from_str(last).unwrap_or(Value::String(last.clone())),
        (Ok(()), None) => Value::Null,
        (Err(error), _) => json!({ "ok": false, "error": error }),
    };
    println!(
        "{}",
        json!({
            "engine": "javascriptcore",
            "version": jsc::VERSION,
            "test": test,
            "initMs": init.as_secs_f64() * 1000.0,
            "totalMs": total.as_secs_f64() * 1000.0,
            "rssKiBBefore": rss_before,
            "rssKiBAfter": rss_kib(),
            "result": result,
        })
    );
    drop(engine);
    Ok(())
}
