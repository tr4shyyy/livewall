use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use crate::config;
use crate::paths::app_root_dir;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;

pub struct MpvPlayer {
    child: Child,
    ipc_pipe: String,
    host_hwnd: HWND,
}

impl MpvPlayer {
    pub fn create(host_hwnd: HWND, video_path: &Path) -> Result<Self> {
        let runtime_dir = find_runtime_dir()?;
        let mpv_exe = runtime_dir.join("mpv.exe");
        if !mpv_exe.exists() {
            return Err(anyhow!("failed to find mpv.exe in {}", runtime_dir.display()));
        }
        let mpv_log = open_mpv_log_file()?;

        let ipc_pipe = format!(r"\\.\pipe\live-wall-mpv-{}", std::process::id());
        let wid = (host_hwnd.0 as usize).to_string();

        let child = Command::new(&mpv_exe)
            .current_dir(&runtime_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(mpv_log))
            .args([
                "--no-config",
                "--force-window=yes",
                "--idle=yes",
                "--keep-open=yes",
                "--loop-file=inf",
                "--mute=yes",
                "--no-osc",
                "--no-input-default-bindings",
                "--hwdec=no",
                "--profile=fast",
                "--msg-level=all=info",
                "--ontop=no",
                &format!("--wid={wid}"),
                &format!("--input-ipc-server={ipc_pipe}"),
            ])
            .arg(video_path)
            .spawn()
            .context("failed to launch mpv")?;

        let player = Self {
            child,
            ipc_pipe,
            host_hwnd,
        };
        player.wait_for_pipe()?;
        player.refresh_input_passthrough();
        Ok(player)
    }

    pub fn set_paused(&mut self, paused: bool) -> Result<()> {
        let command = if paused {
            r#"{ "command": ["set_property", "pause", true] }"#
        } else {
            r#"{ "command": ["set_property", "pause", false] }"#
        };
        self.send_ipc(command)
    }

    pub fn load_file(&mut self, video_path: &Path) -> Result<()> {
        let command = format!(
            r#"{{ "command": ["loadfile", "{}", "replace"] }}"#,
            json_escape(&video_path.to_string_lossy())
        );
        self.send_ipc(&command)?;
        self.refresh_input_passthrough();
        Ok(())
    }

    pub fn refresh_input_passthrough(&self) {
        apply_click_through_to_children(self.host_hwnd);
    }

    fn wait_for_pipe(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if OpenOptions::new().write(true).open(&self.ipc_pipe).is_ok() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }

        Err(anyhow!("mpv IPC pipe did not become available"))
    }

    fn send_ipc(&mut self, command: &str) -> Result<()> {
        let mut pipe = OpenOptions::new()
            .write(true)
            .open(&self.ipc_pipe)
            .with_context(|| format!("failed to open mpv IPC pipe {}", self.ipc_pipe))?;
        pipe.write_all(command.as_bytes())
            .context("failed to write mpv IPC command")?;
        pipe.write_all(b"\n")
            .context("failed to finalize mpv IPC command")?;
        Ok(())
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        let _ = self.send_ipc(r#"{ "command": ["quit"] }"#);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn find_runtime_dir() -> Result<PathBuf> {
    let root = app_root_dir()?;
    find_file(&root, OsStr::new("mpv.exe"))
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .ok_or_else(|| anyhow!("failed to find mpv.exe under {}", root.display()))
}

fn find_file(root: &Path, file_name: &OsStr) -> Option<PathBuf> {
    for entry in std::fs::read_dir(root).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.is_file() && path.file_name() == Some(file_name) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_file(&path, file_name) {
                return Some(found);
            }
        }
    }
    None
}

fn json_escape(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn open_mpv_log_file() -> Result<File> {
    let dir = config::project_dirs()?.data_local_dir().join("logs");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}", dir.display()))?;

    let path = dir.join("mpv.log");
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open {}", path.display()))
}

fn apply_click_through_to_children(host_hwnd: HWND) {
    unsafe extern "system" fn child_enum(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        unsafe {
            let _ = EnableWindow(hwnd, false);
        }
        BOOL(1)
    }

    unsafe {
        let _ = EnableWindow(host_hwnd, false);
        let _ = EnumChildWindows(Some(host_hwnd), Some(child_enum), LPARAM(0));
    }
}
