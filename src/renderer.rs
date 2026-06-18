use std::sync::Mutex;
use once_cell::sync::Lazy;
use windows::core::{w, Interface};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_PRESENT_OPTIONS_NONE,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_POINT_2F,
    D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_MEASURING_MODE_NATURAL,
};

use crate::metrics::METRICS;

static D2D_FACTORY: Lazy<ID2D1Factory> = Lazy::new(|| {
    unsafe {
        D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
            .expect("Failed to create D2D factory")
    }
});

static DWRITE_FACTORY: Lazy<IDWriteFactory> = Lazy::new(|| {
    unsafe {
        DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
            .expect("Failed to create DWrite factory")
    }
});

static RENDER_TARGET: Lazy<Mutex<Option<ID2D1HwndRenderTarget>>> = Lazy::new(|| Mutex::new(None));

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

        let mut rt_guard = RENDER_TARGET.lock().unwrap();
        if rt_guard.is_none() {
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
            *rt_guard = Some(rt);
        }

        let rt_hwnd = rt_guard.as_ref().unwrap();

        // Get window DPI and set it on the render target
        let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd);
        rt_hwnd.SetDpi(dpi as f32, dpi as f32);

        // Check if physical size changed
        let current_pixel_size = rt_hwnd.GetPixelSize();
        if current_pixel_size.width != width || current_pixel_size.height != height {
            rt_hwnd.Resize(&D2D_SIZE_U { width, height })?;
        }

        // Cast to base ID2D1RenderTarget interface for drawing methods
        let rt: ID2D1RenderTarget = rt_hwnd.cast()?;

        rt.BeginDraw();

        // Clear directly to the solid dark gray panel background color to eliminate flickering
        let clear_color = D2D1_COLOR_F {
            r: 0.12,
            g: 0.12,
            b: 0.12,
            a: 1.0,
        };
        rt.Clear(Some(&clear_color));

        // Get logical size (DIPs) for coordinate calculations
        let rt_size = rt.GetSize();
        let logical_width = rt_size.width;
        let logical_height = rt_size.height;

        // Get metrics
        let (cpu, ram, cpu_history, ram_history, cpu_cores_usage) = {
            if let Ok(state) = METRICS.read() {
                (state.cpu_usage, state.ram_usage_pct, state.cpu_history.clone(), state.ram_history.clone(), state.cpu_cores_usage.clone())
            } else {
                (0.0, 0.0, vec![0.0; 60], vec![0.0; 60], Vec::new())
            }
        };

        // Create brushes
        let panel_brush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.12, g: 0.12, b: 0.12, a: 1.0 }, // Dark gray solid background
            None,
        )?;
        let track_brush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.18, g: 0.18, b: 0.18, a: 1.0 }, // Subtle slot background track for bars
            None,
        )?;
        let white_brush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            None,
        )?;
        let cpu_brush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.0, g: 0.6, b: 1.0, a: 1.0 }, // Neon blue
            None,
        )?;
        let ram_brush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.1, g: 0.8, b: 0.2, a: 1.0 }, // Neon green
            None,
        )?;
        let grid_brush = rt.CreateSolidColorBrush(
            &D2D1_COLOR_F { r: 0.25, g: 0.25, b: 0.25, a: 1.0 }, // Solid gray for grid
            None,
        )?;

        // Draw solid background panel covering the ENTIRE client area to remove the magenta border.
        let panel_rect = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: logical_width,
            bottom: logical_height,
        };
        rt.FillRectangle(&panel_rect, &panel_brush);

        // Create text format
        let text_format = DWRITE_FACTORY.CreateTextFormat(
            w!("Segoe UI"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            11.0,
            w!("en-US"),
        )?;

        let show_per_core = crate::config::SHOW_PER_CORE.load(std::sync::atomic::Ordering::Relaxed);

        if show_per_core {
            // ==========================================
            // Mode 2: CPU per Core Bars + RAM Progress Bar (xMeters style)
            // ==========================================
            
            // Draw CPU Column (Left)
            let cpu_text = format!("CPU {:.0}%", cpu);
            let cpu_text_utf16: Vec<u16> = cpu_text.encode_utf16().collect();
            let cpu_text_layout_rect = D2D_RECT_F {
                left: 2.0,
                top: 2.0,
                right: 65.0,
                bottom: 16.0,
            };
            rt.DrawText(
                &cpu_text_utf16,
                &text_format,
                &cpu_text_layout_rect,
                &white_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // CPU Graph Area
            let graph_left = 2.0;
            let graph_right = 62.0;
            let graph_top = 18.0;
            let graph_bottom = logical_height - 4.0;
            let graph_height = graph_bottom - graph_top;
            let graph_width = graph_right - graph_left;

            // Draw per-core bars
            let num_cores = cpu_cores_usage.len();
            if num_cores > 0 {
                let gap = if num_cores > 16 { 0.5 } else { 1.0 };
                let total_gaps = (num_cores - 1) as f32 * gap;
                let bar_width = (graph_width - total_gaps) / num_cores as f32;

                for i in 0..num_cores {
                    let x1 = graph_left + i as f32 * (bar_width + gap);
                    let x2 = x1 + bar_width;
                    
                    // Draw slot background (track)
                    let track_rect = D2D_RECT_F {
                        left: x1,
                        top: graph_top,
                        right: x2,
                        bottom: graph_bottom,
                    };
                    rt.FillRectangle(&track_rect, &track_brush);

                    // Draw filled portion according to core usage
                    let usage = cpu_cores_usage[i];
                    let bar_h = (usage / 100.0) * graph_height;
                    let fill_rect = D2D_RECT_F {
                        left: x1,
                        top: graph_bottom - bar_h,
                        right: x2,
                        bottom: graph_bottom,
                    };
                    rt.FillRectangle(&fill_rect, &cpu_brush);
                }
            } else {
                // Fallback: draw border if cores aren't initialized yet
                let border_rect = D2D_RECT_F {
                    left: graph_left,
                    top: graph_top,
                    right: graph_right,
                    bottom: graph_bottom,
                };
                rt.DrawRectangle(&border_rect, &grid_brush, 0.5, None);
            }

            // Draw RAM Column (Right)
            let ram_text = format!("RAM {:.0}%", ram);
            let ram_text_utf16: Vec<u16> = ram_text.encode_utf16().collect();
            let ram_text_layout_rect = D2D_RECT_F {
                left: 72.0,
                top: 2.0,
                right: 135.0,
                bottom: 16.0,
            };
            rt.DrawText(
                &ram_text_utf16,
                &text_format,
                &ram_text_layout_rect,
                &white_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // RAM Graph Area - single thick bar graph
            let ram_graph_left = 72.0;
            let ram_graph_right = 132.0;
            
            // Draw RAM slot background
            let ram_track_rect = D2D_RECT_F {
                left: ram_graph_left,
                top: graph_top,
                right: ram_graph_right,
                bottom: graph_bottom,
            };
            rt.FillRectangle(&ram_track_rect, &track_brush);

            // Draw RAM filled portion
            let ram_bar_h = (ram / 100.0) * graph_height;
            let ram_fill_rect = D2D_RECT_F {
                left: ram_graph_left,
                top: graph_bottom - ram_bar_h,
                right: ram_graph_right,
                bottom: graph_bottom,
            };
            rt.FillRectangle(&ram_fill_rect, &ram_brush);

        } else {
            // ==========================================
            // Mode 1: Classic Sparkline View (Current style)
            // ==========================================
            
            // Draw CPU Column (Left)
            let cpu_text = format!("CPU {:.0}%", cpu);
            let cpu_text_utf16: Vec<u16> = cpu_text.encode_utf16().collect();
            let cpu_text_layout_rect = D2D_RECT_F {
                left: 2.0,
                top: 2.0,
                right: 65.0,
                bottom: 16.0,
            };
            rt.DrawText(
                &cpu_text_utf16,
                &text_format,
                &cpu_text_layout_rect,
                &white_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // CPU Graph
            let graph_left = 2.0;
            let graph_right = 62.0;
            let graph_top = 18.0;
            let graph_bottom = logical_height - 4.0;
            let graph_height = graph_bottom - graph_top;

            // Draw graph border
            let border_rect = D2D_RECT_F {
                left: graph_left,
                top: graph_top,
                right: graph_right,
                bottom: graph_bottom,
            };
            rt.DrawRectangle(&border_rect, &grid_brush, 0.5, None);

            // Draw sparkline
            let sample_count = cpu_history.len();
            if sample_count > 1 {
                let width_step = (graph_right - graph_left) / (sample_count - 1) as f32;
                for i in 0..sample_count - 1 {
                    let x1 = graph_left + i as f32 * width_step;
                    let y1 = graph_bottom - (cpu_history[i] / 100.0) * graph_height;
                    let x2 = graph_left + (i + 1) as f32 * width_step;
                    let y2 = graph_bottom - (cpu_history[i + 1] / 100.0) * graph_height;
                    rt.DrawLine(
                        D2D_POINT_2F { x: x1, y: y1 },
                        D2D_POINT_2F { x: x2, y: y2 },
                        &cpu_brush,
                        1.0,
                        None,
                    );
                }
            }

            // Draw RAM Column (Right)
            let ram_text = format!("RAM {:.0}%", ram);
            let ram_text_utf16: Vec<u16> = ram_text.encode_utf16().collect();
            let ram_text_layout_rect = D2D_RECT_F {
                left: 72.0,
                top: 2.0,
                right: 135.0,
                bottom: 16.0,
            };
            rt.DrawText(
                &ram_text_utf16,
                &text_format,
                &ram_text_layout_rect,
                &white_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );

            // RAM Graph
            let ram_graph_left = 72.0;
            let ram_graph_right = 132.0;

            let ram_border_rect = D2D_RECT_F {
                left: ram_graph_left,
                top: graph_top,
                right: ram_graph_right,
                bottom: graph_bottom,
            };
            rt.DrawRectangle(&ram_border_rect, &grid_brush, 0.5, None);

            let ram_sample_count = ram_history.len();
            if ram_sample_count > 1 {
                let width_step = (ram_graph_right - ram_graph_left) / (ram_sample_count - 1) as f32;
                for i in 0..ram_sample_count - 1 {
                    let x1 = ram_graph_left + i as f32 * width_step;
                    let y1 = graph_bottom - (ram_history[i] / 100.0) * graph_height;
                    let x2 = ram_graph_left + (i + 1) as f32 * width_step;
                    let y2 = graph_bottom - (ram_history[i + 1] / 100.0) * graph_height;
                    rt.DrawLine(
                        D2D_POINT_2F { x: x1, y: y1 },
                        D2D_POINT_2F { x: x2, y: y2 },
                        &ram_brush,
                        1.0,
                        None,
                    );
                }
            }
        }

        match rt.EndDraw(None, None) {
            Ok(_) => {}
            Err(e) => {
                // D2DERR_RECREATE_TARGET — device lost, drop target so it's rebuilt next paint
                if e.code().0 == 0x8899000C_u32 as i32 {
                    *rt_guard = None;
                } else {
                    return Err(e);
                }
            }
        }

        Ok(())
    }
}
