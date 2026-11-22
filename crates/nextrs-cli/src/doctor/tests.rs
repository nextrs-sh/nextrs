//! Doctor against the redacted `next build` snapshots in `tests/data`
//! (Next.js 16.4.3 and 16.4.6), copied and altered per scenario.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::Parser;

use super::report::{Level, Report, Status};
use super::{DoctorArgs, EnvVar, diagnose, render};

fn snapshot(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data")
        .join(name)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh, writable directory (a copy of `snapshot` when given).
fn scratch(snapshot_name: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nextrs-doctor-unit-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    match snapshot_name {
        Some(name) => copy_dir(&snapshot(name), &dir),
        None => std::fs::create_dir_all(&dir).unwrap(),
    }
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A project as a developer has it: package.json and node_modules/next.
fn installed(dir: &Path, version: &str) {
    write(
        &dir.join("package.json"),
        &format!(r#"{{"name":"app","dependencies":{{"next":"{version}"}}}}"#),
    );
    write(
        &dir.join("node_modules/next/package.json"),
        &format!(r#"{{"name":"next","version":"{version}"}}"#),
    );
}

fn set_build_version(dir: &Path, version: &str) {
    write(
        &dir.join(".next/diagnostics/framework.json"),
        &format!(r#"{{"name":"Next.js","version":"{version}"}}"#),
    );
}

fn args(dir: &Path, extra: &[&str]) -> DoctorArgs {
    let mut argv = vec!["nextrs", "doctor", dir.to_str().unwrap()];
    argv.extend_from_slice(extra);
    match crate::Cli::try_parse_from(argv).unwrap().command {
        crate::Command::Doctor(args) => args,
        _ => unreachable!(),
    }
}

fn run(dir: &Path, extra: &[&str]) -> Report {
    diagnose(&args(dir, extra), &|_| EnvVar::Unset).unwrap()
}

fn level(report: &Report, code: &str) -> Option<Level> {
    report
        .findings
        .iter()
        .find(|f| f.code == code)
        .map(|f| f.level)
}

fn errors(report: &Report) -> Vec<String> {
    report
        .findings
        .iter()
        .filter(|f| f.level == Level::Error)
        .map(|f| format!("{} {}: {}", f.code, f.title, f.message))
        .collect()
}

#[test]
fn a_supported_build_has_no_blockers() {
    let dir = scratch(Some("next-16.4.6-static-params"));
    installed(&dir, "16.4.6");
    let report = run(&dir, &[]);
    assert_eq!(errors(&report), Vec::<String>::new());
    assert_eq!(report.exit_code, 0);
    assert!(report.complete);
    // Off the release platform (a developer's macOS), the only warnings are
    // the platform's own.
    if cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "gnu"
    )) {
        assert_eq!(report.status, Status::Ok, "{:#?}", report.findings);
    } else {
        let warnings: Vec<&str> = report
            .findings
            .iter()
            .filter(|f| f.level == Level::Warning)
            .map(|f| f.code)
            .collect();
        assert!(
            warnings
                .iter()
                .all(|c| ["NEXTRS-DOC-001", "NEXTRS-DOC-002"].contains(c)),
            "{warnings:?}"
        );
    }
    for code in [
        "NEXTRS-DOC-004",
        "NEXTRS-DOC-005",
        "NEXTRS-DOC-103",
        "NEXTRS-DOC-104",
        "NEXTRS-DOC-107",
        "NEXTRS-DOC-200",
        "NEXTRS-DOC-201",
    ] {
        assert_eq!(level(&report, code), Some(Level::Pass), "{code}");
    }
    // Static analysis never claims full compatibility.
    assert_eq!(level(&report, "NEXTRS-DOC-307"), Some(Level::Unknown));
    assert_eq!(report.project.app_router, Some(true));
    assert_eq!(report.environment.next_build.as_deref(), Some("16.4.6"));
}

#[test]
fn a_missing_build_is_an_incomplete_assessment() {
    let dir = scratch(None);
    write(
        &dir.join("package.json"),
        r#"{"name":"app","dependencies":{"next":"^16.4.6"}}"#,
    );
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-101"), Some(Level::Warning));
    assert_eq!(level(&report, "NEXTRS-DOC-004"), Some(Level::Pass));
    assert_eq!(level(&report, "NEXTRS-DOC-005"), Some(Level::Unknown));
    assert_eq!(report.status, Status::Incomplete);
    assert!(!report.complete);
    assert_eq!(report.exit_code, 0);
    // Environment checks still ran.
    assert!(level(&report, "NEXTRS-DOC-001").is_some());
    assert_eq!(run(&dir, &["--strict"]).exit_code, 1);
}

#[test]
fn an_unsupported_installed_major_without_a_build_is_a_blocker() {
    let dir = scratch(None);
    installed(&dir, "15.5.12");
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-005"), Some(Level::Error));
    assert_eq!(report.status, Status::Blocked);
    assert_eq!(report.exit_code, 1);
}

#[test]
fn corrupted_build_metadata_is_a_blocker() {
    let dir = scratch(Some("next-16.4.3-default"));
    write(&dir.join(".next/routes-manifest.json"), "{ not json");
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-102"), Some(Level::Error));
    assert_eq!(report.status, Status::Blocked);
    assert!(!report.complete);
    assert_eq!(report.exit_code, 1);

    // Incomplete: a manifest next build always writes is gone.
    let dir = scratch(Some("next-16.4.3-default"));
    std::fs::remove_file(dir.join(".next/prerender-manifest.json")).unwrap();
    let report = run(&dir, &[]);
    assert_eq!(report.exit_code, 1, "{:#?}", report.findings);
}

#[test]
fn version_policy_is_the_adapters() {
    let dir = scratch(Some("next-16.4.6-static-params"));
    installed(&dir, "15.5.12");
    set_build_version(&dir, "15.5.12");
    let report = run(&dir, &[]);
    let finding = report
        .findings
        .iter()
        .find(|f| f.code == "NEXTRS-DOC-104")
        .unwrap();
    assert_eq!(finding.level, Level::Error);
    assert_eq!(
        finding.inspect_code,
        Some(nextrs_adapter::UNSUPPORTED_VERSION_CODE)
    );
    // The installed version only informs when a build exists.
    assert_eq!(level(&report, "NEXTRS-DOC-005"), Some(Level::Info));
    assert_eq!(report.exit_code, 1);

    let dir = scratch(Some("next-16.4.6-static-params"));
    installed(&dir, "16.5.0");
    set_build_version(&dir, "16.5.0");
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-104"), Some(Level::Warning));
    assert_eq!(report.exit_code, 0);
    assert_eq!(run(&dir, &["--strict"]).exit_code, 1);

    // 16.4.3: tested line, published advisories.
    let report = run(&snapshot("next-16.4.3-default"), &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-104"), Some(Level::Pass));
    assert_eq!(level(&report, "NEXTRS-DOC-006"), Some(Level::Warning));
}

#[test]
fn installed_and_build_versions_must_match() {
    let dir = scratch(Some("next-16.4.6-static-params"));
    installed(&dir, "16.4.7");
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-106"), Some(Level::Warning));
}

#[test]
fn unsupported_framework_features_are_blockers() {
    let dir = scratch(Some("next-16.4.3-default"));
    installed(&dir, "16.4.3");
    write(
        &dir.join(".next/server/middleware-manifest.json"),
        r#"{"version":3,"middleware":{"/":{"name":"middleware"}},"sortedMiddleware":["/"],"functions":{}}"#,
    );
    write(
        &dir.join(".next/server/server-reference-manifest.json"),
        r#"{"node":{},"edge":{"7f00":{"workers":{}}}}"#,
    );
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-212"), Some(Level::Error));
    assert_eq!(level(&report, "NEXTRS-DOC-207"), Some(Level::Error));
    assert_eq!(level(&report, "NEXTRS-DOC-200"), None);
    assert_eq!(report.exit_code, 1);
}

#[test]
fn app_and_pages_routers_are_detected() {
    let app = run(&snapshot("next-16.4.6-static-params"), &[]);
    assert_eq!(level(&app, "NEXTRS-DOC-201"), Some(Level::Pass));
    assert_eq!(level(&app, "NEXTRS-DOC-202"), None);
    assert_eq!(level(&app, "NEXTRS-DOC-223"), Some(Level::Info));
    let pages = run(&snapshot("next-16.4.6-pages-optimized-dynamic"), &[]);
    assert_eq!(level(&pages, "NEXTRS-DOC-202"), Some(Level::Info));
    assert_eq!(pages.project.pages_router, Some(true));
}

#[test]
fn dynamic_routes_need_embedded_v8() {
    let dir = scratch(Some("next-16.4.3-default"));
    installed(&dir, "16.4.3");
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-203"), Some(Level::Info));
    assert_eq!(level(&report, "NEXTRS-DOC-204"), Some(Level::Info));
    assert_eq!(level(&report, "NEXTRS-DOC-401"), Some(Level::Info));
    let none = run(&dir, &["--js-runtime", "none"]);
    assert_eq!(level(&none, "NEXTRS-DOC-401"), Some(Level::Warning));
    let v8 = run(&dir, &["--js-runtime", "embedded-v8"]);
    assert_eq!(level(&v8, "NEXTRS-DOC-401"), Some(Level::Pass));

    // Server Actions: nextrs start refuses them without embedded V8.
    write(
        &dir.join(".next/server/server-reference-manifest.json"),
        r#"{"node":{"7f00":{"workers":{},"layer":{}}},"edge":{}}"#,
    );
    let actions = run(&dir, &["--js-runtime", "none"]);
    assert_eq!(level(&actions, "NEXTRS-DOC-206"), Some(Level::Info));
    assert_eq!(level(&actions, "NEXTRS-DOC-401"), Some(Level::Error));
    assert_eq!(run(&dir, &[]).exit_code, 0);

    // Without node_modules/next, the isolates cannot start.
    std::fs::remove_dir_all(dir.join("node_modules")).unwrap();
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-105"), Some(Level::Error));
    let report = run(&dir, &["--js-runtime", "none"]);
    assert_eq!(level(&report, "NEXTRS-DOC-105"), None);
}

#[test]
fn adapter_diagnostics_are_reported() {
    let dir = scratch(Some("next-16.4.3-adapter"));
    write(&dir.join(".next/BUILD_ID"), "another-build");
    std::fs::remove_dir_all(dir.join(".next/static")).unwrap();
    let report = run(&dir, &[]);
    let stale = report
        .findings
        .iter()
        .find(|f| f.code == "NEXTRS-DOC-110")
        .unwrap();
    assert_eq!(stale.level, Level::Warning);
    assert_eq!(stale.inspect_code, Some("stale-adapter-output"));
    assert_eq!(level(&report, "NEXTRS-DOC-111"), Some(Level::Warning));
    assert_eq!(report.project.metadata_source, Some("internal-manifests"));
    let fresh = run(&snapshot("next-16.4.3-adapter"), &[]);
    assert_eq!(fresh.project.metadata_source, Some("adapter-api"));
    assert_eq!(level(&fresh, "NEXTRS-DOC-110"), None);
}

#[test]
fn invalid_runtime_options_are_reported_as_start_would_refuse_them() {
    let dir = scratch(Some("next-16.4.3-default"));
    installed(&dir, "16.4.3");
    let report = run(
        &dir,
        &[
            "--net-allow",
            "ftp://example.com",
            "--trusted-proxy",
            "not-an-address",
            "--cache-dir",
            dir.join(".next/cache-here").to_str().unwrap(),
            "--image-cache-dir",
            dir.join("images").to_str().unwrap(),
        ],
    );
    assert_eq!(level(&report, "NEXTRS-DOC-402"), Some(Level::Error));
    assert_eq!(level(&report, "NEXTRS-DOC-406"), Some(Level::Error));
    assert_eq!(level(&report, "NEXTRS-DOC-404"), Some(Level::Error));
    assert_eq!(level(&report, "NEXTRS-DOC-405"), Some(Level::Error));
    // Checked, not created.
    assert!(!dir.join(".next/cache-here").exists());
    assert!(!dir.join("images").exists());

    let outside = scratch(None);
    let report = run(
        &dir,
        &[
            "--net-allow",
            "api.example.com",
            "--trusted-proxy",
            "10.0.0.0/8",
            "--cache-dir",
            outside.join("data-cache").to_str().unwrap(),
        ],
    );
    assert_eq!(level(&report, "NEXTRS-DOC-404"), Some(Level::Pass));
    assert_eq!(level(&report, "NEXTRS-DOC-406"), Some(Level::Pass));
    assert_eq!(level(&report, "NEXTRS-DOC-415"), Some(Level::Pass));
    assert!(!outside.join("data-cache").exists());
    // A public bind address without a forwarded-header policy.
    let report = run(&dir, &["--host", "0.0.0.0"]);
    assert_eq!(level(&report, "NEXTRS-DOC-406"), Some(Level::Warning));
}

#[test]
fn env_files_are_reported_by_name_only() {
    let dir = scratch(Some("next-16.4.3-default"));
    installed(&dir, "16.4.3");
    write(
        &dir.join(".env.production"),
        "DATABASE_URL=postgres://app:hunter2@db/app\nNEXT_PUBLIC_SITE=https://x\n",
    );
    let report = run(&dir, &[]);
    let finding = report
        .findings
        .iter()
        .find(|f| f.code == "NEXTRS-DOC-407")
        .unwrap();
    assert_eq!(finding.level, Level::Warning);
    assert!(finding.message.contains("DATABASE_URL"));
    assert!(!finding.message.contains("NEXT_PUBLIC_SITE"));
    let json = render::json(&report);
    assert!(!json.contains("hunter2"));
    assert!(!json.contains("postgres://"));

    let passed = run(&dir, &["--js-env", "DATABASE_URL"]);
    assert_eq!(level(&passed, "NEXTRS-DOC-407"), Some(Level::Pass));
    // Not set where Doctor runs (the test environment sets nothing).
    assert_eq!(level(&passed, "NEXTRS-DOC-408"), Some(Level::Warning));
    let set = diagnose(&args(&dir, &["--js-env", "DATABASE_URL"]), &|_| EnvVar::Set).unwrap();
    assert_eq!(level(&set, "NEXTRS-DOC-408"), None);
    // Set to an empty string: nextrs start passes it without a warning
    // (found on Velasia Airways, whose optional tokens are empty).
    let empty = diagnose(&args(&dir, &["--js-env", "DATABASE_URL"]), &|_| {
        EnvVar::Empty
    })
    .unwrap();
    assert_eq!(level(&empty, "NEXTRS-DOC-408"), None);
}

#[test]
fn node_api_evidence_comes_from_route_traces() {
    let dir = scratch(Some("next-16.4.3-default"));
    installed(&dir, "16.4.3");
    write(
        &dir.join(".next/server/app/api/health/route.js.nft.json"),
        r#"{"version":1,"files":["../../../../../node_modules/pg/lib/index.js",
            "../../../../../node_modules/bcrypt/lib/binding/napi-v3/bcrypt_lib.node",
            "../../../../../node_modules/next/dist/server/next.js"]}"#,
    );
    // next start's own trace: not NextRS's concern.
    write(
        &dir.join(".next/next-server.js.nft.json"),
        r#"{"version":1,"files":["../node_modules/@img/sharp-linux-x64/lib/sharp.node","../node_modules/puppeteer/x.js"]}"#,
    );
    let report = run(&dir, &[]);
    let native = report
        .findings
        .iter()
        .find(|f| f.code == "NEXTRS-DOC-301")
        .unwrap();
    assert_eq!(native.level, Level::Warning);
    assert!(native.message.contains("bcrypt (app/api/health/route.js)"));
    assert!(!native.message.contains("sharp"));
    assert_eq!(level(&report, "NEXTRS-DOC-302"), Some(Level::Warning));
    assert_eq!(level(&report, "NEXTRS-DOC-303"), None);
    // Warnings, not blockers: an optional addon may be loaded in try/catch.
    assert_eq!(report.exit_code, 0);

    // A build with traces and nothing in them.
    write(
        &dir.join(".next/server/app/api/health/route.js.nft.json"),
        r#"{"version":1,"files":["../../../../../node_modules/next/dist/server/next.js"]}"#,
    );
    let clean = run(&dir, &[]);
    assert_eq!(level(&clean, "NEXTRS-DOC-306"), Some(Level::Pass));
}

#[test]
fn plain_text_output_has_no_escape_codes() {
    let dir = scratch(Some("next-16.4.3-default"));
    let report = run(&dir, &["--js-runtime", "none"]);
    let text = render::text(&report, render::Style { color: false });
    assert!(!text.contains('\x1b'));
    for heading in [
        "NextRS Doctor",
        "\nEnvironment\n",
        "\nBuild\n",
        "\nCompatibility\n",
        "\nNode.js APIs\n",
        "\nRuntime configuration\n",
        "\nResult: No confirmed blockers\n",
        "\nNext steps:\n",
        "[NEXTRS-DOC-401]",
        "  ! ",
        "      → Run nextrs start with --js-runtime embedded-v8.",
    ] {
        assert!(text.contains(heading), "{heading:?} in\n{text}");
    }
    assert!(text.trim_end().ends_with("under NextRS."));
    let colored = render::text(&report, render::Style { color: true });
    assert!(colored.contains("\x1b[33m!\x1b[0m"));
}

#[test]
fn json_output_is_one_deterministic_document() {
    let dir = scratch(Some("next-16.4.3-default"));
    installed(&dir, "16.4.3");
    let first = render::json(&run(&dir, &[]));
    let second = render::json(&run(&dir, &[]));
    assert_eq!(first, second);
    let value: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["cliVersion"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["status"], "warnings");
    assert_eq!(value["complete"], true);
    assert_eq!(value["exitCode"], 0);
    assert_eq!(value["project"]["buildFound"], true);
    assert_eq!(value["environment"]["os"], std::env::consts::OS);
    for key in ["passed", "info", "warnings", "errors", "unknown"] {
        assert!(value["summary"][key].is_u64(), "{key}");
    }
    let findings = value["findings"].as_array().unwrap();
    for f in findings {
        for key in ["code", "severity", "category", "title", "message"] {
            assert!(f[key].is_string(), "{key} in {f}");
        }
        assert!(f["code"].as_str().unwrap().starts_with("NEXTRS-DOC-"));
    }
    // Findings carry no absolute path of this machine.
    let canonical = dir.canonicalize().unwrap();
    let text = serde_json::to_string(findings).unwrap();
    assert!(!text.contains(canonical.to_str().unwrap()), "{text}");
}

#[test]
fn findings_are_grouped_by_section() {
    let report = run(&snapshot("next-16.4.3-default"), &[]);
    let categories: Vec<_> = report.findings.iter().map(|f| f.category).collect();
    let mut sorted = categories.clone();
    sorted.sort();
    assert_eq!(categories, sorted);
}

#[test]
fn a_dist_directory_is_accepted() {
    let report = run(&snapshot("next-16.4.6-static-params").join(".next"), &[]);
    assert!(report.project.build_found);
    assert_eq!(level(&report, "NEXTRS-DOC-103"), Some(Level::Pass));
}

#[test]
fn invocation_errors_are_not_reports() {
    let missing = std::env::temp_dir().join("nextrs-doctor-does-not-exist");
    assert!(diagnose(&args(&missing, &[]), &|_| EnvVar::Unset).is_err());
    let file = scratch(None).join("file");
    write(&file, "x");
    assert!(diagnose(&args(&file, &[]), &|_| EnvVar::Unset).is_err());
}

/// An untrusted checkout cannot make the report print a file from outside
/// the project through a symlinked build file.
#[cfg(unix)]
#[test]
fn a_build_id_linked_outside_the_build_is_not_read() {
    let dir = scratch(Some("next-16.4.6-static-params"));
    installed(&dir, "16.4.6");
    let secret = scratch(None).join("credentials");
    write(&secret, "ghp_SECRETTOKEN");
    std::fs::remove_file(dir.join(".next/BUILD_ID")).unwrap();
    std::os::unix::fs::symlink(&secret, dir.join(".next/BUILD_ID")).unwrap();
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-108"), Some(Level::Warning));
    assert!(!report.complete);
    assert_eq!(report.status, Status::Incomplete);
    assert_eq!(report.exit_code, 0);
    assert_eq!(report.project.build_id, None);
    let json = render::json(&report);
    let text = render::text(&report, render::Style { color: false });
    assert!(!json.contains("SECRETTOKEN") && !text.contains("SECRETTOKEN"));
    // A manifest linked outside is refused the same way.
    let dir = scratch(Some("next-16.4.6-static-params"));
    installed(&dir, "16.4.6");
    let outside = scratch(None).join("routes-manifest.json");
    std::fs::rename(dir.join(".next/routes-manifest.json"), &outside).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join(".next/routes-manifest.json")).unwrap();
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-108"), Some(Level::Warning));
    // A link that stays inside the build is fine.
    let dir = scratch(Some("next-16.4.6-static-params"));
    installed(&dir, "16.4.6");
    std::fs::rename(dir.join(".next/BUILD_ID"), dir.join(".next/BUILD_ID.real")).unwrap();
    std::os::unix::fs::symlink("BUILD_ID.real", dir.join(".next/BUILD_ID")).unwrap();
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-108"), None);
    assert!(report.complete);
}

#[cfg(unix)]
#[test]
fn env_files_linked_outside_the_project_are_not_read() {
    let dir = scratch(Some("next-16.4.3-default"));
    installed(&dir, "16.4.3");
    let key = scratch(None).join("id_ed25519");
    write(
        &key,
        "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW=\n",
    );
    std::os::unix::fs::symlink(&key, dir.join(".env")).unwrap();
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-407"), None);
    assert!(!render::json(&report).contains("b3BlbnNzaC1rZXk"));
}

/// The startup checks read stored responses and list public/: Doctor does
/// not run them on a build that points them elsewhere or loops.
#[cfg(unix)]
#[test]
fn startup_checks_are_skipped_for_builds_that_point_outside() {
    let dir = scratch(Some("next-16.4.6-static-params"));
    installed(&dir, "16.4.6");
    let html = walk(&dir.join(".next/server/app"))
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "html"))
        .expect("the snapshot has a prerendered page");
    let outside = scratch(None).join("page.html");
    std::fs::rename(&html, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &html).unwrap();
    let report = run(&dir, &[]);
    assert_eq!(level(&report, "NEXTRS-DOC-107"), Some(Level::Unknown));
    let json = render::json(&report);
    assert!(!json.contains(outside.to_str().unwrap()));

    let dir = scratch(Some("next-16.4.6-static-params"));
    installed(&dir, "16.4.6");
    std::fs::create_dir_all(dir.join("public")).unwrap();
    std::os::unix::fs::symlink(".", dir.join("public/a")).unwrap();
    std::os::unix::fs::symlink(".", dir.join("public/b")).unwrap();
    let report = run(&dir, &[]);
    let finding = report
        .findings
        .iter()
        .find(|f| f.code == "NEXTRS-DOC-107")
        .unwrap();
    assert_eq!(finding.level, Level::Unknown);
    assert!(finding.message.contains("is a link to a directory"));
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        if entry.file_type().unwrap().is_dir() {
            out.extend(walk(&entry.path()));
        } else {
            out.push(entry.path());
        }
    }
    out.sort();
    out
}

