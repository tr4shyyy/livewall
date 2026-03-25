use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use url::Url;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::paths::app_root_dir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub wallpaper: WallpaperSource,
    pub pause: PauseConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallpaperSource {
    pub url: String,
    #[serde(default, skip_serializing)]
    pub playlist: Vec<String>,
    #[serde(default)]
    pub switch_interval_hours: Option<u64>,
    #[serde(default)]
    pub switch_interval_seconds: Option<u64>,
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
                playlist: Vec::new(),
                switch_interval_hours: None,
                switch_interval_seconds: None,
            },
            pause: PauseConfig {
                poll_interval_ms: 1000,
                pause_on_fullscreen: true,
                watched_processes: vec![
                    WatchedProcess {
                        process_name: "VALORANT-Win64-Shipping.exe".to_string(),
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

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn set_video_path(&mut self, video_path: &Path) -> Result<()> {
        let video = video_path
            .canonicalize()
            .with_context(|| format!("failed to resolve {}", video_path.display()))?;
        self.wallpaper.url = Url::from_file_path(&video)
            .map(|url| url.to_string())
            .map_err(|()| anyhow::anyhow!("failed to convert {} to a file URL", video.display()))?;
        self.wallpaper.playlist.clear();
        self.wallpaper.switch_interval_hours = None;
        self.wallpaper.switch_interval_seconds = None;
        Ok(())
    }

    pub fn active_wallpaper_url(&self) -> &str {
        &self.wallpaper.url
    }

    pub fn playlist_interval(&self) -> Option<Duration> {
        if let Some(hours) = self.wallpaper.switch_interval_hours {
            if hours > 0 {
                return Some(Duration::from_secs(hours.saturating_mul(60 * 60)));
            }
        }
        if let Some(seconds) = self.wallpaper.switch_interval_seconds {
            if seconds > 0 {
                return Some(Duration::from_secs(seconds));
            }
        }
        None
    }
}

pub fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("com", "nolyn", "live-wall")
        .context("failed to resolve project directories")
}

pub fn config_path() -> Result<PathBuf> {
    Ok(project_dirs()?.config_dir().join("config.json"))
}

fn default_wallpaper_url() -> Result<String> {
    wallpaper_page_url()
}

fn wallpaper_page_url() -> Result<String> {
    let asset = app_root_dir()?.join("wallpaper.html");
    let canonical = asset
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", asset.display()))?;
    Url::from_file_path(&canonical)
        .map(|url| url.to_string())
        .map_err(|()| anyhow::anyhow!("failed to convert {} to a file URL", canonical.display()))
}

pub fn local_video_path_from_wallpaper_url(url: &str) -> Option<PathBuf> {
    let parsed = Url::parse(url).ok()?;
    if parsed.scheme() == "file" {
        let path = parsed.to_file_path().ok()?;
        if is_video_path(&path) {
            return Some(path);
        }
    }

    let video_param = parsed
        .query_pairs()
        .find(|(key, _)| key == "video")
        .map(|(_, value)| value.to_string())?;
    let parsed_video = Url::parse(&video_param).ok()?;
    if parsed_video.scheme() != "file" {
        return None;
    }

    let path = parsed_video.to_file_path().ok()?;
    if is_video_path(&path) {
        Some(path)
    } else {
        None
    }
}

fn is_video_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("mp4" | "mkv" | "webm" | "mov")
    )
}

pub fn playlist_directory() -> PathBuf {
    app_root_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("PLAYLIST")
}

pub fn playlist_urls_from_directory() -> Result<Vec<String>> {
    let dir = playlist_directory();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && is_video_path(&path) {
            files.push(path);
        }
    }

    files.sort();

    files
        .into_iter()
        .map(|path| {
            let canonical = path
                .canonicalize()
                .with_context(|| format!("failed to resolve {}", path.display()))?;
            Url::from_file_path(&canonical)
                .map(|url| url.to_string())
                .map_err(|()| anyhow::anyhow!("failed to convert {} to a file URL", canonical.display()))
        })
        .collect()
}

pub fn open_in_editor(path: &Path) -> Result<()> {
    let wide_path: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let result = ShellExecuteW(
            Some(HWND(std::ptr::null_mut())),
            PCWSTR::null(),
            PCWSTR(wide_path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );

        if result.0 as usize <= 32 {
            anyhow::bail!("failed to open {} with the default app", path.display());
        }
    }

    Ok(())
}
