use crate::config::DemodMode;
use crate::dsp::TelemetryPacket;
use crate::dsp_worker::{DisplaySnapshot, DspCommand};
use crate::scanner::ScannerState;
use crossbeam_channel::Sender;
use eframe::egui::{self, Color32, RichText, Stroke};

/// Dedicated UI Inspector for real-time SCADA, telemetry and radio modem bursts
pub struct TelemetryView {
    pub is_open: bool,
    pub filter: String,
    pub selected_id: Option<u64>,
    pub auto_scroll: bool,
    pub cleared_before_id: u64,
    pub protocol_filter: Option<String>,
}

impl TelemetryView {
    pub fn new() -> Self {
        Self {
            is_open: false,
            filter: String::new(),
            selected_id: None,
            auto_scroll: true,
            cleared_before_id: 0,
            protocol_filter: None,
        }
    }

    /// Renders the Telemetry & Radio Modem Inspector window
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
        egui::Window::new("🛰️ SCADA & RADIO MODEM INSPECTOR")
            .open(&mut is_open)
            .default_size([940.0, 560.0])
            .min_size([540.0, 320.0])
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
        // Filter packets by cleared ID and user search filters
        let search_lower = self.filter.trim().to_lowercase();
        let filtered_packets: Vec<&TelemetryPacket> = snap
            .telemetry_packets
            .iter()
            .filter(|p| p.id > self.cleared_before_id)
            .filter(|p| {
                if let Some(proto) = &self.protocol_filter {
                    if !p.protocol.to_lowercase().contains(&proto.to_lowercase()) {
                        return false;
                    }
                }
                if search_lower.is_empty() {
                    return true;
                }
                let f_str = format!("{:.4}", p.freq_hz as f64 / 1e6);
                p.protocol.to_lowercase().contains(&search_lower)
                    || f_str.contains(&search_lower)
                    || p.timestamp.to_lowercase().contains(&search_lower)
                    || p.summary.to_lowercase().contains(&search_lower)
                    || p.human_decoded.to_lowercase().contains(&search_lower)
                    || p.station_id.as_deref().unwrap_or("").to_lowercase().contains(&search_lower)
                    || p.raw_hex.to_lowercase().contains(&search_lower)
                    || p.raw_ascii.to_lowercase().contains(&search_lower)
            })
            .collect();

