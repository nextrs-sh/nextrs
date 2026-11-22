//! `nextrs doctor` as a process: exit codes, stdout, environment variables
//! and the project left untouched.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

fn scratch(label: &str, snapshot_name: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nextrs-doctor-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    match snapshot_name {
        Some(name) => copy_dir(&snapshot(name), &dir),
        None => std::fs::create_dir_all(&dir).unwrap(),
    }
    dir
}

fn doctor(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nextrs"));
    command.arg("doctor").args(args);
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().unwrap()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}

/// Every file and directory under `dir`: kind, size, modification time and
/// contents.
fn tree(dir: &Path) -> BTreeMap<PathBuf, (bool, u64, std::time::SystemTime, Vec<u8>)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).unwrap() {
            let entry = entry.unwrap();
            let meta = entry.metadata().unwrap();
            let path = entry.path();
            let contents = if meta.is_file() {
                std::fs::read(&path).unwrap()
            } else {
                stack.push(path.clone());
                Vec::new()
            };
            out.insert(
                path,
                (
                    meta.is_dir(),
                    meta.len(),
                    meta.modified().unwrap(),
                    contents,
                ),
            );
        }
    }
    out
}

#[test]
fn exit_codes() {
    let ok = scratch("exit-ok", Some("next-16.4.6-static-params"));
    let output = doctor(&[ok.to_str().unwrap()], &[]);
    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );

    let corrupt = scratch("exit-corrupt", Some("next-16.4.6-static-params"));
    std::fs::write(corrupt.join(".next/routes-manifest.json"), "[").unwrap();
    assert_eq!(code(&doctor(&[corrupt.to_str().unwrap()], &[])), 1);

    // Warnings fail only with --strict; the missing build is one.
    let empty = scratch("exit-empty", None);
    assert_eq!(code(&doctor(&[empty.to_str().unwrap()], &[])), 0);
    assert_eq!(
        code(&doctor(&[empty.to_str().unwrap(), "--strict"], &[])),
        1
    );

    let missing = doctor(&["/nonexistent/nextrs-doctor-project"], &[]);
    assert_eq!(code(&missing), 2);
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("error: nextrs doctor"));
    assert_eq!(code(&doctor(&[".", "--no-such-flag"], &[])), 2);
    assert_eq!(code(&doctor(&[".", "--js-runtime", "deno"], &[])), 2);
}

#[test]
fn json_mode_prints_exactly_one_document() {
    let dir = scratch("json", Some("next-16.4.3-default"));
    std::fs::create_dir_all(dir.join("node_modules/next")).unwrap();
    std::fs::write(
        dir.join("node_modules/next/package.json"),
        r#"{"version":"16.4.3"}"#,
    )
    .unwrap();
    let output = doctor(
        &[dir.to_str().unwrap(), "--json"],
        // Logging requested: it must stay on stderr.
        &[("RUST_LOG", "debug")],
    );
    assert_eq!(code(&output), 0);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(value["exitCode"], 0);
    // A missing build is still one valid document.
    let empty = scratch("json-empty", None);
    let output = doctor(&[empty.to_str().unwrap(), "--json", "--strict"], &[]);
    assert_eq!(code(&output), 1);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "incomplete");
    assert_eq!(value["complete"], false);
    assert_eq!(value["findings"][0]["code"].as_str().unwrap().len(), 14);
}

#[test]
fn environment_values_never_reach_the_output() {
    let dir = scratch("secrets", Some("next-16.4.3-default"));
    std::fs::write(
        dir.join(".env"),
        "SESSION_SECRET=from-dotenv-file-value\nexport API_TOKEN=\"quoted-token-value\"\n",
    )
    .unwrap();
    for json in [true, false] {
        let mut args = vec![
            dir.to_str().unwrap(),
            "--js-env",
            "SESSION_SECRET",
            "--js-env",
            "NEXT_DEPLOYMENT_ID",
        ];
        if json {
            args.push("--json");
        }
        let output = doctor(
            &args,
            &[
                ("SESSION_SECRET", "from-environment-value"),
                ("NEXT_DEPLOYMENT_ID", "deployment-value"),
                ("NEXT_SERVER_ACTIONS_ENCRYPTION_KEY", "action-key-value"),
            ],
        );
        let all = [output.stdout, output.stderr].concat();
        let all = String::from_utf8_lossy(&all);
        for secret in [
            "from-dotenv-file-value",
            "quoted-token-value",
            "from-environment-value",
            "deployment-value",
            "action-key-value",
        ] {
            assert!(!all.contains(secret), "{secret} leaked:\n{all}");
        }
        assert!(all.contains("API_TOKEN"));
    }
}

#[test]
fn diagnostics_do_not_modify_the_project() {
    let dir = scratch("readonly", Some("next-16.4.3-default"));
    std::fs::create_dir_all(dir.join("node_modules/next")).unwrap();
    std::fs::write(
        dir.join("node_modules/next/package.json"),
        r#"{"version":"16.4.3"}"#,
    )
    .unwrap();
    let elsewhere = scratch("readonly-dirs", None);
    let before = tree(&dir);
    let before_elsewhere = tree(&elsewhere);
    let output = doctor(
        &[
            dir.to_str().unwrap(),
            "--js-runtime",
            "embedded-v8",
            "--cache-dir",
            elsewhere.join("cache").to_str().unwrap(),
            "--image-cache-dir",
            elsewhere.join("images").to_str().unwrap(),
            "--net-allow",
            "api.example.com",
        ],
        &[],
    );
    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(tree(&dir), before);
    assert_eq!(tree(&elsewhere), before_elsewhere);
}

#[test]
fn piped_output_is_plain_text() {
    let dir = scratch("plain", Some("next-16.4.3-default"));
    for env in [
        vec![],
        vec![("NO_COLOR", "1")],
        vec![("TERM", "xterm-256color")],
    ] {
        let output = doctor(&[dir.to_str().unwrap()], &env);
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.starts_with("NextRS Doctor\n"));
        assert!(!stdout.contains('\x1b'));
    }
}

#[test]
fn a_reader_that_stops_early_does_not_crash_doctor() {
    let dir = scratch("closed-stdout", Some("next-16.4.6-static-params"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_nextrs"))
        .args(["doctor", dir.to_str().unwrap(), "--json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Close the read end before Doctor writes its report.
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    // The findings' exit code, not a panic's 101.
    assert!(matches!(code(&output), 0 | 1), "{}", code(&output));
    std::fs::remove_dir_all(&dir).unwrap();
}
