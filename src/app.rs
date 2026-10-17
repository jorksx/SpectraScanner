use crate::config::ScannerConfig;
use crate::dsp_worker::{DisplaySnapshot, DspCommand, DspController};
use crate::gui::{AirView, AisView, AprsView, ControlDeck, DmrView, SpectrumView, TelemetryView, VfoPanel, WaterfallView};
use crate::scanner::vfo::VfoState;
use crate::scanner::ScannerState;
use eframe::egui::{self, Color32, RichText};

pub struct SpectraScannerApp {
    cfg: ScannerConfig,
    dsp: DspController,
    latest_snapshot: DisplaySnapshot,

    waterfall: WaterfallView,
    spectrum: SpectrumView,
    deck: ControlDeck,
    vfo_panel: VfoPanel,

    vfo_rack_height: f32,
    spectrum_height: f32,

    telemetry_view: TelemetryView,
    aprs_view: AprsView,
    dmr_view: DmrView,
    air_view: AirView,
    ais_view: AisView,
    was_telemetry_open: bool,
    was_aprs_open: bool,
    was_dmr_open: bool,
    was_air_open: bool,
    was_ais_open: bool,
    user_qth: (f64, f64),
}

impl SpectraScannerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let cfg = ScannerConfig::load_or_default();
        let dsp = DspController::spawn(cfg.clone());

        let waterfall = WaterfallView::new(1024, 512);
        let spectrum = SpectrumView::new();
        let deck = ControlDeck::new();
        let vfo_panel = VfoPanel::new();

        let fft_size = 2048;
        let latest_snapshot = DisplaySnapshot {
            raw_spectrum: vec![-100.0; fft_size],
            pseudo_spectrum: vec![0.0; fft_size],
            noise_baseline: vec![-100.0; fft_size],
            scan_cursor_hz: cfg.scan_start_hz,
            vfos: Vec::new(),
            center_freq_hz: cfg.center_freq_hz,
            sample_rate_hz: cfg.sample_rate,
            status_message: "Initializing SDR...".to_string(),
            device_running: false,
            active_range: cfg.active_range.clone(),
            latest_aprs: None,
            aprs_packets: Vec::new(),
            latest_dmr: None,
            dmr_packets: Vec::new(),
            latest_telemetry: None,
            telemetry_packets: Vec::new(),
            latest_air: None,
            air_packets: Vec::new(),
            latest_ais: None,
            ais_packets: Vec::new(),
            current_audio_device: cfg.audio_device.clone(),
            blacklist: cfg.blacklist.clone(),
            vdl2_burst_stats: None,
        };

        let (saved_qth, _, _) = crate::gui::aprs_view::load_aprs_data();
        let user_qth = saved_qth.unwrap_or((60.20, 24.90));
        let mut aprs_view = AprsView::new();
        aprs_view.set_qth(user_qth);
        let mut air_view = AirView::new();
        air_view.set_qth(user_qth);
        let mut ais_view = AisView::new();
        ais_view.set_qth(user_qth);
        let _ = dsp.cmd_tx.send(DspCommand::SetQth(user_qth.0, user_qth.1));

        Self {
            cfg,
            dsp,
            latest_snapshot,
            waterfall,
            spectrum,
            deck,
            vfo_panel,
            vfo_rack_height: 140.0,
            spectrum_height: 170.0,
            telemetry_view: TelemetryView::new(),
            aprs_view,
            dmr_view: DmrView::new(),
            air_view,
            ais_view,
            was_telemetry_open: false,
            was_aprs_open: false,
            was_dmr_open: false,
            was_air_open: false,
            was_ais_open: false,
            user_qth,
        }
    }

    /// When any popup window is closed, immediately stops any popup auto-scan
    /// and restores the receiver/scanner to the configured frequency band on the left sidebar.
    fn handle_popup_closed(&mut self) {
        // 1. Any auto-scan or temporary sweep running from the popup is immediately stopped
        self.deck.scanner_state = ScannerState::Paused;
        let _ = self.dsp.cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));

        // 2. Strip any temporary popup profiles from cfg.ranges
        self.cfg.ranges.retain(|r| !crate::config::is_temporary_scan_profile(&r.name));

        // 3. Return to the configured active range defined on the left sidebar
        let active_band = self.cfg.ranges.iter().find(|r| r.name == self.cfg.active_range)
            .or_else(|| self.cfg.ranges.iter().find(|r| r.enabled))
            .or_else(|| self.cfg.ranges.first())
            .cloned();

        if let Some(target) = active_band {
            let usable_bw = ((self.cfg.sample_rate as f64) * 0.85).round() as u64;
            let target_is_wide = (target.scan_end_hz - target.scan_start_hz) > usable_bw;
            let target_center = if target_is_wide {
                target.scan_start_hz + (usable_bw / 2)
            } else {
                target.center_freq_hz
            };

            self.cfg.active_range = target.name.clone();
            self.cfg.center_freq_hz = target_center;
            self.cfg.scan_start_hz = target.scan_start_hz;
            self.cfg.scan_end_hz = target.scan_end_hz;
            self.cfg.scan_step_hz = target.scan_step_hz;

            let _ = self.dsp.cmd_tx.send(DspCommand::RestoreConfiguredBands {
                active_range: target.name.clone(),
            });
        }
    }
}

