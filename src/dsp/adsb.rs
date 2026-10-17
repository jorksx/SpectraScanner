#![allow(dead_code)]

use num_complex::Complex32;
use std::collections::HashMap;
use std::time::Instant;

/// Precomputed Mode S CRC-24 parity lookup table (dump1090 / ICAO Annex 10 Doc 9688)
pub const MODES_CHECKSUM_TABLE: [u32; 112] = [
    0x3935ea, 0x1c9af5, 0xf1b77e, 0x78dbbf, 0xc397db, 0x9e31e9, 0xb0e2f0, 0x587178,
    0x2c38bc, 0x161c5e, 0x0b0e2f, 0xfa7d13, 0x82c48d, 0xbe9842, 0x5f4c21, 0xd05c14,
    0x682e0a, 0x341705, 0xe5f186, 0x72f8c3, 0xc68665, 0x9cb936, 0x4e5c9b, 0xd8d449,
    0x939020, 0x49c810, 0x24e408, 0x127204, 0x093902, 0x049c81, 0xfdb444, 0x7eda22,
    0x3f6d11, 0xe04c8c, 0x702646, 0x381323, 0xe3f395, 0x8e03ce, 0x4701e7, 0xdc7af7,
    0x91c77f, 0xb719bb, 0xa476d9, 0xadc168, 0x56e0b4, 0x2b705a, 0x15b82d, 0xf52612,
    0x7a9309, 0xc2b380, 0x6159c0, 0x30ace0, 0x185670, 0x0c2b38, 0x06159c, 0x030ace,
    0x018567, 0xff38b7, 0x80665f, 0xbfc92b, 0xa01e91, 0xaff54c, 0x57faa6, 0x2bfd53,
    0xea04ad, 0x8af852, 0x457c29, 0xdd4410, 0x6ea208, 0x375104, 0x1ba882, 0x0dd441,
    0xf91024, 0x7c8812, 0x3e4409, 0xe0d800, 0x706c00, 0x383600, 0x1c1b00, 0x0e0d80,
    0x0706c0, 0x038360, 0x01c1b0, 0x00e0d8, 0x00706c, 0x003836, 0x001c1b, 0xfff409,
    0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000,
    0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000,
    0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000, 0x000000,
];

/// Computes the CRC of the data portion (excluding the last 24 bits)
pub fn modes_compute_crc(msg: &[u8], num_bits: usize) -> u32 {
    let mut crc = 0u32;
    let offset = if num_bits == 112 { 0 } else { 56 };
    let data_bits = if num_bits >= 24 { num_bits - 24 } else { 0 };
    for j in 0..data_bits {
        let byte = j / 8;
        let bit = j % 8;
        let bitmask = 1 << (7 - bit);
        if byte < msg.len() && (msg[byte] & bitmask) != 0 {
            crc ^= MODES_CHECKSUM_TABLE[j + offset];
        }
    }
    crc & 0x00FF_FFFF
}

/// Computes the 24-bit Mode S CRC syndrome over a message.
/// For a valid received frame, the returned syndrome is 0.
pub fn modes_checksum(msg: &[u8], num_bits: usize) -> u32 {
    let crc = modes_compute_crc(msg, num_bits);
    let bytes_len = num_bits / 8;
    if bytes_len < 3 || msg.len() < bytes_len {
        return crc;
    }
    let rem = ((msg[bytes_len - 3] as u32) << 16)
        | ((msg[bytes_len - 2] as u32) << 8)
        | (msg[bytes_len - 1] as u32);
    (crc ^ rem) & 0x00FF_FFFF
}

/// Helper function to perform euclidean modulo: a - b * floor(a / b)
#[inline]
pub fn cpr_mod(a: f64, b: f64) -> f64 {
    a - b * (a / b).floor()
}

/// Computes the number of longitude zones (NL) for a given latitude
/// Implements ICAO Annex 10 Compact Position Reporting (CPR) standard NL function
pub fn cpr_nl(lat: f64) -> i32 {
    let lat = lat.abs();
    if lat >= 87.0 {
        return 1;
    }
    let nz = 15.0;
    let num = 1.0 - (1.0 - (std::f64::consts::PI / (2.0 * nz)).cos()) / (lat.to_radians().cos().powi(2));
    if num < -1.0 || num > 1.0 {
        return 1;
    }
    let den = num.acos();
    let nl = (2.0 * std::f64::consts::PI / den).floor() as i32;
    nl.clamp(1, 59)
}

