use std::sync::{LazyLock, Mutex};
use windows::core::{w, Interface};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_PRESENT_OPTIONS_NONE,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F,
    D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_MEASURING_MODE_NATURAL,
};

use crate::metrics::{METRICS, HISTORY_LEN};

/// A snapshot of system metrics for rendering.
struct MetricsSnapshot {
    cpu: f32,
    ram: f32,
    cpu_arr: [f32; HISTORY_LEN],
    ram_arr: [f32; HISTORY_LEN],
    cpu_cores: Vec<f32>,
}

/// D2DERR_RECREATE_TARGET — returned by EndDraw when the D2D device is lost.
const D2DERR_RECREATE_TARGET: i32 = 0x8899_000Cu32 as i32;

static D2D_FACTORY: LazyLock<ID2D1Factory> = LazyLock::new(|| {
    // SAFETY: D2D1CreateFactory is called once on the main thread during COM
    // apartment initialization. D2D1_FACTORY_TYPE_SINGLE_THREADED is correct here
    // because the factory and render targets are only used from the main thread.
    unsafe {
        D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
            .expect("Failed to create D2D factory")
    }
});

static DWRITE_FACTORY: LazyLock<IDWriteFactory> = LazyLock::new(|| {
    // SAFETY: DWriteCreateFactory is thread-safe; DWRITE_FACTORY_TYPE_SHARED allows
    // the factory to be shared across threads, which is fine for our single-thread use.
    unsafe {
        DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
            .expect("Failed to create DWrite factory")
    }
});

/// Bundles the D2D render target with its associated brushes and text format.
/// All resources are tied to the render target and must be recreated together
/// on device-loss (D2DERR_RECREATE_TARGET).
struct RenderResources {
    rt: ID2D1HwndRenderTarget,
    panel_brush: ID2D1SolidColorBrush,
    track_brush: ID2D1SolidColorBrush,
    white_brush: ID2D1SolidColorBrush,
    cpu_brush: ID2D1SolidColorBrush,
    ram_brush: ID2D1SolidColorBrush,
    grid_brush: ID2D1SolidColorBrush,
    text_format: IDWriteTextFormat,
}

impl RenderResources {
    unsafe fn create(hwnd: HWND, width: u32, height: u32) -> windows::core::Result<Self> {
        let rt_properties = D2D1_RENDER_TARGET_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            ..Default::default()
        };
        let hwnd_rt_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: D2D_SIZE_U { width, height },
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        let rt = D2D_FACTORY.CreateHwndRenderTarget(&rt_properties, &hwnd_rt_properties)?;

        let mk = |r, g, b, a| D2D1_COLOR_F { r, g, b, a };
        let panel_brush = rt.CreateSolidColorBrush(&mk(0.12, 0.12, 0.12, 1.0), None)?;
        let track_brush = rt.CreateSolidColorBrush(&mk(0.18, 0.18, 0.18, 1.0), None)?;
        let white_brush = rt.CreateSolidColorBrush(&mk(1.0, 1.0, 1.0, 1.0), None)?;
        let cpu_brush   = rt.CreateSolidColorBrush(&mk(0.0, 0.6, 1.0, 1.0), None)?;
        let ram_brush   = rt.CreateSolidColorBrush(&mk(0.1, 0.8, 0.2, 1.0), None)?;
        let grid_brush  = rt.CreateSolidColorBrush(&mk(0.25, 0.25, 0.25, 1.0), None)?;

        let text_format = DWRITE_FACTORY.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            11.0,
            w!("en-US"),
        )?;

        Ok(Self { rt, panel_brush, track_brush, white_brush, cpu_brush, ram_brush, grid_brush, text_format })
    }
}

static RENDER_TARGET: LazyLock<Mutex<Option<RenderResources>>> =
    LazyLock::new(|| Mutex::new(None));

pub struct Renderer;

impl Renderer {
    pub unsafe fn paint(hwnd: HWND) {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        if hdc.0.is_null() {
            return;
        }

        if let Err(e) = Self::draw(hwnd) {
            crate::log_info(&format!("paint: draw error {:?}", e));
        }

        let _ = EndPaint(hwnd, &ps);
    }

    unsafe fn draw(hwnd: HWND) -> windows::core::Result<()> {
        let mut client_rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut client_rect);
        let width = (client_rect.right - client_rect.left) as u32;
        let height = (client_rect.bottom - client_rect.top) as u32;

        if width == 0 || height == 0 {
            return Ok(());
        }