impl eframe::App for SpectraScannerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();

        // 1. Receive latest display snapshot from background DSP thread
        while let Ok(snap) = self.dsp.display_rx.try_recv() {
            if self.spectrum.show_pseudospectrum {
                self.waterfall.add_line(&snap.pseudo_spectrum, 2.0, 35.0);
            } else {
                self.waterfall.add_line(&snap.raw_spectrum, -110.0, -30.0);
            }
            self.cfg.active_range = snap.active_range.clone();
            if snap.blacklist != self.cfg.blacklist {
                self.cfg.blacklist = snap.blacklist.clone();
            }
            self.latest_snapshot = snap;
        }

        let snap = &self.latest_snapshot;

        // 2. Top bar
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    RichText::new("SPECTRASCANNER")
                        .color(Color32::from_rgb(0, 220, 255))
                        .strong(),
                );
                ui.label(RichText::new("v0.2.0 Tactical SDR Scanner").color(Color32::GRAY));
                ui.separator();
                ui.label(RichText::new(&snap.status_message).color(Color32::from_rgb(255, 200, 100)));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.spectrum.show_pseudospectrum, "Dynamic Noise Floor (Pseudospectrum)");
                    ui.separator();

                    // 1. SCADA & Telemetry Button
                    let telem_count = snap.telemetry_packets.len();
                    let telem_txt = if telem_count > 0 {
                        format!("🛰️ Telemetry ({})", telem_count)
                    } else {
                        "🛰️ Telemetry".to_string()
                    };
                    let is_telem = self.telemetry_view.is_open;
                    let btn_telem = ui.add(
                        egui::Button::new(
                            RichText::new(telem_txt)
                                .color(if is_telem { Color32::BLACK } else { Color32::from_rgb(0, 230, 255) })
                                .size(11.5)
                                .strong(),
                        )
                        .fill(if is_telem { Color32::from_rgb(0, 220, 255) } else { Color32::from_rgb(20, 35, 52) }),
                    );
                    if btn_telem.clicked() {
                        self.telemetry_view.is_open = !self.telemetry_view.is_open;
                    }
                    btn_telem.on_hover_text("Open real-time SCADA & radio modem inspector (SATEL 3AS, Modbus RTU, Wireless M-Bus, POCSAG, FSK)");

                    ui.separator();

                    // 2. DMR Digital Calls Button
                    let dmr_count = snap.dmr_packets.len();
                    let dmr_txt = if dmr_count > 0 {
                        format!("📻 DMR Calls ({})", dmr_count)
                    } else {
                        "📻 DMR Calls".to_string()
                    };
                    let is_dmr = self.dmr_view.is_open;
                    let btn_dmr = ui.add(
                        egui::Button::new(
                            RichText::new(dmr_txt)
                                .color(if is_dmr { Color32::BLACK } else { Color32::from_rgb(0, 255, 180) })
                                .size(11.5)
                                .strong(),
                        )
                        .fill(if is_dmr { Color32::from_rgb(0, 255, 180) } else { Color32::from_rgb(18, 38, 30) }),
                    );
                    if btn_dmr.clicked() {
                        self.dmr_view.is_open = !self.dmr_view.is_open;
                    }
                    btn_dmr.on_hover_text("Open live DMR digital traffic monitor: active talkers, talkgroups, frequencies, and call log");

                    ui.separator();

                    // 3. APRS GPS Tracker & Radar Map Button
                    let aprs_count = snap.aprs_packets.len();
                    let aprs_txt = if aprs_count > 0 {
                        format!("📡 APRS Map ({})", aprs_count)
                    } else {
                        "📡 APRS Map".to_string()
                    };
                    let is_aprs = self.aprs_view.is_open;
                    let btn_aprs = ui.add(
                        egui::Button::new(
                            RichText::new(aprs_txt)
                                .color(if is_aprs { Color32::BLACK } else { Color32::from_rgb(255, 220, 100) })
                                .size(11.5)
                                .strong(),
                        )
                        .fill(if is_aprs { Color32::from_rgb(255, 220, 100) } else { Color32::from_rgb(42, 38, 18) }),
                    );
                    if btn_aprs.clicked() {
                        self.aprs_view.is_open = !self.aprs_view.is_open;
                    }
                    btn_aprs.on_hover_text("Open APRS 144.800 MHz station monitor and 2D tactical GPS coordinate radar map");

                    ui.separator();

                    // 4. AIR Traffic Radar & ACARS/VDL2/ADS-B Button
                    let air_count = self.air_view.saved_aircraft.len();
                    let air_txt = if air_count > 0 {
                        format!("✈️ AIR Traffic ({})", air_count)
                    } else {
                        "✈️ AIR Traffic".to_string()
                    };
                    let is_air = self.air_view.is_open;
                    let btn_air = ui.add(
                        egui::Button::new(
                            RichText::new(air_txt)
                                .color(if is_air { Color32::BLACK } else { Color32::from_rgb(0, 230, 255) })
                                .size(11.5)
                                .strong(),
                        )
                        .fill(if is_air { Color32::from_rgb(0, 230, 255) } else { Color32::from_rgb(15, 38, 52) }),
                    );
                    if btn_air.clicked() {
                        self.air_view.is_open = !self.air_view.is_open;
                    }
                    btn_air.on_hover_text("Open Aviation Traffic Radar (ADS-B 1090 MHz & ACARS / VDL2 aircraft tracking and live decoded messages)");

                    ui.separator();

                    // 5. MARINE / AIS Vessel Radar & Decoder Button
                    let ais_count = self.ais_view.saved_vessels.len();
                    let ais_txt = if ais_count > 0 {
                        format!("🚢 MARINE AIS ({})", ais_count)
                    } else {
                        "🚢 MARINE AIS".to_string()
                    };
                    let is_ais = self.ais_view.is_open;
                    let btn_ais = ui.add(
                        egui::Button::new(
                            RichText::new(ais_txt)
                                .color(if is_ais { Color32::BLACK } else { Color32::from_rgb(0, 255, 180) })
                                .size(11.5)
                                .strong(),
                        )
                        .fill(if is_ais { Color32::from_rgb(0, 255, 180) } else { Color32::from_rgb(12, 38, 30) }),
                    );
                    if btn_ais.clicked() {
                        self.ais_view.is_open = !self.ais_view.is_open;
                    }
                    btn_ais.on_hover_text("Open Marine Traffic Radar (AIS 161.975/162.025 MHz vessel tracking, live ship map, and VHF decoder)");
                });
            });
        });

        // Sync selected VFO ID between VFO rack and Control Deck
        if self.vfo_panel.selected_vfo_id.is_some() && self.deck.selected_vfo_id != self.vfo_panel.selected_vfo_id {
            self.deck.selected_vfo_id = self.vfo_panel.selected_vfo_id;
        }

        // 3. Left Sidebar (Dynamic width 340px, resizable, scrollable)
        egui::SidePanel::left("left_sidebar")
            .default_width(340.0)
            .width_range(300.0..=500.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.spacing_mut().scroll.floating = false;
                ui.spacing_mut().scroll.bar_width = 8.0;
                ui.spacing_mut().scroll.bar_inner_margin = 2.0;
                ui.spacing_mut().scroll.bar_outer_margin = 2.0;

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .show(ui, |ui| {
                        self.deck.show_sidebar(ui, snap, &mut self.cfg, &self.dsp.cmd_tx);
                    });
            });

        // Forward any selection changes made in deck back to vfo_panel
        if self.deck.selected_vfo_id.is_some() {
            self.vfo_panel.selected_vfo_id = self.deck.selected_vfo_id;
        }

        // 4. Central Panel (VFO Rack at top, Spectrum in middle, Waterfall at bottom)
        egui::CentralPanel::default().show(ctx, |ui| {
            // 4.1 Active VFO Rack (Top) - Resizable height
            egui::Frame::none()
                .fill(Color32::from_rgb(14, 18, 26))
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(32, 42, 58)))
                .rounding(5.0)
                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                .show(ui, |ui| {
                    ui.set_height(self.vfo_rack_height);
                    self.vfo_panel.show(ui, &snap.vfos, &mut self.cfg, &self.dsp.cmd_tx, self.vfo_rack_height);
                    self.deck.selected_vfo_id = self.vfo_panel.selected_vfo_id;
                });

            // Resizable Splitter between VFO Rack and Spectrum
            render_horizontal_splitter(ui, &mut self.vfo_rack_height, 70.0, 360.0);

            // Collect VFO markers (only active/pinned channels within max_vfos, NOT history)
            let vfo_markers: Vec<(u64, String, bool, bool)> = snap
                .vfos
                .iter()
                .filter(|v| {
                    let is_visible_locked = v.state == VfoState::Locked && (v.active || v.delay_remaining_sec > 0.0 || v.dmr_info.is_some() || v.latest_telemetry_info.is_some());
                    (v.state == VfoState::Pinned || is_visible_locked) && !v.is_out_of_band
                })
                .map(|v| {
                    let label = if !v.name.is_empty() {
                        v.name.clone()
                    } else {
                        format!("VFO#{} {:.4}M", v.id, v.freq_hz as f64 / 1e6)
                    };
                    (v.freq_hz, label, v.active, v.state == VfoState::Pinned)
                })
                .collect();

            // Collect permanent Bookmark markers for Spectrum
            let bookmark_markers: Vec<(u64, String)> = self.cfg.bookmarks.values()
                .map(|b| (b.freq_hz, b.name.clone()))
                .collect();

            let is_scanning = self.deck.scanner_state == ScannerState::Scanning;

            // 4.2 Real-time Spectrum Analyzer - Resizable height
            let spec_resp = self.spectrum.show(
                ui,
                self.spectrum_height,
                &snap.raw_spectrum,
                &snap.pseudo_spectrum,
                &snap.noise_baseline,
                snap.center_freq_hz,
                snap.sample_rate_hz,
                self.cfg.squelch_snr_db,
                snap.scan_cursor_hz,
                is_scanning,
                &vfo_markers,
                &bookmark_markers,
            );

            // Click on spectrum to set VFO
            if spec_resp.clicked() {
                if let Some(pos) = spec_resp.interact_pointer_pos() {
                    let rect = spec_resp.rect;
                    let norm = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                    let half_bw = (snap.sample_rate_hz as f64) * 0.5;
                    let min_f = snap.center_freq_hz as f64 - half_bw;
                    let raw_hz = (min_f + norm as f64 * (half_bw * 2.0)) as u64;

                    let step = (self.cfg.scan_step_hz as u64).max(1000);
                    let target_hz = ((raw_hz + step / 2) / step) * step;

                    let max_allowed = self.cfg.max_vfos.clamp(1, 10);
                    let slot = snap.vfos.iter().take(max_allowed).position(|v| v.state == VfoState::Idle).unwrap_or(0);

                    let default_mode = self.cfg.ranges.iter().find(|r| r.name == self.cfg.active_range).map(|r| r.mode).unwrap_or(crate::config::DemodMode::AM);
                    let mode = crate::gui::deck::get_frequency_mode(target_hz, &self.cfg, &self.cfg.active_range).unwrap_or(default_mode);
                    let name = self.cfg.get_comment(target_hz, step / 2).map(|b| b.name.clone()).unwrap_or_else(|| format!("{:.4} MHz", target_hz as f64 / 1e6));

                    // Manual click: immediately pause the scanner on clicked frequency
                    self.deck.scanner_state = ScannerState::Paused;
                    self.deck.dial_freq_hz = target_hz;
                    self.deck.manual_mode = mode;
                    self.deck.selected_vfo_id = Some(slot);
                    self.vfo_panel.selected_vfo_id = Some(slot);
                    let _ = self.dsp.cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                    let _ = self.dsp.cmd_tx.send(DspCommand::SetManualVfo(slot, target_hz, mode, name));
                }
            }

            // Resizable Splitter between Spectrum and Waterfall
            render_horizontal_splitter(ui, &mut self.spectrum_height, 80.0, 500.0);

            // 4.3 High-FPS Waterfall Display (fills remaining space)
            let wf_resp = self.waterfall.show(
                ui,
                snap.center_freq_hz,
                snap.sample_rate_hz,
                snap.scan_cursor_hz,
                is_scanning,
                &vfo_markers,
            );

            // Click on waterfall to set VFO
            if wf_resp.clicked() {
                if let Some(pos) = wf_resp.interact_pointer_pos() {
                    let rect = wf_resp.rect;
                    let norm = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                    let half_bw = (snap.sample_rate_hz as f64) * 0.5;
                    let min_f = snap.center_freq_hz as f64 - half_bw;
                    let raw_hz = (min_f + norm as f64 * (half_bw * 2.0)) as u64;

                    let step = (self.cfg.scan_step_hz as u64).max(1000);
                    let target_hz = ((raw_hz + step / 2) / step) * step;

                    let max_allowed = self.cfg.max_vfos.clamp(1, 10);

                    // Check if clicked frequency is already monitored by an existing VFO
                    let existing_slot = snap.vfos.iter().take(max_allowed).position(|v| {
                        (v.state == VfoState::Pinned || v.active || v.state == VfoState::Locked)
                            && v.freq_hz.abs_diff(target_hz) <= (step / 2).max(3000)
                    });

                    let slot = if let Some(idx) = existing_slot {
                        idx
                    } else {
                        // 1. Pick first Idle slot within allowed range
                        snap.vfos.iter().take(max_allowed).position(|v| v.state == VfoState::Idle)
                            // 2. Or recycle oldest History slot within allowed range
                            .or_else(|| {
                                snap.vfos.iter().take(max_allowed).enumerate()
                                    .filter(|(_, v)| v.state == VfoState::History)
                                    .min_by(|a, b| a.1.delay_remaining_sec.partial_cmp(&b.1.delay_remaining_sec).unwrap_or(std::cmp::Ordering::Equal))
                                    .map(|(i, _)| i)
                            })
                            // 3. Or replace currently selected VFO (or slot 0) if all slots are locked/pinned
                            .unwrap_or_else(|| {
                                self.deck.selected_vfo_id.unwrap_or(0) % max_allowed
                            })
                    };

                    let default_mode = self.cfg.ranges.iter().find(|r| r.name == self.cfg.active_range).map(|r| r.mode).unwrap_or(crate::config::DemodMode::AM);
                    let mode = crate::gui::deck::get_frequency_mode(target_hz, &self.cfg, &self.cfg.active_range).unwrap_or(default_mode);
                    let name = self.cfg.get_comment(target_hz, step / 2).map(|b| b.name.clone()).unwrap_or_else(|| format!("{:.4} MHz", target_hz as f64 / 1e6));

                    // Manual click: immediately pause the scanner on clicked frequency
                    self.deck.scanner_state = ScannerState::Paused;
                    self.deck.dial_freq_hz = target_hz;
                    self.deck.manual_mode = mode;
                    self.deck.selected_vfo_id = Some(slot);
                    self.vfo_panel.selected_vfo_id = Some(slot);
                    let _ = self.dsp.cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                    let _ = self.dsp.cmd_tx.send(DspCommand::SetManualVfo(slot, target_hz, mode, name));
                }
            }
        });

        // 5. Tactical Inspectors & Popups
        let prev_telem = self.was_telemetry_open;
        let prev_aprs = self.was_aprs_open;
        let prev_dmr = self.was_dmr_open;
        let prev_air = self.was_air_open;
        let prev_ais = self.was_ais_open;

        self.telemetry_view.show(ctx, snap, &self.dsp.cmd_tx, &mut self.deck.scanner_state);
        self.aprs_view.show(ctx, snap, &self.dsp.cmd_tx, &mut self.deck.scanner_state);
        self.dmr_view.show(ctx, snap, &self.dsp.cmd_tx, &mut self.deck.scanner_state);
        self.air_view.show(ctx, snap, &self.dsp.cmd_tx, &mut self.deck.scanner_state);
        self.ais_view.show(ctx, snap, &self.dsp.cmd_tx, &mut self.deck.scanner_state);

        // Keep Home QTH coordinates 100% synchronized between APRS, Air Traffic, and Marine AIS popups
        if self.aprs_view.user_qth != self.user_qth {
            self.user_qth = self.aprs_view.user_qth;
            self.air_view.set_qth(self.user_qth);
            self.ais_view.set_qth(self.user_qth);
            self.aprs_view.save_current();
            let _ = self.dsp.cmd_tx.send(DspCommand::SetQth(self.user_qth.0, self.user_qth.1));
        } else if self.air_view.center_qth != self.user_qth {
            self.user_qth = self.air_view.center_qth;
            self.aprs_view.set_qth(self.user_qth);
            self.ais_view.set_qth(self.user_qth);
            self.aprs_view.save_current();
            let _ = self.dsp.cmd_tx.send(DspCommand::SetQth(self.user_qth.0, self.user_qth.1));
        } else if self.ais_view.center_qth != self.user_qth {
            self.user_qth = self.ais_view.center_qth;
            self.aprs_view.set_qth(self.user_qth);
            self.air_view.set_qth(self.user_qth);
            self.aprs_view.save_current();
            let _ = self.dsp.cmd_tx.send(DspCommand::SetQth(self.user_qth.0, self.user_qth.1));
        }

        let telem_closed = prev_telem && !self.telemetry_view.is_open;
        let aprs_closed = prev_aprs && !self.aprs_view.is_open;
        let dmr_closed = prev_dmr && !self.dmr_view.is_open;
        let air_closed = prev_air && !self.air_view.is_open;
        let ais_closed = prev_ais && !self.ais_view.is_open;

        if telem_closed || aprs_closed || dmr_closed || air_closed || ais_closed {
            self.handle_popup_closed();
        }

        self.was_telemetry_open = self.telemetry_view.is_open;
        self.was_aprs_open = self.aprs_view.is_open;
        self.was_dmr_open = self.dmr_view.is_open;
        self.was_air_open = self.air_view.is_open;
        self.was_ais_open = self.ais_view.is_open;
    }
}

