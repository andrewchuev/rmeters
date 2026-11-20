// Win32 UI code uses many size_of::<T>() as u32 and DPI f32→i32 conversions
// that are always in range. Allow the lint rather than adding try_from noise.
#![allow(clippy::cast_possible_truncation, clippy::zero_ptr)]

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontIndirectW, CreatePen, CreateSolidBrush, DeleteObject, EndPaint,
    FillRect, HBRUSH, HDC, HGDIOBJ, HMONITOR, LineTo, MonitorFromPoint, MoveToEx,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, PS_SOLID, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    ICC_LINK_CLASS, INITCOMMONCONTROLSEX, InitCommonControlsEx, NMHDR, NMLINK, NM_CLICK,
    SetWindowTheme,
};
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_PUSHBUTTON,
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
    GetClientRect, GetCursorPos, GetDlgCtrlID, HMENU, IsIconic, NONCLIENTMETRICSW,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowTextW,
    ShowWindow, SPI_GETNONCLIENTMETRICS, SW_RESTORE, SW_SHOW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SystemParametersInfoW, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
    WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_DESTROY,
    WM_ERASEBKGND, WM_NOTIFY, WM_PAINT, WM_SETFONT, WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU,
    WS_VISIBLE,
};

const IDC_CHK_AUTOSTART: usize = 2002;
const IDC_BTN_EXIT: usize = 2003;
const IDC_BTN_CLOSE: usize = 2004;
const IDC_CHK_CPU: usize = 2008;
const IDC_CHK_RAM: usize = 2009;
const IDC_CHK_DISKS: usize = 2010;
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

const ICON_OPTION_COUNT: usize = 3;

