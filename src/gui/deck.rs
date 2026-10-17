use crate::config::{Bookmark, DemodMode, ScanRangeProfile, ScannerConfig};
use crate::dsp_worker::{DisplaySnapshot, DspCommand};
use crate::scanner::vfo::VfoState;
use crate::scanner::ScannerState;
use crate::sdr::DeviceSelection;
use crossbeam_channel::Sender;
use eframe::egui::{self, Color32, Pos2, RichText, Stroke};

pub struct ControlDeck {
    pub selected_device: DeviceSelection,
    pub master_volume: f32,
    pub scanner_state: ScannerState,
    pub dial_freq_hz: u64,
    pub new_range_name: String,
    pub new_range_start_mhz: f64,
    pub new_range_end_mhz: f64,
    pub new_range_step_khz: f64,
    pub new_range_mode: DemodMode,
    pub show_band_editor: bool,
    pub audio_devices: Vec<String>,
    pub audio_rx: Option<std::sync::mpsc::Receiver<Vec<String>>>,
    pub new_known_mhz: f64,
    pub new_known_name: String,
    pub new_known_mode: DemodMode,
    pub known_freq_search: String,
    pub edit_band_name: String,
    pub edit_start_str: String,
    pub edit_end_str: String,
    pub edit_step_khz: f64,
    pub edit_mode: DemodMode,
    pub manual_mode: DemodMode,
    pub dial_last_step: std::time::Instant,
    pub selected_vfo_id: Option<usize>,
}

