use std::mem::size_of;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH, CreateFontIndirectW, DeleteObject, HFONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    ICC_LINK_CLASS, INITCOMMONCONTROLSEX, InitCommonControlsEx, NMHDR, NMLINK, NM_CLICK,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_GROUPBOX, BS_PUSHBUTTON,
    CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
    GetWindowLongPtrW, GWLP_USERDATA, HMENU, NONCLIENTMETRICSW, RegisterClassW, SendMessageW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW, ShowWindow, SPI_GETNONCLIENTMETRICS,
    SW_SHOW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WNDCLASSW, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_NOTIFY,
    WM_SETFONT, WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE,
};

const IDC_CHK_PER_CORE: usize = 2001;
const IDC_CHK_AUTOSTART: usize = 2002;
const IDC_BTN_EXIT: usize = 2003;
const IDC_BTN_CLOSE: usize = 2004;

static SETTINGS_HWND: AtomicIsize = AtomicIsize::new(0);
/// Holds the HFONT created in WM_CREATE so it can be deleted on WM_DESTROY.
static SETTINGS_FONT: AtomicIsize = AtomicIsize::new(0);

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

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

            let Ok(hmod) = GetModuleHandleW(None) else { return LRESULT(-1) };

            // DPI-aware system UI font via SPI_GETNONCLIENTMETRICS.
            let mut ncm = NONCLIENTMETRICSW {
                cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
                ..Default::default()
            };
            let _ = SystemParametersInfoW(
                SPI_GETNONCLIENTMETRICS,
                ncm.cbSize,
                Some(&mut ncm as *mut NONCLIENTMETRICSW as *mut core::ffi::c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
            let hfont = CreateFontIndirectW(&ncm.lfMessageFont);
            SETTINGS_FONT.store(hfont.0 as isize, Ordering::Relaxed);
            let fwp = WPARAM(hfont.0 as usize);

            let dpi = GetDpiForWindow(hwnd);
            let sc = |v: i32| -> i32 { (v as f32 * dpi as f32 / 96.0) as i32 };

            let show_per_core = crate::config::SHOW_PER_CORE.load(Ordering::Relaxed);
            let autostart     = crate::config::AUTOSTART_ENABLED.load(Ordering::Relaxed);

            // ── Group: Options ─────────────────────────────────────────────
            if let Ok(g) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("BUTTON"), w!("Options"),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_GROUPBOX as u32),
                sc(8), sc(4), sc(248), sc(80), hwnd, HMENU(null_mut()), hmod, None)
            { SendMessageW(g, WM_SETFONT, fwp, LPARAM(1)); }

            if let Ok(c) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("BUTTON"), w!("Show CPU per Core"),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                sc(20), sc(22), sc(224), sc(22), hwnd, HMENU(IDC_CHK_PER_CORE as *mut _), hmod, None)
            {
                SendMessageW(c, BM_SETCHECK, WPARAM(show_per_core as usize), LPARAM(0));
                SendMessageW(c, WM_SETFONT, fwp, LPARAM(1));
            }

            if let Ok(c) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("BUTTON"), w!("Start with Windows"),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                sc(20), sc(50), sc(224), sc(22), hwnd, HMENU(IDC_CHK_AUTOSTART as *mut _), hmod, None)
            {
                SendMessageW(c, BM_SETCHECK, WPARAM(autostart as usize), LPARAM(0));
                SendMessageW(c, WM_SETFONT, fwp, LPARAM(1));
            }

            // ── Group: About ───────────────────────────────────────────────
            if let Ok(g) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("BUTTON"), w!("About"),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_GROUPBOX as u32),
                sc(8), sc(90), sc(248), sc(106), hwnd, HMENU(null_mut()), hmod, None)
            { SendMessageW(g, WM_SETFONT, fwp, LPARAM(1)); }

            let ver = to_wide(&format!("RMeters  v{}", env!("CARGO_PKG_VERSION")));
            if let Ok(l) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""),
                WS_CHILD | WS_VISIBLE,
                sc(20), sc(110), sc(224), sc(18), hwnd, HMENU(null_mut()), hmod, None)
            {
                let _ = SetWindowTextW(l, PCWSTR(ver.as_ptr()));
                SendMessageW(l, WM_SETFONT, fwp, LPARAM(1));
            }

            if let Ok(l) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("SysLink"),
                w!("<a href=\"https://rmeters.reslab.pro\">rmeters.reslab.pro</a>"),
                WS_CHILD | WS_VISIBLE,
                sc(20), sc(132), sc(224), sc(18), hwnd, HMENU(null_mut()), hmod, None)
            { SendMessageW(l, WM_SETFONT, fwp, LPARAM(1)); }

            if let Ok(l) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("SysLink"),
                w!("<a href=\"mailto:andrew.chuev@gmail.com\">andrew.chuev@gmail.com</a>"),
                WS_CHILD | WS_VISIBLE,
                sc(20), sc(154), sc(224), sc(18), hwnd, HMENU(null_mut()), hmod, None)
            { SendMessageW(l, WM_SETFONT, fwp, LPARAM(1)); }

            // ── Footer buttons ─────────────────────────────────────────────
            if let Ok(b) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("BUTTON"), w!("Exit RMeters"),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
                sc(8), sc(206), sc(108), sc(26), hwnd, HMENU(IDC_BTN_EXIT as *mut _), hmod, None)
            { SendMessageW(b, WM_SETFONT, fwp, LPARAM(1)); }

            if let Ok(b) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("BUTTON"), w!("Close"),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
                sc(148), sc(206), sc(108), sc(26), hwnd, HMENU(IDC_BTN_CLOSE as *mut _), hmod, None)
            { SendMessageW(b, WM_SETFONT, fwp, LPARAM(1)); }

            SETTINGS_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
            LRESULT(0)
        }
        WM_NOTIFY => {
            let nmhdr = &*(lparam.0 as *const NMHDR);
            if nmhdr.code == NM_CLICK {
                let nml = &*(lparam.0 as *const NMLINK);
                let url = PCWSTR(nml.item.szUrl.as_ptr());
                ShellExecuteW(HWND(null_mut()), w!("open"), url,
                    PCWSTR::null(), PCWSTR::null(), SW_SHOW);
            }
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
                    IDC_BTN_CLOSE => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_CLOSE   => { let _ = DestroyWindow(hwnd); LRESULT(0) }
        WM_DESTROY => {
            SETTINGS_HWND.store(0, Ordering::Relaxed);
            let raw = SETTINGS_FONT.swap(0, Ordering::Relaxed);
            if raw != 0 {
                let _ = DeleteObject(HFONT(raw as *mut _));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn show_settings(overlay_hwnd: HWND) {
    unsafe {
        if let Some(existing) = get_settings_hwnd() {
            let _ = SetForegroundWindow(existing);
            return;
        }

        let icc = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LINK_CLASS,
        };
        let _ = InitCommonControlsEx(&icc);

        let Ok(hmod) = GetModuleHandleW(None) else { return };
        let class_name = w!("rmeters_settings");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(settings_wnd_proc),
            hInstance: hmod.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hbrBackground: HBRUSH((COLOR_WINDOW.0 as isize + 1) as *mut _),
            ..Default::default()
        };
        RegisterClassW(&wc);

        // Scale window dimensions by the DPI of the monitor where the overlay lives.
        let dpi = GetDpiForWindow(overlay_hwnd);
        let scale = dpi as f32 / 96.0;
        let win_w = (280.0 * scale) as i32;
        let win_h = (268.0 * scale) as i32;

        // Center on the monitor that contains the overlay.
        let (x, y) = center_on_monitor(overlay_hwnd, win_w, win_h);

        if let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0), class_name, w!("RMeters \u{2014} Settings"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            x, y, win_w, win_h,
            None, HMENU(null_mut()), hmod,
            Some(overlay_hwnd.0 as *const _),
        ) {
            set_window_icon(hwnd);
            apply_dark_titlebar(hwnd);
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
    }
}

/// Sets big and small window icons from the embedded app icon resource.
unsafe fn set_window_icon(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        LoadImageW, IMAGE_FLAGS, IMAGE_ICON, LR_DEFAULTSIZE, WM_SETICON,
    };

    let Ok(hmod) = GetModuleHandleW(None) else { return };
    // ICON_BIG = 1, ICON_SMALL = 0
    if let Ok(h) = LoadImageW(hmod, w!("IDI_APP_ICON"), IMAGE_ICON, 0, 0, LR_DEFAULTSIZE) {
        SendMessageW(hwnd, WM_SETICON, WPARAM(1), LPARAM(h.0 as isize));
    }
    if let Ok(h) = LoadImageW(hmod, w!("IDI_APP_ICON"), IMAGE_ICON, 16, 16, IMAGE_FLAGS(0)) {
        SendMessageW(hwnd, WM_SETICON, WPARAM(0), LPARAM(h.0 as isize));
    }
}

/// Applies a dark title bar on Windows 11 via DWM attribute 20.
unsafe fn apply_dark_titlebar(hwnd: HWND) {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
    let dark: u32 = 1;
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &dark as *const u32 as *const _,
        size_of::<u32>() as u32,
    );
}

/// Returns (x, y) to center a win_w×win_h window on the monitor that contains `ref_hwnd`.
unsafe fn center_on_monitor(ref_hwnd: HWND, win_w: i32, win_h: i32) -> (i32, i32) {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    let hmon = MonitorFromWindow(ref_hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    if GetMonitorInfoW(hmon, &mut mi).as_bool() {
        let rc = mi.rcWork;
        let x = rc.left + (rc.right - rc.left - win_w) / 2;
        let y = rc.top  + (rc.bottom - rc.top  - win_h) / 2;
        return (x, y);
    }

    // Fallback: primary screen center.
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    (
        (GetSystemMetrics(SM_CXSCREEN) - win_w) / 2,
        (GetSystemMetrics(SM_CYSCREEN) - win_h) / 2,
    )
}
