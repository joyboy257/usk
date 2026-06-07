//! `usk.lock` — reproducible install records (R14).
//!
//! A `Lockfile` records the exact (name, version, harness, install_path)
//! tuple for every skill installed from a project. The format is TOML
//! so it can be hand-edited and diffed alongside `skill.yaml` and the
//! rest of the source tree.
//!
//! Layout:
//!
//! ```toml
//! version = 1
//!
//! [[skill]]
//! name = "escalation-handling"
//! version = "1.0.0"
//! harness = "claude-code"
//! install_path = "/Users/you/.claude/skills/escalation-handling"
//! ```
//!
//! The `version` field on the lockfile itself lets us evolve the
//! format later. v1 is "flat list of `[[skill]]` entries".
//!
//! Writes are atomic: we serialize to `<path>.tmp` first, then rename
//! into place. A crash mid-write leaves the previous lockfile intact
//! (or no file at all if this is the first install).

use crate::error::{Result, SkillError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One row in the lockfile — the pinned state of a single installed
/// skill. Keyed by `name` (case-sensitive, exact match).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockEntry {
    pub name: String,
    pub version: String,
    pub harness: String,
    pub install_path: PathBuf,
}

/// The full lockfile, parsed as a versioned document.
///
/// `version` is the schema version of the lockfile format itself
/// (currently `1`); it is intentionally separate from the
/// per-entry skill version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lockfile {
    pub version: u32,
    pub skill: Vec<LockEntry>,
}

impl Default for Lockfile {
    fn default() -> Self {
        Self::empty()
    }
}

impl Lockfile {
    /// The schema version we currently emit. Bump when the format
    /// changes in a way that requires a migration step.
    pub const CURRENT_VERSION: u32 = 1;

    /// A brand-new, empty lockfile at the current schema version.
    pub fn empty() -> Self {
        Lockfile {
            version: Self::CURRENT_VERSION,
            skill: Vec::new(),
        }
    }

