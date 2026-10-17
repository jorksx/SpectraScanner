use crate::config::DemodMode;
use crate::dsp::DmrPacket;
use crate::dsp_worker::{DisplaySnapshot, DspCommand};
use crate::scanner::ScannerState;
use crossbeam_channel::Sender;
use eframe::egui::{self, Color32, RichText, Stroke};

/// Dedicated UI popup showing active DMR talkers, talkgroups, and frequencies
pub struct DmrView {
    pub is_open: bool,
    pub filter: String,
    pub selected_idx: Option<usize>,
    pub auto_scroll: bool,
    pub filter_active_only: bool,
    pub cleared_count: usize,
}

impl DmrView {
    pub fn new() -> Self {
        Self {
            is_open: false,
            filter: String::new(),
            selected_idx: None,
            auto_scroll: true,
            filter_active_only: false,
            cleared_count: 0,
        }
    }

    /// Renders the DMR Traffic & Call Log window
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        snap: &DisplaySnapshot,
        cmd_tx: &Sender<DspCommand>,
        scanner_state: &mut ScannerState,
    ) {
        if !self.is_open {
            return;
        }

        let mut is_open = self.is_open;
        egui::Window::new("📻 DMR CALLS & DIGITAL TRAFFIC")
            .open(&mut is_open)
            .default_size([940.0, 560.0])
            .min_size([550.0, 320.0])
            .max_size([1100.0, 800.0])
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                self.render_content(ui, ctx, snap, cmd_tx, scanner_state);
            });
        self.is_open = is_open;
    }

    fn render_content(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snap: &DisplaySnapshot,
        cmd_tx: &Sender<DspCommand>,
        scanner_state: &mut ScannerState,
    ) {
        let dmr_packets = if self.cleared_count < snap.dmr_packets.len() {
            &snap.dmr_packets[self.cleared_count..]
        } else {
            &[]
        };

        let search_lower = self.filter.trim().to_lowercase();

        let filtered_packets: Vec<(usize, &DmrPacket)> = dmr_packets
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                let is_live = p.timestamp.elapsed().as_secs_f32() < 2.5;
                if self.filter_active_only && !is_live {
                    return false;
                }
                if search_lower.is_empty() {
                    return true;
                }
                let tg_str = p.target_id.map(|t| t.to_string()).unwrap_or_default();
                let src_str = p.source_id.map(|s| s.to_string()).unwrap_or_default();
                let freq_str = format!("{:.4}", p.freq_hz as f64 / 1e6);
                let tg_name = resolve_talkgroup_name(p.target_id).to_lowercase();

                p.summary.to_lowercase().contains(&search_lower)
                    || tg_str.contains(&search_lower)
                    || src_str.contains(&search_lower)
                    || freq_str.contains(&search_lower)
                    || tg_name.contains(&search_lower)
            })
            .collect();

        // 1. TOP TOOLBAR
        ui.horizontal(|ui| {
            ui.label(RichText::new("🔍").size(14.0));
            let search_edit = egui::TextEdit::singleline(&mut self.filter)
                .hint_text("Search Talkgroup (TG), Radio ID, or Frequency...")
                .desired_width(240.0);
            ui.add(search_edit);

            if !self.filter.is_empty() && ui.small_button("✖").clicked() {
                self.filter.clear();
            }

            ui.separator();

            // Quick Filters
            let active_btn = ui.add(
                egui::Button::new(
                    RichText::new("🟢 Active Voice Only")
                        .size(11.0)
                        .color(if self.filter_active_only { Color32::BLACK } else { Color32::from_rgb(0, 255, 128) }),
                )
                .fill(if self.filter_active_only { Color32::from_rgb(0, 255, 128) } else { Color32::from_rgb(18, 38, 28) }),
            );
            if active_btn.clicked() {
                self.filter_active_only = !self.filter_active_only;
            }

            ui.separator();
            ui.checkbox(&mut self.auto_scroll, "Auto-scroll");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(RichText::new("🗑️ Clear").size(11.0).color(Color32::from_rgb(255, 120, 120))).clicked() {
                    self.cleared_count = snap.dmr_packets.len();
                    self.selected_idx = None;
                }

                if ui.button(RichText::new("📋 Copy All").size(11.0).color(Color32::from_rgb(100, 200, 255))).clicked() {
                    let mut export = String::new();
                    export.push_str("TIME\tFREQUENCY (MHz)\tTIMESLOT\tCOLOR CODE\tTALKGROUP\tTALKER ID\tCALL TYPE\tSUMMARY\n");
                    for (_, p) in &filtered_packets {
                        export.push_str(&format!(
                            "{}\t{:.4}\tTS {}\tCC {}\t{}\t{}\t{}\t{}\n",
                            p.time_str,
                            p.freq_hz as f64 / 1e6,
                            p.timeslot,
                            p.color_code,
                            p.target_id.map(|t| format!("TG {}", t)).unwrap_or("-".to_string()),
                            p.source_id.map(|s| format!("ID {}", s)).unwrap_or("-".to_string()),
                            p.call_type,
                            p.summary
                        ));
                    }
                    ctx.copy_text(export);
                }

                ui.label(
                    RichText::new(format!("{}/{} calls", filtered_packets.len(), snap.dmr_packets.len()))
                        .size(11.0)
                        .color(Color32::GRAY),
                );
            });
        });

        ui.add_space(3.0);
        ui.separator();

        // 2. FIXED-BUDGET SPLIT: TABLE (UPPER) & DETAIL INSPECTOR (LOWER)
        let inspector_h = 135.0_f32;
        let avail_h = ui.available_height();
        let table_h = (avail_h - inspector_h).max(85.0_f32);

        // --- CALLS TABLE ---
        egui::Frame::none()
            .fill(Color32::from_rgb(13, 20, 30))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(26, 38, 54)))
            .inner_margin(4.0)
            .show(ui, |ui| {
                // Table Header
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(egui::vec2(95.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("STATUS / TIME").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.allocate_ui_with_layout(egui::vec2(85.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("FREQ (MHz)").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.allocate_ui_with_layout(egui::vec2(75.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("SLOT / CC").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.allocate_ui_with_layout(egui::vec2(185.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("TALKGROUP (\"WHERE\")").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.allocate_ui_with_layout(egui::vec2(150.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("TALKER ID (\"WHO\")").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.allocate_ui_with_layout(egui::vec2(120.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("CALL TYPE").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.label(RichText::new("ACTION").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                });

                ui.separator();

                let mut scroll_area = egui::ScrollArea::vertical()
                    .max_height(table_h)
                    .min_scrolled_height(table_h)
                    .auto_shrink([false, false]);

                if self.auto_scroll {
                    scroll_area = scroll_area.stick_to_bottom(true);
                }

                scroll_area.show(ui, |ui| {
                    if filtered_packets.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(25.0);
                            ui.label(RichText::new("No active or logged DMR calls matching filter.").color(Color32::GRAY));
                            ui.label(RichText::new("Tune to 430-440 MHz DMR repeaters or PMR446 digital channels in NFM mode.")
                                .color(Color32::from_rgb(100, 150, 190)).size(11.0));
                            ui.add_space(25.0);
                        });
                        return;
                    }

                    for &(orig_idx, pkt) in &filtered_packets {
                        let is_selected = self.selected_idx == Some(orig_idx);
                        let is_live = pkt.timestamp.elapsed().as_secs_f32() < 2.5;

                        let row_bg = if is_selected {
                            Color32::from_rgb(26, 52, 80)
                        } else if is_live {
                            Color32::from_rgb(12, 38, 22)
                        } else {
                            Color32::TRANSPARENT
                        };

                        egui::Frame::none()
                            .fill(row_bg)
                            .inner_margin(egui::Margin::symmetric(2.0, 2.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // 1. Status / Time
                                    ui.allocate_ui_with_layout(egui::vec2(95.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        if is_live {
                                            ui.label(RichText::new("● LIVE VOICE").size(10.5).color(Color32::from_rgb(0, 255, 128)).strong());
                                        } else {
                                            ui.label(RichText::new(&pkt.time_str).size(10.5).color(Color32::from_rgb(180, 190, 200)).monospace());
                                        }
                                    });

                                    // 2. Frequency
                                    ui.allocate_ui_with_layout(egui::vec2(85.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let f_str = if pkt.freq_hz > 0 {
                                            format!("{:.4}", pkt.freq_hz as f64 / 1e6)
                                        } else {
                                            "Active VFO".to_string()
                                        };
                                        let f_btn = ui.selectable_label(is_selected, RichText::new(f_str).size(11.5).color(Color32::from_rgb(255, 230, 100)).monospace().strong());
                                        if f_btn.clicked() {
                                            self.selected_idx = Some(orig_idx);
                                        }
                                    });

                                    // 3. Slot / Color Code
                                    ui.allocate_ui_with_layout(egui::vec2(75.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.label(
                                            RichText::new(format!("TS {} | CC {}", pkt.timeslot, pkt.color_code))
                                                .size(10.5)
                                                .color(Color32::from_rgb(140, 200, 255))
                                                .monospace(),
                                        );
                                    });

                                    // 4. Talkgroup ("Where")
                                    ui.allocate_ui_with_layout(egui::vec2(185.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let tg_title = resolve_talkgroup_name(pkt.target_id);
                                        let tg_btn = ui.selectable_label(is_selected, RichText::new(tg_title).size(11.0).color(Color32::from_rgb(0, 230, 255)).strong());
                                        if tg_btn.clicked() {
                                            self.selected_idx = Some(orig_idx);
                                        }
                                    });

                                    // 5. Talker / Radio ID ("Who")
                                    ui.allocate_ui_with_layout(egui::vec2(150.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let talker_str = if let Some(src) = pkt.source_id {
                                            let prefix_note = if src.to_string().starts_with("244") { " [FI]" } else { "" };
                                            format!("ID: {}{}", src, prefix_note)
                                        } else {
                                            "-".to_string()
                                        };
                                        let id_lbl = ui.selectable_label(is_selected, RichText::new(talker_str).size(11.0).color(Color32::from_rgb(255, 220, 130)).monospace().strong());
                                        if id_lbl.clicked() {
                                            self.selected_idx = Some(orig_idx);
                                        }
                                    });

                                    // 6. Call Type
                                    ui.allocate_ui_with_layout(egui::vec2(120.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let type_col = if pkt.call_type.contains("Group") {
                                            Color32::from_rgb(180, 240, 200)
                                        } else if pkt.call_type.contains("Private") {
                                            Color32::from_rgb(255, 180, 220)
                                        } else {
                                            Color32::from_rgb(200, 200, 200)
                                        };
                                        ui.label(RichText::new(&pkt.call_type).size(10.5).color(type_col));
                                    });

                                    // 7. Tune Action
                                    let tune_btn = ui.add(
                                        egui::Button::new(RichText::new("🎯 Tune").size(10.5).color(Color32::from_rgb(0, 230, 255)).strong())
                                            .fill(Color32::from_rgb(20, 40, 60)),
                                    );
                                    if tune_btn.clicked() && pkt.freq_hz > 0 {
                                        *scanner_state = ScannerState::Paused;
                                        let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                                        let comment = format!("DMR TG:{} ID:{}", pkt.target_id.unwrap_or(0), pkt.source_id.unwrap_or(0));
                                        let _ = cmd_tx.send(DspCommand::SetManualVfo(0, pkt.freq_hz, DemodMode::NFM, comment));
                                    }
                                    tune_btn.on_hover_text("Pause scanner and tune VFO 1 to this DMR frequency");
                                });
                            });
                    }
                });
            });

        ui.add_space(3.0);

        // 3. BOTTOM DETAIL INSPECTOR
        let selected_p = self
            .selected_idx
            .and_then(|idx| dmr_packets.get(idx))
            .or_else(|| dmr_packets.last());

        if let Some(pkt) = selected_p {
            egui::Frame::none()
                .fill(Color32::from_rgb(16, 25, 38))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(36, 56, 82)))
                .inner_margin(6.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(inspector_h - 12.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let f_str = if pkt.freq_hz > 0 {
                                    format!("{:.4} MHz", pkt.freq_hz as f64 / 1e6)
                                } else {
                                    "Active Channel".to_string()
                                };
                                ui.label(
                                    RichText::new(f_str)
                                        .size(15.0)
                                        .color(Color32::from_rgb(255, 230, 100))
                                        .monospace()
                                        .strong(),
                                );

                                ui.label(
                                    RichText::new(format!(" [Timeslot {} | Color Code {}] ", pkt.timeslot, pkt.color_code))
                                        .size(12.0)
                                        .color(Color32::from_rgb(0, 220, 255))
                                        .strong(),
                                );

                                ui.label(RichText::new(&pkt.time_str).size(11.0).color(Color32::GRAY).monospace());

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if pkt.freq_hz > 0 {
                                        let tune_btn = ui.add(
                                            egui::Button::new(
                                                RichText::new("🎯 Tune VFO to this DMR Frequency")
                                                    .size(11.0)
                                                    .color(Color32::BLACK)
                                                    .strong(),
                                            )
                                            .fill(Color32::from_rgb(0, 220, 255)),
                                        );
                                        if tune_btn.clicked() {
                                            *scanner_state = ScannerState::Paused;
                                            let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                                            let comment = format!("DMR TG:{} ID:{}", pkt.target_id.unwrap_or(0), pkt.source_id.unwrap_or(0));
                                            let _ = cmd_tx.send(DspCommand::SetManualVfo(0, pkt.freq_hz, DemodMode::NFM, comment));
                                        }
                                    }
                                });
                            });

                            ui.add_space(2.0);

                            // Talkgroup and Talker Breakdown
                            egui::Frame::none()
                                .fill(Color32::from_rgb(10, 18, 28))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(20, 60, 90)))
                                .inner_margin(6.0)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("📻 WHERE:").color(Color32::from_rgb(0, 230, 255)).strong().size(11.0));
                                        ui.label(RichText::new(resolve_talkgroup_name(pkt.target_id)).color(Color32::WHITE).strong().size(11.5));

                                        ui.separator();

                                        ui.label(RichText::new("👤 WHO:").color(Color32::from_rgb(255, 220, 120)).strong().size(11.0));
                                        let talker_detail = if let Some(src) = pkt.source_id {
                                            let is_fi = src.to_string().starts_with("244");
                                            format!("Radio ID: {} ({})", src, if is_fi { "Finland Amateur Radio" } else { "Global ID" })
                                        } else {
                                            "Broadcast / System".to_string()
                                        };
                                        ui.label(RichText::new(talker_detail).color(Color32::from_rgb(220, 240, 255)).size(11.5));
                                    });

                                    ui.add_space(2.0);
                                    ui.label(
                                        RichText::new(format!(
                                            "Summary: {} | Sync: {} | Audio: AMBE+2 Vocoder Synthesized Output",
                                            pkt.summary,
                                            pkt.sync_type.name()
                                        ))
                                        .size(10.5)
                                        .color(Color32::from_rgb(160, 190, 220)),
                                    );
                                });
                        });
                });
        }
    }
}

