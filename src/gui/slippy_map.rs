use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Stroke, TextureHandle, TextureOptions, Vec2};

/// Supported online tile providers (100% keyless OpenStreetMap ecosystem)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MapStyle {
    /// OpenStreetMap Standard: Comprehensive community street map
    OpenStreetMap,
    /// Dark Tactical: High-contrast night radar dark theme transformed from OSM (zero API key needed)
    DarkTactical,
}

impl MapStyle {
    pub fn name(&self) -> &'static str {
        match self {
            MapStyle::OpenStreetMap => "OpenStreetMap",
            MapStyle::DarkTactical => "Dark Tactical",
        }
    }

    pub fn slug(&self) -> &'static str {
        match self {
            MapStyle::OpenStreetMap => "osm",
            MapStyle::DarkTactical => "dark_canvas",
        }
    }

    pub fn url_template(&self, z: u32, x: u32, y: u32) -> String {
        match self {
            MapStyle::OpenStreetMap => {
                let sub = ["a", "b", "c"][(x + y) as usize % 3];
                format!("https://{}.tile.openstreetmap.org/{}/{}/{}.png", sub, z, x, y)
            }
            MapStyle::DarkTactical => {
                // Esri World Dark Gray Base MapServer tile format: /{level}/{row}/{column} -> {z}/{y}/{x}
                format!("https://services.arcgisonline.com/arcgis/rest/services/Canvas/World_Dark_Gray_Base/MapServer/tile/{}/{}/{}", z, y, x)
            }
        }
    }
}

/// Unique identifier for a 256x256 map tile
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub style: MapStyle,
    pub z: u32,
    pub x: u32,
    pub y: u32,
}

/// Background tile manager with persistent disk caching and async fetching
pub struct TileManager {
    _cache_dir: PathBuf,
    textures: HashMap<TileKey, TextureHandle>,
    in_flight: HashSet<TileKey>,
    req_tx: Sender<TileKey>,
    loaded_rx: Receiver<(TileKey, Vec<u8>, u32, u32)>,
    _stop: Arc<AtomicBool>,
}

impl TileManager {
    pub fn new() -> Self {
        let cache_dir = get_tile_cache_path();
        let _ = std::fs::create_dir_all(&cache_dir);

        let (req_tx, req_rx) = unbounded::<TileKey>();
        let (loaded_tx, loaded_rx) = unbounded::<(TileKey, Vec<u8>, u32, u32)>();
        let stop = Arc::new(AtomicBool::new(false));

        // Spawn 3 background download worker threads
        for _ in 0..3 {
            let rx = req_rx.clone();
            let tx = loaded_tx.clone();
            let dir = cache_dir.clone();
            let stop_flag = stop.clone();

            std::thread::spawn(move || {
                while !stop_flag.load(Ordering::Relaxed) {
                    let key = match rx.recv() {
                        Ok(k) => k,
                        Err(_) => break,
                    };

                    let tile_file = dir
                        .join(key.style.slug())
                        .join(format!("{}", key.z))
                        .join(format!("{}_{}.img", key.x, key.y));

                    // 1. Try reading from persistent disk cache
                    let bytes = if tile_file.exists() {
                        std::fs::read(&tile_file).ok()
                    } else {
                        // 2. Fetch from HTTP tile server
                        let url = key.style.url_template(key.z, key.x, key.y);
                        let resp_result = ureq::get(&url)
                            .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) SpectraScanner/0.1.0")
                            .timeout(std::time::Duration::from_secs(6))
                            .call();

                        match resp_result {
                            Ok(resp) => {
                                let mut buf = Vec::new();
                                if resp.into_reader().read_to_end(&mut buf).is_ok() && !buf.is_empty() {
                                    if let Some(parent) = tile_file.parent() {
                                        let _ = std::fs::create_dir_all(parent);
                                    }
                                    let _ = std::fs::write(&tile_file, &buf);
                                    Some(buf)
                                } else {
                                    None
                                }
                            }
                            Err(_) => None,
                        }
                    };

                    // 3. Decode image (PNG or JPEG) to raw RGBA8
                    if let Some(raw_bytes) = bytes {
                        if let Ok(img) = image::load_from_memory(&raw_bytes) {
                            let rgba = img.to_rgba8();
                            let (w, h) = rgba.dimensions();
                            let _ = tx.send((key, rgba.into_raw(), w, h));
                        }
                    }
                }
            });
        }

