use std::sync::RwLock;
use std::thread;
use std::time::Duration;
use sysinfo::System;
use once_cell::sync::Lazy;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::InvalidateRect;

pub struct MetricsState {
    pub cpu_usage: f32,
    pub ram_usage_pct: f32,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    pub cpu_history: Vec<f32>,
    pub ram_history: Vec<f32>,
    pub cpu_cores_usage: Vec<f32>,
}

impl MetricsState {
    fn new() -> Self {
        Self {
            cpu_usage: 0.0,
            ram_usage_pct: 0.0,
            ram_used_gb: 0.0,
            ram_total_gb: 0.0,
            cpu_history: vec![0.0; 60],
            ram_history: vec![0.0; 60],
            cpu_cores_usage: Vec::new(),
        }
    }

    fn push_cpu(&mut self, val: f32) {
        self.cpu_history.remove(0);
        self.cpu_history.push(val);
    }

    fn push_ram(&mut self, val: f32) {
        self.ram_history.remove(0);
        self.ram_history.push(val);
    }
}

pub static METRICS: Lazy<RwLock<MetricsState>> = Lazy::new(|| RwLock::new(MetricsState::new()));
pub static OVERLAY_HWND: Lazy<RwLock<Option<isize>>> = Lazy::new(|| RwLock::new(None));

pub fn start_monitoring() {
    thread::spawn(|| {
        let mut sys = System::new_all();

        // First refresh to initialize CPU values (they will be 0 on first call)
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

            let ram_used_gb = used_mem / (1024.0 * 1024.0 * 1024.0);
            let ram_total_gb = total_mem / (1024.0 * 1024.0 * 1024.0);

            let cpu_cores_usage: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();

            {
                if let Ok(mut state) = METRICS.write() {
                    state.cpu_usage = cpu;
                    state.ram_usage_pct = ram_pct;
                    state.ram_used_gb = ram_used_gb;
                    state.ram_total_gb = ram_total_gb;
                    state.push_cpu(cpu);
                    state.push_ram(ram_pct);
                    state.cpu_cores_usage = cpu_cores_usage;
                }
            }

            // Invalidate the overlay window to trigger redraw
            let hwnd_raw = { *OVERLAY_HWND.read().unwrap() };
            if let Some(raw) = hwnd_raw {
                let hwnd = HWND(raw as *mut std::ffi::c_void);
                unsafe {
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }

            thread::sleep(Duration::from_secs(1));
        }
    });
}
