use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};

pub struct WaterfallView {
    width: usize,
    height: usize,
    buffer: Vec<Color32>,
    texture: Option<TextureHandle>,
}

impl WaterfallView {
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        Self {
            width,
            height,
            buffer: vec![Color32::from_rgb(10, 15, 25); size],
            texture: None,
        }
    }

    /// Adds a new row of FFT / Pseudospectrum data (scrolls waterfall downwards)
    pub fn add_line(&mut self, spectrum_db: &[f32], min_db: f32, max_db: f32) {
        if spectrum_db.is_empty() {
            return;
        }

        // Scroll buffer down by 1 row
        let row_stride = self.width;
        let last_idx = self.height - 1;
        self.buffer.copy_within(0..last_idx * row_stride, row_stride);

        // Render new top row with colormap
        let range = (max_db - min_db).max(1.0);
        let src_len = spectrum_db.len();

        for x in 0..self.width {
            let src_idx = (x * src_len) / self.width;
            let val = spectrum_db[src_idx];
            let norm = ((val - min_db) / range).clamp(0.0, 1.0);
            self.buffer[x] = colormap_magma(norm);
        }
    }

    /// Updates egui texture and paints waterfall
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        center_freq_hz: u64,
        sample_rate_hz: u32,
        _scan_cursor_hz: u64,
        _is_scanning: bool,
        vfo_markers: &[(u64, String, bool, bool)], // (freq, label, is_active, is_pinned)
    ) -> egui::Response {
        // Ensure texture is loaded
        let image = ColorImage {
            size: [self.width, self.height],
            pixels: self.buffer.clone(),
        };

        let texture = self
            .texture
            .get_or_insert_with(|| ui.ctx().load_texture("waterfall", image.clone(), TextureOptions::LINEAR));
        texture.set(image, TextureOptions::LINEAR);

        let available_size = ui.available_size();
        let height = (available_size.y - 4.0).max(350.0);
        let target_size = egui::vec2(available_size.x, height);

        let (rect, response) = ui.allocate_exact_size(target_size, egui::Sense::click());
        let painter = ui.painter_at(rect);

        // Draw waterfall image
        painter.image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        let half_bw = (sample_rate_hz as f64) * 0.5;
        let min_f = (center_freq_hz as f64 - half_bw).max(0.0);
        let max_f = center_freq_hz as f64 + half_bw;
        let bw = max_f - min_f;

        let freq_to_x = |f_hz: u64| -> f32 {
            let norm = ((f_hz as f64 - min_f) / bw).clamp(0.0, 1.0) as f32;
            rect.min.x + norm * rect.width()
        };

        // Draw VFO vertical markers
        for (f_hz, label, is_active, is_pinned) in vfo_markers {
            if *f_hz >= min_f as u64 && *f_hz <= max_f as u64 {
                let x = freq_to_x(*f_hz);
                let col = if *is_active {
                    Color32::from_rgb(50, 255, 80) // Glowing green when speaking
                } else if *is_pinned {
                    Color32::from_rgb(0, 220, 255) // Cyan when pinned
                } else {
                    Color32::from_rgb(140, 160, 180)
                };

                painter.line_segment(
                    [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                    egui::Stroke::new(if *is_active { 2.5_f32 } else { 1.5_f32 }, col),
                );

                // Small badge at bottom of waterfall
                if *is_active || *is_pinned {
                    painter.text(
                        egui::pos2(x, rect.max.y - 14.0),
                        egui::Align2::CENTER_TOP,
                        label,
                        egui::FontId::monospace(9.0),
                        col,
                    );
                }
            }
        }

        response
    }
}

/// Tactical heatmap colormap (navy -> cyan -> amber -> red -> white)
fn colormap_magma(t: f32) -> Color32 {
    if t < 0.25 {
        let n = t / 0.25;
        Color32::from_rgb((10.0 + n * 20.0) as u8, (15.0 + n * 60.0) as u8, (35.0 + n * 140.0) as u8)
    } else if t < 0.50 {
        let n = (t - 0.25) / 0.25;
        Color32::from_rgb((30.0 + n * 50.0) as u8, (75.0 + n * 150.0) as u8, (175.0 - n * 50.0) as u8)
    } else if t < 0.75 {
        let n = (t - 0.50) / 0.25;
        Color32::from_rgb((80.0 + n * 175.0) as u8, (225.0 - n * 30.0) as u8, (125.0 - n * 115.0) as u8)
    } else {
        let n = (t - 0.75) / 0.25;
        Color32::from_rgb(255, (195.0 + n * 60.0) as u8, (10.0 + n * 245.0) as u8)
    }
}
