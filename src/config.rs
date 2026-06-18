use std::sync::atomic::{AtomicBool, Ordering};
use std::fs;

pub static SHOW_PER_CORE: AtomicBool = AtomicBool::new(false);
pub static AUTOSTART_ENABLED: AtomicBool = AtomicBool::new(false);

const CONFIG_PATH: &str = "D:\\Projects\\internal\\rmeters\\config.txt";

pub fn load_config() {
    if let Ok(content) = fs::read_to_string(CONFIG_PATH) {
        let trimmed = content.trim();
        if trimmed == "show_per_core: true" {
            SHOW_PER_CORE.store(true, Ordering::Relaxed);
        } else {
            SHOW_PER_CORE.store(false, Ordering::Relaxed);
        }
    }
    
    // Sync autostart status with Windows Registry
    AUTOSTART_ENABLED.store(is_autostart_enabled(), Ordering::Relaxed);
}

pub fn save_config() {
    let val = SHOW_PER_CORE.load(Ordering::Relaxed);
    let content = format!("show_per_core: {}", val);
    let _ = fs::write(CONFIG_PATH, content);
}

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

        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey,
            0,
            KEY_QUERY_VALUE,
            &mut hkey,
        );

        if status.is_ok() {
            let mut cb_data = 0;
            // First call to get the buffer size (checking if value exists)
            let status_query = RegQueryValueExW(
                hkey,
                name,
                None,
                None,
                None,
                Some(&mut cb_data),
            );
            let _ = RegCloseKey(hkey);
            status_query.is_ok()
        } else {
            false
        }
    }
}

pub fn set_autostart(enable: bool) -> Result<(), std::io::Error> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegSetValueExW, RegDeleteValueW, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ, HKEY,
    };
    use windows::core::w;

    let path = std::env::current_exe()?;
    let path_str = path.to_string_lossy();
    
    // Format command line, e.g. "C:\path\to\rmeters.exe"
    let cmd = format!("\"{}\"", path_str);
    let cmd_wide: Vec<u16> = std::ffi::OsStr::new(&cmd).encode_wide().chain(Some(0)).collect();

    unsafe {
        let mut hkey = HKEY::default();
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let name = w!("rmeters");

        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey,
            0,
            KEY_SET_VALUE,
            &mut hkey,
        );

        if status.is_ok() {
            if enable {
                let _ = RegSetValueExW(
                    hkey,
                    name,
                    0,
                    REG_SZ,
                    Some(std::slice::from_raw_parts(
                        cmd_wide.as_ptr() as *const u8,
                        cmd_wide.len() * 2,
                    )),
                );
            } else {
                let _ = RegDeleteValueW(hkey, name);
            }
            let _ = RegCloseKey(hkey);
        }
    }

    Ok(())
}
