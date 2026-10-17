use crate::config::{Bookmark, DemodMode, ScannerConfig};
use crate::dsp_worker::{DspCommand, VfoDisplayInfo};
use crate::scanner::vfo::VfoState;
use crossbeam_channel::Sender;
use eframe::egui::{self, Color32, RichText};
use std::collections::HashMap;

pub struct VfoPanel {
    pub comment_edits: HashMap<usize, (u64, String)>,
    pub editing_vfo_id: Option<usize>,
    pub selected_vfo_id: Option<usize>,
}

impl VfoPanel {
    pub fn new() -> Self {
        Self {
            comment_edits: HashMap::new(),
            editing_vfo_id: None,
            selected_vfo_id: None,
        }
    }

    /// Renders dynamic VFO rows (Active, Pinned, and History) with responsive side-by-side grid
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        vfos: &[VfoDisplayInfo],
        cfg: &mut ScannerConfig,
        cmd_tx: &Sender<DspCommand>,
        rack_height: f32,
    ) {
        let active_count = vfos
            .iter()
            .take(cfg.max_vfos)
            .filter(|v| {
                v.state == VfoState::Pinned
                    || (v.state == VfoState::Locked && (v.active || v.delay_remaining_sec > 0.0))
            })
            .count();

        let history_count = vfos
            .iter()
            .take(cfg.max_vfos)
            .filter(|v| v.state == VfoState::History)
            .count();

        let visible_vfos: Vec<&VfoDisplayInfo> = vfos
            .iter()
            .take(cfg.max_vfos)
            .filter(|v| match v.state {
                VfoState::Idle => false,
                VfoState::History | VfoState::Pinned => true,
                VfoState::Locked => {
                    v.active
                        || v.delay_remaining_sec > 0.0
                        || v.dmr_info.is_some()
                        || v.latest_telemetry_info.is_some()
                }
            })
            .collect();

        // 1. Header Bar: Channel telemetry and Clear History control
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("CHANNELS:")
                    .color(Color32::from_rgb(180, 195, 210))
                    .size(11.0)
                    .strong(),
            );

            ui.label(
                RichText::new(format!("ACTIVE: {}/{}", active_count, cfg.max_vfos))
                    .color(if active_count > 0 {
                        Color32::from_rgb(0, 255, 128)
                    } else {
                        Color32::from_rgb(120, 140, 160)
                    })
                    .size(11.0)
                    .strong()
                    .monospace(),
            );

            ui.label(
                RichText::new(format!("HISTORY: {}/{}", history_count, cfg.max_vfos))
                    .color(if history_count > 0 {
                        Color32::from_rgb(255, 190, 80)
                    } else {
                        Color32::from_rgb(100, 110, 120)
                    })
                    .size(11.0)
                    .monospace(),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if history_count > 0 {
                    let clear_btn = ui.add(
                        egui::Button::new(
                            RichText::new("🧹 Clear History")
                                .color(Color32::WHITE)
                                .size(10.5),
                        )
                        .fill(Color32::from_rgb(60, 50, 60)),
                    );
                    if clear_btn.clicked() {
                        let _ = cmd_tx.send(DspCommand::ClearHistory);
                    }
                    clear_btn.on_hover_text("Wipe inactive historical channels from list");
                }
            });
        });

        ui.add_space(2.0);

        if visible_vfos.is_empty() {
            // Clean standby banner when scanner is sweeping with no active traffic
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 70.0),
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    ui.label(
                        RichText::new(format!(
                            "⚡ SPECTRAVOX ACTIVE SWEEP (0 / {} Active Channels) — Awaiting incoming traffic...",
                            cfg.max_vfos
                        ))
                        .color(Color32::from_rgb(90, 140, 180))
                        .size(12.5)
                        .italics(),
                    );
                },
            );
        } else {
            // Responsive layout: when width >= 1300 px, place VFO cards side-by-side in 2 columns
            let avail_w = ui.available_width();
            let use_two_cols = avail_w >= 1300.0 && visible_vfos.len() > 1;

            let scroll_h = (rack_height - 32.0).max(60.0);
            egui::ScrollArea::vertical()
                .id_salt("vfo_rack_scroll_area")
                .max_height(scroll_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if use_two_cols {
                        let col_w = ((avail_w - 10.0) / 2.0).floor();
                        for chunk in visible_vfos.chunks(2) {
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(col_w, 36.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        self.render_vfo_card(ui, chunk[0], true, col_w, cfg, cmd_tx);
                                    },
                                );
                                if chunk.len() > 1 {
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(col_w, 36.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            self.render_vfo_card(ui, chunk[1], true, col_w, cfg, cmd_tx);
                                        },
                                    );
                                }
                            });
                            ui.add_space(2.0);
                        }
                    } else {
                        for vfo in visible_vfos {
                            self.render_vfo_card(ui, vfo, false, avail_w, cfg, cmd_tx);
                            ui.add_space(2.0);
                        }
                    }
                });
        }
    }

    /// Renders an individual compact or full-width VFO card
    fn render_vfo_card(
        &mut self,
        ui: &mut egui::Ui,
        vfo: &VfoDisplayInfo,
        is_compact: bool,
        card_width: f32,
        cfg: &mut ScannerConfig,
        cmd_tx: &Sender<DspCommand>,
    ) {
        let is_selected = self.selected_vfo_id == Some(vfo.id);
        let is_pinned = vfo.state == VfoState::Pinned;
        let is_history = vfo.state == VfoState::History;

        let bg_color = if is_selected {
            Color32::from_rgb(18, 30, 40) // Distinct dark teal tint for selected VFO
        } else if is_pinned {
            Color32::from_rgb(16, 28, 42) // Cyan tint for pinned
        } else if is_history {
            Color32::from_rgb(18, 20, 24) // Dim gray tint for history
        } else if vfo.active {
            Color32::from_rgb(16, 32, 22) // Green tint for active voice
        } else {
            Color32::from_rgb(20, 24, 28) // Neutral tint for squelched hold
        };

        let stroke_color = if is_selected {
            Color32::from_rgb(0, 255, 180) // Glowing mint border for selected VFO
        } else if is_pinned {
            Color32::from_rgb(0, 190, 230)
        } else if is_history {
            Color32::from_rgb(45, 52, 60)
        } else if vfo.active {
            Color32::from_rgb(40, 180, 80)
        } else {
            Color32::from_rgb(60, 70, 80)
        };

        egui::Frame::none()
            .fill(bg_color)
            .stroke(egui::Stroke::new(if is_selected { 1.5_f32 } else { 1.0_f32 }, stroke_color))
            .rounding(4.0)
            .inner_margin(egui::Margin::symmetric(6.0, 3.0))
            .show(ui, |ui| {
                ui.set_min_width(card_width - 16.0);
                ui.set_max_width(card_width - 16.0);
                ui.horizontal(|ui| {
                    // 1. [ || ] PAUSE / PIN BUTTON (Enlarged)
                    let pause_btn = ui.add_sized(
                        [36.0, 30.0],
                        egui::Button::new(
                            RichText::new("⏸")
                                .color(if is_pinned { Color32::BLACK } else { Color32::WHITE })
                                .size(14.0)
                                .strong(),
                        )
                        .fill(if is_pinned {
                            Color32::from_rgb(0, 220, 255)
                        } else {
                            Color32::from_rgb(45, 55, 70)
                        }),
                    );
                    if pause_btn.clicked() {
                        self.selected_vfo_id = Some(vfo.id);
                        let _ = cmd_tx.send(DspCommand::TogglePinVfo(vfo.id));
                    }
                    pause_btn.on_hover_text(if is_history {
                        "Pin & Listen: Lock this frequency and un-mute audio"
                    } else {
                        "Pause / Pin: Lock receiver permanently on this channel"
                    });

                    // 2. [ ✕ ] CLOSE / DELETE BUTTON (Enlarged)
                    let close_btn = ui.add_sized(
                        [36.0, 30.0],
                        egui::Button::new(
                            RichText::new("✕")
                                .color(Color32::WHITE)
                                .size(14.0)
                                .strong(),
                        )
                        .fill(Color32::from_rgb(160, 40, 40)),
                    );
                    if close_btn.clicked() {
                        let _ = cmd_tx.send(DspCommand::CloseVfo(vfo.id));
                        let _ = cmd_tx.send(DspCommand::SkipFrequency(vfo.freq_hz, 15));
                    }
                    close_btn.on_hover_text("Close / Release this channel slot");

                    // 3. [ 🚫 ] BLACKLIST BUTTON (Enlarged)
                    let ignore_btn = ui.add_sized(
                        [96.0, 30.0],
                        egui::Button::new(
                            RichText::new("🚫 IGNORE")
                                .color(Color32::WHITE)
                                .size(11.5)
                                .strong(),
                        )
                        .fill(Color32::from_rgb(140, 40, 30)),
                    );
                    if ignore_btn.clicked() {
                        cfg.add_blacklist(vfo.freq_hz);
                        let _ = cmd_tx.send(DspCommand::AddBlacklist(vfo.freq_hz));
                        let _ = cmd_tx.send(DspCommand::CloseVfo(vfo.id));
                    }
                    ignore_btn.on_hover_text("Add to Blacklist: scanner will ignore and never stop on this frequency");

                    // 4. [ 🔊 ] MUTE TOGGLE (Enlarged)
                    let mute_icon = if vfo.muted || is_history { "🔇" } else { "🔊" };
                    let mute_col = if vfo.muted || is_history {
                        Color32::from_rgb(70, 75, 80)
                    } else {
                        Color32::from_rgb(50, 180, 80)
                    };
                    let mute_btn = ui.add_sized(
                        [38.0, 30.0],
                        egui::Button::new(
                            RichText::new(mute_icon)
                                .color(Color32::WHITE)
                                .size(14.0),
                        )
                        .fill(mute_col),
                    );
                    if mute_btn.clicked() {
                        let _ = cmd_tx.send(DspCommand::SetVfoMute(vfo.id, !vfo.muted));
                    }

                    // 5. SNR BADGE (Fixed width: zero layout shift)
                    let snr_text = format!("{:>+3.0}dB", vfo.snr_db);
                    ui.allocate_ui_with_layout(
                        egui::vec2(52.0, 30.0),
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            ui.label(
                                RichText::new(snr_text)
                                    .color(if is_history {
                                        Color32::from_rgb(90, 100, 110)
                                    } else if vfo.snr_db >= 15.0 {
                                        Color32::from_rgb(50, 255, 100)
                                    } else if vfo.snr_db >= 5.0 {
                                        Color32::from_rgb(180, 240, 100)
                                    } else {
                                        Color32::from_rgb(120, 140, 150)
                                    })
                                    .size(11.5)
                                    .monospace()
                                    .strong(),
                            );
                        },
                    );

                    // 6. BIG BOLD FREQUENCY DISPLAY (Fixed width: zero layout shift, click to select VFO)
                    let mhz = vfo.freq_hz as f64 / 1_000_000.0;
                    let freq_str = format!("{:>8.4} MHz", mhz);
                    ui.allocate_ui_with_layout(
                        egui::vec2(122.0, 30.0),
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            let freq_btn = ui.add(
                                egui::Button::new(
                                    RichText::new(freq_str)
                                        .color(if is_selected {
                                            Color32::from_rgb(0, 255, 200)
                                        } else if is_pinned {
                                            Color32::from_rgb(0, 220, 255)
                                        } else if is_history {
                                            Color32::from_rgb(130, 140, 150)
                                        } else if vfo.active {
                                            Color32::from_rgb(50, 255, 120)
                                        } else {
                                            Color32::from_rgb(140, 160, 175)
                                        })
                                        .size(if is_compact { 14.5 } else { 16.0 })
                                        .monospace()
                                        .strong(),
                                )
                                .fill(if is_selected {
                                    Color32::from_rgb(26, 42, 52)
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .frame(is_selected),
                            );
                            if freq_btn.clicked() {
                                self.selected_vfo_id = Some(vfo.id);
                            }
                            freq_btn.on_hover_text(format!("Click to select VFO #{} for left dial tuning", vfo.id));
                        },
                    );

                    // 7. VFO DEMOD MODE SELECTOR
                    let mut cur_mode = vfo.mode;
                    egui::ComboBox::from_id_salt(format!("vfo_mode_box_{}", vfo.id))
                        .selected_text(cur_mode.name())
                        .width(48.0)
                        .show_ui(ui, |ui| {
                            for mode in [
                                DemodMode::WFM,
                                DemodMode::NFM,
                                DemodMode::AM,
                                DemodMode::BCFM,
                                DemodMode::USB,
                                DemodMode::LSB,
                            ] {
                                if ui.selectable_value(&mut cur_mode, mode, mode.name()).clicked() {
                                    let _ = cmd_tx.send(DspCommand::SetVfoMode(vfo.id, mode));
                                }
                            }
                        });

                    // 8. SQUELCH / VOICE / CARRIER / DELAY TIMER / HISTORY BADGE (Fixed 85px container: zero layout shift)
                    let (vad_txt, vad_col) = if is_history {
                        ("[HISTORY]".to_string(), Color32::from_rgb(130, 138, 148))
                    } else if vfo.delay_remaining_sec > 0.0 {
                        (format!("[HOLD {:.1}s]", vfo.delay_remaining_sec), Color32::from_rgb(255, 215, 60))
                    } else if !vfo.active {
                        ("[SQUELCH]".to_string(), Color32::from_rgb(105, 115, 125))
                    } else if cfg.vad_enabled {
                        if vfo.vad_score >= cfg.vad_threshold {
                            ("[VOICE]".to_string(), Color32::from_rgb(0, 255, 120))
                        } else {
                            ("[CARRIER]".to_string(), Color32::from_rgb(255, 175, 40))
                        }
                    } else {
                        ("[SIGNAL]".to_string(), Color32::from_rgb(0, 220, 255))
                    };
                    ui.allocate_ui_with_layout(
                        egui::vec2(85.0, 30.0),
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            ui.label(RichText::new(vad_txt).color(vad_col).monospace().strong().size(10.5));
                        },
                    );

                    // 9. INLINE EDITABLE COMMENT / TAG FIELD (Immediately after status, protected during edit)
                    let is_being_edited = self.editing_vfo_id == Some(vfo.id);
                    let need_refresh = match self.comment_edits.get(&vfo.id) {
                        Some((cached_f, _)) => *cached_f != vfo.freq_hz && !is_being_edited,
                        None => true,
                    };

                    if need_refresh {
                        let init_text = if let Some(bm) = cfg.get_comment(vfo.freq_hz, 3500) {
                            bm.name.clone()
                        } else if !vfo.name.is_empty() && !vfo.name.starts_with("VFO #") && !vfo.name.contains("MHz") {
                            vfo.name.clone()
                        } else {
                            String::new()
                        };
                        self.comment_edits.insert(vfo.id, (vfo.freq_hz, init_text));
                    }

                    let comment_entry = &mut self.comment_edits.get_mut(&vfo.id).unwrap().1;

                    let edit_width = if is_compact { 95.0 } else { 130.0 };
                    let edit_resp = ui.add(
                        egui::TextEdit::singleline(comment_entry)
                            .hint_text("Tag...")
                            .desired_width(edit_width),
                    );

                    // Track focus for Tag editing: protect this VFO from being recycled/replaced
                    if edit_resp.has_focus() {
                        if self.editing_vfo_id != Some(vfo.id) {
                            self.editing_vfo_id = Some(vfo.id);
                            let _ = cmd_tx.send(DspCommand::SetVfoEditing(Some(vfo.id)));
                        }
                    } else if self.editing_vfo_id == Some(vfo.id) {
                        self.editing_vfo_id = None;
                        let _ = cmd_tx.send(DspCommand::SetVfoEditing(None));
                    }

                    if edit_resp.changed() || (edit_resp.lost_focus() && !comment_entry.trim().is_empty()) {
                        let new_name = comment_entry.trim().to_string();
                        let _ = cmd_tx.send(DspCommand::SetVfoComment(vfo.id, new_name.clone()));
                        if !new_name.is_empty() {
                            let bm = Bookmark {
                                freq_hz: vfo.freq_hz,
                                name: new_name,
                                mode: vfo.mode,
                                bandwidth_hz: 12_500,
                            };
                            cfg.bookmarks.insert(vfo.freq_hz, bm.clone());
                            let _ = cfg.save();
                            let _ = cmd_tx.send(DspCommand::AddBookmark(bm));
                        }
                    }

                    // 10. DMR / TELEMETRY BADGE (Placed after Tag so it never causes layout shifts for Tag)
                    if let Some(dmr) = &vfo.dmr_info {
                        ui.label(
                            RichText::new(format!("[{}]", dmr.badge_text()))
                                .color(Color32::from_rgb(0, 230, 255))
                                .monospace()
                                .strong()
                                .size(11.0),
                        );
                    } else if let Some(telemetry_info) = &vfo.latest_telemetry_info {
                        ui.label(
                            RichText::new(format!("[🛰️ {}]", telemetry_info))
                                .color(Color32::from_rgb(0, 255, 180))
                                .monospace()
                                .strong()
                                .size(10.5),
                        );
                    }

                    // 11. INDIVIDUAL VFO VOLUME SLIDER
                    ui.label(
                        RichText::new("VOL")
                            .size(10.0)
                            .color(if is_history { Color32::from_rgb(90, 100, 110) } else { Color32::from_rgb(140, 170, 200) })
                            .strong(),
                    );
                    let slider_w = if is_compact { 80.0 } else { 120.0 };
                    let mut vol = vfo.volume;
                    let slider_col = if is_history {
                        Color32::from_rgb(100, 120, 140)
                    } else {
                        Color32::from_rgb(0, 220, 255)
                    };
                    if mini_tactical_slider(ui, &mut vol, 0.0..=2.0, egui::vec2(slider_w, 22.0), slider_col) {
                        let _ = cmd_tx.send(DspCommand::SetVfoVolume(vfo.id, vol));
                    }
                    let pct = (vol * 100.0).round() as i32;
                    ui.label(
                        RichText::new(format!("{:>3}%", pct))
                            .size(10.0)
                            .color(if is_history { Color32::from_rgb(100, 110, 120) } else { Color32::from_rgb(140, 190, 225) })
                            .monospace(),
                    );

                    // 12. LIVE AUDIO MODULATION OSCILLOSCOPE
                    mini_audio_oscilloscope(ui, &vfo.audio_waveform, vfo.active && !vfo.muted && vfo.volume > 0.0);
                });
            });
    }
}

