# rmeters

A lightweight, high-performance system monitor overlay for the Windows 11 taskbar, written in Rust. It provides real-time CPU and RAM load indicators positioned neatly on your taskbar, mimicking the look and feel of the classic xMeters utility.

![rmeters App Icon](app_icon.png)

## Features

- **Dual Display Modes**:
  - **Classic Mode**: Shows global CPU and RAM usage sparklines (rolling 60-second history).
  - **Per-Core Mode**: Shows individual CPU core usage vertical bars and a thick RAM progress bar (xMeters-style).
- **Native Taskbar Integration**: Custom bezelless popup overlay positioned next to the system tray, behaving as an owned window of the taskbar (`Shell_TrayWnd`) to natively stay on top without flicker.
- **Zero Overhead**: Minimal CPU usage (~0%) and tiny RAM footprint (<12 MB) thanks to Rust and direct Win32/Direct2D hardware-accelerated rendering.
- **High-DPI Support**: Automatically scales layout, fonts, and graphics for any DPI scaling (100%, 150%, 200%, etc.).
- **Windows Autostart Option**: Can be configured to start automatically with Windows via a system tray context menu toggle.
- **Graceful Shutdown**: Handles standard console signals (Ctrl+C) and tray exit command cleanly.

## System Requirements

- Windows 10 or Windows 11 (64-bit)
- Rust toolchain (MSRV 1.75+)

## Build & Run

To build and run the application in development:

```bash
cargo run
```

To compile a final optimized release executable:

```bash
cargo build --release
```

The compiled binary will be located at `target/release/rmeters.exe`.

## How it Works

- **Metrics Collection**: Uses the `sysinfo` crate in a background thread to poll global/per-core CPU usage and memory stats every second.
- **Rendering**: Directly calls Windows **Direct2D** for graphics and **DirectWrite** for crisp text rendering, drawing onto a layered click-through window.
- **Positioning**: Automatically hooks into the tray coordinates and adjusts its position dynamically to fit seamlessly to the left of the system clock.
- **Layering**: Employs Win32 window ownership where the taskbar (`Shell_TrayWnd`) is the parent/owner, guaranteeing the overlay stays on top.
