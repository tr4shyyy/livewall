use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};

pub fn app_root_dir() -> Result<PathBuf> {
    let exe = env::current_exe().context("failed to resolve current executable path")?;
    exe.parent()
        .map(PathBuf::from)
        .context("failed to resolve executable directory")
}