/// Compact tactical volume slider matching the left sidebar styling
fn mini_tactical_slider(
    ui: &mut egui::Ui,
    val: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    size: egui::Vec2,
    accent_color: Color32,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let min_val = *range.start();
    let max_val = *range.end();
    let span = (max_val - min_val).max(0.0001);

    let mut changed = false;
    if resp.dragged() || resp.clicked() {
        if let Some(mouse_pos) = resp.interact_pointer_pos() {
            let margin = 5.0;
            let norm = ((mouse_pos.x - (rect.min.x + margin)) / (rect.width() - margin * 2.0)).clamp(0.0, 1.0);
            let new_val = min_val + norm * span;
            if (*val - new_val).abs() > 0.001 {
                *val = new_val;
                changed = true;
            }
        }
    }

    let norm = ((*val - min_val) / span).clamp(0.0, 1.0);
    let painter = ui.painter_at(rect);

    // Track outer groove
    let track_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, rect.min.y + 2.0),
        egui::pos2(rect.max.x, rect.max.y - 2.0),
    );
    painter.rect_filled(track_rect, 3.0, Color32::from_rgb(12, 16, 24));
    painter.rect_stroke(track_rect, 3.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(35, 48, 65)));

    // Active fill bar
    let fill_w = norm * (track_rect.width() - 4.0);
    if fill_w > 0.0 {
        let fill_rect = egui::Rect::from_min_max(
            egui::pos2(track_rect.min.x + 2.0, track_rect.min.y + 2.0),
            egui::pos2(track_rect.min.x + 2.0 + fill_w, track_rect.max.y - 2.0),
        );
        let fill_col = Color32::from_rgb(
            (accent_color.r() as f32 * 0.75) as u8,
            (accent_color.g() as f32 * 0.75) as u8,
            (accent_color.b() as f32 * 0.75) as u8,
        );
        painter.rect_filled(fill_rect, 2.0, fill_col);
    }

    // Draggable thumb/knob
    let knob_margin = 6.0;
    let knob_x = rect.min.x + knob_margin + norm * (rect.width() - knob_margin * 2.0);
    let knob_rect = egui::Rect::from_center_size(
        egui::pos2(knob_x, rect.center().y),
        egui::vec2(10.0, 14.0),
    );
    let knob_border = if resp.dragged() || resp.hovered() {
        Color32::WHITE
    } else {
        accent_color
    };
    painter.rect_filled(knob_rect, 2.0, Color32::from_rgb(28, 36, 48));
    painter.rect_stroke(knob_rect, 2.0, egui::Stroke::new(1.2_f32, knob_border));

    // Grip line
    painter.line_segment(
        [egui::pos2(knob_x, knob_rect.min.y + 3.0), egui::pos2(knob_x, knob_rect.max.y - 3.0)],
        egui::Stroke::new(1.0_f32, knob_border),
    );

    changed
}

