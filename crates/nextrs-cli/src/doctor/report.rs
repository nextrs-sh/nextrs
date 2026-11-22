//! The diagnostic model shared by every check: findings, their severity,
//! the overall status and the exit code.

use serde::Serialize;

/// Version of the `--json` document. Bumped when a field is removed or its
/// meaning changes; new optional fields keep the version.
pub const SCHEMA_VERSION: u32 = 1;

/// How serious a finding is.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    /// A check that passed.
    Pass,
    /// Context worth knowing; nothing to fix.
    Info,
    /// Doctor could not decide. Never counted as a blocker, even with
    /// `--strict`.
    Unknown,
    /// Likely to matter in production; not proven to break the app.
    Warning,
    /// A confirmed blocker: `nextrs start` refuses, or a part of the app is
    /// known to answer `501` or fail.
    Error,
}

/// The capability statuses of `engineering/compatibility.md`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Support {
    Supported,
    Experimental,
    Partial,
    Unsupported,
    /// Not covered by the compatibility suites, or not determinable.
    Unknown,
}

/// Report sections, in output order.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Environment,
    Build,
    Compatibility,
    NodeApis,
    RuntimeConfig,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Self::Environment,
        Self::Build,
        Self::Compatibility,
        Self::NodeApis,
        Self::RuntimeConfig,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Environment => "Environment",
            Self::Build => "Build",
            Self::Compatibility => "Compatibility",
            Self::NodeApis => "Node.js APIs",
            Self::RuntimeConfig => "Runtime configuration",
        }
    }
}

/// One diagnostic. Codes are stable: tools may match on them.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub code: &'static str,
    #[serde(rename = "severity")]
    pub level: Level,
    pub category: Category,
    pub title: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support: Option<Support>,
    /// A route, file or option, relative to the project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs: Option<&'static str>,
    /// The `nextrs inspect` diagnostic this finding reports, when it comes
    /// from one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspect_code: Option<&'static str>,
}

