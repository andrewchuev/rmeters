use windows::core::w;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, PostMessageW, SetForegroundWindow,
    TrackPopupMenu, MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED,
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WM_NULL,
};

pub const ID_EXIT: usize = 1001;
pub const ID_SHOW_PER_CORE: usize = 1002;
pub const ID_AUTOSTART: usize = 1003;
pub const ID_SETTINGS: usize = 1004;

pub fn show_context_menu(hwnd: HWND) {
    unsafe {
        let hmenu = CreatePopupMenu().expect("Failed to create popup menu");

        let show_per_core = crate::config::SHOW_PER_CORE.load(std::sync::atomic::Ordering::Relaxed);
        let core_flags = if show_per_core { MF_CHECKED } else { MF_UNCHECKED };

        let autostart_enabled = crate::config::AUTOSTART_ENABLED.load(std::sync::atomic::Ordering::Relaxed);
        let autostart_flags = if autostart_enabled { MF_CHECKED } else { MF_UNCHECKED };

        let _ = AppendMenuW(hmenu, MF_STRING | core_flags, ID_SHOW_PER_CORE, w!("Show CPU per Core"));
        let _ = AppendMenuW(hmenu, MF_STRING | autostart_flags, ID_AUTOSTART, w!("Start with Windows"));
        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, w!(""));
        let _ = AppendMenuW(hmenu, MF_STRING, ID_SETTINGS, w!("Settings..."));
        let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, w!(""));
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

        let _ = DestroyMenu(hmenu);
        let _ = PostMessageW(hwnd, WM_NULL, None, None);
    }
}
