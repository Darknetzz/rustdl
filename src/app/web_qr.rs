use eframe::egui;
use eframe::egui::Color32;
use qrcode::QrCode;

/// Draws a compact QR code for LAN web UI onboarding (token URL).
pub fn draw_qr_code(ui: &mut egui::Ui, content: &str, size: f32) {
    let Ok(code) = QrCode::new(content.as_bytes()) else {
        ui.colored_label(Color32::LIGHT_RED, "Could not encode QR data.");
        return;
    };
    let modules = code.width();
    if modules == 0 {
        return;
    }
    let cell = size / modules as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, Color32::WHITE);
    for y in 0..modules {
        for x in 0..modules {
            if code[(x, y)] == qrcode::types::Color::Dark {
                let min = rect.min + egui::vec2(x as f32 * cell, y as f32 * cell);
                let cell_rect = egui::Rect::from_min_size(min, egui::vec2(cell, cell));
                painter.rect_filled(cell_rect, 0.0, Color32::BLACK);
            }
        }
    }
}
