use std::sync::LazyLock;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, HMENU, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};

static WM_TASKBAR_CREATED: LazyLock<u32> =
    LazyLock::new(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) });

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        msg if msg == crate::tray::WM_TRAY_CALLBACK => {
            let lmsg = lparam.0 as u32;
            if lmsg == WM_RBUTTONUP || lmsg == WM_LBUTTONUP {
                crate::settings::show_settings();
            }
            LRESULT(0)
        }
        msg if msg == *WM_TASKBAR_CREATED => {
            crate::tray::reset_after_shell_restart();
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(s) = crate::settings::get_settings_hwnd() {
                let _ = DestroyWindow(s);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Creates the hidden window that owns the tray icons and receives their callbacks.
/// It is a never-shown top-level window rather than a message-only one, because
/// message-only windows do not receive the TaskbarCreated broadcast.
pub fn create_main_window() -> HWND {
    unsafe {
        let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .expect("GetModuleHandleW failed; process has no module handle");
        let class_name = w!("rmeters_main_class");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);

        // Force registration before any broadcast can arrive.
        LazyLock::force(&WM_TASKBAR_CREATED);

        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_name,
            w!("rmeters"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            HMENU::default(),
            instance,
            None,
        )
        .expect("Failed to create main window")
    }
}
