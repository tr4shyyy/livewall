use std::ptr::null_mut;
use std::sync::mpsc;

use anyhow::{Context, Result, anyhow};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2Environment, ICoreWebView2, ICoreWebView2Controller,
};
use webview2_com::{
    CoTaskMemPWSTR, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, ExecuteScriptCompletedHandler,
};
use windows::core::BOOL;
use windows::Win32::Foundation::{E_POINTER, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HBRUSH, MONITORINFO, MONITOR_DEFAULTTONEAREST, MonitorFromWindow,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows,
    FindWindowExW, FindWindowW, GetClientRect, HMENU, MSG, PostQuitMessage, RegisterClassW,
    SEND_MESSAGE_TIMEOUT_FLAGS, SW_SHOW, SendMessageTimeoutW, SetParent, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WNDCLASSW, WM_DESTROY, WS_CHILD, WS_CLIPCHILDREN,
    WS_CLIPSIBLINGS, WS_VISIBLE, PM_REMOVE,
};
use windows::core::{PCWSTR, w};

pub struct WallpaperApp {
    hwnd: HWND,
    webview: ICoreWebView2,
    controller: ICoreWebView2Controller,
}

impl WallpaperApp {
    pub fn create(wallpaper_url: &str) -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .context("failed to initialize COM")?;
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }

        let worker = ensure_worker_window().context("failed to locate WorkerW host")?;
        let hwnd = create_host_window(worker).context("failed to create wallpaper host window")?;
        let (webview, controller) = create_webview(hwnd, wallpaper_url)?;

        Ok(Self {
            hwnd,
            webview,
            controller,
        })
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        let script = if paused {
            "window.weebpSetPaused?.(true); document.body?.classList.add('paused');"
        } else {
            "window.weebpSetPaused?.(false); document.body?.classList.remove('paused');"
        };

        let js = CoTaskMemPWSTR::from(script);
        ExecuteScriptCompletedHandler::wait_for_async_operation(
            Box::new({
                let webview = self.webview.clone();
                move |handler| unsafe {
                    webview
                        .ExecuteScript(*js.as_ref().as_pcwstr(), &handler)
                        .map_err(webview2_com::Error::WindowsError)
                }
            }),
            Box::new(|error_code, _result| error_code),
        )
        .map_err(|err| anyhow!(err.to_string()))
        .context("failed to send pause command to webview")?;
        Ok(())
    }

    pub fn resize_to_parent(&self) -> Result<()> {
        let bounds = client_rect(self.hwnd)?;
        unsafe {
            self.controller
                .SetBounds(bounds)
                .ok()
                .context("failed to resize WebView2 controller")?;
        }
        Ok(())
    }

    pub fn message_loop<F>(&self, mut tick: F) -> Result<()>
    where
        F: FnMut() -> Result<LoopFlow>,
    {
        unsafe {
            let mut msg = MSG::default();
            loop {
                while windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                    &mut msg,
                    None,
                    0,
                    0,
                    PM_REMOVE,
                )
                .as_bool()
                {
                    if msg.message == 0x0012 {
                        return Ok(());
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                match tick()? {
                    LoopFlow::Continue => {}
                    LoopFlow::Exit => return Ok(()),
                }

                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}

pub enum LoopFlow {
    Continue,
    Exit,
}

fn create_host_window(parent: HWND) -> Result<HWND> {
    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_DESTROY => {
                unsafe {
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    let class_name = w!("WeebpRsWallpaperHost");
    let hinstance = unsafe { GetModuleHandleW(None) }.context("failed to get module handle")?;

    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        hInstance: hinstance.into(),
        lpszClassName: class_name,
        hbrBackground: HBRUSH(null_mut()),
        ..Default::default()
    };

    unsafe {
        let _ = RegisterClassW(&wc);
    }

    let rect = monitor_rect_for_window(parent)?;
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("weebp-rs"),
            WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            Some(parent),
            Some(HMENU(null_mut())),
            Some(windows::Win32::Foundation::HINSTANCE(hinstance.0)),
            Some(null_mut()),
        )
    }
    .context("CreateWindowExW failed")?;

    unsafe {
        let _ = SetParent(hwnd, Some(parent));
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    Ok(hwnd)
}

fn create_webview(hwnd: HWND, wallpaper_url: &str) -> Result<(ICoreWebView2, ICoreWebView2Controller)> {
    let environment = {
        let (tx, rx) = mpsc::channel();

        CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
            Box::new(|handler| unsafe {
                CreateCoreWebView2Environment(&handler).map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |error_code, environment| {
                error_code?;
                let _ = tx.send(environment.ok_or_else(|| windows::core::Error::from(E_POINTER)));
                Ok(())
            }),
        )
        .map_err(|err| anyhow!(err.to_string()))
        .context("failed to create WebView2 environment callback")?;

        rx.recv()
            .context("failed to receive WebView2 environment result")?
            .context("WebView2 environment missing")?
    };

    let controller = {
        let (tx, rx) = mpsc::channel();
        let environment = environment.clone();

        CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                environment
                    .CreateCoreWebView2Controller(hwnd, &handler)
                    .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |error_code, controller| {
                error_code?;
                let _ = tx.send(controller.ok_or_else(|| windows::core::Error::from(E_POINTER)));
                Ok(())
            }),
        )
        .map_err(|err| anyhow!(err.to_string()))
        .context("failed to create WebView2 controller callback")?;

        rx.recv()
            .context("failed to receive WebView2 controller result")?
            .context("WebView2 controller missing")?
    };

    let bounds = client_rect(hwnd)?;
    unsafe {
        controller
            .SetBounds(bounds)
            .context("failed to set WebView2 bounds")?;
        controller
            .SetIsVisible(true)
            .context("failed to show WebView2 controller")?;
    }

    let webview = unsafe { controller.CoreWebView2().context("failed to get CoreWebView2")? };
    let url = CoTaskMemPWSTR::from(wallpaper_url);
    unsafe {
        webview
            .Navigate(*url.as_ref().as_pcwstr())
            .context("failed to navigate wallpaper URL")?;
    }

    Ok((webview, controller))
}

fn client_rect(hwnd: HWND) -> Result<RECT> {
    let mut rect = RECT::default();
    unsafe {
        GetClientRect(hwnd, &mut rect)
            .ok()
            .context("GetClientRect failed")?;
    }
    Ok(rect)
}

fn monitor_rect_for_window(hwnd: HWND) -> Result<RECT> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return Err(anyhow!("MonitorFromWindow failed"));
    }

    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        GetMonitorInfoW(monitor, &mut info)
            .ok()
            .context("GetMonitorInfoW failed")?;
    }
    Ok(info.rcMonitor)
}