        // 1. TOP TOOLBAR
        ui.horizontal(|ui| {
            ui.label(RichText::new("🔍").size(14.0));
            let search_edit = egui::TextEdit::singleline(&mut self.filter)
                .hint_text("Search frequency, protocol, station, or text...")
                .desired_width(220.0);
            ui.add(search_edit);

            if !self.filter.is_empty() && ui.small_button("✖").clicked() {
                self.filter.clear();
            }

            ui.separator();

            // Quick protocol filter chips
            let filter_btns = [
                ("All", None),
                ("TPMS", Some("TPMS")),
                ("Weather", Some("Weather")),
                ("EV1527", Some("EV1527")),
                ("SATEL", Some("SATEL")),
                ("Modbus", Some("Modbus")),
                ("M-Bus", Some("M-Bus")),
                ("POCSAG", Some("POCSAG")),
                ("Bursts", Some("Burst")),
            ];

            for (lbl, p_val) in filter_btns {
                let is_selected = self.protocol_filter.as_deref() == p_val;
                let btn = ui.add(
                    egui::Button::new(
                        RichText::new(lbl)
                            .size(11.0)
                            .color(if is_selected { Color32::BLACK } else { Color32::from_rgb(180, 200, 220) })
                            .strong(),
                    )
                    .fill(if is_selected { Color32::from_rgb(0, 220, 255) } else { Color32::from_rgb(26, 38, 54) }),
                );
                if btn.clicked() {
                    self.protocol_filter = p_val.map(|s| s.to_string());
                }
            }

            ui.separator();

            // Auto-scroll toggle
            ui.checkbox(&mut self.auto_scroll, "Auto-scroll");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Clear button
                if ui.button(RichText::new("🗑️ Clear").size(11.0).color(Color32::from_rgb(255, 120, 120))).clicked() {
                    if let Some(last) = snap.telemetry_packets.last() {
                        self.cleared_before_id = last.id;
                    }
                    self.selected_id = None;
                }

                // Copy all button
                if ui.button(RichText::new("📋 Copy All").size(11.0).color(Color32::from_rgb(100, 200, 255))).clicked() {
                    let mut export = String::new();
                    export.push_str("TIME\tFREQUENCY (MHz)\tPROTOCOL\tSTATION/ID\tTELEMETRY ACTION\tSNR\tBURST (ms)\tDECODED INTERPRETATION\n");
                    for p in &filtered_packets {
                        export.push_str(&format!(
                            "{}\t{:.4}\t{}\t{}\t{}\t{:.1} dB\t{:.0} ms\t{}\n",
                            p.timestamp,
                            p.freq_hz as f64 / 1e6,
                            p.protocol,
                            p.station_id.as_deref().unwrap_or("-"),
                            p.summary,
                            p.snr_db,
                            p.burst_ms,
                            p.human_decoded.replace('\n', " ")
                        ));
                    }
                    ctx.copy_text(export);
                }

                ui.label(
                    RichText::new(format!("{}/{} pkts", filtered_packets.len(), snap.telemetry_packets.len()))
                        .size(11.0)
                        .color(Color32::GRAY),
                );
            });
        });

        ui.add_space(3.0);

        // QUICK FREQUENCY TUNING & SCANNING PROFILES
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("🎯 Quick Bands:").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());

            // 403.00 MHz Sonde Center
            let btn_sonde = ui.add(
                egui::Button::new(RichText::new("403.00 MHz (Sondes)").size(11.0).color(Color32::from_rgb(120, 255, 180)).strong())
                    .fill(Color32::from_rgb(15, 45, 30))
            );
            if btn_sonde.clicked() {
                *scanner_state = ScannerState::Paused;
                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                let _ = cmd_tx.send(DspCommand::SetCenterFreq(403_000_000));
                let _ = cmd_tx.send(DspCommand::TuneDataVfo(0, 403_000_000, DemodMode::NFM, "403.00 Met Sonde".to_string()));
            }
            btn_sonde.on_hover_text("Pause scanner and tune to 403.000 MHz (Dedicated data VFO, open squelch, muted audio)");

            // 411.00 MHz SCADA / Water
            let btn_411 = ui.add(
                egui::Button::new(RichText::new("411.00 MHz (SCADA)").size(11.0).color(Color32::from_rgb(100, 200, 255)).strong())
                    .fill(Color32::from_rgb(15, 35, 55))
            );
            if btn_411.clicked() {
                *scanner_state = ScannerState::Paused;
                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                let _ = cmd_tx.send(DspCommand::SetCenterFreq(411_000_000));
                let _ = cmd_tx.send(DspCommand::TuneDataVfo(0, 411_000_000, DemodMode::NFM, "411.00 SCADA Telemetry".to_string()));
            }
            btn_411.on_hover_text("Pause scanner and tune to 411.000 MHz (Dedicated data VFO, open squelch, muted audio)");

            // 433.92 ISM
            let btn_433 = ui.add(
                egui::Button::new(RichText::new("433.92 MHz (ISM / TPMS)").size(11.0).color(Color32::from_rgb(255, 220, 100)).strong())
                    .fill(Color32::from_rgb(30, 40, 20))
            );
            if btn_433.clicked() {
                *scanner_state = ScannerState::Paused;
                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                let _ = cmd_tx.send(DspCommand::SetCenterFreq(433_920_000));
                let _ = cmd_tx.send(DspCommand::TuneDataVfo(0, 433_920_000, DemodMode::NFM, "433.92 ISM / TPMS".to_string()));
            }
            btn_433.on_hover_text("Pause scanner and tune to 433.920 MHz (Dedicated data VFO, open squelch, muted audio)");

            // 442.50 SCADA / SATEL
            let btn_442 = ui.add(
                egui::Button::new(RichText::new("442.50 MHz (SATEL)").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong())
                    .fill(Color32::from_rgb(20, 40, 50))
            );
            if btn_442.clicked() {
                *scanner_state = ScannerState::Paused;
                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                let _ = cmd_tx.send(DspCommand::SetCenterFreq(442_500_000));
                let _ = cmd_tx.send(DspCommand::TuneDataVfo(0, 442_500_000, DemodMode::NFM, "442.50 SCADA Modem".to_string()));
            }
            btn_442.on_hover_text("Pause scanner and tune to 442.500 MHz (Dedicated data VFO, open squelch, muted audio)");

            // PMR446 (Voice: normal squelch and audio)
            let btn_pmr = ui.add(
                egui::Button::new(RichText::new("446.00 MHz (PMR)").size(11.0).color(Color32::from_rgb(255, 160, 100)).strong())
                    .fill(Color32::from_rgb(45, 25, 15))
            );
            if btn_pmr.clicked() {
                *scanner_state = ScannerState::Paused;
                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                let _ = cmd_tx.send(DspCommand::SetCenterFreq(446_000_000));
                let _ = cmd_tx.send(DspCommand::SetManualVfo(0, 446_006_250, DemodMode::NFM, "PMR446 Ch 1".to_string()));
            }
            btn_pmr.on_hover_text("Pause scanner and tune to PMR446 Channel 1 (446.00625 MHz)");

            // 868.30 SRD
            let btn_868 = ui.add(
                egui::Button::new(RichText::new("868.30 MHz (SRD)").size(11.0).color(Color32::from_rgb(180, 200, 255)).strong())
                    .fill(Color32::from_rgb(25, 25, 45))
            );
            if btn_868.clicked() {
                *scanner_state = ScannerState::Paused;
                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                let _ = cmd_tx.send(DspCommand::SetCenterFreq(868_300_000));
                let _ = cmd_tx.send(DspCommand::TuneDataVfo(0, 868_300_000, DemodMode::NFM, "868.30 SRD / Weather".to_string()));
            }
            btn_868.on_hover_text("Pause scanner and tune to 868.300 MHz (Dedicated data VFO, open squelch, muted audio)");

            ui.separator();
            ui.label(RichText::new("📡 Sweep Band:").size(11.0).color(Color32::from_rgb(255, 215, 0)).strong());

            // Scan 400-406 Sondes
            let scan_sonde = ui.add(
                egui::Button::new(RichText::new("▶ Scan 400-406M (Sondes)").size(11.0).color(Color32::BLACK).strong())
                    .fill(Color32::from_rgb(120, 255, 180))
            );
            if scan_sonde.clicked() {
                let _ = cmd_tx.send(DspCommand::SetScanProfile {
                    name: "400-406M Radiosondes".to_string(),
                    center_freq_hz: 403_000_000,
                    scan_start_hz: 400_000_000,
                    scan_end_hz: 406_000_000,
                    scan_step_hz: 25_000,
                    mode: DemodMode::NFM,
                });
                *scanner_state = ScannerState::Scanning;
                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Scanning));
            }
            scan_sonde.on_hover_text("Start scanner sweeping the entire 400.000 - 406.000 MHz radiosonde band");

            // Scan 406-430 SCADA (including 411 MHz)
            let scan_scada_411 = ui.add(
                egui::Button::new(RichText::new("▶ Scan 406-430M (SCADA)").size(11.0).color(Color32::BLACK).strong())
                    .fill(Color32::from_rgb(100, 200, 255))
            );
            if scan_scada_411.clicked() {
                let _ = cmd_tx.send(DspCommand::SetScanProfile {
                    name: "406-430M SCADA Telemetry".to_string(),
                    center_freq_hz: 411_000_000,
                    scan_start_hz: 406_100_000,
                    scan_end_hz: 430_000_000,
                    scan_step_hz: 12_500,
                    mode: DemodMode::NFM,
                });
                *scanner_state = ScannerState::Scanning;
                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Scanning));
            }
            scan_scada_411.on_hover_text("Start scanner sweeping 406.100 - 430.000 MHz municipal SCADA & water telemetry band");

            // Scan 433 ISM
            let scan_433 = ui.add(
                egui::Button::new(RichText::new("▶ Scan 433M ISM (TPMS)").size(11.0).color(Color32::BLACK).strong())
                    .fill(Color32::from_rgb(255, 210, 0))
            );
            if scan_433.clicked() {
                let _ = cmd_tx.send(DspCommand::SetScanProfile {
                    name: "433 MHz ISM Band".to_string(),
                    center_freq_hz: 433_920_000,
                    scan_start_hz: 433_050_000,
                    scan_end_hz: 434_790_000,
                    scan_step_hz: 25_000,
                    mode: DemodMode::NFM,
                });
                *scanner_state = ScannerState::Scanning;
                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Scanning));
            }
            scan_433.on_hover_text("Start scanner sweeping the entire 433.05 - 434.79 MHz ISM band (RTL-433 & TPMS)");

            // Scan SCADA 440-445
            let scan_scada = ui.add(
                egui::Button::new(RichText::new("▶ Scan 440-445M (SATEL)").size(11.0).color(Color32::BLACK).strong())
                    .fill(Color32::from_rgb(0, 220, 255))
            );
            if scan_scada.clicked() {
                let _ = cmd_tx.send(DspCommand::SetScanProfile {
                    name: "SCADA / Telemetry (440-445M)".to_string(),
                    center_freq_hz: 442_500_000,
                    scan_start_hz: 440_000_000,
                    scan_end_hz: 445_000_000,
                    scan_step_hz: 12_500,
                    mode: DemodMode::NFM,
                });
                *scanner_state = ScannerState::Scanning;
                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Scanning));
            }
            scan_scada.on_hover_text("Start scanner sweeping 440.000 - 445.000 MHz SATEL 3AS radio modem band");

            // Scan PMR446
            let scan_pmr = ui.add(
                egui::Button::new(RichText::new("▶ Scan PMR446").size(11.0).color(Color32::BLACK).strong())
                    .fill(Color32::from_rgb(255, 160, 100))
            );
            if scan_pmr.clicked() {
                let _ = cmd_tx.send(DspCommand::SetScanProfile {
                    name: "PMR446 Channels 1-16".to_string(),
                    center_freq_hz: 446_100_000,
                    scan_start_hz: 446_000_000,
                    scan_end_hz: 446_200_000,
                    scan_step_hz: 12_500,
                    mode: DemodMode::NFM,
                });
                *scanner_state = ScannerState::Scanning;
                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Scanning));
            }
            scan_pmr.on_hover_text("Start scanner sweeping 446.000 - 446.200 MHz PMR446 channels");
        });

        ui.add_space(3.0);
        ui.separator();

        // 2. FIXED-BUDGET SPLIT: TABLE (UPPER) & DETAIL INSPECTOR (LOWER)
        // Fixed inspector height prevents dynamic feedback loops that cause window expansion
        let inspector_h = 175.0_f32;
        let avail_h = ui.available_height();
        let table_h = (avail_h - inspector_h).max(85.0_f32);

        // --- PACKET TABLE ---
        egui::Frame::none()
            .fill(Color32::from_rgb(13, 20, 30))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(26, 38, 54)))
            .inner_margin(4.0)
            .show(ui, |ui| {
                // Table Header
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(egui::vec2(60.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("TIME").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.allocate_ui_with_layout(egui::vec2(85.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("FREQ (MHz)").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.allocate_ui_with_layout(egui::vec2(130.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("PROTOCOL").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.allocate_ui_with_layout(egui::vec2(115.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("STATION / ID").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.allocate_ui_with_layout(egui::vec2(230.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("TELEMETRY / ACTION").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.allocate_ui_with_layout(egui::vec2(85.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("SIGNAL").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.allocate_ui_with_layout(egui::vec2(65.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(RichText::new("STATUS").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    });
                    ui.label(RichText::new("ACTION").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                });

                ui.separator();

                // Table Rows Scroll Area
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
                            ui.label(RichText::new("No decoded telemetry packets matching current filter.").color(Color32::GRAY));
                            ui.label(RichText::new("Tune receiver to 440-445 MHz SCADA band or select 'Telemetria / SCADA (440-445M)'.")
                                .color(Color32::from_rgb(100, 150, 190)).size(11.0));
                            ui.add_space(25.0);
                        });
                        return;
                    }

                    for pkt in &filtered_packets {
                        let is_selected = self.selected_id == Some(pkt.id);
                        let row_bg = if is_selected {
                            Color32::from_rgb(26, 52, 80)
                        } else {
                            Color32::TRANSPARENT
                        };

                        egui::Frame::none()
                            .fill(row_bg)
                            .inner_margin(egui::Margin::symmetric(2.0, 2.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // 1. Time
                                    ui.allocate_ui_with_layout(egui::vec2(60.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(&pkt.timestamp).monospace().size(11.0).color(Color32::from_rgb(180, 200, 220)));
                                    });

                                    // 2. Frequency
                                    ui.allocate_ui_with_layout(egui::vec2(85.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let freq_str = format!("{:.4}", pkt.freq_hz as f64 / 1e6);
                                        let freq_lbl = ui.selectable_label(is_selected, RichText::new(freq_str).monospace().strong().size(11.5).color(Color32::from_rgb(255, 230, 100)));
                                        if freq_lbl.clicked() {
                                            self.selected_id = Some(pkt.id);
                                        }
                                    });

                                    // 3. Protocol badge
                                    ui.allocate_ui_with_layout(egui::vec2(130.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let (badge_col, fill_col) = protocol_colors(&pkt.protocol);
                                        let badge = ui.add(
                                            egui::Button::new(RichText::new(&pkt.protocol).size(10.5).color(badge_col).strong())
                                                .fill(fill_col)
                                                .sense(egui::Sense::click()),
                                        );
                                        if badge.clicked() {
                                            self.selected_id = Some(pkt.id);
                                        }
                                    });

                                    // 4. Station / ID
                                    ui.allocate_ui_with_layout(egui::vec2(115.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let st_str = pkt.station_id.as_deref().unwrap_or("-");
                                        let id_lbl = ui.selectable_label(is_selected, RichText::new(st_str).monospace().size(11.0).color(Color32::from_rgb(200, 220, 255)));
                                        if id_lbl.clicked() {
                                            self.selected_id = Some(pkt.id);
                                        }
                                    });

                                    // 5. Summary / Payload description
                                    ui.allocate_ui_with_layout(egui::vec2(230.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let sum_lbl = ui.selectable_label(is_selected, RichText::new(&pkt.summary).size(11.0).color(Color32::WHITE));
                                        if sum_lbl.clicked() {
                                            self.selected_id = Some(pkt.id);
                                        }
                                    });

                                    // 6. Signal SNR & Burst duration
                                    ui.allocate_ui_with_layout(egui::vec2(85.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        let sig_str = format!("+{:.0}dB/{}ms", pkt.snr_db, pkt.burst_ms.round() as u32);
                                        ui.label(RichText::new(sig_str).monospace().size(10.5).color(Color32::from_rgb(140, 210, 140)));
                                    });

                                    // 7. Status / CRC
                                    ui.allocate_ui_with_layout(egui::vec2(65.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                        if pkt.crc_ok {
                                            ui.label(RichText::new("CRC OK").size(10.0).strong().color(Color32::from_rgb(0, 255, 128)));
                                        } else {
                                            ui.label(RichText::new("RAW").size(10.0).color(Color32::from_rgb(140, 150, 160)));
                                        }
                                    });

                                    // 8. Action: Tune VFO directly to frequency
                                    let tune_btn = ui.add(
                                        egui::Button::new(RichText::new("🎯 Tune").size(10.5).color(Color32::from_rgb(0, 230, 255)).strong())
                                            .fill(Color32::from_rgb(20, 40, 60)),
                                    );
                                    if tune_btn.clicked() {
                                        *scanner_state = ScannerState::Paused;
                                        let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                                        let comment = format!("{} {}", pkt.protocol, pkt.station_id.as_deref().unwrap_or(""));
                                        let _ = cmd_tx.send(DspCommand::TuneDataVfo(0, pkt.freq_hz, DemodMode::NFM, comment));
                                    }
                                    tune_btn.on_hover_text("Pause scanner and tune to this data frequency (Dedicated data VFO, open squelch, muted audio)");
                                });
                            });
                    }
                });
            });

        ui.add_space(3.0);

        // 3. BOTTOM DETAIL INSPECTOR (HUMAN-READABLE INTERPRETATION)
        let selected_pkt = self
            .selected_id
            .and_then(|id| filtered_packets.iter().find(|p| p.id == id).copied())
            .or_else(|| filtered_packets.last().copied());

        if let Some(pkt) = selected_pkt {
            egui::Frame::none()
                .fill(Color32::from_rgb(16, 25, 38))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(36, 56, 82)))
                .inner_margin(6.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(inspector_h - 12.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            // Top row: Frequency, protocol badge, time, SNR, burst, and Tune button
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{:.4} MHz", pkt.freq_hz as f64 / 1e6))
                                        .monospace()
                                        .strong()
                                        .size(15.0)
                                        .color(Color32::from_rgb(255, 230, 100)),
                                );

                                let (badge_col, _) = protocol_colors(&pkt.protocol);
                                ui.label(
                                    RichText::new(format!(" [{}] ", pkt.protocol))
                                        .strong()
                                        .size(12.5)
                                        .color(badge_col),
                                );

                                ui.label(
                                    RichText::new(&pkt.timestamp)
                                        .monospace()
                                        .size(11.5)
                                        .color(Color32::GRAY),
                                );

                                ui.separator();

                                ui.label(
                                    RichText::new(format!("SNR: +{:.1} dB", pkt.snr_db))
                                        .size(11.5)
                                        .color(Color32::from_rgb(100, 255, 150)),
                                );

                                ui.label(
                                    RichText::new(format!("Burst: {:.1} ms", pkt.burst_ms))
                                        .size(11.5)
                                        .color(Color32::from_rgb(180, 200, 220)),
                                );

                                if pkt.crc_ok {
                                    ui.label(
                                        RichText::new("✓ CRC Valid")
                                            .strong()
                                            .size(11.5)
                                            .color(Color32::from_rgb(0, 255, 128)),
                                    );
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let tune_btn = ui.add(
                                        egui::Button::new(
                                            RichText::new("🎯 Tune VFO Here")
                                                .size(11.0)
                                                .color(Color32::BLACK)
                                                .strong(),
                                        )
                                        .fill(Color32::from_rgb(0, 220, 255)),
                                    );
                                    if tune_btn.clicked() {
                                        *scanner_state = ScannerState::Paused;
                                        let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                                        let comment = format!("{} {}", pkt.protocol, pkt.station_id.as_deref().unwrap_or(""));
                                        let _ = cmd_tx.send(DspCommand::TuneDataVfo(0, pkt.freq_hz, DemodMode::NFM, comment));
                                    }
                                });
                            });

                            ui.add_space(2.0);

                            // PROMINENT HUMAN-READABLE DECODED INTERPRETATION BOX
                            egui::Frame::none()
                                .fill(Color32::from_rgb(10, 18, 28))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(20, 60, 90)))
                                .inner_margin(6.0)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("📡 DECODED MEANING / TELEMETRY:")
                                                .color(Color32::from_rgb(0, 230, 255))
                                                .strong()
                                                .size(11.0),
                                        );
                                        if let Some(st) = &pkt.station_id {
                                            ui.label(RichText::new(format!("({})", st)).color(Color32::from_rgb(255, 220, 120)).size(11.0));
                                        }
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.small_button("📋 Copy Text").clicked() {
                                                ctx.copy_text(pkt.human_decoded.clone());
                                            }
                                        });
                                    });

                                    ui.add_space(2.0);
                                    ui.label(
                                        RichText::new(&pkt.human_decoded)
                                            .color(Color32::from_rgb(220, 240, 255))
                                            .size(11.5),
                                    );
                                });

                            ui.add_space(3.0);

                            // RAW HEX & ASCII DATA (FIXED WIDTH TO PREVENT WINDOW STRETCHING)
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("RAW HEX:").size(10.0).color(Color32::GRAY).strong());
                                if ui.small_button("📋 Copy").clicked() {
                                    ctx.copy_text(pkt.raw_hex.clone());
                                }
                                let mut hex_display = pkt.raw_hex.as_str();
                                let hex_box = egui::TextEdit::singleline(&mut hex_display)
                                    .font(egui::TextStyle::Monospace)
                                    .text_color(Color32::from_rgb(255, 210, 120))
                                    .desired_width(260.0);
                                ui.add_enabled(false, hex_box);

                                ui.separator();

                                ui.label(RichText::new("ASCII:").size(10.0).color(Color32::GRAY).strong());
                                let mut asc_display = pkt.raw_ascii.as_str();
                                let asc_box = egui::TextEdit::singleline(&mut asc_display)
                                    .font(egui::TextStyle::Monospace)
                                    .text_color(Color32::from_rgb(180, 220, 255))
                                    .desired_width(180.0);
                                ui.add_enabled(false, asc_box);
                            });
                        });
                });
        }
    }
}