        Self {
            _cache_dir: cache_dir,
            textures: HashMap::with_capacity(512),
            in_flight: HashSet::with_capacity(128),
            req_tx,
            loaded_rx,
            _stop: stop,
        }
    }

    /// Pulls newly decoded tile bitmaps from worker threads and uploads them to the GPU
    pub fn update(&mut self, ctx: &egui::Context) {
        let mut count = 0;
        while let Ok((key, raw, w, h)) = self.loaded_rx.try_recv() {
            self.in_flight.remove(&key);
            let name = format!("tile_{}_{}_{}_{}", key.style.slug(), key.z, key.x, key.y);
            let color_img = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &raw);
            let tex = ctx.load_texture(name, color_img, TextureOptions::LINEAR);
            self.textures.insert(key, tex);

            count += 1;
            if count > 40 {
                // Yield to next frame to keep 60 FPS
                break;
            }
        }

        // Limit GPU memory by dropping old textures if cache exceeds 800 items
        if self.textures.len() > 800 {
            // Keep roughly 500 tiles
            let keys_to_remove: Vec<TileKey> = self.textures.keys().take(300).copied().collect();
            for k in keys_to_remove {
                self.textures.remove(&k);
            }
        }
    }

    /// Requests a tile if not already loaded or in flight
    pub fn request_tile(&mut self, key: TileKey) {
        if !self.textures.contains_key(&key) && !self.in_flight.contains(&key) {
            self.in_flight.insert(key);
            let _ = self.req_tx.send(key);
        }
    }

    pub fn get_texture(&self, key: &TileKey) -> Option<&TextureHandle> {
        self.textures.get(key)
    }
}

fn get_tile_cache_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("tile_cache");
        }
    }
    PathBuf::from("tile_cache")
}

/// Interactive Slippy Map component (Web Mercator EPSG:3857)
#[derive(Clone, Debug)]
pub struct SlippyMapState {
    pub center_lat: f64,
    pub center_lon: f64,
    pub zoom: f64,
    pub style: MapStyle,
    pub is_dragging: bool,
    pub drag_start_mouse: Option<Pos2>,
    pub drag_start_center: Option<(f64, f64)>,
}

impl Default for SlippyMapState {
    fn default() -> Self {
        Self {
            center_lat: 60.1699,  // Helsinki, Finland
            center_lon: 24.9384,
            zoom: 8.5,            // Regional view of Southern Finland & Gulf of Finland
            style: MapStyle::OpenStreetMap,
            is_dragging: false,
            drag_start_mouse: None,
            drag_start_center: None,
        }
    }
}

impl SlippyMapState {
    pub fn new(center_lat: f64, center_lon: f64, zoom: f64) -> Self {
        Self {
            center_lat,
            center_lon,
            zoom: zoom.clamp(1.0, 18.0),
            style: MapStyle::OpenStreetMap,
            is_dragging: false,
            drag_start_mouse: None,
            drag_start_center: None,
        }
    }

    /// World width and height in pixels at current zoom level
    #[inline]
    pub fn world_size(&self) -> f64 {
        256.0 * 2.0_f64.powf(self.zoom)
    }

    /// Converts latitude & longitude to screen coordinates Pos2
    pub fn lat_lon_to_screen(&self, lat: f64, lon: f64, rect: Rect) -> Pos2 {
        let w = self.world_size();
        let center_norm_x = (self.center_lon + 180.0) / 360.0;
        let center_lat_clamped = self.center_lat.clamp(-85.0511, 85.0511);
        let center_lat_rad = center_lat_clamped.to_radians();
        let center_norm_y = (1.0 - (center_lat_rad.tan() + 1.0 / center_lat_rad.cos()).ln() / std::f64::consts::PI) / 2.0;

        let center_wx = center_norm_x * w;
        let center_wy = center_norm_y * w;

        let pt_norm_x = (lon + 180.0) / 360.0;
        let pt_lat_clamped = lat.clamp(-85.0511, 85.0511);
        let pt_lat_rad = pt_lat_clamped.to_radians();
        let pt_norm_y = (1.0 - (pt_lat_rad.tan() + 1.0 / pt_lat_rad.cos()).ln() / std::f64::consts::PI) / 2.0;

        let pt_wx = pt_norm_x * w;
        let pt_wy = pt_norm_y * w;

        let sc = rect.center();
        let sx = sc.x + (pt_wx - center_wx) as f32;
        let sy = sc.y + (pt_wy - center_wy) as f32;

        Pos2::new(sx, sy)
    }