/// Decodes Compact Position Reporting (CPR) locally using a known reference position (e.g. Receiver QTH)
/// Resolves unambiguously within a 180 NM (~330 km) radius from the reference point
pub fn cpr_decode_local(
    lat_cpr: f64,
    lon_cpr: f64,
    is_odd: bool,
    ref_lat: f64,
    ref_lon: f64,
) -> Option<(f64, f64)> {
    let dlat = 360.0 / if is_odd { 59.0 } else { 60.0 };
    let j = (ref_lat / dlat).floor() + ((cpr_mod(ref_lat, dlat) / dlat) - lat_cpr + 0.5).floor();
    let mut lat = dlat * (j + lat_cpr);
    if lat > 90.0 {
        lat -= 360.0;
    }

    let nl = cpr_nl(lat) - if is_odd { 1 } else { 0 };
    let nl = nl.max(1);
    let dlon = 360.0 / (nl as f64);
    let m = (ref_lon / dlon).floor() + ((cpr_mod(ref_lon, dlon) / dlon) - lon_cpr + 0.5).floor();
    let mut lon = dlon * (m + lon_cpr);
    if lon > 180.0 {
        lon -= 360.0;
    } else if lon < -180.0 {
        lon += 360.0;
    }

    if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) {
        Some((lat, lon))
    } else {
        None
    }
}

/// Decodes Compact Position Reporting (CPR) globally using an Even and an Odd frame pair
pub fn cpr_decode_global(
    lat_even: f64,
    lon_even: f64,
    lat_odd: f64,
    lon_odd: f64,
    most_recent_is_odd: bool,
) -> Option<(f64, f64)> {
    let dlat_even = 360.0 / 60.0;
    let dlat_odd = 360.0 / 59.0;

    let j = (59.0 * lat_even - 60.0 * lat_odd + 0.5).floor();
    let mut rlat_even = dlat_even * (cpr_mod(j, 60.0) + lat_even);
    let mut rlat_odd = dlat_odd * (cpr_mod(j, 59.0) + lat_odd);

    if rlat_even >= 270.0 {
        rlat_even -= 360.0;
    }
    if rlat_odd >= 270.0 {
        rlat_odd -= 360.0;
    }

    let lat = if most_recent_is_odd { rlat_odd } else { rlat_even };

    if cpr_nl(rlat_even) != cpr_nl(rlat_odd) {
        return None; // Crosses latitude zone boundary
    }

    let nl = cpr_nl(lat);

    let (lon, nl_used) = if most_recent_is_odd {
        let ni = (nl - 1).max(1);
        let m = (lon_even * (nl - 1) as f64 - lon_odd * nl as f64 + 0.5).floor();
        let dlon = 360.0 / (ni as f64);
        (dlon * (cpr_mod(m, ni as f64) + lon_odd), ni)
    } else {
        let ni = nl.max(1);
        let m = (lon_even * (nl - 1) as f64 - lon_odd * nl as f64 + 0.5).floor();
        let dlon = 360.0 / (ni as f64);
        (dlon * (cpr_mod(m, ni as f64) + lon_even), ni)
    };

    let _ = nl_used;
    let mut final_lon = lon;
    if final_lon > 180.0 {
        final_lon -= 360.0;
    } else if final_lon < -180.0 {
        final_lon += 360.0;
    }

    if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&final_lon) {
        Some((lat, final_lon))
    } else {
        None
    }
}

/// Decodes 6-bit Mode S ASCII characters
pub fn decode_modes_char(c: u8) -> char {
    match c {
        1..=26 => (b'A' + c - 1) as char,
        48..=57 => (b'0' + c - 48) as char,
        32 => ' ',
        _ => ' ',
    }
}

/// Represents a decoded ADS-B Mode S message (DF17 / DF18)
#[derive(Clone, Debug, PartialEq)]
pub struct AdsbMessage {
    pub icao: u32,
    pub callsign: Option<String>,
    pub altitude_ft: Option<i32>,
    pub speed_kts: Option<u32>,
    pub heading_deg: Option<f32>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub is_even_cpr: Option<bool>,
    pub raw_cpr: Option<(f64, f64)>,
    pub tc: u8,
}