impl ControlDeck {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let devs = crate::audio::get_output_devices();
            let _ = tx.send(devs);
        });

        Self {
            selected_device: DeviceSelection::AirspyR2,
            master_volume: 1.0,
            scanner_state: ScannerState::Scanning,
            dial_freq_hz: 0,
            new_range_name: "NEW BAND".to_string(),
            new_range_start_mhz: 144.0,
            new_range_end_mhz: 146.0,
            new_range_step_khz: 12.5,
            new_range_mode: DemodMode::NFM,
            show_band_editor: false,
            audio_devices: vec!["Default (Windows System Output)".to_string()],
            audio_rx: Some(rx),
            new_known_mhz: 122.500,
            new_known_name: String::new(),
            new_known_mode: DemodMode::AM,
            known_freq_search: String::new(),
            edit_band_name: String::new(),
            edit_start_str: String::new(),
            edit_end_str: String::new(),
            edit_step_khz: 25.0,
            edit_mode: DemodMode::AM,
            manual_mode: DemodMode::NFM,
            dial_last_step: std::time::Instant::now(),
            selected_vfo_id: None,
        }
    }

    /// Renders modern tactical left sidebar controls (SpectraVox Layout)
    pub fn show_sidebar(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DisplaySnapshot,
        cfg: &mut ScannerConfig,
        cmd_tx: &Sender<DspCommand>,
    ) {
        ui.vertical(|ui| {
            // ==========================================
            // 1. MASTER ENGINE CONTROL CARD
            // ==========================================
            egui::Frame::none()
                .fill(Color32::from_rgb(16, 22, 32))
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(35, 52, 75)))
                .rounding(6.0)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    // Tactical Digital Frequency Dial (placed above START SCANNER)
                    self.render_frequency_dial(ui, snapshot, cfg, cmd_tx);

                    ui.add_space(8.0);

                    // Sweep Toggle Button
                    let (btn_text, btn_col, btn_txt_col) = if self.scanner_state == ScannerState::Scanning {
                        ("⏸  PAUSE SWEEP", Color32::from_rgb(0, 195, 235), Color32::BLACK)
                    } else {
                        ("▶  START SCANNER", Color32::from_rgb(0, 220, 110), Color32::BLACK)
                    };

                    let scan_btn = ui.add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new(RichText::new(btn_text).color(btn_txt_col).size(13.0).strong())
                            .fill(btn_col),
                    );
                    if scan_btn.clicked() {
                        self.scanner_state = match self.scanner_state {
                            ScannerState::Scanning => ScannerState::Paused,
                            ScannerState::Paused => {
                                if let Some(r) = cfg.ranges.iter().find(|r| r.name == cfg.active_range) {
                                    self.manual_mode = r.mode;
                                }
                                ScannerState::Scanning
                            }
                        };
                        let _ = cmd_tx.send(DspCommand::SetScannerState(self.scanner_state));
                    }

                    ui.add_space(6.0);

                    // Manual Demodulation Mode Selector Grid (WFM, NFM, AM, BCFM, USB, LSB)
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("DEMODULATION MODE").size(10.0).color(Color32::from_rgb(140, 160, 185)).strong());
                    });
                    ui.add_space(2.0);

                    let avail_w = ui.available_width();
                    let btn_w = (avail_w - 8.0) / 3.0;
                    let modes = [
                        DemodMode::WFM,
                        DemodMode::NFM,
                        DemodMode::AM,
                        DemodMode::BCFM,
                        DemodMode::USB,
                        DemodMode::LSB,
                    ];

                    egui::Grid::new("manual_demod_mode_grid")
                        .spacing([4.0, 4.0])
                        .show(ui, |ui| {
                            for (i, &mode) in modes.iter().enumerate() {
                                let is_sel = self.manual_mode == mode;
                                let (bg, fg) = if is_sel {
                                    (Color32::from_rgb(0, 220, 255), Color32::BLACK)
                                } else {
                                    (Color32::from_rgb(22, 30, 42), Color32::from_rgb(190, 210, 230))
                                };
                                if ui.add_sized(
                                    [btn_w, 24.0],
                                    egui::Button::new(RichText::new(mode.name()).size(11.0).strong().color(fg)).fill(bg),
                                ).clicked() {
                                    self.manual_mode = mode;
                                    let target_vfo = self.selected_vfo_id.unwrap_or(0).min(cfg.max_vfos.saturating_sub(1));
                                    let _ = cmd_tx.send(DspCommand::SetVfoMode(target_vfo, mode));
                                }
                                if i == 2 {
                                    ui.end_row();
                                }
                            }
                        });

                    ui.add_space(8.0);

                    // Master Volume with Value Badge
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("AUDIO OUTPUT LEVEL").size(10.5).color(Color32::from_rgb(150, 175, 200)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let pct = (self.master_volume * 100.0).round() as i32;
                            ui.label(
                                RichText::new(format!("{:>3}%", pct))
                                    .size(11.5)
                                    .color(Color32::from_rgb(0, 220, 255))
                                    .monospace()
                                    .strong(),
                            );
                        });
                    });

                    ui.add_space(3.0);
                    if tactical_slider(ui, &mut self.master_volume, 0.0..=2.0, Color32::from_rgb(0, 220, 255)) {
                        let _ = cmd_tx.send(DspCommand::SetMasterVolume(self.master_volume));
                    }
                });

            ui.add_space(6.0);

            // ==========================================
            // 2. RECEIVER HARDWARE & AUDIO PLAYBACK CARD
            // ==========================================
            egui::Frame::none()
                .fill(Color32::from_rgb(16, 20, 28))
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(32, 42, 58)))
                .rounding(6.0)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("SDR HARDWARE").size(11.0).color(Color32::from_rgb(180, 200, 220)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (dot_txt, dot_col) = if snapshot.device_running {
                                ("● ONLINE", Color32::from_rgb(0, 255, 128))
                            } else {
                                ("○ OFFLINE", Color32::from_rgb(255, 60, 60))
                            };
                            ui.label(RichText::new(dot_txt).size(10.0).color(dot_col).monospace().strong());
                        });
                    });

                    ui.add_space(4.0);

                    // Device selection dropdown
                    let prev_dev = self.selected_device;
                    egui::ComboBox::from_id_salt("side_dev_select")
                        .selected_text(self.selected_device.name())
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.selected_device, DeviceSelection::AirspyR2, "Airspy R2 (10 MSPS)");
                            ui.selectable_value(&mut self.selected_device, DeviceSelection::RtlSdr, "RTL-SDR (2.4 MSPS)");
                            ui.selectable_value(&mut self.selected_device, DeviceSelection::SimulatedRf, "Simulated RF Generator");
                        });
                    if prev_dev != self.selected_device {
                        let _ = cmd_tx.send(DspCommand::SwitchDevice(self.selected_device));
                    }

                    ui.add_space(6.0);

                    // RF Gain with Badge and Wide Tactical Slider
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("RF LNA GAIN").size(10.0).color(Color32::from_rgb(140, 160, 180)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{}/21", cfg.rf_gain))
                                    .size(11.5)
                                    .color(Color32::from_rgb(255, 190, 80))
                                    .monospace()
                                    .strong(),
                            );
                        });
                    });

                    ui.add_space(3.0);
                    if tactical_slider_u32(ui, &mut cfg.rf_gain, 0, 21, Color32::from_rgb(255, 180, 50)) {
                        let _ = cmd_tx.send(DspCommand::SetRfGain(cfg.rf_gain));
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Audio Playback Device Section
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("AUDIO PLAYBACK DEVICE").size(10.0).color(Color32::from_rgb(140, 165, 190)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let is_refreshing = self.audio_rx.is_some();
                            let btn_text = if is_refreshing { "⏳" } else { "🔄" };
                            if ui.add_enabled(!is_refreshing, egui::Button::new(RichText::new(btn_text).size(10.0)))
                                .on_hover_text("Refresh audio device list")
                                .clicked()
                            {
                                let (tx, rx) = std::sync::mpsc::channel();
                                self.audio_rx = Some(rx);
                                std::thread::spawn(move || {
                                    let devs = crate::audio::get_output_devices();
                                    let _ = tx.send(devs);
                                });
                            }
                        });
                    });

                    ui.add_space(2.0);

                    // Check for background audio device scan results (never block main UI thread!)
                    if let Some(rx) = &self.audio_rx {
                        if let Ok(devices) = rx.try_recv() {
                            self.audio_devices = devices;
                            self.audio_rx = None;
                        }
                    }

                    let cur_device_label = if cfg.audio_device.is_empty() {
                        "Default (Windows System Output)"
                    } else {
                        cfg.audio_device.as_str()
                    };

                    egui::ComboBox::from_id_salt("side_audio_playback_device_select")
                        .selected_text(cur_device_label)
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for dev in &self.audio_devices {
                                let is_sel = cfg.audio_device == *dev;
                                if ui.selectable_label(is_sel, dev).clicked() {
                                    cfg.audio_device = dev.clone();
                                    let _ = cfg.save();
                                    let _ = cmd_tx.send(DspCommand::SetAudioDevice(dev.clone()));
                                }
                            }
                        });

                    if !snapshot.current_audio_device.is_empty() {
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(format!("● Active Output: {}", snapshot.current_audio_device))
                                .size(9.5)
                                .color(Color32::from_rgb(0, 220, 140))
                                .monospace(),
                        );
                    }
                });

            ui.add_space(6.0);

            // ==========================================
            // 3. SCANNER DETECTION & TIMING CARD
            // ==========================================
            egui::Frame::none()
                .fill(Color32::from_rgb(16, 20, 28))
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(32, 42, 58)))
                .rounding(6.0)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.label(RichText::new("DETECTION & TIMING").size(11.0).color(Color32::from_rgb(180, 200, 220)).strong());
                    ui.add_space(6.0);

                    // 3.1 Squelch Threshold (Wide Tactical Slider)
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("SQUELCH THRESHOLD").size(10.0).color(Color32::from_rgb(140, 160, 180)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let sq_col = if cfg.squelch_snr_db <= 6.0 {
                                Color32::from_rgb(50, 255, 120)
                            } else if cfg.squelch_snr_db <= 12.0 {
                                Color32::from_rgb(255, 200, 60)
                            } else {
                                Color32::from_rgb(255, 120, 80)
                            };
                            ui.label(
                                RichText::new(format!("{:>+4.1} dB", cfg.squelch_snr_db))
                                    .size(11.5)
                                    .color(sq_col)
                                    .monospace()
                                    .strong(),
                            );
                        });
                    });

                    ui.add_space(3.0);
                    if tactical_slider(ui, &mut cfg.squelch_snr_db, 1.0..=20.0, Color32::from_rgb(50, 255, 120)) {
                        let _ = cmd_tx.send(DspCommand::SetSquelch(cfg.squelch_snr_db));
                    }

                    ui.add_space(6.0);

                    // 3.2 Carrier Hold Delay (Wide Tactical Slider)
                    let mut delay_sec = cfg.hangover_ms as f32 / 1000.0;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("CARRIER HOLD DELAY").size(10.0).color(Color32::from_rgb(140, 160, 180)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{:.2} s", delay_sec))
                                    .size(11.5)
                                    .color(Color32::from_rgb(0, 220, 255))
                                    .monospace()
                                    .strong(),
                            );
                        });
                    });

                    ui.add_space(3.0);
                    if tactical_slider(ui, &mut delay_sec, 0.5..=10.0, Color32::from_rgb(0, 200, 255)) {
                        cfg.hangover_ms = (delay_sec * 1000.0).round() as u64;
                        let _ = cfg.save();
                        let _ = cmd_tx.send(DspCommand::SetHangoverMs(cfg.hangover_ms));
                    }

                    ui.add_space(6.0);

                    // 3.3 Max Concurrent Channels (Wide Tactical Slider)
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("MAX ACTIVE CHANNELS").size(10.0).color(Color32::from_rgb(140, 160, 180)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{}/10", cfg.max_vfos))
                                    .size(11.5)
                                    .color(Color32::from_rgb(160, 220, 90))
                                    .monospace()
                                    .strong(),
                            );
                        });
                    });

                    ui.add_space(3.0);
                    if tactical_slider_int(ui, &mut cfg.max_vfos, 1, 10, Color32::from_rgb(140, 230, 80)) {
                        let _ = cmd_tx.send(DspCommand::SetMaxVfos(cfg.max_vfos));
                    }

                    ui.add_space(6.0);

                    // 3.4 VAD Voice Filter
                    ui.horizontal(|ui| {
                        let vad_chk = ui.checkbox(&mut cfg.vad_enabled, RichText::new("VAD Voice Filter").size(10.5));
                        if vad_chk.changed() {
                            let _ = cmd_tx.send(DspCommand::SetVad(cfg.vad_enabled, cfg.vad_threshold));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (v_txt, v_col) = if cfg.vad_enabled {
                                ("ACTIVE", Color32::from_rgb(0, 255, 128))
                            } else {
                                ("OFF", Color32::from_rgb(100, 110, 120))
                            };
                            ui.label(RichText::new(v_txt).size(10.0).color(v_col).monospace().strong());
                        });
                    });

                    if cfg.vad_enabled {
                        ui.add_space(3.0);
                        if tactical_slider(ui, &mut cfg.vad_threshold, 0.1..=0.8, Color32::from_rgb(255, 160, 40)) {
                            let _ = cmd_tx.send(DspCommand::SetVad(cfg.vad_enabled, cfg.vad_threshold));
                        }
                    }
                });

            ui.add_space(6.0);

            // ==========================================
            // 4. FREQUENCY BAND MATRIX CARD
            // ==========================================
            egui::Frame::none()
                .fill(Color32::from_rgb(16, 20, 28))
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(32, 42, 58)))
                .rounding(6.0)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("FREQUENCY BANDS").size(11.0).color(Color32::from_rgb(180, 200, 220)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let active_count = cfg.ranges.iter().filter(|r| r.enabled).count();
                            ui.label(
                                RichText::new(format!("MULTI-SCAN: {}", active_count))
                                    .size(9.5)
                                    .color(if active_count > 1 {
                                        Color32::from_rgb(0, 255, 128)
                                    } else {
                                        Color32::from_rgb(120, 140, 160)
                                    })
                                    .monospace()
                                    .strong(),
                            );
                        });
                    });

                    ui.add_space(4.0);

                    // Band List Items
                    for range in &cfg.ranges.clone() {
                        let is_active = cfg.active_range == range.name;

                        ui.push_id(format!("band_row_{}", range.name), |ui| {
                            ui.horizontal(|ui| {
                                let mut enabled = range.enabled;
                                let chk = ui.checkbox(&mut enabled, "");
                                if chk.changed() {
                                    if let Some(r) = cfg.ranges.iter_mut().find(|r| r.name == range.name) {
                                        r.enabled = enabled;
                                    }
                                    let _ = cfg.save();
                                    let _ = cmd_tx.send(DspCommand::SetRangeEnabled {
                                        name: range.name.clone(),
                                        enabled,
                                    });
                                }
                                chk.on_hover_text("Include this band in multi-band rotational scanning sweep");

                                let btn_w = ui.available_width();
                                let start_m = range.scan_start_hz as f64 / 1e6;
                                let end_m = range.scan_end_hz as f64 / 1e6;

                                let (bg_col, txt_col) = if is_active {
                                    (Color32::from_rgb(0, 180, 100), Color32::BLACK)
                                } else if range.enabled {
                                    (Color32::from_rgb(25, 45, 65), Color32::from_rgb(200, 225, 250))
                                } else {
                                    (Color32::from_rgb(24, 28, 36), Color32::from_rgb(140, 150, 160))
                                };

                                let btn = ui.add_sized(
                                    [btn_w, 24.0],
                                    egui::Button::new(
                                        RichText::new(format!("{}  [{:.1}-{:.1}M]", range.name, start_m, end_m))
                                            .color(txt_col)
                                            .size(10.5)
                                            .strong(),
                                    )
                                    .fill(bg_col),
                                );

                                if btn.clicked() {
                                    // 1. Clean up temporary popup profiles
                                    cfg.ranges.retain(|r| !crate::config::is_temporary_scan_profile(&r.name));

                                    // 2. Select this band as active and single-enabled for dedicated listening
                                    cfg.active_range = range.name.clone();
                                    self.manual_mode = range.mode;
                                    self.dial_freq_hz = range.scan_start_hz;
                                    for r in &mut cfg.ranges {
                                        r.enabled = r.name == range.name;
                                    }
                                    let _ = cfg.save();

                                    // 3. Sync enabled states to worker
                                    for r in &cfg.ranges {
                                        let _ = cmd_tx.send(DspCommand::SetRangeEnabled {
                                            name: r.name.clone(),
                                            enabled: r.enabled,
                                        });
                                    }

                                    // 4. Tune to this band
                                    let usable_bw = ((cfg.sample_rate as f64) * 0.85).round() as u64;
                                    let is_wide = (range.scan_end_hz - range.scan_start_hz) > usable_bw;
                                    let center_hz = if is_wide {
                                        range.scan_start_hz + (usable_bw / 2)
                                    } else {
                                        range.center_freq_hz
                                    };
                                    cfg.center_freq_hz = center_hz;
                                    cfg.scan_start_hz = range.scan_start_hz;
                                    cfg.scan_end_hz = range.scan_end_hz;
                                    cfg.scan_step_hz = range.scan_step_hz;

                                    let _ = cmd_tx.send(DspCommand::SetScanProfile {
                                        name: range.name.clone(),
                                        center_freq_hz: center_hz,
                                        scan_start_hz: range.scan_start_hz,
                                        scan_end_hz: range.scan_end_hz,
                                        scan_step_hz: range.scan_step_hz,
                                        mode: range.mode,
                                    });
                                }
                            });
                        });
                        ui.add_space(1.0);
                    }

                    ui.add_space(6.0);

                    // Toggle Button for Band Management Editor
                    let editor_toggle = if self.show_band_editor { "▼ HIDE BAND PROFILE EDITOR" } else { "⚙ MANAGE / ADD BAND PROFILES" };
                    if ui.add_sized(
                        [ui.available_width(), 26.0],
                        egui::Button::new(RichText::new(editor_toggle).size(11.0).strong().color(Color32::from_rgb(0, 220, 255)))
                            .fill(Color32::from_rgb(22, 30, 42)),
                    ).clicked() {
                        self.show_band_editor = !self.show_band_editor;
                    }

                    // Spacious Tactical Band Editor
                    if self.show_band_editor {
                        ui.add_space(6.0);

                        // 1. Edit Active Band
                        egui::Frame::none()
                            .fill(Color32::from_rgb(20, 26, 36))
                            .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(45, 60, 82)))
                            .rounding(5.0)
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                let active_name = cfg.active_range.clone();
                                if let Some(pos) = cfg.ranges.iter().position(|r| r.name == active_name) {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("EDIT ACTIVE:").size(10.5).color(Color32::from_rgb(0, 220, 255)).strong());
                                        ui.label(RichText::new(&active_name).size(11.0).color(Color32::WHITE).strong());
                                    });
                                    ui.add_space(6.0);

                                    let cur_name = cfg.ranges[pos].name.clone();
                                    if self.edit_band_name != active_name {
                                        self.edit_band_name = active_name.clone();
                                        self.edit_start_str = format!("{:.4}", cfg.ranges[pos].scan_start_hz as f64 / 1e6);
                                        self.edit_end_str = format!("{:.4}", cfg.ranges[pos].scan_end_hz as f64 / 1e6);
                                        self.edit_step_khz = cfg.ranges[pos].scan_step_hz as f64 / 1e3;
                                        self.edit_mode = cfg.ranges[pos].mode;
                                    }

                                    let mut changed = false;

                                    // Start Frequency
                                    ui.label(RichText::new("START FREQUENCY (MHz)").size(10.0).color(Color32::from_rgb(150, 170, 190)).strong());
                                    let mut start_val = self.edit_start_str.parse::<f64>().unwrap_or(cfg.ranges[pos].scan_start_hz as f64 / 1e6);
                                    ui.horizontal(|ui| {
                                        // 1. Direct keyboard text typing
                                        let text_resp = ui.add(
                                            egui::TextEdit::singleline(&mut self.edit_start_str)
                                                .desired_width(70.0)
                                                .hint_text("MHz")
                                        );
                                        if text_resp.lost_focus() || text_resp.changed() {
                                            if let Ok(val) = self.edit_start_str.trim().parse::<f64>() {
                                                start_val = val.max(0.5);
                                                changed = true;
                                            }
                                        }

                                        // 2. Mouse sliding DragValue
                                        if ui.add_sized([65.0, 24.0], egui::DragValue::new(&mut start_val).speed(0.025).range(1.0..=2000.0).suffix("M")).changed() {
                                            self.edit_start_str = format!("{:.4}", start_val);
                                            changed = true;
                                        }

                                        // 3. Step buttons
                                        if ui.add_sized([34.0, 24.0], egui::Button::new("-1M")).clicked() {
                                            start_val = (start_val - 1.0).max(0.5);
                                            self.edit_start_str = format!("{:.4}", start_val);
                                            changed = true;
                                        }
                                        if ui.add_sized([34.0, 24.0], egui::Button::new("+1M")).clicked() {
                                            start_val += 1.0;
                                            self.edit_start_str = format!("{:.4}", start_val);
                                            changed = true;
                                        }
                                    });

                                    ui.add_space(4.0);

                                    // Stop Frequency
                                    ui.label(RichText::new("STOP FREQUENCY (MHz)").size(10.0).color(Color32::from_rgb(150, 170, 190)).strong());
                                    let mut end_val = self.edit_end_str.parse::<f64>().unwrap_or(cfg.ranges[pos].scan_end_hz as f64 / 1e6);
                                    ui.horizontal(|ui| {
                                        // 1. Direct keyboard text typing
                                        let text_resp = ui.add(
                                            egui::TextEdit::singleline(&mut self.edit_end_str)
                                                .desired_width(70.0)
                                                .hint_text("MHz")
                                        );
                                        if text_resp.lost_focus() || text_resp.changed() {
                                            if let Ok(val) = self.edit_end_str.trim().parse::<f64>() {
                                                end_val = val.max(0.5);
                                                changed = true;
                                            }
                                        }

                                        // 2. Mouse sliding DragValue
                                        if ui.add_sized([65.0, 24.0], egui::DragValue::new(&mut end_val).speed(0.025).range(1.0..=2000.0).suffix("M")).changed() {
                                            self.edit_end_str = format!("{:.4}", end_val);
                                            changed = true;
                                        }

                                        // 3. Step buttons
                                        if ui.add_sized([34.0, 24.0], egui::Button::new("-1M")).clicked() {
                                            end_val = (end_val - 1.0).max(0.5);
                                            self.edit_end_str = format!("{:.4}", end_val);
                                            changed = true;
                                        }
                                        if ui.add_sized([34.0, 24.0], egui::Button::new("+1M")).clicked() {
                                            end_val += 1.0;
                                            self.edit_end_str = format!("{:.4}", end_val);
                                            changed = true;
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // Step Pills
                                    ui.label(RichText::new("CHANNEL STEP").size(10.0).color(Color32::from_rgb(150, 170, 190)).strong());
                                    ui.horizontal(|ui| {
                                        let steps = [(8.33, "8.33k"), (12.5, "12.5k"), (25.0, "25k"), (50.0, "50k")];
                                        for (s_val, s_label) in steps {
                                            let is_sel = (self.edit_step_khz - s_val).abs() < 0.05;
                                            let btn = ui.add_sized(
                                                [44.0, 22.0],
                                                egui::Button::new(RichText::new(s_label).size(11.0).strong().color(if is_sel { Color32::BLACK } else { Color32::WHITE }))
                                                    .fill(if is_sel { Color32::from_rgb(0, 220, 255) } else { Color32::from_rgb(32, 40, 52) }),
                                            );
                                            if btn.clicked() {
                                                self.edit_step_khz = s_val;
                                                changed = true;
                                            }
                                        }
                                        if ui.add_sized([55.0, 22.0], egui::DragValue::new(&mut self.edit_step_khz).speed(0.1).suffix("k")).changed() {
                                            changed = true;
                                        }
                                    });

                                    ui.add_space(6.0);

                                    // Mode Segmented Buttons
                                    ui.label(RichText::new("MODULATION MODE").size(10.0).color(Color32::from_rgb(150, 170, 190)).strong());
                                    let modes = [
                                        DemodMode::WFM,
                                        DemodMode::NFM,
                                        DemodMode::AM,
                                        DemodMode::BCFM,
                                        DemodMode::USB,
                                        DemodMode::LSB,
                                    ];
                                    let btn_w = (ui.available_width() - 8.0) / 3.0;
                                    egui::Grid::new("edit_range_mode_grid")
                                        .spacing([4.0, 4.0])
                                        .show(ui, |ui| {
                                            for (i, &m) in modes.iter().enumerate() {
                                                let is_sel = self.edit_mode == m;
                                                let btn = ui.add_sized(
                                                    [btn_w, 24.0],
                                                    egui::Button::new(RichText::new(m.name()).size(11.0).strong().color(if is_sel { Color32::BLACK } else { Color32::WHITE }))
                                                        .fill(if is_sel { Color32::from_rgb(0, 220, 120) } else { Color32::from_rgb(32, 40, 52) }),
                                                );
                                                if btn.clicked() {
                                                    self.edit_mode = m;
                                                    changed = true;
                                                }
                                                if i == 2 {
                                                    ui.end_row();
                                                }
                                            }
                                        });

                                    ui.add_space(8.0);

                                    // Save Button
                                    let save_btn = ui.add_sized(
                                        [ui.available_width(), 28.0],
                                        egui::Button::new(RichText::new("💾 SAVE BAND PROFILE").size(11.5).strong().color(Color32::BLACK))
                                            .fill(Color32::from_rgb(0, 220, 140)),
                                    );
                                    if save_btn.clicked() || changed {
                                        let start_hz = (start_val * 1e6).round() as u64;
                                        let end_hz = (end_val * 1e6).round() as u64;
                                        let usable_bw = ((cfg.sample_rate as f64) * 0.85).round() as u64;
                                        let is_wide = (end_hz - start_hz) > usable_bw;
                                        let center_hz = if is_wide {
                                            start_hz + (usable_bw / 2)
                                        } else {
                                            (start_hz + end_hz) / 2
                                        };
                                        let step_hz = (self.edit_step_khz * 1e3).round() as u32;

                                        cfg.ranges[pos].scan_start_hz = start_hz;
                                        cfg.ranges[pos].scan_end_hz = end_hz;
                                        cfg.ranges[pos].center_freq_hz = center_hz;
                                        cfg.ranges[pos].scan_step_hz = step_hz;
                                        cfg.ranges[pos].mode = self.edit_mode;

                                        cfg.scan_start_hz = start_hz;
                                        cfg.scan_end_hz = end_hz;
                                        cfg.center_freq_hz = center_hz;
                                        cfg.scan_step_hz = step_hz;

                                        let _ = cfg.save();
                                        let _ = cmd_tx.send(DspCommand::SetScanProfile {
                                            name: cur_name,
                                            center_freq_hz: center_hz,
                                            scan_start_hz: start_hz,
                                            scan_end_hz: end_hz,
                                            scan_step_hz: step_hz,
                                            mode: self.edit_mode,
                                        });
                                    }

                                    if cfg.ranges.len() > 1 {
                                        ui.add_space(4.0);
                                        let del_btn = ui.add_sized(
                                            [ui.available_width(), 24.0],
                                            egui::Button::new(RichText::new("🗑 Delete This Band").size(11.0).color(Color32::WHITE))
                                                .fill(Color32::from_rgb(140, 35, 35)),
                                        );
                                        if del_btn.clicked() {
                                            cfg.ranges.remove(pos);
                                            let next_range = cfg.ranges[0].clone();
                                            cfg.active_range = next_range.name.clone();
                                            cfg.center_freq_hz = next_range.center_freq_hz;
                                            cfg.scan_start_hz = next_range.scan_start_hz;
                                            cfg.scan_end_hz = next_range.scan_end_hz;
                                            cfg.scan_step_hz = next_range.scan_step_hz;
                                            let _ = cfg.save();
                                            let _ = cmd_tx.send(DspCommand::SetScanProfile {
                                                name: next_range.name,
                                                center_freq_hz: next_range.center_freq_hz,
                                                scan_start_hz: next_range.scan_start_hz,
                                                scan_end_hz: next_range.scan_end_hz,
                                                scan_step_hz: next_range.scan_step_hz,
                                                mode: next_range.mode,
                                            });
                                        }
                                    }
                                }
                            });

                        ui.add_space(6.0);

                        // 2. Create New Band
                        egui::Frame::none()
                            .fill(Color32::from_rgb(18, 24, 32))
                            .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(38, 50, 68)))
                            .rounding(5.0)
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new("➕ ADD NEW BAND PROFILE").size(10.5).color(Color32::from_rgb(0, 200, 255)).strong());
                                ui.add_space(4.0);

                                ui.label(RichText::new("BAND NAME").size(10.0).color(Color32::from_rgb(140, 160, 180)).strong());
                                ui.add_sized([ui.available_width(), 24.0], egui::TextEdit::singleline(&mut self.new_range_name));

                                ui.add_space(4.0);
                                ui.label(RichText::new("FREQUENCY BOUNDS (MHz)").size(10.0).color(Color32::from_rgb(140, 160, 180)).strong());
                                ui.horizontal(|ui| {
                                    ui.add_sized([105.0, 24.0], egui::DragValue::new(&mut self.new_range_start_mhz).speed(0.025).suffix(" MHz"));
                                    ui.label("➔");
                                    ui.add_sized([105.0, 24.0], egui::DragValue::new(&mut self.new_range_end_mhz).speed(0.025).suffix(" MHz"));
                                });

                                ui.add_space(4.0);
                                ui.label(RichText::new("CHANNEL STEP").size(10.0).color(Color32::from_rgb(140, 160, 180)).strong());
                                ui.horizontal(|ui| {
                                    let steps = [(8.33, "8.33k"), (12.5, "12.5k"), (25.0, "25k"), (50.0, "50k")];
                                    for (s_val, s_label) in steps {
                                        let is_sel = (self.new_range_step_khz - s_val).abs() < 0.05;
                                        if ui.add_sized([44.0, 20.0], egui::Button::new(RichText::new(s_label).size(10.0)).fill(if is_sel { Color32::from_rgb(0, 220, 255) } else { Color32::from_rgb(30, 36, 48) })).clicked() {
                                            self.new_range_step_khz = s_val;
                                        }
                                    }
                                });

                                ui.add_space(4.0);
                                ui.label(RichText::new("DEMODULATION").size(10.0).color(Color32::from_rgb(140, 160, 180)).strong());
                                 let modes = [
                                     DemodMode::WFM,
                                     DemodMode::NFM,
                                     DemodMode::AM,
                                     DemodMode::BCFM,
                                     DemodMode::USB,
                                     DemodMode::LSB,
                                 ];
                                 let btn_w = (ui.available_width() - 8.0) / 3.0;
                                 egui::Grid::new("new_range_mode_grid")
                                     .spacing([4.0, 4.0])
                                     .show(ui, |ui| {
                                         for (i, &m) in modes.iter().enumerate() {
                                             let is_sel = self.new_range_mode == m;
                                             if ui.add_sized(
                                                 [btn_w, 22.0],
                                                 egui::Button::new(RichText::new(m.name()).size(11.0).strong().color(if is_sel { Color32::BLACK } else { Color32::WHITE }))
                                                     .fill(if is_sel { Color32::from_rgb(0, 220, 120) } else { Color32::from_rgb(30, 36, 48) }),
                                             ).clicked() {
                                                 self.new_range_mode = m;
                                             }
                                             if i == 2 {
                                                 ui.end_row();
                                             }
                                         }
                                     });

                                ui.add_space(6.0);
                                let add_btn = ui.add_sized(
                                    [ui.available_width(), 26.0],
                                    egui::Button::new(RichText::new("➕ CREATE BAND PROFILE").size(11.5).strong().color(Color32::BLACK))
                                        .fill(Color32::from_rgb(0, 180, 240)),
                                );
                                if add_btn.clicked() {
                                    let start_hz = (self.new_range_start_mhz * 1e6).round() as u64;
                                    let end_hz = (self.new_range_end_mhz * 1e6).round() as u64;
                                    let usable_bw = ((cfg.sample_rate as f64) * 0.85).round() as u64;
                                    let is_wide = (end_hz - start_hz) > usable_bw;
                                    let center_hz = if is_wide {
                                        start_hz + (usable_bw / 2)
                                    } else {
                                        (start_hz + end_hz) / 2
                                    };
                                    let step_hz = (self.new_range_step_khz * 1e3).round() as u32;

                                    let new_profile = ScanRangeProfile {
                                        name: self.new_range_name.clone(),
                                        center_freq_hz: center_hz,
                                        scan_start_hz: start_hz,
                                        scan_end_hz: end_hz,
                                        scan_step_hz: step_hz,
                                        mode: self.new_range_mode,
                                        enabled: true,
                                    };

                                    cfg.ranges.push(new_profile.clone());
                                    cfg.active_range = new_profile.name.clone();
                                    cfg.center_freq_hz = center_hz;
                                    cfg.scan_start_hz = start_hz;
                                    cfg.scan_end_hz = end_hz;
                                    cfg.scan_step_hz = step_hz;
                                    let _ = cfg.save();

                                    let _ = cmd_tx.send(DspCommand::SetScanProfile {
                                        name: new_profile.name,
                                        center_freq_hz: center_hz,
                                        scan_start_hz: start_hz,
                                        scan_end_hz: end_hz,
                                        scan_step_hz: step_hz,
                                        mode: new_profile.mode,
                                    });
                                }
                            });
                    }
                });

            ui.add_space(6.0);

            // ==========================================
            // 6. KNOWN FREQUENCIES (Bookmarks Manager)
            // ==========================================
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 22, 30))
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(38, 55, 75)))
                .rounding(6.0)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("📖 KNOWN FREQUENCIES").size(13.0).color(Color32::from_rgb(0, 220, 255)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{}", cfg.bookmarks.len()))
                                    .size(12.0)
                                    .color(Color32::from_rgb(0, 230, 180))
                                    .monospace()
                                    .strong(),
                            );
                        });
                    });

                    ui.add_space(5.0);

                    // Add New Frequency Row
                    ui.horizontal(|ui| {
                        ui.add_sized([105.0, 24.0], egui::DragValue::new(&mut self.new_known_mhz).speed(0.025).suffix(" MHz"));
                        egui::ComboBox::from_id_salt("new_known_mode_box")
                            .selected_text(self.new_known_mode.name())
                            .width(55.0)
                            .show_ui(ui, |ui| {
                                for m in [
                                    DemodMode::WFM,
                                    DemodMode::NFM,
                                    DemodMode::AM,
                                    DemodMode::BCFM,
                                    DemodMode::USB,
                                    DemodMode::LSB,
                                ] {
                                    ui.selectable_value(&mut self.new_known_mode, m, m.name());
                                }
                            });
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let text_w = ui.available_width() - 65.0;
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_known_name)
                                .hint_text("Station / Tag (e.g. EFHK)...")
                                .desired_width(text_w),
                        );
                        let add_btn = ui.add_sized(
                            [58.0, 24.0],
                            egui::Button::new(RichText::new("➕ Add").size(11.5).strong().color(Color32::BLACK))
                                .fill(Color32::from_rgb(0, 220, 180)),
                        );
                        if add_btn.clicked() && !self.new_known_name.trim().is_empty() {
                            let freq_hz = (self.new_known_mhz * 1e6).round() as u64;
                            let name = self.new_known_name.trim().to_string();
                            let bm = Bookmark {
                                freq_hz,
                                name: name.clone(),
                                mode: self.new_known_mode,
                                bandwidth_hz: 12_500,
                            };
                            cfg.bookmarks.insert(freq_hz, bm.clone());
                            let _ = cfg.save();
                            let _ = cmd_tx.send(DspCommand::AddBookmark(bm));
                            self.new_known_name.clear();
                        }
                    });

                    ui.add_space(6.0);

                    // Filter search if many bookmarks
                    if cfg.bookmarks.len() > 4 {
                        ui.add_sized(
                            [ui.available_width(), 24.0],
                            egui::TextEdit::singleline(&mut self.known_freq_search)
                                .hint_text("🔍 Filter known channels..."),
                        );
                        ui.add_space(4.0);
                    }

                    // Scrollable list of recognized bookmarks (tall, spacious, clear)
                    let filter = self.known_freq_search.to_lowercase();
                    let mut sorted_bms: Vec<Bookmark> = cfg.bookmarks.values().cloned().collect();
                    sorted_bms.sort_by_key(|b| b.freq_hz);

                    let mut to_delete = None;
                    let known_h = if cfg.bookmarks.len() > 8 { 600.0 } else if cfg.bookmarks.len() > 3 { 420.0 } else { 260.0 };
                    egui::ScrollArea::vertical()
                        .id_salt("deck_known_freqs_scroll_area")
                        .max_height(known_h)
                        .min_scrolled_height(240.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for bm in sorted_bms {
                                if !filter.is_empty()
                                    && !bm.name.to_lowercase().contains(&filter)
                                    && !format!("{:.4}", bm.freq_hz as f64 / 1e6).contains(&filter)
                                {
                                    continue;
                                }
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                                    let mhz = bm.freq_hz as f64 / 1e6;
                                    let tune_btn = ui.add(
                                        egui::Button::new(
                                            RichText::new(format!("{:>7.3}", mhz))
                                                .size(13.0)
                                                .monospace()
                                                .color(Color32::from_rgb(0, 230, 255))
                                                .strong(),
                                        )
                                        .fill(Color32::from_rgb(20, 28, 40)),
                                    );
                                    if tune_btn.clicked() {
                                        let target_vfo = self.selected_vfo_id.unwrap_or(0);
                                        self.dial_freq_hz = bm.freq_hz;
                                        self.manual_mode = bm.mode;
                                        self.scanner_state = ScannerState::Paused;
                                        let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                                        let _ = cmd_tx.send(DspCommand::SetManualVfo(target_vfo, bm.freq_hz, bm.mode, bm.name.clone()));
                                    }
                                    tune_btn.on_hover_text(format!("Click to tune VFO to {:.3} MHz {}", mhz, bm.mode.name()));

                                    ui.label(
                                        RichText::new(bm.mode.name())
                                            .size(11.0)
                                            .monospace()
                                            .color(Color32::from_rgb(120, 180, 220))
                                            .strong(),
                                    );
                                    let tag_w = ui.available_width() - 32.0;
                                    ui.add_sized([tag_w, 22.0], egui::Label::new(RichText::new(&bm.name).size(13.0).color(Color32::WHITE).strong()).truncate());

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let del_btn = ui.add_sized(
                                            [26.0, 22.0],
                                            egui::Button::new(RichText::new("✕").size(11.0).color(Color32::from_rgb(220, 130, 130)))
                                                .fill(Color32::from_rgb(55, 30, 30)),
                                        );
                                        if del_btn.clicked() {
                                            to_delete = Some(bm.freq_hz);
                                        }
                                        del_btn.on_hover_text("Delete known frequency");
                                    });
                                });
                                ui.add_space(2.0);
                            }
                        });

                    if let Some(f) = to_delete {
                        cfg.bookmarks.remove(&f);
                        let _ = cfg.save();
                        let _ = cmd_tx.send(DspCommand::RemoveBookmark(f));
                    }
                });

            ui.add_space(6.0);

            // ==========================================
            // 7. SIGNAL BLACKLIST (Full Sidebar Width)
            // ==========================================
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 20, 26))
                .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(45, 32, 35)))
                .rounding(6.0)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🚫 IGNORED FREQUENCIES").size(13.0).color(Color32::from_rgb(255, 120, 100)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{}", cfg.blacklist.len()))
                                    .size(12.0)
                                    .color(Color32::from_rgb(255, 170, 130))
                                    .monospace()
                                    .strong(),
                            );
                        });
                    });

                    ui.add_space(5.0);

                    if cfg.blacklist.is_empty() {
                        ui.label(RichText::new("No frequencies blacklisted").size(12.0).color(Color32::from_rgb(140, 150, 160)).italics());
                    } else {
                        let mut to_remove = None;
                        let blacklist_h = if cfg.blacklist.len() > 6 { 500.0 } else if cfg.blacklist.len() > 2 { 350.0 } else { 220.0 };
                        egui::ScrollArea::vertical()
                            .id_salt("deck_blacklist_scroll_area")
                            .max_height(blacklist_h)
                            .min_scrolled_height(200.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for &f in &cfg.blacklist {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                                        let mhz = f as f64 / 1e6;
                                        ui.label(
                                            RichText::new(format!("{:>8.4} MHz", mhz))
                                                .size(13.5)
                                                .monospace()
                                                .color(Color32::from_rgb(255, 185, 140))
                                                .strong(),
                                        );

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let del_btn = ui.add_sized(
                                                [28.0, 22.0],
                                                egui::Button::new(RichText::new(" ✕ ").color(Color32::WHITE).size(11.0).strong())
                                                    .fill(Color32::from_rgb(140, 40, 40)),
                                            );
                                            if del_btn.clicked() {
                                                to_remove = Some(f);
                                            }
                                            del_btn.on_hover_text("Unblock frequency");
                                        });
                                    });
                                    ui.add_space(2.0);
                                }
                            });

                        if let Some(f) = to_remove {
                            cfg.remove_blacklist(f);
                            let _ = cmd_tx.send(DspCommand::RemoveBlacklist(f));
                        }

                        ui.add_space(5.0);

                        if ui.add_sized(
                            [ui.available_width(), 26.0],
                            egui::Button::new(RichText::new("🗑 Clear All Blocked").color(Color32::WHITE).size(11.5).strong())
                                .fill(Color32::from_rgb(120, 35, 35)),
                        ).clicked() {
                            let to_clear = cfg.blacklist.clone();
                            for f in to_clear {
                                cfg.remove_blacklist(f);
                                let _ = cmd_tx.send(DspCommand::RemoveBlacklist(f));
                            }
                        }
                    }
                });
        });
    }
}