    /// Converts screen coordinate Pos2 to (latitude, longitude)
    pub fn screen_to_lat_lon(&self, pos: Pos2, rect: Rect) -> (f64, f64) {
        let w = self.world_size();
        let center_norm_x = (self.center_lon + 180.0) / 360.0;
        let center_lat_clamped = self.center_lat.clamp(-85.0511, 85.0511);
        let center_lat_rad = center_lat_clamped.to_radians();
        let center_norm_y = (1.0 - (center_lat_rad.tan() + 1.0 / center_lat_rad.cos()).ln() / std::f64::consts::PI) / 2.0;

        let center_wx = center_norm_x * w;
        let center_wy = center_norm_y * w;

        let sc = rect.center();
        let pt_wx = center_wx + (pos.x - sc.x) as f64;
        let pt_wy = center_wy + (pos.y - sc.y) as f64;

        let pt_norm_x = pt_wx / w;
        let pt_norm_y = (pt_wy / w).clamp(0.0001, 0.9999);

        let lon = pt_norm_x * 360.0 - 180.0;
        let lat_rad = (std::f64::consts::PI * (1.0 - 2.0 * pt_norm_y)).exp().atan() * 2.0 - std::f64::consts::FRAC_PI_2;
        let lat = lat_rad.to_degrees().clamp(-85.0511, 85.0511);

        (lat, lon)
    }

