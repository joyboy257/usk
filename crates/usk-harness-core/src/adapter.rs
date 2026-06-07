use std::path::Path;
use crate::error::Result;
use usk_core::schema::Skill;

pub trait HarnessAdapter: Send + Sync {
    fn name(&self) -> &str;

    fn convert(&self, skill: &Skill, source_dir: &Path, output_dir: &Path) -> Result<()>;

    fn install_path(&self) -> Option<std::path::PathBuf> {
        None
    }

    fn supported_versions(&self) -> &[semver::VersionReq] {
        &[]
    }
}
