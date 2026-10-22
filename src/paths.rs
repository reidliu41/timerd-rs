use std::env;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;

use anyhow::{Context, Result};
use nix::unistd::Uid;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub socket: PathBuf,
    pub state: PathBuf,
    pub daemon_log: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let runtime_base = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(format!("/tmp/timer-rs-{}", Uid::effective())));
        let runtime_dir = runtime_base.join("timer-rs");

        let state_base = if let Some(path) = env::var_os("XDG_STATE_HOME") {
            PathBuf::from(path)
        } else {
            let home = env::var_os("HOME").context("HOME is not set")?;
            PathBuf::from(home).join(".local/state")
        };
        let state_dir = state_base.join("timer-rs");

        create_private_dir(&runtime_dir)?;
        create_private_dir(&state_dir)?;

        Ok(Self {
            socket: runtime_dir.join("timer.sock"),
            state: state_dir.join("state.json"),
            daemon_log: state_dir.join("daemon.log"),
        })
    }
}

fn create_private_dir(path: &std::path::Path) -> Result<()> {
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create directory {}", path.display()))?;
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect directory {}", path.display()))?;
    if !metadata.file_type().is_dir() || metadata.uid() != Uid::effective().as_raw() {
        anyhow::bail!(
            "refusing to use untrusted directory {} (it must be a real directory owned by the current user)",
            path.display()
        );
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("failed to protect directory {}", path.display()))?;
    Ok(())
}