/// Parses a 112-bit (14-byte) raw Mode S Extended Squitter ADS-B frame
pub fn parse_adsb_frame(
    msg: &[u8; 14],
    receiver_qth: Option<(f64, f64)>,
    last_even: Option<(f64, f64, Instant)>,
    last_odd: Option<(f64, f64, Instant)>,
) -> Option<AdsbMessage> {
    // 1. Verify CRC-24 (DF17/18 parity checksum over 112 bits must equal 0)
    if modes_checksum(msg, 112) != 0 {
        return None;
    }

    // 2. Downlink Format (DF)
    let df = (msg[0] >> 3) & 0x1F;
    if df != 17 && df != 18 {
        return None;
    }

    // 3. 24-bit ICAO aircraft address
    let icao = ((msg[1] as u32) << 16) | ((msg[2] as u32) << 8) | (msg[3] as u32);
    if icao == 0 || icao == 0xFFFFFF {
        return None;
    }

    // 4. Type Code (TC) in payload
    let tc = (msg[4] >> 3) & 0x1F;

    let mut callsign = None;
    let mut altitude_ft = None;
    let mut speed_kts = None;
    let mut heading_deg = None;
    let mut latitude = None;
    let mut longitude = None;
    let mut is_even_cpr = None;
    let mut raw_cpr = None;

    match tc {
        // TC 1..=4: Aircraft Identification & Category (Callsign)
        1..=4 => {
            let chars = [
                (msg[5] >> 2) & 0x3F,
                ((msg[5] & 0x03) << 4) | (msg[6] >> 4),
                ((msg[6] & 0x0F) << 2) | (msg[7] >> 6),
                msg[7] & 0x3F,
                (msg[8] >> 2) & 0x3F,
                ((msg[8] & 0x03) << 4) | (msg[9] >> 4),
                ((msg[9] & 0x0F) << 2) | (msg[10] >> 6),
                msg[10] & 0x3F,
            ];
            let raw_cs: String = chars.iter().map(|&c| decode_modes_char(c)).collect();
            let cs = raw_cs.trim();
            if !cs.is_empty() {
                callsign = Some(cs.to_string());
            }
        }

        // TC 9..=18 & 20..=22: Airborne Position (Barometric / GNSS Altitude & CPR coordinates)
        9..=18 | 20..=22 => {
            // Altitude decoding (12 bits)
            let raw_alt = (((msg[5] as u16) & 0x01) << 11) | ((msg[6] as u16) << 3) | ((msg[7] as u16) >> 5);
            let q_bit = (raw_alt & 0x10) != 0;
            if q_bit {
                let n = ((raw_alt >> 5) << 4) | (raw_alt & 0x0F);
                let alt = (n as i32) * 25 - 1000;
                if (-1000..=65000).contains(&alt) {
                    altitude_ft = Some(alt);
                }
            }

            // CPR Format (0 = Even, 1 = Odd)
            let is_odd = ((msg[6] >> 2) & 0x01) == 1;
            is_even_cpr = Some(!is_odd);

            // 17-bit CPR Latitude & Longitude
            let raw_lat = (((msg[6] as u32) & 0x03) << 15) | ((msg[7] as u32) << 7) | ((msg[8] as u32) >> 1);
            let raw_lon = (((msg[8] as u32) & 0x01) << 16) | ((msg[9] as u32) << 8) | (msg[10] as u32);
            let cpr_lat = (raw_lat as f64) / 131072.0;
            let cpr_lon = (raw_lon as f64) / 131072.0;
            raw_cpr = Some((cpr_lat, cpr_lon));

            // Priority 1: Local CPR decoding if Receiver QTH is known
            if let Some((qth_lat, qth_lon)) = receiver_qth {
                if let Some((la, lo)) = cpr_decode_local(cpr_lat, cpr_lon, is_odd, qth_lat, qth_lon) {
                    latitude = Some(la);
                    longitude = Some(lo);
                }
            }

            // Priority 2: Global CPR pairing if paired frame is available within 10 seconds
            if latitude.is_none() {
                if is_odd {
                    if let Some((even_lat, even_lon, t)) = last_even {
                        if t.elapsed().as_secs() <= 10 {
                            if let Some((la, lo)) = cpr_decode_global(even_lat, even_lon, cpr_lat, cpr_lon, true) {
                                latitude = Some(la);
                                longitude = Some(lo);
                            }
                        }
                    }
                } else {
                    if let Some((odd_lat, odd_lon, t)) = last_odd {
                        if t.elapsed().as_secs() <= 10 {
                            if let Some((la, lo)) = cpr_decode_global(cpr_lat, cpr_lon, odd_lat, odd_lon, false) {
                                latitude = Some(la);
                                longitude = Some(lo);
                            }
                        }
                    }
                }
            }
        }

        // TC 19: Airborne Velocity & Heading
        19 => {
            let sub = msg[4] & 0x07;
            if sub == 1 || sub == 2 {
                // Ground speed
                let ew_dir = (msg[5] >> 2) & 0x01;
                let ew_vel = (((msg[5] as i32) & 0x03) << 8) | (msg[6] as i32);
                let v_ew = if ew_dir == 1 { -(ew_vel - 1) } else { ew_vel - 1 };

                let ns_dir = (msg[7] >> 7) & 0x01;
                let ns_vel = (((msg[7] as i32) & 0x7F) << 3) | ((msg[8] as i32) >> 5);
                let v_ns = if ns_dir == 1 { -(ns_vel - 1) } else { ns_vel - 1 };

                let spd = ((v_ew * v_ew + v_ns * v_ns) as f64).sqrt().round() as u32;
                let mut hdg = (v_ew as f64).atan2(v_ns as f64).to_degrees();
                if hdg < 0.0 {
                    hdg += 360.0;
                }

                speed_kts = Some(spd);
                heading_deg = Some(hdg as f32);
            }
        }

        _ => {}
    }

    Some(AdsbMessage {
        icao,
        callsign,
        altitude_ft,
        speed_kts,
        heading_deg,
        latitude,
        longitude,
        is_even_cpr,
        raw_cpr,
        tc,
    })
}

