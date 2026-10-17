use crate::config::DemodMode;
use crate::dsp::AprsPacket;
use crate::dsp_worker::{DisplaySnapshot, DspCommand};
use crate::gui::slippy_map::{MapStyle, SlippyMapState, TileManager};
use crate::scanner::ScannerState;
use crossbeam_channel::Sender;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};
use std::collections::HashMap;



// ============================================================================
// APRS STORAGE (Persistent heard stations to aprs_stations.json)
// ============================================================================

fn get_aprs_storage_path() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("aprs_stations.json");
            if p.exists() {
                return p;
            }
        }
    }
    let local = std::path::PathBuf::from("aprs_stations.json");
    if local.exists() {
        return local;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("aprs_stations.json");
        }
    }
    local
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AprsStorageData {
    pub user_qth_lat: f64,
    pub user_qth_lon: f64,
    pub stations: Vec<AprsPacket>,
    #[serde(default)]
    pub station_tracks: HashMap<String, Vec<(f64, f64, String)>>,
}

pub fn save_aprs_data(user_qth: (f64, f64), stations: &[AprsPacket], tracks: &HashMap<String, Vec<(f64, f64, String)>>) {
    let path = get_aprs_storage_path();
    let data = AprsStorageData {
        user_qth_lat: user_qth.0,
        user_qth_lon: user_qth.1,
        stations: stations.to_vec(),
        station_tracks: tracks.clone(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&data) {
        let _ = std::fs::write(path, json);
    }
}

pub fn load_aprs_data() -> (Option<(f64, f64)>, Vec<AprsPacket>, HashMap<String, Vec<(f64, f64, String)>>) {
    let path = get_aprs_storage_path();
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(path) {
            if let Ok(parsed) = serde_json::from_str::<AprsStorageData>(&data) {
                return (Some((parsed.user_qth_lat, parsed.user_qth_lon)), parsed.stations, parsed.station_tracks);
            }
            if let Ok(packets) = serde_json::from_str::<Vec<AprsPacket>>(&data) {
                return (None, packets, HashMap::new());
            }
        }
    }
    (None, Vec::new(), HashMap::new())
}

pub fn load_user_qth() -> Option<(f64, f64)> {
    let (qth, _, _) = load_aprs_data();
    qth
}

#[allow(dead_code)]
pub fn save_user_qth_only(user_qth: (f64, f64)) {
    let (_, stations, tracks) = load_aprs_data();
    save_aprs_data(user_qth, &stations, &tracks);
}

// ============================================================================
// APRS VIEW UI COMPONENT
// ============================================================================

/// Dedicated UI popup with persistent APRS station monitor and 2D global vector map
pub struct AprsView {
    pub is_open: bool,
    pub filter: String,
    pub selected_call: Option<String>,
    pub center_override: Option<(f64, f64)>,
    pub auto_scroll: bool,
    pub user_qth: (f64, f64),
    pub edit_qth_lat: String,
    pub edit_qth_lon: String,
    pub saved_stations: HashMap<String, AprsPacket>,
    pub history_packets: Vec<AprsPacket>,
    pub station_tracks: HashMap<String, Vec<(f64, f64, String)>>,
    pub dirty: bool,
    pub last_processed_packet_id: u64,
    pub map_state: SlippyMapState,
    pub tile_mgr: TileManager,
}

impl AprsView {
    pub fn new() -> Self {
        let (saved_qth, loaded, tracks) = load_aprs_data();
        let user_qth = saved_qth.unwrap_or((60.20, 24.90));
        let mut map = HashMap::new();
        for p in &loaded {
            map.insert(p.source.clone(), p.clone());
        }

        Self {
            is_open: false,
            filter: String::new(),
            selected_call: None,
            center_override: None,
            auto_scroll: true,
            user_qth,
            edit_qth_lat: format!("{:.4}", user_qth.0),
            edit_qth_lon: format!("{:.4}", user_qth.1),
            saved_stations: map,
            history_packets: loaded,
            station_tracks: tracks,
            dirty: false,
            last_processed_packet_id: 0,
            map_state: SlippyMapState::new(user_qth.0, user_qth.1, 8.5),
            tile_mgr: TileManager::new(),
        }
    }

