//! The machine Doctor runs on, and what kind of project the path is.

use std::path::Path;

use nextrs_adapter::{VersionSupport, below_security_baseline, version_support};

use super::report::{Category, Finding, Level, Support};
use super::{DOCS_COMPAT_VERSIONS, DOCS_INSTALL};

/// The glibc floor of the published binaries: `scripts/package-npm.sh`
/// `MIN_GLIBC` and the npm launcher `packages/cli/bin/nextrs` (a test keeps
/// them equal).
pub const PUBLISHED_MIN_GLIBC: (u32, u32) = (2, 35);

/// The only platform release binaries exist for.
const PUBLISHED_OS: &str = "linux";
const PUBLISHED_ARCH: &str = "x86_64";

pub fn libc_family() -> &'static str {
    if cfg!(all(target_os = "linux", target_env = "gnu")) {
        "glibc"
    } else if cfg!(target_env = "musl") {
        "musl"
    } else {
        "other"
    }
}

/// The running glibc's version (`gnu_get_libc_version`), not the one the
/// binary was linked against.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn glibc_version() -> Option<String> {
    // SAFETY: glibc returns a pointer to a static, NUL-terminated string.
    let version = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) };
    version.to_str().ok().map(str::to_owned)
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn glibc_version() -> Option<String> {
    None
}

fn parse_glibc(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    Some((major, minor))
}

/// Platform, C library and NextRS version.
pub fn platform_findings(os: &str, arch: &str, glibc: Option<&str>) -> Vec<Finding> {
    let mut findings = Vec::new();
    let platform = format!("{} {arch}", os_name(os));
    findings.push(if os == PUBLISHED_OS && arch == PUBLISHED_ARCH {
        Finding::new(
            "NEXTRS-DOC-001",
            Level::Pass,
            Category::Environment,
            platform,
            "The platform NextRS release binaries are built and tested for.",
        )
        .support(Support::Supported)
    } else {
        Finding::new(
            "NEXTRS-DOC-001",
            Level::Warning,
            Category::Environment,
            format!("{platform} is not a release platform"),
            "Release binaries exist for Linux x86_64 only; this binary was built from source for \
             another platform, which no compatibility suite covers.",
        )
        .support(Support::Unknown)
        .suggest("Validate on Linux x86_64 before relying on this platform.")
        .docs(DOCS_INSTALL)
    });

    let (min_major, min_minor) = PUBLISHED_MIN_GLIBC;
    findings.push(match (libc_family(), glibc) {
        ("glibc", Some(version)) => match parse_glibc(version) {
            Some(found) if found >= PUBLISHED_MIN_GLIBC => Finding::new(
                "NEXTRS-DOC-002",
                Level::Pass,
                Category::Environment,
                format!("glibc {version}"),
                format!("Release binaries need glibc {min_major}.{min_minor} or later."),
            ),
            Some(_) => Finding::new(
                "NEXTRS-DOC-002",
                Level::Warning,
                Category::Environment,
                format!("glibc {version} is older than {min_major}.{min_minor}"),
                format!(
                    "Release binaries need glibc {min_major}.{min_minor} or later (this one was built \
                     from source). Ubuntu 22.04+ and Debian 12+ are tested."
                ),
            )
            .suggest(format!(
                "Deploy on a distribution or base image with glibc {min_major}.{min_minor} or later."
            ))
            .docs(DOCS_INSTALL),
            None => Finding::new(
                "NEXTRS-DOC-002",
                Level::Unknown,
                Category::Environment,
                format!("glibc version {version:?} not recognised"),
                format!("Release binaries need glibc {min_major}.{min_minor} or later."),
            ),
        },
        (family, _) => Finding::new(
            "NEXTRS-DOC-002",
            Level::Warning,
            Category::Environment,
            format!("C library: {family}"),
            format!(
                "Release binaries need glibc {min_major}.{min_minor} or later; other C libraries \
                 (musl, as on Alpine) are untested."
            ),
        )
        .support(Support::Unknown)
        .docs(DOCS_INSTALL),
    });

    findings.push(Finding::new(
        "NEXTRS-DOC-003",
        Level::Info,
        Category::Environment,
        format!("NextRS {}", env!("CARGO_PKG_VERSION")),
        "The nextrs binary running this check. It serves Next.js production builds with \
         embedded V8; Node.js is needed only for next build.",
    ));
    findings
}

fn os_name(os: &str) -> String {
    match os {
        "linux" => "Linux".into(),
        "macos" => "macOS".into(),
        "windows" => "Windows".into(),
        other => other.into(),
    }
}

/// What `package.json` says, read without running anything.
#[derive(Debug, Default)]
pub struct Package {
    pub name: Option<String>,
    /// The `next` range from dependencies or devDependencies.
    pub next_range: Option<String>,
}