#[test]
fn project_text_cannot_drive_the_terminal() {
    let dir = scratch(Some("next-16.4.6-static-params"));
    write(
        &dir.join("package.json"),
        "{\"name\":\"app\\u001b]52;c;ZXZpbA==\\u0007\\u001b[2K\",\"dependencies\":{\"next\":\"16.4.6\"}}",
    );
    write(
        &dir.join("node_modules/next/package.json"),
        r#"{"name":"next","version":"16.4.6"}"#,
    );
    let report = run(&dir, &[]);
    let text = render::text(&report, render::Style { color: false });
    assert!(
        !text.contains('\u{1b}') && !text.contains('\u{7}'),
        "{text}"
    );
    assert!(text.contains("app\\u{1b}]52;c;ZXZpbA=="));
}

#[test]
fn the_cache_directory_is_not_checked_without_the_executor() {
    let dir = scratch(Some("next-16.4.6-static-params"));
    installed(&dir, "16.4.6");
    // Inside the build: refused with the executor, ignored by nextrs start
    // without it.
    let inside = dir.join(".next/cache-dir");
    let inside = inside.to_str().unwrap();
    let with = run(&dir, &["--cache-dir", inside]);
    assert_eq!(level(&with, "NEXTRS-DOC-404"), Some(Level::Error));
    let without = run(&dir, &["--js-runtime", "none", "--cache-dir", inside]);
    assert_eq!(level(&without, "NEXTRS-DOC-404"), None);
}
