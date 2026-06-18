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

    // Sync autostart status with Windows Registry
    AUTOSTART_ENABLED.store(is_autostart_enabled(), Ordering::Relaxed);
}

pub fn save_config() {
    let path = config_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let per_core = SHOW_PER_CORE.load(Ordering::Relaxed);
    let overlay_x = OVERLAY_X.load(Ordering::Relaxed);
    let overlay_y = OVERLAY_Y.load(Ordering::Relaxed);
    let _ = fs::write(path, format!(
        "show_per_core: {}\noverlay_x: {}\noverlay_y: {}\n",
        per_core, overlay_x, overlay_y
    ));
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
