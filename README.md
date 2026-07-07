# rmeters

A lightweight, high-performance system monitor overlay for the Windows 11 taskbar, written in Rust. It provides real-time CPU and RAM load indicators positioned neatly on your taskbar, mimicking the look and feel of the classic xMeters utility.

**[Download Latest Release](https://github.com/andrewchuev/rmeters/releases/latest)**

![rmeters App Icon](app_icon.png)

## Features

- **Multiple Display Modes**:
  - **Taskbar Overlay**: Bezelless overlay positioned next to the system tray.
  - **System Tray Icons**: Real-time scrolling sparkline graphs showing CPU and RAM load in the system tray.
  - **Combined Mode**: Both modes can be used simultaneously or individually.
- **Detachable Overlay Window**: The taskbar overlay can be unlocked from the taskbar row and dragged freely to any part of the desktop. The custom coordinate position is saved and restored across sessions.
- **Dual Overlay Graph Modes**:
  - **Classic Mode**: Shows global CPU and RAM usage as sparklines (rolling 60-second history).
  - **Per-Core Mode**: Shows individual CPU core usage as vertical bars alongside a RAM sparkline history graph.
- **Settings Window**: Right-click on the taskbar overlay or either tray icon to open the Settings window — toggle display modes, autostart, overlay lock, or exit the app.
- **No Dependencies**: Statically linked against the C runtime — runs on a clean Windows install without installing Visual C++ Redistributable.
- **Installer Included**: Ships with an Inno Setup installer (`rmeters-setup.exe`) that handles Start Menu shortcuts and optional autostart, as well as a portable zip for those who prefer no installation.
- **Zero Overhead**: Minimal CPU usage (~0%) and tiny RAM footprint (<12 MB) thanks to Rust and Direct2D hardware-accelerated rendering.
- **High-DPI Support**: Automatically scales layout, fonts, and graphics for any DPI scaling (100%, 150%, 200%, etc.).
- **Fullscreen Auto-Hide**: Automatically hides the overlay when a fullscreen application is running and restores it on exit.
- **Drag to Reposition**: In taskbar-locked mode, the panel can be dragged horizontally along the taskbar; its position is saved across sessions.

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

Right-click on the overlay panel to open the Settings window:

| Setting | Description |
|---|---|
| Show CPU per Core | Toggle between classic sparkline and per-core bar modes |
| Show Taskbar Overlay | Enable or disable the taskbar overlay window |
| Lock Overlay to Taskbar | When unchecked, the overlay window can be dragged and placed anywhere on the screen |
| Show Tray Icons | Enable or disable real-time CPU & RAM load scrolling graph icons in the system tray |
| Start with Windows | Enable / disable autostart via the registry |
| Exit RMeters | Quit the application |

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

- **Metrics Collection**: Uses the `sysinfo` crate in a background thread to poll global/per-core CPU usage and memory stats every second. To ensure near-instantaneous startup and zero performance overhead, the metrics engine is initialized lazily using `System::new()`, completely avoiding full-system scans (such as scanning active processes, disks, or network interfaces).
- **Rendering**: Calls Windows **Direct2D** and **DirectWrite** for hardware-accelerated graphics and crisp text, drawn onto a layered window. A snapshot-based architecture (`MetricsSnapshot`) is used to pass metrics to the drawing pipeline without holding locks, avoiding thread contention.
- **Positioning**: Hooks into tray area coordinates and repositions dynamically to fit seamlessly to the left of the system clock. Saved X position persists across sessions.
- **Layering**: The taskbar (`Shell_TrayWnd`) is the window owner, guaranteeing the overlay stays on top natively. The 1-second timer reasserts TOPMOST to recover from Win+D and minimize/restore animations.
- **Mouse Input**: The overlay receives right-click events directly while `WS_EX_NOACTIVATE` prevents it from stealing focus from other windows.
- **Architecture & Reliability**:
  - Modernized Win32 handle management using safe `::default()` constructors instead of legacy raw null pointers.
  - Reliable autostart toggling via Win32 registry APIs with correct error propagation based on direct return values (as registry APIs do not populate the thread's last error code).