impl Finding {
    pub fn new(
        code: &'static str,
        level: Level,
        category: Category,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            level,
            category,
            title: title.into(),
            message: message.into(),
            support: None,
            component: None,
            suggestion: None,
            docs: None,
            inspect_code: None,
        }
    }

    pub fn support(mut self, support: Support) -> Self {
        self.support = Some(support);
        self
    }

    pub fn component(mut self, component: impl Into<String>) -> Self {
        self.component = Some(component.into());
        self
    }

    pub fn suggest(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn docs(mut self, docs: &'static str) -> Self {
        self.docs = Some(docs);
        self
    }

    pub fn inspect_code(mut self, code: &'static str) -> Self {
        self.inspect_code = Some(code);
        self
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct Summary {
    pub passed: usize,
    pub info: usize,
    pub warnings: usize,
    pub errors: usize,
    pub unknown: usize,
}

/// The overall verdict. `blocked` wins over `incomplete`, which wins over
/// `warnings`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// No blockers, no warnings, every check ran.
    Ok,
    /// No blockers; warnings to review.
    Warnings,
    /// No blockers found, but checks that need a production build (or a
    /// readable one) did not run.
    Incomplete,
    /// One or more confirmed blockers.
    Blocked,
}

impl Status {
    pub fn headline(self) -> &'static str {
        match self {
            Self::Ok | Self::Warnings => "No confirmed blockers",
            Self::Incomplete => "Incomplete: no confirmed blockers so far",
            Self::Blocked => "Confirmed blockers found",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    /// The path as given on the command line.
    pub path: String,
    /// `package.json#name`.
    pub name: Option<String>,
    /// A `package.json` that depends on `next`.
    pub nextjs_project: bool,
    pub build_found: bool,
    pub build_id: Option<String>,
    /// `adapter-api` or `internal-manifests`.
    pub metadata_source: Option<&'static str>,
    pub app_router: Option<bool>,
    pub pages_router: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentInfo {
    pub os: &'static str,
    pub arch: &'static str,
    /// `glibc`, `musl` or `other`.
    pub libc: &'static str,
    pub glibc_version: Option<String>,
    /// From `node_modules/next/package.json`.
    pub next_installed: Option<String>,
    /// The version the build was made with, and where it was read.
    pub next_build: Option<String>,
    pub next_build_source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub schema_version: u32,
    pub cli_version: &'static str,
    pub status: Status,
    /// Whether every check Doctor implements ran. Even then, Doctor does not
    /// prove runtime compatibility.
    pub complete: bool,
    pub strict: bool,
    pub exit_code: u8,
    pub project: ProjectInfo,
    pub environment: EnvironmentInfo,
    pub findings: Vec<Finding>,
    pub summary: Summary,
}

impl Report {
    /// Orders the findings (section, then the order checks added them) and
    /// computes the summary, status and exit code.
    pub fn finish(
        project: ProjectInfo,
        environment: EnvironmentInfo,
        mut findings: Vec<Finding>,
        complete: bool,
        strict: bool,
    ) -> Self {
        // Stable: within a section, checks keep their order.
        findings.sort_by_key(|f| f.category);
        let summary = summarize(&findings);
        let status = status(&summary, complete);
        Self {
            schema_version: SCHEMA_VERSION,
            cli_version: env!("CARGO_PKG_VERSION"),
            status,
            complete,
            strict,
            exit_code: exit_code(&summary, strict),
            project,
            environment,
            findings,
            summary,
        }
    }
}

pub fn summarize(findings: &[Finding]) -> Summary {
    let mut summary = Summary::default();
    for finding in findings {
        match finding.level {
            Level::Pass => summary.passed += 1,
            Level::Info => summary.info += 1,
            Level::Unknown => summary.unknown += 1,
            Level::Warning => summary.warnings += 1,
            Level::Error => summary.errors += 1,
        }
    }
    summary
}

pub fn status(summary: &Summary, complete: bool) -> Status {
    if summary.errors > 0 {
        Status::Blocked
    } else if !complete {
        Status::Incomplete
    } else if summary.warnings > 0 {
        Status::Warnings
    } else {
        Status::Ok
    }
}

/// `0`: no confirmed blockers. `1`: blockers (with `--strict`, warnings
/// too). Unknown findings never fail. (`2`, invocation errors, is decided
/// before a report exists.)
pub fn exit_code(summary: &Summary, strict: bool) -> u8 {
    if summary.errors > 0 || (strict && summary.warnings > 0) {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(level: Level) -> Finding {
        Finding::new("NEXTRS-DOC-000", level, Category::Build, "t", "m")
    }

    fn summary_of(levels: &[Level]) -> Summary {
        summarize(&levels.iter().map(|l| finding(*l)).collect::<Vec<_>>())
    }

    #[test]
    fn exit_codes_follow_the_documented_convention() {
        let clean = summary_of(&[Level::Pass, Level::Info]);
        assert_eq!(exit_code(&clean, false), 0);
        assert_eq!(exit_code(&clean, true), 0);
        let warned = summary_of(&[Level::Pass, Level::Warning]);
        assert_eq!(exit_code(&warned, false), 0);
        assert_eq!(exit_code(&warned, true), 1);
        let blocked = summary_of(&[Level::Error]);
        assert_eq!(exit_code(&blocked, false), 1);
        assert_eq!(exit_code(&blocked, true), 1);
    }

    #[test]
    fn strict_mode_does_not_turn_unknowns_into_failures() {
        let unknown = summary_of(&[Level::Unknown, Level::Unknown, Level::Info]);
        assert_eq!(exit_code(&unknown, true), 0);
        assert_eq!(status(&unknown, true), Status::Ok);
    }

    #[test]
    fn status_precedence() {
        assert_eq!(status(&summary_of(&[Level::Error]), false), Status::Blocked);
        assert_eq!(
            status(&summary_of(&[Level::Warning]), false),
            Status::Incomplete
        );
        assert_eq!(
            status(&summary_of(&[Level::Warning]), true),
            Status::Warnings
        );
        assert_eq!(status(&summary_of(&[Level::Pass]), true), Status::Ok);
    }
}