/// Returns distinctive foreground and background badge colors for each protocol
fn protocol_colors(proto: &str) -> (Color32, Color32) {
    let lower = proto.to_lowercase();
    if lower.contains("tpms") {
        (Color32::from_rgb(255, 215, 0), Color32::from_rgb(55, 45, 10)) // Gold
    } else if lower.contains("weather") || lower.contains("nexus") {
        (Color32::from_rgb(50, 230, 255), Color32::from_rgb(10, 45, 55)) // Aqua
    } else if lower.contains("ev1527") || lower.contains("remote") {
        (Color32::from_rgb(210, 130, 255), Color32::from_rgb(45, 15, 60)) // Purple
    } else if lower.contains("satel") {
        (Color32::from_rgb(0, 230, 255), Color32::from_rgb(10, 45, 65))
    } else if lower.contains("modbus") {
        (Color32::from_rgb(255, 175, 50), Color32::from_rgb(55, 35, 10))
    } else if lower.contains("m-bus") || lower.contains("wmbus") {
        (Color32::from_rgb(100, 255, 140), Color32::from_rgb(15, 50, 25))
    } else if lower.contains("pocsag") {
        (Color32::from_rgb(255, 120, 220), Color32::from_rgb(55, 15, 45))
    } else if lower.contains("sonde") || lower.contains("met") {
        (Color32::from_rgb(80, 255, 190), Color32::from_rgb(15, 50, 35))
    } else if lower.contains("mdc") {
        (Color32::from_rgb(255, 150, 70), Color32::from_rgb(55, 25, 10))
    } else if lower.contains("crc ok") {
        (Color32::from_rgb(120, 255, 180), Color32::from_rgb(20, 45, 30))
    } else {
        (Color32::from_rgb(160, 200, 240), Color32::from_rgb(22, 35, 50))
    }
}
