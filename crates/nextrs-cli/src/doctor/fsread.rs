//! Reading the inspected project's files without letting it point Doctor
//! elsewhere.
//!
//! The project may be anyone's checkout, `.next` included. A report must
//! never carry data from outside it: a `.next/BUILD_ID` or a `.env` that is a
//! symlink to `/proc/self/environ` or a credentials file would otherwise be
//! printed (the build ID in a title, `.env` "names" from its lines). Every
//! file Doctor reads itself is therefore read only when, after symlinks, it is
//! a regular file inside one of the project's own directories, and only up to
//! a size limit.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// `package.json`, a `.env` file.
pub const SMALL: u64 = 4 << 20;
/// A manifest or a file trace.
pub const LARGE: u64 = 64 << 20;

/// Whether `path` resolves (symlinks followed) to something inside one of
/// `roots`, which must be canonical. A path that does not exist is not
/// contained.
pub fn contained(path: &Path, roots: &[PathBuf]) -> bool {
    path.canonicalize()
        .is_ok_and(|real| roots.iter().any(|root| real.starts_with(root)))
}

/// Whether `path`, written by the build, stays lexically below `base`: no
/// `..` component, no absolute component replacing it.
pub fn lexically_below(path: &Path, base: &Path) -> bool {
    path.starts_with(base)
        && !path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
}

/// The UTF-8 text of `path` when it is a regular file inside `roots`, of at
/// most `limit` bytes. `None` otherwise, as for a missing file.
pub fn read_contained(path: &Path, roots: &[PathBuf], limit: u64) -> Option<String> {
    let real = path.canonicalize().ok()?;
    if !roots.iter().any(|root| real.starts_with(root)) {
        return None;
    }
    read_regular(&real, limit)
}

/// The UTF-8 text of `path` when it is a regular file of at most `limit`
/// bytes, wherever it is. For files whose content Doctor only matches
/// against expected values (an installed package's version).
pub fn read_regular(path: &Path, limit: u64) -> Option<String> {
    // Checked before opening: opening a FIFO for reading blocks until a
    // writer appears.
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > limit {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut text = String::new();
    // procfs files report a length of 0: the cap holds for them too.
    file.take(limit.saturating_add(1))
        .read_to_string(&mut text)
        .ok()?;
    (u64::try_from(text.len()).unwrap_or(u64::MAX) <= limit).then_some(text)
}

/// Report text from the project, with control characters made visible, so
/// that a package name or route cannot move the terminal's cursor, recolor
/// the report or write to the clipboard.
pub fn printable(text: &str) -> String {
    if !text.chars().any(char::is_control) {
        return text.to_owned();
    }
    text.chars()
        .map(|c| {
            if c.is_control() {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nextrs-doctor-fsread-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn files_inside_the_roots_are_read() {
        let dir = scratch("inside");
        std::fs::write(dir.join("a.json"), "{}").unwrap();
        let roots = vec![dir.clone()];
        assert_eq!(
            read_contained(&dir.join("a.json"), &roots, SMALL).as_deref(),
            Some("{}")
        );
        assert!(contained(&dir.join("a.json"), &roots));
        assert!(!contained(&dir.join("missing"), &roots));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_out_of_the_roots_are_not_followed() {
        let dir = scratch("symlink");
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(dir.join("secret"), "TOKEN=hunter2\n").unwrap();
        std::os::unix::fs::symlink(dir.join("secret"), project.join(".env")).unwrap();
        std::fs::write(project.join("real"), "x").unwrap();
        std::os::unix::fs::symlink(project.join("real"), project.join("link")).unwrap();
        let roots = vec![project.clone()];
        assert_eq!(read_contained(&project.join(".env"), &roots, SMALL), None);
        assert!(!contained(&project.join(".env"), &roots));
        // A symlink that stays inside the project is fine.
        assert_eq!(
            read_contained(&project.join("link"), &roots, SMALL).as_deref(),
            Some("x")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn oversized_files_and_non_files_are_not_read() {
        let dir = scratch("limits");
        std::fs::write(dir.join("big"), "0123456789").unwrap();
        let roots = vec![dir.clone()];
        assert_eq!(read_contained(&dir.join("big"), &roots, 9), None);
        assert!(read_contained(&dir.join("big"), &roots, 10).is_some());
        assert_eq!(read_contained(&dir, &roots, SMALL), None);
        assert_eq!(read_regular(Path::new("/dev/zero"), SMALL), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_paths_must_stay_below_their_base() {
        let base = Path::new("/srv/app/.next");
        assert!(lexically_below(
            Path::new("/srv/app/.next/server/app/index.html"),
            base
        ));
        assert!(!lexically_below(
            Path::new("/srv/app/.next/server/../../../etc/passwd"),
            base
        ));
        assert!(!lexically_below(Path::new("/etc/passwd"), base));
    }

    #[test]
    fn control_characters_are_made_visible() {
        assert_eq!(printable("my-app"), "my-app");
        assert_eq!(
            printable("a\u{1b}]52;c;eA==\u{7}b"),
            "a\\u{1b}]52;c;eA==\\u{7}b"
        );
        assert_eq!(printable("x\ny"), "x\\ny");
    }
}
