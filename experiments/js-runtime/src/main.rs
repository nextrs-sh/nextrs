//! Phase 0 JavaScript engine experiment: embeds V8 (via deno_core) and
//! QuickJS (via rquickjs) in a Rust process and runs the same JS drivers in
//! both. Prints one JSON line per run.
//!
//!   js-experiment <v8|qjs> hello
//!   js-experiment <v8|qjs> async
//!   js-experiment <v8|qjs> module
//!   js-experiment <v8|qjs> react
//!   js-experiment <v8|qjs> rsc
//!   js-experiment <v8|qjs> next <project-dir> <entry> <level 0-3>
//!
//! Experimental code: no sandboxing beyond what each engine provides; the
//! `next` test reads arbitrary files below the given project directory.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail};
use serde_json::{Value, json};

static REPORTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn report(value: String) {
    REPORTS.lock().unwrap().push(value);
}

/// What one test needs: scripts to evaluate in order, then either a call
/// expression or an ES module to run.
struct Plan {
    scripts: Vec<PathBuf>,
    call: Option<String>,
    module: Option<PathBuf>,
}

fn plan(js: &Path, test: &str, args: &[String]) -> anyhow::Result<Plan> {
    let drivers = js.join("drivers/tests.js");
    let call = |name: &str, args: Value| Some(format!("__runTest({name:?}, {args})"));
    Ok(match test {
        "hello" | "async" => Plan {
            scripts: vec![drivers],
            call: call(test, json!([])),
            module: None,
        },
        "module" => Plan {
            scripts: vec![],
            call: None,
            module: Some(js.join("modules/main.mjs")),
        },
        "react" => Plan {
            scripts: vec![js.join("dist/react-ssr.js"), drivers],
            call: call("react", json!([])),
            module: None,
        },
        "rsc" => Plan {
            scripts: vec![js.join("dist/rsc-flight.js"), drivers],
            call: call("rsc", json!([])),
            module: None,
        },
        "next" => {
            let [project, entry, level] = args else {
                bail!("usage: next <project-dir> <entry> <level>");
            };
            let project = std::fs::canonicalize(project)?;
            let level: u8 = level.parse()?;
            Plan {
                scripts: vec![js.join("drivers/next-host.js"), drivers],
                call: call("next", json!([project.to_string_lossy(), entry, level])),
                module: None,
            }
        }
        other => bail!("unknown test {other}"),
    })
}

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
        .map_or(0, |pages| pages * 4)
}

struct Measured {
    init: Duration,
    engine_heap_bytes: usize,
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [engine, test, rest @ ..] = args.as_slice() else {
        bail!("usage: js-experiment <v8|qjs> <test> [args]");
    };
    let js = Path::new(env!("CARGO_MANIFEST_DIR")).join("js");
    let plan = plan(&js, test, rest)?;

    let rss_before = rss_kib();
    let started = Instant::now();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let measured = runtime.block_on(async {
        match engine.as_str() {
            "v8" => v8_engine::run(&plan).await,
            "qjs" => qjs_engine::run(&plan).await,
            other => Err(anyhow!("unknown engine {other}")),
        }
    });
    let total = started.elapsed();
    let reports = std::mem::take(&mut *REPORTS.lock().unwrap());

    let (status, result) = match (&measured, reports.last()) {
        (Ok(_), Some(last)) => ("completed", serde_json::from_str(last)?),
        (Ok(_), None) => ("no-report", Value::Null),
        (Err(error), _) => (
            "engine-error",
            json!({ "ok": false, "error": format!("{error:#}") }),
        ),
    };
    let line = json!({
        "engine": engine,
        "test": test,
        "status": status,
        "initMs": measured.as_ref().map(|m| m.init.as_secs_f64() * 1000.0).ok(),
        "totalMs": total.as_secs_f64() * 1000.0,
        "engineHeapBytes": measured.as_ref().map(|m| m.engine_heap_bytes).ok(),
        "rssKiBBefore": rss_before,
        "rssKiBAfter": rss_kib(),
        "result": result,
    });
    println!("{line}");
    Ok(())
}

mod v8_engine {
    use std::rc::Rc;
    use std::time::Instant;

