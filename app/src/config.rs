use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub wallpaper: WallpaperSource,
    pub pause: PauseConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallpaperSource {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PauseConfig {
    pub poll_interval_ms: u64,
    pub pause_on_fullscreen: bool,
    pub watched_processes: Vec<WatchedProcess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchedProcess {
    pub process_name: String,
    #[serde(default)]
    pub match_mode: ProcessMatchMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProcessMatchMode {
    #[default]
    Exact,
    Contains,
}

impl AppConfig {
    pub fn load_or_create() -> Result<(Self, PathBuf)> {
        let path = config_path()?;
        if path.exists() {
            let contents = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let config = serde_json::from_str::<Self>(&contents)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            return Ok((config, path));
        }

        let config = Self::default_with_asset()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let contents = serde_json::to_string_pretty(&config)?;
        fs::write(&path, contents)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok((config, path))
    }

    fn default_with_asset() -> Result<Self> {
        Ok(Self {
            wallpaper: WallpaperSource {
                url: default_wallpaper_url()?,
            },
            pause: PauseConfig {
                poll_interval_ms: 1000,
                pause_on_fullscreen: true,
                watched_processes: vec![
                    WatchedProcess {
                        process_name: "valorant.exe".to_string(),
                        match_mode: ProcessMatchMode::Exact,
                    },
                    WatchedProcess {
                        process_name: "obs64.exe".to_string(),
                        match_mode: ProcessMatchMode::Exact,
                    },
                ],
            },
        })
    }
}

pub fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("com", "nolyn", "weebp-rs")
        .context("failed to resolve project directories")
}

pub fn config_path() -> Result<PathBuf> {
    Ok(project_dirs()?.config_dir().join("config.json"))
}

fn default_wallpaper_url() -> Result<String> {
    let asset = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("app")
        .join("assets")
        .join("wallpaper.html");
    let canonical = asset
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", asset.display()))?;
    Url::from_file_path(&canonical)
        .map(|url| url.to_string())
        .map_err(|()| anyhow::anyhow!("failed to convert {} to a file URL", canonical.display()))
}