    pub fn set_qth(&mut self, qth: (f64, f64)) {
        self.user_qth = qth;
        self.edit_qth_lat = format!("{:.4}", qth.0);
        self.edit_qth_lon = format!("{:.4}", qth.1);
    }

    pub fn save_current(&self) {
        let list: Vec<AprsPacket> = self.saved_stations.values().cloned().collect();
        save_aprs_data(self.user_qth, &list, &self.station_tracks);
    }

    /// Renders the APRS Monitor & Tactical Map window
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

        self.tile_mgr.update(ctx);

        let mut is_open = self.is_open;
        egui::Window::new("📡 APRS TRACKER & GLOBAL MAP (144.800 MHz)")
            .open(&mut is_open)
            .default_size([980.0, 620.0])
            .min_size([560.0, 360.0])
            .max_size([1150.0, 850.0])
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
        // Integrate new incoming live packets strictly by sequential packet ID
        for p in &snap.aprs_packets {
            if p.id > self.last_processed_packet_id {
                self.last_processed_packet_id = p.id;

                // Update station in saved_stations
                if let Some(existing) = self.saved_stations.get_mut(&p.source) {
                    existing.timestamp = p.timestamp.clone();
                    existing.payload = p.payload.clone();
                    existing.comment = p.comment.clone();
                    existing.destination = p.destination.clone();
                    existing.digipeaters = p.digipeaters.clone();
                    // Crucial: PRESERVE GPS coordinates if new packet doesn't have them!
                    if let (Some(lat), Some(lon)) = (p.latitude, p.longitude) {
                        existing.latitude = Some(lat);
                        existing.longitude = Some(lon);
                        let track = self.station_tracks.entry(p.source.clone()).or_default();
                        track.push((lat, lon, p.timestamp.clone()));
                        if track.len() > 50 {
                            track.remove(0);
                        }
                    }
                } else {
                    if let (Some(lat), Some(lon)) = (p.latitude, p.longitude) {
                        let track = self.station_tracks.entry(p.source.clone()).or_default();
                        track.push((lat, lon, p.timestamp.clone()));
                        if track.len() > 50 {
                            track.remove(0);
                        }
                    }
                    self.saved_stations.insert(p.source.clone(), p.clone());
                }

                self.history_packets.push(p.clone());
                if self.history_packets.len() > 1000 {
                    self.history_packets.remove(0);
                }
                self.dirty = true;
            }
        }

        if self.dirty {
            let list: Vec<AprsPacket> = self.saved_stations.values().cloned().collect();
            save_aprs_data(self.user_qth, &list, &self.station_tracks);
            self.dirty = false;
        }

        // Filter packets
        let search_lower = self.filter.trim().to_lowercase();
        let filtered_packets: Vec<AprsPacket> = self.history_packets
            .iter()
            .filter(|p| {
                if search_lower.is_empty() {
                    return true;
                }
                p.source.to_lowercase().contains(&search_lower)
                    || p.destination.to_lowercase().contains(&search_lower)
                    || p.comment.to_lowercase().contains(&search_lower)
                    || p.payload.to_lowercase().contains(&search_lower)
            })
            .cloned()
            .collect();

