//! Windows: winit/egui set only the small title-bar icon via `with_window_icon`. The taskbar uses
//! `WM_SETICON` / `ICON_BIG`, which stays unset unless `with_taskbar_icon` is used — egui-winit does
//! not wire that up. eframe's fallback uses `GetActiveWindow()`, which is often wrong for winit.
//! We set both icons using the real HWND from [`raw_window_handle`].

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::IconData;
use image::{imageops::FilterType, RgbaImage};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIcon, DestroyIcon, SendMessageW, HICON, ICON_BIG, ICON_SMALL, WM_SETICON,
};

static ICONS_APPLIED: AtomicBool = AtomicBool::new(false);

const PIXEL_SIZE: usize = 4;

pub fn apply_native_window_icons(frame: &impl HasWindowHandle, icon: &IconData) {
    if ICONS_APPLIED.load(Ordering::SeqCst) {
        return;
    }
    let Some(hwnd) = hwnd_from_frame(frame) else {
        return;
    };
    let Some(big) = hicon_from_icon_data(icon, 256) else {
        return;
    };
    let Some(small) = hicon_from_icon_data(icon, 32) else {
        unsafe {
            DestroyIcon(big);
        }
        return;
    };
    unsafe {
        let prev_big = SendMessageW(hwnd, WM_SETICON, ICON_BIG as WPARAM, big as LPARAM);
        if prev_big != 0 {
            DestroyIcon(prev_big as HICON);
        }
        let prev_small = SendMessageW(hwnd, WM_SETICON, ICON_SMALL as WPARAM, small as LPARAM);
        if prev_small != 0 {
            DestroyIcon(prev_small as HICON);
        }
    }
    ICONS_APPLIED.store(true, Ordering::SeqCst);
}

fn hwnd_from_frame(frame: &impl HasWindowHandle) -> Option<HWND> {
    let handle = frame.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
        _ => None,
    }
}

fn hicon_from_icon_data(data: &IconData, size: u32) -> Option<HICON> {
    let img = RgbaImage::from_raw(data.width, data.height, data.rgba.clone())?;
    let scaled = image::imageops::resize(&img, size, size, FilterType::Lanczos3);
    let width = scaled.width() as i32;
    let height = scaled.height() as i32;
    let mut rgba = scaled.into_raw();
    let pixel_count = rgba.len() / PIXEL_SIZE;
    if pixel_count * PIXEL_SIZE != rgba.len() {
        return None;
    }
    let mut and_mask = Vec::with_capacity(pixel_count);
    for chunk in rgba.chunks_exact_mut(PIXEL_SIZE) {
        and_mask.push(chunk[3].wrapping_sub(u8::MAX));
        chunk.swap(0, 2);
    }
    let handle = unsafe {
        CreateIcon(
            0,
            width,
            height,
            1,
            (PIXEL_SIZE * 8) as u8,
            and_mask.as_ptr(),
            rgba.as_ptr(),
        )
    };
    if handle == 0 {
        None
    } else {
        Some(handle)
    }
}
