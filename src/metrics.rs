use std::collections::VecDeque;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::thread;
use std::time::Duration;
use sysinfo::System;
use windows::Win32::Foundation::HWND;

pub const HISTORY_LEN: usize = 60;

/// Rolling one-sample-per-second history of a percentage metric.
pub struct History(VecDeque<f32>);

impl History {
    pub fn new() -> Self {
        Self(VecDeque::from(vec![0.0f32; HISTORY_LEN]))
    }

    pub fn push(&mut self, val: f32) {
        self.0.pop_front();
        self.0.push_back(val);
    }

    pub fn values(&self) -> &VecDeque<f32> {
        &self.0
    }
}

pub struct MetricsState {
    pub cpu_usage: f32,
    pub ram_usage_pct: f32,
    pub cpu_history: History,
    pub ram_history: History,
    pub disks: Vec<crate::disk::DiskSample>,
}

impl MetricsState {
    fn new() -> Self {
        Self {
            cpu_usage: 0.0,
            ram_usage_pct: 0.0,
            cpu_history: History::new(),
            ram_history: History::new(),
            disks: Vec::new(),
        }
    }
}

/// Raw HWND of the hidden main window that owns the tray icons. 0 means not yet set.
pub static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

/// Starts the background thread that polls system metrics once per second
/// and refreshes the tray icons.
pub fn start_monitoring() {
    thread::spawn(|| {
        let mut sys = System::new();
        let mut state = MetricsState::new();

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

            state.cpu_usage = cpu;
            state.ram_usage_pct = ram_pct;
            state.cpu_history.push(cpu);
            state.ram_history.push(ram_pct);
            state.disks = if crate::config::SHOW_DISKS.load(Ordering::Relaxed) {
                crate::disk::sample()
            } else {
                Vec::new()
            };

            let raw = MAIN_HWND.load(Ordering::Relaxed);
            if raw != 0 {
                // SAFETY: raw is the HWND stored by main.rs; the window lives until
                // the message loop exits, after which the process terminates.
                let hwnd = HWND(raw as *mut std::ffi::c_void);
                crate::tray::update_tray(hwnd, &state);
            }

            thread::sleep(Duration::from_secs(1));
        }
    });
}
