//! What the memory is allowed to read.
//!
//! The rule is a short allow-list, not a long deny-list: documents, under a directory called `docs`, as UTF-8 text of
//! a known extension and a sane size, never through a symlink, never from a hidden or build directory, never with a
//! name that suggests a credential. A file that fails any test is *reported* (with the reason), not silently skipped.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

/// Directories whose contents are third-party code or build output: they carry other people's text, which is both noise
/// and an injection channel.
pub const EXCLUDED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "venv",
    "__pycache__",
    "coverage",
    "out",
    "site-packages",
];

/// File names (lower-cased, substring) that suggest a credential or key.
const SENSITIVE_NAME_PARTS: &[&str] = &[
    "secret",
    "credential",
    "password",
    "passwd",
    "private",
    "id_rsa",
    "id_ed25519",
    ".pem",
    ".key",
    "token",
];

#[derive(Debug, Clone)]
pub struct GuardConfig {
    /// Extensions that are indexed, lower-case, without the dot.
    pub extensions: Vec<String>,
    /// If set, a file must have an ancestor directory with this name (below the root) to be indexed.
    pub only_under: Option<String>,
    pub max_file_bytes: u64,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            extensions: vec!["md".into(), "txt".into()],
            only_under: Some("docs".into()),
            max_file_bytes: 200_000,
        }
    }
}

/// Why a path was not indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Skip {
    Symlink,
    Hidden,
    ExcludedDirectory,
    NotUnderDocs,
    WrongExtension,
    SensitiveName,
    TooLarge,
    NotUtf8,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub path: String,
    pub reason: Skip,
}

/// Why a caller-supplied relative path is refused by [`PathGuard::resolve`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GuardError {
    #[error("the path is empty, absolute, or contains a NUL byte")]
    Malformed,
    #[error("the path climbs out of the root with '..'")]
    Traversal,
    #[error("the path goes through a symbolic link")]
    Symlink,
    #[error("the path is not a file the memory may read: {0:?}")]
    NotAllowed(Skip),
    #[error("the path does not exist")]
    Missing,
}