/// Resolves the expected demodulation mode for a given frequency based on bookmarks, active band, or band plan heuristics
pub fn get_frequency_mode(freq_hz: u64, cfg: &ScannerConfig, active_range_name: &str) -> Option<DemodMode> {
    // 1. Exact or near bookmark
    if let Some(bm) = cfg.get_comment(freq_hz, 5000) {
        return Some(bm.mode);
    }
    // 2. Currently active profile if frequency is within its bounds
    if let Some(r) = cfg.ranges.iter().find(|r| r.name == active_range_name && freq_hz >= r.scan_start_hz && freq_hz <= r.scan_end_hz) {
        return Some(r.mode);
    }
    // 3. Any enabled profile covering this frequency
    if let Some(r) = cfg.ranges.iter().find(|r| r.enabled && freq_hz >= r.scan_start_hz && freq_hz <= r.scan_end_hz) {
        return Some(r.mode);
    }
    // 4. Any profile in config covering this frequency
    if let Some(r) = cfg.ranges.iter().find(|r| freq_hz >= r.scan_start_hz && freq_hz <= r.scan_end_hz) {
        return Some(r.mode);
    }
    // 5. Standard ITU / band plan heuristics
    if (118_000_000..=137_000_000).contains(&freq_hz) {
        return Some(DemodMode::AM);
    }
    if (87_500_000..=108_000_000).contains(&freq_hz) {
        return Some(DemodMode::BCFM);
    }
    None
}

