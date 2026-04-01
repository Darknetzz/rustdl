//! Raster app icon (window + in-app logo). egui recommends ~256² for [`egui::IconData`];
//! 64² often looks bad or fails platform expectations on Windows taskbars.

use eframe::egui::{ColorImage, Context, IconData, TextureHandle, TextureOptions};
use image::{imageops::FilterType, RgbaImage};

const W: usize = 64;
const H: usize = 64;

fn icon_rgba_64() -> Vec<u8> {
    let width = W;
    let height = H;
    let mut rgba = vec![0_u8; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) * 4;
            let t = y as f32 / (height.saturating_sub(1) as f32);
            let bg_r = 20.0 + t * 18.0;
            let bg_g = 24.0 + t * 22.0;
            let bg_b = 28.0 + t * 30.0;
            rgba[i] = bg_r.round().clamp(0.0, 255.0) as u8;
            rgba[i + 1] = bg_g.round().clamp(0.0, 255.0) as u8;
            rgba[i + 2] = bg_b.round().clamp(0.0, 255.0) as u8;
            rgba[i + 3] = 255;
        }
    }

    let glyph = [234_u8, 238_u8, 244_u8, 255_u8];
    for y in 14..34 {
        for x in 30..34 {
            let i = (y * width + x) * 4;
            rgba[i..i + 4].copy_from_slice(&glyph);
        }
    }
    for row in 0..12 {
        let y = 28 + row;
        let left = 32_i32 - row as i32;
        let right = 32_i32 + row as i32;
        for x in left..=right {
            if !(0..width as i32).contains(&x) || !(0..height as i32).contains(&(y as i32)) {
                continue;
            }
            let i = (y * width + x as usize) * 4;
            rgba[i..i + 4].copy_from_slice(&glyph);
        }
    }
    for y in 44..50 {
        for x in 16..48 {
            let i = (y * width + x) * 4;
            rgba[i..i + 4].copy_from_slice(&glyph);
        }
    }
    for y in 13..20 {
        for x in 44..51 {
            let i = (y * width + x) * 4;
            rgba[i] = 233;
            rgba[i + 1] = 110;
            rgba[i + 2] = 54;
            rgba[i + 3] = 255;
        }
    }

    rgba
}

/// Icon for the native window / taskbar (high resolution for Windows scaling).
pub fn window_icon() -> IconData {
    let rgba = icon_rgba_64();
    let img = RgbaImage::from_raw(64, 64, rgba).expect("icon 64x64");
    let scaled = image::imageops::resize(&img, 256, 256, FilterType::Lanczos3);
    IconData {
        rgba: scaled.into_raw(),
        width: 256,
        height: 256,
    }
}

/// Small texture for the in-app title row.
pub fn load_logo_texture(ctx: &Context) -> TextureHandle {
    let rgba = icon_rgba_64();
    let img = RgbaImage::from_raw(64, 64, rgba).expect("icon 64x64");
    let scaled = image::imageops::resize(&img, 48, 48, FilterType::Lanczos3);
    let ci = ColorImage::from_rgba_unmultiplied([48, 48], scaled.as_raw());
    ctx.load_texture("rustdl_logo", ci, TextureOptions::LINEAR)
}