    use deno_core::{
        FsModuleLoader, JsRuntime, ModuleSpecifier, PollEventLoopOptions, RuntimeOptions,
        extension, op2,
    };

    use super::{Measured, Plan};

    #[op2(fast)]
    fn op_report(#[string] value: String) {
        super::report(value);
    }

    #[op2]
    #[string]
    fn op_read_text(#[string] path: String) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    #[op2(fast)]
    fn op_is_file(#[string] path: String) -> bool {
        std::path::Path::new(&path).is_file()
    }

    #[op2(fast)]
    fn op_is_dir(#[string] path: String) -> bool {
        std::path::Path::new(&path).is_dir()
    }

    extension!(
        nextrs_experiment,
        ops = [op_report, op_read_text, op_is_file, op_is_dir]
    );

    /// deno_core ships V8, microtasks, and timer primitives, but no
    /// setTimeout global, console, TextEncoder, streams, process or require.
    const PRELUDE: &str = r#"
((core) => {
  const print = (stream) => (...args) => core.print(args.map(String).join(" ") + "\n", stream);
  globalThis.console = {
    log: print(false), info: print(false), debug: print(false),
    warn: print(true), error: print(true), trace: print(true),
    time() {}, timeEnd() {}, group() {}, groupEnd() {}, dir: print(false), table: print(false),
  };
  globalThis.__report = (value) => core.ops.op_report(value);
  globalThis.setTimeout = (cb, ms = 0, ...args) => core.createTimer(cb, ms, args, false, true);
  globalThis.setInterval = (cb, ms = 0, ...args) => core.createTimer(cb, ms, args, true, true);
  globalThis.clearTimeout = (id) => id !== undefined && core.cancelTimer(id);
  globalThis.clearInterval = globalThis.clearTimeout;
  globalThis.__host = {
    readText: (path) => core.ops.op_read_text(path),
    isFile: (path) => core.ops.op_is_file(path),
    isDir: (path) => core.ops.op_is_dir(path),
  };
})(Deno.core);
"#;

    pub async fn run(plan: &Plan) -> anyhow::Result<Measured> {
        let started = Instant::now();
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![nextrs_experiment::init()],
            module_loader: Some(Rc::new(FsModuleLoader)),
            ..Default::default()
        });
        let init = started.elapsed();
        runtime.execute_script("[nextrs:prelude]", PRELUDE)?;
        for script in &plan.scripts {
            let code = std::fs::read_to_string(script)?;
            let name = script.to_string_lossy().into_owned();
            runtime.execute_script(name, code)?;
        }
        if let Some(module) = &plan.module {
            let specifier = ModuleSpecifier::from_file_path(module)
                .map_err(|()| anyhow::anyhow!("bad module path"))?;
            let id = runtime.load_main_es_module(&specifier).await?;
            let evaluated = runtime.mod_evaluate(id);
            runtime
                .run_event_loop(PollEventLoopOptions::default())
                .await?;
            evaluated.await?;
        }
        if let Some(call) = &plan.call {
            runtime.execute_script("[nextrs:call]", call.clone())?;
            runtime
                .run_event_loop(PollEventLoopOptions::default())
                .await?;
        }
        let heap = runtime.v8_isolate().get_heap_statistics();
        Ok(Measured {
            init,
            engine_heap_bytes: heap.used_heap_size(),
        })
    }
}

mod qjs_engine {
    use std::cell::{Cell, RefCell};
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    use anyhow::anyhow;
    use rquickjs::loader::{FileResolver, ScriptLoader};
    use rquickjs::prelude::Opt;
    use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Function, Module};

    use super::{Measured, Plan};

    const PRELUDE: &str = r#"
