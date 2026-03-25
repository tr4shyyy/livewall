use std::path::Path;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, Result, anyhow};
use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, NIN_SELECT, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetCursorPos, HICON, IDI_APPLICATION, IMAGE_ICON, LR_LOADFROMFILE, LoadIconW, LoadImageW,
    MF_DISABLED, MF_GRAYED, MF_SEPARATOR, MF_STRING, PostQuitMessage, RegisterClassW,
    SetForegroundWindow, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, TrackPopupMenu,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_COMMAND, WM_CONTEXTMENU, WM_DESTROY,
    WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

use crate::paths::app_root_dir;

const APP_NAME: &str = "Live Wall";

const TRAY_ICON_UID: u32 = 1;
const WM_TRAYICON: u32 = WM_APP + 1;
const IDM_STATUS: usize = 1001;
const IDM_CHOOSE_VIDEO: usize = 1002;
const IDM_NEXT_VIDEO: usize = 1003;
const IDM_EDIT_CONFIG: usize = 1004;
const IDM_QUIT: usize = 1005;

static PLAYBACK_STATUS: AtomicU32 = AtomicU32::new(0);
static PENDING_ACTION: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    ChooseVideo,
    NextVideo,
    EditConfig,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayPlaybackStatus {
    Running,
    PausedFullscreen,
    PausedWatchedProcess,
}

impl TrayPlaybackStatus {
    fn as_label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::PausedFullscreen => "Paused: fullscreen app",
            Self::PausedWatchedProcess => "Paused: watched app",
        }
    }

    fn as_tip(self) -> &'static str {
        match self {
            Self::Running => "Live Wall: running",
            Self::PausedFullscreen => "Live Wall: paused for fullscreen app",
            Self::PausedWatchedProcess => "Live Wall: paused for watched app",
        }
    }

    fn into_raw(self) -> u32 {
        match self {
            Self::Running => 0,
            Self::PausedFullscreen => 1,
            Self::PausedWatchedProcess => 2,
        }
    }

    fn from_raw(raw: u32) -> Self {
        match raw {
            1 => Self::PausedFullscreen,
            2 => Self::PausedWatchedProcess,
            _ => Self::Running,
        }
    }
}

pub struct TrayIcon {
    hwnd: HWND,
}