/// Compact tactical audio oscilloscope displaying live speech/demodulated waveform
fn mini_audio_oscilloscope(ui: &mut egui::Ui, waveform: &[f32; 32], is_speaking: bool) {
    let size = egui::vec2(85.0, 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Dark tactical background with subtle border
    painter.rect_filled(rect, 3.0, Color32::from_rgb(10, 14, 20));
    painter.rect_stroke(rect, 3.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(28, 40, 56)));

    let cy = rect.center().y;
    // Center zero reference line
    painter.line_segment(
        [egui::pos2(rect.min.x + 2.0, cy), egui::pos2(rect.max.x - 2.0, cy)],
        egui::Stroke::new(0.5_f32, Color32::from_rgb(25, 36, 48)),
    );

    let n = waveform.len();
    if n >= 2 {
        let step_x = (rect.width() - 4.0) / (n - 1) as f32;
        let half_h = (rect.height() * 0.5) - 3.0;
        let mut points = Vec::with_capacity(n);

        for (i, &sample) in waveform.iter().enumerate() {
            let px = rect.min.x + 2.0 + (i as f32 * step_x);
            let py = (cy - (sample * half_h)).clamp(rect.min.y + 2.0, rect.max.y - 2.0);
            points.push(egui::pos2(px, py));
        }

        let line_color = if is_speaking {
            Color32::from_rgb(0, 255, 140) // Glowing vibrant green during active voice
        } else {
            Color32::from_rgb(45, 65, 80) // Dim idle flat line
        };

        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(if is_speaking { 1.5_f32 } else { 1.0_f32 }, line_color),
        ));
    }

    resp.on_hover_text("Live Audio Modulation Oscilloscope");
}
