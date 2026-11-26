use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, DeleteDC, DeleteObject,
    GetDC, LineTo, MoveToEx, PatBlt, ReleaseDC, SelectObject, BLACKNESS, HGDIOBJ, PS_SOLID,
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
use crate::ring_icon::{create_ring_icon, Rgb, RingColors};

pub const WM_TRAY_CALLBACK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 1024;
const ID_TRAY_CPU: u32 = 1;
const ID_TRAY_RAM: u32 = 2;
/// Disk icons use ID_TRAY_DISK_BASE + drive index (A: = 0 … Z: = 25).
const ID_TRAY_DISK_BASE: u32 = 100;

// Sparkline colors in BGR order (Windows COLORREF convention).
const COLOR_CPU: u32 = 0x00FF9900;
const COLOR_RAM: u32 = 0x0033CC19;

// Disk ring colors (RGB).
const DISK_OK: Rgb = [60, 210, 110]; // green
const DISK_WARN: Rgb = [255, 210, 0]; // yellow
const DISK_CRIT: Rgb = [255, 60, 60]; // red
const DISK_FREE: Rgb = [90, 90, 90];
const DISK_HOLE: Rgb = [28, 28, 28];

// Used-space thresholds in percent.
const DISK_WARN_PCT: f64 = 85.0;
const DISK_CRIT_PCT: f64 = 95.0;

#[derive(Debug, PartialEq)]
enum DiskLevel {
    Ok,
    Low,
    Critical,
}

fn used_pct(free_bytes: u64, total_bytes: u64) -> f64 {
    if total_bytes == 0 {
        return 0.0;
    }
    let used = total_bytes - free_bytes.min(total_bytes);
    used as f64 * 100.0 / total_bytes as f64
}

fn disk_level(used_pct: f64) -> DiskLevel {
    if used_pct >= DISK_CRIT_PCT {
        DiskLevel::Critical
    } else if used_pct >= DISK_WARN_PCT {
        DiskLevel::Low
    } else {
        DiskLevel::Ok
    }
}

/// Icons currently registered with the shell: (uID, HICON as isize).
/// Shared between the monitoring thread and the UI thread.
static ADDED: Mutex<Vec<(u32, isize)>> = Mutex::new(Vec::new());

/// What a tray icon draws.
enum IconKind<'a> {
    /// Scrolling history graph in a BGR color.
    Sparkline {
        color: u32,
        history: &'a VecDeque<f32>,
    },
    /// Donut chart of a used share (0..=1) with a centered label.
    Ring {
        used: f32,
        color: Rgb,
        label: String,
    },
}

/// One tray icon to show on this tick.
struct IconSpec<'a> {
    id: u32,
    kind: IconKind<'a>,
    tip: String,
}

fn set_tooltip(tip: &mut [u16; 128], text: &str) {
    let wide: Vec<u16> = text
        .encode_utf16()
        .take(127)
        .chain(std::iter::once(0))
        .collect();
    tip[..wide.len()].copy_from_slice(&wide);
}

unsafe fn create_sparkline_icon(
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

        // Scale value to full icon height. At least one pixel is drawn so an idle
        // metric still shows a baseline instead of an invisible icon.
        let h = ((val.clamp(0.0, 100.0) / 100.0 * (cy - 1) as f32) as i32).max(1);

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
            kind: IconKind::Sparkline {
                color: COLOR_CPU,
                history: state.cpu_history.values(),
            },
            tip: format!("CPU: {:.0}%", state.cpu_usage),
        });
    }
    if crate::config::SHOW_RAM.load(Ordering::Relaxed) {
        specs.push(IconSpec {
            id: ID_TRAY_RAM,
            kind: IconKind::Sparkline {
                color: COLOR_RAM,
                history: state.ram_history.values(),
            },
            tip: format!("RAM: {:.0}%", state.ram_usage_pct),
        });
    }
    if crate::config::SHOW_DISKS.load(Ordering::Relaxed) {
        for d in &state.disks {
            let pct = used_pct(d.free_bytes, d.total_bytes);
            let (color, note) = match disk_level(pct) {
                DiskLevel::Ok => (DISK_OK, ""),
                DiskLevel::Low => (DISK_WARN, " (low space)"),
                DiskLevel::Critical => (DISK_CRIT, " (critically low)"),
            };
            specs.push(IconSpec {
                id: ID_TRAY_DISK_BASE + (d.letter as u32 - 'A' as u32),
                kind: IconKind::Ring {
                    used: (pct / 100.0) as f32,
                    color,
                    label: d.letter.to_string(),
                },
                tip: format!(
                    "Disk {}: {:.0}% used{}\n{} free of {}",
                    d.letter,
                    pct,
                    note,
                    format_bytes(d.free_bytes),
                    format_bytes(d.total_bytes),
                ),
            });
        }
    }
    specs
}

/// Formats a byte count with binary units, as Explorer does (but labelled GB/TB).
fn format_bytes(bytes: u64) -> String {
    const GB: f64 = (1u64 << 30) as f64;
    const TB: f64 = (1u64 << 40) as f64;
    let b = bytes as f64;
    if b >= TB {
        format!("{:.2} TB", b / TB)
    } else {
        format!("{:.1} GB", b / GB)
    }
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
            let icon = match &spec.kind {
                IconKind::Sparkline { color, history } => {
                    create_sparkline_icon(history, *color, cx, cy)
                }
                IconKind::Ring { used, color, label } => {
                    let colors = RingColors {
                        used: *color,
                        free: DISK_FREE,
                        hole: DISK_HOLE,
                    };
                    create_ring_icon(cx as usize, *used, &colors, label)
                }
            };
            let Ok(hicon) = icon else {
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

#[cfg(test)]
mod tests {
    use super::{disk_level, format_bytes, used_pct, DiskLevel};

    fn level(free: u64, total: u64) -> DiskLevel {
        disk_level(used_pct(free, total))
    }

    #[test]
    fn disk_level_follows_used_space_thresholds() {
        assert_eq!(level(50, 100), DiskLevel::Ok);
        assert_eq!(level(16, 100), DiskLevel::Ok);
        assert_eq!(level(15, 100), DiskLevel::Low);
        assert_eq!(level(6, 100), DiskLevel::Low);
        assert_eq!(level(5, 100), DiskLevel::Critical);
        assert_eq!(level(0, 100), DiskLevel::Critical);
        assert_eq!(level(0, 0), DiskLevel::Ok);
    }

    #[test]
    fn used_pct_handles_bad_input() {
        assert_eq!(used_pct(25, 100), 75.0);
        assert_eq!(used_pct(200, 100), 0.0);
        assert_eq!(used_pct(0, 0), 0.0);
    }

    #[test]
    fn formats_gigabytes_and_terabytes() {
        assert_eq!(format_bytes(0), "0.0 GB");
        assert_eq!(format_bytes(512 << 30), "512.0 GB");
        assert_eq!(format_bytes(3 << 39), "1.50 TB");
    }
}
