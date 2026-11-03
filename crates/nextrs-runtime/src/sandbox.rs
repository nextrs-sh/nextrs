//! Filesystem capability for JavaScript running in the executor.
//!
//! Next.js handlers read manifests, route entries and chunks at request time,
//! so the engine needs a real `fs`. It gets read access to an explicit list
//! of roots and nothing else. There is no write capability: none of the
//! tested routes write, and adding one must be a deliberate decision.

use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FsPolicy {
    /// Canonical (symlink-resolved) roots.
    read_roots: Vec<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FsDenied {
    /// The path is outside every approved root, or is not a plain absolute
    /// path. Reported for nonexistent paths too, so that the policy cannot be
    /// used to probe for files elsewhere.
    OutsideRoots,
    NotFound,
}

impl fmt::Display for FsDenied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideRoots => f.write_str("EACCES"),
            Self::NotFound => f.write_str("ENOENT"),
        }
    }
}

impl FsPolicy {
    /// Roots that do not exist are skipped.
    pub fn new(read_roots: impl IntoIterator<Item = PathBuf>) -> Self {
        let read_roots = read_roots
            .into_iter()
            .filter_map(|root| std::fs::canonicalize(root).ok())
            .collect();
        Self { read_roots }
    }

    pub fn read_roots(&self) -> &[PathBuf] {
        &self.read_roots
    }

    /// Resolves `path` for reading. Returns the canonical path only if both
    /// the requested path (lexically) and its symlink-resolved target lie
    /// inside an approved root.
    pub fn resolve_read(&self, path: &str) -> Result<PathBuf, FsDenied> {
        let lexical = lexical_absolute(path).ok_or(FsDenied::OutsideRoots)?;
        if !self.inside(&lexical) {
            return Err(FsDenied::OutsideRoots);
        }
        let canonical = std::fs::canonicalize(&lexical).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => FsDenied::NotFound,
            _ => FsDenied::OutsideRoots,
        })?;
        if !self.inside(&canonical) {
            return Err(FsDenied::OutsideRoots);
        }
        Ok(canonical)
    }

    fn inside(&self, path: &Path) -> bool {
        self.read_roots.iter().any(|root| path.starts_with(root))
    }
}

/// `..` and `.` resolved without touching the filesystem. `None` for relative
/// paths, NUL bytes and `..` above the filesystem root.
fn lexical_absolute(path: &str) -> Option<PathBuf> {
    if path.contains('\0') || !path.starts_with('/') {
        return None;
    }
    let mut out = PathBuf::from("/");
    for component in Path::new(path).components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(part) => out.push(part),
            Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree() -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "nextrs-sandbox-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(base.join("project/.next/server")).unwrap();
        std::fs::write(base.join("project/.next/server/a.json"), "{}").unwrap();
        std::fs::write(base.join("project/.env"), "SECRET=1").unwrap();
        std::fs::write(base.join("outside.txt"), "x").unwrap();
        base
    }

    #[test]
    fn allows_files_inside_roots() {
        let base = temp_tree();
        let policy = FsPolicy::new([base.join("project/.next")]);
        let file = base.join("project/.next/server/a.json");
        assert!(policy.resolve_read(file.to_str().unwrap()).is_ok());
    }

    #[test]
    fn rejects_traversal_siblings_and_relative_paths() {
        let base = temp_tree();
        let policy = FsPolicy::new([base.join("project/.next")]);
        let root = base.join("project/.next");
        let root = root.to_str().unwrap();
        for path in [
            format!("{root}/../.env"),
            format!("{root}/server/../../.env"),
            format!("{root}/../../outside.txt"),
            format!("{root}/server/a.json\0"),
            "project/.next/server/a.json".to_owned(),
            "/etc/passwd".to_owned(),
        ] {
            assert_eq!(
                policy.resolve_read(&path),
                Err(FsDenied::OutsideRoots),
                "{path}"
            );
        }
        // A root's name as a prefix of a sibling directory is not inside it.
        std::fs::create_dir_all(base.join("project/.next-evil")).unwrap();
        std::fs::write(base.join("project/.next-evil/x"), "x").unwrap();
        let sibling = base.join("project/.next-evil/x");
        assert_eq!(
            policy.resolve_read(sibling.to_str().unwrap()),
            Err(FsDenied::OutsideRoots)
        );
    }

    #[test]
    fn missing_files_inside_roots_are_not_found_but_outside_are_denied() {
        let base = temp_tree();
        let policy = FsPolicy::new([base.join("project/.next")]);
        let inside = base.join("project/.next/missing.json");
        let outside = base.join("missing.json");
        assert_eq!(
            policy.resolve_read(inside.to_str().unwrap()),
            Err(FsDenied::NotFound)
        );
        assert_eq!(
            policy.resolve_read(outside.to_str().unwrap()),
            Err(FsDenied::OutsideRoots)
        );
    }

    #[test]
    fn rejects_symlinks_escaping_roots() {
        let base = temp_tree();
        let policy = FsPolicy::new([base.join("project/.next")]);
        let link = base.join("project/.next/server/escape");
        std::os::unix::fs::symlink(base.join("project/.env"), &link).unwrap();
        assert_eq!(
            policy.resolve_read(link.to_str().unwrap()),
            Err(FsDenied::OutsideRoots)
        );
    }
}
