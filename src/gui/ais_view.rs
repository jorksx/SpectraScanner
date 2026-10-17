use crate::config::DemodMode;
use crate::dsp::{AisPacket, AisStation};
use crate::dsp_worker::{DisplaySnapshot, DspCommand};
use crate::gui::slippy_map::{MapStyle, SlippyMapState, TileManager};
use crate::scanner::{vfo::VfoState, ScannerState};
use crossbeam_channel::Sender;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};
use std::collections::HashMap;
use std::time::Instant;

/// Finnish and Baltic Sea major ports and lighthouses to display on the maritime map
const PORTS: &[(&str, &str, f64, f64)] = &[
    ("HEL", "Helsinki Port (Vuosaari)", 60.218, 25.188),
    ("TKU", "Turku Port", 60.435, 22.220),
    ("KOT", "Kotka Port (Mussalo)", 60.433, 26.920),
    ("HGO", "Hanko Port", 59.822, 22.955),
    ("POR", "Pori (Mäntyluoto)", 61.595, 21.498),
    ("RAU", "Rauma Port", 61.127, 21.465),
    ("VAA", "Vaasa Port", 63.090, 21.530),
    ("KOK", "Kokkola Port", 63.840, 23.080),
    ("OUL", "Oulu Port", 65.000, 25.420),
    ("KMI", "Kemi Port (Ajos)", 65.670, 24.520),
    ("MHN", "Mariehamn Port", 60.095, 19.928),
    ("TLL", "Tallinn Port (Muuga)", 59.495, 24.970),
    ("STO", "Stockholm Port", 59.340, 18.120),
    ("GLL", "Harmaja Lighthouse", 60.105, 24.975),
    ("SUO", "Suomenlinna", 60.145, 24.985),
    ("POR", "Porkkala Lighthouse", 59.980, 24.400),
    ("RUS", "Porvoo (Sköldvik)", 60.310, 25.550),
    ("JAA", "Jussarö Lighthouse", 59.820, 23.570),
    ("BOG", "Bogskär Lighthouse", 59.503, 20.415),
    ("MAR", "Märket Lighthouse", 60.301, 19.131),
];

/// Represents persistent antenna coverage reception profile for maritime AIS
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AisCoverageData {
    pub center_qth: (f64, f64),
    /// Max distance in km per azimuth bin (72 bins, 5 degrees each)
    pub azimuth_max_km: Vec<f32>,
    pub max_range_km: f32,
    pub total_ais_packets: u64,
}

impl Default for AisCoverageData {
    fn default() -> Self {
        Self {
            center_qth: (60.20, 24.90),
            azimuth_max_km: vec![15.0; 72], // 15 km baseline reception area
            max_range_km: 15.0,
            total_ais_packets: 0,
        }
    }
}

pub fn get_ais_coverage_storage_path() -> std::path::PathBuf {
    let local = std::path::PathBuf::from("ais_coverage.json");
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("ais_coverage.json");
        }
    }
    local
}

pub fn save_ais_coverage(data: &AisCoverageData) {
    let path = get_ais_coverage_storage_path();
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = std::fs::write(path, json);
    }
}

pub fn load_ais_coverage() -> AisCoverageData {
    let path = get_ais_coverage_storage_path();
    if let Ok(content) = std::fs::read_to_string(path) {
        if let Ok(data) = serde_json::from_str::<AisCoverageData>(&content) {
            if data.azimuth_max_km.len() == 72 {
                return data;
            }
        }
    }
    AisCoverageData::default()
}

/// Marine Traffic & AIS VHF Radar Monitoring View
pub struct AisView {
    pub is_open: bool,
    pub filter: String,
    pub selected_mmsi: Option<u32>,
    pub center_override: Option<(f64, f64)>,
    pub center_qth: (f64, f64),
    pub edit_qth_lat: String,
    pub edit_qth_lon: String,
    pub saved_vessels: HashMap<u32, AisStation>,
    pub history_packets: Vec<AisPacket>,
    pub show_ports: bool,
    pub show_trails: bool,
    pub show_coverage: bool,
    pub map_state: SlippyMapState,
    pub tile_mgr: TileManager,
    pub coverage: AisCoverageData,
    pub coverage_dirty: bool,
}

impl Default for AisView {
    fn default() -> Self {
        let qth = crate::gui::aprs_view::load_user_qth().unwrap_or((60.20, 24.90));
        let mut cov = load_ais_coverage();
        cov.center_qth = qth;
        Self {
            is_open: false,
            filter: String::new(),
            selected_mmsi: None,
            center_override: None,
            center_qth: qth,
            edit_qth_lat: format!("{:.4}", qth.0),
            edit_qth_lon: format!("{:.4}", qth.1),
            saved_vessels: HashMap::new(),
            history_packets: Vec::new(),
            show_ports: true,
            show_trails: true,
            show_coverage: true,
            map_state: SlippyMapState::new(qth.0, qth.1, 8.0),
            tile_mgr: TileManager::new(),
            coverage: cov,
            coverage_dirty: false,
        }
    }
}