impl ControlDeck {
    /// Renders tactical LCD backlit digital frequency dial with per-digit scroll wheel tuning
    fn render_frequency_dial(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DisplaySnapshot,
        cfg: &mut ScannerConfig,
        cmd_tx: &Sender<DspCommand>,
    ) {
        // Frequency to display
        let is_scanning = self.scanner_state == ScannerState::Scanning;
        let display_freq = if is_scanning {
            let f = if snapshot.scan_cursor_hz > 0 {
                snapshot.scan_cursor_hz
            } else {
                snapshot.center_freq_hz
            };
            self.dial_freq_hz = f;

            let cur_band = if !snapshot.active_range.is_empty() {
                snapshot.active_range.as_str()
            } else {
                cfg.active_range.as_str()
            };

            // Check if any VFO is currently active / locked on a signal
            let active_vfo = self
                .selected_vfo_id
                .and_then(|id| {
                    snapshot.vfos.iter().find(|v| {
                        v.id == id
                            && (v.state == VfoState::Pinned
                                || (v.state == VfoState::Locked && (v.active || v.delay_remaining_sec > 0.0))
                                || v.active)
                            && !v.is_out_of_band
                    })
                })
                .or_else(|| {
                    snapshot.vfos.iter().find(|v| {
                        (v.state == VfoState::Pinned
                            || (v.state == VfoState::Locked && (v.active || v.delay_remaining_sec > 0.0))
                            || v.active)
                            && !v.is_out_of_band
                    })
                });

            let mode = active_vfo
                .map(|v| v.mode)
                .or_else(|| get_frequency_mode(f, cfg, cur_band))
                .or_else(|| cfg.ranges.iter().find(|r| r.name == cur_band).map(|r| r.mode))
                .unwrap_or(self.manual_mode);

            self.manual_mode = mode;
            f
        } else {
            // Manual / paused tuning: stay locked to the selected VFO slot
            let target_id = self.selected_vfo_id.unwrap_or(0).min(cfg.max_vfos.saturating_sub(1));
            self.selected_vfo_id = Some(target_id);

            let active_vfo = snapshot.vfos.iter().find(|v| v.id == target_id);

            let now = std::time::Instant::now();
            let is_user_scrolling = now.duration_since(self.dial_last_step).as_millis() < 300;

            if let Some(v) = active_vfo {
                if !is_user_scrolling && v.freq_hz > 0 && (v.state == VfoState::Pinned || v.state == VfoState::Locked) {
                    self.dial_freq_hz = v.freq_hz;
                    self.manual_mode = v.mode;
                }
            } else if self.dial_freq_hz == 0 {
                self.dial_freq_hz = snapshot
                    .vfos
                    .first()
                    .map(|v| v.freq_hz)
                    .unwrap_or(snapshot.center_freq_hz);
                if self.dial_freq_hz == 0 {
                    self.dial_freq_hz = 103_923_500;
                }
            }
            let cur_band = if !snapshot.active_range.is_empty() {
                snapshot.active_range.as_str()
            } else {
                cfg.active_range.as_str()
            };
            if let Some(m) = get_frequency_mode(self.dial_freq_hz, cfg, cur_band) {
                if !is_user_scrolling {
                    self.manual_mode = m;
                }
            }
            self.dial_freq_hz
        };

        // Header badge
        ui.horizontal(|ui| {
            if is_scanning {
                let cur_band = if !snapshot.active_range.is_empty() {
                    snapshot.active_range.as_str()
                } else {
                    cfg.active_range.as_str()
                };
                let badge_text = if let Some(id) = self.selected_vfo_id {
                    let vfo_name = snapshot.vfos.iter().find(|v| v.id == id).map(|v| v.name.as_str()).unwrap_or("");
                    if !vfo_name.is_empty() {
                        format!("⚡ LOCKED: {} [{}]", vfo_name, self.manual_mode.name())
                    } else {
                        format!("⚡ SWEEP: {} [{}]", cur_band, self.manual_mode.name())
                    }
                } else {
                    format!("⚡ SWEEP: {} [{}]", cur_band, self.manual_mode.name())
                };
                ui.label(
                    RichText::new(badge_text)
                        .size(10.0)
                        .color(Color32::from_rgb(0, 240, 255))
                        .strong(),
                );
            } else {
                let badge_text = if let Some(id) = self.selected_vfo_id {
                    let vfo_name = snapshot.vfos.iter().find(|v| v.id == id).map(|v| v.name.as_str()).unwrap_or("");
                    if !vfo_name.is_empty() {
                        format!("🎯 VFO #{} ({}) [{}]", id, vfo_name, self.manual_mode.name())
                    } else {
                        format!("🎯 VFO #{} DIAL [{}]", id, self.manual_mode.name())
                    }
                } else {
                    format!("🎯 VFO DIAL [{}]", self.manual_mode.name())
                };
                ui.label(
                    RichText::new(badge_text)
                        .size(10.0)
                        .color(Color32::from_rgb(0, 255, 128))
                        .strong(),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new("SCROLL DIGIT TO TUNE")
                        .size(9.0)
                        .color(Color32::from_rgb(120, 150, 180))
                        .monospace(),
                );
            });
        });

