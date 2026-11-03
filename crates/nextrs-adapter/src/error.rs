use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum InspectError {
    /// No `BUILD_ID` in the dist directory: `next build` has not run.
    NoBuildOutput {
        dist_dir: PathBuf,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    Invalid {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for InspectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoBuildOutput { dist_dir } => write!(
                f,
                "no Next.js build output at {} (run `next build` first)",
                dist_dir.display()
            ),
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::Json { path, source } => {
                write!(f, "invalid JSON in {}: {source}", path.display())
            }
            Self::Invalid { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for InspectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}
