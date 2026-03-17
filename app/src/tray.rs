use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

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
    GetCursorPos, HICON, IDI_APPLICATION, LoadIconW, MF_DISABLED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, PostQuitMessage, RegisterClassW, SetForegroundWindow, TPM_BOTTOMALIGN,
    TPM_LEFTALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_COMMAND, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

const TRAY_ICON_UID: u32 = 1;
const WM_TRAYICON: u32 = WM_APP + 1;
const IDM_STATUS: usize = 1001;
const IDM_QUIT: usize = 1002;

static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static PLAYBACK_STATUS: AtomicU32 = AtomicU32::new(0);

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
            Self::Running => "weebp-rs: running",
            Self::PausedFullscreen => "weebp-rs: paused for fullscreen app",
            Self::PausedWatchedProcess => "weebp-rs: paused for watched app",
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
        QUIT_REQUESTED.store(false, Ordering::SeqCst);
        PLAYBACK_STATUS.store(TrayPlaybackStatus::Running.into_raw(), Ordering::SeqCst);

        let class_name = w!("WeebpRsTrayWindow");
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
                w!("weebp-rs tray"),
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

    pub fn should_quit(&self) -> bool {
        QUIT_REQUESTED.load(Ordering::SeqCst)
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
            if command_id == IDM_QUIT {
                QUIT_REQUESTED.store(true, Ordering::SeqCst);
                unsafe {
                    PostQuitMessage(0);
                }
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
    let status_text = wide_null(format!("Status: {}", status.as_label()));
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
