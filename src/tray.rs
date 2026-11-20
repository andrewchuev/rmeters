use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, DeleteDC, DeleteObject,
    GetDC, HGDIOBJ, LineTo, MoveToEx, PatBlt, ReleaseDC, SelectObject, BLACKNESS, PS_SOLID,
    WHITENESS,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, GetSystemMetrics, HICON, ICONINFO, SM_CXSMICON, SM_CYSMICON,
};

use crate::metrics::MetricsState;

pub const WM_TRAY_CALLBACK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 1024;
const ID_TRAY_CPU: u32 = 1;
const ID_TRAY_RAM: u32 = 2;

// Colors in BGR order (Windows COLORREF convention).
const COLOR_CPU: u32 = 0x00FF9900;
const COLOR_RAM: u32 = 0x0033CC19;

/// Icons currently registered with the shell: (uID, HICON as isize).
/// Shared between the monitoring thread and the UI thread.
static ADDED: Mutex<Vec<(u32, isize)>> = Mutex::new(Vec::new());

/// One tray icon to show on this tick.
struct IconSpec<'a> {
    id: u32,
    color: u32,
    history: &'a VecDeque<f32>,
    tip: String,
}

fn set_tooltip(tip: &mut [u16; 128], text: &str) {
    let wide: Vec<u16> = text.encode_utf16().take(127).chain(std::iter::once(0)).collect();
    tip[..wide.len()].copy_from_slice(&wide);
}

unsafe fn create_dynamic_icon(
    history: &VecDeque<f32>,
    color: u32,
    cx: i32,
    cy: i32,
) -> Result<HICON, windows::core::Error> {
    let hdc_screen = GetDC(HWND::default());
    let hdc_mem = CreateCompatibleDC(hdc_screen);
    let hbm_color = CreateCompatibleBitmap(hdc_screen, cx, cy);
    let old_color_bitmap = SelectObject(hdc_mem, HGDIOBJ(hbm_color.0));

    // Fill color bitmap with black (opaque base for transparent icon)
    let _ = PatBlt(hdc_mem, 0, 0, cx, cy, BLACKNESS);

    let hbm_mask = CreateBitmap(cx, cy, 1, 1, None);
    if hbm_mask.0.is_null() {
        let _ = DeleteObject(HGDIOBJ(hbm_color.0));
        SelectObject(hdc_mem, old_color_bitmap);
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(HWND::default(), hdc_screen);
        return Err(windows::core::Error::from_win32());
    }
    let hdc_mask = CreateCompatibleDC(hdc_screen);
    let old_mask_bitmap = SelectObject(hdc_mask, HGDIOBJ(hbm_mask.0));

    // Fill mask bitmap with white (1 = transparent)
    let _ = PatBlt(hdc_mask, 0, 0, cx, cy, WHITENESS);

    // Setup pens (glowing color and black mask pen)
    let pen_graph = CreatePen(PS_SOLID, 1, COLORREF(color));
    let pen_black = CreatePen(PS_SOLID, 1, COLORREF(0));

    let old_color_pen = SelectObject(hdc_mem, HGDIOBJ(pen_graph.0));
    let old_mask_pen = SelectObject(hdc_mask, HGDIOBJ(pen_black.0));

    // Draw the sparkline columns spanning the full width of the icon
    let hist_len = history.len();
    for x in 0..cx {
        let val = if hist_len >= cx as usize {
            history[hist_len - cx as usize + x as usize]
        } else if x as usize >= cx as usize - hist_len {
            history[x as usize - (cx as usize - hist_len)]
        } else {
            0.0
        };

        // Scale value to full icon height
        let h = (val.clamp(0.0, 100.0) / 100.0 * (cy - 1) as f32) as i32;

        let _ = MoveToEx(hdc_mem, x, cy - 1, None);
        let _ = LineTo(hdc_mem, x, cy - 1 - h);

        let _ = MoveToEx(hdc_mask, x, cy - 1, None);
        let _ = LineTo(hdc_mask, x, cy - 1 - h);
    }

    // Clean up drawing resources
    SelectObject(hdc_mem, old_color_pen);
    SelectObject(hdc_mask, old_mask_pen);
    let _ = DeleteObject(HGDIOBJ(pen_graph.0));
    let _ = DeleteObject(HGDIOBJ(pen_black.0));

    SelectObject(hdc_mem, old_color_bitmap);
    SelectObject(hdc_mask, old_mask_bitmap);
    let _ = DeleteDC(hdc_mem);
    let _ = DeleteDC(hdc_mask);
    let _ = ReleaseDC(HWND::default(), hdc_screen);

    let icon_info = ICONINFO {
        fIcon: windows::Win32::Foundation::BOOL::from(true),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: hbm_mask,
        hbmColor: hbm_color,
    };
    let hicon = CreateIconIndirect(&icon_info);

    let _ = DeleteObject(HGDIOBJ(hbm_color.0));
    let _ = DeleteObject(HGDIOBJ(hbm_mask.0));

    hicon
}