/// Real-time 1090 MHz Mode S ADS-B Demodulator and State Tracker
#[derive(Clone, Debug)]
pub struct AdsbDecoder {
    sample_rate: f32,
    pub receiver_qth: Option<(f64, f64)>,
    last_even: HashMap<u32, (f64, f64, Instant)>,
    last_odd: HashMap<u32, (f64, f64, Instant)>,
    known_callsigns: HashMap<u32, String>,
    known_altitudes: HashMap<u32, i32>,
    known_speeds: HashMap<u32, u32>,
    known_headings: HashMap<u32, f32>,
    tail_mag: Vec<u16>,
}

impl AdsbDecoder {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            receiver_qth: None,
            last_even: HashMap::new(),
            last_odd: HashMap::new(),
            known_callsigns: HashMap::new(),
            known_altitudes: HashMap::new(),
            known_speeds: HashMap::new(),
            known_headings: HashMap::new(),
            tail_mag: Vec::new(),
        }
    }

    pub fn set_receiver_qth(&mut self, qth: (f64, f64)) {
        self.receiver_qth = Some(qth);
    }

    /// Primary DSP demodulator: scans wideband IQ samples for 1090 MHz Mode S preambles & PPM data
    pub fn process_iq(&mut self, iq: &[Complex32], sample_rate: f32, _snr_db: f32) -> Vec<AdsbMessage> {
        let mut results = Vec::new();
        if iq.is_empty() {
            return results;
        }

        let effective_rate = if sample_rate >= 1_000_000.0 { sample_rate } else { self.sample_rate.max(2_000_000.0) };
        let is_10m = (effective_rate - 10_000_000.0).abs() < 500_000.0;
        let s: usize = if is_10m { 5 } else { 1 };

        // 1. Convert input I/Q to 16-bit magnitudes.
        // For 10 MSPS (Airspy R2): retain full 100 ns resolution to detect pulses at any sample phase.
        // For 2 MSPS (RTL-SDR): native 0.5 µs resolution.
        // For other rates: resample linearly to standard 2.0 MSPS.
        let cur_mag: Vec<u16> = if is_10m {
            iq.iter().map(|s| (s.norm() * 65535.0).min(65535.0) as u16).collect()
        } else if (effective_rate - 2_000_000.0).abs() < 100_000.0 {
            iq.iter().map(|s| (s.norm() * 65535.0).min(65535.0) as u16).collect()
        } else {
            let step = effective_rate / 2_000_000.0;
            let out_len = (iq.len() as f32 / step).floor() as usize;
            let mut out = Vec::with_capacity(out_len);
            for k in 0..out_len {
                let pos = k as f32 * step;
                let idx0 = pos.floor() as usize;
                let idx1 = (idx0 + 1).min(iq.len() - 1);
                let frac = pos - idx0 as f32;
                let m0 = iq[idx0].norm();
                let m1 = iq[idx1].norm();
                let m = (m0 * (1.0 - frac) + m1 * frac) * 65535.0;
                out.push(m.min(65535.0) as u16);
            }
            out
        };

        // Prepend tail from previous batch to guarantee no boundary loss
        let mut raw_mag = Vec::with_capacity(self.tail_mag.len() + cur_mag.len());
        raw_mag.extend_from_slice(&self.tail_mag);
        raw_mag.extend_from_slice(&cur_mag);

        let min_required = (16 + 224) * s; // 240 symbols
        if raw_mag.len() < min_required + s {
            self.tail_mag = raw_mag;
            return results;
        }

        // 2. Compute symbol-matched integrated energy box_mag[k]
        // If s == 5 (10 MSPS): moving boxcar sum of 5 samples (0.5 µs pulse duration)
        // If s == 1 (2 MSPS): direct magnitude value
        let box_mag: Vec<u32> = if s == 5 {
            let mut bm = Vec::with_capacity(raw_mag.len().saturating_sub(4));
            let mut run_sum: u32 = raw_mag[..5].iter().map(|&x| x as u32).sum();
            bm.push(run_sum);
            for i in 5..raw_mag.len() {
                run_sum += raw_mag[i] as u32;
                run_sum -= raw_mag[i - 5] as u32;
                bm.push(run_sum);
            }
            bm
        } else {
            raw_mag.iter().map(|&x| x as u32).collect()
        };

        let max_search = box_mag.len().saturating_sub(min_required);
        let mut j = 0;
        let scale_floor = 200 * s as u32;

        while j < max_search {
            // Preamble pulse pattern in units of 0.5 µs symbol steps:
            // Pulses: 0 (0.0 µs), 2 (1.0 µs), 7 (3.5 µs), 9 (4.5 µs)
            // Valleys: 1 (0.5 µs), 3 (1.5 µs), 4..6 (2.0-3.0 µs), 8 (4.0 µs)
            let p0 = box_mag[j];
            let p1 = box_mag[j + s];
            let p2 = box_mag[j + 2 * s];
            let p3 = box_mag[j + 3 * s];
            let p4 = box_mag[j + 4 * s];
            let p5 = box_mag[j + 5 * s];
            let p6 = box_mag[j + 6 * s];
            let p7 = box_mag[j + 7 * s];
            let p8 = box_mag[j + 8 * s];
            let p9 = box_mag[j + 9 * s];

            // Primary peak-and-valley relations (dump1090 standard)
            if !(p0 > p1 && p1 < p2 && p2 > p3 && p3 < p0 && p4 < p0 && p5 < p0 && p6 < p0 && p7 > p8 && p8 < p9 && p9 > p6) {
                j += 1;
                continue;
            }

            // High spike level threshold (~66% of pulse peak average)
            let high = (p0 + p2 + p7 + p9) / 6;
            if high < scale_floor {
                j += 1;
                continue;
            }

            // Valleys between pulses 2 and 3 must be below threshold
            if p4 >= high || p5 >= high {
                j += 1;
                continue;
            }

            // Space between preamble and data (samples 11 to 14) must be quiet
            if box_mag[j + 11 * s] >= high
                || box_mag[j + 12 * s] >= high
                || box_mag[j + 13 * s] >= high
                || box_mag[j + 14 * s] >= high
            {
                j += 1;
                continue;
            }

            // Demodulate 112 bits PPM (Manchester bit i at symbol 16 + 2*i vs 16 + 2*i + 1)
            let mut bits = [0u8; 112];
            let noise_delta = 250 * s as i32;
            for i in 0..112 {
                let low = box_mag[j + (16 + 2 * i) * s] as i32;
                let high_s = box_mag[j + (16 + 2 * i + 1) * s] as i32;
                let delta = (low - high_s).abs();
                bits[i] = if i > 0 && delta < noise_delta {
                    bits[i - 1]
                } else if low > high_s {
                    1
                } else {
                    0
                };
            }

            // Pack 112 bits into 14 bytes
            let mut frame = [0u8; 14];
            for (b, &bit) in bits.iter().enumerate() {
                frame[b / 8] |= bit << (7 - (b % 8));
            }

            let df = (frame[0] >> 3) & 0x1F;
            if df == 17 || df == 18 {
                // Verify 24-bit Mode S CRC parity syndrome
                if modes_checksum(&frame, 112) == 0 {
                    if let Some(msg) = self.process_raw_frame(&frame) {
                        results.push(msg);
                        j += min_required; // Advance past this decoded frame
                        continue;
                    }
                }
            }

            j += 1;
        }

        // Keep last samples for the next batch to guarantee no boundary loss
        let keep_len = min_required.min(raw_mag.len());
        self.tail_mag = raw_mag[raw_mag.len() - keep_len..].to_vec();

        results
    }

    /// Ingests and updates state tracking from a raw 14-byte frame
    pub fn process_raw_frame(&mut self, frame: &[u8; 14]) -> Option<AdsbMessage> {
        let df = (frame[0] >> 3) & 0x1F;
        if df != 17 && df != 18 {
            return None;
        }

        let icao = ((frame[1] as u32) << 16) | ((frame[2] as u32) << 8) | (frame[3] as u32);
        let last_ev = self.last_even.get(&icao).copied();
        let last_od = self.last_odd.get(&icao).copied();

        if let Some(mut parsed) = parse_adsb_frame(frame, self.receiver_qth, last_ev, last_od) {
            // Update callsign cache
            if let Some(ref cs) = parsed.callsign {
                self.known_callsigns.insert(icao, cs.clone());
            } else if let Some(known_cs) = self.known_callsigns.get(&icao) {
                parsed.callsign = Some(known_cs.clone());
            }

            // Update altitude cache
            if let Some(alt) = parsed.altitude_ft {
                self.known_altitudes.insert(icao, alt);
            } else if let Some(&known_alt) = self.known_altitudes.get(&icao) {
                parsed.altitude_ft = Some(known_alt);
            }

            // Update velocity cache
            if let Some(spd) = parsed.speed_kts {
                self.known_speeds.insert(icao, spd);
            } else if let Some(&known_spd) = self.known_speeds.get(&icao) {
                parsed.speed_kts = Some(known_spd);
            }

            if let Some(hdg) = parsed.heading_deg {
                self.known_headings.insert(icao, hdg);
            } else if let Some(&known_hdg) = self.known_headings.get(&icao) {
                parsed.heading_deg = Some(known_hdg);
            }

            // Update CPR frames cache
            if let (Some(is_even), Some((cpr_la, cpr_lo))) = (parsed.is_even_cpr, parsed.raw_cpr) {
                let now = Instant::now();
                if is_even {
                    self.last_even.insert(icao, (cpr_la, cpr_lo, now));
                } else {
                    self.last_odd.insert(icao, (cpr_la, cpr_lo, now));
                }
            }

            return Some(parsed);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_crc24_zero_remainder_on_valid_message() {
        // Known valid Mode S Extended Squitter test vector
        let hex = "8D40621D58C382D690C8AC2863A7";
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();

        assert_eq!(bytes.len(), 14);
        let rem = modes_checksum(&bytes, 112);
        assert_eq!(rem, 0, "Valid Mode S frame must produce CRC-24 syndrome of 0");
    }

    #[test]
    fn test_cpr_nl_function() {
        assert_eq!(cpr_nl(0.0), 59, "Equator must have 59 longitude zones");
        assert_eq!(cpr_nl(60.0), 29, "60 degrees N must have 29 longitude zones");
        assert_eq!(cpr_nl(88.0), 1, "Poles must have 1 longitude zone");
    }

    #[test]
    fn test_cpr_local_decode_accurate() {
        // Helsinki QTH
        let qth = (60.1699, 24.9384);
        let target_lat = 60.3172;
        let target_lon = 24.9633;

        let dlat = 360.0 / 60.0;
        let cpr_lat = cpr_mod(target_lat, dlat) / dlat;

        let nl = cpr_nl(target_lat);
        let dlon = 360.0 / (nl as f64);
        let cpr_lon = cpr_mod(target_lon, dlon) / dlon;

        let res = cpr_decode_local(cpr_lat, cpr_lon, false, qth.0, qth.1);
        assert!(res.is_some());
        let (la, lo) = res.unwrap();
        assert!((la - target_lat).abs() < 0.001);
        assert!((lo - target_lon).abs() < 0.001);
    }

    #[test]
    fn test_adsb_callsign_decoding() {
        // Callsign message for aircraft ICAO 40621D: "EZY854P "
        let mut frame = [0u8; 14];
        frame[0] = 0x8D; // DF17
        frame[1] = 0x40; // ICAO 40621D
        frame[2] = 0x62;
        frame[3] = 0x1D;
        frame[4] = 0x20; // TC 4
        // Encode "EZY854"
        let chars = [5, 26, 25, 56, 53, 52, 32, 32]; // E Z Y 8 5 4 ' ' ' '
        frame[5] = (chars[0] << 2) | (chars[1] >> 4);
        frame[6] = ((chars[1] & 0x0F) << 4) | (chars[2] >> 2);
        frame[7] = ((chars[2] & 0x03) << 6) | chars[3];
        frame[8] = (chars[4] << 2) | (chars[5] >> 4);
        frame[9] = ((chars[5] & 0x0F) << 4) | (chars[6] >> 2);
        frame[10] = ((chars[6] & 0x03) << 6) | chars[7];

        let crc = modes_compute_crc(&frame, 112);
        frame[11] = ((crc >> 16) & 0xFF) as u8;
        frame[12] = ((crc >> 8) & 0xFF) as u8;
        frame[13] = (crc & 0xFF) as u8;

        let msg = parse_adsb_frame(&frame, None, None, None).unwrap();
        assert_eq!(msg.icao, 0x40621D);
        assert_eq!(msg.callsign, Some("EZY854".to_string()));
    }

    #[test]
    fn test_adsb_process_iq_10msps_demodulation() {
        let mut frame = [0u8; 14];
        frame[0] = 0x8D; // DF17
        frame[1] = 0x40; // ICAO 40621D
        frame[2] = 0x62;
        frame[3] = 0x1D;
        frame[4] = 0x20; // TC 4
        let chars = [5, 26, 25, 56, 53, 52, 32, 32];
        frame[5] = (chars[0] << 2) | (chars[1] >> 4);
        frame[6] = ((chars[1] & 0x0F) << 4) | (chars[2] >> 2);
        frame[7] = ((chars[2] & 0x03) << 6) | chars[3];
        frame[8] = (chars[4] << 2) | (chars[5] >> 4);
        frame[9] = ((chars[5] & 0x0F) << 4) | (chars[6] >> 2);
        frame[10] = ((chars[6] & 0x03) << 6) | chars[7];
        let crc = modes_compute_crc(&frame, 112);
        frame[11] = ((crc >> 16) & 0xFF) as u8;
        frame[12] = ((crc >> 8) & 0xFF) as u8;
        frame[13] = (crc & 0xFF) as u8;

        // Build 240 symbols: 16 preamble symbols + 224 data symbols
        let mut symbols = [0.05f32; 240];
        // Preamble pulses at 0, 2, 7, 9
        symbols[0] = 0.85;
        symbols[2] = 0.85;
        symbols[7] = 0.85;
        symbols[9] = 0.85;
        // 112 data bits
        for i in 0..112 {
            let byte = frame[i / 8];
            let bit = (byte >> (7 - (i % 8))) & 1;
            if bit == 1 {
                symbols[16 + 2 * i] = 0.85;
                symbols[16 + 2 * i + 1] = 0.05;
            } else {
                symbols[16 + 2 * i] = 0.05;
                symbols[16 + 2 * i + 1] = 0.85;
            }
        }

        // Test with 10 MSPS and an arbitrary 3-sample phase offset
        let mut iq = vec![Complex32::new(0.02, 0.02); 3]; // 3-sample lead
        for &sym in &symbols {
            for _ in 0..5 {
                iq.push(Complex32::new(sym, 0.0));
            }
        }
        iq.extend(vec![Complex32::new(0.02, 0.02); 50]);

        let mut decoder = AdsbDecoder::new(10_000_000.0);
        let msgs = decoder.process_iq(&iq, 10_000_000.0, 20.0);
        assert_eq!(msgs.len(), 1, "Must decode 10 MSPS ADS-B packet even with phase offset");
        assert_eq!(msgs[0].icao, 0x40621D);
        assert_eq!(msgs[0].callsign, Some("EZY854".to_string()));
    }
}
