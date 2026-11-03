//! Renders one request through the embedded executor and prints the result.
//!
//!   cargo run -p nextrs-runtime --example render -- <project-dir> <entry> <url> [header:value ...]
//!
//! `<entry>` is relative to `<project>/.next/server`, e.g. `app/dynamic/page.js`.

use std::path::PathBuf;
use std::time::Instant;

use nextrs_runtime::{Executor, ExecutorConfig, RenderRequest, ResponseEvent};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber_init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [project, entry, url, headers @ ..] = args.as_slice() else {
        return Err("usage: render <project-dir> <entry> <url> [name:value ...]".into());
    };
    let project = std::fs::canonicalize(project)?;
    let dist = project.join(".next");
    let started = Instant::now();
    let mut config = ExecutorConfig::new(project.clone(), dist.clone());
    config.trace_node_api = std::env::var_os("NEXTRS_TRACE_NODE_API").is_some();
    let executor = Executor::start(config)?;
    eprintln!("[render] executor ready in {:?}", started.elapsed());
    let headers = headers
        .iter()
        .filter_map(|h| h.split_once(':'))
        .map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned()))
        .chain([("host".to_owned(), "127.0.0.1".to_owned())])
        .collect();
    let request = RenderRequest {
        method: "GET".into(),
        url: url.clone(),
        headers,
        body: Vec::new(),
        entry: PathBuf::from(&dist).join("server").join(entry),
        remote_address: Some("127.0.0.1".into()),
        initial_headers: Vec::new(),
    };
    let rendered = Instant::now();
    let mut events = executor.render(request)?;
    let mut body = Vec::new();
    let mut chunks = 0;
    while let Some(event) = events.blocking_recv() {
        match event {
            ResponseEvent::Head { status, headers } => {
                eprintln!("[render] status {status} after {:?}", rendered.elapsed());
                for (name, value) in headers {
                    eprintln!("[render]   {name}: {value}");
                }
            }
            ResponseEvent::Chunk(chunk) => {
                chunks += 1;
                eprintln!(
                    "[render] chunk {chunks}: {} bytes at {:?}",
                    chunk.len(),
                    rendered.elapsed()
                );
                body.extend_from_slice(&chunk);
            }
            ResponseEvent::End => break,
            ResponseEvent::Error(error) => {
                eprintln!("[render] error: {error}");
                break;
            }
        }
    }
    eprintln!(
        "[render] {} bytes in {chunks} chunks, {:?}",
        body.len(),
        rendered.elapsed()
    );
    print!("{}", String::from_utf8_lossy(&body));
    Ok(())
}

fn tracing_subscriber_init() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nextrs=info".into()),
        )
        .with_writer(std::io::stderr)
        .try_init();
}
