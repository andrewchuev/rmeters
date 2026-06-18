#![windows_subsystem = "windows"]

mod config;
mod metrics;
mod renderer;
mod tray;
mod window;

use std::fs::OpenOptions;
use std::io::Write;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, TranslateMessage, DispatchMessageW, MSG, WM_CLOSE,
};

#[link(name = "kernel32")]
extern "system" {
    fn SetConsoleCtrlHandler(
        handlerroutine: Option<unsafe extern "system" fn(u32) -> windows::Win32::Foundation::BOOL>,
        add: windows::Win32::Foundation::BOOL,
    ) -> windows::Win32::Foundation::BOOL;
}

unsafe extern "system" fn ctrl_handler(_ctrl_type: u32) -> windows::Win32::Foundation::BOOL {
    log_info("Ctrl+C or console event received, exiting...");
    let hwnd_raw = {
        if let Ok(handle) = crate::metrics::OVERLAY_HWND.read() {
            *handle
        } else {
            None
        }
    };
    if let Some(raw) = hwnd_raw {
        let hwnd = windows::Win32::Foundation::HWND(raw as *mut std::ffi::c_void);
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            hwnd,
            WM_CLOSE,
            None,
            None,
        );
    }
    windows::Win32::Foundation::BOOL::from(true)
}

pub fn log_info(msg: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("D:\\Projects\\internal\\rmeters\\run_log.txt")
    {
        let _ = writeln!(file, "[LOG] {}", msg);
    }
}

fn main() -> windows::core::Result<()> {
    // Clear log file at start
    std::fs::write("D:\\Projects\\internal\\rmeters\\run_log.txt", "").ok();
    
    log_info("rmeters starting... ");

    // Register console control handler to handle Ctrl+C
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(ctrl_handler), windows::Win32::Foundation::BOOL::from(true));
        log_info("Console control handler registered");
    }

    // 0. Load layout configuration
    config::load_config();
    log_info("Configuration loaded");

    // 1. Set process DPI awareness so that window bounds and coordinates align properly
    unsafe {
        let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
        log_info("DPI awareness set");
    }

    // 2. Initialize COM for the main thread (used by shell elements and DirectWrite)
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        log_info("COM initialized");
    }

    // 3. Start the system metrics polling thread
    metrics::start_monitoring();
    log_info("Monitoring thread started");

    // 4. Create the overlay window
    let hwnd = window::create_overlay();
    log_info(&format!("Overlay window created: HWND = {:?}", hwnd.0));

    // 5. Store the raw handle value for the monitoring thread to trigger repaints
    {
        if let Ok(mut handle) = metrics::OVERLAY_HWND.write() {
            *handle = Some(hwnd.0 as isize);
        }
    }

    // 6. Register the system tray icon
    tray::add_tray_icon(hwnd);
    log_info("Tray icon registered");

    // 7. Main message loop
    log_info("Entering message loop...");
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    log_info("Message loop exited");

    // 8. Clean up COM resources on exit
    unsafe {
        CoUninitialize();
        log_info("COM uninitialized");
    }

    Ok(())
}
