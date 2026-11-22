//! Framework features the build uses, classified with the capability
//! matrix of `engineering/compatibility.md`. Every classification assumes
//! `nextrs start --js-runtime embedded-v8`, the mode that runs the most;
//! what other flags change is reported with the runtime configuration.

use nextrs_adapter::{BuildInfo, Rendering, RouteKind, Runtime, Severity};

use super::report::{Category, Finding, Level, Support};
use super::{DOCS_COMPAT_GAPS, DOCS_COMPAT_MATRIX, DOCS_COMPAT_UNSUPPORTED};

/// Adapter diagnostics these checks (and the runtime-configuration ones)
/// report themselves; the rest pass through unchanged.
pub const REPORTED_CODES: &[&str] = &[
    "javascript-required",
    "edge-runtime",
    "pages-router",
    "server-actions",
    "edge-server-actions",
    "isr",
    "proxy",
    "middleware",
    "skip-proxy-url-normalize",
    "routing-rules",
    "external-rewrites",
    "i18n",
    "i18n-app-router",
    "remote-images",
    "image-optimizer",
    "image-cache-handler",
];

pub fn listed(items: &[String], limit: usize) -> String {
    let shown = items.len().min(limit);
    let mut text = items[..shown].join(", ");
    if items.len() > shown {
        text.push_str(&format!(" and {} more", items.len() - shown));
    }
    text
}

fn pages(info: &BuildInfo, keep: impl Fn(&nextrs_adapter::Route) -> bool) -> Vec<String> {
    info.routes
        .iter()
        .filter(|r| !r.internal && keep(r))
        .map(|r| r.page.clone())
        .collect()
}

/// `required-server-files.json#config.<key>`: the resolved next.config.
fn next_config(info: &BuildInfo, key: &str) -> Option<serde_json::Value> {
    let roots: Vec<_> = info.dist_dir.canonicalize().into_iter().collect();
    let text = super::fsread::read_contained(
        &info.dist_dir.join("required-server-files.json"),
        &roots,
        super::fsread::LARGE,
    )?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("config")?.get(key).cloned()
}

