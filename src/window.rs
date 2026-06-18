use std::ptr::null_mut;
use once_cell::sync::Lazy;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM, COLORREF};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW, FindWindowW, GetWindowRect,
    KillTimer, RegisterClassW, RegisterWindowMessageW, SetLayeredWindowAttributes, SetTimer,
    SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, HMENU,
    HWND_TOPMOST, LWA_COLORKEY, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
    WM_CREATE, WM_DESTROY, WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_NCHITTEST, WM_NCRBUTTONUP,
    WM_PAINT, WM_RBUTTONUP, WM_TIMER, WM_WINDOWPOSCHANGING, WINDOWPOS, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::Win32::Graphics::Gdi::HBRUSH;

use crate::renderer::Renderer;

static WM_TASKBAR_CREATED: Lazy<u32> = Lazy::new(|| unsafe {
    RegisterWindowMessageW(w!("TaskbarCreated"))
});

pub const OVERLAY_WIDTH: i32 = 140;

pub unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            SetTimer(hwnd, 1, 1000, None);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == 1 {
                reposition_window(hwnd);
            }
            LRESULT(0)
        }
        WM_NCHITTEST => {
            LRESULT(2) // HTCAPTION — whole window is drag handle
        }
        WM_NCRBUTTONUP => {
            crate::settings::show_settings(hwnd);
            LRESULT(0)
        }
        WM_WINDOWPOSCHANGING => {
            // Lock Y and height to the taskbar so the overlay stays on the taskbar row
            let pos = &mut *(lparam.0 as *mut WINDOWPOS);
            if (pos.flags.0 & SWP_NOMOVE.0) == 0 {
                let h_taskbar = FindWindowW(w!("Shell_TrayWnd"), None)
                    .unwrap_or(HWND(null_mut()));
                if !h_taskbar.0.is_null() {
                    let mut r = RECT::default();
                    if GetWindowRect(h_taskbar, &mut r).is_ok() {
                        pos.y  = r.top;
                        pos.cy = r.bottom - r.top;
                    }
                }
            }
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                crate::config::OVERLAY_X.store(rect.left, std::sync::atomic::Ordering::Relaxed);
                crate::config::save_config();
            }
            LRESULT(0)
        }
        WM_PAINT => {
            Renderer::paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_RBUTTONUP => {
            crate::settings::show_settings(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(s) = crate::settings::get_settings_hwnd() {
                let _ = DestroyWindow(s);
            }
            let _ = KillTimer(hwnd, 1);
            windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            LRESULT(0)
        }
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

        // Shell_TrayWnd as owner keeps our overlay above the taskbar automatically.
        // The shell maintains the "owned window above owner" z-order invariant,
        // so we never need to call SetWindowPos(HWND_TOPMOST) ourselves.
        let h_taskbar = FindWindowW(w!("Shell_TrayWnd"), None).unwrap_or(HWND(null_mut()));

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class_name,
            w!("rmeters_overlay"),
            WS_POPUP,
            0, 0, OVERLAY_WIDTH, 48,
            h_taskbar,
            HMENU(null_mut()),
            instance,
            None,
        ).expect("Failed to create overlay window");

        SetLayeredWindowAttributes(hwnd, COLORREF(0x00FF00FF), 255, LWA_COLORKEY)
            .expect("Failed to set layered window attributes");

        reposition_window(hwnd);

        let _ = ShowWindow(hwnd, SW_SHOW);
        crate::log_info("Overlay created (owned by Shell_TrayWnd, no periodic TOPMOST reassertion)");

        hwnd
    }
}

pub fn reposition_window(hwnd: HWND) {
    unsafe {
        // While a tray popup menu is open, skip the HWND_TOPMOST reassertion.
        // SetWindowPos(HWND_TOPMOST) on a window owned by Shell_TrayWnd cascades
        // Shell_TrayWnd upward in the TOPMOST band, which buries the popup menu
        // behind the taskbar. When no menu is open the cascade is harmless.
        let menu_open = FindWindowW(w!("#32768"), None)
            .map(|w| !w.0.is_null())
            .unwrap_or(false);

        let h_taskbar = FindWindowW(w!("Shell_TrayWnd"), None).unwrap_or(HWND(null_mut()));
        if h_taskbar.0.is_null() {
            return;
        }

        let h_tray = FindWindowExW(h_taskbar, None, w!("TrayNotifyWnd"), None)
            .unwrap_or(HWND(null_mut()));
        if h_tray.0.is_null() {
            return;
        }

        let mut taskbar_rect = RECT::default();
        let mut tray_rect    = RECT::default();

        if GetWindowRect(h_taskbar, &mut taskbar_rect).is_ok()
            && GetWindowRect(h_tray, &mut tray_rect).is_ok()
        {
            let dpi    = GetDpiForWindow(hwnd);
            let scale  = dpi as f32 / 96.0;
            let phys_w = (OVERLAY_WIDTH as f32 * scale) as i32;
            let phys_h = taskbar_rect.bottom - taskbar_rect.top;
            let margin = (10.0 * scale) as i32;

            let saved_x = crate::config::OVERLAY_X.load(std::sync::atomic::Ordering::Relaxed);
            let x = if saved_x >= 0
                && saved_x >= taskbar_rect.left
                && saved_x + phys_w <= taskbar_rect.right
            {
                saved_x
            } else {
                tray_rect.left - phys_w - margin
            };

            let (insert_after, flags) = if menu_open {
                // Menu open: reposition only, don't touch z-order
                (HWND(null_mut()), SWP_NOACTIVATE | SWP_NOZORDER)
            } else {
                // No menu: also reassert z-order above taskbar (recovers from Win+D etc.)
                (HWND_TOPMOST, SWP_NOACTIVATE)
            };

            SetWindowPos(hwnd, insert_after, x, taskbar_rect.top, phys_w, phys_h, flags).ok();
        }
    }
}