        ui.add_space(3.0);

        // Digits breakdown
        let f = display_freq;
        let is_ghz = f >= 1_000_000_000;
        let ghz_digit = ((f / 1_000_000_000) % 10) as u8;
        let d100m = ((f / 100_000_000) % 10) as u8;
        let d10m = ((f / 10_000_000) % 10) as u8;
        let d1m = ((f / 1_000_000) % 10) as u8;
        let d100k = ((f / 100_000) % 10) as u8;
        let d10k = ((f / 10_000) % 10) as u8;
        let d1k = ((f / 1_000) % 10) as u8;
        let d100 = ((f / 100) % 10) as u8;

        struct DigitSlot {
            digit: u8,
            weight_hz: u64,
            tag: &'static str,
            label: &'static str,
        }

        let whole_digits: Vec<DigitSlot> = if is_ghz {
            vec![
                DigitSlot { digit: ghz_digit, weight_hz: 1_000_000_000, tag: "1G", label: "1000 MHz (1 GHz)" },
                DigitSlot { digit: d100m, weight_hz: 100_000_000, tag: "100M", label: "100 MHz" },
                DigitSlot { digit: d10m, weight_hz: 10_000_000, tag: "10M", label: "10 MHz" },
                DigitSlot { digit: d1m, weight_hz: 1_000_000, tag: "1M", label: "1 MHz" },
            ]
        } else {
            vec![
                DigitSlot { digit: d100m, weight_hz: 100_000_000, tag: "100M", label: "100 MHz" },
                DigitSlot { digit: d10m, weight_hz: 10_000_000, tag: "10M", label: "10 MHz" },
                DigitSlot { digit: d1m, weight_hz: 1_000_000, tag: "1M", label: "1 MHz" },
            ]
        };