fn is_set(value: &Option<serde_json::Value>) -> bool {
    match value {
        None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false)) => false,
        Some(serde_json::Value::Object(map)) => !map.is_empty(),
        Some(serde_json::Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

pub fn findings(info: &BuildInfo) -> Vec<Finding> {
    let mut out = Vec::new();
    let diagnostic = |code: &str| info.diagnostics.iter().find(|d| d.code == code);
    let c = Category::Compatibility;

    // Routers.
    if info.routers.app {
        let app = pages(info, |r| matches!(r.kind, RouteKind::AppPage));
        let prerendered = pages(info, |r| {
            r.kind == RouteKind::AppPage && r.rendering == Rendering::Static
        });
        out.push(
            Finding::new(
                "NEXTRS-DOC-201",
                Level::Pass,
                c,
                "App Router detected",
                format!(
                    "{} App Router pages, {} fully prerendered (served from Rust).",
                    app.len(),
                    prerendered.len()
                ),
            )
            .support(Support::Supported)
            .docs(DOCS_COMPAT_MATRIX),
        );
    }
    if info.routers.pages {
        out.push(
            Finding::new(
                "NEXTRS-DOC-202",
                Level::Info,
                c,
                "Pages Router detected (experimental)",
                "Pages without data functions are served from Rust; getServerSideProps, \
                 getStaticProps, API routes and /_next/data run Next's compiled handlers in \
                 embedded V8.",
            )
            .support(Support::Experimental)
            .inspect_code("pages-router")
            .docs(DOCS_COMPAT_MATRIX),
        );
    }

    // Per-request rendering.
    let dynamic = pages(info, |r| {
        matches!(r.kind, RouteKind::AppPage | RouteKind::PagesPage)
            && r.rendering == Rendering::Dynamic
            && r.runtime == Runtime::Nodejs
    });
    if !dynamic.is_empty() {
        out.push(
            Finding::new(
                "NEXTRS-DOC-203",
                Level::Info,
                c,
                format!("{} dynamic SSR pages (experimental)", dynamic.len()),
                format!(
                    "Rendered per request by Next's compiled handlers in embedded V8: {}.",
                    listed(&dynamic, 8)
                ),
            )
            .support(Support::Experimental)
            .docs(DOCS_COMPAT_MATRIX),
        );
    }
    let handlers = pages(info, |r| {
        matches!(r.kind, RouteKind::AppRoute | RouteKind::PagesApi)
            && r.rendering != Rendering::Static
            && r.runtime == Runtime::Nodejs
    });
    if !handlers.is_empty() {
        out.push(
            Finding::new(
                "NEXTRS-DOC-204",
                Level::Info,
                c,
                format!(
                    "{} Route Handlers and API routes (experimental)",
                    handlers.len()
                ),
                format!("Run in embedded V8: {}.", listed(&handlers, 8)),
            )
            .support(Support::Experimental)
            .docs(DOCS_COMPAT_MATRIX),
        );
    }
    // Dynamic routes with prerendered params (generateStaticParams): the
    // listed params are served prerendered, others rendered by the
    // executor; the adapter leaves their rendering open.
    let static_params = pages(info, |r| {
        matches!(r.rendering, Rendering::Unknown { .. })
            && info.prerenders.iter().any(|p| p.route == r.page)
    });
    if !static_params.is_empty() {
        out.push(
            Finding::new(
                "NEXTRS-DOC-223",
                Level::Info,
                c,
                format!(
                    "{} generateStaticParams routes (experimental)",
                    static_params.len()
                ),
                format!(
                    "Listed params are served prerendered; with embedded V8 other params render on \
                     their first request (or answer 404 with dynamicParams = false): {}.",
                    listed(&static_params, 6)
                ),
            )
            .support(Support::Experimental)
            .docs(DOCS_COMPAT_MATRIX),
        );
    }
    let uncertain: Vec<String> = info
        .routes
        .iter()
        .filter(|r| !r.internal && !static_params.contains(&r.page))
        .filter_map(|r| match &r.rendering {
            Rendering::Unknown { reason } => Some(format!("{} ({reason})", r.page)),
            _ => None,
        })
        .collect();
    if !uncertain.is_empty() {
        out.push(Finding::new(
            "NEXTRS-DOC-205",
            Level::Unknown,
            c,
            format!("Rendering of {} routes not determined", uncertain.len()),
            format!(
                "The build metadata does not settle how these render: {}.",
                listed(&uncertain, 5)
            ),
        ));
    }

    // Server Actions.
    if info.server_actions.count > 0 {
        out.push(
            Finding::new(
                "NEXTRS-DOC-206",
                Level::Info,
                c,
                format!(
                    "{} Server Actions detected (experimental)",
                    info.server_actions.count
                ),
                "Next's compiled action handler runs in embedded V8 (Node.js runtime only; not \
                 with interception routes).",
            )
            .support(Support::Experimental)
            .inspect_code("server-actions")
            .docs(DOCS_COMPAT_MATRIX),
        );
    }
    if info.server_actions.edge_count > 0 {
        out.push(
            Finding::new(
                "NEXTRS-DOC-207",
                Level::Error,
                c,
                "Edge-runtime Server Actions",
                format!(
                    "{} Server Actions target the edge runtime, which NextRS does not run; \
                     nextrs start refuses this build.",
                    info.server_actions.edge_count
                ),
            )
            .support(Support::Unsupported)
            .inspect_code("edge-server-actions")
            .suggest("Move these actions to the Node.js runtime (remove runtime = 'edge').")
            .docs(DOCS_COMPAT_UNSUPPORTED),
        );
    }

    // ISR and Cache Components.
    let revalidated: Vec<String> = info
        .prerenders
        .iter()
        .filter(|p| p.revalidate.is_some())
        .map(|p| p.pathname.clone())
        .collect();
    if !revalidated.is_empty() {
        out.push(
            Finding::new(
                "NEXTRS-DOC-208",
                Level::Info,
                c,
                format!(
                    "ISR on {} prerendered paths (experimental)",
                    revalidated.len()
                ),
                format!(
                    "Next's handler regenerates them in embedded V8 and stores them in the shared \
                     cache: {}.",
                    listed(&revalidated, 6)
                ),
            )
            .support(Support::Experimental)
            .inspect_code("isr")
            .docs(DOCS_COMPAT_MATRIX),
        );
    }
    let ppr = info.prerenders.iter().filter(|p| p.postponed).count();
    let cache_components = is_set(&next_config(info, "cacheComponents"));
    if ppr > 0 || cache_components {
        let mut message = String::new();
        if cache_components {
            message.push_str("cacheComponents is enabled. ");
        }
        if ppr > 0 {
            message.push_str(&format!("{ppr} Partial Prerendering shells. "));
        }
        message.push_str(
            "\"use cache\" functions share one cache across isolates and Partial Prerendering \
             shells are completed by Next's handler; \"use cache: private\" is untested.",
        );
        out.push(
            Finding::new(
                "NEXTRS-DOC-209",
                Level::Info,
                c,
                "Cache Components (partial support)",
                message,
            )
            .support(Support::Partial)
            .docs(DOCS_COMPAT_MATRIX),
        );
    }
    if is_set(&next_config(info, "cacheHandler")) || is_set(&next_config(info, "cacheHandlers")) {
        out.push(
            Finding::new(
                "NEXTRS-DOC-210",
                Level::Unknown,
                c,
                "Custom cache handler not tested",
                "next.config sets cacheHandler or cacheHandlers; no NextRS compatibility suite \
                 covers custom handlers.",
            )
            .support(Support::Unknown)
            .suggest("Test caching and revalidation against next start before relying on them.")
            .docs(DOCS_COMPAT_MATRIX),
        );
    }

    // Proxy and middleware.
    if let Some(proxy) = &info.proxy {
        out.push(
            Finding::new(
                "NEXTRS-DOC-211",
                Level::Info,
                c,
                "Node.js proxy (middleware) detected (experimental)",
                format!(
                    "Next's compiled proxy runs in embedded V8 before every matched request ({} \
                     matchers).",
                    proxy.matchers.len()
                ),
            )
            .support(Support::Experimental)
            .component(".next/server/middleware.js")
            .inspect_code("proxy")
            .docs(DOCS_COMPAT_MATRIX),
        );
    }
    if let Some(d) = diagnostic("middleware") {
        out.push(
            Finding::new(
                "NEXTRS-DOC-212",
                Level::Error,
                c,
                "Edge-runtime middleware",
                format!("{}; nextrs start refuses this build.", d.message),
            )
            .support(Support::Unsupported)
            .inspect_code(d.code)
            .suggest(
                "Use the Node.js proxy (proxy.ts, the Next.js 16 default) instead of edge \
                 middleware.",
            )
            .docs(DOCS_COMPAT_UNSUPPORTED),
        );
    }
    if let Some(d) = diagnostic("skip-proxy-url-normalize") {
        out.push(
            Finding::new(
                "NEXTRS-DOC-213",
                Level::Error,
                c,
                "skipProxyUrlNormalize with a proxy",
                format!("{}; nextrs start refuses this build.", d.message),
            )
            .support(Support::Unsupported)
            .inspect_code(d.code)
            .suggest("Remove skipProxyUrlNormalize (skipMiddlewareUrlNormalize) from next.config.")
            .docs(DOCS_COMPAT_UNSUPPORTED),
        );
    }

    // next.config rules.
    if info.routing.custom_count() > 0 {
        let redirects = info
            .routing
            .redirects
            .iter()
            .filter(|r| !r.internal)
            .count();
        let rewrites = info.routing.before_files.len()
            + info.routing.after_files.len()
            + info.routing.fallback.len();
        out.push(
            Finding::new(
                "NEXTRS-DOC-214",
                Level::Info,
                c,
                "next.config headers, redirects and rewrites (experimental)",
                format!(
                    "{} header rules, {redirects} redirects and {rewrites} rewrites, applied in \
                     Rust in next start's order.",
                    info.routing.headers.len() + info.routing.on_match_headers.len()
                ),
            )
            .support(Support::Experimental)
            .inspect_code("routing-rules")
            .docs(DOCS_COMPAT_MATRIX),
        );
    }

    // Images.
    if info.images.as_ref().is_some_and(|i| !i.disabled()) {
        out.push(
            Finding::new(
                "NEXTRS-DOC-215",
                Level::Info,
                c,
                "Image optimization (experimental)",
                "/_next/image is answered in Rust. AVIF/HEIF and JPEG-compressed TIFF sources are \
                 served unchanged, and PNG output is lossless.",
            )
            .support(Support::Experimental)
            .docs(DOCS_COMPAT_GAPS),
        );
    }
    if let Some(d) = diagnostic("image-cache-handler") {
        out.push(
            Finding::new(
                "NEXTRS-DOC-216",
                Level::Warning,
                c,
                "images.customCacheHandler is ignored",
                d.message.clone(),
            )
            .support(Support::Unsupported)
            .inspect_code(d.code)
            .suggest("Use --image-cache-dir for a persistent image cache.")
            .docs(DOCS_COMPAT_GAPS),
        );
    }

    // Edge runtime and interception routes.
    let edge = pages(info, |r| r.runtime == Runtime::Edge);
    if !edge.is_empty() {
        out.push(
            Finding::new(
                "NEXTRS-DOC-217",
                Level::Error,
                c,
                format!("{} edge-runtime routes", edge.len()),
                format!(
                    "NextRS does not run the edge runtime; these routes answer 501: {}.",
                    listed(&edge, 8)
                ),
            )
            .support(Support::Unsupported)
            .inspect_code("edge-runtime")
            .suggest("Remove export const runtime = 'edge' so they build for the Node.js runtime.")
            .docs(DOCS_COMPAT_UNSUPPORTED),
        );
    }
    if info.has_interception_routes() {
        let intercepting = pages(info, |r| r.page.contains("/(."));
        out.push(
            Finding::new(
                "NEXTRS-DOC-218",
                // Only executor-served routes answer 501: a build that runs
                // no JavaScript is served as before.
                if super::needs_javascript(info) {
                    Level::Error
                } else {
                    Level::Warning
                },
                c,
                "Interception routes",
                format!(
                    "The executor does not support builds with interception routes ({}): their \
                     dynamic routes answer 501 and Server Actions are not routed. Prerendered \
                     pages are still served.",
                    listed(&intercepting, 5)
                ),
            )
            .support(Support::Unsupported)
            .docs(DOCS_COMPAT_UNSUPPORTED),
        );
    }

    // i18n.
    if let Some(d) = diagnostic("i18n-app-router") {
        out.push(
            Finding::new(
                "NEXTRS-DOC-219",
                Level::Error,
                c,
                "Next's i18n config with App Router routes",
                format!("{}; nextrs start refuses this build.", d.message),
            )
            .support(Support::Unsupported)
            .inspect_code(d.code)
            .suggest("Use next-intl's middleware for App Router locale routing (no i18n config).")
            .docs(DOCS_COMPAT_UNSUPPORTED),
        );
    }
    if let Some(d) = diagnostic("i18n") {
        out.push(if d.severity == Severity::Unsupported {
            Finding::new(
                "NEXTRS-DOC-219",
                Level::Error,
                c,
                "i18n config NextRS cannot read",
                format!("{}; nextrs start refuses this build.", d.message),
            )
            .support(Support::Unsupported)
            .inspect_code(d.code)
            .docs(DOCS_COMPAT_UNSUPPORTED)
        } else {
            Finding::new(
                "NEXTRS-DOC-219",
                Level::Info,
                c,
                "Pages Router i18n (experimental)",
                d.message.clone(),
            )
            .support(Support::Experimental)
            .inspect_code(d.code)
            .docs(DOCS_COMPAT_MATRIX)
        });
    }

    // instrumentation.ts.
    if info.dist_dir.join("server/instrumentation.js").is_file() {
        out.push(
            Finding::new(
                "NEXTRS-DOC-222",
                Level::Info,
                c,
                "instrumentation.ts detected (experimental)",
                "register() runs once per isolate: N times with --isolates N.",
            )
            .support(Support::Experimental)
            .component(".next/server/instrumentation.js")
            .docs(DOCS_COMPAT_MATRIX),
        );
    }

    let blockers = out.iter().any(|f| f.level == Level::Error)
        || info
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Unsupported && !REPORTED_CODES.contains(&d.code));
    if !blockers {
        out.push(
            Finding::new(
                "NEXTRS-DOC-200",
                Level::Pass,
                c,
                "No known unsupported route features",
                "Nothing in the build metadata matches a feature NextRS refuses or answers 501 \
                 for. This is not proof of compatibility: only the routes NextRS's fixtures and \
                 certification applications use have been verified.",
            )
            .docs(DOCS_COMPAT_GAPS),
        );
    }
    out
}
