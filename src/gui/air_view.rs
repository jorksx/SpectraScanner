use crate::config::DemodMode;
use crate::dsp::air::{is_valid_callsign, is_valid_icao_aircraft, is_valid_registration};
use crate::dsp::{AirPacket, AirStation};
use crate::dsp_worker::{DisplaySnapshot, DspCommand};
use crate::gui::slippy_map::{MapStyle, SlippyMapState, TileManager};
use crate::scanner::{ScannerState, vfo::VfoState};
use crossbeam_channel::Sender;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};
use std::collections::HashMap;
use std::time::Instant;

/// Finnish and international airports to display on the aviation map
const AIRPORTS: &[(&str, &str, f64, f64)] = &[
    ("EFHK", "Helsinki-Vantaa", 60.317, 24.963),
    ("EFTP", "Tampere-Pirkkala", 61.414, 23.604),
    ("EFTU", "Turku", 60.514, 22.263),
    ("EFOU", "Oulu", 64.930, 25.355),
    ("EFRO", "Rovaniemi", 66.565, 25.830),
    ("EFKU", "Kuopio", 63.007, 27.798),
    ("EFVA", "Vaasa", 63.051, 21.762),
    ("EFJY", "Jyväskylä", 62.399, 25.678),
    ("EFPO", "Pori", 61.462, 21.796),
    ("EFET", "Enontekiö", 68.363, 23.424),
    ("EFIV", "Ivalo", 68.607, 27.405),
    ("EFKT", "Kittilä", 67.701, 24.847),
    ("EFMA", "Maarianhamina", 60.122, 19.898),
    ("ESSA", "Tukholma Arlanda", 59.652, 17.919),
    ("ENGM", "Oslo Gardermoen", 60.198, 11.100),
    ("EKCH", "Kööpenhamina", 55.618, 12.656),
    ("EETN", "Tallinna", 59.413, 24.833),
    ("EVRA", "Riika", 56.924, 23.971),
];

/// Represents persistent antenna coverage reception profile
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AirCoverageData {
    pub center_qth: (f64, f64),
    /// Max distance in km per azimuth bin (72 bins, 5 degrees each)
    pub azimuth_max_km: Vec<f32>,
    pub max_range_km: f32,
    pub total_adsb_packets: u64,
}

impl Default for AirCoverageData {
    fn default() -> Self {
        Self {
            center_qth: (60.20, 24.90),
            azimuth_max_km: vec![15.0; 72], // 15 km baseline reception area
            max_range_km: 15.0,
            total_adsb_packets: 0,
        }
    }
}

pub fn get_air_coverage_storage_path() -> std::path::PathBuf {
    let local = std::path::PathBuf::from("air_coverage.json");
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("air_coverage.json");
        }
    }
    local
}

pub fn save_air_coverage(data: &AirCoverageData) {
    let path = get_air_coverage_storage_path();
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = std::fs::write(path, json);
    }
}

pub fn load_air_coverage() -> AirCoverageData {
    let path = get_air_coverage_storage_path();
    if let Ok(content) = std::fs::read_to_string(path) {
        if let Ok(data) = serde_json::from_str::<AirCoverageData>(&content) {
            if data.azimuth_max_km.len() == 72 {
                return data;
            }
        }
    }
    AirCoverageData::default()
}

/// Computes destination point given start point, distance in km, and bearing in degrees
pub fn destination_point(lat: f64, lon: f64, dist_km: f64, bearing_deg: f64) -> (f64, f64) {
    let r_earth = 6371.0;
    let d = dist_km / r_earth;
    let brng = bearing_deg.to_radians();
    let lat1 = lat.to_radians();
    let lon1 = lon.to_radians();

    let lat2 = (lat1.sin() * d.cos() + lat1.cos() * d.sin() * brng.cos()).asin();
    let lon2 = lon1 + (brng.sin() * d.sin() * lat1.cos()).atan2(d.cos() - lat1.sin() * lat2.sin());

    (lat2.to_degrees(), lon2.to_degrees())
}

/// Aviation Traffic & ACARS/VDL2 Radar Monitoring View
pub struct AirView {
    pub is_open: bool,
    pub filter: String,
    pub selected_call: Option<String>,
    pub center_override: Option<(f64, f64)>,
    pub center_qth: (f64, f64),
    pub edit_qth_lat: String,
    pub edit_qth_lon: String,
    pub saved_aircraft: HashMap<String, AirStation>,
    pub history_packets: Vec<AirPacket>,
    pub show_airports: bool,
    pub show_range_rings: bool,
    pub show_trails: bool,
    pub show_coverage: bool,
    pub show_raw_bursts: bool,
    pub map_state: SlippyMapState,
    pub tile_mgr: TileManager,
    pub coverage: AirCoverageData,
    pub coverage_dirty: bool,
}

impl Default for AirView {
    fn default() -> Self {
        let qth = crate::gui::aprs_view::load_user_qth().unwrap_or((60.20, 24.90));
        let mut cov = load_air_coverage();
        cov.center_qth = qth;
        Self {
            is_open: false,
            filter: String::new(),
            selected_call: None,
            center_override: None,
            center_qth: qth,
            edit_qth_lat: format!("{:.4}", qth.0),
            edit_qth_lon: format!("{:.4}", qth.1),
            saved_aircraft: HashMap::new(),
            history_packets: Vec::new(),
            show_airports: true,
            show_range_rings: true,
            show_trails: true,
            show_coverage: true,
            show_raw_bursts: false,
            map_state: SlippyMapState::new(qth.0, qth.1, 7.5),
            tile_mgr: TileManager::new(),
            coverage: cov,
            coverage_dirty: false,
        }
    }
}

