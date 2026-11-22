//! Node.js APIs NextRS does not provide, judged from evidence `next build`
//! itself produced: the file traces (`*.nft.json`) Next writes for every
//! server entry. A trace lists what Next's tracer found reachable from the
//! entry's server code (client-only imports, type imports and
//! tree-shaken code are not in it), so a package or native addon listed
//! there is part of that route's server runtime. Nothing is executed and no
//! source text is scanned.
//!
//! Next's own server trace (`next-server.js.nft.json`) is not read: it
//! describes `next start`, which NextRS replaces (sharp, for example, is
//! traced there for Next's image optimizer).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use nextrs_adapter::BuildInfo;

use super::DOCS_COMPAT_GAPS;
use super::compat::listed;
use super::report::{Category, Finding, Level, Support};

/// Packages whose purpose is a TCP connection (database, cache, mail and
/// message-queue clients). NextRS has no `node:net`/`node:tls` sockets, so
/// they cannot connect; HTTP-based clients over `fetch` work.
pub const SOCKET_PACKAGES: &[&str] = &[
    "pg",
    "postgres",
    "mysql",
    "mysql2",
    "mariadb",
    "mongodb",
    "mongoose",
    "ioredis",
    "redis",
    "@redis/client",
    "memcached",
    "nodemailer",
    "tedious",
    "mssql",
    "oracledb",
    "cassandra-driver",
    "amqplib",
    "kafkajs",
    "mqtt",
    "nats",
];

/// Packages whose purpose is starting other processes (headless browsers).
/// NextRS has no `node:child_process`.
pub const PROCESS_PACKAGES: &[&str] = &[
    "puppeteer",
    "puppeteer-core",
    "playwright",
    "playwright-core",
    "@sparticuz/chromium",
];

/// Server entries whose traces are read, relative to `.next/server`.
fn trace_files(server_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for top in ["app", "pages"] {
        collect(&server_dir.join(top), &mut files);
    }
    for entry in ["middleware.js.nft.json", "instrumentation.js.nft.json"] {
        let path = server_dir.join(entry);
        if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    files
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Not following symlinks: a trace is a regular file next build wrote.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect(&path, out);
        } else if kind.is_file() && path.to_string_lossy().ends_with(".nft.json") {
            out.push(path);
        }
    }
}

/// The package a traced path belongs to: the last `node_modules/<name>` (or
/// `node_modules/@scope/<name>`) in it.
pub fn package_of(path: &str) -> Option<String> {
    let rest = &path[path.rfind("node_modules/")? + "node_modules/".len()..];
    let mut parts = rest.split('/');
    let first = parts.next().filter(|p| !p.is_empty())?;
    if first.starts_with('@') {
        let second = parts.next().filter(|p| !p.is_empty())?;
        Some(format!("{first}/{second}"))
    } else {
        Some(first.to_owned())
    }
}

#[derive(Default)]
pub struct TraceEvidence {
    /// Server entries read.
    pub traces: usize,
    /// Traces that could not be parsed.
    pub unreadable: Vec<String>,
    /// Native addon (`.node`) package -> entries that trace it.
    pub native: BTreeMap<String, BTreeSet<String>>,
    pub sockets: BTreeMap<String, BTreeSet<String>>,
    pub processes: BTreeMap<String, BTreeSet<String>>,
}

pub fn analyze(dist_dir: &Path) -> TraceEvidence {
    let server_dir = dist_dir.join("server");
    // Traces are files next build wrote inside .next: never read one that
    // resolves elsewhere.
    let roots: Vec<_> = dist_dir.canonicalize().into_iter().collect();
    let mut evidence = TraceEvidence::default();
    for file in trace_files(&server_dir) {
        let entry = file
            .strip_prefix(&server_dir)
            .unwrap_or(&file)
            .to_string_lossy()
            .trim_end_matches(".nft.json")
            .to_owned();
        let parsed = super::fsread::read_contained(&file, &roots, super::fsread::LARGE)
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        let Some(files) = parsed
            .as_ref()
            .and_then(|v| v.get("files"))
            .and_then(|f| f.as_array())
        else {
            evidence.unreadable.push(entry);
            continue;
        };
        evidence.traces += 1;
        for path in files.iter().filter_map(|f| f.as_str()) {
            let Some(package) = package_of(path) else {
                continue;
            };
            if path.ends_with(".node") {
                evidence
                    .native
                    .entry(package.clone())
                    .or_default()
                    .insert(entry.clone());
            }
            if SOCKET_PACKAGES.contains(&package.as_str()) {
                evidence
                    .sockets
                    .entry(package.clone())
                    .or_default()
                    .insert(entry.clone());
            }
            if PROCESS_PACKAGES.contains(&package.as_str()) {
                evidence
                    .processes
                    .entry(package)
                    .or_default()
                    .insert(entry.clone());
            }
        }
    }
    evidence
}