        // Snapshot metrics without holding the lock during drawing.
        let metrics = {
            let mut cpu_arr = [0.0f32; HISTORY_LEN];
            let mut ram_arr = [0.0f32; HISTORY_LEN];
            let state = METRICS.read().unwrap_or_else(|p| p.into_inner());
            for (i, &v) in state.cpu_history.iter().enumerate() {
                cpu_arr[i] = v;
            }
            for (i, &v) in state.ram_history.iter().enumerate() {
                ram_arr[i] = v;
            }
            MetricsSnapshot {
                cpu: state.cpu_usage,
                ram: state.ram_usage_pct,
                cpu_arr,
                ram_arr,
                cpu_cores: state.cpu_cores_usage.clone(),
            }
        };

        let mut guard = RENDER_TARGET.lock().unwrap_or_else(|p| p.into_inner());

        if guard.is_none() {
            *guard = Some(RenderResources::create(hwnd, width, height)?);
        }

        // Run the entire draw sequence in a scoped block so that the immutable
        // borrow on `guard` (via `res`) ends before we need to mutably clear it
        // on device-loss below.
        let draw_result: windows::core::Result<()> = {
            let res = guard.as_ref().unwrap();

            let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd);
            res.rt.SetDpi(dpi as f32, dpi as f32);

            let current_size = res.rt.GetPixelSize();
            if current_size.width != width || current_size.height != height {
                res.rt.Resize(&D2D_SIZE_U { width, height })?;
            }

            let rt: ID2D1RenderTarget = res.rt.cast()?;
            rt.BeginDraw();

            let clear_color = D2D1_COLOR_F { r: 0.12, g: 0.12, b: 0.12, a: 1.0 };
            rt.Clear(Some(&clear_color));

            let rt_size = rt.GetSize();
            let lw = rt_size.width;
            let lh = rt_size.height;

            let panel_rect = D2D_RECT_F { left: 0.0, top: 0.0, right: lw, bottom: lh };
            rt.FillRectangle(&panel_rect, &res.panel_brush);

            let show_per_core =
                crate::config::SHOW_PER_CORE.load(std::sync::atomic::Ordering::Relaxed);

            if show_per_core {
                Self::draw_per_core(&rt, res, &metrics, lw, lh);
            } else {
                Self::draw_sparklines(&rt, res, &metrics, lh);
            }

