use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
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

pub const WM_TRAY_CALLBACK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 1024;
pub const ID_TRAY_CPU: u32 = 1;
pub const ID_TRAY_RAM: u32 = 2;

static HICON_CPU: AtomicIsize = AtomicIsize::new(0);
static HICON_RAM: AtomicIsize = AtomicIsize::new(0);
static TRAY_ADDED: AtomicBool = AtomicBool::new(false);

fn set_tooltip(tip: &mut [u16; 128], text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let len = wide.len().min(128);
    tip[..len].copy_from_slice(&wide[..len]);
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
        let h = (val / 100.0 * (cy - 1) as f32) as i32;

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
    let hicon = CreateIconIndirect(&icon_info)?;

    let _ = DeleteObject(HGDIOBJ(hbm_color.0));
    let _ = DeleteObject(HGDIOBJ(hbm_mask.0));

    Ok(hicon)
}

pub fn update_tray(
    hwnd: HWND,
    cpu: f32,
    ram: f32,
    cpu_history: &VecDeque<f32>,
    ram_history: &VecDeque<f32>,
) {
    let show = crate::config::SHOW_TRAY.load(Ordering::Relaxed);
    if !show {
        remove_tray(hwnd);
        return;
    }

    unsafe {
        let cx = GetSystemMetrics(SM_CXSMICON);
        let cy = GetSystemMetrics(SM_CYSMICON);

        // CPU Icon (light blue: BGR format is 0x00FF9900)
        let cpu_color = 0x00FF9900;
        let cpu_hicon = match create_dynamic_icon(cpu_history, cpu_color, cx, cy) {
            Ok(h) => h,
            Err(_) => return,
        };

        // RAM Icon (light green: BGR format is 0x0033CC19)
        let ram_color = 0x0033CC19;
        let ram_hicon = match create_dynamic_icon(ram_history, ram_color, cx, cy) {
            Ok(h) => h,
            Err(_) => {
                let _ = DestroyIcon(cpu_hicon);
                return;
            }
        };

        let mut nid_cpu = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: ID_TRAY_CPU,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY_CALLBACK,
            hIcon: cpu_hicon,
            ..Default::default()
        };
        set_tooltip(&mut nid_cpu.szTip, &format!("CPU: {:.0}%", cpu));

        let mut nid_ram = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: ID_TRAY_RAM,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY_CALLBACK,
            hIcon: ram_hicon,
            ..Default::default()
        };
        set_tooltip(&mut nid_ram.szTip, &format!("RAM: {:.0}%", ram));

        let added = TRAY_ADDED.load(Ordering::Relaxed);
        if added {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid_cpu);
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid_ram);
        } else {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid_cpu);
            let _ = Shell_NotifyIconW(NIM_ADD, &nid_ram);
            TRAY_ADDED.store(true, Ordering::Relaxed);
        }

        // Clean up previous icon handles
        let old_cpu = HICON_CPU.swap(cpu_hicon.0 as isize, Ordering::Relaxed);
        if old_cpu != 0 {
            let _ = DestroyIcon(HICON(old_cpu as *mut _));
        }

        let old_ram = HICON_RAM.swap(ram_hicon.0 as isize, Ordering::Relaxed);
        if old_ram != 0 {
            let _ = DestroyIcon(HICON(old_ram as *mut _));
        }
    }
}

pub fn remove_tray(hwnd: HWND) {
    let added = TRAY_ADDED.swap(false, Ordering::Relaxed);
    if added {
        unsafe {
            let nid_cpu = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: ID_TRAY_CPU,
                ..Default::default()
            };
            let nid_ram = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: ID_TRAY_RAM,
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid_cpu);
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid_ram);

            let old_cpu = HICON_CPU.swap(0, Ordering::Relaxed);
            if old_cpu != 0 {
                let _ = DestroyIcon(HICON(old_cpu as *mut _));
            }

            let old_ram = HICON_RAM.swap(0, Ordering::Relaxed);
            if old_ram != 0 {
                let _ = DestroyIcon(HICON(old_ram as *mut _));
            }
        }
    }
}
