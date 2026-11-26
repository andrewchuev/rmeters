use windows::core::HSTRING;
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};

// From WinBase.h; defined locally to avoid pulling in Win32_System_WindowsProgramming.
const DRIVE_FIXED: u32 = 3;

/// Space usage of one local fixed volume.
pub struct DiskSample {
    pub letter: char,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

/// Returns space usage for every local fixed volume (C:, D:, …), in drive-letter order.
/// Removable, network and optical drives are skipped.
pub fn sample() -> Vec<DiskSample> {
    // SAFETY: plain Win32 queries with valid null-terminated paths.
    let logical = unsafe { GetLogicalDrives() };
    (0..26u32)
        .filter(|i| logical & (1 << i) != 0)
        .map(|i| char::from(b'A' + i as u8))
        .filter(|&letter| unsafe { GetDriveTypeW(&root_path(letter)) } == DRIVE_FIXED)
        .map(|letter| {
            let (free_bytes, total_bytes) = free_space(letter);
            DiskSample {
                letter,
                free_bytes,
                total_bytes,
            }
        })
        .collect()
}

fn root_path(letter: char) -> HSTRING {
    HSTRING::from(format!("{letter}:\\"))
}

fn free_space(letter: char) -> (u64, u64) {
    let mut free = 0u64;
    let mut total = 0u64;
    // SAFETY: valid root path and output pointers.
    let ok =
        unsafe { GetDiskFreeSpaceExW(&root_path(letter), Some(&mut free), Some(&mut total), None) };
    if ok.is_ok() {
        (free, total)
    } else {
        (0, 0)
    }
}
