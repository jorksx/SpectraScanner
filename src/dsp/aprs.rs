use num_complex::Complex32;
use std::f32::consts::PI;
use std::time::SystemTime;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AprsPacket {
    #[serde(default)]
    pub id: u64,
    pub source: String,
    pub destination: String,
    pub digipeaters: Vec<String>,
    pub payload: String,
    pub timestamp: String,
    pub comment: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// 2nd-order Biquad Bandpass Filter (Audio EQ Cookbook)
#[derive(Clone, Debug)]
pub struct BiquadFilter {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadFilter {
    pub fn bandpass(f0: f32, q: f32, fs: f32) -> Self {
        let w0 = 2.0 * PI * f0 / fs;
        let alpha = w0.sin() / (2.0 * q);
        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * w0.cos();
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// High-performance 1200 Baud Bell 202 AFSK / AX.25 HDLC APRS Packet Decoder
pub struct AprsDecoder {
    #[allow(dead_code)]
    sample_rate: f32,
    // Matched Bandpass Filter Pair (Mark 1200 Hz, Space 2200 Hz)
    mark_filter: BiquadFilter,
    space_filter: BiquadFilter,
    // Envelope detectors and smoothed discriminator
    mark_env: f32,
    space_env: f32,
    disc_smooth: f32,
    last_disc: f32,
    // DPLL Clock Recovery
    clock_phase: f32,
    phase_step: f32,
    last_raw_bit: bool,
    // HDLC Bit Stream and Framing
    ones: usize,
    octet: u8,
    bit_count: usize,
    frame_bytes: Vec<u8>,
    // Complex IQ state for direct FM discriminator
    last_iq: Complex32,
    // Decoded packets buffer (keeps recent 30 packets)
    pub packets: Vec<AprsPacket>,
    pub latest_packet: Option<AprsPacket>,
    pub next_id: u64,
}

impl AprsDecoder {
    pub fn new(sample_rate: f32) -> Self {
        let mark_filter = BiquadFilter::bandpass(1200.0, 1.6, sample_rate);
        let space_filter = BiquadFilter::bandpass(2200.0, 1.6, sample_rate);

        Self {
            sample_rate,
            mark_filter,
            space_filter,
            mark_env: 0.0,
            space_env: 0.0,
            disc_smooth: 0.0,
            last_disc: 0.0,
            clock_phase: 0.0,
            phase_step: 1200.0 / sample_rate, // 1/40 at 48 kHz
            last_raw_bit: false,
            ones: 0,
            octet: 0,
            bit_count: 0,
            frame_bytes: Vec::with_capacity(512),
            last_iq: Complex32::new(1.0, 0.0),
            packets: Vec::with_capacity(32),
            latest_packet: None,
            next_id: 1,
        }
    }

    /// Process downconverted complex IQ samples directly using instantaneous FM discriminator
    pub fn process_samples(&mut self, iq: &[Complex32]) {
        for &s in iq {
            let diff = s * self.last_iq.conj();
            let raw_fm = diff.im.atan2(diff.re);
            self.last_iq = s;
            self.process_audio_sample(raw_fm);
        }
    }

    /// Process a stream of demodulated mono audio samples (48,000 Hz)
    #[allow(dead_code)]
    pub fn process_audio(&mut self, audio: &[f32]) {
        for &sample in audio {
            self.process_audio_sample(sample);
        }
    }

    /// Process a single audio sample through the Bell 202 matched filters and DPLL
    #[inline]
    fn process_audio_sample(&mut self, sample: f32) {
        let m_out = self.mark_filter.process(sample);
        let s_out = self.space_filter.process(sample);

        // Fast envelope tracking
        self.mark_env = self.mark_env * 0.80 + m_out.abs() * 0.20;
        self.space_env = self.space_env * 0.80 + s_out.abs() * 0.20;

        // Bipolar discriminator (Mark > 0, Space < 0)
        let disc = self.mark_env - self.space_env;
        self.disc_smooth = self.disc_smooth * 0.80 + disc * 0.20;

        // DPLL zero-crossing synchronization (symbol boundaries at phase 0.0)
        let cur_disc = self.disc_smooth;
        if (cur_disc > 0.0 && self.last_disc <= 0.0) || (cur_disc <= 0.0 && self.last_disc > 0.0) {
            if self.clock_phase > 0.5 {
                self.clock_phase += (1.0 - self.clock_phase) * 0.35;
            } else {
                self.clock_phase -= self.clock_phase * 0.35;
            }
        }
        self.last_disc = cur_disc;

        // Advance symbol clock
        let old_phase = self.clock_phase;
        self.clock_phase += self.phase_step;
        if self.clock_phase >= 1.0 {
            self.clock_phase -= 1.0;
        }

        // Optimal sampling: sample at phase crossing 0.5 (exact center of symbol eye)
        if old_phase < 0.5 && self.clock_phase >= 0.5 {
            let raw_bit = cur_disc > 0.0;
            // NRZI decoding: transition = 0, no transition = 1
            let decoded_bit = if raw_bit == self.last_raw_bit { 1 } else { 0 };
            self.last_raw_bit = raw_bit;

            self.handle_bit(decoded_bit);
        }
    }

    /// Robust HDLC bit receiver with 5-one destuffing and 0x7E flag synchronization
    fn handle_bit(&mut self, bit: u8) {
        if bit == 1 {
            self.ones += 1;
            if self.ones == 7 {
                // Abort sequence (>6 ones): reset receiver frame
                self.frame_bytes.clear();
                self.bit_count = 0;
                self.octet = 0;
            } else if self.ones < 7 {
                self.octet = (self.octet >> 1) | 0x80;
                self.bit_count += 1;
                if self.bit_count == 8 {
                    self.frame_bytes.push(self.octet);
                    self.bit_count = 0;
                    self.octet = 0;
                    if self.frame_bytes.len() > 512 {
                        self.frame_bytes.clear();
                    }
                }
            }
        } else {
            // bit == 0
            if self.ones == 5 {
                // Stuffed 0 bit: drop it!
                self.ones = 0;
            } else if self.ones == 6 {
                // Flag (0x7E, 01111110) candidate!
                self.ones = 0;
                if self.frame_bytes.len() >= 16 {
                    self.process_frame();
                }
                self.frame_bytes.clear();
                self.bit_count = 0;
                self.octet = 0;
            } else {
                self.ones = 0;
                self.octet >>= 1;
                self.bit_count += 1;
                if self.bit_count == 8 {
                    self.frame_bytes.push(self.octet);
                    self.bit_count = 0;
                    self.octet = 0;
                    if self.frame_bytes.len() > 512 {
                        self.frame_bytes.clear();
                    }
                }
            }
        }
    }

    /// Verifies standard AX.25 CRC-16 (FCS)
    pub fn check_crc(bytes: &[u8]) -> bool {
        if bytes.len() < 16 {
            return false;
        }
        let mut crc = 0xFFFFu16;
        for &b in bytes {
            for i in 0..8 {
                let bit = ((b >> i) & 1) as u16;
                let c_bit = crc & 1;
                crc >>= 1;
                if (c_bit ^ bit) != 0 {
                    crc ^= 0x8408;
                }
            }
        }
        crc == 0xF0B8
    }

    /// Checks if a callsign looks like a plausible amateur radio callsign
    pub fn is_valid_callsign(call: &str) -> bool {
        let base = call.split('-').next().unwrap_or(call);
        base.len() >= 2 && base.len() <= 7 && base.chars().all(|c| c.is_ascii_alphanumeric())
    }

    /// Processes completed AX.25 HDLC frame
    fn process_frame(&mut self) {
        if self.frame_bytes.len() < 16 {
            return;
        }

        // Verify FCS CRC-16
        let crc_ok = Self::check_crc(&self.frame_bytes);

        // Parse AX.25 Destination (bytes 0..7)
        let (dest, _) = Self::decode_callsign(&self.frame_bytes[0..7]);
        if dest.is_empty() {
            return;
        }

        // Parse AX.25 Source (bytes 7..14)
        let (src, mut is_last) = Self::decode_callsign(&self.frame_bytes[7..14]);
        // Strict FCS CRC-16 check: Reject corrupted packets immediately to eliminate noise and ghost stations
        if !crc_ok || !Self::is_valid_callsign(&src) {
            return;
        }

        // Digipeaters (optional, 7 bytes each)
        let mut offset = 14;
        let mut digis = Vec::new();

        while !is_last && offset + 7 <= self.frame_bytes.len() {
            let (digi, last) = Self::decode_callsign(&self.frame_bytes[offset..offset + 7]);
            if !digi.is_empty() {
                digis.push(digi);
            }
            is_last = last;
            offset += 7;
        }

        // Control byte and PID
        if offset + 2 > self.frame_bytes.len() {
            return;
        }

        let control = self.frame_bytes[offset];
        offset += 1;
        // In APRS UI frames, control is typically 0x03 or 0x13
        if control != 0x03 && control != 0x13 {
            return;
        }

        let pid = self.frame_bytes[offset];
        offset += 1;
        // PID 0xF0 = no layer 3 protocol (standard APRS)
        if pid != 0xF0 {
            return;
        }

        // Info field (payload text) - discard 2 FCS CRC bytes at the end
        let payload_len = if self.frame_bytes.len() >= offset + 2 {
            self.frame_bytes.len() - offset - 2
        } else {
            self.frame_bytes.len() - offset
        };

        if payload_len == 0 {
            return;
        }

        let info_slice = &self.frame_bytes[offset..offset + payload_len];
        let payload = String::from_utf8_lossy(info_slice).trim().to_string();

        if payload.is_empty() {
            return;
        }

        let (lat, lon, comment) = Self::parse_aprs_payload(&payload);
        let timestamp = Self::get_timestamp();

        let packet_id = self.next_id;
        self.next_id += 1;

        let packet = AprsPacket {
            id: packet_id,
            source: src,
            destination: dest,
            digipeaters: digis,
            payload,
            timestamp,
            comment,
            latitude: lat,
            longitude: lon,
        };

        self.latest_packet = Some(packet.clone());
        if self.packets.len() >= 30 {
            self.packets.remove(0);
        }
        self.packets.push(packet);
    }

    /// Decodes a 7-byte AX.25 callsign + SSID
    pub fn decode_callsign(bytes: &[u8]) -> (String, bool) {
        if bytes.len() < 7 {
            return (String::new(), true);
        }
        let mut call = String::new();
        for &b in &bytes[0..6] {
            let ch = (b >> 1) as char;
            if ch != ' ' && (ch.is_ascii_alphanumeric() || ch == '-') {
                call.push(ch);
            }
        }
        let ssid_byte = bytes[6];
        let ssid = (ssid_byte >> 1) & 0x0F;
        let is_last = (ssid_byte & 0x01) != 0;

        if !call.is_empty() && ssid > 0 {
            call = format!("{}-{}", call, ssid);
        }
        (call, is_last)
    }

    /// Parses APRS position coordinates e.g. 6010.50N/02457.30E
    pub fn parse_aprs_payload(payload: &str) -> (Option<f64>, Option<f64>, String) {
        let chars: Vec<char> = payload.chars().collect();
        if chars.is_empty() {
            return (None, None, String::new());
        }

        // Position packet indicators: '!', '=', '/', '@'
        let mut start_idx = 0;
        if chars[0] == '!' || chars[0] == '=' || chars[0] == '/' || chars[0] == '@' {
            start_idx = 1;
            // Time field for '@' or '/' is 7 chars (e.g. 092345z)
            if (chars[0] == '@' || chars[0] == '/') && chars.len() > 8 {
                start_idx = 8;
            }
        }

        // Search for DDMM.hhN and DDDMM.hhE
        if chars.len() >= start_idx + 18 {
            let lat_chars = &chars[start_idx..start_idx + 8];
            let sep = chars[start_idx + 8];
            let lon_chars = &chars[start_idx + 9..start_idx + 18];

            let lat_hemi = lat_chars[7];
            let lon_hemi = lon_chars[8];

            if (sep == '/' || sep == '\\' || sep == '$' || sep == 'r')
                && (lat_hemi == 'N' || lat_hemi == 'S')
                && (lon_hemi == 'E' || lon_hemi == 'W')
            {
                // Parse Latitude DDMM.MM using safe char extraction
                let deg_str: String = lat_chars[0..2].iter().collect();
                let min_str: String = lat_chars[2..7].iter().collect();

                let deg = deg_str.parse::<f64>().ok();
                let min = min_str.parse::<f64>().ok();

                let lat = if let (Some(d), Some(m)) = (deg, min) {
                    let mut val = d + (m / 60.0);
                    if lat_hemi == 'S' {
                        val = -val;
                    }
                    Some(val)
                } else {
                    None
                };

                // Parse Longitude DDDMM.MM using safe char extraction
                let lon_deg_str: String = lon_chars[0..3].iter().collect();
                let lon_min_str: String = lon_chars[3..8].iter().collect();

                let l_deg = lon_deg_str.parse::<f64>().ok();
                let l_min = lon_min_str.parse::<f64>().ok();

                let lon = if let (Some(d), Some(m)) = (l_deg, l_min) {
                    let mut val = d + (m / 60.0);
                    if lon_hemi == 'W' {
                        val = -val;
                    }
                    Some(val)
                } else {
                    None
                };

                let comment: String = if chars.len() > start_idx + 19 {
                    chars[start_idx + 19..].iter().collect()
                } else {
                    String::new()
                };

                return (lat, lon, comment.trim().to_string());
            }
        }

        (None, None, payload.to_string())
    }

    fn get_timestamp() -> String {
        let now = SystemTime::now();
        let dur = now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
        let secs = dur.as_secs();
        let hours = (secs / 3600) % 24;
        let mins = (secs / 60) % 60;
        let s = secs % 60;
        format!("{:02}:{:02}:{:02}", hours, mins, s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_callsign() {
        let bytes = vec![0x9E, 0x90, 0x64, 0xB0, 0xB2, 0xB4, 0x13];
        let (call, last) = AprsDecoder::decode_callsign(&bytes);
        assert_eq!(call, "OH2XYZ-9");
        assert!(last);
    }

    #[test]
    fn test_parse_position_payload() {
        let payload = "!6010.50N/02457.30E#OH2XYZ Mobile Helsinki";
        let (lat, lon, comment) = AprsDecoder::parse_aprs_payload(payload);
        assert!(lat.is_some());
        assert!(lon.is_some());
        let lat_val = lat.unwrap();
        let lon_val = lon.unwrap();
        assert!((lat_val - 60.175).abs() < 0.001);
        assert!((lon_val - 24.955).abs() < 0.001);
        assert_eq!(comment, "OH2XYZ Mobile Helsinki");
    }

    #[test]
    fn test_end_to_end_aprs_demodulation() {
        let mut decoder = AprsDecoder::new(48_000.0);

        // Build valid AX.25 packet: OH2XYZ>APRS:!6010.50N/02457.30E#Test
        let mut frame_bytes = Vec::new();
        // Destination: APRS (shifted << 1, SSID 0)
        let dest = "APRS  ";
        for c in dest.chars() {
            frame_bytes.push((c as u8) << 1);
        }
        frame_bytes.push(0x60); // SSID 0

        // Source: OH2XYZ (shifted << 1, SSID 0, last=1)
        let src = "OH2XYZ";
        for c in src.chars() {
            frame_bytes.push((c as u8) << 1);
        }
        frame_bytes.push(0x61); // SSID 0, last=1

        // Control & PID
        frame_bytes.push(0x03);
        frame_bytes.push(0xF0);

        // Payload
        let payload = "!6010.50N/02457.30E#Test APRS";
        frame_bytes.extend_from_slice(payload.as_bytes());

        // FCS CRC-16
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

        // Verify FCS passes
        assert!(AprsDecoder::check_crc(&frame_bytes));

        // Generate HDLC bitstream with preamble flags and bit stuffing
        let mut bits = Vec::new();
        // 16 preamble flags
        for _ in 0..16 {
            for i in 0..8 {
                bits.push((0x7Eu8 >> i) & 1);
            }
        }

        let mut ones = 0;
        for &b in &frame_bytes {
            for i in 0..8 {
                let bit = (b >> i) & 1;
                bits.push(bit);
                if bit == 1 {
                    ones += 1;
                    if ones == 5 {
                        bits.push(0); // stuffed bit
                        ones = 0;
                    }
                } else {
                    ones = 0;
                }
            }
        }

        // 4 postamble flags
        for _ in 0..4 {
            for i in 0..8 {
                bits.push((0x7Eu8 >> i) & 1);
            }
        }

        // NRZI encoding
        let mut nrzi = Vec::new();
        let mut state = 0u8;
        for &b in &bits {
            if b == 0 {
                state ^= 1;
            }
            nrzi.push(state);
        }

        // Synthesize 48 kHz Bell 202 audio
        let fs = 48_000.0f32;
        let sps = 40; // 48000 / 1200
        let mut audio = Vec::new();
        let mut phase = 0.0f32;
        for &bit in &nrzi {
            let freq = if bit == 0 { 1200.0f32 } else { 2200.0f32 };
            for _ in 0..sps {
                audio.push(phase.sin());
                phase += 2.0 * PI * freq / fs;
                if phase > 2.0 * PI {
                    phase -= 2.0 * PI;
                }
            }
        }

        decoder.process_audio(&audio);

        assert!(decoder.latest_packet.is_some(), "APRS packet must be decoded successfully!");
        let pkt = decoder.latest_packet.unwrap();
        assert_eq!(pkt.source, "OH2XYZ");
        assert_eq!(pkt.destination, "APRS");
        assert!(pkt.latitude.is_some());
        assert!(pkt.longitude.is_some());
        let lat = pkt.latitude.unwrap();
        let lon = pkt.longitude.unwrap();
        assert!((lat - 60.175).abs() < 0.001);
        assert!((lon - 24.955).abs() < 0.001);
    }
}