/// Resolves DMR talkgroup ID numbers to human-readable names (Finland & Worldwide standards)
fn resolve_talkgroup_name(tg_opt: Option<u32>) -> String {
    let tg = match tg_opt {
        Some(t) => t,
        None => return "All Call / Broadcast".to_string(),
    };

    match tg {
        91 => "TG 91 (Worldwide English)".to_string(),
        92 => "TG 92 (Europe)".to_string(),
        244 => "TG 244 (Finland Nationwide)".to_string(),
        2441 => "TG 2441 (Uusimaa)".to_string(),
        2442 => "TG 2442 (Varsinais-Suomi)".to_string(),
        2443 => "TG 2443 (Häme / Pirkanmaa)".to_string(),
        2444 => "TG 2444 (Kaakkois-Suomi)".to_string(),
        2445 => "TG 2445 (Keski-Suomi)".to_string(),
        2446 => "TG 2446 (Pohjanmaa)".to_string(),
        2447 => "TG 2447 (Pohjois-Karjala)".to_string(),
        2448 => "TG 2448 (Pohjois-Savo)".to_string(),
        2449 => "TG 2449 (Lappi)".to_string(),
        24499 => "TG 24499 (Echo / Parrot Test)".to_string(),
        9990 => "TG 9990 (Audio Parrot)".to_string(),
        other => format!("TG {} (Talkgroup)", other),
    }
}