fn base_nid(hwnd: HWND, id: u32) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: id,
        ..Default::default()
    }
}

fn icon_specs(state: &MetricsState) -> Vec<IconSpec<'_>> {
    let mut specs = Vec::new();
    if crate::config::SHOW_CPU.load(Ordering::Relaxed) {
        specs.push(IconSpec {
            id: ID_TRAY_CPU,
            color: COLOR_CPU,
            history: state.cpu_history.values(),
            tip: format!("CPU: {:.0}%", state.cpu_usage),
        });
    }
    if crate::config::SHOW_RAM.load(Ordering::Relaxed) {
        specs.push(IconSpec {
            id: ID_TRAY_RAM,
            color: COLOR_RAM,
            history: state.ram_history.values(),
            tip: format!("RAM: {:.0}%", state.ram_usage_pct),
        });
    }
    specs
}

/// Adds, refreshes and removes tray icons so they match the current settings and metrics.
pub fn update_tray(hwnd: HWND, state: &MetricsState) {
    let specs = icon_specs(state);
    let mut added = ADDED.lock().unwrap_or_else(|p| p.into_inner());
    // Checked under the lock so an update cannot race past remove_tray on exit.
    if crate::metrics::MAIN_HWND.load(Ordering::Relaxed) == 0 {
        return;
    }

    unsafe {
        // Remove icons that are no longer wanted.
        added.retain(|&(id, hicon)| {
            if specs.iter().any(|s| s.id == id) {
                return true;
            }
            let _ = Shell_NotifyIconW(NIM_DELETE, &base_nid(hwnd, id));
            let _ = DestroyIcon(HICON(hicon as *mut _));
            false
        });

        let cx = GetSystemMetrics(SM_CXSMICON);
        let cy = GetSystemMetrics(SM_CYSMICON);

        for spec in &specs {
            let Ok(hicon) = create_dynamic_icon(spec.history, spec.color, cx, cy) else {
                continue;
            };
            let mut nid = NOTIFYICONDATAW {
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_TRAY_CALLBACK,
                hIcon: hicon,
                ..base_nid(hwnd, spec.id)
            };
            set_tooltip(&mut nid.szTip, &spec.tip);

            if let Some(entry) = added.iter_mut().find(|(id, _)| *id == spec.id) {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
                let _ = DestroyIcon(HICON(entry.1 as *mut _));
                entry.1 = hicon.0 as isize;
            } else if Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                added.push((spec.id, hicon.0 as isize));
            } else {
                let _ = DestroyIcon(hicon);
            }
        }
    }
}

/// Removes all tray icons; called on exit.
pub fn remove_tray(hwnd: HWND) {
    let mut added = ADDED.lock().unwrap_or_else(|p| p.into_inner());
    for (id, hicon) in added.drain(..) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &base_nid(hwnd, id));
            let _ = DestroyIcon(HICON(hicon as *mut _));
        }
    }
}

/// Forgets registered icons after Explorer restarts (its icon list is gone),
/// so the next update re-adds them.
pub fn reset_after_shell_restart() {
    let mut added = ADDED.lock().unwrap_or_else(|p| p.into_inner());
    for (_, hicon) in added.drain(..) {
        unsafe {
            let _ = DestroyIcon(HICON(hicon as *mut _));
        }
    }
}
