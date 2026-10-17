use num_complex::Complex32;
use std::time::{Instant, SystemTime};

fn get_timestamp() -> String {
    let now = SystemTime::now();
    let dur = now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let hours = (secs / 3600) % 24;
    let mins = (secs / 60) % 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}.{:03}", hours, mins, s, millis)
}

/// Represents a decoded marine AIS (Automatic Identification System) packet
#[derive(Clone, Debug, PartialEq)]
pub struct AisPacket {
    pub mmsi: u32,
    pub msg_type: u8,
    pub timestamp: String,
    pub freq_hz: u64,
    pub channel: String, // "A" (161.975 MHz) or "B" (162.025 MHz)
    pub name: Option<String>,
    pub callsign: Option<String>,
    pub country: String,
    pub vessel_type: Option<String>,
    pub nav_status: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub sog_knots: Option<f32>,
    pub cog_deg: Option<f32>,
    pub heading_deg: Option<f32>,
    pub destination: Option<String>,
    pub draught_m: Option<f32>,
    pub length_m: Option<u16>,
    pub beam_m: Option<u16>,
    pub nmea_sentence: String,
    pub human_summary: String,
    pub signal_snr_db: f32,
}

/// Represents a vessel or marine station tracked in memory
#[derive(Clone, Debug)]
pub struct AisStation {
    pub mmsi: u32,
    pub name: Option<String>,
    pub callsign: Option<String>,
    pub country: String,
    pub vessel_type: Option<String>,
    pub nav_status: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub sog_knots: Option<f32>,
    pub cog_deg: Option<f32>,
    pub heading_deg: Option<f32>,
    pub destination: Option<String>,
    pub draught_m: Option<f32>,
    pub length_m: Option<u16>,
    pub beam_m: Option<u16>,
    pub last_seen: Instant,
    pub last_timestamp: String,
    pub packet_count: u32,
    pub latest_summary: String,
    pub track_history: Vec<(f64, f64)>,
}

/// Validates if an MMSI is structurally conforming to ITU-R M.585 / ITU Radio Regulations
pub fn is_valid_mmsi(mmsi: u32) -> bool {
    // Standard maritime MMSI formatted as 9 digits
    if mmsi < 2_010_000 || mmsi > 999_999_999 {
        return false;
    }
    let s = format!("{:09}", mmsi);
    // Coastal Station: 00MIDxxxx
    if s.starts_with("00") {
        if let Ok(mid) = s[2..5].parse::<u32>() {
            return (201..=775).contains(&mid);
        }
        return false;
    }
    // Group ship station: 0MIDxxxxx
    if s.starts_with('0') {
        if let Ok(mid) = s[1..4].parse::<u32>() {
            return (201..=775).contains(&mid);
        }
        return false;
    }
    // SAR Aircraft: 111MIDxxx
    if s.starts_with("111") {
        if let Ok(mid) = s[3..6].parse::<u32>() {
            return (201..=775).contains(&mid);
        }
        return false;
    }
    // AIS-SART (970xxxxxx), MOB (972xxxxxx), EPIRB-AIS (974xxxxxx)
    if s.starts_with("970") || s.starts_with("972") || s.starts_with("974") {
        return true;
    }
    // Associated craft: 98MIDxxxx
    if s.starts_with("98") {
        if let Ok(mid) = s[2..5].parse::<u32>() {
            return (201..=775).contains(&mid);
        }
        return false;
    }
    // AtoN: 99MIDxxxx
    if s.starts_with("99") {
        if let Ok(mid) = s[2..5].parse::<u32>() {
            return (201..=775).contains(&mid);
        }
        return false;
    }
    // Standard individual ship station: MIDxxxxxx
    if let Ok(mid) = s[0..3].parse::<u32>() {
        return (201..=775).contains(&mid);
    }
    false
}

/// Resolves Maritime Mobile Service Identity (MMSI) to country of registration
pub fn resolve_mmsi_country(mmsi: u32) -> &'static str {
    if !is_valid_mmsi(mmsi) {
        return "Unknown";
    }
    let s = format!("{:09}", mmsi);
    if s.starts_with("00") {
        return "Coast Radio Station";
    }
    if s.starts_with("111") {
        return "SAR Aircraft";
    }
    if s.starts_with("970") {
        return "AIS SART";
    }
    if s.starts_with("972") {
        return "Man Overboard (MOB)";
    }
    if s.starts_with("974") {
        return "EPIRB AIS";
    }
    if s.starts_with("99") {
        return "Aid to Navigation (AtoN)";
    }

    let mid = match s[0..3].parse::<u32>() {
        Ok(v) => v,
        Err(_) => return "International",
    };

    match mid {
        230 => "Finland",
        276 => "Estonia",
        265 | 266 => "Sweden",
        257..=259 => "Norway",
        219 | 220 => "Denmark",
        211 => "Germany",
        261 => "Poland",
        275 => "Latvia",
        277 => "Lithuania",
        273 => "Russia",
        232..=235 => "United Kingdom",
        226..=228 => "France",
        244..=246 => "Netherlands",
        205 => "Belgium",
        247 => "Italy",
        224 | 225 => "Spain",
        263 => "Portugal",
        237 => "Greece",
        271 => "Turkey",
        316 => "Canada",
        338 | 366..=369 => "United States",
        351..=357 => "Panama",
        370..=374 => "Panama",
        636 => "Liberia",
        538 => "Marshall Islands",
        248 | 249 | 256 => "Malta",
        209 | 210 | 212 => "Cyprus",
        412 | 413 | 414 => "China",
        431 | 432 => "Japan",
        440 | 441 => "South Korea",
        563 | 564 | 565 | 566 => "Singapore",
        _ => "International",
    }
}