        let frac_digits = [
            DigitSlot { digit: d100k, weight_hz: 100_000, tag: "100k", label: "100 kHz" },
            DigitSlot { digit: d10k, weight_hz: 10_000, tag: "10k", label: "10 kHz" },
            DigitSlot { digit: d1k, weight_hz: 1_000, tag: "1k", label: "1 kHz" },
            DigitSlot { digit: d100, weight_hz: 100, tag: "100", label: "100 Hz" },
        ];

        let now = std::time::Instant::now();
        let can_wheel_step = now.duration_since(self.dial_last_step).as_millis() >= 60;
        let mut did_step = false;

        let digit_width = if is_ghz { 25.0 } else { 29.5 };
        let digit_height = 48.0;

        // Frame enclosing the entire frequency display
        let frame_resp = egui::Frame::none()
            .fill(Color32::from_rgb(10, 14, 20))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(28, 44, 62)))
            .rounding(5.0)
            .inner_margin(egui::Margin::symmetric(6.0, 5.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);

                    // Helper closure to render one digit box
                    let mut render_digit = |ui: &mut egui::Ui, slot: &DigitSlot| -> Option<i64> {
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(digit_width, digit_height), egui::Sense::click());
                        let painter = ui.painter_at(rect);

                        let mut delta_steps: i64 = 0;
                        if resp.hovered() {
                            let mut wheel_y: f32 = 0.0;
                            ui.input(|i| {
                                for event in &i.events {
                                    if let egui::Event::MouseWheel { delta, .. } = event {
                                        wheel_y += delta.y;
                                    }
                                }
                            });

                            if wheel_y.abs() > 0.001 && can_wheel_step {
                                delta_steps = if wheel_y > 0.0 { 1 } else { -1 };
                                did_step = true;
                                // Consume scroll immediately so parent sidebar ScrollArea never moves!
                                ui.ctx().input_mut(|i| {
                                    i.raw_scroll_delta = egui::Vec2::ZERO;
                                    i.smooth_scroll_delta = egui::Vec2::ZERO;
                                    i.events.retain(|e| !matches!(e, egui::Event::MouseWheel { .. }));
                                });
                            }
                        }
                        if resp.clicked() {
                            delta_steps = 1;
                            did_step = true;
                        } else if resp.secondary_clicked() {
                            delta_steps = -1;
                            did_step = true;
                        }

                        // Background and border
                        let bg_col = if resp.hovered() {
                            Color32::from_rgb(20, 38, 58)
                        } else {
                            Color32::from_rgb(13, 18, 26)
                        };
                        let border_col = if resp.hovered() {
                            Color32::from_rgb(0, 230, 255)
                        } else {
                            Color32::from_rgb(28, 40, 55)
                        };

                        painter.rect_filled(rect, 4.0, bg_col);
                        painter.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, border_col));

                        // Indicator arrow when hovered
                        if resp.hovered() {
                            painter.text(
                                Pos2::new(rect.center().x, rect.min.y + 5.0),
                                egui::Align2::CENTER_CENTER,
                                "▲",
                                egui::FontId::proportional(8.0),
                                Color32::from_rgb(0, 220, 255),
                            );
                        }

                        // Big Bold Digit
                        let digit_col = if resp.hovered() {
                            Color32::from_rgb(255, 255, 120)
                        } else {
                            Color32::from_rgb(0, 240, 255)
                        };

                        painter.text(
                            Pos2::new(rect.center().x, rect.min.y + 20.0),
                            egui::Align2::CENTER_CENTER,
                            slot.digit.to_string(),
                            egui::FontId::monospace(21.0),
                            digit_col,
                        );

                        // Subtle Decade Tag (100M, 10M, 1M, 100k, 10k, 1k, 100)
                        let tag_col = if resp.hovered() {
                            Color32::from_rgb(0, 240, 255)
                        } else {
                            Color32::from_rgb(100, 130, 160)
                        };

                        painter.text(
                            Pos2::new(rect.center().x, rect.max.y - 7.0),
                            egui::Align2::CENTER_CENTER,
                            slot.tag,
                            egui::FontId::monospace(8.5),
                            tag_col,
                        );

                        resp.on_hover_text(format!(
                            "Scroll wheel: ±{}\nLeft click: +1 | Right click: -1",
                            slot.label
                        ));

                        if delta_steps != 0 {
                            Some(delta_steps)
                        } else {
                            None
                        }
                    };

                    let mut tune_change_hz: i64 = 0;

                    // Render Whole MHz digits
                    for slot in &whole_digits {
                        if let Some(steps) = render_digit(ui, slot) {
                            tune_change_hz += steps * slot.weight_hz as i64;
                        }
                    }

                    // Decimal point separator
                    let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, digit_height), egui::Sense::hover());
                    let dot_painter = ui.painter_at(dot_rect);
                    dot_painter.circle_filled(
                        Pos2::new(dot_rect.center().x, dot_rect.min.y + 26.0),
                        2.8,
                        Color32::from_rgb(0, 240, 255),
                    );

                    // Render Fractional digits
                    for slot in &frac_digits {
                        if let Some(steps) = render_digit(ui, slot) {
                            tune_change_hz += steps * slot.weight_hz as i64;
                        }
                    }

                    // Unit label "MHz" cleanly aligned directly after digits
                    let (mhz_rect, _) = ui.allocate_exact_size(egui::vec2(32.0, digit_height), egui::Sense::hover());
                    let mhz_painter = ui.painter_at(mhz_rect);
                    mhz_painter.text(
                        Pos2::new(mhz_rect.min.x + 2.0, mhz_rect.min.y + 21.0),
                        egui::Align2::LEFT_CENTER,
                        "MHz",
                        egui::FontId::monospace(13.0),
                        Color32::from_rgb(0, 220, 255),
                    );

                    // If user scrolled or clicked any digit
                    if tune_change_hz != 0 {
                        let new_freq = (display_freq as i64 + tune_change_hz).clamp(500_000, 2_000_000_000) as u64;
                        self.dial_freq_hz = new_freq;
                        self.scanner_state = ScannerState::Paused;
                        let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));

                        let target_vfo = self.selected_vfo_id.unwrap_or(0).min(cfg.max_vfos.saturating_sub(1));
                        let cur_band = if !snapshot.active_range.is_empty() {
                            snapshot.active_range.as_str()
                        } else {
                            cfg.active_range.as_str()
                        };
                        let mode = get_frequency_mode(new_freq, cfg, cur_band).unwrap_or(self.manual_mode);
                        self.manual_mode = mode;
                        let name = cfg
                            .get_comment(new_freq, 5000)
                            .map(|b| b.name.clone())
                            .unwrap_or_else(|| format!("{:.4} MHz", new_freq as f64 / 1e6));
                        let _ = cmd_tx.send(DspCommand::SetManualVfo(target_vfo, new_freq, mode, name));
                    }
                });
            });

        if did_step {
            self.dial_last_step = now;
        }

        // If pointer is anywhere over the dial frame, swallow scroll so outer ScrollArea never moves
        if ui.rect_contains_pointer(frame_resp.response.rect) {
            ui.ctx().input_mut(|i| {
                i.raw_scroll_delta = egui::Vec2::ZERO;
                i.smooth_scroll_delta = egui::Vec2::ZERO;
                i.events.retain(|e| !matches!(e, egui::Event::MouseWheel { .. }));
            });
        }
    }
}