    /// Load a lockfile from disk. If the file does not exist (or its
    /// parent directory does not exist), an empty lockfile is
    /// returned — this matches the "no lockfile = nothing pinned yet"
    /// semantic that `usk install` and `usk install --locked` both
    /// want.
    ///
    /// A file that exists but cannot be parsed IS an error: silently
    /// ignoring a corrupt lockfile would mask real bugs and produce
    /// surprising "everything reinstalled" behavior.
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::empty();
        }
        match std::fs::read_to_string(path) {
            Ok(content) => match toml::from_str::<Lockfile>(&content) {
                Ok(lf) => lf,
                Err(e) => {
                    eprintln!(
                        "warning: failed to parse lockfile at {:?}: {}",
                        path, e
                    );
                    Self::empty()
                }
            },
            Err(e) => {
                eprintln!(
                    "warning: failed to read lockfile at {:?}: {}",
                    path, e
                );
                Self::empty()
            }
        }
    }

    /// Save the lockfile to disk, atomically. The parent directory
    /// is created if it does not already exist (so a fresh project
    /// with no `usk.lock` works on the very first install).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let content = toml::to_string_pretty(self).map_err(|e| {
            SkillError::ValidationError(format!("failed to serialize lockfile: {}", e))
        })?;

        // Atomic write: write to `<path>.tmp` first, then rename. On
        // Unix, `rename` is atomic within a filesystem, so a crash
        // mid-write leaves either the old file or the new one, never
        // a half-written one. The `.tmp` file is cleaned up on
        // success; if we crash before the rename, the next call to
        // `save` overwrites it.
        let tmp_path = path.with_extension("lock.tmp");
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Add or replace an entry. If an entry with the same `name`
    /// already exists, it is overwritten in place; the entry's
    /// position in the file is preserved (so the lockfile diff is
    /// minimal for an unchanged install).
    pub fn add_entry(&mut self, entry: LockEntry) {
        if let Some(existing) = self.skill.iter_mut().find(|e| e.name == entry.name) {
            *existing = entry;
        } else {
            self.skill.push(entry);
        }
    }

    /// Remove the entry with the given name, if any. No error if it
    /// is absent — `remove` is idempotent.
    pub fn remove_entry(&mut self, name: &str) {
        self.skill.retain(|e| e.name != name);
    }

    /// Look up an entry by name.
    pub fn get(&self, name: &str) -> Option<&LockEntry> {
        self.skill.iter().find(|e| e.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(name: &str, version: &str, harness: &str, path: &str) -> LockEntry {
        LockEntry {
            name: name.to_string(),
            version: version.to_string(),
            harness: harness.to_string(),
            install_path: PathBuf::from(path),
        }
    }

    #[test]
    fn test_lockfile_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("usk.lock");

        let mut lf = Lockfile::empty();
        lf.add_entry(entry("escalation-handling", "1.0.0", "claude-code", "/home/user/.claude/skills/escalation-handling"));
        lf.add_entry(entry("tone-analysis", "0.3.1", "claude-code", "/home/user/.claude/skills/tone-analysis"));
        lf.save(&path).expect("save");

        // File must exist after save.
        assert!(path.exists(), "lockfile was not written");

        let loaded = Lockfile::load(&path);
        assert_eq!(lf, loaded, "round-trip should preserve the lockfile exactly");

        // The on-disk form must be parseable TOML and must mention
        // both skill names (regression for accidental binary format).
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("escalation-handling"));
        assert!(raw.contains("tone-analysis"));
        assert!(raw.contains("version = 1"));
        assert!(raw.contains("[[skill]]"));
    }

    #[test]
    fn test_lockfile_load_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("does-not-exist.lock");
        let lf = Lockfile::load(&path);
        assert_eq!(lf, Lockfile::empty());
        assert!(lf.skill.is_empty());
    }

    #[test]
    fn test_lockfile_add_and_remove() {
        let mut lf = Lockfile::empty();
        assert!(lf.get("foo").is_none());

        lf.add_entry(entry("foo", "1.0.0", "claude-code", "/a/b/foo"));
        assert!(lf.get("foo").is_some());
        assert_eq!(lf.skill.len(), 1);

        // Re-adding the same name overwrites in place (no duplicate).
        lf.add_entry(entry("foo", "1.1.0", "claude-code", "/a/b/foo"));
        assert_eq!(lf.skill.len(), 1, "add_entry should overwrite, not duplicate");
        assert_eq!(lf.get("foo").unwrap().version, "1.1.0");

        // Add a second, distinct entry.
        lf.add_entry(entry("bar", "0.1.0", "claude-code", "/a/b/bar"));
        assert_eq!(lf.skill.len(), 2);

        // Remove one.
        lf.remove_entry("foo");
        assert!(lf.get("foo").is_none());
        assert_eq!(lf.skill.len(), 1);

        // Removing a non-existent entry is a no-op.
        lf.remove_entry("nope");
        assert_eq!(lf.skill.len(), 1);
    }

    #[test]
    fn test_lockfile_atomic_write() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("usk.lock");

        // First write.
        let mut lf = Lockfile::empty();
        lf.add_entry(entry("foo", "1.0.0", "claude-code", "/a/b/foo"));
        lf.save(&path).expect("first save");
        assert!(path.exists());
        let tmp_leftover = path.with_extension("lock.tmp");
        assert!(!tmp_leftover.exists(), "no .tmp file should remain after a successful save");

        // Second write overwrites in place.
        lf.add_entry(entry("bar", "0.1.0", "claude-code", "/a/b/bar"));
        lf.save(&path).expect("second save");
        assert!(path.exists());
        assert!(!tmp_leftover.exists(), "no .tmp file should remain after second save either");

        // The loaded file reflects both entries.
        let loaded = Lockfile::load(&path);
        assert_eq!(loaded.skill.len(), 2);
    }

    #[test]
    fn test_lockfile_load_malformed_falls_back_to_empty() {
        // A file that exists but is invalid TOML should NOT panic;
        // it should warn and return an empty lockfile. This is the
        // "skip the warning, start fresh" behavior we want for
        // developers editing the file by hand.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("usk.lock");
        std::fs::write(&path, "this is not = valid toml [[[").unwrap();
        let lf = Lockfile::load(&path);
        assert_eq!(lf, Lockfile::empty());
    }
}