/// Resolves standard ITU-R M.1371 Navigational Status codes
pub fn resolve_nav_status(status_code: u8) -> &'static str {
    match status_code {
        0 => "Under way using engine",
        1 => "At anchor",
        2 => "Not under command",
        3 => "Restricted maneuverability",
        4 => "Constrained by draught",
        5 => "Moored",
        6 => "Aground",
        7 => "Engaged in fishing",
        8 => "Under way sailing",
        9 => "Hazardous cargo (HSC)",
        10 => "Hazardous cargo (WIG)",
        11 => "Power-driven towing astern",
        12 => "Power-driven pushing ahead",
        14 => "AIS-SART / MOB active",
        15 => "Undefined",
        _ => "Unknown status",
    }
}

/// Resolves standard ITU-R M.1371 Ship Type codes
pub fn resolve_vessel_type(type_code: u8) -> &'static str {
    match type_code {
        30 => "Fishing Vessel",
        31 | 32 => "Towing Vessel",
        33 => "Dredger / Underwater Ops",
        34 => "Diving Operations",
        35 => "Military Vessel",
        36 => "Sailing Vessel",
        37 => "Pleasure Craft / Yacht",
        40..=49 => "High Speed Craft (HSC)",
        50 => "Pilot Vessel",
        51 => "Search and Rescue (SAR)",
        52 => "Tugboat",
        53 => "Port Tender",
        54 => "Anti-Pollution Vessel",
        55 => "Law Enforcement / Coast Guard",
        58 => "Medical Transport",
        60..=69 => "Passenger Ship / Ferry",
        70..=79 => "Cargo Ship",
        80..=89 => "Tanker",
        90..=99 => "Other / Specialized",
        _ => "Vessel",
    }
}

/// Extracts an unsigned bit field from a slice of bits
pub fn get_bits(bits: &[u8], start: usize, len: usize) -> u64 {
    let mut val = 0u64;
    for i in 0..len {
        if start + i < bits.len() {
            val = (val << 1) | (bits[start + i] as u64);
        }
    }
    val
}

/// Extracts a two's-complement signed bit field
pub fn get_signed_bits(bits: &[u8], start: usize, len: usize) -> i64 {
    let raw = get_bits(bits, start, len);
    if len == 0 || len > 64 {
        return 0;
    }
    let sign_bit = 1u64 << (len - 1);
    if (raw & sign_bit) != 0 {
        (raw as i64) - ((1u64 << len) as i64)
    } else {
        raw as i64
    }
}

/// Decodes 6-bit ASCII characters used in AIS Message 5, 19, and 24
pub fn get_ascii_string(bits: &[u8], start: usize, num_chars: usize) -> String {
    let mut s = String::with_capacity(num_chars);
    for i in 0..num_chars {
        let code = get_bits(bits, start + i * 6, 6) as u8;
        let ch = if code < 32 {
            (code + 64) as char
        } else {
            code as char
        };
        s.push(ch);
    }
    s.trim().trim_matches('@').trim().to_string()
}

/// Encodes raw payload bits into standard 6-bit ASCII armoring for NMEA !AIVDM
pub fn encode_sixbit_nmea(payload_bits: &[u8]) -> (String, u8) {
    let mut s = String::new();
    let num_bits = payload_bits.len();
    let num_chars = (num_bits + 5) / 6;
    let fill_bits = if num_bits % 6 == 0 { 0 } else { (6 - (num_bits % 6)) as u8 };

    for i in 0..num_chars {
        let mut val = 0u8;
        for b in 0..6 {
            let bit_idx = i * 6 + b;
            let bit = if bit_idx < num_bits { payload_bits[bit_idx] } else { 0 };
            val = (val << 1) | bit;
        }
        let ch = if val < 40 { (val + 48) as char } else { (val + 56) as char };
        s.push(ch);
    }
    (s, fill_bits)
}

/// Calculates standard NMEA 0183 XOR checksum string ("*hh")
pub fn calculate_nmea_checksum(sentence: &str) -> String {
    let mut sum: u8 = 0;
    let bytes = sentence.as_bytes();
    let mut in_payload = false;
    for &b in bytes {
        if b == b'!' || b == b'$' {
            in_payload = true;
            continue;
        }
        if b == b'*' {
            break;
        }
        if in_payload {
            sum ^= b;
        }
    }
    format!("{:02X}", sum)
}