/// Helper for drawing modern, wide, tactile sliders
fn tactical_slider(
    ui: &mut egui::Ui,
    val: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    accent_color: Color32,
) -> bool {
    let width = ui.available_width();
    let height = 24.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
    let min_val = *range.start();
    let max_val = *range.end();
    let span = (max_val - min_val).max(0.0001);

    let mut changed = false;
    if resp.dragged() || resp.clicked() {
        if let Some(mouse_pos) = resp.interact_pointer_pos() {
            let margin = 10.0;
            let norm = ((mouse_pos.x - (rect.min.x + margin)) / (rect.width() - margin * 2.0)).clamp(0.0, 1.0);
            let new_val = min_val + norm * span;
            if (*val - new_val).abs() > 0.0001 {
                *val = new_val;
                changed = true;
            }
        }
    }

    let norm = ((*val - min_val) / span).clamp(0.0, 1.0);
    let painter = ui.painter_at(rect);

    // Track outer groove
    let track_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, rect.min.y + 4.0),
        egui::pos2(rect.max.x, rect.max.y - 4.0),
    );
    painter.rect_filled(track_rect, 4.0, Color32::from_rgb(12, 16, 24));
    painter.rect_stroke(track_rect, 4.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(35, 48, 65)));

    // Active fill bar
    let fill_w = norm * (track_rect.width() - 8.0);
    if fill_w > 0.0 {
        let fill_rect = egui::Rect::from_min_max(
            egui::pos2(track_rect.min.x + 4.0, track_rect.min.y + 2.5),
            egui::pos2(track_rect.min.x + 4.0 + fill_w, track_rect.max.y - 2.5),
        );
        let fill_col = Color32::from_rgb(
            (accent_color.r() as f32 * 0.7) as u8,
            (accent_color.g() as f32 * 0.7) as u8,
            (accent_color.b() as f32 * 0.7) as u8,
        );
        painter.rect_filled(fill_rect, 2.5, fill_col);
    }

    // Draggable thumb/handle
    let knob_margin = 10.0;
    let knob_x = rect.min.x + knob_margin + norm * (rect.width() - knob_margin * 2.0);
    let knob_rect = egui::Rect::from_center_size(
        egui::pos2(knob_x, rect.center().y),
        egui::vec2(16.0, 20.0),
    );
    let knob_border = if resp.dragged() || resp.hovered() {
        Color32::WHITE
    } else {
        accent_color
    };
    painter.rect_filled(knob_rect, 3.0, Color32::from_rgb(28, 36, 48));
    painter.rect_stroke(knob_rect, 3.0, egui::Stroke::new(1.5_f32, knob_border));

    // Grip lines on thumb
    painter.line_segment(
        [egui::pos2(knob_x - 2.0, knob_rect.min.y + 5.0), egui::pos2(knob_x - 2.0, knob_rect.max.y - 5.0)],
        egui::Stroke::new(1.0_f32, knob_border),
    );
    painter.line_segment(
        [egui::pos2(knob_x + 2.0, knob_rect.min.y + 5.0), egui::pos2(knob_x + 2.0, knob_rect.max.y - 5.0)],
        egui::Stroke::new(1.0_f32, knob_border),
    );

    changed
}