/// Interactive horizontal drag splitter allowing user to resize panel heights
fn render_horizontal_splitter(
    ui: &mut egui::Ui,
    height: &mut f32,
    min_h: f32,
    max_h: f32,
) {
    let bar_height = 8.0;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), bar_height),
        egui::Sense::click_and_drag(),
    );

    if resp.dragged() {
        let delta_y = ui.input(|i| i.pointer.delta().y);
        *height = (*height + delta_y).clamp(min_h, max_h);
    }

    let is_active = resp.hovered() || resp.dragged();
    if is_active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }

    let painter = ui.painter_at(rect);
    let line_col = if resp.dragged() {
        Color32::from_rgb(0, 220, 255)
    } else if resp.hovered() {
        Color32::from_rgb(70, 130, 190)
    } else {
        Color32::from_rgb(26, 38, 54)
    };

    // Center divider line
    painter.line_segment(
        [
            egui::pos2(rect.min.x, rect.center().y),
            egui::pos2(rect.max.x, rect.center().y),
        ],
        egui::Stroke::new(if is_active { 2.0_f32 } else { 1.0_f32 }, line_col),
    );

    // Grip dots in center
    let cx = rect.center().x;
    let cy = rect.center().y;
    let dot_col = if is_active {
        Color32::WHITE
    } else {
        Color32::from_rgb(60, 80, 110)
    };
    painter.circle_filled(egui::pos2(cx - 14.0, cy), 1.5, dot_col);
    painter.circle_filled(egui::pos2(cx, cy), 1.5, dot_col);
    painter.circle_filled(egui::pos2(cx + 14.0, cy), 1.5, dot_col);
}
