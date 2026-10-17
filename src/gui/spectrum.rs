use eframe::egui::{self, Color32, Pos2, Stroke};

pub struct SpectrumView {
    pub show_pseudospectrum: bool,
    pub smoothed_curve: Vec<f32>,
    pub peak_falling: Vec<f32>,
    pub last_center_hz: u64,
}

impl SpectrumView {
    pub fn new() -> Self {
        Self {
            show_pseudospectrum: true,
            smoothed_curve: Vec::new(),
            peak_falling: Vec::new(),
            last_center_hz: 0,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        height: f32,
        spectrum_db: &[f32],
        pseudo_db: &[f32],
        noise_baseline: &[f32],
        center_freq_hz: u64,
        sample_rate_hz: u32,
        squelch_snr_db: f32,
        scan_cursor_hz: u64,
        is_scanning: bool,
        vfo_markers: &[(u64, String, bool, bool)], // (freq, label, is_active, is_pinned)
        bookmark_markers: &[(u64, String)],        // (freq, label)
    ) -> egui::Response {
        let is_retune = center_freq_hz != self.last_center_hz;
        if is_retune {
            self.smoothed_curve.clear();
            self.peak_falling.clear();
            self.last_center_hz = center_freq_hz;
        }

        let available_size = ui.available_size();
        let target_size = egui::vec2(available_size.x, height);

        let (rect, response) = ui.allocate_exact_size(target_size, egui::Sense::click());
        let painter = ui.painter_at(rect);

        // Background
        painter.rect_filled(rect, 4.0, Color32::from_rgb(12, 16, 24));
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, Color32::from_rgb(35, 45, 65)));

        let half_bw = (sample_rate_hz as f64) * 0.5;
        let min_f = (center_freq_hz as f64 - half_bw).max(0.0);
        let max_f = center_freq_hz as f64 + half_bw;
        let bw = max_f - min_f;

        let freq_to_x = |f_hz: u64| -> f32 {
            let norm = ((f_hz as f64 - min_f) / bw).clamp(0.0, 1.0) as f32;
            rect.min.x + norm * rect.width()
        };

        let (db_min, db_max) = if self.show_pseudospectrum {
            (-5.0f32, 45.0f32)
        } else {
            (-120.0f32, -20.0f32)
        };

        let db_to_y = |db: f32| -> f32 {
            let norm = ((db - db_min) / (db_max - db_min)).clamp(0.0, 1.0);
            rect.max.y - norm * rect.height()
        };

