//! Donut-chart tray icon: a ring filled clockwise from 12 o'clock by the used
//! share, a dark center and a label (the drive letter) centered in it.
//! Rendered in software with 4x4 supersampling into a 32-bit alpha icon,
//! because plain GDI cannot antialias arcs.

use std::f32::consts::TAU;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject,
    DrawTextW, SelectObject, SetBkMode, SetTextColor, ANTIALIASED_QUALITY, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    DT_CENTER, DT_NOCLIP, DT_SINGLELINE, DT_VCENTER, FW_BOLD, HGDIOBJ, OUT_DEFAULT_PRECIS,
    TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

pub type Rgb = [u8; 3];

/// Hole radius as a share of the outer radius; the ring is the rest.
const HOLE_RATIO: f32 = 0.6;
const SUPERSAMPLE: usize = 4;
const LABEL_RGB: Rgb = [255, 255, 255];

pub struct RingColors {
    pub used: Rgb,
    pub free: Rgb,
    pub hole: Rgb,
}

/// Straight-alpha RGBA pixel with channels in 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Px {
    pub rgb: [f32; 3],
    pub a: f32,
}

const CLEAR: Px = Px { rgb: [0.0; 3], a: 0.0 };

fn to_f(c: Rgb) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

/// Renders the ring and hole into a `size`×`size` row-major buffer.
/// `used` is the filled share of the ring, clamped to 0..=1.
pub fn render_ring(size: usize, used: f32, colors: &RingColors) -> Vec<Px> {
    let used = used.clamp(0.0, 1.0);
    let r_out = size as f32 / 2.0;
    let r_in = r_out * HOLE_RATIO;
    let (c_used, c_free, c_hole) = (to_f(colors.used), to_f(colors.free), to_f(colors.hole));
    let step = 1.0 / SUPERSAMPLE as f32;
    let total = (SUPERSAMPLE * SUPERSAMPLE) as f32;

    let mut out = vec![CLEAR; size * size];
    for y in 0..size {
        for x in 0..size {
            let mut sum = [0.0f32; 3];
            let mut hits = 0.0f32;
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let dx = x as f32 + (sx as f32 + 0.5) * step - r_out;
                    let dy = y as f32 + (sy as f32 + 0.5) * step - r_out;
                    let d = (dx * dx + dy * dy).sqrt();
                    if d > r_out {
                        continue;
                    }
                    let c = if d < r_in {
                        c_hole
                    } else {
                        // Angle clockwise from 12 o'clock, in 0..TAU.
                        let angle = dx.atan2(-dy).rem_euclid(TAU);
                        if angle < used * TAU { c_used } else { c_free }
                    };
                    for i in 0..3 {
                        sum[i] += c[i];
                    }
                    hits += 1.0;
                }
            }
            if hits > 0.0 {
                out[y * size + x] = Px {
                    rgb: [sum[0] / hits, sum[1] / hits, sum[2] / hits],
                    a: hits / total,
                };
            }
        }
    }
    out
}

/// Composites a white label with per-pixel `coverage` (0..=255) over `pixels`,
/// shifted by (`dx`, `dy`).
pub fn blend_label(pixels: &mut [Px], size: usize, coverage: &[u8], cov_size: usize, dx: i32, dy: i32) {
    let label = to_f(LABEL_RGB);
    for cy in 0..cov_size {
        for cx in 0..cov_size {
            let c = coverage[cy * cov_size + cx] as f32 / 255.0;
            if c == 0.0 {
                continue;
            }
            let (x, y) = (cx as i32 + dx, cy as i32 + dy);
            if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
                continue;
            }
            let p = &mut pixels[y as usize * size + x as usize];
            let a = c + p.a * (1.0 - c);
            let under = p.a * (1.0 - c);
            for (ch, l) in p.rgb.iter_mut().zip(label) {
                *ch = (l * c + *ch * under) / a;
            }
            p.a = a;
        }
    }
}

