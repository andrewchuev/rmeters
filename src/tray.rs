use windows::core::w;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, NIF_ICON, NIF_MESSAGE, NIF_TIP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, GetCursorPos, LoadIconW, PostMessageW, SetForegroundWindow,
    TrackPopupMenu, MF_STRING, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WM_NULL,
};

pub const WM_TRAY_ICON: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 1;
pub const TRAY_ICON_ID: u32 = 1;
pub const ID_EXIT: usize = 1001;
pub const ID_SHOW_PER_CORE: usize = 1002;
pub const ID_AUTOSTART: usize = 1003;

pub fn add_tray_icon(hwnd: HWND) {
    unsafe {
        let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
        let hicon = LoadIconW(instance, w!("IDI_APP_ICON")).expect("Failed to load icon");

        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY_ICON,
            hIcon: hicon,
            ..Default::default()
        };

        let tip = w!("rmeters System Monitor");
        let tip_slice = tip.as_wide();
        let copy_len = std::cmp::min(tip_slice.len(), nid.szTip.len() - 1);
        nid.szTip[..copy_len].copy_from_slice(&tip_slice[..copy_len]);

        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

pub fn remove_tray_icon(hwnd: HWND) {
    unsafe {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            ..Default::default()
        };
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

pub fn show_tray_menu(hwnd: HWND) {
    unsafe {
        let hmenu = CreatePopupMenu().expect("Failed to create popup menu");
        
        use windows::Win32::UI::WindowsAndMessaging::{MF_CHECKED, MF_UNCHECKED};
        let show_per_core = crate::config::SHOW_PER_CORE.load(std::sync::atomic::Ordering::Relaxed);
        let core_flags = if show_per_core { MF_CHECKED } else { MF_UNCHECKED };
        
        let autostart_enabled = crate::config::AUTOSTART_ENABLED.load(std::sync::atomic::Ordering::Relaxed);
        let autostart_flags = if autostart_enabled { MF_CHECKED } else { MF_UNCHECKED };
        
        let _ = AppendMenuW(hmenu, MF_STRING | core_flags, ID_SHOW_PER_CORE, w!("Show CPU per Core"));
        let _ = AppendMenuW(hmenu, MF_STRING | autostart_flags, ID_AUTOSTART, w!("Start with Windows"));
        let _ = AppendMenuW(hmenu, MF_STRING, ID_EXIT, w!("Exit"));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);

        // Required so that clicking outside the menu dismisses it
        let _ = SetForegroundWindow(hwnd);

        let _ = TrackPopupMenu(
            hmenu,
            TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            0,
            hwnd,
            None,
        );

        let _ = PostMessageW(hwnd, WM_NULL, None, None);
    }
}
