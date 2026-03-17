use std::ffi::OsString;
use std::mem::size_of;
use std::os::windows::ffi::OsStringExt;

use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, MAX_PATH, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, MonitorFromWindow,
};
use windows::Win32::System::ProcessStatus::K32GetModuleBaseNameW;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetForegroundWindow, GetWindowLongW, GetWindowRect,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, WS_EX_TOOLWINDOW,
};

use crate::config::{PauseConfig, ProcessMatchMode, WatchedProcess};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseReason {
    Fullscreen,
    WatchedProcess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackDirective {
    Run,
    Pause(PauseReason),
}

pub fn evaluate(config: &PauseConfig) -> PlaybackDirective {
    if process_match(&config.watched_processes) {
        return PlaybackDirective::Pause(PauseReason::WatchedProcess);
    }

    if config.pause_on_fullscreen && fullscreen_foreground_window() {
        return PlaybackDirective::Pause(PauseReason::Fullscreen);
    }

    PlaybackDirective::Run
}

fn process_match(processes: &[WatchedProcess]) -> bool {
    if processes.is_empty() {
        return false;
    }

    let mut state = ProcessScanState {
        watched: processes,
        matched: false,
    };

    unsafe {
        let _ = EnumWindows(
            Some(enum_process_windows),
            LPARAM((&mut state as *mut ProcessScanState).cast::<core::ffi::c_void>() as isize),
        );
    }

    state.matched
}

unsafe extern "system" fn enum_process_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = unsafe { &mut *(lparam.0 as *mut ProcessScanState) };
    if state.matched {
        return BOOL(0);
    }

    if !window_counts_as_running_app(hwnd) {
        return BOOL(1);
    }

    if let Some(process_name) = process_name_for_window(hwnd) {
        state.matched = state.watched.iter().any(|rule| match rule.match_mode {
            ProcessMatchMode::Exact => process_name.eq_ignore_ascii_case(&rule.process_name),
            ProcessMatchMode::Contains => process_name
                .to_ascii_lowercase()
                .contains(&rule.process_name.to_ascii_lowercase()),
        });
    }

    BOOL((!state.matched) as i32)
}

fn fullscreen_foreground_window() -> bool {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() || !window_counts_as_running_app(hwnd) {
        return false;
    }

    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return false;
    }

    let window_rect = unsafe {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }
        rect
    };

    let monitor_rect = match monitor_rect(monitor) {
        Some(rect) => rect,
        None => return false,
    };

    window_rect.left <= monitor_rect.left
        && window_rect.top <= monitor_rect.top
        && window_rect.right >= monitor_rect.right
        && window_rect.bottom >= monitor_rect.bottom
}

fn monitor_rect(monitor: HMONITOR) -> Option<windows::Win32::Foundation::RECT> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };

    unsafe { GetMonitorInfoW(monitor, &mut info).ok().ok()? };
    Some(info.rcMonitor)
}

fn window_counts_as_running_app(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return false;
        }

        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
    }

    true
}

fn process_name_for_window(hwnd: HWND) -> Option<String> {
    let mut process_id = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    }
    if process_id == 0 {
        return None;
    }

    let process = unsafe {
        OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, process_id)
    }
    .ok()?;

    let mut buffer = vec![0u16; MAX_PATH as usize];
    let len = unsafe { K32GetModuleBaseNameW(process, None, &mut buffer) } as usize;

    unsafe {
        let _ = CloseHandle(process);
    }

    if len == 0 {
        return None;
    }

    Some(
        OsString::from_wide(&buffer[..len])
            .to_string_lossy()
            .into_owned(),
    )
}

struct ProcessScanState<'a> {
    watched: &'a [WatchedProcess],
    matched: bool,
}
