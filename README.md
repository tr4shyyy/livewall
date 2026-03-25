# Live Wall

![Live Wall Icon](./icon.png)

Lightweight Windows live wallpaper app built in Rust.

## Current Features

- embeds the wallpaper behind desktop icons
- uses `mpv` for local video wallpapers
- tray icon with:
  - `Choose Video...`
  - `Next Video`
  - `Edit Config`
  - `Quit`
- folder-driven playlist rotation from `PLAYLIST`
- hour-based rotation intervals
- pauses automatically for fullscreen apps
- pauses automatically for watched apps like `VALORANT-Win64-Shipping.exe`
- resumes automatically when those conditions clear
- runs as a no-console Windows app when built normally

## Project Layout

- Rust app source: [app](C:/Users/nolyn/live-wall/app)
- playlist folder: [PLAYLIST](C:/Users/nolyn/live-wall/PLAYLIST)
- release build script: [build-release.ps1](C:/Users/nolyn/live-wall/build-release.ps1)
- runtime config: [config.json](C:/Users/nolyn/AppData/Roaming/nolyn/live-wall/config/config.json)

## Run

```powershell
cargo run
```

## Build

Normal release build:

```powershell
cargo build --release
```

Release build with `icon.ico` linked into the final executable:

```powershell
.\build-release.ps1
```

Release package with the executable plus required runtime files:

```powershell
.\build-release-package.ps1
```

Output:

```text
target\release\live-wall.exe
dist\Live Wall v0.1.0
dist\Live Wall v0.1.0-windows-x64.zip
```

## Config

Example:

```json
{
  "wallpaper": {
    "url": "file:///C:/Users/nolyn/live-wall/PLAYLIST/omeba.mp4",
    "switch_interval_hours": 6
  },
  "pause": {
    "poll_interval_ms": 1000,
      "pause_on_fullscreen": true,
      "watched_processes": [
        {
        "process_name": "VALORANT-Win64-Shipping.exe",
          "match_mode": "exact"
        },
        {
        "process_name": "obs64.exe",
        "match_mode": "exact"
      }
    ]
  }
}
```

Notes:

- `switch_interval_hours` can be values like `1`, `6`, `12`, or `24`
- playlist contents come from whatever supported video files are in [PLAYLIST](C:/Users/nolyn/live-wall/PLAYLIST)
- supported video types are `.mp4`, `.mkv`, `.webm`, `.mov`
- `match_mode` supports `exact` and `contains`
- if your existing config still says `valorant.exe`, change it to `VALORANT-Win64-Shipping.exe`

### Pause On Specific Processes

Use `pause.watched_processes` to pause playback whenever one of the listed apps is running.

Example:

```json
{
  "pause": {
    "poll_interval_ms": 1000,
    "pause_on_fullscreen": true,
    "watched_processes": [
      {
        "process_name": "VALORANT-Win64-Shipping.exe",
        "match_mode": "exact"
      },
      {
        "process_name": "obs64.exe",
        "match_mode": "exact"
      },
      {
        "process_name": "Discord",
        "match_mode": "contains"
      }
    ]
  }
}
```

Notes:

- use `exact` when you know the full executable name, like `VALORANT-Win64-Shipping.exe`
- use `contains` when you want a broader match against the process name
- playback resumes automatically when none of the watched processes are active

## Assets

- app icon source: [icon.png](C:/Users/nolyn/live-wall/icon.png)
- tray icon source: [tray_icon.png](C:/Users/nolyn/live-wall/tray_icon.png)
- compiled tray icon: [tray_icon.ico](C:/Users/nolyn/live-wall/tray_icon.ico)
- compiled app icon: [icon.ico](C:/Users/nolyn/live-wall/icon.ico)

## Notes

- release builds should be shipped with `wallpaper.html`, `tray_icon.ico`, `icon.ico`, `PLAYLIST`, and the bundled `mpv` runtime folder next to the executable
- local video playback expects the bundled `mpv` runtime folder to be present next to the executable
- the tray uses `tray_icon.ico` first and falls back to `icon.ico`
- the runtime config opens through your default editor from the tray