#[derive(Debug, Clone)]
pub struct PathGuard {
    root: PathBuf,
    config: GuardConfig,
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl PathGuard {
    /// # Errors
    /// The root does not exist.
    pub fn new(root: &Path, config: GuardConfig) -> std::io::Result<Self> {
        Ok(Self {
            root: root.canonicalize()?,
            config,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    /// Verdict on a file by its name and place, before opening it.
    fn judge_name(&self, path: &Path) -> Result<(), Skip> {
        let rel = path.strip_prefix(&self.root).unwrap_or(path);
        let mut components: Vec<String> = rel
            .components()
            .filter_map(|c| match c {
                Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        let file = components.pop().unwrap_or_default();

        if components
            .iter()
            .chain(std::iter::once(&file))
            .any(|c| c.starts_with('.'))
        {
            return Err(Skip::Hidden);
        }
        if components
            .iter()
            .any(|c| EXCLUDED_DIRS.contains(&c.as_str()))
        {
            return Err(Skip::ExcludedDirectory);
        }
        if let Some(required) = &self.config.only_under
            && !components.iter().any(|c| c == required)
        {
            return Err(Skip::NotUnderDocs);
        }
        let extension = Path::new(&file)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !self.config.extensions.contains(&extension) {
            return Err(Skip::WrongExtension);
        }
        let lowered = file.to_ascii_lowercase();
        if SENSITIVE_NAME_PARTS
            .iter()
            .any(|part| lowered.contains(part))
        {
            return Err(Skip::SensitiveName);
        }
        Ok(())
    }

    /// Every file under the root the memory may read, and every file it was told to leave alone, with the reason.
    /// Symbolic links are never followed, so a link inside the root cannot lead out of it.
    #[must_use]
    pub fn walk(&self) -> (Vec<PathBuf>, Vec<Skipped>) {
        let (mut allowed, mut skipped) = (Vec::new(), Vec::new());
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                skipped.push(Skipped {
                    path: self.relative(&dir),
                    reason: Skip::Unreadable,
                });
                continue;
            };
            let mut paths: Vec<PathBuf> =
                entries.filter_map(Result::ok).map(|e| e.path()).collect();
            paths.sort();
            for path in paths {
                let Ok(meta) = fs::symlink_metadata(&path) else {
                    continue;
                };
                let shown = self.relative(&path);
                if meta.file_type().is_symlink() {
                    skipped.push(Skipped {
                        path: shown,
                        reason: Skip::Symlink,
                    });
                } else if meta.is_dir() {
                    let name = name_of(&path);
                    if name.starts_with('.') {
                        skipped.push(Skipped {
                            path: shown,
                            reason: Skip::Hidden,
                        });
                    } else if EXCLUDED_DIRS.contains(&name.as_str()) {
                        skipped.push(Skipped {
                            path: shown,
                            reason: Skip::ExcludedDirectory,
                        });
                    } else {
                        stack.push(path);
                    }
                } else if meta.is_file() {
                    match self.judge_name(&path) {
                        Err(reason) => skipped.push(Skipped {
                            path: shown,
                            reason,
                        }),
                        Ok(()) if meta.len() > self.config.max_file_bytes => {
                            skipped.push(Skipped {
                                path: shown,
                                reason: Skip::TooLarge,
                            })
                        }
                        Ok(()) => allowed.push(path),
                    }
                }
            }
        }
        allowed.sort();
        (allowed, skipped)
    }

    /// Turn a caller-supplied path *relative to the root* into a file the memory may read.
    ///
    /// # Errors
    /// See [`GuardError`]. Nothing outside the root, nothing through a link, nothing the walk would have skipped.
    pub fn resolve(&self, relative: &str) -> Result<PathBuf, GuardError> {
        if relative.is_empty() || relative.contains('\0') || Path::new(relative).is_absolute() {
            return Err(GuardError::Malformed);
        }
        let mut current = self.root.clone();
        for component in Path::new(relative).components() {
            match component {
                Component::Normal(part) => current.push(part),
                Component::CurDir => {}
                Component::ParentDir => return Err(GuardError::Traversal),
                Component::RootDir | Component::Prefix(_) => return Err(GuardError::Malformed),
            }
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(GuardError::Symlink),
                Ok(_) => {}
                Err(_) => return Err(GuardError::Missing),
            }
        }
        if !current.starts_with(&self.root) {
            return Err(GuardError::Traversal);
        }
        if !fs::symlink_metadata(&current).is_ok_and(|m| m.is_file()) {
            return Err(GuardError::Missing);
        }
        self.judge_name(&current).map_err(GuardError::NotAllowed)?;
        if fs::metadata(&current).is_ok_and(|m| m.len() > self.config.max_file_bytes) {
            return Err(GuardError::NotAllowed(Skip::TooLarge));
        }
        Ok(current)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }

    fn reasons(skipped: &[Skipped]) -> Vec<(String, Skip)> {
        skipped.iter().map(|s| (s.path.clone(), s.reason)).collect()
    }

    fn guard(root: &Path) -> PathGuard {
        PathGuard::new(root, GuardConfig::default()).expect("guard")
    }

    #[test]
    fn indexes_documents_under_docs_and_says_why_it_left_the_rest() {
        let dir = tempfile::tempdir().expect("dir");
        let r = dir.path();
        write(r, "app/docs/guide.md", "# Guide");
        write(r, "app/docs/notes.txt", "notes");
        write(r, "app/src/main.rs", "fn main() {}");
        write(r, "app/docs/diagram.png", "x");
        write(r, "README.md", "# top level, not under docs");
        write(r, "app/docs/.env", "KEY=value");
        write(r, "app/.git/docs/config.md", "# hidden dir");
        write(
            r,
            "app/docs/node_modules/pkg/docs/readme.md",
            "# third party",
        );
        write(r, "app/docs/credentials.md", "# x");
        write(r, "app/docs/id_rsa.md", "x");

        let (allowed, skipped) = guard(r).walk();
        let names: Vec<String> = allowed
            .iter()
            .map(|p| {
                p.strip_prefix(r.canonicalize().expect("canon"))
                    .expect("rel")
                    .display()
                    .to_string()
            })
            .collect();
        assert_eq!(names, ["app/docs/guide.md", "app/docs/notes.txt"]);
        let reasons = reasons(&skipped);
        for expected in [
            ("app/src/main.rs", Skip::NotUnderDocs),
            ("app/docs/diagram.png", Skip::WrongExtension),
            ("README.md", Skip::NotUnderDocs),
            ("app/docs/.env", Skip::Hidden),
            ("app/.git", Skip::Hidden),
            ("app/docs/node_modules", Skip::ExcludedDirectory),
            ("app/docs/credentials.md", Skip::SensitiveName),
            ("app/docs/id_rsa.md", Skip::SensitiveName),
        ] {
            assert!(
                reasons.contains(&(expected.0.to_owned(), expected.1)),
                "missing {expected:?} in {reasons:?}"
            );
        }
    }

    #[test]
    fn symbolic_links_are_reported_and_never_followed() {
        let dir = tempfile::tempdir().expect("dir");
        let outside = tempfile::tempdir().expect("outside");
        let r = dir.path();
        write(outside.path(), "stolen.md", "# outside the root");
        write(r, "docs/real.md", "# real");
        symlink(outside.path().join("stolen.md"), r.join("docs/link.md")).expect("file link");
        symlink(outside.path(), r.join("docs/dirlink")).expect("dir link");
        symlink("/etc/passwd", r.join("docs/passwd.md")).expect("passwd link");

        let (allowed, skipped) = guard(r).walk();
        assert_eq!(allowed.len(), 1);
        let reasons = reasons(&skipped);
        for name in ["docs/link.md", "docs/dirlink", "docs/passwd.md"] {
            assert!(
                reasons.contains(&(name.to_owned(), Skip::Symlink)),
                "{name} not reported: {reasons:?}"
            );
        }
    }

    #[test]
    fn a_file_over_the_size_limit_is_reported() {
        let dir = tempfile::tempdir().expect("dir");
        write(dir.path(), "docs/huge.md", &"x".repeat(200_001));
        let (allowed, skipped) = guard(dir.path()).walk();
        assert!(allowed.is_empty());
        assert_eq!(skipped[0].reason, Skip::TooLarge);
    }

    #[test]
    fn resolve_accepts_a_plain_relative_path() {
        let dir = tempfile::tempdir().expect("dir");
        write(dir.path(), "docs/guide.md", "# g");
        assert!(guard(dir.path()).resolve("docs/guide.md").is_ok());
        assert!(guard(dir.path()).resolve("./docs/./guide.md").is_ok());
    }

    #[test]
    fn resolve_refuses_every_way_out_of_the_root() {
        let dir = tempfile::tempdir().expect("dir");
        let outside = tempfile::tempdir().expect("outside");
        write(outside.path(), "secret.md", "# s");
        write(dir.path(), "docs/guide.md", "# g");
        symlink(outside.path(), dir.path().join("docs/out")).expect("link");
        symlink(
            outside.path().join("secret.md"),
            dir.path().join("docs/leak.md"),
        )
        .expect("link");
        let g = guard(dir.path());

        assert_eq!(g.resolve("../etc/passwd"), Err(GuardError::Traversal));
        assert_eq!(
            g.resolve("docs/../../etc/passwd"),
            Err(GuardError::Traversal)
        );
        assert_eq!(g.resolve("/etc/passwd"), Err(GuardError::Malformed));
        assert_eq!(g.resolve(""), Err(GuardError::Malformed));
        assert_eq!(g.resolve("docs/guide.md\0.png"), Err(GuardError::Malformed));
        assert_eq!(g.resolve("docs/out/secret.md"), Err(GuardError::Symlink));
        assert_eq!(g.resolve("docs/leak.md"), Err(GuardError::Symlink));
        assert_eq!(g.resolve("docs/missing.md"), Err(GuardError::Missing));
        assert_eq!(
            g.resolve("docs"),
            Err(GuardError::Missing),
            "a directory is not a file"
        );
    }

    #[test]
    fn resolve_applies_the_same_name_rules_as_the_walk() {
        let dir = tempfile::tempdir().expect("dir");
        write(dir.path(), "docs/.env", "K=v");
        write(dir.path(), "src/lib.md", "x");
        write(dir.path(), "docs/api_token.md", "x");
        let g = guard(dir.path());
        assert_eq!(
            g.resolve("docs/.env"),
            Err(GuardError::NotAllowed(Skip::Hidden))
        );
        assert_eq!(
            g.resolve("src/lib.md"),
            Err(GuardError::NotAllowed(Skip::NotUnderDocs))
        );
        assert_eq!(
            g.resolve("docs/api_token.md"),
            Err(GuardError::NotAllowed(Skip::SensitiveName))
        );
    }
}