impl TrayIcon {
    pub fn create() -> Result<Self> {
        PLAYBACK_STATUS.store(TrayPlaybackStatus::Running.into_raw(), Ordering::SeqCst);
        PENDING_ACTION.store(0, Ordering::SeqCst);

        let class_name = w!("LiveWallTrayWindow");
        let hinstance = unsafe { GetModuleHandleW(None) }.context("failed to get module handle")?;

        let wc = WNDCLASSW {
            lpfnWndProc: Some(tray_wndproc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        unsafe {
            let _ = RegisterClassW(&wc);
        }

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!("Live Wall tray"),
                WINDOW_STYLE(WS_OVERLAPPED.0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(windows::Win32::Foundation::HINSTANCE(hinstance.0)),
                Some(null_mut()),
            )
        }
        .context("failed to create tray window")?;

        let tray = Self { hwnd };
        tray.install_icon(TrayPlaybackStatus::Running)?;
        Ok(tray)
    }

    pub fn set_status(&self, status: TrayPlaybackStatus) -> Result<()> {
        PLAYBACK_STATUS.store(status.into_raw(), Ordering::SeqCst);
        let mut nid = self.notify_icon_data(status.as_tip());
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &mut nid).ok()?;
        }
        Ok(())
    }

    pub fn take_action(&self) -> Option<TrayAction> {
        match PENDING_ACTION.swap(0, Ordering::SeqCst) {
            1 => Some(TrayAction::ChooseVideo),
            2 => Some(TrayAction::NextVideo),
            3 => Some(TrayAction::EditConfig),
            4 => Some(TrayAction::Quit),
            _ => None,
        }
    }

    fn install_icon(&self, status: TrayPlaybackStatus) -> Result<()> {
        let mut nid = self.notify_icon_data(status.as_tip());
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &mut nid)
                .ok()
                .context("failed to add tray icon")?;
        }
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        unsafe {
            Shell_NotifyIconW(NIM_SETVERSION, &mut nid)
                .ok()
                .context("failed to set tray icon version")?;
        }
        Ok(())
    }

    fn notify_icon_data(&self, tip: &str) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd;
        nid.uID = TRAY_ICON_UID;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAYICON;
        nid.hIcon = load_default_icon().unwrap_or_default();
        write_wide_truncated(&mut nid.szTip, tip);
        nid
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd;
        nid.uID = TRAY_ICON_UID;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &mut nid);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn tray_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TRAYICON => {
            let event = (lparam.0 as u32) & 0xffff;
            if event == WM_RBUTTONUP
                || event == WM_LBUTTONUP
                || event == WM_CONTEXTMENU
                || event == NIN_SELECT
            {
                let _ = show_context_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let command_id = wparam.0 & 0xffff;
            match command_id {
                IDM_CHOOSE_VIDEO => {
                    PENDING_ACTION.store(1, Ordering::SeqCst);
                }
                IDM_NEXT_VIDEO => {
                    PENDING_ACTION.store(2, Ordering::SeqCst);
                }
                IDM_EDIT_CONFIG => {
                    PENDING_ACTION.store(3, Ordering::SeqCst);
                }
                IDM_QUIT => {
                    PENDING_ACTION.store(4, Ordering::SeqCst);
                    unsafe {
                        PostQuitMessage(0);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn show_context_menu(hwnd: HWND) -> Result<()> {
    let menu = unsafe { CreatePopupMenu() }.context("failed to create tray menu")?;
    let status = TrayPlaybackStatus::from_raw(PLAYBACK_STATUS.load(Ordering::SeqCst));
    let status_text = wide_null(format!("{APP_NAME}: {}", status.as_label()));
    let choose_video_text = wide_null("Choose Video...");
    let next_video_text = wide_null("Next Video");
    let edit_config_text = wide_null("Edit Config");
    let quit_text = wide_null("Quit");

    unsafe {
        AppendMenuW(
            menu,
            MF_STRING | MF_DISABLED | MF_GRAYED,
            IDM_STATUS,
            PCWSTR(status_text.as_ptr()),
        )
        .context("failed to add tray status item")?;
        AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null())
            .context("failed to add tray separator")?;
        AppendMenuW(menu, MF_STRING, IDM_CHOOSE_VIDEO, PCWSTR(choose_video_text.as_ptr()))
            .context("failed to add choose video item")?;
        AppendMenuW(menu, MF_STRING, IDM_NEXT_VIDEO, PCWSTR(next_video_text.as_ptr()))
            .context("failed to add next video item")?;
        AppendMenuW(menu, MF_STRING, IDM_EDIT_CONFIG, PCWSTR(edit_config_text.as_ptr()))
            .context("failed to add edit config item")?;
        AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null())
            .context("failed to add tray separator")?;
        AppendMenuW(menu, MF_STRING, IDM_QUIT, PCWSTR(quit_text.as_ptr()))
            .context("failed to add tray quit item")?;
    }

    let result = (|| -> Result<()> {
        let mut point = POINT::default();
        unsafe {
            GetCursorPos(&mut point).context("failed to read cursor position")?;
            let _ = SetForegroundWindow(hwnd);
            if !TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                Some(0),
                hwnd,
                None,
            )
            .as_bool()
            {
                return Err(anyhow!("failed to show tray menu"));
            }
        }
        Ok(())
    })();

    unsafe {
        let _ = DestroyMenu(menu);
    }

    result
}

fn load_default_icon() -> Result<HICON> {
    let root = app_root_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    for icon_path in [root.join("tray_icon.ico"), root.join("icon.ico")] {
        if icon_path.exists() {
            let wide_path = wide_null(icon_path.to_string_lossy());
            unsafe {
                if let Ok(handle) = LoadImageW(
                    None,
                    PCWSTR(wide_path.as_ptr()),
                    IMAGE_ICON,
                    0,
                    0,
                    LR_LOADFROMFILE,
                ) {
                    if !handle.is_invalid() {
                        return Ok(HICON(handle.0));
                    }
                }
            }
        }
    }

    unsafe { LoadIconW(None, IDI_APPLICATION) }.context("failed to load default tray icon")
}

fn write_wide_truncated(buffer: &mut [u16], text: &str) {
    if buffer.is_empty() {
        return;
    }
    let max_len = buffer.len().saturating_sub(1);

    for slot in buffer.iter_mut() {
        *slot = 0;
    }

    for (dst, src) in buffer.iter_mut().take(max_len).zip(text.encode_utf16()) {
        *dst = src;
    }
}

fn wide_null(text: impl AsRef<str>) -> Vec<u16> {
    text.as_ref().encode_utf16().chain(std::iter::once(0)).collect()
}
