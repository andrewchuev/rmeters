// Win32 UI code uses many size_of::<T>() as u32 and DPI f32→i32 conversions
// that are always in range. Allow the lint rather than adding try_from noise.
#![allow(clippy::cast_possible_truncation, clippy::zero_ptr)]

use std::mem::size_of;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontIndirectW, CreatePen, CreateSolidBrush, DeleteObject, EndPaint,
    HBRUSH, HDC, HGDIOBJ, LineTo, MoveToEx, PAINTSTRUCT, PS_SOLID,
    SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    ICC_LINK_CLASS, INITCOMMONCONTROLSEX, InitCommonControlsEx, NMHDR, NMLINK, NM_CLICK,
    SetWindowTheme,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_PUSHBUTTON,
    CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
    GetClientRect, GetDlgCtrlID, GetWindowLongPtrW, GWLP_USERDATA, HMENU, NONCLIENTMETRICSW,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW,
    ShowWindow, SPI_GETNONCLIENTMETRICS, SW_SHOW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SystemParametersInfoW, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
    WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_DESTROY,
    WM_NOTIFY, WM_PAINT, WM_SETFONT, WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU,
    WS_VISIBLE,
};

const IDC_CHK_PER_CORE: usize = 2001;
const IDC_CHK_AUTOSTART: usize = 2002;
const IDC_BTN_EXIT: usize = 2003;
const IDC_BTN_CLOSE: usize = 2004;
const IDC_LBL_VERSION: usize = 3000;
const IDC_LBL_OPTIONS: usize = 3001;
const IDC_LBL_ABOUT: usize = 3002;

// Colors in BGR order (Windows COLORREF convention)
const COLOR_BG: u32    = 0x1E1E1E;
const COLOR_TEXT: u32  = 0xD0D0D0;
const COLOR_DIM: u32   = 0x686868;
const COLOR_WHITE: u32 = 0xF0F0F0;
const COLOR_SEP: u32   = 0x3C3C3C;

static SETTINGS_HWND: AtomicIsize = AtomicIsize::new(0);
static SETTINGS_FONT: AtomicIsize = AtomicIsize::new(0);
static DARK_BG_BRUSH: AtomicIsize = AtomicIsize::new(0);

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Scales a logical pixel value to physical pixels using the window's DPI.
fn dpi_scale(v: i32, dpi: u32) -> i32 {
    (v as f32 * dpi as f32 / 96.0) as i32
}

/// Returns the current settings window handle, or None if it is not open.
pub fn get_settings_hwnd() -> Option<HWND> {
    let raw = SETTINGS_HWND.load(Ordering::Relaxed);
    if raw != 0 { Some(HWND(raw as *mut _)) } else { None }
}

unsafe fn dark_brush() -> HBRUSH {
    let existing = DARK_BG_BRUSH.load(Ordering::Relaxed);
    if existing != 0 {
        return HBRUSH(existing as *mut _);
    }
    let b = CreateSolidBrush(COLORREF(COLOR_BG));
    DARK_BG_BRUSH.store(b.0 as isize, Ordering::Relaxed);
    b
}