        // 1. TOP TOOLBAR
        ui.horizontal(|ui| {
            // Direct 1-click Quick-Tune to APRS frequency
            let tune_aprs = ui.add(
                egui::Button::new(
                    RichText::new("🎯 Tune 144.800 MHz")
                        .size(11.0)
                        .color(Color32::BLACK)
                        .strong(),
                )
                .fill(Color32::from_rgb(0, 230, 255)),
            );
            if tune_aprs.clicked() {
                *scanner_state = ScannerState::Paused;
                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                let _ = cmd_tx.send(DspCommand::TuneDataVfo(0, 144_800_000, DemodMode::NFM, "APRS 144.800".to_string()));
            }
            tune_aprs.on_hover_text("Pause scanner and tune to APRS 144.800 MHz (Dedicated data VFO, open squelch, muted audio)");

            ui.separator();

            ui.label(RichText::new("🔍").size(13.0));
            let search_edit = egui::TextEdit::singleline(&mut self.filter)
                .hint_text("Search callsign or text...")
                .desired_width(140.0);
            ui.add(search_edit);

            if !self.filter.is_empty() && ui.small_button("✖").clicked() {
                self.filter.clear();
            }

            ui.separator();

            // Map Style selector (100% keyless OpenStreetMap ecosystem)
            egui::ComboBox::from_id_salt("aprs_map_style_combo")
                .selected_text(self.map_state.style.name())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.map_state.style, MapStyle::OpenStreetMap, "🌍 OpenStreetMap");
                    ui.selectable_value(&mut self.map_state.style, MapStyle::DarkTactical, "🌙 Dark Tactical");
                });

            // Zoom controls
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
            if ui.button(RichText::new("🏠 Home").size(11.0)).clicked() {
                self.map_state.center_lat = self.user_qth.0;
                self.map_state.center_lon = self.user_qth.1;
                self.map_state.zoom = 9.0;
            }

            ui.separator();

            // User QTH / Home Location configuration (atomic horizontal block - never splits!)
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
                        self.save_current();
                    }
                }
            });

            ui.separator();
            ui.checkbox(&mut self.auto_scroll, "Auto-scroll");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(RichText::new("🗑️ Clear All").size(11.0).color(Color32::from_rgb(255, 120, 120))).clicked() {
                    self.last_processed_packet_id = snap.aprs_packets.last().map(|p| p.id).unwrap_or(0);
                    self.saved_stations.clear();
                    self.history_packets.clear();
                    self.station_tracks.clear();
                    save_aprs_data(self.user_qth, &[], &self.station_tracks);
                    self.selected_call = None;
                }

                ui.label(
                    RichText::new(format!(
                        "{} saved stations ({} packets)",
                        self.saved_stations.len(),
                        filtered_packets.len()
                    ))
                    .size(11.0)
                    .color(Color32::GRAY),
                );
            });
        });

        ui.add_space(3.0);
        ui.separator();

        // 2. MAIN SPLIT: GEOGRAPHIC MAP (LEFT 54%) & STATION / PACKET LOG (RIGHT 46%)
        let avail_w = ui.available_width();
        let avail_h = ui.available_height().max(200.0);
        let map_w = (avail_w * 0.54).max(300.0);
        let list_w = (avail_w - map_w - 10.0).max(240.0);

        ui.horizontal(|ui| {
            // --- LEFT: GEOGRAPHIC 2D MAP WIDGET ---
            ui.allocate_ui_with_layout(
                egui::vec2(map_w, avail_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    self.render_tactical_map(ui, map_w, avail_h);
                },
            );

            ui.separator();

            // --- RIGHT: STATION DETAIL & PACKET TABLE ---
            ui.allocate_ui_with_layout(
                egui::vec2(list_w, avail_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    self.render_station_panel(ui, ctx, &filtered_packets, list_w, avail_h, cmd_tx, scanner_state);
                },
            );
        });
    }

    /// Renders the geographic 2D map with world vector coastlines, lakes, cities, and APRS stations
    fn render_tactical_map(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        height: f32,
    ) {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        // Fill nautical chart background
        painter.rect_filled(rect, 4.0, Color32::from_rgb(10, 16, 26));
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, Color32::from_rgb(26, 42, 62)));

        if let Some(center) = self.center_override {
            self.map_state.center_lat = center.0;
            self.map_state.center_lon = center.1;
            self.center_override = None;
        }

        // Handle interactive drag and mouse wheel zoom on OpenStreetMap
        self.map_state.handle_interaction(ui, rect, &resp);

        // Render OpenStreetMap / Voyager / Dark tiles with smooth LOD scaling and disk cache
        self.map_state.render_tiles(&painter, rect, &mut self.tile_mgr);

        // Project latitude & longitude to screen Pos2 via Web Mercator projection
        let to_pos = |lat: f64, lon: f64| -> Pos2 {
            self.map_state.lat_lon_to_screen(lat, lon, rect)
        };

        let home_pos = to_pos(self.user_qth.0, self.user_qth.1);

        // --- LAYER 5: DRAW SNAIL TRAILS (LAST 50 PACKETS FOR MOVED STATIONS) ---
        for (_call, track) in &self.station_tracks {
            if track.len() >= 2 {
                let (first_lat, first_lon, _) = track[0];
                let has_moved = track.iter().any(|(lat, lon, _)| {
                    (*lat - first_lat).abs() > 0.0001 || (*lon - first_lon).abs() > 0.0001
                });

                if has_moved {
                    let n = track.len();
                    for i in 0..n - 1 {
                        let p1 = to_pos(track[i].0, track[i].1);
                        let p2 = to_pos(track[i + 1].0, track[i + 1].1);
                        if rect.contains(p1) || rect.contains(p2) {
                            let progress = (i + 1) as f32 / n as f32;
                            let alpha = (50.0 + progress * 180.0) as u8;
                            let trail_color = Color32::from_rgba_premultiplied(220, 25, 45, alpha);
                            painter.line_segment([p1, p2], Stroke::new(2.2_f32, trail_color));

                            // Breadcrumb dot along snail trail
                            painter.circle_filled(p1, 2.2, Color32::from_rgba_premultiplied(20, 20, 20, alpha));
                            painter.circle_filled(p1, 1.5, trail_color);
                        }
                    }
                }
            }
        }

        // --- LAYER 6: USER HOME QTH MARKER (HIGH-CONTRAST ROYAL BLUE PIN) ---
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

        // --- LAYER 7: PLOT RECEIVED STATIONS (ALL UNIFIED HIGH-CONTRAST BOLD RED) ---
        let mut clicked_call = None;
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());

        for (call, pkt) in &self.saved_stations {
            if let (Some(lat), Some(lon)) = (pkt.latitude, pkt.longitude) {
                let station_pos = to_pos(lat, lon);

                // Only render if within map rectangle bounds
                if !rect.contains(station_pos) {
                    continue;
                }

                let is_selected = self.selected_call.as_deref() == Some(call.as_str());
                let is_hovered = pointer_pos.map_or(false, |p| p.distance(station_pos) < 14.0);

                if is_hovered && resp.clicked() {
                    clicked_call = Some(call.clone());
                }

                // If selected, draw vector line from Home QTH to this station with distance badge
                if is_selected && rect.contains(home_pos) {
                    painter.line_segment([home_pos, station_pos], Stroke::new(1.5_f32, Color32::from_rgb(255, 180, 0)));
                    let mid_pos = Pos2::new((home_pos.x + station_pos.x) * 0.5, (home_pos.y + station_pos.y) * 0.5);
                    let (dist_km, _, compass) = calculate_distance_bearing(self.user_qth.0, self.user_qth.1, lat, lon);
                    painter.rect_filled(
                        egui::Rect::from_center_size(mid_pos, egui::vec2(74.0, 16.0)),
                        3.0,
                        Color32::from_rgb(14, 24, 38),
                    );
                    painter.text(
                        mid_pos,
                        egui::Align2::CENTER_CENTER,
                        format!("{:.1} km {}", dist_km, compass),
                        egui::FontId::monospace(9.5),
                        Color32::from_rgb(255, 215, 0),
                    );
                }

                // Unified high-contrast color for ALL received stations:
                let station_col = Color32::from_rgb(220, 20, 45); // Bold Crimson Red

                // Draw pulsing ring for selected or hovered
                if is_selected {
                    painter.circle_stroke(station_pos, 10.0, Stroke::new(2.0_f32, Color32::from_rgb(255, 215, 0)));
                } else if is_hovered {
                    painter.circle_stroke(station_pos, 8.0, Stroke::new(1.2_f32, Color32::WHITE));
                }

                // Station blip: dark shadow, crimson body, crisp white center dot
                painter.circle_filled(station_pos, 6.0, Color32::from_rgb(15, 15, 20));
                painter.circle_filled(station_pos, 4.8, station_col);
                painter.circle_filled(station_pos, 1.8, Color32::WHITE);
                painter.circle_stroke(station_pos, 4.8, Stroke::new(1.0_f32, Color32::from_rgb(20, 20, 20)));

                // Callsign label with high-contrast dark background pill (super legible on white/light maps)
                let text_pos = Pos2::new(station_pos.x, station_pos.y - 12.0);
                let label_w = (call.len() as f32 * 6.8 + 8.0).max(36.0);
                let text_rect = Rect::from_center_size(text_pos, egui::vec2(label_w, 14.0));
                painter.rect_filled(
                    text_rect,
                    2.5,
                    if is_selected {
                        Color32::from_rgba_premultiplied(190, 25, 45, 235)
                    } else {
                        Color32::from_rgba_premultiplied(14, 20, 30, 215)
                    },
                );
                painter.rect_stroke(
                    text_rect,
                    2.5,
                    Stroke::new(0.8_f32, if is_selected { Color32::from_rgb(255, 215, 0) } else { Color32::from_rgb(50, 70, 95) }),
                );
                painter.text(
                    text_pos,
                    egui::Align2::CENTER_CENTER,
                    call,
                    egui::FontId::monospace(if is_selected { 10.0 } else { 9.0 }),
                    Color32::WHITE,
                );
            }
        }

        if let Some(c) = clicked_call {
            self.selected_call = Some(c);
        }

        // Tactical scale bar, navigation buttons, and coordinates overlay
        self.map_state.render_controls_overlay(ui, rect, self.user_qth);
    }

    /// Renders the station detail panel and packet table
    fn render_station_panel(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        packets: &[AprsPacket],
        _width: f32,
        height: f32,
        cmd_tx: &Sender<DspCommand>,
        scanner_state: &mut ScannerState,
    ) {
        let detail_h = 160.0_f32;
        let table_h = (height - detail_h - 15.0).max(100.0_f32);

        // 1. SELECTED STATION DETAILS CARD
        let selected_p = self
            .selected_call
            .as_ref()
            .and_then(|c| self.saved_stations.get(c))
            .or_else(|| packets.last());

        if let Some(pkt) = selected_p {
            egui::Frame::none()
                .fill(Color32::from_rgb(14, 22, 34))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 52, 76)))
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&pkt.source)
                                .size(16.0)
                                .color(Color32::from_rgb(0, 230, 255))
                                .monospace()
                                .strong(),
                        );
                        ui.label(RichText::new(format!("➔ {}", pkt.destination)).size(11.0).color(Color32::GRAY).monospace());

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let tune_btn = ui.add(
                                egui::Button::new(RichText::new("🎯 Tune 144.800 MHz").size(10.5).color(Color32::BLACK).strong())
                                    .fill(Color32::from_rgb(0, 220, 255)),
                            );
                            if tune_btn.clicked() {
                                *scanner_state = ScannerState::Paused;
                                let _ = cmd_tx.send(DspCommand::SetScannerState(ScannerState::Paused));
                                let _ = cmd_tx.send(DspCommand::TuneDataVfo(0, 144_800_000, DemodMode::NFM, format!("APRS {}", pkt.source)));
                            }
                        });
                    });

                    ui.add_space(2.0);

                    // Position, Maidenhead Grid & Distance from QTH
                    if let (Some(lat), Some(lon)) = (pkt.latitude, pkt.longitude) {
                        let grid = lat_lon_to_maidenhead(lat, lon);
                        let (dist_km, bearing_deg, compass) = calculate_distance_bearing(self.user_qth.0, self.user_qth.1, lat, lon);

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("📍 {:.4}°N, {:.4}°E  [{}]", lat, lon, grid))
                                    .size(11.0)
                                    .color(Color32::from_rgb(50, 255, 130))
                                    .monospace()
                                    .strong(),
                            );
                            ui.separator();
                            ui.label(
                                RichText::new(format!("📏 {:.1} km ({}° {}) from Home", dist_km, bearing_deg as u32, compass))
                                    .size(11.0)
                                    .color(Color32::from_rgb(255, 215, 0))
                                    .strong(),
                            );
                        });

                        ui.add_space(2.0);

                        // Direct one-click web map buttons
                        ui.horizontal(|ui| {
                            let osm_btn = ui.add(
                                egui::Button::new(RichText::new("🌍 OpenStreetMap").size(10.5).color(Color32::BLACK).strong())
                                    .fill(Color32::from_rgb(120, 220, 140)),
                            );
                            if osm_btn.clicked() {
                                let url = format!("https://www.openstreetmap.org/?mlat={:.5}&mlon={:.5}#map=13/{:.5}/{:.5}", lat, lon, lat, lon);
                                ctx.open_url(egui::OpenUrl::new_tab(url));
                            }
                            osm_btn.on_hover_text("Open exact station coordinates in OpenStreetMap in your browser");

                            let aprs_btn = ui.add(
                                egui::Button::new(RichText::new("📡 APRS.fi").size(10.5).color(Color32::BLACK).strong())
                                    .fill(Color32::from_rgb(255, 180, 80)),
                            );
                            if aprs_btn.clicked() {
                                let url = format!("https://aprs.fi/info/a/{}", pkt.source);
                                ctx.open_url(egui::OpenUrl::new_tab(url));
                            }
                            aprs_btn.on_hover_text("Open live station track and history on APRS.fi in your browser");
                        });
                    } else {
                        ui.label(RichText::new("📍 No GPS coordinates in payload").size(11.0).color(Color32::GRAY));
                    }

                    // Comment / Weather / Telemetry
                    if !pkt.comment.is_empty() {
                        ui.label(
                            RichText::new(format!("\"{}\"", pkt.comment))
                                .size(11.0)
                                .color(Color32::from_rgb(255, 220, 130))
                                .italics(),
                        );
                    }

                    // Raw AX.25 info
                    ui.label(
                        RichText::new(format!("Time: {} | Digis: {}", pkt.timestamp, pkt.digipeaters.join(", ")))
                            .size(9.5)
                            .color(Color32::from_rgb(140, 160, 180)),
                    );
                });
        }

        ui.add_space(3.0);

        // 2. CHRONOLOGICAL PACKET TABLE
        egui::Frame::none()
            .fill(Color32::from_rgb(12, 18, 28))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(24, 36, 52)))
            .inner_margin(4.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("PACKET LOG (PERSISTED)").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{} packets", packets.len())).size(10.0).color(Color32::GRAY));
                    });
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
                    if packets.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);
                            ui.label(RichText::new("No APRS packets heard yet on 144.800 MHz.").color(Color32::GRAY));
                            ui.add_space(20.0);
                        });
                        return;
                    }

                    for pkt in packets {
                        let is_selected = self.selected_call.as_deref() == Some(&pkt.source);
                        let row_bg = if is_selected { Color32::from_rgb(24, 48, 76) } else { Color32::TRANSPARENT };

                        egui::Frame::none()
                            .fill(row_bg)
                            .inner_margin(egui::Margin::symmetric(2.0, 2.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&pkt.timestamp).size(10.0).color(Color32::GRAY).monospace());

                                    let call_btn = ui.selectable_label(
                                        is_selected,
                                        RichText::new(&pkt.source).size(11.0).color(Color32::from_rgb(0, 230, 255)).monospace().strong(),
                                    );
                                    if call_btn.clicked() {
                                        self.selected_call = Some(pkt.source.clone());
                                    }

                                    if let (Some(lat), Some(lon)) = (pkt.latitude, pkt.longitude) {
                                        let (dist_km, _, compass) = calculate_distance_bearing(self.user_qth.0, self.user_qth.1, lat, lon);
                                        ui.label(
                                            RichText::new(format!("{:.1}km {}", dist_km, compass))
                                                .size(9.5)
                                                .color(Color32::from_rgb(255, 215, 0))
                                                .monospace(),
                                        );
                                    }

                                    let comment_snip = if pkt.comment.len() > 26 {
                                        format!("{}...", &pkt.comment[..26])
                                    } else {
                                        pkt.comment.clone()
                                    };
                                    ui.label(RichText::new(comment_snip).size(10.0).color(Color32::from_rgb(220, 220, 220)));
                                });
                            });
                    }
                });
            });
    }
}