pub fn read_package(project_dir: &Path) -> Option<Package> {
    let text = std::fs::read_to_string(project_dir.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let next_range = ["dependencies", "devDependencies"]
        .iter()
        .find_map(|key| value.get(key)?.get("next")?.as_str())
        .map(str::to_owned);
    Some(Package {
        name: value
            .get("name")
            .and_then(|n| n.as_str())
            .map(str::to_owned),
        next_range,
    })
}

/// `<dir>/node_modules/next/package.json#version`.
pub fn installed_next(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("node_modules/next/package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("version")?.as_str().map(str::to_owned)
}

/// Is this a Next.js project, and which Next.js is installed. With a build,
/// the build's version is what NextRS checks (see the build findings); the
/// installed version is compared with it there.
pub fn project_findings(
    package: Option<&Package>,
    has_config: bool,
    installed: Option<&str>,
    build_found: bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    findings.push(match package {
        Some(Package {
            next_range: Some(range),
            ..
        }) => Finding::new(
            "NEXTRS-DOC-004",
            Level::Pass,
            Category::Environment,
            "Next.js project",
            format!("package.json depends on next ({range})."),
        )
        .component("package.json"),
        _ if build_found => Finding::new(
            "NEXTRS-DOC-004",
            Level::Info,
            Category::Environment,
            "Next.js build directory",
            "No package.json that depends on next, but a production build is present (a copied \
             build or a standalone bundle).",
        ),
        _ if has_config => Finding::new(
            "NEXTRS-DOC-004",
            Level::Warning,
            Category::Environment,
            "next.config found, but no next dependency",
            "package.json does not list next in dependencies or devDependencies.",
        )
        .suggest("Run Doctor in the directory of the Next.js application."),
        _ => Finding::new(
            "NEXTRS-DOC-004",
            Level::Warning,
            Category::Environment,
            "Not identified as a Next.js project",
            "No package.json depending on next, no next.config and no .next build in this \
             directory.",
        )
        .suggest("Run Doctor in the directory of the Next.js application (nextrs doctor <path>)."),
    });

    match installed {
        Some(version) => {
            // The same policy nextrs start applies to the build's version.
            let (level, support, note) = match version_support(version) {
                VersionSupport::Tested => (
                    Level::Pass,
                    Support::Supported,
                    "the release line NextRS is tested with",
                ),
                VersionSupport::Untested => (
                    Level::Warning,
                    Support::Unknown,
                    "a supported major, but not the tested release line",
                ),
                VersionSupport::Unsupported => (
                    Level::Error,
                    Support::Unsupported,
                    "not a Next.js major this NextRS release serves; nextrs start refuses its builds",
                ),
            };
            // With a build, the build's version decides (reported there);
            // the installed one only informs.
            let level = if build_found && level != Level::Pass {
                Level::Info
            } else {
                level
            };
            let mut finding = Finding::new(
                "NEXTRS-DOC-005",
                level,
                Category::Environment,
                format!("Next.js {version} installed"),
                format!(
                    "node_modules/next is {version}: {note} ({}.x).",
                    nextrs_adapter::TESTED_NEXT_VERSION
                ),
            )
            .support(support)
            .component("node_modules/next")
            .docs(DOCS_COMPAT_VERSIONS);
            if level == Level::Error || level == Level::Warning {
                finding = finding.suggest(format!(
                    "Use Next.js {}.x (the pinned baseline is listed in the compatibility guide).",
                    nextrs_adapter::TESTED_NEXT_VERSION
                ));
            }
            findings.push(finding);
            if !build_found && below_security_baseline(version) {
                let (major, minor, patch) = nextrs_adapter::NEXT_SECURITY_BASELINE;
                findings.push(
                    Finding::new(
                        "NEXTRS-DOC-006",
                        Level::Warning,
                        Category::Environment,
                        format!("Next.js {version} has published security advisories"),
                        format!(
                            "Versions older than {major}.{minor}.{patch} are affected by published \
                             Next.js advisories; NextRS runs Next's own handlers, so they apply."
                        ),
                    )
                    .suggest(format!(
                        "Upgrade to Next.js {major}.{minor}.{patch} or later."
                    ))
                    .docs(DOCS_COMPAT_VERSIONS),
                );
            }
        }
        None if !build_found => findings.push(
            Finding::new(
                "NEXTRS-DOC-005",
                Level::Unknown,
                Category::Environment,
                "Installed Next.js version unknown",
                "node_modules/next/package.json was not found here (dependencies not installed, \
                 or hoisted to a workspace root Doctor can only learn from a build).",
            )
            .component("node_modules/next"),
        ),
        // Reported by the build checks.
        None => {}
    }
    findings
}

/// Files `next start` loads into `process.env` in production, in its
/// precedence order. NextRS loads none of them.
pub const ENV_FILES: [&str; 4] = [
    ".env.production.local",
    ".env.local",
    ".env.production",
    ".env",
];

/// The variable names `.env` files in `project_dir` define (values are never
/// kept), without `NEXT_PUBLIC_` ones, which `next build` inlines. Sorted,
/// deduplicated; with the files that define any.
pub fn env_file_names(project_dir: &Path) -> (Vec<&'static str>, Vec<String>) {
    let mut files = Vec::new();
    let mut names = Vec::new();
    for file in ENV_FILES {
        let Ok(text) = std::fs::read_to_string(project_dir.join(file)) else {
            continue;
        };
        let found = dotenv_names(&text);
        if !found.is_empty() {
            files.push(file);
        }
        names.extend(found);
    }
    names.retain(|n| !n.starts_with("NEXT_PUBLIC_"));
    names.sort();
    names.dedup();
    (files, names)
}

/// Names defined by a dotenv file (`NAME=value`, `export NAME=value`),
/// skipping comments and the continuation lines of multi-line quoted
/// values. The values themselves are dropped here.
pub fn dotenv_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut open_quote: Option<char> = None;
    for line in text.lines() {
        if let Some(quote) = open_quote {
            if line.contains(quote) {
                open_quote = None;
            }
            continue;
        }
        let line = line.trim_start();
        if line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let valid = !name.is_empty()
            && name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            continue;
        }
        names.push(name.to_owned());
        let value = value.trim_start();
        if let Some(quote) = value
            .chars()
            .next()
            .filter(|c| matches!(c, '"' | '\'' | '`'))
        {
            // Closed on the same line: anything after the first char.
            if !value[1..].contains(quote) {
                open_quote = Some(quote);
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glibc_versions_are_compared_numerically() {
        assert_eq!(parse_glibc("2.35"), Some((2, 35)));
        assert_eq!(parse_glibc("2.39"), Some((2, 39)));
        assert_eq!(parse_glibc("2.4"), Some((2, 4)));
        assert!(parse_glibc("2.4").unwrap() < PUBLISHED_MIN_GLIBC);
        assert_eq!(parse_glibc("garbage"), None);
        let old = platform_findings("linux", "x86_64", Some("2.31"));
        assert_eq!(old[1].level, Level::Warning);
        let new = platform_findings("linux", "x86_64", Some("2.39"));
        assert_eq!(new[0].level, Level::Pass);
        assert_eq!(new[1].level, Level::Pass);
        let other = platform_findings("linux", "aarch64", Some("2.39"));
        assert_eq!(other[0].level, Level::Warning);
    }

    /// The published floor is also declared by the npm launcher and the
    /// packaging script; keep them equal.
    #[test]
    fn the_glibc_floor_matches_the_npm_package() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (major, minor) = PUBLISHED_MIN_GLIBC;
        let floor = format!("{major}.{minor}");
        let script = std::fs::read_to_string(root.join("scripts/package-npm.sh")).unwrap();
        assert!(script.contains(&format!("MIN_GLIBC={floor}\n")));
        let launcher = std::fs::read_to_string(root.join("packages/cli/bin/nextrs")).unwrap();
        assert!(launcher.contains(&format!("NextRS needs glibc {floor} or later")));
    }

    #[test]
    fn dotenv_names_never_include_values() {
        let text = "# comment\nDATABASE_URL=postgres://user:hunter2@db/app\nexport API_KEY='abc'\n\
                    MULTI=\"line one\nFAKE_NAME=inside the value\nend\"\n  SPACED = 1\nNEXT_PUBLIC_X=1\n\
                    1BAD=x\nnot a line\n";
        let names = dotenv_names(text);
        assert_eq!(
            names,
            [
                "DATABASE_URL",
                "API_KEY",
                "MULTI",
                "SPACED",
                "NEXT_PUBLIC_X"
            ]
        );
        assert!(!names.iter().any(|n| n.contains("hunter2")));
    }

    #[test]
    fn installed_version_policy_matches_the_adapter() {
        let tested = project_findings(None, false, Some("16.4.6"), false);
        assert_eq!(tested[1].level, Level::Pass);
        let untested = project_findings(None, false, Some("16.5.0"), false);
        assert_eq!(untested[1].level, Level::Warning);
        let unsupported = project_findings(None, false, Some("15.5.12"), false);
        assert_eq!(unsupported[1].level, Level::Error);
        // With a build, the build's version decides.
        let with_build = project_findings(None, false, Some("15.5.12"), true);
        assert_eq!(with_build[1].level, Level::Info);
        let advisories = project_findings(None, false, Some("16.3.0"), false);
        assert!(advisories.iter().any(|f| f.code == "NEXTRS-DOC-006"));
    }
}
