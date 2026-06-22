use std::collections::VecDeque;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{LazyLock, RwLock};
use std::thread;
use std::time::Duration;
use sysinfo::System;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::InvalidateRect;

pub const HISTORY_LEN: usize = 60;

pub struct MetricsState {
    pub cpu_usage: f32,
    pub ram_usage_pct: f32,
    pub cpu_history: VecDeque<f32>,
    pub ram_history: VecDeque<f32>,
    pub cpu_cores_usage: Vec<f32>,
}

impl MetricsState {
    fn new() -> Self {
        Self {
            cpu_usage: 0.0,
            ram_usage_pct: 0.0,
            cpu_history: VecDeque::from(vec![0.0f32; HISTORY_LEN]),
            ram_history: VecDeque::from(vec![0.0f32; HISTORY_LEN]),
            cpu_cores_usage: Vec::new(),
        }
    }

    fn push_cpu(&mut self, val: f32) {
        self.cpu_history.pop_front();
        self.cpu_history.push_back(val);
    }

    fn push_ram(&mut self, val: f32) {
        self.ram_history.pop_front();
        self.ram_history.push_back(val);
    }
}

pub static METRICS: LazyLock<RwLock<MetricsState>> =
    LazyLock::new(|| RwLock::new(MetricsState::new()));

/// Stores the raw HWND value of the overlay window. 0 means not yet set.
pub static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);

/// Starts the background thread that polls system metrics once per second
/// and invalidates the overlay window to trigger a repaint.
pub fn start_monitoring() {
    thread::spawn(|| {
        let mut sys = System::new_all();

        // First refresh to prime CPU values (they return 0 on the very first call)
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        thread::sleep(Duration::from_millis(500));

        loop {
            sys.refresh_cpu_usage();
            sys.refresh_memory();

            let cpu = sys.global_cpu_info().cpu_usage();

            let total_mem = sys.total_memory() as f32;
            let used_mem = sys.used_memory() as f32;
            let ram_pct = if total_mem > 0.0 {
                (used_mem / total_mem) * 100.0
            } else {
                0.0
            };

            let cpu_cores_usage: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();

            {
                let mut state = match METRICS.write() {
                    Ok(g) => g,
                    // Recover from a poisoned lock so the monitoring thread stays alive.
                    Err(p) => p.into_inner(),
                };
                state.cpu_usage = cpu;
                state.ram_usage_pct = ram_pct;
                state.push_cpu(cpu);
                state.push_ram(ram_pct);
                state.cpu_cores_usage = cpu_cores_usage;
            }

            let raw = OVERLAY_HWND.load(Ordering::Relaxed);
            if raw != 0 {
                // SAFETY: raw is a valid HWND stored by the main thread in main.rs.
                // It remains valid for the lifetime of the overlay window, which
                // outlives this background thread (WM_DESTROY posts WM_QUIT before
                // the process exits).
                let hwnd = HWND(raw as *mut std::ffi::c_void);
                unsafe {
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }

            thread::sleep(Duration::from_secs(1));
        }
    });
}
