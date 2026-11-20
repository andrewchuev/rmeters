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

pub struct DiskState {
    pub letter: char,
    pub active_pct: f32,
    pub free_bytes: u64,
    pub total_bytes: u64,
    pub history: History,
}

pub struct MetricsState {
    pub cpu_usage: f32,
    pub ram_usage_pct: f32,
    pub cpu_history: History,
    pub ram_history: History,
    pub disks: Vec<DiskState>,
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

    /// Applies fresh disk samples, keeping the history of volumes that are still present.
    fn update_disks(&mut self, samples: Vec<crate::disk::DiskSample>) {
        let mut old = std::mem::take(&mut self.disks);
        self.disks = samples
            .into_iter()
            .map(|s| {
                let mut history = old
                    .iter()
                    .position(|d| d.letter == s.letter)
                    .map(|i| old.swap_remove(i).history)
                    .unwrap_or_else(History::new);
                history.push(s.active_pct);
                DiskState {
                    letter: s.letter,
                    active_pct: s.active_pct,
                    free_bytes: s.free_bytes,
                    total_bytes: s.total_bytes,
                    history,
                }
            })
            .collect();
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
        let mut disks = crate::disk::DiskMonitor::new();

        // First refresh to prime CPU and disk rate values (they return 0 on the very first call)
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let _ = disks.sample();
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
            if crate::config::SHOW_DISKS.load(Ordering::Relaxed) {
                state.update_disks(disks.sample());
            } else {
                state.disks.clear();
            }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::DiskSample;

    fn sample(letter: char, active_pct: f32) -> DiskSample {
        DiskSample { letter, active_pct, free_bytes: 1, total_bytes: 2 }
    }

    #[test]
    fn disk_history_survives_and_resets_per_volume() {
        let mut state = MetricsState::new();
        state.update_disks(vec![sample('C', 10.0)]);
        state.update_disks(vec![sample('D', 5.0), sample('C', 20.0)]);

        let c = state.disks.iter().find(|d| d.letter == 'C').unwrap();
        let hist: Vec<f32> = c.history.values().iter().rev().take(2).copied().collect();
        assert_eq!(hist, vec![20.0, 10.0]);
        assert_eq!(c.history.values().len(), HISTORY_LEN);

        let d = state.disks.iter().find(|d| d.letter == 'D').unwrap();
        assert_eq!(d.history.values().iter().filter(|&&v| v != 0.0).count(), 1);

        state.update_disks(vec![sample('D', 1.0)]);
        assert_eq!(state.disks.len(), 1);
    }
}