    /// Handles mouse drag panning and mouse wheel zoom centered at cursor
    pub fn handle_interaction(&mut self, ui: &egui::Ui, rect: Rect, resp: &egui::Response) {
        // Drag panning
        if resp.drag_started() {
            self.is_dragging = true;
            self.drag_start_mouse = resp.interact_pointer_pos();
            self.drag_start_center = Some((self.center_lat, self.center_lon));
        }

        if self.is_dragging && resp.dragged() {
            let delta = resp.drag_delta();
            let w = self.world_size();
            // dx in normalized longitude
            let d_lon = -(delta.x as f64 / w) * 360.0;

            // dy in normalized latitude
            let cos_lat = self.center_lat.to_radians().cos().max(0.05);
            let d_lat = (delta.y as f64 / w) * (360.0 * cos_lat);

            self.center_lon += d_lon;
            self.center_lat = (self.center_lat + d_lat).clamp(-85.0, 85.0);

            // Wrap longitude [-180, 180]
            if self.center_lon > 180.0 {
                self.center_lon -= 360.0;
            } else if self.center_lon < -180.0 {
                self.center_lon += 360.0;
            }
        }

        if resp.drag_stopped() {
            self.is_dragging = false;
            self.drag_start_mouse = None;
            self.drag_start_center = None;
        }

        // Mouse wheel zoom
        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                let old_zoom = self.zoom;
                let new_zoom = (self.zoom + if scroll > 0.0 { 0.3 } else { -0.3 }).clamp(1.0, 18.0);

                if (new_zoom - old_zoom).abs() > 0.001 {
                    if let Some(mouse_pos) = resp.hover_pos() {
                        // Zoom centered towards mouse pointer
                        let (mouse_lat, mouse_lon) = self.screen_to_lat_lon(mouse_pos, rect);
                        self.zoom = new_zoom;
                        let (new_mouse_lat, new_mouse_lon) = self.screen_to_lat_lon(mouse_pos, rect);
                        self.center_lat += mouse_lat - new_mouse_lat;
                        self.center_lon += mouse_lon - new_mouse_lon;
                    } else {
                        self.zoom = new_zoom;
                    }
                }
            }
        }
    }

    /// Renders visible slippy tiles onto the egui canvas
    pub fn render_tiles(&self, painter: &egui::Painter, rect: Rect, tile_mgr: &mut TileManager) {
        let base_z = (self.zoom.floor() as u32).clamp(0, 18);
        let num_tiles = 1u32 << base_z;

        let w = self.world_size();
        let scale = 2.0_f64.powf(self.zoom - base_z as f64);
        let tile_size_px = 256.0 * scale;

        let center_norm_x = (self.center_lon + 180.0) / 360.0;
        let center_lat_clamped = self.center_lat.clamp(-85.0511, 85.0511);
        let center_lat_rad = center_lat_clamped.to_radians();
        let center_norm_y = (1.0 - (center_lat_rad.tan() + 1.0 / center_lat_rad.cos()).ln() / std::f64::consts::PI) / 2.0;

        let center_wx = center_norm_x * w;
        let center_wy = center_norm_y * w;

        let sc = rect.center();

        // Calculate visible bounding box in tile coordinates
        let min_wx = center_wx - (sc.x - rect.min.x) as f64;
        let max_wx = center_wx + (rect.max.x - sc.x) as f64;
        let min_wy = center_wy - (sc.y - rect.min.y) as f64;
        let max_wy = center_wy + (rect.max.y - sc.y) as f64;

        let min_tx = (min_wx / tile_size_px).floor() as i32;
        let max_tx = (max_wx / tile_size_px).floor() as i32;
        let min_ty = ((min_wy / tile_size_px).floor() as i32).max(0);
        let max_ty = ((max_wy / tile_size_px).floor() as i32).min(num_tiles as i32 - 1);

        let bg_color = match self.style {
            MapStyle::DarkTactical => Color32::from_rgb(18, 22, 30),
            _ => Color32::from_rgb(220, 226, 232),
        };
        painter.rect_filled(rect, 4.0, bg_color);

        for ty in min_ty..=max_ty {
            for tx in min_tx..=max_tx {
                let wrapped_x = tx.rem_euclid(num_tiles as i32) as u32;
                let actual_y = ty as u32;

                let key = TileKey {
                    style: self.style,
                    z: base_z,
                    x: wrapped_x,
                    y: actual_y,
                };

                let tile_wx = (tx as f64) * tile_size_px;
                let tile_wy = (ty as f64) * tile_size_px;

                let screen_x = sc.x + (tile_wx - center_wx) as f32;
                let screen_y = sc.y + (tile_wy - center_wy) as f32;
                let tile_rect = Rect::from_min_size(
                    Pos2::new(screen_x, screen_y),
                    Vec2::new(tile_size_px as f32, tile_size_px as f32),
                );

                // Check intersection with screen
                if !rect.intersects(tile_rect) {
                    continue;
                }

                if let Some(tex) = tile_mgr.get_texture(&key) {
                    painter.image(
                        tex.id(),
                        tile_rect,
                        Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else {
                    // Try parent tile as low-res fallback for instant display
                    let parent_drawn = if base_z > 0 {
                        let parent_key = TileKey {
                            style: self.style,
                            z: base_z - 1,
                            x: wrapped_x / 2,
                            y: actual_y / 2,
                        };
                        if let Some(p_tex) = tile_mgr.get_texture(&parent_key) {
                            let u0 = (wrapped_x % 2) as f32 * 0.5;
                            let v0 = (actual_y % 2) as f32 * 0.5;
                            let uv = Rect::from_min_max(Pos2::new(u0, v0), Pos2::new(u0 + 0.5, v0 + 0.5));
                            painter.image(p_tex.id(), tile_rect, uv, Color32::WHITE);
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if !parent_drawn {
                        // Subtle grid placeholder
                        let line_col = match self.style {
                            MapStyle::DarkTactical => Color32::from_rgb(30, 36, 48),
                            _ => Color32::from_rgb(200, 210, 220),
                        };
                        painter.rect_stroke(tile_rect, 0.0, Stroke::new(0.5_f32, line_col));
                    }

                    // Request the high-resolution tile from disk or server
                    tile_mgr.request_tile(key);
                }
            }
        }
    }

    /// Renders tactical scale bar and navigation buttons overlay on map
    pub fn render_controls_overlay(&mut self, ui: &egui::Ui, rect: Rect, home_qth: (f64, f64)) {
        let painter = ui.painter_at(rect);

        // 1. Compass / North Indicator (Top Right)
        let compass_pos = Pos2::new(rect.max.x - 28.0, rect.min.y + 28.0);
        painter.circle_filled(compass_pos, 14.0, Color32::from_rgba_premultiplied(10, 16, 26, 200));
        painter.circle_stroke(compass_pos, 14.0, Stroke::new(1.0_f32, Color32::from_rgb(0, 220, 255)));
        painter.text(compass_pos, egui::Align2::CENTER_CENTER, "N", egui::FontId::proportional(11.0), Color32::from_rgb(0, 220, 255));

        // 2. Zoom and Home Buttons (Top Left or Top Right below compass)
        let btn_rect_plus = Rect::from_min_size(Pos2::new(rect.max.x - 38.0, rect.min.y + 50.0), Vec2::new(24.0, 22.0));
        let btn_rect_minus = Rect::from_min_size(Pos2::new(rect.max.x - 38.0, rect.min.y + 75.0), Vec2::new(24.0, 22.0));
        let btn_rect_home = Rect::from_min_size(Pos2::new(rect.max.x - 38.0, rect.min.y + 100.0), Vec2::new(24.0, 22.0));

        let draw_btn = |r: Rect, txt: &str| {
            painter.rect_filled(r, 3.0, Color32::from_rgba_premultiplied(14, 22, 34, 220));
            painter.rect_stroke(r, 3.0, Stroke::new(1.0_f32, Color32::from_rgb(40, 65, 95)));
            painter.text(r.center(), egui::Align2::CENTER_CENTER, txt, egui::FontId::proportional(12.0), Color32::WHITE);
        };

        draw_btn(btn_rect_plus, "+");
        draw_btn(btn_rect_minus, "−");
        draw_btn(btn_rect_home, "🏠");

        // Handle button clicks
        if ui.input(|i| i.pointer.primary_clicked()) {
            if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                if btn_rect_plus.contains(pos) {
                    self.zoom = (self.zoom + 1.0).min(18.0);
                } else if btn_rect_minus.contains(pos) {
                    self.zoom = (self.zoom - 1.0).max(1.0);
                } else if btn_rect_home.contains(pos) {
                    self.center_lat = home_qth.0;
                    self.center_lon = home_qth.1;
                    self.zoom = 9.0;
                }
            }
        }

        // 3. Metric Scale Bar (Bottom Left)
        let lat_rad = self.center_lat.to_radians();
        let meters_per_px = (40_075_016.686 * lat_rad.cos()) / self.world_size();
        let scale_w_px = 120.0f32;
        let km_at_scale = (meters_per_px * scale_w_px as f64) / 1000.0;

        let (nice_km, nice_px) = if km_at_scale >= 1000.0 {
            (1000.0, (1000.0 / km_at_scale * scale_w_px as f64) as f32)
        } else if km_at_scale >= 500.0 {
            (500.0, (500.0 / km_at_scale * scale_w_px as f64) as f32)
        } else if km_at_scale >= 200.0 {
            (200.0, (200.0 / km_at_scale * scale_w_px as f64) as f32)
        } else if km_at_scale >= 100.0 {
            (100.0, (100.0 / km_at_scale * scale_w_px as f64) as f32)
        } else if km_at_scale >= 50.0 {
            (50.0, (50.0 / km_at_scale * scale_w_px as f64) as f32)
        } else if km_at_scale >= 20.0 {
            (20.0, (20.0 / km_at_scale * scale_w_px as f64) as f32)
        } else if km_at_scale >= 10.0 {
            (10.0, (10.0 / km_at_scale * scale_w_px as f64) as f32)
        } else if km_at_scale >= 5.0 {
            (5.0, (5.0 / km_at_scale * scale_w_px as f64) as f32)
        } else if km_at_scale >= 2.0 {
            (2.0, (2.0 / km_at_scale * scale_w_px as f64) as f32)
        } else if km_at_scale >= 1.0 {
            (1.0, (1.0 / km_at_scale * scale_w_px as f64) as f32)
        } else {
            (0.5, (0.5 / km_at_scale * scale_w_px as f64) as f32)
        };

        let scale_p0 = Pos2::new(rect.min.x + 12.0, rect.max.y - 14.0);
        let scale_p1 = Pos2::new(scale_p0.x + nice_px, scale_p0.y);
        painter.line_segment([scale_p0, scale_p1], Stroke::new(2.5_f32, Color32::from_rgb(0, 220, 255)));
        painter.line_segment([Pos2::new(scale_p0.x, scale_p0.y - 4.0), scale_p0], Stroke::new(2.5_f32, Color32::from_rgb(0, 220, 255)));
        painter.line_segment([Pos2::new(scale_p1.x, scale_p1.y - 4.0), scale_p1], Stroke::new(2.5_f32, Color32::from_rgb(0, 220, 255)));

        let scale_txt = if nice_km >= 1.0 {
            format!("{:.0} km", nice_km)
        } else {
            format!("{:.0} m", nice_km * 1000.0)
        };
        painter.text(
            Pos2::new(scale_p1.x + 6.0, scale_p0.y - 2.0),
            egui::Align2::LEFT_CENTER,
            scale_txt,
            egui::FontId::monospace(9.5),
            Color32::from_rgb(0, 220, 255),
        );

        // 4. Coordinates at center / cursor
        let coord_txt = format!("{:.4}°N, {:.4}°E (z={:.1})", self.center_lat, self.center_lon, self.zoom);
        painter.text(
            Pos2::new(rect.max.x - 10.0, rect.max.y - 12.0),
            egui::Align2::RIGHT_BOTTOM,
            coord_txt,
            egui::FontId::monospace(9.0),
            Color32::from_rgb(140, 170, 200),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_mercator_projection_roundtrip() {
        let map = SlippyMapState::new(60.1699, 24.9384, 9.0);
        let screen_rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 600.0));

        let test_coords = [
            (60.1699, 24.9384), // Helsinki
            (61.4978, 23.7610), // Tampere
            (60.4518, 22.2666), // Turku
            (65.0121, 25.4651), // Oulu
            (51.5074, -0.1278), // London
            (40.7128, -74.0060),// New York
        ];

        for &(lat, lon) in &test_coords {
            let screen_pos = map.lat_lon_to_screen(lat, lon, screen_rect);
            let (unproj_lat, unproj_lon) = map.screen_to_lat_lon(screen_pos, screen_rect);

            assert!((lat - unproj_lat).abs() < 0.0001, "Latitude round-trip failed: {} vs {}", lat, unproj_lat);
            assert!((lon - unproj_lon).abs() < 0.0001, "Longitude round-trip failed: {} vs {}", lon, unproj_lon);
        }
    }

    #[test]
    fn test_map_styles_url_formatting() {
        let osm_url = MapStyle::OpenStreetMap.url_template(10, 582, 295);
        assert!(osm_url.contains("tile.openstreetmap.org/10/582/295.png"));

        let dark_url = MapStyle::DarkTactical.url_template(10, 582, 295);
        assert!(dark_url.contains("services.arcgisonline.com/arcgis/rest/services/Canvas/World_Dark_Gray_Base/MapServer/tile/10/295/582"));
    }

    #[test]
    fn test_world_size_scaling() {
        let map = SlippyMapState::new(0.0, 0.0, 1.0);
        assert_eq!(map.world_size(), 512.0);

        let map2 = SlippyMapState::new(0.0, 0.0, 10.0);
        assert_eq!(map2.world_size(), 256.0 * 1024.0);
    }

    #[test]
    fn test_esri_fetch() {
        let resp = ureq::get("https://services.arcgisonline.com/arcgis/rest/services/Canvas/World_Dark_Gray_Base/MapServer/tile/10/297/582")
            .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
            .timeout(std::time::Duration::from_secs(4))
            .call();
        match resp {
            Ok(r) => {
                let mut buf = Vec::new();
                r.into_reader().read_to_end(&mut buf).unwrap();
                let img = image::load_from_memory(&buf).expect("Failed to decode image");
                let (w, h) = img.to_rgba8().dimensions();
                println!("ESRI HELSINKI DECODED OK: {}x{} bytes: {}", w, h, buf.len());
                assert_eq!(w, 256);
                assert_eq!(h, 256);
            }
            Err(e) => {
                panic!("ESRI ERR: {:?}", e);
            }
        }
    }
}
