use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::fs;
use std::path::PathBuf;

pub static SHOW_PER_CORE: AtomicBool = AtomicBool::new(false);
pub static AUTOSTART_ENABLED: AtomicBool = AtomicBool::new(false);
pub static OVERLAY_X: AtomicI32 = AtomicI32::new(-1);
pub static OVERLAY_Y: AtomicI32 = AtomicI32::new(-1);

fn config_path() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("rmeters").join("config.txt")
}

/// Returns the directory used for all rmeters data files (config, log).
pub fn data_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("rmeters")
}

/// Loads configuration from %APPDATA%\rmeters\config.txt.
pub fn load_config() {
    if let Ok(content) = fs::read_to_string(config_path()) {
        let parse = |key_name: &str| -> Option<&str> {
            content.lines().find_map(|line| {
                let mut parts = line.splitn(2, ':');
                let key = parts.next()?.trim();
                let val = parts.next()?.trim();
                if key == key_name { Some(val) } else { None }
            })
        };

        let show = parse("show_per_core").map(|v| v == "true").unwrap_or(false);
        SHOW_PER_CORE.store(show, Ordering::Relaxed);

        let x = parse("overlay_x").and_then(|v| v.parse::<i32>().ok()).unwrap_or(-1);
        OVERLAY_X.store(x, Ordering::Relaxed);

        let y = parse("overlay_y").and_then(|v| v.parse::<i32>().ok()).unwrap_or(-1);
        OVERLAY_Y.store(y, Ordering::Relaxed);
    }

    // Sync autostart status with the Windows Registry.
    AUTOSTART_ENABLED.store(is_autostart_enabled(), Ordering::Relaxed);
}

/// Persists current configuration to %APPDATA%\rmeters\config.txt.
pub fn save_config() {
    let path = config_path();
    if let Some(dir) = path.parent() {
        if let Err(e) = fs::create_dir_all(dir) {
            crate::log_info(&format!("save_config: failed to create config directory: {e}"));
            return;
        }
    }
    let per_core = SHOW_PER_CORE.load(Ordering::Relaxed);
    let overlay_x = OVERLAY_X.load(Ordering::Relaxed);
    let overlay_y = OVERLAY_Y.load(Ordering::Relaxed);
    if let Err(e) = fs::write(
        path,
        format!("show_per_core: {per_core}\noverlay_x: {overlay_x}\noverlay_y: {overlay_y}\n"),
    ) {
        crate::log_info(&format!("save_config: write failed: {e}"));
    }
}

/// Returns true if the rmeters autostart registry value is present.
pub fn is_autostart_enabled() -> bool {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER,
        KEY_QUERY_VALUE, HKEY,
    };
    use windows::core::w;

    unsafe {
        let mut hkey = HKEY::default();
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let name = w!("rmeters");

        let status = RegOpenKeyExW(HKEY_CURRENT_USER, subkey, 0, KEY_QUERY_VALUE, &mut hkey);
        if status.is_err() {
            return false;
        }

        let mut cb_data = 0;
        let status_query = RegQueryValueExW(hkey, name, None, None, None, Some(&mut cb_data));
        let _ = RegCloseKey(hkey);
        status_query.is_ok()
    }
}

/// Adds or removes the rmeters autostart registry value.
/// Returns `Err` if the registry key cannot be opened or the value cannot be written.
pub fn set_autostart(enable: bool) -> Result<(), std::io::Error> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegSetValueExW, RegDeleteValueW, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ, HKEY,
    };
    use windows::core::w;

    let path = std::env::current_exe()?;
    let path_str = path.to_string_lossy();
    let cmd = format!("\"{}\"", path_str);
    let cmd_wide: Vec<u16> = std::ffi::OsStr::new(&cmd).encode_wide().chain(Some(0)).collect();

    unsafe {
        let mut hkey = HKEY::default();
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let name = w!("rmeters");

        let status = RegOpenKeyExW(HKEY_CURRENT_USER, subkey, 0, KEY_SET_VALUE, &mut hkey);
        if status.is_err() {
            return Err(std::io::Error::last_os_error());
        }

        let write_result = if enable {
            RegSetValueExW(
                hkey,
                name,
                0,
                REG_SZ,
                Some(
                    // SAFETY: cmd_wide is a valid Vec<u16> with a null terminator.
                    // Reinterpreting its contents as a &[u8] byte slice with
                    // byte length = element_count * 2 is correct for UTF-16 LE data.
                    std::slice::from_raw_parts(cmd_wide.as_ptr() as *const u8, cmd_wide.len() * 2),
                ),
            )
        } else {
            RegDeleteValueW(hkey, name)
        };

        let _ = RegCloseKey(hkey);

        write_result.ok().map_err(|_| std::io::Error::last_os_error())
    }
}
