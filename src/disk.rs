use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PdhRemoveCounter, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA,
    PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE,
};

// From WinBase.h; defined locally to avoid pulling in Win32_System_WindowsProgramming.
const DRIVE_FIXED: u32 = 3;

/// One sample for a local fixed volume.
pub struct DiskSample {
    pub letter: char,
    /// Share of time the volume was busy over the last interval, 0–100.
    pub active_pct: f32,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

struct Volume {
    letter: char,
    counter: isize,
}

/// Samples "active time" of local fixed volumes via the LogicalDisk "% Idle Time"
/// performance counter (the same source Task Manager uses) and tracks volumes
/// being added or removed.
pub struct DiskMonitor {
    query: isize,
    drive_mask: u32,
    volumes: Vec<Volume>,
}

impl DiskMonitor {
    pub fn new() -> Self {
        let mut query = 0isize;
        // SAFETY: PdhOpenQueryW writes a query handle into `query`; null data source = live data.
        let status = unsafe { PdhOpenQueryW(None, 0, &mut query) };
        if status != 0 {
            crate::log_info(&format!("PdhOpenQueryW failed: {status:#x}"));
            query = 0;
        }
        Self { query, drive_mask: 0, volumes: Vec::new() }
    }

    /// Bitmask of drive letters (bit 0 = A:) that are local fixed volumes.
    fn fixed_drive_mask() -> u32 {
        // SAFETY: plain Win32 queries with valid null-terminated paths.
        unsafe {
            let logical = GetLogicalDrives();
            (0..26u32)
                .filter(|i| logical & (1 << i) != 0)
                .filter(|&i| GetDriveTypeW(&root_path(letter(i))) == DRIVE_FIXED)
                .fold(0, |mask, i| mask | (1 << i))
        }
    }

    /// Rebuilds the counter set when the set of fixed volumes changes.
    fn sync_volumes(&mut self) {
        let mask = Self::fixed_drive_mask();
        if mask == self.drive_mask {
            return;
        }
        self.drive_mask = mask;

        // SAFETY: counters belong to self.query and are removed exactly once.
        unsafe {
            for v in self.volumes.drain(..) {
                if v.counter != 0 {
                    let _ = PdhRemoveCounter(v.counter);
                }
            }
        }

        for i in (0..26u32).filter(|i| mask & (1 << i) != 0) {
            let letter = letter(i);
            let mut counter = 0isize;
            if self.query != 0 {
                let path = HSTRING::from(format!("\\LogicalDisk({letter}:)\\% Idle Time"));
                // SAFETY: valid query handle and null-terminated counter path.
                let status = unsafe { PdhAddEnglishCounterW(self.query, &path, 0, &mut counter) };
                if status != 0 {
                    crate::log_info(&format!("PdhAddEnglishCounterW({letter}:) failed: {status:#x}"));
                    counter = 0;
                }
            }
            self.volumes.push(Volume { letter, counter });
        }
    }

    /// Takes one sample of every fixed volume. Call about once per second;
    /// rate counters report 0 on the first call after a volume appears.
    pub fn sample(&mut self) -> Vec<DiskSample> {
        self.sync_volumes();

        if self.query != 0 {
            // SAFETY: valid query handle.
            unsafe {
                let _ = PdhCollectQueryData(self.query);
            }
        }

        self.volumes
            .iter()
            .map(|v| {
                let (free_bytes, total_bytes) = free_space(v.letter);
                DiskSample {
                    letter: v.letter,
                    active_pct: active_pct(v.counter),
                    free_bytes,
                    total_bytes,
                }
            })
            .collect()
    }
}

impl Drop for DiskMonitor {
    fn drop(&mut self) {
        if self.query != 0 {
            // SAFETY: closing the query also frees its counters.
            unsafe {
                let _ = PdhCloseQuery(self.query);
            }
        }
    }
}

fn letter(index: u32) -> char {
    char::from(b'A' + index as u8)
}

fn root_path(letter: char) -> HSTRING {
    HSTRING::from(format!("{letter}:\\"))
}

fn active_pct(counter: isize) -> f32 {
    if counter == 0 {
        return 0.0;
    }
    let mut value = PDH_FMT_COUNTERVALUE::default();
    // SAFETY: counter is a valid handle from PdhAddEnglishCounterW.
    let status = unsafe { PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, None, &mut value) };
    if status != 0
        || (value.CStatus != PDH_CSTATUS_VALID_DATA && value.CStatus != PDH_CSTATUS_NEW_DATA)
    {
        return 0.0;
    }
    // SAFETY: PDH_FMT_DOUBLE was requested, so the union holds doubleValue.
    let idle = unsafe { value.Anonymous.doubleValue };
    (100.0 - idle).clamp(0.0, 100.0) as f32
}

fn free_space(letter: char) -> (u64, u64) {
    let mut free = 0u64;
    let mut total = 0u64;
    // SAFETY: valid root path and output pointers.
    let ok = unsafe {
        GetDiskFreeSpaceExW(&root_path(letter), Some(&mut free), Some(&mut total), None)
    };
    if ok.is_ok() { (free, total) } else { (0, 0) }
}
