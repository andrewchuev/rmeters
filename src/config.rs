use std::sync::atomic::{AtomicBool, Ordering};
use std::fs;
use std::path::PathBuf;

pub static AUTOSTART_ENABLED: AtomicBool = AtomicBool::new(false);
pub static SHOW_CPU: AtomicBool = AtomicBool::new(true);
pub static SHOW_RAM: AtomicBool = AtomicBool::new(true);

fn config_path() -> PathBuf {
    data_dir().join("config.txt")
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
        let flag = |key_name: &str, default: bool| -> bool {
            parse(key_name).map(|v| v == "true").unwrap_or(default)
        };

        SHOW_CPU.store(flag("show_cpu", true), Ordering::Relaxed);
        SHOW_RAM.store(flag("show_ram", true), Ordering::Relaxed);
    }

    // Tray icons are the only UI, so never start with all of them hidden.
    if !any_icon_enabled() {
        SHOW_CPU.store(true, Ordering::Relaxed);
    }

    // Sync autostart status with the Windows Registry.
    AUTOSTART_ENABLED.store(is_autostart_enabled(), Ordering::Relaxed);
}

/// Returns true when at least one tray icon group is enabled.
pub fn any_icon_enabled() -> bool {
    SHOW_CPU.load(Ordering::Relaxed) || SHOW_RAM.load(Ordering::Relaxed)
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
    let show_cpu = SHOW_CPU.load(Ordering::Relaxed);
    let show_ram = SHOW_RAM.load(Ordering::Relaxed);

    if let Err(e) = fs::write(
        path,
        format!(
            "show_cpu: {show_cpu}\n\
             show_ram: {show_ram}\n"
        ),
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
            return Err(std::io::Error::from_raw_os_error(status.0 as i32));
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

        if write_result.is_err() {
            Err(std::io::Error::from_raw_os_error(write_result.0 as i32))
        } else {
            Ok(())
        }
    }
}