        // Grid lines (dB)
        let db_step = if self.show_pseudospectrum { 10.0 } else { 20.0 };
        let mut cur_db = (db_min / db_step).ceil() * db_step;
        while cur_db <= db_max {
            let y = db_to_y(cur_db);
            painter.line_segment(
                [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                Stroke::new(0.5_f32, Color32::from_rgb(25, 35, 50)),
            );
            let label = if self.show_pseudospectrum {
                format!("+{}dB", cur_db as i32)
            } else {
                format!("{}dB", cur_db as i32)
            };
            painter.text(
                egui::pos2(rect.min.x + 4.0, y - 10.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::monospace(10.0),
                Color32::from_rgb(80, 100, 130),
            );
            cur_db += db_step;
        }

        // Draw Frequency scale ticks & labels at bottom
        let freq_step_mhz = if bw >= 8_000_000.0 {
            1.0
        } else if bw >= 4_000_000.0 {
            0.5
        } else {
            0.2
        };
        let start_mhz = (min_f / 1e6 / freq_step_mhz).ceil() * freq_step_mhz;
        let mut f_mhz = start_mhz;
        while f_mhz * 1e6 <= max_f {
            let fx = freq_to_x((f_mhz * 1e6) as u64);
            painter.line_segment(
                [egui::pos2(fx, rect.max.y - 6.0), egui::pos2(fx, rect.max.y)],
                Stroke::new(1.0_f32, Color32::from_rgb(50, 70, 95)),
            );
            painter.text(
                egui::pos2(fx, rect.max.y - 16.0),
                egui::Align2::CENTER_BOTTOM,
                format!("{:.1}", f_mhz),
                egui::FontId::monospace(9.5),
                Color32::from_rgb(70, 90, 120),
            );
            f_mhz += freq_step_mhz;
        }

        // Squelch Cutoff threshold
        let squelch_threshold = if self.show_pseudospectrum {
            squelch_snr_db
        } else {
            let avg_base = if !noise_baseline.is_empty() {
                noise_baseline.iter().sum::<f32>() / noise_baseline.len() as f32
            } else {
                -100.0
            };
            avg_base + squelch_snr_db
        };

        // Draw Squelch Threshold Line
        let squelch_y = db_to_y(squelch_threshold);
        painter.line_segment(
            [egui::pos2(rect.min.x, squelch_y), egui::pos2(rect.max.x, squelch_y)],
            Stroke::new(1.2_f32, Color32::from_rgb(255, 60, 50)),
        );
        painter.text(
            egui::pos2(rect.max.x - 120.0, squelch_y - 12.0),
            egui::Align2::LEFT_TOP,
            format!("--- SQUELCH +{:.1} dB ---", squelch_snr_db),
            egui::FontId::monospace(10.0),
            Color32::from_rgb(255, 90, 70),
        );

        // Draw Spectrum Curve
        let active_data = if self.show_pseudospectrum {
            pseudo_db
        } else {
            spectrum_db
        };

        if !active_data.is_empty() {
            let n = active_data.len();

            let width = rect.width();
            let step_px = 1.5f32;
            let num_points = ((width / step_px).ceil() as usize) + 2;

            if self.smoothed_curve.len() != num_points {
                let init_val = if self.show_pseudospectrum { 0.0 } else { -110.0 };
                self.smoothed_curve = vec![init_val; num_points];
                self.peak_falling = vec![-150.0; num_points];
            }

            let mut points = Vec::with_capacity(num_points);
            let mut peak_caps: Vec<[Pos2; 2]> = Vec::new();

            let mut x_pixel = 0.0f32;
            let mut pt_idx = 0;
            while x_pixel <= width && pt_idx < num_points {
                let bin_start = ((x_pixel / width) * (n as f32)) as usize;
                let bin_end = (((x_pixel + step_px) / width) * (n as f32)).ceil() as usize;
                let bin_end = bin_end.max(bin_start + 1).min(n);

                let mut peak_db = -150.0f32;
                for i in bin_start..bin_end {
                    peak_db = peak_db.max(active_data[i]);
                }
                // Display true peak magnitude within the pixel's bin window so signals stand out tall and crisp
                let raw_val = peak_db;

                // 1. Calmer, smoother green line (EMA with fast attack, smooth decay)
                let prev_smooth = self.smoothed_curve[pt_idx];
                let smooth_db = if is_retune || prev_smooth < -140.0 {
                    raw_val
                } else if raw_val > prev_smooth {
                    prev_smooth * 0.30 + raw_val * 0.70 // Fast, crisp attack on rising carrier
                } else {
                    prev_smooth * 0.85 + raw_val * 0.15 // Smooth and calm on noise decay
                };
                self.smoothed_curve[pt_idx] = smooth_db;

                // 2. Falling peak history for strong peaks above squelch
                // "koville piikeille myös spektriin sellanen tippuva historia. jos on suqlchin alapuolelle niin tätä spektrin historia falling ei ole."
                let prev_peak = self.peak_falling[pt_idx];
                let current_peak = if smooth_db >= squelch_threshold {
                    if smooth_db > prev_peak {
                        smooth_db
                    } else {
                        // Smooth gravity drop (0.45 dB per frame)
                        (prev_peak - 0.45).max(squelch_threshold)
                    }
                } else {
                    // Below squelch threshold: no falling history shown!
                    -150.0
                };
                self.peak_falling[pt_idx] = current_peak;

                let px = rect.min.x + x_pixel;
                let py = db_to_y(smooth_db);
                points.push(Pos2::new(px, py));

                // If peak is above squelch and above the current curve, record peak cap
                if current_peak >= squelch_threshold && current_peak > smooth_db + 0.6 {
                    let peak_y = db_to_y(current_peak);
                    peak_caps.push([Pos2::new(px - 1.5, peak_y), Pos2::new(px + 1.5, peak_y)]);
                }

                x_pixel += step_px;
                pt_idx += 1;
            }

            // Draw calm green curve
            if points.len() >= 2 {
                let stroke_color = if self.show_pseudospectrum {
                    Color32::from_rgb(0, 255, 180)
                } else {
                    Color32::from_rgb(0, 190, 255)
                };
                painter.add(egui::Shape::line(points, Stroke::new(1.6_f32, stroke_color)));
            }

            // Draw falling peak history caps in bright gold/amber for peaks above squelch
            for cap in peak_caps {
                painter.line_segment(cap, Stroke::new(2.0_f32, Color32::from_rgb(255, 215, 60)));
            }

            // Draw Dynamic Noise Floor line when in raw mode
            if !self.show_pseudospectrum && !noise_baseline.is_empty() {
                let mut nf_points = Vec::with_capacity(rect.width() as usize);
                let nb_len = noise_baseline.len();
                let width = rect.width();
                let mut x_p = 0.0f32;
                while x_p <= width {
                    let idx = (((x_p / width) * (nb_len as f32)) as usize).min(nb_len - 1);
                    let db = noise_baseline[idx];
                    nf_points.push(Pos2::new(rect.min.x + x_p, db_to_y(db)));
                    x_p += 2.0;
                }
                if nf_points.len() >= 2 {
                    painter.add(egui::Shape::line(
                        nf_points,
                        Stroke::new(1.0_f32, Color32::from_rgb(255, 140, 0)),
                    ));
                }
            }
        }

        // Draw permanent Known Frequency markers from Bookmarks
        for (f_hz, name) in bookmark_markers {
            if *f_hz >= min_f as u64 && *f_hz <= max_f as u64 {
                // If a VFO is already actively locked or pinned on this frequency, let the VFO marker handle it
                let has_active_vfo = vfo_markers.iter().any(|(vf, _, _, _)| vf.abs_diff(*f_hz) <= 4000);
                if !has_active_vfo {
                    let x = freq_to_x(*f_hz);
                    let mark_col = Color32::from_rgb(0, 175, 225);

                    // Vertical dashed-style guideline
                    let stem_y = (rect.min.y + 20.0).min(rect.max.y);
                    painter.line_segment(
                        [egui::pos2(x, stem_y), egui::pos2(x, rect.max.y)],
                        Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(0, 160, 220, 80)),
                    );

                    // Marker pin at top
                    painter.circle_filled(egui::pos2(x, stem_y), 2.5, mark_col);

                    // Top label badge with bookmark name
                    let badge_text = name.as_str();
                    let text_w = (badge_text.len() as f32 * 6.5 + 8.0).max(28.0);
                    let badge_rect = egui::Rect::from_center_size(
                        egui::pos2(x, rect.min.y + 9.0),
                        egui::vec2(text_w, 13.0),
                    );
                    painter.rect_filled(badge_rect, 2.0, Color32::from_rgba_premultiplied(12, 18, 28, 220));
                    painter.rect_stroke(badge_rect, 2.0, Stroke::new(0.8_f32, Color32::from_rgba_premultiplied(0, 180, 230, 140)));

                    painter.text(
                        egui::pos2(x, rect.min.y + 9.0),
                        egui::Align2::CENTER_CENTER,
                        badge_text,
                        egui::FontId::monospace(8.5),
                        Color32::from_rgb(170, 225, 255),
                    );
                }
            }
        }

        // Draw VFO markers (Stem Pins with Circular Heads and Badges matching MykolaSDR)
        for (f_hz, label, is_active, is_pinned) in vfo_markers {
            if *f_hz >= min_f as u64 && *f_hz <= max_f as u64 {
                let x = freq_to_x(*f_hz);
                let col = if *is_active {
                    Color32::from_rgb(50, 255, 80) // Glowing green when audio is unmuted/speaking
                } else if *is_pinned {
                    Color32::from_rgb(0, 220, 255) // Cyan when pinned/paused
                } else {
                    Color32::from_rgb(255, 215, 0) // Amber/Yellow when detected
                };

                // Vertical stem pin
                painter.line_segment(
                    [egui::pos2(x, rect.min.y + 18.0), egui::pos2(x, rect.max.y)],
                    Stroke::new(if *is_active { 2.0_f32 } else { 1.2_f32 }, col),
                );

                // Circular pin cap at top
                painter.circle_filled(egui::pos2(x, rect.min.y + 18.0), 3.5, col);

                // Top label box
                let badge_rect = egui::Rect::from_center_size(
                    egui::pos2(x, rect.min.y + 8.0),
                    egui::vec2((label.len() as f32 * 7.0 + 8.0).max(30.0), 14.0),
                );
                painter.rect_filled(badge_rect, 2.0, Color32::from_rgba_premultiplied(15, 20, 30, 220));
                painter.rect_stroke(badge_rect, 2.0, Stroke::new(1.0_f32, col));

                painter.text(
                    egui::pos2(x, rect.min.y + 8.0),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::monospace(9.5),
                    col,
                );
            }
        }

        // Subtle sweep indicator along the top ruler line
        if is_scanning {
            if scan_cursor_hz >= min_f as u64 && scan_cursor_hz <= max_f as u64 {
                let x = freq_to_x(scan_cursor_hz);
                painter.line_segment(
                    [egui::pos2(x, rect.min.y), egui::pos2(x, rect.min.y + 5.0)],
                    Stroke::new(2.0_f32, Color32::from_rgb(0, 220, 255)),
                );
            }
        }

        response
    }
}