/// Icon-visibility checkboxes: (control id, label, config flag).
fn icon_options() -> [(usize, PCWSTR, &'static AtomicBool); ICON_OPTION_COUNT] {
    [
        (IDC_CHK_CPU, w!("Show CPU Icon"), &crate::config::SHOW_CPU),
        (IDC_CHK_RAM, w!("Show RAM Icon"), &crate::config::SHOW_RAM),
        (IDC_CHK_DISKS, w!("Show Disk Icons"), &crate::config::SHOW_DISKS),
    ]
}

// Vertical layout in logical pixels, derived from the number of option rows.
// All values are client-area coordinates; the window size is derived from them.
const PAD: i32 = 16; // outer margin on every side
const SEP_GAP: i32 = 12; // space above and below each separator
const CLIENT_W: i32 = 280;
const CONTENT_W: i32 = CLIENT_W - 2 * PAD;
const BTN_GAP: i32 = 8;
const BTN_W: i32 = (CONTENT_W - BTN_GAP) / 2;
const BTN_H: i32 = 28;
const ROW_H: i32 = 22;
const LABEL_H: i32 = 16;
const LINK_H: i32 = 18;
const VERSION_H: i32 = 20;

const VERSION_TOP: i32 = PAD;
const SEP_OPTIONS: i32 = VERSION_TOP + VERSION_H + SEP_GAP;
const OPTIONS_LABEL_TOP: i32 = SEP_OPTIONS + SEP_GAP;
const OPTIONS_TOP: i32 = OPTIONS_LABEL_TOP + LABEL_H + 4;
const OPTION_ROWS: i32 = ICON_OPTION_COUNT as i32 + 1; // + autostart
const SEP_ABOUT: i32 = OPTIONS_TOP + OPTION_ROWS * ROW_H + SEP_GAP;
const ABOUT_TOP: i32 = SEP_ABOUT + SEP_GAP;
const LINKS_TOP: i32 = ABOUT_TOP + LABEL_H + 4;
const SEP_BUTTONS: i32 = LINKS_TOP + 2 * LINK_H + 4 + SEP_GAP;
const BUTTONS_TOP: i32 = SEP_BUTTONS + SEP_GAP;
const CLIENT_H: i32 = BUTTONS_TOP + BTN_H + PAD;

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

            let autostart = crate::config::AUTOSTART_ENABLED.load(Ordering::Relaxed);

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
            let ver = HSTRING::from(format!("RMeters  v{}", env!("CARGO_PKG_VERSION")));
            if let Ok(l) = ctrl!(w!("STATIC"), w!(""), 0, PAD, VERSION_TOP, CONTENT_W, VERSION_H, IDC_LBL_VERSION) {
                let _ = SetWindowTextW(l, &ver);
                SendMessageW(l, WM_SETFONT, fwp, LPARAM(1));
            }

            // ── Options ────────────────────────────────────────────────────
            if let Ok(l) = ctrl!(w!("STATIC"), w!("OPTIONS"), 0, PAD, OPTIONS_LABEL_TOP, CONTENT_W, LABEL_H, IDC_LBL_OPTIONS) {
                SendMessageW(l, WM_SETFONT, fwp, LPARAM(1));
            }
            let mut row_y = OPTIONS_TOP;
            for (id, label, flag) in icon_options() {
                if let Ok(c) = ctrl!(w!("BUTTON"), label,
                    BS_AUTOCHECKBOX as u32, PAD, row_y, CONTENT_W, ROW_H, id)
                {
                    let checked = flag.load(Ordering::Relaxed);
                    SendMessageW(c, BM_SETCHECK, WPARAM(checked as usize), LPARAM(0));
                    SendMessageW(c, WM_SETFONT, fwp, LPARAM(1));
                    let _ = SetWindowTheme(c, w!("DarkMode_Explorer"), PCWSTR::null());
                }
                row_y += ROW_H;
            }
            if let Ok(c) = ctrl!(w!("BUTTON"), w!("Start with Windows"),
                BS_AUTOCHECKBOX as u32, PAD, row_y, CONTENT_W, ROW_H, IDC_CHK_AUTOSTART)
            {
                SendMessageW(c, BM_SETCHECK, WPARAM(autostart as usize), LPARAM(0));
                SendMessageW(c, WM_SETFONT, fwp, LPARAM(1));
                let _ = SetWindowTheme(c, w!("DarkMode_Explorer"), PCWSTR::null());
            }

            // ── About ──────────────────────────────────────────────────────
            if let Ok(l) = ctrl!(w!("STATIC"), w!("ABOUT"), 0, PAD, ABOUT_TOP, CONTENT_W, LABEL_H, IDC_LBL_ABOUT) {
                SendMessageW(l, WM_SETFONT, fwp, LPARAM(1));
            }
            if let Ok(l) = ctrl!(w!("SysLink"),
                w!("<a href=\"https://rmeters.reslab.pro\">rmeters.reslab.pro</a>"),
                0, PAD, LINKS_TOP, CONTENT_W, LINK_H, 0usize)
            {
                SendMessageW(l, WM_SETFONT, fwp, LPARAM(1));
                let _ = SetWindowTheme(l, w!(""), w!(""));
            }
            if let Ok(l) = ctrl!(w!("SysLink"),
                w!("<a href=\"mailto:andrew.chuev@gmail.com\">andrew.chuev@gmail.com</a>"),
                0, PAD, LINKS_TOP + LINK_H + 4, CONTENT_W, LINK_H, 0usize)
            {
                SendMessageW(l, WM_SETFONT, fwp, LPARAM(1));
                let _ = SetWindowTheme(l, w!(""), w!(""));
            }

            // ── Buttons ────────────────────────────────────────────────────
            if let Ok(b) = ctrl!(w!("BUTTON"), w!("Exit RMeters"),
                BS_PUSHBUTTON as u32, PAD, BUTTONS_TOP, BTN_W, BTN_H, IDC_BTN_EXIT)
            {
                SendMessageW(b, WM_SETFONT, fwp, LPARAM(1));
                let _ = SetWindowTheme(b, w!("DarkMode_Explorer"), PCWSTR::null());
            }
            if let Ok(b) = ctrl!(w!("BUTTON"), w!("Close"),
                BS_PUSHBUTTON as u32, CLIENT_W - PAD - BTN_W, BUTTONS_TOP, BTN_W, BTN_H, IDC_BTN_CLOSE)
            {
                SendMessageW(b, WM_SETFONT, fwp, LPARAM(1));
                let _ = SetWindowTheme(b, w!("DarkMode_Explorer"), PCWSTR::null());
            }

            SETTINGS_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
            LRESULT(0)
        }

        WM_ERASEBKGND => {
            let hdc = HDC(wparam.0 as *mut _);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            FillRect(hdc, &rc, dark_brush());
            LRESULT(1)
        }

        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            let dpi = GetDpiForWindow(hwnd);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let x1 = dpi_scale(PAD, dpi);
            let x2 = rc.right - dpi_scale(PAD, dpi);

            let pen = CreatePen(PS_SOLID, 1, COLORREF(COLOR_SEP));
            let old = SelectObject(hdc, HGDIOBJ(pen.0));
            for y in [dpi_scale(SEP_OPTIONS, dpi), dpi_scale(SEP_ABOUT, dpi), dpi_scale(SEP_BUTTONS, dpi)] {
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
                ShellExecuteW(HWND::default(), w!("open"), url,
                    PCWSTR::null(), PCWSTR::null(), SW_SHOW);
            }
            LRESULT(0)
        }

        WM_COMMAND => {
            let id     = wparam.0 & 0xffff;
            let notify = (wparam.0 >> 16) as u32;
            let ctrl   = HWND(lparam.0 as *mut _);

            if notify == BN_CLICKED {
                if let Some((_, _, flag)) = icon_options().into_iter().find(|o| o.0 == id) {
                    let checked = SendMessageW(ctrl, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == 1;
                    flag.store(checked, Ordering::Relaxed);
                    if crate::config::any_icon_enabled() {
                        // The monitoring thread applies the change on its next tick.
                        crate::config::save_config();
                    } else {
                        // Keep at least one tray icon so settings stay reachable.
                        flag.store(true, Ordering::Relaxed);
                        SendMessageW(ctrl, BM_SETCHECK, WPARAM(1), LPARAM(0));
                    }
                    return LRESULT(0);
                }
                match id {
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
                        let _ = DestroyWindow(hwnd);
                        let main = crate::metrics::MAIN_HWND.load(Ordering::Relaxed);
                        if main != 0 {
                            let _ = DestroyWindow(HWND(main as *mut _));
                        }
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

/// Opens the settings window centered on the monitor under the cursor (the clicked tray icon).
/// If the window is already open, brings it to the foreground instead.
pub fn show_settings() {
    unsafe {
        if let Some(existing) = get_settings_hwnd() {
            bring_to_front(existing);
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

        let hmon = cursor_monitor();
        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        // The layout is in client coordinates; add the title bar and borders on top.
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: dpi_scale(CLIENT_W, dpi_x),
            bottom: dpi_scale(CLIENT_H, dpi_x),
        };
        let _ = AdjustWindowRectExForDpi(&mut rc, style, false, WINDOW_EX_STYLE(0), dpi_x);
        let win_w = rc.right - rc.left;
        let win_h = rc.bottom - rc.top;

        let (x, y) = center_on_monitor(hmon, win_w, win_h);

        if let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0), class_name, w!("RMeters \u{2014} Settings"),
            style,
            x, y, win_w, win_h,
            None, HMENU::default(), hmod,
            None,
        ) {
            set_window_icon(hwnd);
            apply_dark_titlebar(hwnd);
            let _ = ShowWindow(hwnd, SW_SHOW);
            bring_to_front(hwnd);
        }
    }
}

unsafe fn bring_to_front(hwnd: HWND) {
    // The process's first ShowWindow call takes the launch show state instead of
    // the one passed (e.g. minimized when started from a minimized shortcut or a
    // background shell). The settings window is now the first window ever shown,
    // so restore it explicitly.
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }
    // Tray clicks do not give us foreground rights by default; the shell
    // allows it right after a notification-icon callback.
    let _ = SetForegroundWindow(hwnd);
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

unsafe fn cursor_monitor() -> HMONITOR {
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST)
}

unsafe fn center_on_monitor(hmon: HMONITOR, win_w: i32, win_h: i32) -> (i32, i32) {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
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
