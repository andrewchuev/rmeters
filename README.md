# rmeters

A lightweight, high-performance system monitor for the Windows notification area (system tray), written in Rust. It shows real-time CPU, RAM and disk load as tiny scrolling graphs right next to the clock, in the spirit of the classic xMeters utility.

**[Download Latest Release](https://github.com/andrewchuev/rmeters/releases/latest)**

![rmeters App Icon](app_icon.png)

## Features

- **Tray Icons**: Real-time scrolling sparkline graphs (rolling 60-second history) for CPU (blue) and RAM (green) load in the system tray. Hover an icon for the exact value.
- **Disk Icons**: One orange icon per local fixed volume (C:, D:, …) graphing its active time, the same metric Task Manager shows. The tooltip shows the drive letter, active time and free space. Volumes that appear or disappear are picked up automatically; removable and network drives are not shown.
- **Per-Icon Toggles**: Each icon can be shown or hidden individually; at least one always stays visible so the settings remain reachable.
- **Settings Window**: Click (left or right) any tray icon to open the Settings window — toggle icons, autostart, or exit the app.
- **No Dependencies**: Statically linked against the C runtime — runs on a clean Windows install without installing Visual C++ Redistributable.
- **Installer Included**: Ships with an Inno Setup installer (`rmeters-setup.exe`) that handles Start Menu shortcuts and optional autostart, as well as a portable zip for those who prefer no installation.
- **Zero Overhead**: Minimal CPU usage (~0%) and a tiny RAM footprint.
- **High-DPI Support**: Icons and the Settings window scale for any DPI setting (100%, 150%, 200%, etc.).
- **Explorer Restart Recovery**: Icons are re-added automatically when Explorer restarts.

## Installation

### Installer (recommended)

Download `rmeters-setup.exe` from the [latest release](https://github.com/andrewchuev/rmeters/releases/latest) and run it. The installer will:

- Place `rmeters.exe` in `%ProgramFiles%\RMeters`
- Create a Start Menu shortcut
- Optionally register rmeters to start with Windows
- Register an uninstaller in "Add or remove programs"

### Portable

Download `rmeters-windows-x64-portable.zip`, extract anywhere, and run `rmeters.exe` directly.

## Usage

Click any rmeters tray icon to open the Settings window:

| Setting | Description |
|---|---|
| Show CPU Icon | Show or hide the CPU load graph icon |
| Show RAM Icon | Show or hide the RAM usage graph icon |
| Show Disk Icons | Show or hide the per-volume disk activity icons |
| Start with Windows | Enable / disable autostart via the registry |
| Exit RMeters | Quit the application |

Windows may place new icons in the hidden overflow area; drag them onto the taskbar (or enable them in *Settings → Personalization → Taskbar → Other system tray icons*) to keep them always visible.

Settings are stored in `%APPDATA%\rmeters\config.txt`.

## System Requirements

- Windows 10 or Windows 11 (64-bit)
- No additional runtime libraries required

## Build from Source

```bash
# Development
cargo run

# Optimized release binary
cargo build --release
```

The compiled binary will be at `target/release/rmeters.exe`.

## How it Works

- **Metrics Collection**: A background thread polls CPU and memory stats every second using the `sysinfo` crate, initialized lazily with `System::new()` to avoid full-system scans.
- **Disk Activity**: Fixed volumes are found with `GetLogicalDrives` + `GetDriveTypeW`. Active time is `100 − \LogicalDisk(X:)\% Idle Time` read through the Performance Data Helper (PDH) API; free space comes from `GetDiskFreeSpaceExW`.
- **Icons**: Each tick, every enabled icon is drawn as a GDI sparkline into a fresh `HICON` (color bitmap plus transparency mask) and pushed to the shell with `Shell_NotifyIconW`; the previous handle is destroyed.
- **Hidden Main Window**: A never-shown top-level window owns the icons and receives their click callbacks. It is not a message-only window, so it also receives the `TaskbarCreated` broadcast used to re-add icons after an Explorer restart.
- **Architecture & Reliability**:
  - Modernized Win32 handle management using safe `::default()` constructors instead of legacy raw null pointers.
  - Reliable autostart toggling via Win32 registry APIs with correct error propagation based on direct return values (as registry APIs do not populate the thread's last error code).
