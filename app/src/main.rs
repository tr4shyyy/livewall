mod config;
mod monitor;
mod tray;
mod wallpaper;

use std::time::{Duration, Instant};

use anyhow::Result;
use monitor::{PauseReason, PlaybackDirective};
use tray::{TrayIcon, TrayPlaybackStatus};
use tracing::{info, warn};
use wallpaper::{LoopFlow, WallpaperApp};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "weebp_rs=info".into()),
        )
        .init();

    let (config, path) = config::AppConfig::load_or_create()?;
    info!("loaded config from {}", path.display());

    let app = WallpaperApp::create(&config.wallpaper.url)?;
    app.resize_to_parent()?;
    let tray = TrayIcon::create()?;

    let mut last_state = PlaybackDirective::Run;
    let mut last_poll = Instant::now() - Duration::from_millis(config.pause.poll_interval_ms);

    app.message_loop(|| {
        if tray.should_quit() {
            return Ok(LoopFlow::Exit);
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

        Ok(LoopFlow::Continue)
    })?;

    warn!("wallpaper host exited");
    Ok(())
}