impl AisView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_qth(&mut self, qth: (f64, f64)) {
        self.center_qth = qth;
        self.edit_qth_lat = format!("{:.4}", qth.0);
        self.edit_qth_lon = format!("{:.4}", qth.1);
        self.coverage.center_qth = qth;
        save_ais_coverage(&self.coverage);
    }

    /// Ingests newly decoded AIS packets from DisplaySnapshot
    pub fn ingest_packets(&mut self, packets: &[AisPacket]) {
        let now = Instant::now();

        // 1-hour timeout: Remove vessels not heard for > 3600 seconds (1 hour)
        self.saved_vessels.retain(|_, station| {
            now.saturating_duration_since(station.last_seen).as_secs() < 3600
        });

        // Evict any invalid MMSI or out-of-range vessels (e.g. from noise or previous glitches)
        self.saved_vessels.retain(|mmsi, station| {
            if !crate::dsp::ais::is_valid_mmsi(*mmsi) {
                return false;
            }
            if let (Some(la), Some(lo)) = (station.latitude, station.longitude) {
                let (dist_km, _, _) = crate::gui::aprs_view::calculate_distance_bearing(
                    self.center_qth.0,
                    self.center_qth.1,
                    la,
                    lo,
                );
                if dist_km > 600.0 {
                    return false;
                }
            }
            true
        });

        for p in packets {
            let mmsi = p.mmsi;
            if !crate::dsp::ais::is_valid_mmsi(mmsi) {
                continue;
            }

            // Terrestrial VHF line-of-sight sanity check: cannot exceed 600 km from QTH
            if let (Some(la), Some(lo)) = (p.latitude, p.longitude) {
                let (dist_km, _, _) = crate::gui::aprs_view::calculate_distance_bearing(
                    self.center_qth.0,
                    self.center_qth.1,
                    la,
                    lo,
                );
                if dist_km > 600.0 {
                    continue;
                }
            }

            // Check if packet was already ingested into history
            let is_new = !self.history_packets.iter().any(|h| h.timestamp == p.timestamp && h.mmsi == p.mmsi && h.channel == p.channel && h.nmea_sentence == p.nmea_sentence);
            if !is_new {
                continue;
            }

            self.history_packets.push(p.clone());
            if self.history_packets.len() > 600 {
                self.history_packets.remove(0);
            }

            if let Some(station) = self.saved_vessels.get_mut(&mmsi) {
                station.last_seen = now;
                station.last_timestamp = p.timestamp.clone();
                station.packet_count += 1;
                station.latest_summary = p.human_summary.clone();
                if let Some(name) = &p.name {
                    station.name = Some(name.clone());
                }
                if let Some(call) = &p.callsign {
                    station.callsign = Some(call.clone());
                }
                if let Some(vt) = &p.vessel_type {
                    station.vessel_type = Some(vt.clone());
                }
                if let Some(ns) = &p.nav_status {
                    station.nav_status = Some(ns.clone());
                }
                if let Some(sog) = p.sog_knots {
                    if sog <= 102.2 {
                        station.sog_knots = Some(sog);
                    }
                }
                if let Some(cog) = p.cog_deg {
                    station.cog_deg = Some(cog);
                }
                if let Some(h) = p.heading_deg {
                    station.heading_deg = Some(h);
                }
                if let Some(dest) = &p.destination {
                    station.destination = Some(dest.clone());
                }
                if let Some(dr) = p.draught_m {
                    station.draught_m = Some(dr);
                }
                if let Some(l) = p.length_m {
                    station.length_m = Some(l);
                    station.beam_m = p.beam_m;
                }
                if let (Some(la), Some(lo)) = (p.latitude, p.longitude) {
                    station.latitude = Some(la);
                    station.longitude = Some(lo);
                    // Append to snail tail track history with jump filter
                    let should_push = match station.track_history.last() {
                        Some(&(last_la, last_lo)) => {
                            let d_lat = (last_la - la).abs();
                            let d_lon = (last_lo - lo).abs();
                            (d_lat > 0.0002 || d_lon > 0.0002) && (d_lat < 1.5 && d_lon < 2.5)
                        }
                        None => true,
                    };
                    if should_push {
                        station.track_history.push((la, lo));
                        if station.track_history.len() > 200 {
                            station.track_history.remove(0);
                        }
                    }

                    // Update live antenna coverage overlay
                    let (dist_km, bearing_deg, _) = crate::gui::aprs_view::calculate_distance_bearing(
                        self.center_qth.0,
                        self.center_qth.1,
                        la,
                        lo,
                    );
                    let bin = (((bearing_deg % 360.0) / 5.0).floor() as usize) % 72;
                    let dist_f32 = dist_km as f32;
                    if dist_f32 > self.coverage.azimuth_max_km[bin] && dist_f32 <= 450.0 {
                        self.coverage.azimuth_max_km[bin] = dist_f32;
                        self.coverage.max_range_km = self.coverage.max_range_km.max(dist_f32);
                        self.coverage.total_ais_packets = self.coverage.total_ais_packets.saturating_add(1);
                        self.coverage_dirty = true;
                    }
                }
            } else {
                let mut track = Vec::new();
                if let (Some(la), Some(lo)) = (p.latitude, p.longitude) {
                    track.push((la, lo));
                    let (dist_km, bearing_deg, _) = crate::gui::aprs_view::calculate_distance_bearing(
                        self.center_qth.0,
                        self.center_qth.1,
                        la,
                        lo,
                    );
                    let bin = (((bearing_deg % 360.0) / 5.0).floor() as usize) % 72;
                    let dist_f32 = dist_km as f32;
                    if dist_f32 > self.coverage.azimuth_max_km[bin] && dist_f32 <= 450.0 {
                        self.coverage.azimuth_max_km[bin] = dist_f32;
                        self.coverage.max_range_km = self.coverage.max_range_km.max(dist_f32);
                        self.coverage.total_ais_packets = self.coverage.total_ais_packets.saturating_add(1);
                        self.coverage_dirty = true;
                    }
                }

                let station = AisStation {
                    mmsi,
                    name: p.name.clone(),
                    callsign: p.callsign.clone(),
                    country: p.country.clone(),
                    vessel_type: p.vessel_type.clone(),
                    nav_status: p.nav_status.clone(),
                    latitude: p.latitude,
                    longitude: p.longitude,
                    sog_knots: p.sog_knots,
                    cog_deg: p.cog_deg,
                    heading_deg: p.heading_deg,
                    destination: p.destination.clone(),
                    draught_m: p.draught_m,
                    length_m: p.length_m,
                    beam_m: p.beam_m,
                    last_seen: now,
                    last_timestamp: p.timestamp.clone(),
                    packet_count: 1,
                    latest_summary: p.human_summary.clone(),
                    track_history: track,
                };
                self.saved_vessels.insert(mmsi, station);
            }
        }

        // Save coverage if updated
        if self.coverage_dirty {
            save_ais_coverage(&self.coverage);
            self.coverage_dirty = false;
        }
    }

    /// Renders the complete Marine Traffic Radar & AIS VHF Decoder window
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        snapshot: &DisplaySnapshot,
        cmd_tx: &Sender<DspCommand>,
        scanner_state: &mut ScannerState,
    ) {
        self.ingest_packets(&snapshot.ais_packets);

        if !self.is_open {
            return;
        }

        self.tile_mgr.update(ctx);

        // Main UI Window
        let mut is_open = self.is_open;
        egui::Window::new("🚢 Marine Traffic Radar & AIS VHF Decoder")
            .open(&mut is_open)
            .default_size([1120.0, 750.0])
            .min_size([720.0, 520.0])
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                self.render_contents(ui, snapshot, cmd_tx, scanner_state);
            });
        self.is_open = is_open;
    }

    /// Renders the top toolbar, split interactive map, and decoded packet table
    fn render_contents(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DisplaySnapshot,
        cmd_tx: &Sender<DspCommand>,
        scanner_state: &mut ScannerState,
    ) {
        // Top Toolbar
        ui.horizontal_wrapped(|ui| {
            ui.heading(RichText::new("🚢 MARINE AIS RADAR").size(15.0).strong().color(Color32::from_rgb(0, 240, 180)));
            ui.separator();

            let active_targets = self.saved_vessels.values().filter(|s| s.latitude.is_some()).count();
            ui.label(RichText::new(format!("Vessels: {}", self.saved_vessels.len())).size(12.0).strong());
            ui.label(RichText::new(format!("Mapped: {}", active_targets)).size(12.0).color(Color32::from_rgb(0, 240, 180)));
            ui.label(RichText::new(format!("Max Range: {:.1} km", self.coverage.max_range_km)).size(12.0).color(Color32::from_rgb(255, 205, 50)));

            ui.separator();

            // Quick Marine VHF Tuning Buttons
            let is_ais_1_active = snapshot.vfos.iter().any(|v| v.freq_hz == 161_975_000 && (v.active || v.state == VfoState::Pinned));
            let is_ais_2_active = snapshot.vfos.iter().any(|v| v.freq_hz == 162_025_000 && (v.active || v.state == VfoState::Pinned));
            let is_ch16_active = snapshot.vfos.iter().any(|v| v.freq_hz == 156_800_000 && (v.active || v.state == VfoState::Pinned));
            let is_dsc_active = snapshot.vfos.iter().any(|v| v.freq_hz == 156_525_000 && (v.active || v.state == VfoState::Pinned));
            let is_vts_active = snapshot.vfos.iter().any(|v| v.freq_hz == 156_600_000 && (v.active || v.state == VfoState::Pinned));

            // Concurrent channels indicator
            let active_marine_count = snapshot.vfos.iter().filter(|v| (v.state == VfoState::Pinned || v.active) && v.freq_hz >= 156_000_000 && v.freq_hz <= 162_100_000).count();
            if active_marine_count > 1 {
                ui.label(
                    RichText::new(format!("⚡ {} CHANNELS ACTIVE", active_marine_count))
                        .size(11.0)
                        .color(Color32::from_rgb(0, 255, 136))
                        .strong(),
                );
            }

            // 161.975 MHz (AIS 1 - Ch 87B)
            let btn_ais1_txt = if is_ais_1_active { "● 161.975 AIS 1" } else { "161.975 AIS 1" };
            let btn_ais1_bg = if is_ais_1_active { Color32::from_rgb(0, 230, 180) } else { Color32::from_rgb(18, 38, 34) };
            let btn_ais1_fg = if is_ais_1_active { Color32::BLACK } else { Color32::from_rgb(160, 240, 210) };
            let btn_ais1 = ui.add(
                egui::Button::new(RichText::new(btn_ais1_txt).size(11.0).strong().color(btn_ais1_fg))
                    .fill(btn_ais1_bg),
            ).on_hover_text("Toggle Marine AIS Channel 1 (161.975 MHz 9600 baud GMSK)");
            if btn_ais1.clicked() {
                Self::toggle_channel(cmd_tx, scanner_state, snapshot, 161_975_000, DemodMode::NFM, "AIS 1 (161.975)", false, is_ais_1_active);
            }

            // 162.025 MHz (AIS 2 - Ch 88B)
            let btn_ais2_txt = if is_ais_2_active { "● 162.025 AIS 2" } else { "162.025 AIS 2" };
            let btn_ais2_bg = if is_ais_2_active { Color32::from_rgb(0, 230, 180) } else { Color32::from_rgb(18, 38, 34) };
            let btn_ais2_fg = if is_ais_2_active { Color32::BLACK } else { Color32::from_rgb(160, 240, 210) };
            let btn_ais2 = ui.add(
                egui::Button::new(RichText::new(btn_ais2_txt).size(11.0).strong().color(btn_ais2_fg))
                    .fill(btn_ais2_bg),
            ).on_hover_text("Toggle Marine AIS Channel 2 (162.025 MHz 9600 baud GMSK)");
            if btn_ais2.clicked() {
                Self::toggle_channel(cmd_tx, scanner_state, snapshot, 162_025_000, DemodMode::NFM, "AIS 2 (162.025)", false, is_ais_2_active);
            }

            // 156.800 MHz (VHF Ch 16 - Distress & Calling)
            let btn_ch16_txt = if is_ch16_active { "● 156.800 Ch 16" } else { "156.800 Ch 16" };
            let btn_ch16_bg = if is_ch16_active { Color32::from_rgb(255, 80, 80) } else { Color32::from_rgb(48, 20, 22) };
            let btn_ch16_fg = if is_ch16_active { Color32::BLACK } else { Color32::from_rgb(255, 140, 140) };
            let btn_ch16 = ui.add(
                egui::Button::new(RichText::new(btn_ch16_txt).size(11.0).strong().color(btn_ch16_fg))
                    .fill(btn_ch16_bg),
            ).on_hover_text("Toggle International Maritime Distress & Calling VHF Ch 16 (156.800 MHz FM - Voice)");
            if btn_ch16.clicked() {
                Self::toggle_channel(cmd_tx, scanner_state, snapshot, 156_800_000, DemodMode::NFM, "VHF Ch 16 (156.800)", true, is_ch16_active);
            }

            // 156.525 MHz (VHF Ch 70 - DSC Digital)
            let btn_dsc_txt = if is_dsc_active { "● 156.525 Ch 70 DSC" } else { "156.525 Ch 70 DSC" };
            let btn_dsc_bg = if is_dsc_active { Color32::from_rgb(255, 190, 40) } else { Color32::from_rgb(42, 34, 15) };
            let btn_dsc_fg = if is_dsc_active { Color32::BLACK } else { Color32::from_rgb(255, 210, 100) };
            let btn_dsc = ui.add(
                egui::Button::new(RichText::new(btn_dsc_txt).size(11.0).strong().color(btn_dsc_fg))
                    .fill(btn_dsc_bg),
            ).on_hover_text("Toggle Digital Selective Calling VHF Ch 70 (156.525 MHz)");
            if btn_dsc.clicked() {
                Self::toggle_channel(cmd_tx, scanner_state, snapshot, 156_525_000, DemodMode::NFM, "VHF Ch 70 (156.525)", false, is_dsc_active);
            }

            // 156.600 MHz (VTS / Helsinki Traffic)
            let btn_vts_txt = if is_vts_active { "● 156.600 VTS" } else { "156.600 VTS" };
            let btn_vts_bg = if is_vts_active { Color32::from_rgb(80, 200, 255) } else { Color32::from_rgb(18, 32, 45) };
            let btn_vts_fg = if is_vts_active { Color32::BLACK } else { Color32::from_rgb(160, 215, 255) };
            let btn_vts = ui.add(
                egui::Button::new(RichText::new(btn_vts_txt).size(11.0).color(btn_vts_fg))
                    .fill(btn_vts_bg),
            ).on_hover_text("Toggle Port Operations / VTS VHF Ch 12 (156.600 MHz FM - Voice)");
            if btn_vts.clicked() {
                Self::toggle_channel(cmd_tx, scanner_state, snapshot, 156_600_000, DemodMode::NFM, "VTS Ch 12 (156.600)", true, is_vts_active);
            }

            // Auto-Scan Marine VHF Button
            let is_scanning = *scanner_state == ScannerState::Scanning;
            let (scan_txt, scan_col) = if is_scanning {
                ("⏸ PAUSE SCAN", Color32::from_rgb(0, 200, 235))
            } else {
                ("📡 SCAN MARINE", Color32::from_rgb(255, 190, 40))
            };
            let btn_scan = ui.add(
                egui::Button::new(RichText::new(scan_txt).size(11.0).strong().color(Color32::BLACK))
                    .fill(scan_col),
            ).on_hover_text("Automatically sweep full Marine VHF band 156.000 - 162.025 MHz NFM (25 kHz step)");
            if btn_scan.clicked() {
                if is_scanning {
                    *scanner_state = ScannerState::Paused;
                    let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                } else {
                    *scanner_state = ScannerState::Scanning;
                    let _ = cmd_tx.send(DspCommand::SetScanProfile {
                        name: "MARINE (VHF Meriradio)".to_string(),
                        center_freq_hz: 159_000_000,
                        scan_start_hz: 156_000_000,
                        scan_end_hz: 162_025_000,
                        scan_step_hz: 25_000,
                        mode: DemodMode::NFM,
                    });
                    let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Scanning));
                }
            }

            ui.separator();

            // Layer Toggles
            ui.toggle_value(&mut self.show_ports, "⚓ Ports & Lights");
            ui.toggle_value(&mut self.show_trails, "〰 Trails");
            ui.toggle_value(&mut self.show_coverage, "📡 Coverage");

            ui.separator();

            // Map Style selector
            egui::ComboBox::from_id_salt("ais_map_style")
                .selected_text(self.map_state.style.name())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.map_state.style, MapStyle::DarkTactical, "Dark Tactical");
                    ui.selectable_value(&mut self.map_state.style, MapStyle::OpenStreetMap, "OpenStreetMap");
                });

            ui.separator();

            // User QTH (Base Station Coordinates)
            ui.label(RichText::new("QTH:").size(11.0).strong());
            let lat_resp = ui.add(egui::TextEdit::singleline(&mut self.edit_qth_lat).desired_width(60.0));
            let lon_resp = ui.add(egui::TextEdit::singleline(&mut self.edit_qth_lon).desired_width(60.0));
            if lat_resp.lost_focus() || lon_resp.lost_focus() {
                if let (Ok(lat), Ok(lon)) = (self.edit_qth_lat.parse::<f64>(), self.edit_qth_lon.parse::<f64>()) {
                    if (lat - self.center_qth.0).abs() > 0.0001 || (lon - self.center_qth.1).abs() > 0.0001 {
                        self.set_qth((lat, lon));
                    }
                }
            }
        });

        ui.add_space(4.0);

        // Split view: Interactive Slippy Map (top 58%) and Decoded Traffic Table (bottom 42%)
        let available_height = ui.available_height();
        let map_height = (available_height * 0.58).clamp(240.0, 650.0);

        ui.allocate_ui(egui::vec2(ui.available_width(), map_height), |ui| {
            self.render_map(ui, snapshot);
        });

        ui.separator();

        // Lower: Decoded Marine Traffic Table
        self.render_table(ui);
    }

    /// Toggles a specific marine frequency into a dedicated monitoring VFO (up to max concurrent channels)
    fn toggle_channel(
        cmd_tx: &Sender<DspCommand>,
        scanner_state: &mut ScannerState,
        snapshot: &DisplaySnapshot,
        target_freq: u64,
        mode: DemodMode,
        name: &str,
        is_voice: bool,
        is_currently_active: bool,
    ) {
        *scanner_state = ScannerState::Paused;
        let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));

        // Center SDR frequency at 159.000 MHz so ALL Marine frequencies (156.0 - 162.025 MHz) fit simultaneously in 10 MHz passband!
        let half_rate = (snapshot.sample_rate_hz / 2) as u64;
        let min_f = snapshot.center_freq_hz.saturating_sub(half_rate);
        let max_f = snapshot.center_freq_hz + half_rate;
        if target_freq < min_f || target_freq > max_f || snapshot.center_freq_hz < 155_000_000 || snapshot.center_freq_hz > 163_000_000 {
            let _ = cmd_tx.send(DspCommand::SetCenterFreq(159_000_000));
        }

        let mut current_channels: Vec<(u64, DemodMode, String, bool)> = snapshot
            .vfos
            .iter()
            .filter(|v| (v.state == VfoState::Pinned || v.active) && !v.is_out_of_band && v.freq_hz >= 156_000_000 && v.freq_hz <= 162_100_000)
            .map(|v| {
                let voice = v.freq_hz == 156_800_000 || v.freq_hz == 156_600_000;
                (v.freq_hz, v.mode, v.name.clone(), voice)
            })
            .collect();

        if is_currently_active {
            current_channels.retain(|c| c.0 != target_freq);
        } else {
            current_channels.retain(|c| c.0 != target_freq);
            current_channels.push((target_freq, mode, name.to_string(), is_voice));
        }

        let _ = cmd_tx.send(DspCommand::TuneMultiChannels(current_channels));
    }

    /// Renders the interactive 2D Slippy Map with vessel icons, headings, snail tails, and coverage
    fn render_map(&mut self, ui: &mut egui::Ui, _snapshot: &DisplaySnapshot) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        // Center override on selected vessel
        if let Some((c_lat, c_lon)) = self.center_override {
            self.map_state.center_lat = c_lat;
            self.map_state.center_lon = c_lon;
            self.center_override = None;
        }

        // Handle panning and zooming
        self.map_state.handle_interaction(ui, rect, &response);

        // Render base map tiles
        self.map_state.render_tiles(&painter, rect, &mut self.tile_mgr);

        let to_pos = |lat: f64, lon: f64| -> Pos2 {
            self.map_state.lat_lon_to_screen(lat, lon, rect)
        };

        // QTH Center Marker (Anchor / Home)
        let qth_pos = to_pos(self.center_qth.0, self.center_qth.1);
        painter.circle_filled(qth_pos, 4.5, Color32::from_rgb(0, 255, 180));
        painter.circle_stroke(qth_pos, 8.0, Stroke::new(1.5_f32, Color32::from_rgb(0, 255, 180)));
        painter.text(
            Pos2::new(qth_pos.x + 10.0, qth_pos.y),
            egui::Align2::LEFT_CENTER,
            "QTH (My Station)",
            egui::FontId::proportional(10.5),
            Color32::from_rgb(0, 255, 180),
        );

        // Draw Maritime Coverage Polygon
        if self.show_coverage && self.coverage.azimuth_max_km.len() == 72 {
            let mut poly_pts = Vec::with_capacity(72);
            for i in 0..72 {
                let bearing = (i as f64) * 5.0;
                let dist = self.coverage.azimuth_max_km[i] as f64;
                let pt = crate::gui::air_view::destination_point(self.center_qth.0, self.center_qth.1, dist, bearing);
                poly_pts.push(to_pos(pt.0, pt.1));
            }
            if poly_pts.len() >= 3 {
                let fill_col = match self.map_state.style {
                    MapStyle::DarkTactical => Color32::from_rgba_unmultiplied(0, 240, 160, 22),
                    _ => Color32::from_rgba_unmultiplied(0, 120, 210, 30),
                };
                let stroke_col = match self.map_state.style {
                    MapStyle::DarkTactical => Color32::from_rgba_unmultiplied(0, 255, 180, 120),
                    _ => Color32::from_rgba_unmultiplied(0, 100, 200, 140),
                };
                painter.add(egui::Shape::convex_polygon(poly_pts.clone(), fill_col, Stroke::new(1.2_f32, stroke_col)));
            }
        }

        // Draw Major Ports & Lighthouses
        if self.show_ports {
            for &(code, name, lat, lon) in PORTS {
                let pos = to_pos(lat, lon);
                if rect.contains(pos) {
                    painter.circle_filled(pos, 3.2, Color32::from_rgb(255, 200, 50));
                    painter.text(
                        Pos2::new(pos.x + 5.0, pos.y - 3.0),
                        egui::Align2::LEFT_CENTER,
                        format!("⚓ {} {}", code, name),
                        egui::FontId::proportional(9.5),
                        match self.map_state.style {
                            MapStyle::DarkTactical => Color32::from_rgb(255, 220, 120),
                            _ => Color32::from_rgb(140, 90, 10),
                        },
                    );
                }
            }
        }

        // Draw Tracked Vessels
        let now = Instant::now();
        for (&mmsi, st) in &self.saved_vessels {
            let is_selected = self.selected_mmsi == Some(mmsi);

            // Snail tail trajectory
            if self.show_trails && st.track_history.len() > 1 {
                let total_pts = st.track_history.len();
                for (idx, window) in st.track_history.windows(2).enumerate() {
                    let p1 = to_pos(window[0].0, window[0].1);
                    let p2 = to_pos(window[1].0, window[1].1);
                    if rect.contains(p1) || rect.contains(p2) {
                        let progress = (idx + 1) as f32 / total_pts as f32;
                        let alpha = ((progress * 180.0) + 75.0).clamp(50.0, 255.0) as u8;
                        let stroke_col = if is_selected {
                            Color32::from_rgba_unmultiplied(255, 215, 0, alpha)
                        } else {
                            match self.map_state.style {
                                MapStyle::DarkTactical => Color32::from_rgba_unmultiplied(0, 240, 180, alpha),
                                _ => Color32::from_rgba_unmultiplied(0, 110, 220, alpha),
                            }
                        };
                        let stroke_width = if is_selected { 2.4_f32 } else { 1.5_f32 };
                        painter.line_segment([p1, p2], Stroke::new(stroke_width, stroke_col));
                    }
                }
            }

            if let (Some(lat), Some(lon)) = (st.latitude, st.longitude) {
                let pos = to_pos(lat, lon);
                if rect.contains(pos) {
                    let age_sec = now.saturating_duration_since(st.last_seen).as_secs();

                    // Color code by vessel category
                    let base_col = if is_selected {
                        Color32::from_rgb(255, 220, 0)
                    } else if let Some(vt) = &st.vessel_type {
                        if vt.contains("Passenger") || vt.contains("Ferry") {
                            Color32::from_rgb(0, 230, 140) // Emerald green
                        } else if vt.contains("Cargo") || vt.contains("Tanker") {
                            Color32::from_rgb(255, 140, 20) // Orange / amber
                        } else if vt.contains("Tug") || vt.contains("Pilot") {
                            Color32::from_rgb(255, 215, 0) // Golden yellow
                        } else if vt.contains("Search and Rescue") || vt.contains("Law Enforcement") || vt.contains("Military") {
                            Color32::from_rgb(255, 60, 60) // Emergency red
                        } else if vt.contains("Sailing") || vt.contains("Pleasure") {
                            Color32::from_rgb(0, 190, 255) // Cyan / light blue
                        } else if vt.contains("Fishing") {
                            Color32::from_rgb(230, 80, 220) // Magenta
                        } else {
                            Color32::from_rgb(80, 180, 240)
                        }
                    } else {
                        Color32::from_rgb(80, 180, 240)
                    };

                    let final_col = if age_sec > 600 {
                        Color32::from_rgb(
                            (base_col.r() as f32 * 0.6) as u8,
                            (base_col.g() as f32 * 0.6) as u8,
                            (base_col.b() as f32 * 0.6) as u8,
                        )
                    } else {
                        base_col
                    };

                    // Draw Directional Ship Hull Polygon if heading or COG available
                    let orientation = st.heading_deg.or(st.cog_deg);
                    if let Some(angle_deg) = orientation {
                        let rad = (angle_deg - 90.0).to_radians();
                        let dir = egui::vec2(rad.cos(), rad.sin());
                        let normal = egui::vec2(-dir.y, dir.x);

                        // Ship hull shape: pointed bow (p_bow), port beam (p_port), starboard beam (p_star), stern
                        let length = 14.0_f32;
                        let beam = 6.0_f32;
                        let p_bow = pos + dir * (length * 0.6);
                        let p_port_bow = pos + dir * (length * 0.1) + normal * (beam * 0.5);
                        let p_star_bow = pos + dir * (length * 0.1) - normal * (beam * 0.5);
                        let p_port_stern = pos - dir * (length * 0.4) + normal * (beam * 0.45);
                        let p_star_stern = pos - dir * (length * 0.4) - normal * (beam * 0.45);

                        // Draw drop shadow
                        let shadow_offset = egui::vec2(1.5, 1.5);
                        painter.add(egui::Shape::convex_polygon(
                            vec![
                                p_bow + shadow_offset,
                                p_port_bow + shadow_offset,
                                p_port_stern + shadow_offset,
                                p_star_stern + shadow_offset,
                                p_star_bow + shadow_offset,
                            ],
                            Color32::from_rgba_premultiplied(10, 15, 25, 180),
                            Stroke::NONE,
                        ));

                        // Draw hull
                        painter.add(egui::Shape::convex_polygon(
                            vec![p_bow, p_port_bow, p_port_stern, p_star_stern, p_star_bow],
                            final_col,
                            Stroke::new(1.2_f32, Color32::from_rgb(10, 20, 30)),
                        ));

                        // Speed vector line ahead of bow (proportional to SOG)
                        if let Some(sog) = st.sog_knots {
                            if sog > 0.5 {
                                let line_len = (sog * 1.5).clamp(8.0, 35.0);
                                painter.line_segment([p_bow, p_bow + dir * line_len], Stroke::new(1.5_f32, final_col));
                            }
                        }
                    } else {
                        // Stationary / unknown heading circle with center dot
                        painter.circle_filled(pos, 5.0, final_col);
                        painter.circle_stroke(pos, 5.5, Stroke::new(1.2_f32, Color32::from_rgb(10, 20, 30)));
                    }

                    // Vessel Name / MMSI Label & Speed
                    let display_name = st.name.as_deref().unwrap_or("");
                    let label = if !display_name.is_empty() {
                        if let Some(sog) = st.sog_knots {
                            format!("{} ({:.1} kn)", display_name, sog)
                        } else {
                            display_name.to_string()
                        }
                    } else if let Some(sog) = st.sog_knots {
                        format!("{} ({:.1} kn)", mmsi, sog)
                    } else {
                        format!("{}", mmsi)
                    };

                    let label_pos = Pos2::new(pos.x + 8.0, pos.y - 6.0);
                    // Dark label background pill
                    let galley = painter.layout_no_wrap(label.clone(), egui::FontId::proportional(10.0), Color32::WHITE);
                    let pill_rect = Rect::from_min_size(label_pos, galley.size()).expand(2.0);
                    painter.rect_filled(pill_rect, 2.0, Color32::from_rgba_premultiplied(15, 22, 35, 210));
                    painter.galley(label_pos, galley, Color32::WHITE);
                }
            }
        }
    }

    /// Renders the lower decoded marine traffic table
    fn render_table(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("🔍 Search:").size(12.0).strong());
            ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(180.0));
            if !self.filter.is_empty() {
                if ui.small_button("Clear").clicked() {
                    self.filter.clear();
                }
            }
            ui.separator();
            ui.label(RichText::new(format!("Showing {} vessels", self.saved_vessels.len())).size(11.0).color(Color32::from_rgb(160, 210, 255)));
        });

        ui.add_space(2.0);

        let filter_lower = self.filter.to_lowercase();
        let mut vessels: Vec<&AisStation> = self.saved_vessels.values().collect();
        // Sort by last seen (newest first)
        vessels.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));

        egui::ScrollArea::vertical()
            .id_salt("ais_traffic_table")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("ais_grid")
                    .striped(true)
                    .spacing([10.0, 4.0])
                    .min_col_width(50.0)
                    .show(ui, |ui| {
                        // Headers
                        ui.label(RichText::new("MMSI").strong().color(Color32::from_rgb(0, 230, 180)));
                        ui.label(RichText::new("Vessel Name").strong().color(Color32::from_rgb(0, 230, 180)));
                        ui.label(RichText::new("Flag / Country").strong().color(Color32::from_rgb(0, 230, 180)));
                        ui.label(RichText::new("Type").strong().color(Color32::from_rgb(0, 230, 180)));
                        ui.label(RichText::new("Status").strong().color(Color32::from_rgb(0, 230, 180)));
                        ui.label(RichText::new("SOG").strong().color(Color32::from_rgb(0, 230, 180)));
                        ui.label(RichText::new("COG").strong().color(Color32::from_rgb(0, 230, 180)));
                        ui.label(RichText::new("Destination").strong().color(Color32::from_rgb(0, 230, 180)));
                        ui.label(RichText::new("Distance to QTH").strong().color(Color32::from_rgb(0, 230, 180)));
                        ui.label(RichText::new("Last Seen").strong().color(Color32::from_rgb(0, 230, 180)));
                        ui.end_row();

                        let now = Instant::now();
                        for st in vessels {
                            // Filter matching
                            if !filter_lower.is_empty() {
                                let mmsi_s = st.mmsi.to_string();
                                let name_s = st.name.as_deref().unwrap_or("").to_lowercase();
                                let call_s = st.callsign.as_deref().unwrap_or("").to_lowercase();
                                let dest_s = st.destination.as_deref().unwrap_or("").to_lowercase();
                                let country_s = st.country.to_lowercase();
                                if !mmsi_s.contains(&filter_lower)
                                    && !name_s.contains(&filter_lower)
                                    && !call_s.contains(&filter_lower)
                                    && !dest_s.contains(&filter_lower)
                                    && !country_s.contains(&filter_lower)
                                {
                                    continue;
                                }
                            }

                            let is_selected = self.selected_mmsi == Some(st.mmsi);
                            let mmsi_col = if is_selected { Color32::from_rgb(255, 220, 0) } else { Color32::from_rgb(180, 240, 255) };

                            // MMSI button (click to select and center map)
                            if ui.link(RichText::new(st.mmsi.to_string()).strong().color(mmsi_col)).clicked() {
                                self.selected_mmsi = Some(st.mmsi);
                                if let (Some(la), Some(lo)) = (st.latitude, st.longitude) {
                                    self.center_override = Some((la, lo));
                                }
                            }

                            // Vessel Name
                            let name_txt = st.name.as_deref().unwrap_or("---");
                            ui.label(RichText::new(name_txt).strong());

                            // Country
                            ui.label(&st.country);

                            // Vessel Type
                            let vt_txt = st.vessel_type.as_deref().unwrap_or("Vessel");
                            ui.label(vt_txt);

                            // Nav Status
                            let ns_txt = st.nav_status.as_deref().unwrap_or("---");
                            ui.label(ns_txt);

                            // Speed Over Ground (SOG)
                            if let Some(sog) = st.sog_knots {
                                ui.label(format!("{:.1} kn", sog));
                            } else {
                                ui.label("---");
                            }

                            // Course Over Ground (COG)
                            if let Some(cog) = st.cog_deg {
                                ui.label(format!("{:.0}°", cog));
                            } else {
                                ui.label("---");
                            }

                            // Destination
                            let dest_txt = st.destination.as_deref().unwrap_or("---");
                            ui.label(dest_txt);

                            // Distance & Bearing to QTH
                            if let (Some(la), Some(lo)) = (st.latitude, st.longitude) {
                                let (dist_km, bearing_deg, compass) = crate::gui::aprs_view::calculate_distance_bearing(
                                    self.center_qth.0,
                                    self.center_qth.1,
                                    la,
                                    lo,
                                );
                                ui.label(format!("{:.1} km ({:.0}° {})", dist_km, bearing_deg, compass));
                            } else {
                                ui.label("No GPS");
                            }

                            // Last seen
                            let age_sec = now.saturating_duration_since(st.last_seen).as_secs();
                            let age_txt = if age_sec < 60 {
                                format!("{}s ago", age_sec)
                            } else {
                                format!("{}m ago", age_sec / 60)
                            };
                            ui.label(RichText::new(age_txt).color(if age_sec < 60 { Color32::from_rgb(0, 240, 180) } else { Color32::GRAY }));

                            ui.end_row();
                        }
                    });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ais_view_rejects_phantom_arctic_and_invalid_mmsi() {
        let mut view = AisView::new();
        view.set_qth((60.20, 24.90)); // Helsinki

        // 1. Packet with invalid MMSI 815493 (Coast Radio Station glitch)
        let invalid_mmsi_pkt = AisPacket {
            mmsi: 815493,
            msg_type: 1,
            timestamp: "12:00:00".to_string(),
            freq_hz: 161_975_000,
            channel: "A".to_string(),
            name: None,
            callsign: None,
            country: "Coast Radio Station".to_string(),
            vessel_type: None,
            nav_status: Some("At anchor".to_string()),
            latitude: Some(75.0),
            longitude: Some(-95.0),
            sog_knots: Some(34.4),
            cog_deg: Some(91.0),
            heading_deg: None,
            destination: None,
            draught_m: None,
            length_m: None,
            beam_m: None,
            nmea_sentence: String::new(),
            human_summary: "Phantom vessel".to_string(),
            signal_snr_db: 12.0,
        };

        view.ingest_packets(&[invalid_mmsi_pkt]);
        assert_eq!(view.saved_vessels.len(), 0, "Invalid MMSI must be rejected!");

        // 2. Packet with valid MMSI but > 600 km away in Canadian Arctic (4,242 km away)
        let arctic_pkt = AisPacket {
            mmsi: 230123456, // Valid Finland MMSI
            msg_type: 1,
            timestamp: "12:00:01".to_string(),
            freq_hz: 161_975_000,
            channel: "A".to_string(),
            name: Some("ARCTIC EXPLORER".to_string()),
            callsign: None,
            country: "Finland".to_string(),
            vessel_type: Some("Cargo Ship".to_string()),
            nav_status: Some("Under way using engine".to_string()),
            latitude: Some(75.5),  // Arctic
            longitude: Some(-94.2), // Bathurst/Cornwallis Island
            sog_knots: Some(15.0),
            cog_deg: Some(90.0),
            heading_deg: None,
            destination: None,
            draught_m: None,
            length_m: None,
            beam_m: None,
            nmea_sentence: String::new(),
            human_summary: "Far out".to_string(),
            signal_snr_db: 12.0,
        };

        view.ingest_packets(&[arctic_pkt]);
        assert_eq!(view.saved_vessels.len(), 0, "Out of range position (>600km) must be rejected!");

        // 3. Packet with valid Finnish MMSI within direct line-of-sight range (~20 km)
        let valid_baltic_pkt = AisPacket {
            mmsi: 230123456,
            msg_type: 1,
            timestamp: "12:00:02".to_string(),
            freq_hz: 161_975_000,
            channel: "A".to_string(),
            name: Some("SUOMENLAHTI 1".to_string()),
            callsign: Some("OJEK".to_string()),
            country: "Finland".to_string(),
            vessel_type: Some("Passenger Ship / Ferry".to_string()),
            nav_status: Some("Under way using engine".to_string()),
            latitude: Some(60.10),
            longitude: Some(25.00),
            sog_knots: Some(18.2),
            cog_deg: Some(165.0),
            heading_deg: Some(165.0),
            destination: Some("TALLINN".to_string()),
            draught_m: Some(6.5),
            length_m: Some(180),
            beam_m: Some(28),
            nmea_sentence: String::new(),
            human_summary: "Ferry".to_string(),
            signal_snr_db: 18.0,
        };

        view.ingest_packets(&[valid_baltic_pkt]);
        assert_eq!(view.saved_vessels.len(), 1, "Valid Baltic vessel must be accepted!");
        let vessel = view.saved_vessels.get(&230123456).expect("Vessel must exist");
        assert_eq!(vessel.name.as_deref(), Some("SUOMENLAHTI 1"));
        assert_eq!(vessel.packet_count, 1);

        // 4. Ingesting duplicate packet should NOT increment packet_count or modify last_seen
        let old_last_seen = vessel.last_seen;
        std::thread::sleep(std::time::Duration::from_millis(15));
        let duplicate_pkt = AisPacket {
            mmsi: 230123456,
            msg_type: 1,
            timestamp: "12:00:02".to_string(), // identical timestamp & msg_type
            freq_hz: 161_975_000,
            channel: "A".to_string(),
            name: Some("SUOMENLAHTI 1".to_string()),
            callsign: Some("OJEK".to_string()),
            country: "Finland".to_string(),
            vessel_type: Some("Passenger Ship / Ferry".to_string()),
            nav_status: Some("Under way using engine".to_string()),
            latitude: Some(60.10),
            longitude: Some(25.00),
            sog_knots: Some(18.2),
            cog_deg: Some(165.0),
            heading_deg: Some(165.0),
            destination: Some("TALLINN".to_string()),
            draught_m: Some(6.5),
            length_m: Some(180),
            beam_m: Some(28),
            nmea_sentence: String::new(),
            human_summary: "Ferry".to_string(),
            signal_snr_db: 18.0,
        };
        view.ingest_packets(&[duplicate_pkt]);
        let vessel_after = view.saved_vessels.get(&230123456).unwrap();
        assert_eq!(vessel_after.packet_count, 1, "Duplicate packet must not increment count!");
        assert_eq!(vessel_after.last_seen, old_last_seen, "Duplicate packet must not reset last_seen!");
    }
}