/// Bounding box (x0, y0, x1, y1), inclusive, of non-zero coverage.
pub fn ink_bounds(coverage: &[u8], size: usize) -> Option<(usize, usize, usize, usize)> {
    let mut b: Option<(usize, usize, usize, usize)> = None;
    for y in 0..size {
        for x in 0..size {
            if coverage[y * size + x] > 0 {
                b = Some(match b {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
        }
    }
    b
}

/// Rasterizes `text` in bold Segoe UI with grayscale antialiasing and returns
/// its coverage in a `buf`×`buf` buffer (text roughly centered).
unsafe fn render_text_coverage(text: &str, font_px: i32, buf: usize) -> Option<Vec<u8>> {
    let (bits, hbm) = new_dib(buf)?;
    let hdc = CreateCompatibleDC(None);
    let old_bmp = SelectObject(hdc, HGDIOBJ(hbm.0));

    let font = CreateFontW(
        -font_px, 0, 0, 0, FW_BOLD.0 as i32, 0, 0, 0,
        DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32, CLIP_DEFAULT_PRECIS.0 as u32,
        ANTIALIASED_QUALITY.0 as u32, DEFAULT_PITCH.0 as u32,
        windows::core::w!("Segoe UI"),
    );
    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, COLORREF(0x00FF_FFFF));
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut rc = RECT { left: 0, top: 0, right: buf as i32, bottom: buf as i32 };
    DrawTextW(hdc, &mut wide, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOCLIP);

    // GDI writes gray levels into the color channels; take one as coverage.
    let px = std::slice::from_raw_parts(bits as *const u8, buf * buf * 4);
    let coverage = px.as_chunks::<4>().0.iter().map(|p| p[1]).collect();

    SelectObject(hdc, old_font);
    SelectObject(hdc, old_bmp);
    let _ = DeleteObject(HGDIOBJ(font.0));
    let _ = DeleteObject(HGDIOBJ(hbm.0));
    let _ = DeleteDC(hdc);
    Some(coverage)
}

/// Creates a zeroed top-down 32-bit DIB section.
unsafe fn new_dib(size: usize) -> Option<(*mut core::ffi::c_void, windows::Win32::Graphics::Gdi::HBITMAP)> {
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let hbm = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
    if bits.is_null() {
        let _ = DeleteObject(HGDIOBJ(hbm.0));
        return None;
    }
    Some((bits, hbm))
}

/// Builds the donut icon with `label` centered in the hole.
pub unsafe fn create_ring_icon(
    size: usize,
    used: f32,
    colors: &RingColors,
    label: &str,
) -> windows::core::Result<HICON> {
    let mut pixels = render_ring(size, used, colors);

    // Center the label by its actual ink, not its line box, so letters with
    // different shapes all sit in the middle of the hole.
    let hole = size as f32 * HOLE_RATIO;
    let font_px = (hole * 0.95).round() as i32;
    let buf = size * 2;
    if let Some(cov) = render_text_coverage(label, font_px, buf) {
        if let Some((x0, y0, x1, y1)) = ink_bounds(&cov, buf) {
            let mid = size as f32 / 2.0;
            let dx = (mid - (x0 + x1 + 1) as f32 / 2.0).round() as i32;
            let dy = (mid - (y0 + y1 + 1) as f32 / 2.0).round() as i32;
            blend_label(&mut pixels, size, &cov, buf, dx, dy);
        }
    }

    let (bits, hbm_color) = new_dib(size).ok_or_else(windows::core::Error::from_win32)?;
    let dst = std::slice::from_raw_parts_mut(bits as *mut u8, size * size * 4);
    for (p, out) in pixels.iter().zip(dst.as_chunks_mut::<4>().0) {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        // Icons take straight (non-premultiplied) BGRA.
        out.copy_from_slice(&[q(p.rgb[2]), q(p.rgb[1]), q(p.rgb[0]), q(p.a)]);
    }

    // The alpha channel defines transparency; the AND mask just has to exist.
    let mask_row = size.div_ceil(16) * 2;
    let mask = vec![0u8; mask_row * size];
    let hbm_mask = CreateBitmap(size as i32, size as i32, 1, 1, Some(mask.as_ptr() as *const _));

    let info = ICONINFO {
        fIcon: true.into(),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: hbm_mask,
        hbmColor: hbm_color,
    };
    let icon = CreateIconIndirect(&info);
    let _ = DeleteObject(HGDIOBJ(hbm_color.0));
    let _ = DeleteObject(HGDIOBJ(hbm_mask.0));
    icon
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLORS: RingColors = RingColors { used: [255, 0, 0], free: [0, 255, 0], hole: [0, 0, 255] };

    fn at(px: &[Px], size: usize, x: usize, y: usize) -> Px {
        px[y * size + x]
    }

    #[test]
    fn ring_fills_clockwise_from_top() {
        let size = 32;
        let px = render_ring(size, 0.25, &COLORS);
        // Right side, upper half of the ring: within the first quarter -> used.
        let p = at(&px, size, 26, 10);
        assert!(p.rgb[0] > 0.9 && p.a == 1.0, "{p:?}");
        // Left side of the ring -> free.
        let p = at(&px, size, 3, 16);
        assert!(p.rgb[1] > 0.9 && p.a == 1.0, "{p:?}");
        // Center -> hole.
        let p = at(&px, size, 16, 16);
        assert!(p.rgb[2] > 0.9 && p.a == 1.0, "{p:?}");
        // Corner -> outside the disc.
        assert_eq!(at(&px, size, 0, 0).a, 0.0);
    }

    #[test]
    fn ring_handles_empty_and_full() {
        let size = 16;
        let empty = render_ring(size, 0.0, &COLORS);
        let full = render_ring(size, 1.0, &COLORS);
        assert!(at(&empty, size, 8, 1).rgb[1] > 0.9);
        assert!(at(&full, size, 8, 1).rgb[0] > 0.9);
    }

    #[test]
    fn label_blends_and_bounds_are_found() {
        let size = 4;
        let mut cov = vec![0u8; size * size];
        cov[5] = 255;
        cov[10] = 128;
        assert_eq!(ink_bounds(&cov, size), Some((1, 1, 2, 2)));
        assert_eq!(ink_bounds(&[0u8; 16], size), None);

        let mut px = vec![CLEAR; size * size];
        blend_label(&mut px, size, &cov, size, 0, 0);
        assert_eq!(px[5].a, 1.0);
        assert_eq!(px[5].rgb, [1.0, 1.0, 1.0]);
        assert!((px[10].a - 128.0 / 255.0).abs() < 1e-6);
    }
}