pub unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            // SAFETY: lparam for WM_CREATE is always a pointer to CREATESTRUCTW
            // as specified by the Win32 API contract.
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);

            let Ok(hmod) = GetModuleHandleW(None) else { return LRESULT(-1) };

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

            // Delete any previously stored font before overwriting the slot,
            // in case the settings window is opened more than once per session.
            let old_font = SETTINGS_FONT.swap(hfont.0 as isize, Ordering::Relaxed);
            if old_font != 0 {
                let _ = DeleteObject(HGDIOBJ(old_font as *mut _));
            }

            let fwp = WPARAM(hfont.0 as usize);
            let dpi = GetDpiForWindow(hwnd);

            let show_per_core = crate::config::SHOW_PER_CORE.load(Ordering::Relaxed);
            let autostart     = crate::config::AUTOSTART_ENABLED.load(Ordering::Relaxed);

            macro_rules! ctrl {
                ($class:expr, $text:expr, $style:expr, $x:expr, $y:expr, $w:expr, $h:expr, $id:expr) => {{
                    CreateWindowExW(WINDOW_EX_STYLE(0), $class, $text,
                        WS_CHILD | WS_VISIBLE | WINDOW_STYLE($style),
                        dpi_scale($x, dpi), dpi_scale($y, dpi),
                        dpi_scale($w, dpi), dpi_scale($h, dpi),
                        hwnd, HMENU($id as *mut _), hmod, None)
                }};
            }

            // ── Version label ──────────────────────────────────────────────
            let ver = to_wide(&format!("RMeters  v{}", env!("CARGO_PKG_VERSION")));
            if let Ok(l) = ctrl!(w!("STATIC"), w!(""), 0, 16, 14, 248, 20, IDC_LBL_VERSION) {
                let _ = SetWindowTextW(l, PCWSTR(ver.as_ptr()));
                SendMessageW(l, WM_SETFONT, fwp, LPARAM(1));
            }

            // ── Options ────────────────────────────────────────────────────
            if let Ok(l) = ctrl!(w!("STATIC"), w!("OPTIONS"), 0, 16, 52, 80, 16, IDC_LBL_OPTIONS) {
                SendMessageW(l, WM_SETFONT, fwp, LPARAM(1));
            }
            if let Ok(c) = ctrl!(w!("BUTTON"), w!("Show CPU per Core"),
                BS_AUTOCHECKBOX as u32, 16, 72, 248, 22, IDC_CHK_PER_CORE)
            {
                SendMessageW(c, BM_SETCHECK, WPARAM(show_per_core as usize), LPARAM(0));
                SendMessageW(c, WM_SETFONT, fwp, LPARAM(1));
                let _ = SetWindowTheme(c, w!("DarkMode_Explorer"), PCWSTR::null());
            }
            if let Ok(c) = ctrl!(w!("BUTTON"), w!("Start with Windows"),
                BS_AUTOCHECKBOX as u32, 16, 96, 248, 22, IDC_CHK_AUTOSTART)
            {
                SendMessageW(c, BM_SETCHECK, WPARAM(autostart as usize), LPARAM(0));
                SendMessageW(c, WM_SETFONT, fwp, LPARAM(1));
                let _ = SetWindowTheme(c, w!("DarkMode_Explorer"), PCWSTR::null());
            }

            // ── About ──────────────────────────────────────────────────────
            if let Ok(l) = ctrl!(w!("STATIC"), w!("ABOUT"), 0, 16, 136, 80, 16, IDC_LBL_ABOUT) {
                SendMessageW(l, WM_SETFONT, fwp, LPARAM(1));
            }
            if let Ok(l) = ctrl!(w!("SysLink"),
                w!("<a href=\"https://rmeters.reslab.pro\">rmeters.reslab.pro</a>"),
                0, 16, 156, 248, 18, 0usize)
            {
                SendMessageW(l, WM_SETFONT, fwp, LPARAM(1));
                let _ = SetWindowTheme(l, w!(""), w!(""));
            }
            if let Ok(l) = ctrl!(w!("SysLink"),
                w!("<a href=\"mailto:andrew.chuev@gmail.com\">andrew.chuev@gmail.com</a>"),
                0, 16, 178, 248, 18, 0usize)
            {
                SendMessageW(l, WM_SETFONT, fwp, LPARAM(1));
                let _ = SetWindowTheme(l, w!(""), w!(""));
            }

            // ── Buttons ────────────────────────────────────────────────────
            if let Ok(b) = ctrl!(w!("BUTTON"), w!("Exit RMeters"),
                BS_PUSHBUTTON as u32, 16, 212, 120, 28, IDC_BTN_EXIT)
            {
                SendMessageW(b, WM_SETFONT, fwp, LPARAM(1));
                let _ = SetWindowTheme(b, w!("DarkMode_Explorer"), PCWSTR::null());
            }
            if let Ok(b) = ctrl!(w!("BUTTON"), w!("Close"),
                BS_PUSHBUTTON as u32, 144, 212, 120, 28, IDC_BTN_CLOSE)
            {
                SendMessageW(b, WM_SETFONT, fwp, LPARAM(1));
                let _ = SetWindowTheme(b, w!("DarkMode_Explorer"), PCWSTR::null());
            }

            SETTINGS_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
            LRESULT(0)
        }

        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            let dpi = GetDpiForWindow(hwnd);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let x1 = dpi_scale(16, dpi);
            let x2 = rc.right - dpi_scale(16, dpi);

            let pen = CreatePen(PS_SOLID, 1, COLORREF(COLOR_SEP));
            let old = SelectObject(hdc, HGDIOBJ(pen.0));
            for y in [dpi_scale(40, dpi), dpi_scale(124, dpi), dpi_scale(202, dpi)] {
                let _ = MoveToEx(hdc, x1, y, None);
                let _ = LineTo(hdc, x2, y);
            }
            SelectObject(hdc, old);
            let _ = DeleteObject(HGDIOBJ(pen.0));

            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }

        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut _);
            let ctrl = HWND(lparam.0 as *mut _);
            SetBkMode(hdc, TRANSPARENT);
            let id = GetDlgCtrlID(ctrl) as usize;
            let color = if id == IDC_LBL_VERSION {
                COLOR_WHITE
            } else if id == IDC_LBL_OPTIONS || id == IDC_LBL_ABOUT {
                COLOR_DIM
            } else {
                COLOR_TEXT
            };
            SetTextColor(hdc, COLORREF(color));
            LRESULT(dark_brush().0 as isize)
        }

        WM_CTLCOLORBTN => {
            let hdc = HDC(wparam.0 as *mut _);
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(COLOR_TEXT));
            LRESULT(dark_brush().0 as isize)
        }

        WM_NOTIFY => {
            // SAFETY: lparam for WM_NOTIFY is always a pointer to NMHDR as per Win32 contract.
            let nmhdr = &*(lparam.0 as *const NMHDR);
            if nmhdr.code == NM_CLICK {
                // SAFETY: NM_CLICK from a SysLink control guarantees lparam points to NMLINK.
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
                        match crate::config::set_autostart(checked) {
                            Ok(()) => {
                                crate::config::AUTOSTART_ENABLED.store(checked, Ordering::Relaxed);
                            }
                            Err(e) => {
                                crate::log_info(&format!("set_autostart failed: {e}"));
                            }
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

            let font_raw = SETTINGS_FONT.swap(0, Ordering::Relaxed);
            if font_raw != 0 {
                let _ = DeleteObject(HGDIOBJ(font_raw as *mut _));
            }

            // Release the cached background brush created in dark_brush().
            let brush_raw = DARK_BG_BRUSH.swap(0, Ordering::Relaxed);
            if brush_raw != 0 {
                let _ = DeleteObject(HGDIOBJ(brush_raw as *mut _));
            }

            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Opens the settings window centered on the monitor containing the overlay.
/// If the window is already open, brings it to the foreground instead.
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
            hbrBackground: dark_brush(),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let dpi = GetDpiForWindow(overlay_hwnd);
        let scale = dpi as f32 / 96.0;
        let win_w = (280.0 * scale) as i32;
        let win_h = (282.0 * scale) as i32;

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

unsafe fn set_window_icon(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        LoadImageW, IMAGE_FLAGS, IMAGE_ICON, LR_DEFAULTSIZE, WM_SETICON,
    };
    let Ok(hmod) = GetModuleHandleW(None) else { return };
    if let Ok(h) = LoadImageW(hmod, w!("IDI_APP_ICON"), IMAGE_ICON, 0, 0, LR_DEFAULTSIZE) {
        SendMessageW(hwnd, WM_SETICON, WPARAM(1), LPARAM(h.0 as isize));
    }
    if let Ok(h) = LoadImageW(hmod, w!("IDI_APP_ICON"), IMAGE_ICON, 16, 16, IMAGE_FLAGS(0)) {
        SendMessageW(hwnd, WM_SETICON, WPARAM(0), LPARAM(h.0 as isize));
    }
}

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

unsafe fn center_on_monitor(ref_hwnd: HWND, win_w: i32, win_h: i32) -> (i32, i32) {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    let hmon = MonitorFromWindow(ref_hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    if GetMonitorInfoW(hmon, &mut mi).as_bool() {
        let rc = mi.rcWork;
        return (
            rc.left + (rc.right - rc.left - win_w) / 2,
            rc.top  + (rc.bottom - rc.top  - win_h) / 2,
        );
    }
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    (
        (GetSystemMetrics(SM_CXSCREEN) - win_w) / 2,
        (GetSystemMetrics(SM_CYSCREEN) - win_h) / 2,
    )
}