globalThis.queueMicrotask ??= (cb) => { Promise.resolve().then(cb); };
globalThis.console = {
  log: (...a) => __print(a.map(String).join(" ")), info: (...a) => __print(a.map(String).join(" ")),
  debug() {}, warn: (...a) => __print(a.map(String).join(" ")), error: (...a) => __print(a.map(String).join(" ")),
  trace() {}, time() {}, timeEnd() {}, group() {}, groupEnd() {}, dir() {}, table() {},
};
globalThis.setInterval = () => { throw new Error("setInterval not provided in the QuickJS host"); };
globalThis.clearInterval = globalThis.clearTimeout;
"#;

    fn install(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
        let globals = ctx.globals();
        globals.set("__report", Function::new(ctx.clone(), super::report)?)?;
        globals.set(
            "__print",
            Function::new(ctx.clone(), |s: String| eprintln!("{s}"))?,
        )?;
        let host = rquickjs::Object::new(ctx.clone())?;
        host.set(
            "readText",
            Function::new(ctx.clone(), |path: String| {
                std::fs::read_to_string(path).ok()
            })?,
        )?;
        host.set(
            "isFile",
            Function::new(ctx.clone(), |path: String| {
                std::path::Path::new(&path).is_file()
            })?,
        )?;
        host.set(
            "isDir",
            Function::new(ctx.clone(), |path: String| {
                std::path::Path::new(&path).is_dir()
            })?,
        )?;
        globals.set("__host", host)?;

        globals.set("setTimeout", Function::new(ctx.clone(), set_timeout)?)?;
        globals.set(
            "clearTimeout",
            Function::new(ctx.clone(), |id: Opt<u32>| {
                if let Some(id) = id.0 {
                    CANCELLED.with(|c| c.borrow_mut().insert(id));
                }
            })?,
        )?;
        ctx.eval::<(), _>(PRELUDE)?;
        Ok(())
    }

    thread_local! {
        static NEXT_TIMER: Cell<u32> = const { Cell::new(0) };
        static CANCELLED: RefCell<HashSet<u32>> = RefCell::default();
    }

    /// One spawned future per timer on the runtime's scheduler.
    fn set_timeout<'js>(ctx: Ctx<'js>, callback: Function<'js>, ms: Opt<f64>) -> u32 {
        let id = NEXT_TIMER.with(|n| {
            n.set(n.get() + 1);
            n.get()
        });
        let delay = Duration::from_secs_f64(ms.0.unwrap_or(0.0).max(0.0) / 1000.0);
        ctx.spawn(async move {
            tokio::time::sleep(delay).await;
            if !CANCELLED.with(|c| c.borrow_mut().remove(&id)) {
                let _ = callback.call::<_, ()>(());
            }
        });
        id
    }

    fn describe(ctx: &Ctx<'_>, result: rquickjs::Result<()>) -> anyhow::Result<()> {
        result.catch(ctx).map_err(|e| anyhow!("{e}"))
    }

    pub async fn run(plan: &Plan) -> anyhow::Result<Measured> {
        let started = Instant::now();
        let runtime = AsyncRuntime::new()?;
        runtime.set_max_stack_size(8 * 1024 * 1024).await;
        // rquickjs' FileResolver treats search paths as relative to the
        // working directory, so the module test runs from the module's dir.
        if let Some(dir) = plan.module.as_ref().and_then(|m| m.parent()) {
            std::env::set_current_dir(dir)?;
        }
        runtime
            .set_loader(
                FileResolver::default()
                    .with_path(".")
                    .with_pattern("{}.mjs"),
                ScriptLoader::default().with_extension("mjs"),
            )
            .await;
        let context = AsyncContext::full(&runtime).await?;
        let init = started.elapsed();

        let scripts: Vec<String> = plan
            .scripts
            .iter()
            .map(std::fs::read_to_string)
            .collect::<Result<_, _>>()?;
        let module = match &plan.module {
            Some(path) => Some((
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                std::fs::read_to_string(path)?,
            )),
            None => None,
        };
        let call = plan.call.clone();

        context
            .async_with(async |ctx| {
                describe(&ctx, install(&ctx))?;
                for code in scripts {
                    describe(&ctx, ctx.eval::<(), _>(code))?;
                }
                if let Some((name, source)) = module {
                    let promise = Module::evaluate(ctx.clone(), name, source);
                    match promise {
                        Ok(promise) => describe(&ctx, promise.into_future::<()>().await)?,
                        Err(e) => describe(&ctx, Err(e))?,
                    }
                }
                if let Some(call) = call {
                    describe(&ctx, ctx.eval::<(), _>(call))?;
                }
                Ok::<_, anyhow::Error>(())
            })
            .await?;
        runtime.idle().await;
        let usage = runtime.memory_usage().await;
        Ok(Measured {
            init,
            engine_heap_bytes: usage.memory_used_size as usize,
        })
    }
}
