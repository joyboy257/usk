use std::path::{Path, PathBuf};

use crate::error::{HarnessError, Result};
use usk_core::schema::Skill;

/// The canonical trait every harness adapter implements.
///
/// In v0.2 (U4) the trait gained two mandatory methods — `enable` and
/// `disable` — so the CLI can offer a uniform `usk enable <name>` /
/// `usk disable <name>` surface across all harnesses. Each adapter
/// chooses its own filesystem primitive (symlink, manifest flag, etc.)
/// but the user-facing contract is the same.
///
/// Implementations must be `Send + Sync` so the CLI can hold them in a
/// `Box<dyn HarnessAdapter>` and pass them across async boundaries.
pub trait HarnessAdapter: Send + Sync {
    /// The harness key as it appears in `skill.yaml` and on the CLI
    /// (e.g. `"claude-code"`, `"codex-cli"`).
    fn name(&self) -> &str;

    /// Render the skill into the harness's native install path.
    ///
    /// `output_dir` is the directory the harness will read the skill
    /// from. The adapter is responsible for writing whatever file
    /// shape its harness expects (`SKILL.md`, `agent.yaml`, ...).
    fn convert(&self, skill: &Skill, source_dir: &Path, output_dir: &Path) -> Result<()>;

    /// The root directory under which the harness expects per-skill
    /// entries (e.g. `~/.claude/skills/`). `None` for harnesses that
    /// have no fixed root.
    ///
    /// Concrete adapters must override this.
    fn install_root(&self) -> Option<PathBuf>;

    /// The per-skill projection path the harness reads from. Most
    /// adapters get this for free from `install_root`; overrides are
    /// for harnesses with non-standard layouts (Codex's `<name>.yaml`
    /// file rather than a `<name>/` directory, for example).
    fn install_path(&self, name: &str) -> Option<PathBuf> {
        self.install_root().map(|root| root.join(name))
    }

    /// Make the skill visible to the harness. The adapter's
    /// `store_root` is `~/.usk/store/<harness>/`; the per-skill source
    /// of truth lives at `store_root.join(name)`. The default
    /// implementation symlinks the harness's `install_path(name)` to
    /// `store_root.join(name)` — the canonical projection shape.
    ///
    /// Adapters whose projection is a single file (e.g. Codex) may
    /// override this to point at `store_root.join(name).join(<file>)`
    /// instead of the directory.
    fn enable(&self, name: &str, store_root: &Path) -> Result<()> {
        let target = self
            .install_path(name)
            .ok_or_else(|| HarnessError::ConversionError(format!(
                "harness '{}' has no install_path for '{}'",
                self.name(),
                name
            )))?;
        let source = store_root.join(name);

        // Idempotent: if the projection already points at the right
        // place, do nothing.
        if target.is_symlink() {
            if let Ok(existing) = std::fs::read_link(&target) {
                if existing == source {
                    return Ok(());
                }
                return Err(HarnessError::ConversionError(format!(
                    "{} already exists and points to {} (expected {})",
                    target.display(),
                    existing.display(),
                    source.display()
                )));
            }
        }
        if target.exists() {
            return Err(HarnessError::ConversionError(format!(
                "{} exists and is not a symlink; remove it before enabling",
                target.display()
            )));
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::os::unix::fs::symlink(&source, &target)?;
        Ok(())
    }

    /// Hide the skill from the harness without removing its files.
    /// The store entry is untouched; only the projection goes away.
    /// Idempotent: no error if the projection is already absent.
    fn disable(&self, name: &str) -> Result<()> {
        let target = match self.install_path(name) {
            Some(p) => p,
            None => return Ok(()),
        };
        if target.is_symlink() {
            std::fs::remove_file(&target)?;
        }
        Ok(())
    }

    /// Semver version requirements this adapter is known to work
    /// with. The default is an empty set (any version).
    fn supported_versions(&self) -> &[semver::VersionReq] {
        &[]
    }
}