/// Computes great-circle distance (km), forward bearing (degrees), and 8-point compass direction
pub fn calculate_distance_bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> (f64, f64, &'static str) {
    let r = 6371.0; // Earth radius in kilometers
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();

    let a = (d_lat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    let dist_km = r * c;

    let y = d_lon.sin() * lat2_rad.cos();
    let x = lat1_rad.cos() * lat2_rad.sin() - lat1_rad.sin() * lat2_rad.cos() * d_lon.cos();
    let bearing_deg = (y.atan2(x).to_degrees() + 360.0) % 360.0;

    let compass = match bearing_deg {
        b if b >= 337.5 || b < 22.5 => "N",
        b if (22.5..67.5).contains(&b) => "NE",
        b if (67.5..112.5).contains(&b) => "E",
        b if (112.5..157.5).contains(&b) => "SE",
        b if (157.5..202.5).contains(&b) => "S",
        b if (202.5..247.5).contains(&b) => "SW",
        b if (247.5..292.5).contains(&b) => "W",
        _ => "NW",
    };

    (dist_km, bearing_deg, compass)
}

/// Converts decimal degrees latitude & longitude to 6-character Maidenhead locator
pub fn lat_lon_to_maidenhead(lat: f64, lon: f64) -> String {
    let lon_adj = (lon + 180.0).clamp(0.0, 360.0);
    let lat_adj = (lat + 90.0).clamp(0.0, 180.0);

    let f1 = (b'A' + (lon_adj / 20.0) as u8) as char;
    let f2 = (b'A' + (lat_adj / 10.0) as u8) as char;
    let f3 = (b'0' + ((lon_adj % 20.0) / 2.0) as u8) as char;
    let f4 = (b'0' + ((lat_adj % 10.0) / 1.0) as u8) as char;
    let f5 = (b'A' + (((lon_adj % 2.0) * 12.0) as u8)) as char;
    let f6 = (b'A' + (((lat_adj % 1.0) * 24.0) as u8)) as char;

    format!("{}{}{}{}{}{}", f1, f2, f3, f4, f5, f6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_distance_bearing_helsinki_to_tampere() {
        let (dist, bearing, compass) = calculate_distance_bearing(60.17, 24.94, 61.50, 23.76);
        assert!((dist - 161.0).abs() < 5.0, "Helsinki-Tampere distance should be ~161 km, got {}", dist);
        assert_eq!(compass, "NW");
        assert!(bearing > 300.0 && bearing < 350.0);
    }

    #[test]
    fn test_aprs_storage_data_serialization_and_qth() {
        let mut tracks = HashMap::new();
        tracks.insert("OH2XYZ-9".to_string(), vec![
            (60.20, 24.90, "12:00:00".to_string()),
            (60.21, 24.92, "12:01:00".to_string()),
        ]);

        let dummy_pkt = AprsPacket {
            id: 1,
            source: "OH2XYZ-9".to_string(),
            destination: "APRS".to_string(),
            digipeaters: vec!["WIDE1-1".to_string()],
            payload: "=6012.00N/02454.00E#Car".to_string(),
            latitude: Some(60.20),
            longitude: Some(24.90),
            comment: "Mobile".to_string(),
            timestamp: "12:00:00".to_string(),
        };

        let storage = AprsStorageData {
            user_qth_lat: 60.1699,
            user_qth_lon: 24.9384,
            stations: vec![dummy_pkt],
            station_tracks: tracks,
        };

        let json = serde_json::to_string(&storage).expect("Failed to serialize AprsStorageData");
        let parsed: AprsStorageData = serde_json::from_str(&json).expect("Failed to deserialize AprsStorageData");

        assert_eq!(parsed.user_qth_lat, 60.1699);
        assert_eq!(parsed.user_qth_lon, 24.9384);
        assert_eq!(parsed.stations.len(), 1);
        assert_eq!(parsed.stations[0].source, "OH2XYZ-9");
        let track = parsed.station_tracks.get("OH2XYZ-9").expect("Missing track for OH2XYZ-9");
        assert_eq!(track.len(), 2);
    }

    #[test]
    fn test_snail_tail_capping_at_50_packets() {
        let mut track: Vec<(f64, f64, String)> = Vec::new();
        for i in 0..60 {
            track.push((60.0 + (i as f64 * 0.001), 24.0 + (i as f64 * 0.001), format!("12:{:02}:00", i)));
            if track.len() > 50 {
                track.remove(0);
            }
        }
        assert_eq!(track.len(), 50);
        assert!((track[0].0 - (60.0 + 10.0 * 0.001)).abs() < 1e-6);
        assert!((track.last().unwrap().0 - (60.0 + 59.0 * 0.001)).abs() < 1e-6);
    }
}
