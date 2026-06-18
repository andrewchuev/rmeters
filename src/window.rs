use std::ptr::null_mut;
use once_cell::sync::Lazy;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM, COLORREF};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW, FindWindowW, GetWindowRect,
    KillTimer, RegisterClassW, RegisterWindowMessageW, SetLayeredWindowAttributes, SetTimer,
    SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, HMENU, HWND_TOPMOST, LWA_COLORKEY,
    SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_CREATE, WM_DESTROY, WM_ERASEBKGND,
    WM_PAINT, WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::Win32::Graphics::Gdi::HBRUSH;

static WM_TASKBAR_CREATED: Lazy<u32> = Lazy::new(|| unsafe {
    RegisterWindowMessageW(w!("TaskbarCreated"))
});

use crate::renderer::Renderer;

pub const OVERLAY_WIDTH: i32 = 140;

pub unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            SetTimer(hwnd, 1, 1000, None); // full reposition (taskbar may move/resize)
            SetTimer(hwnd, 2, 200, None);  // fast Z-order reassertion
            LRESULT(0)
        }
        WM_TIMER => {
            match wparam.0 {
                1 => reposition_window(hwnd),
                2 => {
                    SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0,
                        SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE).ok();
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_PAINT => {
            Renderer::paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => {
            LRESULT(1)
        }
        WM_RBUTTONUP => {
            crate::settings::show_settings(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(s) = crate::settings::get_settings_hwnd() {
                let _ = DestroyWindow(s);
            }
            let _ = KillTimer(hwnd, 1);
            let _ = KillTimer(hwnd, 2);
            windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            LRESULT(0)
        }
        // Explorer restarted — taskbar was recreated, re-anchor the overlay
        msg if msg == *WM_TASKBAR_CREATED => {
            reposition_window(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn create_overlay() -> HWND {
    unsafe {
        crate::log_info("create_overlay started");
        let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
        let class_name = w!("rmeters_overlay_class");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hbrBackground: HBRUSH(null_mut()),
            ..Default::default()
        };

        RegisterClassW(&wc);
        crate::log_info("Window class registered");

        // Find the taskbar window handle to set as the owner.
        // The OS manager (DWM) guarantees that an owned window always stays in front of its owner,
        // which natively prevents our overlay from being hidden behind the taskbar.
        let h_taskbar = FindWindowW(w!("Shell_TrayWnd"), None).unwrap_or(HWND(null_mut()));
        crate::log_info(&format!("create_overlay: Found taskbar handle = {:?}", h_taskbar.0));

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class_name,
            w!("rmeters_overlay"),
            WS_POPUP,
            0,
            0,
            OVERLAY_WIDTH,
            48,
            h_taskbar, 
            HMENU(null_mut()),
            instance,
            None,
        ).expect("Failed to create overlay window");
        crate::log_info("CreateWindowExW succeeded");

        // Magenta (0x00FF00FF) is the transparent color key
        SetLayeredWindowAttributes(hwnd, COLORREF(0x00FF00FF), 255, LWA_COLORKEY)
            .expect("Failed to set layered window attributes");
        crate::log_info("Layered attributes set");

        reposition_window(hwnd);

        let _ = ShowWindow(hwnd, SW_SHOW);
        crate::log_info("ShowWindow called");

        hwnd
    }
}

pub fn reposition_window(hwnd: HWND) {
    unsafe {
        let h_taskbar = FindWindowW(w!("Shell_TrayWnd"), None).unwrap_or(HWND(null_mut()));
        if h_taskbar.0.is_null() {
            return;
        }

        let h_tray = FindWindowExW(h_taskbar, None, w!("TrayNotifyWnd"), None).unwrap_or(HWND(null_mut()));
        if h_tray.0.is_null() {
            return;
        }

        let mut taskbar_rect = RECT::default();
        let mut tray_rect = RECT::default();

        if GetWindowRect(h_taskbar, &mut taskbar_rect).is_ok()
            && GetWindowRect(h_tray, &mut tray_rect).is_ok()
        {
            let dpi = GetDpiForWindow(hwnd);
            let scale = dpi as f32 / 96.0;
            let physical_width = (OVERLAY_WIDTH as f32 * scale) as i32;
            let margin = (10.0 * scale) as i32;
            let taskbar_height = taskbar_rect.bottom - taskbar_rect.top;
            let left = tray_rect.left - physical_width - margin;
            let top = taskbar_rect.top;

            // Always reassert position AND topmost Z-order so the overlay never gets
            // permanently buried behind the taskbar or shell notification windows.
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                left,
                top,
                physical_width,
                taskbar_height,
                SWP_NOACTIVATE,
            ).ok();
        }
    }
}