/// Helper for discrete integer tactical sliders
fn tactical_slider_int(
    ui: &mut egui::Ui,
    val: &mut usize,
    min: usize,
    max: usize,
    accent_color: Color32,
) -> bool {
    let mut f = *val as f32;
    if tactical_slider(ui, &mut f, (min as f32)..=(max as f32), accent_color) {
        let rounded = f.round() as usize;
        let clamped = rounded.clamp(min, max);
        if *val != clamped {
            *val = clamped;
            return true;
        }
    }
    false
}

/// Helper for discrete u32 tactical sliders
fn tactical_slider_u32(
    ui: &mut egui::Ui,
    val: &mut u32,
    min: u32,
    max: u32,
    accent_color: Color32,
) -> bool {
    let mut f = *val as f32;
    if tactical_slider(ui, &mut f, (min as f32)..=(max as f32), accent_color) {
        let rounded = f.round() as u32;
        let clamped = rounded.clamp(min, max);
        if *val != clamped {
            *val = clamped;
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_frequency_dial_decade_stepping() {
        let base_freq = 123_000_000_u64; // 123.0000 MHz

        // Stepping 10 MHz decade by exactly 1 step
        let step_10m = 10_000_000_i64;
        let new_freq = (base_freq as i64 + step_10m) as u64;
        assert_eq!(new_freq, 133_000_000, "1 notch on 10 MHz must yield 133.0000 MHz, not 153.0000 MHz");

        // Stepping 1 MHz decade by exactly 1 step
        let step_1m = 1_000_000_i64;
        let new_freq_1m = (base_freq as i64 + step_1m) as u64;
        assert_eq!(new_freq_1m, 124_000_000, "1 notch on 1 MHz must yield 124.0000 MHz, not 126 or 141 MHz");

        // Stepping 100 kHz decade by exactly 1 step
        let step_100k = 100_000_i64;
        let new_freq_100k = (base_freq as i64 + step_100k) as u64;
        assert_eq!(new_freq_100k, 123_100_000, "1 notch on 100 kHz must yield 123.1000 MHz");

        // Stepping 1 kHz decade by exactly 1 step
        let step_1k = 1_000_i64;
        let new_freq_1k = (base_freq as i64 + step_1k) as u64;
        assert_eq!(new_freq_1k, 123_001_000, "1 notch on 1 kHz must yield 123.0010 MHz");
    }

    #[test]
    fn test_digit_decomposition_sub_ghz() {
        let f = 123_456_700_u64; // 123.4567 MHz
        let is_ghz = f >= 1_000_000_000;
        assert!(!is_ghz, "123 MHz is sub-GHz, must not show 1 GHz box");

        let d100m = ((f / 100_000_000) % 10) as u8;
        let d10m = ((f / 10_000_000) % 10) as u8;
        let d1m = ((f / 1_000_000) % 10) as u8;
        let d100k = ((f / 100_000) % 10) as u8;
        let d10k = ((f / 10_000) % 10) as u8;
        let d1k = ((f / 1_000) % 10) as u8;
        let d100 = ((f / 100) % 10) as u8;

        assert_eq!(d100m, 1);
        assert_eq!(d10m, 2);
        assert_eq!(d1m, 3);
        assert_eq!(d100k, 4);
        assert_eq!(d10k, 5);
        assert_eq!(d1k, 6);
        assert_eq!(d100, 7);
    }

    #[test]
    fn test_dial_tracks_active_vfo() {
        use crate::dsp_worker::{DisplaySnapshot, VfoDisplayInfo};
        use crate::scanner::vfo::VfoState;
        use crate::config::DemodMode;

        let deck = super::ControlDeck::new();
        assert_eq!(deck.selected_vfo_id, None);

        // Simulated snapshot where Air Traffic tuned 136.9750 MHz AM as VFO 0
        let snapshot = DisplaySnapshot {
            center_freq_hz: 136_975_000,
            sample_rate_hz: 10_000_000,
            raw_spectrum: vec![],
            pseudo_spectrum: vec![],
            noise_baseline: vec![],
            scan_cursor_hz: 0,
            vfos: vec![
                VfoDisplayInfo {
                    id: 0,
                    freq_hz: 136_975_000,
                    name: "VDL2 136.975".to_string(),
                    mode: DemodMode::AM,
                    state: VfoState::Pinned,
                    snr_db: 22.0,
                    vad_score: 0.9,
                    active: true,
                    is_out_of_band: false,
                    muted: true,
                    volume: 1.0,
                    delay_remaining_sec: 0.0,
                    dmr_info: None,
                    latest_telemetry_info: None,
                    audio_waveform: [0.0; 32],
                    squelch_open: true,
                }
            ],
            status_message: "Airband".to_string(),
            device_running: true,
            active_range: "Airband".to_string(),
            latest_aprs: None,
            aprs_packets: vec![],
            latest_dmr: None,
            dmr_packets: vec![],
            latest_telemetry: None,
            telemetry_packets: vec![],
            latest_air: None,
            air_packets: vec![],
            latest_ais: None,
            ais_packets: vec![],
            current_audio_device: "Default".to_string(),
            blacklist: vec![],
            vdl2_burst_stats: None,
        };

        // Active VFO lookup logic matches VFO 0
        let active_vfo = deck
            .selected_vfo_id
            .and_then(|id| snapshot.vfos.iter().find(|v| v.id == id && (v.state == VfoState::Pinned || v.active)))
            .or_else(|| snapshot.vfos.iter().find(|v| v.id == 0 && (v.state == VfoState::Pinned || v.active)));

        assert!(active_vfo.is_some());
        let v = active_vfo.unwrap();
        assert_eq!(v.freq_hz, 136_975_000);
        assert_eq!(v.mode, DemodMode::AM);
        assert_eq!(v.name, "VDL2 136.975");
    }

    #[test]
    fn test_get_frequency_mode_resolution() {
        use crate::config::{ScannerConfig, DemodMode};
        let cfg = ScannerConfig::default();

        // Airband frequency within "AIR" profile (118.1 - 127.0 MHz) -> AM
        let air_mode = super::get_frequency_mode(122_500_000, &cfg, "AIR");
        assert_eq!(air_mode, Some(DemodMode::AM), "122.5 MHz Airband must resolve to AM");

        // Broadcast FM band -> BCFM
        let fm_mode = super::get_frequency_mode(98_500_000, &cfg, "AIR");
        assert_eq!(fm_mode, Some(DemodMode::BCFM), "98.5 MHz FM Broadcast must resolve to BCFM");

        // PMR446 band -> NFM
        let pmr_mode = super::get_frequency_mode(446_050_000, &cfg, "AIR");
        assert_eq!(pmr_mode, Some(DemodMode::NFM), "446.050 MHz PMR446 must resolve to NFM");
    }
}
