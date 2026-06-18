use std::ptr::null_mut;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH, GetStockObject, DEFAULT_GUI_FONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_PUSHBUTTON,
    CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW,
    DestroyWindow, GetSystemMetrics, GetWindowLongPtrW, GWLP_USERDATA, HMENU,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW,
    ShowWindow, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WNDCLASSW, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_SETFONT,
    WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE,
};

const IDC_CHK_PER_CORE: usize = 2001;
const IDC_CHK_AUTOSTART: usize = 2002;
const IDC_BTN_EXIT: usize = 2003;

static SETTINGS_HWND: AtomicIsize = AtomicIsize::new(0);

pub fn get_settings_hwnd() -> Option<HWND> {
    let raw = SETTINGS_HWND.load(Ordering::Relaxed);
    if raw != 0 { Some(HWND(raw as *mut _)) } else { None }
}

pub unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);

            let instance = GetModuleHandleW(None).unwrap();
            let font = GetStockObject(DEFAULT_GUI_FONT);

            let show_per_core = crate::config::SHOW_PER_CORE.load(Ordering::Relaxed);
            let autostart    = crate::config::AUTOSTART_ENABLED.load(Ordering::Relaxed);

            let chk1 = CreateWindowExW(
                WINDOW_EX_STYLE(0), w!("BUTTON"), w!("Show CPU per Core"),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                16, 16, 232, 24, hwnd, HMENU(IDC_CHK_PER_CORE as *mut _), instance, None,
            ).unwrap();
            SendMessageW(chk1, BM_SETCHECK, WPARAM(if show_per_core { 1 } else { 0 }), LPARAM(0));
            SendMessageW(chk1, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));

            let chk2 = CreateWindowExW(
                WINDOW_EX_STYLE(0), w!("BUTTON"), w!("Start with Windows"),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                16, 48, 232, 24, hwnd, HMENU(IDC_CHK_AUTOSTART as *mut _), instance, None,
            ).unwrap();
            SendMessageW(chk2, BM_SETCHECK, WPARAM(if autostart { 1 } else { 0 }), LPARAM(0));
            SendMessageW(chk2, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));

            let btn = CreateWindowExW(
                WINDOW_EX_STYLE(0), w!("BUTTON"), w!("Exit RMeters"),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
                128, 100, 120, 28, hwnd, HMENU(IDC_BTN_EXIT as *mut _), instance, None,
            ).unwrap();
            SendMessageW(btn, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));

            SETTINGS_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id     = wparam.0 & 0xffff;
            let notify = (wparam.0 >> 16) as u32;
            let ctrl   = HWND(lparam.0 as *mut _);

            if notify == BN_CLICKED {
                match id {
                    IDC_CHK_PER_CORE => {
                        let checked = SendMessageW(ctrl, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1;
                        crate::config::SHOW_PER_CORE.store(checked, Ordering::Relaxed);
                        crate::config::save_config();
                        let overlay = HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut _);
                        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(overlay, None, false);
                    }
                    IDC_CHK_AUTOSTART => {
                        let checked = SendMessageW(ctrl, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1;
                        if crate::config::set_autostart(checked).is_ok() {
                            crate::config::AUTOSTART_ENABLED.store(checked, Ordering::Relaxed);
                        }
                    }
                    IDC_BTN_EXIT => {
                        let overlay = HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut _);
                        let _ = DestroyWindow(hwnd);
                        let _ = DestroyWindow(overlay);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            SETTINGS_HWND.store(0, Ordering::Relaxed);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn show_settings(overlay_hwnd: HWND) {
    unsafe {
        // If already open, bring to front instead of creating a second instance
        if let Some(existing) = get_settings_hwnd() {
            let _ = SetForegroundWindow(existing);
            return;
        }

        let instance = GetModuleHandleW(None).unwrap();
        let class_name = w!("rmeters_settings");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(settings_wnd_proc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 as isize + 1) as *mut _),
            ..Default::default()
        };
        RegisterClassW(&wc); // ignore error if class already registered

        let win_w = 264i32;
        let win_h = 160i32;
        let x = (GetSystemMetrics(SM_CXSCREEN) - win_w) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - win_h) / 2;

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("RMeters — Settings"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            x, y, win_w, win_h,
            None,
            HMENU(null_mut()),
            instance,
            Some(overlay_hwnd.0 as *const _),
        ).expect("Failed to create settings window");

        let _ = ShowWindow(hwnd, SW_SHOW);
    }
}