impl AirView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_qth(&mut self, qth: (f64, f64)) {
        self.center_qth = qth;
        self.edit_qth_lat = format!("{:.4}", qth.0);
        self.edit_qth_lon = format!("{:.4}", qth.1);
        self.coverage.center_qth = qth;
        save_air_coverage(&self.coverage);
    }

    /// Ingests real-time air packets from DisplaySnapshot
    pub fn ingest_packets(&mut self, packets: &[AirPacket]) {
        let now = Instant::now();

        // 1-Hour Timeout: Evict any aircraft that hasn't been heard in over an hour (3600s)
        self.saved_aircraft.retain(|_, st| now.duration_since(st.last_seen).as_secs() <= 3600);

        for p in packets {
            // NEVER treat raw RF bursts as aircraft!
            if p.callsign == "VDL2-BURST" || p.callsign.is_empty() {
                if self.show_raw_bursts {
                    if !self.history_packets.iter().any(|h| h.timestamp == p.timestamp && h.callsign == p.callsign && h.raw_text == p.raw_text) {
                        self.history_packets.push(p.clone());
                        if self.history_packets.len() > 600 {
                            self.history_packets.remove(0);
                        }
                    }
                }
                continue;
            }

            // Reject any unverified pseudo-callsigns or noise bursts that don't represent real aircraft
            let has_valid_call = is_valid_callsign(&p.callsign);
            let has_valid_tail = is_valid_registration(&p.tail);
            let has_coords = p.latitude.is_some() && p.longitude.is_some();
            let is_adsb = p.message_type.contains("ADS-B");
            let is_verified_icao = p.callsign.starts_with("ICAO:") && {
                let hex_str = p.callsign.trim_start_matches("ICAO:");
                if let Ok(addr) = u32::from_str_radix(hex_str, 16) {
                    is_valid_icao_aircraft(addr) && (p.altitude_ft.is_some() || p.airline.is_some() || has_coords || p.origin.is_some())
                } else {
                    false
                }
            };

            let is_verified = has_valid_call || has_valid_tail || has_coords || is_verified_icao || is_adsb;

            if !is_verified {
                if self.show_raw_bursts {
                    if !self.history_packets.iter().any(|h| h.timestamp == p.timestamp && h.callsign == p.callsign && h.raw_text == p.raw_text) {
                        self.history_packets.push(p.clone());
                        if self.history_packets.len() > 600 {
                            self.history_packets.remove(0);
                        }
                    }
                }
                continue;
            }

            let is_new = !self.history_packets.iter().any(|h| {
                h.timestamp == p.timestamp
                    && h.callsign == p.callsign
                    && h.raw_text == p.raw_text
                    && h.latitude == p.latitude
                    && h.longitude == p.longitude
            });
            if !is_new {
                continue;
            }
            self.history_packets.push(p.clone());
            if self.history_packets.len() > 600 {
                self.history_packets.remove(0);
            }

            let key = if !p.callsign.is_empty() {
                p.callsign.clone()
            } else if !p.tail.is_empty() {
                p.tail.clone()
            } else {
                continue;
            };

            if let Some(station) = self.saved_aircraft.get_mut(&key) {
                station.last_seen = now;
                station.last_timestamp = p.timestamp.clone();
                station.packet_count += 1;
                station.latest_message = p.raw_text.clone();
                station.latest_summary = p.human_summary.clone();
                if let Some(r) = &p.route {
                    station.route = Some(r.clone());
                }
                if let Some(o) = &p.origin {
                    station.origin = Some(o.clone());
                }
                if let Some(d) = &p.destination {
                    station.destination = Some(d.clone());
                }
                if let Some(act) = &p.aircraft_type {
                    station.aircraft_type = Some(act.clone());
                }
                if let Some(a) = p.altitude_ft {
                    station.altitude_ft = Some(a);
                }
                if let Some(s) = p.speed_kts {
                    station.speed_kts = Some(s);
                }
                if let Some(h) = p.heading_deg {
                    station.heading_deg = Some(h);
                }
                if let Some(wp) = &p.waypoint {
                    station.last_waypoint = Some(wp.clone());
                }
                if let (Some(la), Some(lo)) = (p.latitude, p.longitude) {
                    station.latitude = Some(la);
                    station.longitude = Some(lo);
                    // Add to snail tail history if moved
                    let should_push = match station.track_history.last() {
                        Some(&(last_la, last_lo)) => (last_la - la).abs() > 0.0003 || (last_lo - lo).abs() > 0.0003,
                        None => true,
                    };
                    if should_push {
                        station.track_history.push((la, lo));
                        if station.track_history.len() > 200 {
                            station.track_history.remove(0);
                        }
                    }

                    // Update antenna coverage reception profile
                    let (dist_km, bearing_deg, _) = crate::gui::aprs_view::calculate_distance_bearing(
                        self.center_qth.0,
                        self.center_qth.1,
                        la,
                        lo,
                    );
                    if dist_km > 0.0 && dist_km < 600.0 {
                        let sector = (((bearing_deg / 360.0) * 72.0).floor() as usize) % 72;
                        if (dist_km as f32) > self.coverage.azimuth_max_km[sector] {
                            self.coverage.azimuth_max_km[sector] = dist_km as f32;
                            self.coverage.max_range_km = self.coverage.max_range_km.max(dist_km as f32);
                            self.coverage_dirty = true;
                        }
                        self.coverage.total_adsb_packets = self.coverage.total_adsb_packets.saturating_add(1);
                    }
                }
            } else {
                let mut tracks = Vec::new();
                if let (Some(la), Some(lo)) = (p.latitude, p.longitude) {
                    tracks.push((la, lo));

                    // Update antenna coverage reception profile
                    let (dist_km, bearing_deg, _) = crate::gui::aprs_view::calculate_distance_bearing(
                        self.center_qth.0,
                        self.center_qth.1,
                        la,
                        lo,
                    );
                    if dist_km > 0.0 && dist_km < 600.0 {
                        let sector = (((bearing_deg / 360.0) * 72.0).floor() as usize) % 72;
                        if (dist_km as f32) > self.coverage.azimuth_max_km[sector] {
                            self.coverage.azimuth_max_km[sector] = dist_km as f32;
                            self.coverage.max_range_km = self.coverage.max_range_km.max(dist_km as f32);
                            self.coverage_dirty = true;
                        }
                        self.coverage.total_adsb_packets = self.coverage.total_adsb_packets.saturating_add(1);
                    }
                }
                let station = AirStation {
                    tail: p.tail.clone(),
                    callsign: p.callsign.clone(),
                    airline: p.airline.clone(),
                    aircraft_type: p.aircraft_type.clone(),
                    route: p.route.clone(),
                    origin: p.origin.clone(),
                    destination: p.destination.clone(),
                    altitude_ft: p.altitude_ft,
                    speed_kts: p.speed_kts,
                    heading_deg: p.heading_deg,
                    latitude: p.latitude,
                    longitude: p.longitude,
                    last_waypoint: p.waypoint.clone(),
                    last_seen: now,
                    last_timestamp: p.timestamp.clone(),
                    packet_count: 1,
                    latest_message: p.raw_text.clone(),
                    latest_summary: p.human_summary.clone(),
                    track_history: tracks,
                };
                self.saved_aircraft.insert(key, station);
            }
        }

        // Auto-save antenna coverage profile to local disk if expanded
        if self.coverage_dirty {
            save_air_coverage(&self.coverage);
            self.coverage_dirty = false;
        }
    }

    /// Smart channel switcher: manages multiple VHF airband channels concurrently or tunes 1090 MHz ADS-B radar
    pub fn toggle_channel(
        cmd_tx: &Sender<DspCommand>,
        scanner_state: &mut ScannerState,
        snapshot: &DisplaySnapshot,
        target_f: u64,
        mode: DemodMode,
        name: &str,
        is_currently_active: bool,
    ) {
        *scanner_state = ScannerState::Paused;
        let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));

        if target_f == 1_090_000_000 {
            if is_currently_active {
                // Return back to VHF airband (center frequency 134.250 MHz)
                let _ = cmd_tx.send(DspCommand::SetCenterFreq(134_250_000));
                let _ = cmd_tx.send(DspCommand::TuneMultiDataVfos(vec![
                    (136_975_000, DemodMode::AM, "VDL2 136.975".to_string()),
                    (131_725_000, DemodMode::AM, "ACARS 131.725".to_string()),
                ]));
            } else {
                // Physically tune SDR directly to 1090.000 MHz Mode S ADS-B radar frequency
                let _ = cmd_tx.send(DspCommand::SetCenterFreq(1_090_000_000));
                let _ = cmd_tx.send(DspCommand::TuneMultiDataVfos(vec![
                    (1_090_000_000, DemodMode::AM, "ADS-B 1090".to_string()),
                ]));
            }
            return;
        }

        // If SDR was on 1090 MHz and user selects VHF frequency, retune SDR center frequency back to VHF airband
        if snapshot.center_freq_hz >= 1_080_000_000 {
            let _ = cmd_tx.send(DspCommand::SetCenterFreq(134_250_000));
        }

        let mut current_channels: Vec<(u64, DemodMode, String, bool)> = snapshot
            .vfos
            .iter()
            .filter(|v| (v.state == VfoState::Pinned || v.active) && !v.is_out_of_band && v.freq_hz >= 118_000_000 && v.freq_hz <= 138_000_000)
            .map(|v| {
                let is_voice = v.freq_hz == 118_100_000 || v.freq_hz == 121_500_000;
                (v.freq_hz, v.mode, v.name.clone(), is_voice)
            })
            .collect();

        let is_voice = target_f == 118_100_000 || target_f == 121_500_000;
        if is_currently_active {
            current_channels.retain(|c| c.0 != target_f);
        } else {
            current_channels.retain(|c| c.0 != target_f);
            current_channels.insert(0, (target_f, mode, name.to_string(), is_voice));
        }

        let _ = cmd_tx.send(DspCommand::TuneMultiChannels(current_channels));
    }

    /// Renders the AIR Traffic Radar popup window
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        snapshot: &DisplaySnapshot,
        cmd_tx: &Sender<DspCommand>,
        scanner_state: &mut ScannerState,
    ) {
        self.ingest_packets(&snapshot.air_packets);

        if !self.is_open {
            return;
        }

        self.tile_mgr.update(ctx);

        let mut is_open = self.is_open;
        egui::Window::new("✈️ AIR TRAFFIC & ACARS / VDL2 / ADS-B RADAR")
            .id(egui::Id::new("air_traffic_radar_window"))
            .open(&mut is_open)
            .default_size(egui::vec2(900.0, 580.0))
            .min_size(egui::vec2(520.0, 360.0))
            .max_size(egui::vec2(1150.0, 800.0))
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                self.render_ui(ui, ctx, snapshot, cmd_tx, scanner_state);
            });
        self.is_open = is_open;
    }

    fn render_ui(
        &mut self,
        ui: &mut egui::Ui,
        _ctx: &egui::Context,
        snapshot: &DisplaySnapshot,
        cmd_tx: &Sender<DspCommand>,
        scanner_state: &mut ScannerState,
    ) {
        let is_131_725_active = snapshot.vfos.iter().any(|v| v.freq_hz == 131_725_000 && (v.state == VfoState::Pinned || v.active) && !v.is_out_of_band);
        let is_136_975_active = snapshot.vfos.iter().any(|v| v.freq_hz == 136_975_000 && (v.state == VfoState::Pinned || v.active) && !v.is_out_of_band);
        let is_131_525_active = snapshot.vfos.iter().any(|v| v.freq_hz == 131_525_000 && (v.state == VfoState::Pinned || v.active) && !v.is_out_of_band);
        let is_118_100_active = snapshot.vfos.iter().any(|v| v.freq_hz == 118_100_000 && (v.state == VfoState::Pinned || v.active) && !v.is_out_of_band);
        let is_121_500_active = snapshot.vfos.iter().any(|v| v.freq_hz == 121_500_000 && (v.state == VfoState::Pinned || v.active) && !v.is_out_of_band);
        let is_1090_active = (snapshot.center_freq_hz >= 1_080_000_000 && snapshot.center_freq_hz <= 1_100_000_000)
            || snapshot.vfos.iter().any(|v| v.freq_hz == 1_090_000_000 && (v.state == VfoState::Pinned || v.active) && !v.is_out_of_band);

        let active_channel_count = [is_131_725_active, is_136_975_active, is_131_525_active, is_118_100_active, is_121_500_active, is_1090_active].iter().filter(|&&a| a).count();

        // ====================================================================
        // 1. TOP CONTROL BAR (Standard frequencies, Auto-Scan, Quick Tuners)
        // ====================================================================
        egui::Frame::none()
            .fill(Color32::from_rgb(14, 20, 30))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 48, 72)))
            .rounding(5.0)
            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("📻 FREQUENCIES:").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());

                    // 131.725 MHz (Europe ACARS Primary - Data Mode: Squelch open, Audio Mute)
                    let btn_131_txt = if is_131_725_active { "● 131.725 ACARS" } else { "131.725 ACARS" };
                    let btn_131_bg = if is_131_725_active { Color32::from_rgb(0, 230, 255) } else { Color32::from_rgb(20, 32, 48) };
                    let btn_131_fg = if is_131_725_active { Color32::BLACK } else { Color32::from_rgb(180, 220, 255) };
                    let btn_131 = ui.add(
                        egui::Button::new(RichText::new(btn_131_txt).size(11.0).strong().color(btn_131_fg))
                            .fill(btn_131_bg),
                    ).on_hover_text(if is_131_725_active {
                        "131.725 MHz is ACTIVE! Click to toggle off."
                    } else {
                        "Toggle Europe ACARS Primary 131.725 MHz AM into a parallel VFO"
                    });
                    if btn_131.clicked() {
                        Self::toggle_channel(cmd_tx, scanner_state, snapshot, 131_725_000, DemodMode::AM, "ACARS 131.725", is_131_725_active);
                    }

                    // 136.975 MHz (Europe VDL2 Primary - Data Mode: Squelch open, Audio Mute)
                    let vdl2_burst_str = if let Some((cnt, _, _)) = snapshot.vdl2_burst_stats {
                        if cnt > 0 {
                            format!(" [⚡{}]", cnt)
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };
                    let btn_136_txt = if is_136_975_active {
                        format!("● 136.975 VDL2{}", vdl2_burst_str)
                    } else {
                        format!("136.975 VDL2{}", vdl2_burst_str)
                    };
                    let btn_136_bg = if is_136_975_active { Color32::from_rgb(0, 255, 170) } else { Color32::from_rgb(20, 32, 48) };
                    let btn_136_fg = if is_136_975_active { Color32::BLACK } else { Color32::from_rgb(180, 220, 255) };
                    let hover_tip = if let Some((cnt, snr, dur)) = snapshot.vdl2_burst_stats {
                        format!(
                            "Europe VDL Mode 2 Primary 136.975 MHz AM (Data Mode)\nTotal RF Bursts Captured: {}\nLatest SNR: {:.1} dB\nLatest Duration: {:.0} ms\nClick to toggle parallel VFO monitoring.",
                            cnt, snr, dur
                        )
                    } else {
                        "Toggle Europe VDL Mode 2 Primary 136.975 MHz AM into a parallel VFO".to_string()
                    };
                    let btn_136 = ui.add(
                        egui::Button::new(RichText::new(btn_136_txt).size(11.0).strong().color(btn_136_fg))
                            .fill(btn_136_bg),
                    ).on_hover_text(hover_tip);
                    if btn_136.clicked() {
                        Self::toggle_channel(cmd_tx, scanner_state, snapshot, 136_975_000, DemodMode::AM, "VDL2 136.975", is_136_975_active);
                    }

                    // 131.525 MHz (International ACARS - Data Mode: Squelch open, Audio Mute)
                    let btn_intl_txt = if is_131_525_active { "● 131.525 Intl" } else { "131.525 Intl" };
                    let btn_intl_bg = if is_131_525_active { Color32::from_rgb(80, 210, 255) } else { Color32::from_rgb(20, 32, 48) };
                    let btn_intl_fg = if is_131_525_active { Color32::BLACK } else { Color32::from_rgb(180, 220, 255) };
                    let btn_intl = ui.add(
                        egui::Button::new(RichText::new(btn_intl_txt).size(11.0).strong().color(btn_intl_fg))
                            .fill(btn_intl_bg),
                    ).on_hover_text(if is_131_525_active {
                        "131.525 MHz is ACTIVE! Click to toggle off."
                    } else {
                        "Toggle International ACARS 131.525 MHz AM into a parallel VFO"
                    });
                    if btn_intl.clicked() {
                        Self::toggle_channel(cmd_tx, scanner_state, snapshot, 131_525_000, DemodMode::AM, "ACARS 131.525", is_131_525_active);
                    }

                    // 118.100 MHz (Helsinki Tower - Voice)
                    let btn_twr_txt = if is_118_100_active { "● 118.100 TWR" } else { "118.100 TWR" };
                    let btn_twr_bg = if is_118_100_active { Color32::from_rgb(255, 200, 80) } else { Color32::from_rgb(30, 42, 60) };
                    let btn_twr_fg = if is_118_100_active { Color32::BLACK } else { Color32::WHITE };
                    let btn_twr = ui.add(
                        egui::Button::new(RichText::new(btn_twr_txt).size(11.0).color(btn_twr_fg))
                            .fill(btn_twr_bg),
                    ).on_hover_text(if is_118_100_active {
                        "118.100 MHz is ACTIVE! Click to toggle off."
                    } else {
                        "Toggle Helsinki-Vantaa Tower 118.100 MHz AM (Voice channel)"
                    });
                    if btn_twr.clicked() {
                        Self::toggle_channel(cmd_tx, scanner_state, snapshot, 118_100_000, DemodMode::AM, "EFHK TWR 118.100", is_118_100_active);
                    }

                    // 121.500 MHz (Emergency Guard - Voice)
                    let btn_guard_txt = if is_121_500_active { "● 121.500 Guard" } else { "121.500 Guard" };
                    let btn_guard_bg = if is_121_500_active { Color32::from_rgb(255, 80, 80) } else { Color32::from_rgb(50, 25, 30) };
                    let btn_guard_fg = if is_121_500_active { Color32::BLACK } else { Color32::from_rgb(255, 120, 120) };
                    let btn_guard = ui.add(
                        egui::Button::new(RichText::new(btn_guard_txt).size(11.0).color(btn_guard_fg))
                            .fill(btn_guard_bg),
                    ).on_hover_text(if is_121_500_active {
                        "121.500 MHz is ACTIVE! Click to toggle off."
                    } else {
                        "Toggle International Emergency Guard 121.500 MHz AM (Voice channel)"
                    });
                    if btn_guard.clicked() {
                        Self::toggle_channel(cmd_tx, scanner_state, snapshot, 121_500_000, DemodMode::AM, "GUARD 121.500", is_121_500_active);
                    }

                    // 1090.000 MHz (ADS-B Mode S Extended Squitter - 2 Hz Live GPS Positions)
                    let btn_1090_txt = if is_1090_active { "● 1090 ADS-B" } else { "1090 ADS-B" };
                    let btn_1090_bg = if is_1090_active { Color32::from_rgb(0, 255, 170) } else { Color32::from_rgb(20, 32, 48) };
                    let btn_1090_fg = if is_1090_active { Color32::BLACK } else { Color32::from_rgb(180, 255, 210) };
                    let btn_1090 = ui.add(
                        egui::Button::new(RichText::new(btn_1090_txt).size(11.0).strong().color(btn_1090_fg))
                            .fill(btn_1090_bg),
                    ).on_hover_text(if is_1090_active {
                        "1090.000 MHz ADS-B is ACTIVE! Click to toggle back to VHF airband."
                    } else {
                        "Tune SDR to 1090.000 MHz ADS-B Mode S radar frequency (2 Hz live GPS positions)"
                    });
                    if btn_1090.clicked() {
                        Self::toggle_channel(cmd_tx, scanner_state, snapshot, 1_090_000_000, DemodMode::AM, "ADS-B 1090", is_1090_active);
                    }

                    ui.add_space(6.0);

                    // Automatic Airband Scanning Button
                    let is_scanning = *scanner_state == ScannerState::Scanning;
                    let (scan_txt, scan_col) = if is_scanning {
                        ("⏸ PAUSE SCAN", Color32::from_rgb(0, 195, 235))
                    } else {
                        ("📡 SCAN AIRBAND", Color32::from_rgb(255, 190, 40))
                    };

                    let btn_autoscan = ui.add(
                        egui::Button::new(RichText::new(scan_txt).size(11.0).strong().color(Color32::BLACK))
                            .fill(scan_col),
                    ).on_hover_text("Automatically sweep entire VHF airband 118.000 - 136.975 MHz AM (8.33 kHz channel step)");
                    if btn_autoscan.clicked() {
                        if is_scanning {
                            *scanner_state = ScannerState::Paused;
                            let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                        } else {
                            *scanner_state = ScannerState::Scanning;
                            let _ = cmd_tx.send(DspCommand::SetScanProfile {
                                name: "AIRBAND (VHF Aviation)".to_string(),
                                center_freq_hz: 127_500_000,
                                scan_start_hz: 118_000_000,
                                scan_end_hz: 136_975_000,
                                scan_step_hz: 8333,
                                mode: DemodMode::AM,
                            });
                            let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Scanning));
                        }
                    }

                    ui.add_space(6.0);

                    // Multi-VFO status indicator
                    if active_channel_count > 1 {
                        ui.label(
                            RichText::new(format!("⚡ {} VFOs ACTIVE CONCURRENTLY ({} MHz SDR Bandwidth)", active_channel_count, snapshot.sample_rate_hz / 1_000_000))
                                .size(10.5)
                                .color(Color32::from_rgb(0, 255, 136))
                                .strong(),
                        );
                    }

                    // Clear / stats
                    ui.label(RichText::new(format!("Aircraft: {}", self.saved_aircraft.len())).size(11.0).color(Color32::from_rgb(180, 205, 230)));
                    if ui.button(RichText::new("🗑 Clear").size(10.0)).clicked() {
                        self.saved_aircraft.clear();
                        self.history_packets.clear();
                    }
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);

                // ROW 2: RADAR MAP CONTROLS, HOME QTH & DISPLAY LAYERS
                ui.horizontal_wrapped(|ui| {
                    // Map Style selector
                    egui::ComboBox::from_id_salt("air_map_style_combo")
                        .selected_text(self.map_state.style.name())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.map_state.style, MapStyle::OpenStreetMap, "🌍 OpenStreetMap");
                            ui.selectable_value(&mut self.map_state.style, MapStyle::DarkTactical, "🌙 Dark Tactical");
                        });

                    if ui.button(RichText::new("➕ In").size(11.0)).clicked() {
                        self.map_state.zoom = (self.map_state.zoom + 1.0).min(18.0);
                    }
                    if ui.button(RichText::new("➖ Out").size(11.0)).clicked() {
                        self.map_state.zoom = (self.map_state.zoom - 1.0).max(1.0);
                    }
                    if ui.button(RichText::new("🌍 World").size(11.0)).clicked() {
                        self.map_state.center_lat = 40.0;
                        self.map_state.center_lon = 10.0;
                        self.map_state.zoom = 3.0;
                    }
                    if ui.button(RichText::new("📍 Finland").size(11.0)).clicked() {
                        self.map_state.center_lat = 62.0;
                        self.map_state.center_lon = 25.5;
                        self.map_state.zoom = 6.0;
                    }
                    if ui.button(RichText::new("🏠 Home").size(11.0)).clicked() {
                        self.map_state.center_lat = self.center_qth.0;
                        self.map_state.center_lon = self.center_qth.1;
                        self.map_state.zoom = 9.0;
                    }
                    if ui.button(RichText::new("🛫 EFHK").size(11.0)).clicked() {
                        self.map_state.center_lat = 60.317;
                        self.map_state.center_lon = 24.963;
                        self.map_state.zoom = 11.0;
                    }

                    ui.separator();

                    // User QTH / Home Location configuration
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🏠 QTH:").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                        let lat_edit = ui.add(egui::TextEdit::singleline(&mut self.edit_qth_lat).desired_width(50.0));
                        ui.label("°N");
                        let lon_edit = ui.add(egui::TextEdit::singleline(&mut self.edit_qth_lon).desired_width(50.0));
                        ui.label("°E");
                        let enter_pressed = (lat_edit.has_focus() || lon_edit.has_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if lat_edit.lost_focus() || lon_edit.lost_focus() || enter_pressed {
                            if let (Ok(lat), Ok(lon)) = (self.edit_qth_lat.trim().parse::<f64>(), self.edit_qth_lon.trim().parse::<f64>()) {
                                let new_qth = (lat.clamp(-90.0, 90.0), lon.clamp(-180.0, 180.0));
                                self.set_qth(new_qth);
                            }
                        }
                    });

                    ui.separator();
                    ui.checkbox(&mut self.show_airports, RichText::new("Airports").size(10.5));
                    ui.checkbox(&mut self.show_range_rings, RichText::new("Rings").size(10.5));
                    ui.checkbox(&mut self.show_trails, RichText::new("Trails").size(10.5));
                    ui.checkbox(&mut self.show_coverage, RichText::new("Coverage").size(10.5))
                        .on_hover_text("Show antenna reception coverage (polar coverage overlay). Saved automatically to local storage.");

                    ui.separator();
                    let info_lbl = ui.label(RichText::new("ℹ VHF vs ADS-B").size(10.5).color(Color32::from_rgb(0, 220, 255)));
                    info_lbl.on_hover_text(
                        "VHF ACARS (131.725 MHz) and VDL2 (136.975 MHz) transmit airline operational data (routes, OOOI, weather).\nOver Finland and Europe, GPS coordinates are rarely sent over VHF datalink because ground radar monitors airspace.\nContinuous real-time 2 Hz GPS positions and flight tracks are broadcast on 1090 MHz ADS-B (Mode S).\nClick '1090 ADS-B' to physically tune the SDR to 1090 MHz to receive live GPS positions."
                    );
                });
            });

        ui.add_space(4.0);

        // ====================================================================
        // 2. MAIN SPLIT VIEW: RADAR MAP (LEFT) & FLIGHT DATA / LOGS (RIGHT)
        // ====================================================================
        let avail_w = ui.available_width();
        let avail_h = ui.available_height().max(250.0);
        let map_w = (avail_w * 0.50).clamp(240.0, 560.0);
        let right_w = (avail_w - map_w - 12.0).max(200.0);

        ui.horizontal(|ui| {
            // LEFT: 2D Radar Map
            ui.allocate_ui_with_layout(
                egui::vec2(map_w, avail_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    self.render_map(ui, map_w, avail_h);
                },
            );

            ui.separator();

            // RIGHT: Flight Inspector & Live Decoded Message Log
            ui.allocate_ui_with_layout(
                egui::vec2(right_w, avail_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    self.render_flight_panel(ui, right_w, avail_h, cmd_tx, scanner_state, snapshot);
                },
            );
        });
    }

    /// Renders 2D tactical aviation radar map with coastlines, aircraft, and range rings
    fn render_map(&mut self, ui: &mut egui::Ui, width: f32, height: f32) {
        let actual_w = width.min(ui.available_width()).max(200.0);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(actual_w, height), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        if let Some(center) = self.center_override {
            self.map_state.center_lat = center.0;
            self.map_state.center_lon = center.1;
            self.center_override = None;
        }

        // Handle interactive drag and mouse wheel zoom on OpenStreetMap
        self.map_state.handle_interaction(ui, rect, &resp);

        // Render OpenStreetMap / Voyager / Dark tiles with smooth LOD scaling and disk cache
        self.map_state.render_tiles(&painter, rect, &mut self.tile_mgr);

        let to_pos = |lat: f64, lon: f64| -> Pos2 {
            self.map_state.lat_lon_to_screen(lat, lon, rect)
        };

        // Radar Range Rings around Helsinki-Vantaa (EFHK)
        if self.show_range_rings {
            let efhk_pos = to_pos(self.center_qth.0, self.center_qth.1);
            let meters_per_px = (40_075_016.686 * self.center_qth.0.to_radians().cos()) / self.map_state.world_size();
            let ring_stroke = match self.map_state.style {
                MapStyle::DarkTactical => Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(0, 220, 255, 45)),
                _ => Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(0, 140, 200, 75)),
            };
            for &dist_km in &[25.0, 50.0, 100.0, 200.0, 300.0, 500.0] {
                let r_px = (dist_km * 1000.0 / meters_per_px) as f32;
                if r_px > 10.0 && r_px < rect.width() * 2.5 {
                    painter.circle_stroke(efhk_pos, r_px, ring_stroke);
                    painter.text(
                        Pos2::new(efhk_pos.x + 4.0, efhk_pos.y - r_px + 2.0),
                        egui::Align2::LEFT_TOP,
                        format!("{:.0} km", dist_km),
                        egui::FontId::monospace(9.0),
                        match self.map_state.style {
                            MapStyle::DarkTactical => Color32::from_rgb(0, 200, 255),
                            _ => Color32::from_rgb(20, 80, 140),
                        },
                    );
                }
            }
        }

        // Draw Reception Coverage Overlay from Home QTH (Persistent Local Memory)
        if self.show_coverage && self.coverage.azimuth_max_km.len() == 72 {
            let home_pos = to_pos(self.center_qth.0, self.center_qth.1);
            let num_sectors = 72;
            let sector_deg = 360.0 / (num_sectors as f64);
            let mut perimeter_pts: Vec<Pos2> = Vec::with_capacity(num_sectors);

            for i in 0..num_sectors {
                let bearing = (i as f64) * sector_deg;
                let dist_km = (self.coverage.azimuth_max_km[i] as f64).max(12.0);
                let (plat, plon) = destination_point(self.center_qth.0, self.center_qth.1, dist_km, bearing);
                let pt = to_pos(plat, plon);
                perimeter_pts.push(pt);
            }

            // Draw filled polygon fan from home_pos to perimeter points
            let fill_col = match self.map_state.style {
                MapStyle::DarkTactical => Color32::from_rgba_unmultiplied(0, 210, 255, 30),
                _ => Color32::from_rgba_unmultiplied(0, 140, 230, 28),
            };

            for i in 0..num_sectors {
                let p1 = perimeter_pts[i];
                let p2 = perimeter_pts[(i + 1) % num_sectors];
                painter.add(egui::Shape::convex_polygon(
                    vec![home_pos, p1, p2],
                    fill_col,
                    Stroke::NONE,
                ));
            }

            // Draw perimeter outline
            let stroke_col = match self.map_state.style {
                MapStyle::DarkTactical => Color32::from_rgba_unmultiplied(0, 230, 255, 170),
                _ => Color32::from_rgba_unmultiplied(0, 130, 220, 180),
            };
            for i in 0..num_sectors {
                let p1 = perimeter_pts[i];
                let p2 = perimeter_pts[(i + 1) % num_sectors];
                painter.line_segment([p1, p2], Stroke::new(1.3_f32, stroke_col));
            }
        }

        // Draw User Home QTH Marker (High-Contrast Royal Blue Pin)
        let home_pos = to_pos(self.center_qth.0, self.center_qth.1);
        if rect.contains(home_pos) {
            painter.circle_filled(home_pos, 7.5, Color32::from_rgb(10, 16, 26)); // drop shadow
            painter.circle_filled(home_pos, 5.5, Color32::from_rgb(0, 115, 230)); // royal blue pin
            painter.circle_filled(home_pos, 2.5, Color32::WHITE); // white core
            painter.circle_stroke(home_pos, 5.5, Stroke::new(1.3_f32, Color32::from_rgb(15, 25, 40)));

            let text_pos = Pos2::new(home_pos.x, home_pos.y + 13.0);
            let pill_rect = Rect::from_center_size(text_pos, egui::vec2(60.0, 15.0));
            painter.rect_filled(pill_rect, 3.0, Color32::from_rgba_premultiplied(12, 20, 32, 220));
            painter.rect_stroke(pill_rect, 3.0, Stroke::new(1.0_f32, Color32::from_rgb(0, 140, 255)));
            painter.text(
                text_pos,
                egui::Align2::CENTER_CENTER,
                "🏠 HOME QTH",
                egui::FontId::proportional(9.5),
                Color32::WHITE,
            );
        }

        // Draw Airports
        if self.show_airports {
            for &(icao, name, lat, lon) in AIRPORTS {
                let pos = to_pos(lat, lon);
                if rect.contains(pos) {
                    painter.circle_filled(pos, 3.0, Color32::from_rgb(0, 200, 255));
                    painter.text(
                        Pos2::new(pos.x + 5.0, pos.y - 4.0),
                        egui::Align2::LEFT_CENTER,
                        format!("{} {}", icao, name),
                        egui::FontId::proportional(9.5),
                        match self.map_state.style {
                            MapStyle::DarkTactical => Color32::from_rgb(140, 190, 230),
                            _ => Color32::from_rgb(20, 70, 130),
                        },
                    );
                }
            }
        }

        // Draw Tracked Aircraft
        let now = Instant::now();
        for (call, st) in &self.saved_aircraft {
            let is_selected = self.selected_call.as_deref() == Some(call.as_str());

            // Flight track trail history (Snail tail)
            if self.show_trails && st.track_history.len() > 1 {
                let total_pts = st.track_history.len();
                for (idx, window) in st.track_history.windows(2).enumerate() {
                    let p1 = to_pos(window[0].0, window[0].1);
                    let p2 = to_pos(window[1].0, window[1].1);
                    if rect.contains(p1) || rect.contains(p2) {
                        let progress = (idx + 1) as f32 / total_pts as f32; // 0.0 .. 1.0 (1.0 = newest near aircraft)
                        let alpha = ((progress * 190.0) + 65.0).clamp(50.0, 255.0) as u8;
                        let stroke_col = if is_selected {
                            Color32::from_rgba_unmultiplied(255, 200, 30, alpha)
                        } else {
                            match self.map_state.style {
                                MapStyle::DarkTactical => Color32::from_rgba_unmultiplied(0, 230, 180, alpha),
                                _ => Color32::from_rgba_unmultiplied(10, 100, 225, alpha),
                            }
                        };
                        let stroke_width = if is_selected {
                            (1.2 + progress * 1.6).min(2.8)
                        } else {
                            (0.8 + progress * 1.2).min(2.0)
                        };
                        painter.line_segment([p1, p2], Stroke::new(stroke_width, stroke_col));
                    }
                }
            }

            if let (Some(lat), Some(lon)) = (st.latitude, st.longitude) {
                let pos = to_pos(lat, lon);
                if rect.contains(pos) {
                    let age_sec = now.saturating_duration_since(st.last_seen).as_secs();
                    let plane_col = if is_selected {
                        Color32::from_rgb(255, 205, 0)
                    } else if age_sec < 60 {
                        match self.map_state.style {
                            MapStyle::DarkTactical => Color32::from_rgb(0, 230, 180),
                            _ => Color32::from_rgb(10, 100, 235), // Bold royal sapphire blue on light map
                        }
                    } else if age_sec < 300 {
                        Color32::from_rgb(0, 170, 230)
                    } else {
                        Color32::from_rgb(70, 105, 150) // Slate blue for older aircraft
                    };

                    // Directional heading vector / needle
                    if let Some(heading) = st.heading_deg {
                        let rad = (heading - 90.0).to_radians();
                        let dir = egui::vec2(rad.cos(), rad.sin());
                        let p_start = pos + dir * 6.0;
                        let p_end = pos + dir * 18.0;
                        // Dark drop shadow behind needle
                        painter.line_segment([p_start, p_end], Stroke::new(3.0_f32, Color32::from_rgba_premultiplied(10, 15, 25, 210)));
                        // High-contrast needle
                        let needle_col = if is_selected {
                            Color32::from_rgb(255, 215, 40)
                        } else {
                            plane_col
                        };
                        painter.line_segment([p_start, p_end], Stroke::new(1.8_f32, needle_col));
                    }

                    // If selected, draw tactical target reticle
                    if is_selected {
                        painter.circle_stroke(pos, 14.0, Stroke::new(1.5_f32, Color32::from_rgb(255, 215, 0)));
                        painter.line_segment([Pos2::new(pos.x - 18.0, pos.y), Pos2::new(pos.x - 9.0, pos.y)], Stroke::new(1.2_f32, Color32::from_rgb(255, 215, 0)));
                        painter.line_segment([Pos2::new(pos.x + 9.0, pos.y), Pos2::new(pos.x + 18.0, pos.y)], Stroke::new(1.2_f32, Color32::from_rgb(255, 215, 0)));
                        painter.line_segment([Pos2::new(pos.x, pos.y - 18.0), Pos2::new(pos.x, pos.y - 9.0)], Stroke::new(1.2_f32, Color32::from_rgb(255, 215, 0)));
                        painter.line_segment([Pos2::new(pos.x, pos.y + 9.0), Pos2::new(pos.x, pos.y + 18.0)], Stroke::new(1.2_f32, Color32::from_rgb(255, 215, 0)));
                    }

                    // Aircraft blip: drop shadow, high-contrast body, crisp white core dot, dark stroke
                    painter.circle_filled(pos, 6.5, Color32::from_rgb(12, 16, 26)); // drop shadow
                    painter.circle_filled(pos, 5.0, plane_col);
                    painter.circle_filled(pos, 2.0, Color32::WHITE);
                    painter.circle_stroke(pos, 5.0, Stroke::new(1.0_f32, Color32::from_rgb(10, 15, 25)));
                    painter.circle_stroke(pos, 8.0, Stroke::new(1.0_f32, plane_col));

                    // Aircraft flight tag (Callsign + FL altitude + Speed)
                    let alt_str = st.altitude_ft.map(|a| format!(" FL{:03}", a / 100)).unwrap_or_default();
                    let spd_str = st.speed_kts.map(|s| format!(" {}kt", s)).unwrap_or_default();
                    let line1 = format!("✈ {}{}{}", call, alt_str, spd_str);
                    let line2 = if is_selected {
                        Some(format!("📍 {:.4}°N, {:.4}°E", lat, lon))
                    } else if let Some(r) = &st.route {
                        Some(r.clone())
                    } else {
                        None
                    };

                    let len1 = line1.chars().count();
                    let pill_w = (len1 as f32 * 6.8 + 14.0).max(52.0);
                    let has_line2 = line2.is_some();
                    let pill_h = if has_line2 { 25.0 } else { 15.0 };
                    let pill_rect = Rect::from_min_size(
                        Pos2::new(pos.x + 9.0, pos.y - pill_h / 2.0),
                        egui::vec2(pill_w, pill_h),
                    );

                    // Dark tactical pill badge (100% readable on light OpenStreetMap and dark maps)
                    painter.rect_filled(
                        pill_rect,
                        3.0,
                        if is_selected {
                            Color32::from_rgba_premultiplied(20, 24, 38, 240)
                        } else {
                            Color32::from_rgba_premultiplied(12, 16, 26, 235)
                        },
                    );
                    painter.rect_stroke(
                        pill_rect,
                        3.0,
                        Stroke::new(
                            1.0_f32,
                            if is_selected {
                                Color32::from_rgb(255, 215, 40)
                            } else {
                                Color32::from_rgb(0, 140, 220)
                            },
                        ),
                    );

                    // Text line 1: Icon + Callsign + Altitude + Speed
                    painter.text(
                        Pos2::new(pill_rect.left() + 5.0, pill_rect.top() + if has_line2 { 1.5 } else { 1.0 }),
                        egui::Align2::LEFT_TOP,
                        &line1,
                        egui::FontId::proportional(10.0),
                        if is_selected {
                            Color32::from_rgb(255, 235, 110)
                        } else {
                            Color32::from_rgb(240, 250, 255)
                        },
                    );

                    // Text line 2: Route or Lat/Lon (if present)
                    if let Some(l2) = line2 {
                        painter.text(
                            Pos2::new(pill_rect.left() + 5.0, pill_rect.top() + 12.5),
                            egui::Align2::LEFT_TOP,
                            l2,
                            egui::FontId::proportional(9.0),
                            if is_selected {
                                Color32::from_rgb(255, 210, 80)
                            } else {
                                Color32::from_rgb(0, 220, 255)
                            },
                        );
                    }

                    // Click aircraft blip to select and focus
                    if resp.clicked() {
                        if let Some(mouse_pos) = resp.interact_pointer_pos() {
                            if mouse_pos.distance(pos) < 18.0 {
                                self.selected_call = Some(call.clone());
                            }
                        }
                    }
                }
            }
        }

        // Tactical scale bar, navigation buttons, and coordinates overlay
        self.map_state.render_controls_overlay(ui, rect, self.center_qth);
    }

    /// Renders flight inspector table and clear, human-readable message feed
    fn render_flight_panel(
        &mut self,
        ui: &mut egui::Ui,
        _width: f32,
        height: f32,
        cmd_tx: &Sender<DspCommand>,
        scanner_state: &mut ScannerState,
        snapshot: &DisplaySnapshot,
    ) {
        // Search filter
        ui.horizontal(|ui| {
            ui.label(RichText::new("🔍").size(12.0));
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Search flight, registration or route...")
                    .desired_width(180.0),
            );
            if !self.filter.is_empty() && ui.small_button("✖").clicked() {
                self.filter.clear();
            }
        });

        ui.add_space(3.0);

        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);

        let search_lower = self.filter.trim().to_lowercase();
        let top_box_h = (height * 0.45).max(180.0);
        let bottom_box_h = (height - top_box_h - 45.0).max(180.0);

        // ====================================================================
        // A. TRACKED FLIGHTS (TABLE)
        // ====================================================================
        ui.label(RichText::new("TRACKED FLIGHTS").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());

        egui::Frame::none()
            .fill(Color32::from_rgb(12, 16, 24))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(28, 42, 60)))
            .rounding(4.0)
            .inner_margin(egui::Margin::symmetric(6.0, 4.0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("air_stations_scroll")
                    .max_height(top_box_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut stations: Vec<&AirStation> = self.saved_aircraft.values().collect();
                        stations.sort_by_key(|s| std::cmp::Reverse(s.last_seen));

                        for st in stations {
                            let matches = search_lower.is_empty()
                                || st.callsign.to_lowercase().contains(&search_lower)
                                || st.tail.to_lowercase().contains(&search_lower)
                                || st.airline.as_deref().unwrap_or("").to_lowercase().contains(&search_lower)
                                || st.aircraft_type.as_deref().unwrap_or("").to_lowercase().contains(&search_lower)
                                || st.route.as_deref().unwrap_or("").to_lowercase().contains(&search_lower);

                            if !matches {
                                continue;
                            }

                            let is_sel = self.selected_call.as_deref() == Some(&st.callsign);
                            let bg_color = if is_sel {
                                Color32::from_rgb(30, 50, 75)
                            } else {
                                Color32::from_rgb(16, 22, 32)
                            };

                            egui::Frame::none()
                                .fill(bg_color)
                                .stroke(Stroke::new(1.0_f32, if is_sel { Color32::from_rgb(0, 220, 255) } else { Color32::from_rgb(24, 34, 48) }))
                                .rounding(3.0)
                                .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        // Selection button
                                        if ui.selectable_label(is_sel, RichText::new(format!("✈ {}", st.callsign)).strong().color(Color32::from_rgb(0, 230, 255))).clicked() {
                                             self.selected_call = Some(st.callsign.clone());
                                             if let (Some(la), Some(lo)) = (st.latitude, st.longitude) {
                                                 self.center_override = Some((la, lo));
                                             }
                                         }

                                         if !st.tail.is_empty() && st.tail != st.callsign {
                                             ui.label(RichText::new(format!("({})", st.tail)).size(10.0).color(Color32::from_rgb(150, 175, 200)));
                                         }

                                         if let Some(a) = st.altitude_ft {
                                             ui.label(RichText::new(format!("FL{:03}", a / 100)).size(10.0).color(Color32::WHITE).strong());
                                         }

                                         ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                             ui.label(RichText::new(&st.last_timestamp).size(9.5).color(Color32::from_rgb(130, 150, 170)));
                                         });
                                     });

                                     // Second row: airline, aircraft type, route
                                     if st.airline.is_some() || st.aircraft_type.is_some() || st.route.is_some() {
                                         ui.horizontal(|ui| {
                                             if let Some(airl) = &st.airline {
                                                 ui.label(RichText::new(airl).size(10.0).color(Color32::from_rgb(255, 200, 80)));
                                             }
                                             if let Some(act) = &st.aircraft_type {
                                                 ui.label(RichText::new(format!("• {}", act)).size(10.0).color(Color32::from_rgb(140, 210, 255)));
                                             }
                                             if let Some(r) = &st.route {
                                                 ui.label(RichText::new(r).size(10.0).color(Color32::from_rgb(100, 230, 150)).strong());
                                             }
                                         });
                                     }

                                     // Third row: GPS coordinates and SHOW ON MAP button
                                     ui.horizontal(|ui| {
                                         if let (Some(la), Some(lo)) = (st.latitude, st.longitude) {
                                             let (dist_km, _, compass) = crate::gui::aprs_view::calculate_distance_bearing(self.center_qth.0, self.center_qth.1, la, lo);
                                             ui.label(
                                                 RichText::new(format!("📍 {:.4}° N, {:.4}° E ({:.0} km {})", la, lo, dist_km, compass))
                                                     .size(10.5)
                                                     .color(Color32::from_rgb(0, 255, 170))
                                                     .strong(),
                                             );
                                             let btn = ui.add(
                                                 egui::Button::new(
                                                     RichText::new("🗺 SHOW ON MAP").size(9.5).color(Color32::BLACK).strong(),
                                                 ).fill(Color32::from_rgb(0, 220, 255)),
                                             );
                                             if btn.clicked() {
                                                 self.selected_call = Some(st.callsign.clone());
                                                 self.center_override = Some((la, lo));
                                                 self.map_state.center_lat = la;
                                                 self.map_state.center_lon = lo;
                                                 self.map_state.zoom = 10.5;
                                             }
                                         } else {
                                             let pos_info = if let Some(wp) = &st.last_waypoint {
                                                 format!("📍 Waypoint: {}", wp)
                                             } else if let Some(orig) = &st.origin {
                                                 format!("📍 Ground Link / Station: {}", orig)
                                             } else {
                                                 "📍 Position: En route (GPS pending)".to_string()
                                             };
                                             let lbl = ui.label(RichText::new(pos_info).size(9.5).color(Color32::from_rgb(140, 160, 180)));
                                             lbl.on_hover_text(
                                                 "VHF ACARS / VDL2 (131–136 MHz) transmits operational airline data (routes, weather).\nContinuous real-time 2 Hz GPS transmission and radar position tracking occurs on 1090 MHz ADS-B."
                                             );
                                         }
                                     });

                                     // Fourth row: summary
                                     if !st.latest_summary.is_empty() {
                                         ui.add(egui::Label::new(RichText::new(&st.latest_summary).size(9.5).color(Color32::from_rgb(180, 200, 220))).wrap());
                                     }
                                 });
                            ui.add_space(2.0);
                        }
                    });
            });

        ui.add_space(4.0);

        // ====================================================================
        // B. DECODED MESSAGES (LIVE LOG)
        // ====================================================================
        ui.horizontal(|ui| {
            ui.label(RichText::new("DECODED MESSAGES (LIVE LOG)").size(11.0).color(Color32::from_rgb(0, 255, 170)).strong());
            ui.checkbox(&mut self.show_raw_bursts, RichText::new("Show RF energy bursts").size(10.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{} messages", self.history_packets.len())).size(10.0).color(Color32::from_rgb(140, 160, 180)));
            });
        });

        egui::Frame::none()
            .fill(Color32::from_rgb(10, 14, 20))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(25, 38, 55)))
            .rounding(4.0)
            .inner_margin(egui::Margin::symmetric(6.0, 4.0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("air_messages_scroll")
                    .max_height(bottom_box_h)
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut packets = self.history_packets.clone();
                        packets.reverse(); // Newest first

                        for p in packets {
                            let matches = search_lower.is_empty()
                                || p.callsign.to_lowercase().contains(&search_lower)
                                || p.tail.to_lowercase().contains(&search_lower)
                                || p.human_summary.to_lowercase().contains(&search_lower)
                                || p.raw_text.to_lowercase().contains(&search_lower);

                            if !matches {
                                continue;
                            }

                            egui::Frame::none()
                                .fill(Color32::from_rgb(14, 18, 26))
                                .stroke(Stroke::new(0.5_f32, Color32::from_rgb(30, 42, 58)))
                                .rounding(3.0)
                                .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&p.timestamp).monospace().size(10.0).color(Color32::from_rgb(120, 150, 180)));
                                        ui.label(RichText::new(&p.callsign).strong().size(10.5).color(Color32::from_rgb(0, 220, 255)));

                                        if !p.message_type.is_empty() {
                                             ui.label(RichText::new(&p.message_type).size(10.0).color(Color32::from_rgb(255, 190, 80)));
                                         }

                                         ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                             let is_freq_active = snapshot.vfos.iter().any(|v| v.freq_hz == p.freq_hz && (v.state == VfoState::Pinned || v.active) && !v.is_out_of_band);
                                             let btn_color = if is_freq_active { Color32::from_rgb(0, 255, 136) } else { Color32::from_rgb(100, 200, 150) };
                                             let freq_label = format!("{} {:.3}M", if is_freq_active { "●" } else { "" }, p.freq_hz as f64 / 1e6);
                                             let freq_btn = ui.small_button(RichText::new(freq_label).size(9.5).color(btn_color))
                                                 .on_hover_text("Monitor this frequency (parallel VFO)");
                                             if freq_btn.clicked() {
                                                 Self::toggle_channel(cmd_tx, scanner_state, snapshot, p.freq_hz, DemodMode::AM, &p.callsign, is_freq_active);
                                             }
                                         });
                                     });

                                     // Human readable summary
                                     ui.add(egui::Label::new(RichText::new(&p.human_summary).size(10.5).color(Color32::from_rgb(220, 235, 245)).strong()).wrap());

                                     // Quick jump to map if GPS coordinates exist
                                     if let (Some(la), Some(lo)) = (p.latitude, p.longitude) {
                                         ui.horizontal(|ui| {
                                             if ui.add(
                                                 egui::Button::new(
                                                     RichText::new(format!("📍 Coordinates: {:.4}° N, {:.4}° E  ➔  SHOW ON MAP", la, lo))
                                                         .size(9.5)
                                                         .strong()
                                                         .color(Color32::BLACK),
                                                 ).fill(Color32::from_rgb(0, 230, 255)),
                                             ).clicked() {
                                                 self.center_override = Some((la, lo));
                                                 self.map_state.zoom = 10.0;
                                                 self.selected_call = Some(p.callsign.clone());
                                             }
                                         });
                                     }
                                });
                            ui.add_space(2.0);
                        }
                    });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_air_view_qth_and_sync() {
        let mut air = AirView::new();
        air.set_qth((60.1699, 24.9384));
        assert_eq!(air.center_qth, (60.1699, 24.9384));
        assert_eq!(air.edit_qth_lat, "60.1699");
        assert_eq!(air.edit_qth_lon, "24.9384");

        let mut aprs = crate::gui::aprs_view::AprsView::new();
        aprs.set_qth(air.center_qth);
        assert_eq!(aprs.user_qth, (60.1699, 24.9384));
        assert_eq!(aprs.edit_qth_lat, "60.1699");
        assert_eq!(aprs.edit_qth_lon, "24.9384");
    }

    #[test]
    fn test_air_view_distance_to_qth() {
        let qth = (60.1699, 24.9384); // Helsinki
        let aircraft_pos = (61.4978, 23.7610); // Tampere
        let (dist, _, compass) = crate::gui::aprs_view::calculate_distance_bearing(qth.0, qth.1, aircraft_pos.0, aircraft_pos.1);
        assert!((dist - 161.0).abs() < 5.0);
        assert_eq!(compass, "NW");
    }

    #[test]
    fn test_air_coverage_and_snail_tail_and_timeout() {
        let mut air = AirView::new();
        air.set_qth((60.1699, 24.9384));

        let pkt1 = AirPacket {
            freq_hz: 1_090_000_000,
            timestamp: "12:00:00".to_string(),
            callsign: "FIN123".to_string(),
            tail: "OH-LWA".to_string(),
            airline: Some("Finnair".to_string()),
            aircraft_type: Some("Airbus A350".to_string()),
            origin: None,
            destination: None,
            route: None,
            altitude_ft: Some(35000),
            speed_kts: Some(480),
            heading_deg: Some(90.0),
            latitude: Some(60.50),
            longitude: Some(25.50),
            waypoint: None,
            message_type: "ADS-B Mode S".to_string(),
            human_summary: "Finnair FIN123 ADS-B".to_string(),
            raw_text: "ADS-B 1090".to_string(),
            signal_snr_db: 22.0,
        };

        air.ingest_packets(&[pkt1]);
        assert_eq!(air.saved_aircraft.len(), 1);
        let st = air.saved_aircraft.get("FIN123").unwrap();
        assert_eq!(st.track_history.len(), 1);
        assert!(air.coverage.max_range_km > 15.0);

        // Move plane to test snail tail growth
        let mut pkt2 = air.history_packets[0].clone();
        pkt2.latitude = Some(60.55);
        pkt2.longitude = Some(25.60);
        air.ingest_packets(&[pkt2]);
        let st = air.saved_aircraft.get("FIN123").unwrap();
        assert_eq!(st.track_history.len(), 2, "Snail tail must grow with position updates");

        // Test 1-hour timeout eviction
        if let Some(plane) = air.saved_aircraft.get_mut("FIN123") {
            plane.last_seen = Instant::now() - std::time::Duration::from_secs(3601);
        }
        air.ingest_packets(&[]);
        assert_eq!(air.saved_aircraft.len(), 0, "Aircraft older than 1 hour must be evicted");
    }
}
