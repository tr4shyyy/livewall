#![windows_subsystem = "windows"]

mod config;
mod monitor;
mod mpv;
mod picker;
mod tray;
mod wallpaper;

use std::time::{Duration, Instant};
use std::path::PathBuf;

use anyhow::Result;
use monitor::{PauseReason, PlaybackDirective};
use tray::{TrayAction, TrayIcon, TrayPlaybackStatus};
use tracing::{info, warn};
use wallpaper::{LoopFlow, WallpaperApp};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "live_wall=info".into()),
        )
        .init();

    let (mut config, path) = config::AppConfig::load_or_create()?;
    let mut playlist_urls = config::playlist_urls_from_directory()?;
    if !playlist_urls.is_empty() && !playlist_urls.contains(&config.wallpaper.url) {
        config.wallpaper.url = playlist_urls[0].clone();
    }
    info!("loaded config from {}", path.display());

    let mut app = WallpaperApp::create(&config.wallpaper.url)?;
    app.resize_to_parent()?;
    let tray = TrayIcon::create()?;

    let mut last_state = PlaybackDirective::Run;
    let mut last_poll = Instant::now() - Duration::from_millis(config.pause.poll_interval_ms);
    let mut last_playlist_switch = Instant::now();

    app.message_loop(|app| {
        app.refresh_input_passthrough();

        if let Some(action) = tray.take_action() {
            match action {
                TrayAction::ChooseVideo => {
                    if let Some(file) = picker::choose_video_file(&default_video_dir())? {
                        config.set_video_path(&file)?;
                        config.save_to(&path)?;
                        app.navigate(&config.wallpaper.url)?;
                        info!("switched wallpaper to {}", file.display());
                        playlist_urls.clear();
                        last_state = PlaybackDirective::Run;
                        last_playlist_switch = Instant::now();
                    }
                }
                TrayAction::NextVideo => {
                    playlist_urls = config::playlist_urls_from_directory()?;
                    if playlist_urls.len() >= 2 {
                        let current_index = playlist_urls
                            .iter()
                            .position(|entry| entry == config.active_wallpaper_url())
                            .unwrap_or(0);
                        let next_index = (current_index + 1) % playlist_urls.len();
                        config.wallpaper.url = playlist_urls[next_index].clone();
                        app.navigate(config.active_wallpaper_url())?;
                        config.save_to(&path)?;
                        info!("skipped to next playlist wallpaper {}", config.active_wallpaper_url());
                        last_playlist_switch = Instant::now();
                    }
                }
                TrayAction::EditConfig => {
                    config::open_in_editor(&path)?;
                }
                TrayAction::Quit => return Ok(LoopFlow::Exit),
            }
        }

        if last_poll.elapsed() < Duration::from_millis(config.pause.poll_interval_ms) {
            return Ok(LoopFlow::Continue);
        }
        last_poll = Instant::now();

        let next_state = monitor::evaluate(&config.pause);
        if next_state != last_state {
            match next_state {
                PlaybackDirective::Run => info!("resuming wallpaper playback"),
                PlaybackDirective::Pause(PauseReason::Fullscreen) => {
                    info!("pausing wallpaper because a fullscreen window is active")
                }
                PlaybackDirective::Pause(PauseReason::WatchedProcess) => {
                    info!("pausing wallpaper because a watched process is active")
                }
            }

            app.set_paused(!matches!(next_state, PlaybackDirective::Run))?;
            tray.set_status(match next_state {
                PlaybackDirective::Run => TrayPlaybackStatus::Running,
                PlaybackDirective::Pause(PauseReason::Fullscreen) => {
                    TrayPlaybackStatus::PausedFullscreen
                }
                PlaybackDirective::Pause(PauseReason::WatchedProcess) => {
                    TrayPlaybackStatus::PausedWatchedProcess
                }
            })?;
            last_state = next_state;
        }

        if matches!(last_state, PlaybackDirective::Run) {
            if let Some(interval) = config.playlist_interval() {
                if last_playlist_switch.elapsed() >= interval {
                    playlist_urls = config::playlist_urls_from_directory()?;
                    if playlist_urls.len() >= 2 {
                        let current_index = playlist_urls
                            .iter()
                            .position(|entry| entry == config.active_wallpaper_url())
                            .unwrap_or(0);
                        let next_index = (current_index + 1) % playlist_urls.len();
                        config.wallpaper.url = playlist_urls[next_index].clone();
                        app.navigate(config.active_wallpaper_url())?;
                        config.save_to(&path)?;
                        info!("switched playlist wallpaper to {}", config.active_wallpaper_url());
                        last_playlist_switch = Instant::now();
                    }
                }
            }
        }

        Ok(LoopFlow::Continue)
    })?;

    warn!("wallpaper host exited");
    Ok(())
}

fn default_video_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
