#![windows_subsystem = "windows"]

mod config;
mod metrics;
mod renderer;
mod settings;
mod window;

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::Ordering;
use windows::core::w;
use windows::Win32::Foundation::{BOOL, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, TranslateMessage, DispatchMessageW, MSG, WM_CLOSE,
};

fn log_path() -> std::path::PathBuf {
    config::data_dir().join("run_log.txt")
}

pub fn log_info(msg: &str) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path()) {
        let _ = writeln!(file, "[LOG] {}", msg);
    }
}

unsafe extern "system" fn ctrl_handler(_ctrl_type: u32) -> BOOL {
    log_info("Ctrl+C or console event received, exiting...");
    let raw = crate::metrics::OVERLAY_HWND.load(Ordering::Relaxed);
    if raw != 0 {
        // SAFETY: raw is the HWND stored by main after the overlay window was created.
        let hwnd = windows::Win32::Foundation::HWND(raw as *mut std::ffi::c_void);
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(hwnd, WM_CLOSE, None, None);
    }
    BOOL::from(true)
}

fn main() -> windows::core::Result<()> {
    // Ensure the data directory exists before the first log write.
    let _ = std::fs::create_dir_all(config::data_dir());

    // Clear the log file on each startup.
    let _ = std::fs::write(log_path(), "");

    log_info("rmeters starting...");

    // Ensure only one instance is running.
    // SAFETY: CreateMutexW is a straightforward Win32 call with no alignment
    // or lifetime requirements beyond the arguments passed.
    let mutex_result = unsafe {
        CreateMutexW(None, true, w!("Global\\rmeters_single_instance"))
    };
    match mutex_result {
        Err(e) => {
            log_info(&format!("CreateMutexW failed: {e:?}"));
            return Ok(());
        }
        Ok(_mutex) => {
            // _mutex intentionally kept alive until process exit to hold the mutex.
            // Do not call CloseHandle: the OS releases it when the process terminates,
            // which is the correct single-instance guard lifetime.
            if unsafe { windows::Win32::Foundation::GetLastError() } == ERROR_ALREADY_EXISTS {
                log_info("Another instance is already running, exiting.");
                return Ok(());
            }
        }
    }

    // Register a Ctrl+C handler so the overlay can be closed from the console.
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(ctrl_handler), BOOL::from(true));
        log_info("Console control handler registered");
    }

    config::load_config();
    log_info("Configuration loaded");

    unsafe {
        let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
        log_info("DPI awareness set");
    }

    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        log_info("COM initialized");
    }

    metrics::start_monitoring();
    log_info("Monitoring thread started");

    let hwnd = window::create_overlay();
    log_info(&format!("Overlay window created: HWND = {:?}", hwnd.0));

    metrics::OVERLAY_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

    log_info("Entering message loop...");
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    log_info("Message loop exited");

    unsafe {
        CoUninitialize();
        log_info("COM uninitialized");
    }

    Ok(())
}