/// Real-time 9600 Baud GMSK / HDLC Marine AIS Packet Decoder
pub struct AisDecoder {
    #[allow(dead_code)]
    sample_rate: f32,
    last_iq: Complex32,
    // Lowpass filter state
    lp_state_1: f32,
    lp_alpha: f32,
    // DC baseline tracker
    dc_baseline: f32,
    // Standard GNUAIS 32-bit Phase-Locked Loop (DPLL)
    pll: u32,
    pll_inc: u32,
    last_disc_sign: bool,
    last_raw_bit: bool,
    // HDLC State Machine
    shift_reg: u8,
    history_reg: u32,
    ones_count: usize,
    in_frame: bool,
    frame_bits: Vec<u8>,
    // Decoded packets buffer
    pub packets: Vec<AisPacket>,
    pub latest_packet: Option<AisPacket>,
}

impl AisDecoder {
    pub fn new(sample_rate: f32) -> Self {
        let dt = 1.0 / sample_rate;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * 7200.0);
        let lp_alpha = (dt / (rc + dt)).clamp(0.01, 1.0);
        let pll_inc = ((9600.0 / sample_rate) * 4294967296.0) as u32;

        Self {
            sample_rate,
            last_iq: Complex32::new(1.0, 0.0),
            lp_state_1: 0.0,
            lp_alpha,
            dc_baseline: 0.0,
            pll: 0,
            pll_inc,
            last_disc_sign: false,
            last_raw_bit: false,
            shift_reg: 0,
            history_reg: 0,
            ones_count: 0,
            in_frame: false,
            frame_bits: Vec::with_capacity(1024),
            packets: Vec::with_capacity(32),
            latest_packet: None,
        }
    }

    pub fn reset(&mut self) {
        self.last_iq = Complex32::new(1.0, 0.0);
        self.lp_state_1 = 0.0;
        self.dc_baseline = 0.0;
        self.pll = 0;
        self.last_disc_sign = false;
        self.last_raw_bit = false;
        self.shift_reg = 0;
        self.history_reg = 0;
        self.ones_count = 0;
        self.in_frame = false;
        self.frame_bits.clear();
    }

    /// Process downconverted complex IQ samples (typically 48,000 Hz)
    pub fn process_samples(&mut self, iq: &[Complex32], freq_hz: u64, snr_db: f32) {
        for &s in iq {
            let diff = s * self.last_iq.conj();
            let raw_fm = diff.im.atan2(diff.re);
            self.last_iq = s;
            self.process_sample(raw_fm, freq_hz, snr_db);
        }
    }

    /// Process FM discriminator audio samples (48,000 Hz)
    #[allow(dead_code)]
    pub fn process_audio(&mut self, audio: &[f32], freq_hz: u64, snr_db: f32) {
        for &sample in audio {
            self.process_sample(sample, freq_hz, snr_db);
        }
    }

    /// Feeds a single demodulated discriminator sample through GNUAIS DPLL and HDLC framer
    #[inline]
    fn process_sample(&mut self, sample: f32, freq_hz: u64, snr_db: f32) {
        // Lowpass filter for discriminator noise reduction
        let a = self.lp_alpha;
        let inv_a = 1.0 - a;
        self.lp_state_1 = self.lp_state_1 * inv_a + sample * a;
        let filtered = self.lp_state_1;

        // Leaky DC baseline tracker to remove carrier frequency offsets (+/- 1-2 kHz)
        self.dc_baseline = self.dc_baseline * 0.996 + filtered * 0.004;
        let disc = filtered - self.dc_baseline;

        // GNUAIS Phase-Locked Loop (DPLL) Symbol Timing Recovery:
        // When discriminator crosses zero, evaluate phase error
        let curr_sign = disc > 0.0;
        if curr_sign != self.last_disc_sign {
            // Transitions are aligned to phase 0.5 (halfway in accumulator, 0x8000_0000)
            if self.pll < (1u32 << 31) {
                self.pll = self.pll.saturating_add(self.pll_inc / 8);
            } else {
                self.pll = self.pll.saturating_sub(self.pll_inc / 8);
            }
            self.last_disc_sign = curr_sign;
        }

        // Advance PLL phase
        let (new_pll, overflow) = self.pll.overflowing_add(self.pll_inc);
        self.pll = new_pll;

        // Slicer triggers when accumulator overflows (phase 0.0 / 1.0, exactly 180° away from transitions)
        if overflow {
            let bit = disc > 0.0;
            // NRZI decoding: transition = 0, no transition = 1
            let data_bit = if bit == self.last_raw_bit { 1u8 } else { 0u8 };
            self.last_raw_bit = bit;

            self.process_bit(data_bit, freq_hz, snr_db);
        }
    }

    /// Processes a single NRZI data bit through HDLC framer and bit destuffing
    fn process_bit(&mut self, bit: u8, freq_hz: u64, snr_db: f32) {
        self.shift_reg = (self.shift_reg >> 1) | (bit << 7);
        self.history_reg = (self.history_reg << 1) | (bit as u32);

        // Check for HDLC flag (0x7E = 01111110)
        if self.shift_reg == 0x7E {
            if self.in_frame {
                // Minimum AIS message is 168 bits + 16 bits FCS = 184 bits
                // Discard trailing 7 bits that belong to the flag
                if self.frame_bits.len() >= 184 + 7 {
                    let total_len = self.frame_bits.len() - 7;
                    let candidate_bits = &self.frame_bits[..total_len];
                    if let Some(pkt) = Self::parse_ais_frame(candidate_bits, freq_hz, snr_db) {
                        self.latest_packet = Some(pkt.clone());
                        self.packets.push(pkt);
                        if self.packets.len() > 100 {
                            self.packets.remove(0);
                        }
                    }
                }
            }
            // Flag received: start accumulating payload bits for the frame
            self.in_frame = true;
            self.frame_bits.clear();
            self.ones_count = 0;
            return;
        }

        if !self.in_frame {
            return;
        }

        // Bit destuffing: a '0' after five consecutive '1's was inserted by transmitter
        if self.ones_count == 5 {
            if bit == 0 {
                // Destuffed bit, discard
                self.ones_count = 0;
                return;
            } else {
                // 6 ones in frame data: either part of closing flag or abort
                self.ones_count = 6;
                self.frame_bits.push(bit);
                return;
            }
        }

        if bit == 1 {
            self.ones_count += 1;
            if self.ones_count >= 7 {
                // Abort sequence (>= 7 consecutive ones): reset receiver frame
                self.in_frame = false;
                self.frame_bits.clear();
                self.ones_count = 0;
                return;
            }
        } else {
            self.ones_count = 0;
        }

        self.frame_bits.push(bit);

        // Buffer overrun safeguard (max AIS multi-slot packet is ~1024 bits)
        if self.frame_bits.len() > 1100 {
            self.in_frame = false;
            self.frame_bits.clear();
            self.ones_count = 0;
        }
    }

    /// Verifies HDLC 16-bit CRC-CCITT (polynomial 0x1021 / reversed 0x8408)
    pub fn check_hdlc_crc(frame_bytes: &[u8]) -> bool {
        if frame_bytes.len() < 2 {
            return false;
        }
        let mut crc = 0xFFFFu16;
        for &byte in frame_bytes {
            for i in 0..8 {
                let bit = ((byte >> i) & 1) as u16;
                let c_bit = crc & 1;
                crc >>= 1;
                if (c_bit ^ bit) != 0 {
                    crc ^= 0x8408;
                }
            }
        }
        // Standard HDLC FCS residual check
        crc == 0xF0B8
    }

    /// Converts bit stream to byte vector (LSB first) and validates CRC
    pub fn parse_ais_frame(bits: &[u8], freq_hz: u64, snr_db: f32) -> Option<AisPacket> {
        let num_bytes = bits.len() / 8;
        if num_bytes < 22 {
            return None;
        }

        let mut bytes = Vec::with_capacity(num_bytes);
        for i in 0..num_bytes {
            let mut byte = 0u8;
            for b in 0..8 {
                byte |= bits[i * 8 + b] << b;
            }
            bytes.push(byte);
        }

        if !Self::check_hdlc_crc(&bytes) {
            return None;
        }

        // Payload bits exclude the last 16 FCS bits
        let payload_len = bits.len().saturating_sub(16);
        let payload_bits = &bits[..payload_len];
        if payload_bits.len() < 38 {
            return None;
        }

        let msg_type = get_bits(payload_bits, 0, 6) as u8;
        if msg_type == 0 || msg_type > 27 {
            return None;
        }

        let mmsi = get_bits(payload_bits, 8, 30) as u32;
        if !is_valid_mmsi(mmsi) {
            return None;
        }

        let country = resolve_mmsi_country(mmsi).to_string();
        let channel = if freq_hz >= 162_000_000 { "B" } else { "A" };

        let mut packet = AisPacket {
            mmsi,
            msg_type,
            timestamp: get_timestamp(),
            freq_hz,
            channel: channel.to_string(),
            name: None,
            callsign: None,
            country,
            vessel_type: None,
            nav_status: None,
            latitude: None,
            longitude: None,
            sog_knots: None,
            cog_deg: None,
            heading_deg: None,
            destination: None,
            draught_m: None,
            length_m: None,
            beam_m: None,
            nmea_sentence: String::new(),
            human_summary: String::new(),
            signal_snr_db: snr_db,
        };

        match msg_type {
            1 | 2 | 3 => {
                // Class A Position Report
                if payload_bits.len() >= 168 {
                    let status = get_bits(payload_bits, 38, 4) as u8;
                    packet.nav_status = Some(resolve_nav_status(status).to_string());

                    let sog_raw = get_bits(payload_bits, 50, 10) as u16;
                    if sog_raw < 1022 {
                        let sog = (sog_raw as f32) * 0.1;
                        if sog <= 102.2 {
                            packet.sog_knots = Some(sog);
                        }
                    }

                    let lon_raw = get_signed_bits(payload_bits, 61, 28);
                    if lon_raw != 0x6791AC0 && lon_raw.abs() <= 108_000_000 {
                        let lon = (lon_raw as f64) / 600_000.0;
                        if (-180.0..=180.0).contains(&lon) {
                            packet.longitude = Some(lon);
                        }
                    }

                    let lat_raw = get_signed_bits(payload_bits, 89, 27);
                    if lat_raw != 0x3412140 && lat_raw.abs() <= 54_000_000 {
                        let lat = (lat_raw as f64) / 600_000.0;
                        if (-90.0..=90.0).contains(&lat) {
                            packet.latitude = Some(lat);
                        }
                    }

                    let cog_raw = get_bits(payload_bits, 116, 12) as u16;
                    if cog_raw < 3600 {
                        packet.cog_deg = Some((cog_raw as f32) * 0.1);
                    }

                    let heading_raw = get_bits(payload_bits, 128, 9) as u16;
                    if heading_raw < 360 {
                        packet.heading_deg = Some(heading_raw as f32);
                    }
                }
            }
            4 => {
                // Base Station Report
                if payload_bits.len() >= 168 {
                    packet.vessel_type = Some("Base Station".to_string());
                    let lon_raw = get_signed_bits(payload_bits, 79, 28);
                    if lon_raw != 0x6791AC0 && lon_raw.abs() <= 108_000_000 {
                        packet.longitude = Some((lon_raw as f64) / 600_000.0);
                    }

                    let lat_raw = get_signed_bits(payload_bits, 107, 27);
                    if lat_raw != 0x3412140 && lat_raw.abs() <= 54_000_000 {
                        packet.latitude = Some((lat_raw as f64) / 600_000.0);
                    }
                }
            }
            5 => {
                // Static and Voyage Related Data
                if payload_bits.len() >= 420 {
                    let callsign = get_ascii_string(payload_bits, 70, 7);
                    if !callsign.is_empty() {
                        packet.callsign = Some(callsign);
                    }

                    let name = get_ascii_string(payload_bits, 112, 20);
                    if !name.is_empty() {
                        packet.name = Some(name);
                    }

                    let type_code = get_bits(payload_bits, 232, 8) as u8;
                    packet.vessel_type = Some(resolve_vessel_type(type_code).to_string());

                    let dim_a = get_bits(payload_bits, 240, 9) as u16;
                    let dim_b = get_bits(payload_bits, 249, 9) as u16;
                    let dim_c = get_bits(payload_bits, 258, 6) as u16;
                    let dim_d = get_bits(payload_bits, 264, 6) as u16;
                    if (dim_a + dim_b) > 0 {
                        packet.length_m = Some(dim_a + dim_b);
                        packet.beam_m = Some(dim_c + dim_d);
                    }

                    let draught_raw = get_bits(payload_bits, 294, 8) as u8;
                    if draught_raw > 0 {
                        packet.draught_m = Some((draught_raw as f32) * 0.1);
                    }

                    let destination = get_ascii_string(payload_bits, 302, 20);
                    if !destination.is_empty() {
                        packet.destination = Some(destination);
                    }
                }
            }
            9 => {
                // Standard SAR Aircraft Position Report
                if payload_bits.len() >= 148 {
                    packet.vessel_type = Some("SAR Aircraft".to_string());
                    let sog_raw = get_bits(payload_bits, 50, 10) as u16;
                    if sog_raw < 1023 {
                        packet.sog_knots = Some((sog_raw as f32) * 0.1);
                    }

                    let lon_raw = get_signed_bits(payload_bits, 61, 28);
                    if lon_raw != 0x6791AC0 && lon_raw.abs() <= 108_000_000 {
                        packet.longitude = Some((lon_raw as f64) / 600_000.0);
                    }

                    let lat_raw = get_signed_bits(payload_bits, 89, 27);
                    if lat_raw != 0x3412140 && lat_raw.abs() <= 54_000_000 {
                        packet.latitude = Some((lat_raw as f64) / 600_000.0);
                    }

                    let cog_raw = get_bits(payload_bits, 116, 12) as u16;
                    if cog_raw < 3600 {
                        packet.cog_deg = Some((cog_raw as f32) * 0.1);
                    }
                }
            }
            18 => {
                // Standard Class B Position Report
                if payload_bits.len() >= 168 {
                    packet.vessel_type = Some("Class B Vessel".to_string());
                    let sog_raw = get_bits(payload_bits, 46, 10) as u16;
                    if sog_raw < 1023 {
                        packet.sog_knots = Some((sog_raw as f32) * 0.1);
                    }

                    let lon_raw = get_signed_bits(payload_bits, 57, 28);
                    if lon_raw != 0x6791AC0 && lon_raw.abs() <= 108_000_000 {
                        packet.longitude = Some((lon_raw as f64) / 600_000.0);
                    }

                    let lat_raw = get_signed_bits(payload_bits, 85, 27);
                    if lat_raw != 0x3412140 && lat_raw.abs() <= 54_000_000 {
                        packet.latitude = Some((lat_raw as f64) / 600_000.0);
                    }

                    let cog_raw = get_bits(payload_bits, 112, 12) as u16;
                    if cog_raw < 3600 {
                        packet.cog_deg = Some((cog_raw as f32) * 0.1);
                    }

                    let heading_raw = get_bits(payload_bits, 124, 9) as u16;
                    if heading_raw < 360 {
                        packet.heading_deg = Some(heading_raw as f32);
                    }
                }
            }
            19 => {
                // Extended Class B Position Report
                if payload_bits.len() >= 312 {
                    let sog_raw = get_bits(payload_bits, 46, 10) as u16;
                    if sog_raw < 1023 {
                        packet.sog_knots = Some((sog_raw as f32) * 0.1);
                    }

                    let lon_raw = get_signed_bits(payload_bits, 57, 28);
                    if lon_raw != 0x6791AC0 && lon_raw.abs() <= 108_000_000 {
                        packet.longitude = Some((lon_raw as f64) / 600_000.0);
                    }

                    let lat_raw = get_signed_bits(payload_bits, 85, 27);
                    if lat_raw != 0x3412140 && lat_raw.abs() <= 54_000_000 {
                        packet.latitude = Some((lat_raw as f64) / 600_000.0);
                    }

                    let cog_raw = get_bits(payload_bits, 112, 12) as u16;
                    if cog_raw < 3600 {
                        packet.cog_deg = Some((cog_raw as f32) * 0.1);
                    }

                    let name = get_ascii_string(payload_bits, 143, 20);
                    if !name.is_empty() {
                        packet.name = Some(name);
                    }

                    let type_code = get_bits(payload_bits, 263, 8) as u8;
                    packet.vessel_type = Some(resolve_vessel_type(type_code).to_string());
                }
            }
            21 => {
                // Aid to Navigation (AtoN) Report
                if payload_bits.len() >= 272 {
                    packet.vessel_type = Some("Aid to Navigation".to_string());
                    let name = get_ascii_string(payload_bits, 43, 20);
                    if !name.is_empty() {
                        packet.name = Some(name);
                    }

                    let lon_raw = get_signed_bits(payload_bits, 164, 28);
                    if lon_raw != 0x6791AC0 && lon_raw.abs() <= 108_000_000 {
                        packet.longitude = Some((lon_raw as f64) / 600_000.0);
                    }

                    let lat_raw = get_signed_bits(payload_bits, 192, 27);
                    if lat_raw != 0x3412140 && lat_raw.abs() <= 54_000_000 {
                        packet.latitude = Some((lat_raw as f64) / 600_000.0);
                    }
                }
            }
            24 => {
                // Class B Static Data
                if payload_bits.len() >= 160 {
                    let part_num = get_bits(payload_bits, 38, 2) as u8;
                    if part_num == 0 {
                        let name = get_ascii_string(payload_bits, 40, 20);
                        if !name.is_empty() {
                            packet.name = Some(name);
                        }
                    } else if part_num == 1 && payload_bits.len() >= 168 {
                        let type_code = get_bits(payload_bits, 40, 8) as u8;
                        packet.vessel_type = Some(resolve_vessel_type(type_code).to_string());
                        let callsign = get_ascii_string(payload_bits, 90, 7);
                        if !callsign.is_empty() {
                            packet.callsign = Some(callsign);
                        }
                    }
                }
            }
            _ => {}
        }

        // Generate NMEA !AIVDM sentence
        let (sixbit, fill) = encode_sixbit_nmea(payload_bits);
        let raw_sentence = format!("!AIVDM,1,1,,{},{},{}", channel, sixbit, fill);
        let csum = calculate_nmea_checksum(&raw_sentence);
        packet.nmea_sentence = format!("{}*{}", raw_sentence, csum);

        // Generate Human-readable summary
        let mut parts = Vec::new();
        parts.push(format!("MMSI: {}", mmsi));
        if let Some(name) = &packet.name {
            parts.push(format!("\"{}\"", name));
        }
        if let Some(vt) = &packet.vessel_type {
            parts.push(vt.clone());
        }
        if let (Some(la), Some(lo)) = (packet.latitude, packet.longitude) {
            parts.push(format!("{:.4}°N, {:.4}°E", la, lo));
        }
        if let Some(sog) = packet.sog_knots {
            parts.push(format!("{:.1} kts", sog));
        }
        if let Some(cog) = packet.cog_deg {
            parts.push(format!("{:.0}°", cog));
        }
        if let Some(dest) = &packet.destination {
            parts.push(format!("-> {}", dest));
        }
        packet.human_summary = parts.join(" | ");

        Some(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mmsi_country_and_vessel_type_resolution() {
        assert_eq!(resolve_mmsi_country(230999000), "Finland");
        assert_eq!(resolve_mmsi_country(276123456), "Estonia");
        assert_eq!(resolve_mmsi_country(265000000), "Sweden");
        assert_eq!(resolve_mmsi_country(002300000), "Coast Radio Station");
        assert_eq!(resolve_mmsi_country(111230001), "SAR Aircraft");

        assert_eq!(resolve_vessel_type(70), "Cargo Ship");
        assert_eq!(resolve_vessel_type(80), "Tanker");
        assert_eq!(resolve_vessel_type(60), "Passenger Ship / Ferry");
        assert_eq!(resolve_vessel_type(52), "Tugboat");
        assert_eq!(resolve_vessel_type(51), "Search and Rescue (SAR)");
    }

    #[test]
    fn test_nmea_checksum_and_sixbit_encoding() {
        let sentence = "!AIVDM,1,1,,A,13aEO:0P0002CA4F<4?a0?v00000,0";
        let csum = calculate_nmea_checksum(sentence);
        assert_eq!(csum.len(), 2);

        // Verification of CRC calculation
        let mut frame_bytes = vec![0x10, 0x21, 0x55, 0xAA];
        let mut crc = 0xFFFFu16;
        for &b in &frame_bytes {
            for i in 0..8 {
                let bit = ((b >> i) & 1) as u16;
                let c_bit = crc & 1;
                crc >>= 1;
                if (c_bit ^ bit) != 0 {
                    crc ^= 0x8408;
                }
            }
        }
        crc ^= 0xFFFF;
        frame_bytes.push((crc & 0xFF) as u8);
        frame_bytes.push(((crc >> 8) & 0xFF) as u8);
        assert!(AisDecoder::check_hdlc_crc(&frame_bytes));
    }

    #[test]
    fn test_ais_class_a_position_report_decode() {
        // Construct a synthetic 168-bit Type 1 position report:
        // Type 1, MMSI 230123456 (Finland), SOG 12.5 kts, Lat 60.1500° N, Lon 24.9500° E, COG 180.0°
        let mut bits = vec![0u8; 168];

        // Type 1 (6 bits): 000001
        bits[5] = 1;

        // MMSI 230123456 (30 bits, bits 8..38)
        let mmsi = 230123456u32;
        for i in 0..30 {
            bits[8 + i] = ((mmsi >> (29 - i)) & 1) as u8;
        }

        // Nav Status: 0 (Under way using engine, bits 38..42) -> all 0s

        // SOG: 12.5 kts = 125 (10 bits, bits 50..60)
        let sog = 125u16;
        for i in 0..10 {
            bits[50 + i] = ((sog >> (9 - i)) & 1) as u8;
        }

        // Longitude: 24.9500° * 600,000 = 14,970,000 (28 bits signed, bits 61..89)
        let lon = 14_970_000i64;
        for i in 0..28 {
            bits[61 + i] = ((lon >> (27 - i)) & 1) as u8;
        }

        // Latitude: 60.1500° * 600,000 = 36,090,000 (27 bits signed, bits 89..116)
        let lat = 36_090_000i64;
        for i in 0..27 {
            bits[89 + i] = ((lat >> (26 - i)) & 1) as u8;
        }

        // COG: 180.0° = 1800 (12 bits, bits 116..128)
        let cog = 1800u16;
        for i in 0..12 {
            bits[116 + i] = ((cog >> (11 - i)) & 1) as u8;
        }

        // Append 16-bit FCS
        let num_bytes = bits.len() / 8;
        let mut bytes = Vec::with_capacity(num_bytes);
        for i in 0..num_bytes {
            let mut byte = 0u8;
            for b in 0..8 {
                byte |= bits[i * 8 + b] << b;
            }
            bytes.push(byte);
        }

        let mut crc = 0xFFFFu16;
        for &b in &bytes {
            for i in 0..8 {
                let bit = ((b >> i) & 1) as u16;
                let c_bit = crc & 1;
                crc >>= 1;
                if (c_bit ^ bit) != 0 {
                    crc ^= 0x8408;
                }
            }
        }
        crc ^= 0xFFFF;
        let fcs_low = (crc & 0xFF) as u8;
        let fcs_high = ((crc >> 8) & 0xFF) as u8;

        for b in 0..8 {
            bits.push((fcs_low >> b) & 1);
        }
        for b in 0..8 {
            bits.push((fcs_high >> b) & 1);
        }

        let pkt = AisDecoder::parse_ais_frame(&bits, 161_975_000, 15.0).expect("Must decode valid AIS packet");
        assert_eq!(pkt.mmsi, 230123456);
        assert_eq!(pkt.country, "Finland");
        assert_eq!(pkt.msg_type, 1);
        assert_eq!(pkt.channel, "A");
        assert!((pkt.latitude.unwrap() - 60.1500).abs() < 0.001);
        assert!((pkt.longitude.unwrap() - 24.9500).abs() < 0.001);
        assert!((pkt.sog_knots.unwrap() - 12.5).abs() < 0.1);
        assert!((pkt.cog_deg.unwrap() - 180.0).abs() < 0.1);
        assert!(pkt.nmea_sentence.starts_with("!AIVDM,1,1,,A,"));
    }

    #[test]
    fn test_valid_and_invalid_mmsi_structures() {
        // Valid Finnish ship station
        assert!(is_valid_mmsi(230123456));
        assert_eq!(resolve_mmsi_country(230123456), "Finland");

        // Valid Swedish ship station
        assert!(is_valid_mmsi(265987654));
        assert_eq!(resolve_mmsi_country(265987654), "Sweden");

        // Valid Finnish Coast Radio station (002300200)
        assert!(is_valid_mmsi(2300200));
        assert_eq!(resolve_mmsi_country(2300200), "Coast Radio Station");

        // Valid Finnish Aid to Navigation (992301234)
        assert!(is_valid_mmsi(992301234));
        assert_eq!(resolve_mmsi_country(992301234), "Aid to Navigation (AtoN)");

        // Valid AIS SART
        assert!(is_valid_mmsi(970123456));
        assert_eq!(resolve_mmsi_country(970123456), "AIS SART");

        // Invalid noise numbers:
        // 815493 (the Arctic phantom vessel reported by user)
        assert!(!is_valid_mmsi(815493));
        assert_eq!(resolve_mmsi_country(815493), "Unknown");

        assert!(!is_valid_mmsi(0));
        assert!(!is_valid_mmsi(123));
        assert!(!is_valid_mmsi(100000000)); // MID 100 is unallocated
        assert!(!is_valid_mmsi(800000000)); // MID 800 is unallocated
    }

    #[test]
    fn test_end_to_end_ais_demodulation() {
        let mut decoder = AisDecoder::new(48_000.0);

        // 1. Construct 168-bit Type 1 position report (MMSI 230123456, Finland, 60.15N, 24.95E)
        let mut data_bits = vec![0u8; 168];
        data_bits[5] = 1; // Type 1

        let mmsi = 230123456u32;
        for i in 0..30 {
            data_bits[8 + i] = ((mmsi >> (29 - i)) & 1) as u8;
        }

        let sog = 125u16; // 12.5 kts
        for i in 0..10 {
            data_bits[50 + i] = ((sog >> (9 - i)) & 1) as u8;
        }

        let lon = 14_970_000i64; // 24.9500 E
        for i in 0..28 {
            data_bits[61 + i] = ((lon >> (27 - i)) & 1) as u8;
        }

        let lat = 36_090_000i64; // 60.1500 N
        for i in 0..27 {
            data_bits[89 + i] = ((lat >> (26 - i)) & 1) as u8;
        }

        let cog = 1800u16; // 180.0 deg
        for i in 0..12 {
            data_bits[116 + i] = ((cog >> (11 - i)) & 1) as u8;
        }

        // 2. Compute 16-bit HDLC FCS CRC
        let num_bytes = data_bits.len() / 8;
        let mut bytes = Vec::with_capacity(num_bytes);
        for i in 0..num_bytes {
            let mut byte = 0u8;
            for b in 0..8 {
                byte |= data_bits[i * 8 + b] << b;
            }
            bytes.push(byte);
        }

        let mut crc = 0xFFFFu16;
        for &b in &bytes {
            for i in 0..8 {
                let bit = ((b >> i) & 1) as u16;
                let c_bit = crc & 1;
                crc >>= 1;
                if (c_bit ^ bit) != 0 {
                    crc ^= 0x8408;
                }
            }
        }
        crc ^= 0xFFFF;
        let fcs_low = (crc & 0xFF) as u8;
        let fcs_high = ((crc >> 8) & 0xFF) as u8;

        for b in 0..8 {
            data_bits.push((fcs_low >> b) & 1);
        }
        for b in 0..8 {
            data_bits.push((fcs_high >> b) & 1);
        }

        // 3. Bit-stuffing (insert 0 after five consecutive 1s)
        let mut stuffed_bits = Vec::new();
        let mut ones = 0;
        for &b in &data_bits {
            stuffed_bits.push(b);
            if b == 1 {
                ones += 1;
                if ones == 5 {
                    stuffed_bits.push(0);
                    ones = 0;
                }
            } else {
                ones = 0;
            }
        }

        // 4. Assemble complete HDLC packet:
        // 24-bit training sequence (alternating 0101) + start flag (0x7E) + stuffed data + end flag (0x7E) + postamble
        let mut frame_bits = Vec::new();
        for i in 0..24 {
            frame_bits.push((i % 2) as u8);
        }
        for b in 0..8 {
            frame_bits.push((0x7Eu8 >> b) & 1);
        }
        frame_bits.extend(stuffed_bits);
        for b in 0..8 {
            frame_bits.push((0x7Eu8 >> b) & 1);
        }
        for _ in 0..8 {
            frame_bits.push(0);
        }

        // 5. NRZI encoding (0 = toggle state, 1 = keep state)
        let mut nrzi_symbols = Vec::new();
        let mut state = 0u8;
        for &b in &frame_bits {
            if b == 0 {
                state ^= 1;
            }
            nrzi_symbols.push(state);
        }

        // 6. Synthesize 48 kHz FM / Baseband IQ samples
        // 9600 baud -> 5 samples per symbol at 48,000 Hz
        let fs = 48_000.0f32;
        let mut iq = Vec::new();

        // Lead-in silence (40 samples)
        for _ in 0..40 {
            iq.push(Complex32::new(0.001, 0.001));
        }

        let mut phase = 0.0f32;
        let carrier_offset = -600.0f32; // -600 Hz carrier offset
        for &sym in &nrzi_symbols {
            let freq_dev = if sym == 0 { -4800.0f32 } else { 4800.0f32 } + carrier_offset;
            let phase_inc = 2.0 * std::f32::consts::PI * freq_dev / fs;
            for _ in 0..5 {
                phase += phase_inc;
                iq.push(Complex32::new(phase.cos(), phase.sin()));
            }
        }

        // Post-burst silence
        for _ in 0..40 {
            iq.push(Complex32::new(0.001, 0.001));
        }

        decoder.process_samples(&iq, 161_975_000, 20.0);

        assert_eq!(decoder.packets.len(), 1, "Expected exactly 1 decoded AIS packet");
        let pkt = &decoder.packets[0];
        assert_eq!(pkt.mmsi, 230123456);
        assert_eq!(pkt.country, "Finland");
        assert_eq!(pkt.msg_type, 1);
        assert!((pkt.latitude.unwrap() - 60.1500).abs() < 0.001);
        assert!((pkt.longitude.unwrap() - 24.9500).abs() < 0.001);
    }
}
