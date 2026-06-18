use std::ptr::null_mut;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM, COLORREF};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, FindWindowExW, FindWindowW, GetWindowRect, RegisterClassW,
    SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, HMENU, SWP_NOACTIVATE,
    SW_SHOW, WM_DESTROY, WM_PAINT, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, SetLayeredWindowAttributes, LWA_COLORKEY,
    WM_COMMAND, WM_CREATE, WM_TIMER, WM_RBUTTONUP, WM_CONTEXTMENU, DestroyWindow, SetTimer,
    KillTimer, HWND_TOPMOST, WS_EX_NOACTIVATE, WM_ERASEBKGND,
};
use windows::Win32::Graphics::Gdi::HBRUSH;

use crate::renderer::Renderer;
use crate::tray::{WM_TRAY_ICON, ID_EXIT, ID_SHOW_PER_CORE, ID_AUTOSTART, show_tray_menu};

pub const OVERLAY_WIDTH: i32 = 140;

pub unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            // Set a timer to check for taskbar position/size changes every 1 second
            SetTimer(hwnd, 1, 1000, None);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == 1 {
                reposition_window(hwnd);
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
        WM_TRAY_ICON => {
            let event = lparam.0 as u32;
            if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                show_tray_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            if id == ID_EXIT {
                let _ = DestroyWindow(hwnd);
            } else if id == ID_SHOW_PER_CORE {
                let show_per_core = crate::config::SHOW_PER_CORE.load(std::sync::atomic::Ordering::Relaxed);
                crate::config::SHOW_PER_CORE.store(!show_per_core, std::sync::atomic::Ordering::Relaxed);
                crate::config::save_config();
                
                // Force immediate repaint
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hwnd, None, false);
            } else if id == ID_AUTOSTART {
                let autostart_enabled = crate::config::AUTOSTART_ENABLED.load(std::sync::atomic::Ordering::Relaxed);
                let new_val = !autostart_enabled;
                if crate::config::set_autostart(new_val).is_ok() {
                    crate::config::AUTOSTART_ENABLED.store(new_val, std::sync::atomic::Ordering::Relaxed);
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(hwnd, 1);
            crate::tray::remove_tray_icon(hwnd);
            windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
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
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
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
        static mut LAST_LEFT: i32 = 0;
        static mut LAST_TOP: i32 = 0;
        static mut LAST_WIDTH: i32 = 0;
        static mut LAST_HEIGHT: i32 = 0;

        let h_taskbar = FindWindowW(w!("Shell_TrayWnd"), None).unwrap_or(HWND(null_mut()));
        if h_taskbar.0.is_null() {
            crate::log_info("reposition_window: Shell_TrayWnd not found");
            return;
        }

        let h_tray = FindWindowExW(h_taskbar, None, w!("TrayNotifyWnd"), None).unwrap_or(HWND(null_mut()));
        if h_tray.0.is_null() {
            crate::log_info("reposition_window: TrayNotifyWnd not found");
            return;
        }

        let mut taskbar_rect = RECT::default();
        let mut tray_rect = RECT::default();

        if GetWindowRect(h_taskbar, &mut taskbar_rect).is_ok() && GetWindowRect(h_tray, &mut tray_rect).is_ok() {
            // Get DPI for the window to scale coordinates
            let dpi = GetDpiForWindow(hwnd);
            let scale = dpi as f32 / 96.0;

            let physical_width = (OVERLAY_WIDTH as f32 * scale) as i32;
            let margin = (10.0 * scale) as i32;

            let taskbar_height = taskbar_rect.bottom - taskbar_rect.top;
            let left = tray_rect.left - physical_width - margin;
            let top = taskbar_rect.top;

            // Check if coordinates actually changed
            let mut changed = false;
            if left != LAST_LEFT || top != LAST_TOP || physical_width != LAST_WIDTH || taskbar_height != LAST_HEIGHT {
                LAST_LEFT = left;
                LAST_TOP = top;
                LAST_WIDTH = physical_width;
                LAST_HEIGHT = taskbar_height;
                changed = true;
            }

            if changed {
                crate::log_info(&format!(
                    "reposition_window: positioning overlay to left={}, top={}, width={}, height={}, dpi={}",
                    left, top, physical_width, taskbar_height, dpi
                ));

                // Force topmost Z-order to overlay properly on the taskbar without SWP_NOZORDER
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
        } else {
            crate::log_info("reposition_window: GetWindowRect failed");
        }
    }
}
