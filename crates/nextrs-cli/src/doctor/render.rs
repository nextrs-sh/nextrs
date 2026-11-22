//! Text and JSON output.

use std::fmt::Write;

use super::report::{Category, Level, Report, Status};

/// ANSI styling, or none.
#[derive(Clone, Copy)]
pub struct Style {
    pub color: bool,
}

impl Style {
    /// Colors only on a terminal, and never with `NO_COLOR` (any value) or
    /// `TERM=dumb`.
    pub fn detect() -> Self {
        use std::io::IsTerminal;
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        let dumb = std::env::var_os("TERM").is_some_and(|t| t == "dumb");
        Self {
            color: std::io::stdout().is_terminal() && !no_color && !dumb,
        }
    }

    fn paint(self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }
}

fn mark(level: Level) -> &'static str {
    match level {
        Level::Pass => "✓",
        Level::Info => "i",
        Level::Unknown => "?",
        Level::Warning => "!",
        Level::Error => "✗",
    }
}

fn color(level: Level) -> &'static str {
    match level {
        Level::Pass => "32",
        Level::Info => "36",
        Level::Unknown => "35",
        Level::Warning => "33",
        Level::Error => "31",
    }
}

/// The final step every report ends with.
pub const TEST_STEP: &str = "Test the production build with nextrs start (adding \
     --js-runtime embedded-v8 for dynamic routes) and compare its responses with next start.";

pub fn text(report: &Report, style: Style) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}\n", style.paint("1", "NextRS Doctor"));
    let name = report
        .project
        .name
        .as_deref()
        .map(|n| format!(" ({n})"))
        .unwrap_or_default();
    let _ = writeln!(out, "Project: {}{name}", report.project.path);
    for category in Category::ALL {
        let findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.category == category)
            .collect();
        if findings.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n{}", style.paint("1", category.title()));
        for f in findings {
            let _ = writeln!(
                out,
                "  {} {}  {}",
                style.paint(color(f.level), mark(f.level)),
                f.title,
                style.paint("2", &format!("[{}]", f.code))
            );
            if f.level == Level::Pass {
                continue;
            }
            let _ = writeln!(out, "      {}", f.message);
            if let Some(suggestion) = &f.suggestion {
                let _ = writeln!(out, "      → {suggestion}");
            }
        }
    }
    let s = &report.summary;
    let headline = match report.status {
        Status::Blocked => style.paint("1;31", report.status.headline()),
        Status::Incomplete => style.paint("1;33", report.status.headline()),
        Status::Warnings | Status::Ok => style.paint("1;32", report.status.headline()),
    };
    let _ = writeln!(out, "\nResult: {headline}");
    let _ = writeln!(
        out,
        "Errors: {}  Warnings: {}  Unknown: {}  Info: {}  Passed: {}",
        s.errors, s.warnings, s.unknown, s.info, s.passed
    );
    if !report.complete {
        let _ = writeln!(
            out,
            "Assessment incomplete: checks that need a readable production build did not run."
        );
    }
    if report.strict && s.warnings > 0 && s.errors == 0 {
        let _ = writeln!(out, "--strict: warnings fail this check.");
    }

    let _ = writeln!(out, "\nNext steps:");
    for (i, step) in next_steps(report).iter().enumerate() {
        let _ = writeln!(out, "  {}. {step}", i + 1);
    }
    let _ = writeln!(
        out,
        "\n{}",
        style.paint(
            "2",
            "Exit code 0 means no confirmed blockers; it does not prove the application works \
             under NextRS."
        )
    );
    out
}

/// Suggestions, most serious first, without repeats; then the test step.
pub fn next_steps(report: &Report) -> Vec<String> {
    let mut steps: Vec<String> = Vec::new();
    for level in [Level::Error, Level::Warning, Level::Info, Level::Unknown] {
        for f in report.findings.iter().filter(|f| f.level == level) {
            if let Some(s) = &f.suggestion
                && !steps.contains(s)
            {
                steps.push(s.clone());
            }
        }
    }
    steps.push(TEST_STEP.to_owned());
    steps
}

pub fn json(report: &Report) -> String {
    serde_json::to_string_pretty(report).expect("the report serializes")
}