fn ensure_worker_window() -> Result<HWND> {
    unsafe extern "system" fn enum_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let shell_view = unsafe {
            FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), PCWSTR::null())
                .unwrap_or_default()
        };

        if !shell_view.is_invalid() {
            let worker = unsafe {
                FindWindowExW(None, Some(hwnd), w!("WorkerW"), PCWSTR::null())
                    .unwrap_or_default()
            };
            if !worker.is_invalid() {
                let target = lparam.0 as *mut HWND;
                unsafe {
                    *target = worker;
                }
                return BOOL(0);
            }
        }

        BOOL(1)
    }

    let progman = unsafe { FindWindowW(w!("Progman"), PCWSTR::null()) }.unwrap_or_default();
    if progman.is_invalid() {
        return Err(anyhow!("failed to find Progman"));
    }

    unsafe {
        let mut _result = 0usize;
        let _ = SendMessageTimeoutW(
            progman,
            0x052C,
            WPARAM(0xD),
            LPARAM(0),
            SEND_MESSAGE_TIMEOUT_FLAGS(0),
            1000,
            Some(&mut _result),
        );
        let _ = SendMessageTimeoutW(
            progman,
            0x052C,
            WPARAM(0xD),
            LPARAM(1),
            SEND_MESSAGE_TIMEOUT_FLAGS(0),
            1000,
            Some(&mut _result),
        );
    }

    let mut worker = HWND(null_mut());
    unsafe {
        EnumWindows(
            Some(enum_windows),
            LPARAM((&mut worker as *mut HWND).cast::<core::ffi::c_void>() as isize),
        )
        .ok()
        .context("EnumWindows failed")?;
    }

    if worker.is_invalid() {
        Ok(progman)
    } else {
        Ok(worker)
    }
}