fn usage(found: &BTreeMap<String, BTreeSet<String>>) -> String {
    let items: Vec<String> = found
        .iter()
        .map(|(package, entries)| {
            let entries: Vec<String> = entries.iter().cloned().collect();
            format!("{package} ({})", listed(&entries, 3))
        })
        .collect();
    listed(&items, 6)
}

pub fn findings(info: &BuildInfo) -> Vec<Finding> {
    let n = Category::NodeApis;
    let evidence = analyze(&info.dist_dir);
    let mut out = Vec::new();
    if !evidence.native.is_empty() {
        out.push(
            Finding::new(
                "NEXTRS-DOC-301",
                Level::Warning,
                n,
                "Native Node.js addons in server code",
                format!(
                    "Next's file traces include native addons (.node files): {}. NextRS cannot \
                     load native addons; requiring one throws NextRSUnsupportedError. An optional \
                     addon loaded inside try/catch falls back, a required one fails the route.",
                    usage(&evidence.native)
                ),
            )
            .support(Support::Unsupported)
            .suggest(
                "Replace the package with a pure-JavaScript or WebAssembly alternative, or check \
                 that the route works without it under nextrs start.",
            )
            .docs(DOCS_COMPAT_GAPS),
        );
    }
    if !evidence.sockets.is_empty() {
        out.push(
            Finding::new(
                "NEXTRS-DOC-302",
                Level::Warning,
                n,
                "TCP socket clients in server code",
                format!(
                    "Next's file traces include clients that connect over TCP: {}. NextRS has no \
                     node:net or node:tls sockets, so they cannot connect.",
                    usage(&evidence.sockets)
                ),
            )
            .support(Support::Unsupported)
            .suggest(
                "Use an HTTP-based client over fetch (allowed with --net-allow), or move the \
                 connection behind an HTTP API.",
            )
            .docs(DOCS_COMPAT_GAPS),
        );
    }
    if !evidence.processes.is_empty() {
        out.push(
            Finding::new(
                "NEXTRS-DOC-303",
                Level::Warning,
                n,
                "Process-spawning packages in server code",
                format!(
                    "Next's file traces include packages that start other processes: {}. NextRS \
                     has no node:child_process.",
                    usage(&evidence.processes)
                ),
            )
            .support(Support::Unsupported)
            .suggest("Move browser automation or process work to a separate service.")
            .docs(DOCS_COMPAT_GAPS),
        );
    }
    if !evidence.unreadable.is_empty() {
        out.push(Finding::new(
            "NEXTRS-DOC-304",
            Level::Unknown,
            n,
            format!("{} file traces unreadable", evidence.unreadable.len()),
            format!("Not analyzed: {}.", listed(&evidence.unreadable, 5)),
        ));
    }
    if evidence.traces == 0 && evidence.unreadable.is_empty() {
        out.push(Finding::new(
            "NEXTRS-DOC-305",
            Level::Unknown,
            n,
            "No server file traces",
            "The build has no .nft.json traces for its server entries (a fully static build, or \
             one copied without them), so native addons and socket clients were not checked.",
        ));
    } else if out.is_empty() {
        out.push(Finding::new(
            "NEXTRS-DOC-306",
            Level::Pass,
            n,
            "No native addons or socket clients traced",
            format!(
                "{} server entry traces checked for native addons, TCP clients and \
                 process-spawning packages.",
                evidence.traces
            ),
        ));
    }
    // Always: what the traces cannot show.
    out.push(
        Finding::new(
            "NEXTRS-DOC-307",
            Level::Unknown,
            n,
            "Node.js API usage not fully analyzed",
            "NextRS provides a narrow Node.js layer: http, https, net, dns, child_process, \
             worker_threads, vm, zlib streams and native addons are unavailable, and node:crypto \
             is partial. Doctor does not analyze which built-in modules or APIs the server code \
             calls; unsupported calls throw NextRSUnsupportedError at runtime.",
        )
        .support(Support::Unknown)
        .suggest(
            "Exercise every server route under nextrs start --js-runtime embedded-v8 and look for \
             NextRSUnsupportedError in the logs.",
        )
        .docs(DOCS_COMPAT_GAPS),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_are_named_from_the_last_node_modules() {
        assert_eq!(
            package_of("../../node_modules/pg/lib/index.js").as_deref(),
            Some("pg")
        );
        assert_eq!(
            package_of("../node_modules/@redis/client/dist/index.js").as_deref(),
            Some("@redis/client")
        );
        assert_eq!(
            package_of("../node_modules/.pnpm/pg@8/node_modules/pg/lib/x.js").as_deref(),
            Some("pg")
        );
        assert_eq!(package_of("../chunks/ssr/app.js"), None);
        assert_eq!(package_of("node_modules/@scope"), None);
    }
}