            rt.EndDraw(None, None)
        };

        match draw_result {
            Ok(()) => {}
            Err(ref e) if e.code().0 == D2DERR_RECREATE_TARGET => {
                // Drop all resources; they will be recreated on the next paint.
                *guard = None;
            }
            Err(e) => return Err(e),
        }

        Ok(())
    }

    unsafe fn draw_per_core(
        rt: &ID2D1RenderTarget,
        res: &RenderResources,
        metrics: &MetricsSnapshot,
        lw: f32,
        lh: f32,
    ) {
        // CPU header label
        let cpu_text = format!("CPU {:.0}%", metrics.cpu);
        let cpu_wide: Vec<u16> = cpu_text.encode_utf16().collect();
        rt.DrawText(
            &cpu_wide,
            &res.text_format,
            &D2D_RECT_F { left: 2.0, top: 2.0, right: 65.0, bottom: 16.0 },
            &res.white_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );

        let graph_left   = 2.0f32;
        let graph_right  = 62.0f32;
        let graph_top    = 18.0f32;
        let graph_bottom = lh - 4.0;
        let graph_height = graph_bottom - graph_top;
        let graph_width  = graph_right - graph_left;

        let num_cores = metrics.cpu_cores.len();
        if num_cores > 0 {
            let gap = if num_cores > 16 { 0.5f32 } else { 1.0 };
            let total_gaps = (num_cores - 1) as f32 * gap;
            let bar_width = (graph_width - total_gaps) / num_cores as f32;

            for (i, &usage) in metrics.cpu_cores.iter().enumerate() {
                let x1 = graph_left + i as f32 * (bar_width + gap);
                let x2 = x1 + bar_width;
                rt.FillRectangle(
                    &D2D_RECT_F { left: x1, top: graph_top, right: x2, bottom: graph_bottom },
                    &res.track_brush,
                );
                let bar_h = (usage / 100.0) * graph_height;
                rt.FillRectangle(
                    &D2D_RECT_F { left: x1, top: graph_bottom - bar_h, right: x2, bottom: graph_bottom },
                    &res.cpu_brush,
                );
            }
        } else {
            rt.DrawRectangle(
                &D2D_RECT_F { left: graph_left, top: graph_top, right: graph_right, bottom: graph_bottom },
                &res.grid_brush, 0.5, None,
            );
        }

        // RAM header label
        let ram_text = format!("RAM {:.0}%", metrics.ram);
        let ram_wide: Vec<u16> = ram_text.encode_utf16().collect();
        rt.DrawText(
            &ram_wide,
            &res.text_format,
            &D2D_RECT_F { left: 72.0, top: 2.0, right: 135.0, bottom: 16.0 },
            &res.white_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );

        let ram_left  = 72.0f32;
        let ram_right = 132.0f32;

        rt.DrawRectangle(
            &D2D_RECT_F { left: ram_left, top: graph_top, right: ram_right, bottom: graph_bottom },
            &res.grid_brush, 0.5, None,
        );

        let ram_width_step = (ram_right - ram_left) / (HISTORY_LEN - 1) as f32;
        for (i, pair) in metrics.ram_arr.windows(2).enumerate() {
            let x1 = ram_left + i as f32 * ram_width_step;
            let y1 = graph_bottom - (pair[0] / 100.0) * graph_height;
            let x2 = ram_left + (i + 1) as f32 * ram_width_step;
            let y2 = graph_bottom - (pair[1] / 100.0) * graph_height;
            rt.DrawLine(
                D2D_POINT_2F { x: x1, y: y1 },
                D2D_POINT_2F { x: x2, y: y2 },
                &res.ram_brush,
                1.0,
                None,
            );
        }

        let _ = lw; // unused in per-core layout
    }

    unsafe fn draw_sparklines(
        rt: &ID2D1RenderTarget,
        res: &RenderResources,
        metrics: &MetricsSnapshot,
        lh: f32,
    ) {
        // CPU header label
        let cpu_text = format!("CPU {:.0}%", metrics.cpu);
        let cpu_wide: Vec<u16> = cpu_text.encode_utf16().collect();
        rt.DrawText(
            &cpu_wide,
            &res.text_format,
            &D2D_RECT_F { left: 2.0, top: 2.0, right: 65.0, bottom: 16.0 },
            &res.white_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );

        let graph_left   = 2.0f32;
        let graph_right  = 62.0f32;
        let graph_top    = 18.0f32;
        let graph_bottom = lh - 4.0;
        let graph_height = graph_bottom - graph_top;

        rt.DrawRectangle(
            &D2D_RECT_F { left: graph_left, top: graph_top, right: graph_right, bottom: graph_bottom },
            &res.grid_brush, 0.5, None,
        );

        let width_step = (graph_right - graph_left) / (HISTORY_LEN - 1) as f32;
        for (i, pair) in metrics.cpu_arr.windows(2).enumerate() {
            let x1 = graph_left + i as f32 * width_step;
            let y1 = graph_bottom - (pair[0] / 100.0) * graph_height;
            let x2 = graph_left + (i + 1) as f32 * width_step;
            let y2 = graph_bottom - (pair[1] / 100.0) * graph_height;
            rt.DrawLine(
                D2D_POINT_2F { x: x1, y: y1 },
                D2D_POINT_2F { x: x2, y: y2 },
                &res.cpu_brush,
                1.0,
                None,
            );
        }

        // RAM header label
        let ram_text = format!("RAM {:.0}%", metrics.ram);
        let ram_wide: Vec<u16> = ram_text.encode_utf16().collect();
        rt.DrawText(
            &ram_wide,
            &res.text_format,
            &D2D_RECT_F { left: 72.0, top: 2.0, right: 135.0, bottom: 16.0 },
            &res.white_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );

        let ram_left  = 72.0f32;
        let ram_right = 132.0f32;

        rt.DrawRectangle(
            &D2D_RECT_F { left: ram_left, top: graph_top, right: ram_right, bottom: graph_bottom },
            &res.grid_brush, 0.5, None,
        );

        let ram_width_step = (ram_right - ram_left) / (HISTORY_LEN - 1) as f32;
        for (i, pair) in metrics.ram_arr.windows(2).enumerate() {
            let x1 = ram_left + i as f32 * ram_width_step;
            let y1 = graph_bottom - (pair[0] / 100.0) * graph_height;
            let x2 = ram_left + (i + 1) as f32 * ram_width_step;
            let y2 = graph_bottom - (pair[1] / 100.0) * graph_height;
            rt.DrawLine(
                D2D_POINT_2F { x: x1, y: y1 },
                D2D_POINT_2F { x: x2, y: y2 },
                &res.ram_brush,
                1.0,
                None,
            );
        }
    }
}
