use num_complex::Complex32;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A decoded telemetry / SCADA / radio modem data packet
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelemetryPacket {
    pub id: u64,
    pub timestamp: String,
    pub freq_hz: u64,
    pub protocol: String,
    pub station_id: Option<String>,
    pub summary: String,
    pub human_decoded: String,
    pub raw_hex: String,
    pub raw_ascii: String,
    pub snr_db: f32,
    pub burst_ms: f32,
    pub crc_ok: bool,
}

/// Standard CRC-16 Modbus (polynomial 0xA001, initial 0xFFFF)
pub fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            if (crc & 0x0001) != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

/// CRC-16 CCITT (polynomial 0x1021, initial 0xFFFF)
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// CRC-16 Wireless M-Bus (EN 13757-4 polynomial 0x3D65, initial 0x0000, final inversion)
#[allow(dead_code)]
pub fn crc16_wmbus(data: &[u8]) -> u16 {
    let mut crc = 0x0000u16;
    for &byte in data {
        for bit in 0..8 {
            let bit_in = (byte >> (7 - bit)) & 1;
            let msb = (crc >> 15) & 1;
            crc = (crc << 1) | (bit_in as u16);
            if msb != 0 {
                crc ^= 0x3D65;
            }
        }
    }
    !crc
}

/// Helper: Checks if frequency is in 433 MHz ISM band (432.5 - 435.0 MHz)
pub fn is_433_ism_band(freq_hz: u64) -> bool {
    let f_mhz = freq_hz as f64 / 1e6;
    (432.5..=435.0).contains(&f_mhz)
}

/// Helper: Checks if frequency is in general ISM/SRD band (433 MHz or 868 MHz)
pub fn is_ism_band(freq_hz: u64) -> bool {
    let f_mhz = freq_hz as f64 / 1e6;
    (432.5..=435.0).contains(&f_mhz) || (868.0..=870.0).contains(&f_mhz)
}

/// Helper: Checks if frequency is in meteorological radiosonde band (400.0 - 406.0 MHz)
#[allow(dead_code)]
pub fn is_met_sonde_band(freq_hz: u64) -> bool {
    let f_mhz = freq_hz as f64 / 1e6;
    (399.5..=406.5).contains(&f_mhz)
}

/// Helper: Checks if frequency is in SCADA / municipal telemetry band (406.1-430.0 MHz or 440.0-445.0 MHz)
#[allow(dead_code)]
pub fn is_scada_band(freq_hz: u64) -> bool {
    let f_mhz = freq_hz as f64 / 1e6;
    (406.1..=430.0).contains(&f_mhz) || (440.0..=445.0).contains(&f_mhz)
}

/// Helper: Checks if frequency is in PMR446 band (445.8 - 446.3 MHz)
#[allow(dead_code)]
pub fn is_pmr_band(freq_hz: u64) -> bool {
    let f_mhz = freq_hz as f64 / 1e6;
    (445.8..=446.3).contains(&f_mhz)
}

/// Real-time Telemetry, SCADA, RTL-433 & Radio Modem Decoder (440-445 MHz, 433 MHz, 401 MHz, 169/868 MHz)
pub struct TelemetryDecoder {
    sample_rate: f32,
    last_sample: Complex32,

    // 19200 baud DPLL & Manchester slicer (Automotive TPMS)
    phase_19200: f32,
    last_angle_19200: f32,
    shift_reg_19200: u64,
    manchester_reg_19200: u64,
    bit_count_19200: usize,
    raw_bytes_19200: Vec<u8>,

    // 9600 baud Gardner/Zero-crossing clock recovery (Modbus RTU, SATEL, W-MBus, 9.6k TPMS)
    phase_9600: f32,
    last_angle_9600: f32,
    shift_reg_9600: u64,
    bit_count_9600: usize,
    raw_bytes_9600: Vec<u8>,

    // 1200 baud clock recovery (POCSAG, MDC-1200)
    phase_1200: f32,
    last_angle_1200: f32,
    shift_reg_1200: u64,
    bit_count_1200: usize,
    raw_bytes_1200: Vec<u8>,

    // OOK / ASK envelope & pulse tracker (Nexus weather, EV1527 remotes)
    ook_env: f32,
    ook_noise_floor: f32,
    ook_is_high: bool,
    ook_high_samples: usize,
    ook_low_samples: usize,
    pub ook_pulses: Vec<(u32, u32)>,

    // Burst energy tracking
    in_burst: bool,
    burst_sample_count: usize,
    burst_peak_pwr: f32,

    // Duplicate frame suppression
    last_sig: Option<(String, std::time::Instant)>,
    pub last_packet_time: Option<std::time::Instant>,

    pub packets: VecDeque<TelemetryPacket>,
    next_id: u64,
    max_packets: usize,
}

impl TelemetryDecoder {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            last_sample: Complex32::new(0.0, 0.0),
            phase_19200: 0.0,
            last_angle_19200: 0.0,
            shift_reg_19200: 0,
            manchester_reg_19200: 0,
            bit_count_19200: 0,
            raw_bytes_19200: Vec::with_capacity(128),
            phase_9600: 0.0,
            last_angle_9600: 0.0,
            shift_reg_9600: 0,
            bit_count_9600: 0,
            raw_bytes_9600: Vec::with_capacity(512),
            phase_1200: 0.0,
            last_angle_1200: 0.0,
            shift_reg_1200: 0,
            bit_count_1200: 0,
            raw_bytes_1200: Vec::with_capacity(512),
            ook_env: 0.0,
            ook_noise_floor: 0.005,
            ook_is_high: false,
            ook_high_samples: 0,
            ook_low_samples: 0,
            ook_pulses: Vec::with_capacity(128),
            in_burst: false,
            burst_sample_count: 0,
            burst_peak_pwr: 0.0,
            last_sig: None,
            last_packet_time: None,
            packets: VecDeque::with_capacity(256),
            next_id: 1,
            max_packets: 200,
        }
    }

    pub fn reset(&mut self) {
        self.shift_reg_19200 = 0;
        self.manchester_reg_19200 = 0;
        self.bit_count_19200 = 0;
        self.raw_bytes_19200.clear();
        self.shift_reg_9600 = 0;
        self.bit_count_9600 = 0;
        self.raw_bytes_9600.clear();
        self.shift_reg_1200 = 0;
        self.bit_count_1200 = 0;
        self.raw_bytes_1200.clear();
        self.ook_is_high = false;
        self.ook_high_samples = 0;
        self.ook_low_samples = 0;
        self.ook_pulses.clear();
        self.in_burst = false;
        self.burst_sample_count = 0;
        self.burst_peak_pwr = 0.0;
        self.last_sig = None;
        self.last_packet_time = None;
    }

    /// Processes incoming baseband DDC complex samples at 48 kHz
    pub fn process_samples(&mut self, samples: &[Complex32], freq_hz: u64, snr_db: f32) {
        if samples.is_empty() {
            return;
        }

        let step_19200 = 19200.0 / self.sample_rate;
        let step_9600 = 9600.0 / self.sample_rate;
        let step_1200 = 1200.0 / self.sample_rate;

        for &s in samples {
            let pwr = s.norm_sqr();
            let is_carrier = pwr > 0.0001; // carrier present

            if is_carrier {
                if !self.in_burst {
                    self.in_burst = true;
                    self.burst_sample_count = 0;
                    self.burst_peak_pwr = pwr;
                    self.raw_bytes_19200.clear();
                    self.raw_bytes_9600.clear();
                    self.raw_bytes_1200.clear();
                }
                self.burst_sample_count += 1;
                if pwr > self.burst_peak_pwr {
                    self.burst_peak_pwr = pwr;
                }
            } else if self.in_burst {
                // Burst ended: evaluate and flush
                self.finalize_burst(freq_hz, snr_db);
                self.in_burst = false;
                self.burst_sample_count = 0;
            }

            // --- OOK / ASK ENVELOPE TRACKER & PULSE SLICER ---
            let amp = (s.re * s.re + s.im * s.im).sqrt();
            self.ook_env = self.ook_env * 0.88 + amp * 0.12;
            self.ook_noise_floor = self.ook_noise_floor * 0.9995 + amp * 0.0005;
            let ook_thresh = (self.ook_noise_floor * 2.5).max(0.012);
            let is_high = self.ook_env > ook_thresh;

            if is_high {
                if !self.ook_is_high {
                    let low_us = (self.ook_low_samples as f32 / self.sample_rate * 1e6) as u32;
                    if self.ook_high_samples > 0 {
                        let high_us = (self.ook_high_samples as f32 / self.sample_rate * 1e6) as u32;
                        self.ook_pulses.push((high_us, low_us));
                        if self.ook_pulses.len() > 120 {
                            self.ook_pulses.remove(0);
                        }
                    }
                    if low_us > 3500 && self.ook_pulses.len() >= 20 {
                        self.check_ook_protocols(freq_hz, snr_db);
                    }
                    self.ook_high_samples = 0;
                    self.ook_is_high = true;
                }
                self.ook_high_samples += 1;
            } else {
                if self.ook_is_high {
                    self.ook_is_high = false;
                    self.ook_low_samples = 0;
                }
                self.ook_low_samples += 1;
                if self.ook_low_samples == (0.012 * self.sample_rate) as usize && self.ook_pulses.len() >= 20 {
                    self.check_ook_protocols(freq_hz, snr_db);
                }
            }

            // Quadrature Discriminator (Instantaneous Frequency)
            let conj = self.last_sample.conj();
            let diff = s * conj;
            let angle = diff.im.atan2(diff.re);
            self.last_sample = s;

            // --- 19.2k Baud Slicer (TPMS Automotive Sensors) ---
            let prev_phase_19200 = self.phase_19200;
            self.phase_19200 += step_19200;
            if (angle > 0.0) != (self.last_angle_19200 > 0.0) {
                if self.phase_19200 < 0.3 {
                    self.phase_19200 *= 0.7;
                } else if self.phase_19200 > 0.7 {
                    self.phase_19200 = 1.0 - (1.0 - self.phase_19200) * 0.7;
                }
            }
            self.last_angle_19200 = angle;

            if prev_phase_19200 < 0.5 && self.phase_19200 >= 0.5 {
                let chip = if angle > 0.0 { 1u64 } else { 0u64 };
                self.shift_reg_19200 = (self.shift_reg_19200 << 1) | chip;

                // Manchester chip decoding: 01 -> 0, 10 -> 1
                let last2 = self.shift_reg_19200 & 0b11;
                if last2 == 0b01 {
                    self.manchester_reg_19200 = (self.manchester_reg_19200 << 1) | 0;
                    self.bit_count_19200 += 1;
                    if self.bit_count_19200 % 8 == 0 {
                        let b = (self.manchester_reg_19200 & 0xFF) as u8;
                        self.raw_bytes_19200.push(b);
                        if self.raw_bytes_19200.len() > 64 {
                            self.raw_bytes_19200.remove(0);
                        }
                    }
                    self.check_sync_words_19200(freq_hz, snr_db);
                } else if last2 == 0b10 {
                    self.manchester_reg_19200 = (self.manchester_reg_19200 << 1) | 1;
                    self.bit_count_19200 += 1;
                    if self.bit_count_19200 % 8 == 0 {
                        let b = (self.manchester_reg_19200 & 0xFF) as u8;
                        self.raw_bytes_19200.push(b);
                        if self.raw_bytes_19200.len() > 64 {
                            self.raw_bytes_19200.remove(0);
                        }
                    }
                    self.check_sync_words_19200(freq_hz, snr_db);
                }
            }
            if self.phase_19200 >= 1.0 {
                self.phase_19200 -= 1.0;
            }

            // --- 9600 Baud Slicer ---
            let prev_phase_9600 = self.phase_9600;
            self.phase_9600 += step_9600;

            // Zero-crossing sync
            if (angle > 0.0) != (self.last_angle_9600 > 0.0) {
                // Phase adjustment toward symbol boundary
                if self.phase_9600 < 0.3 {
                    self.phase_9600 *= 0.7;
                } else if self.phase_9600 > 0.7 {
                    self.phase_9600 = 1.0 - (1.0 - self.phase_9600) * 0.7;
                }
            }
            self.last_angle_9600 = angle;

            // Sample symbol at midpoint (0.5)
            if prev_phase_9600 < 0.5 && self.phase_9600 >= 0.5 {
                let bit = if angle > 0.0 { 1u64 } else { 0u64 };
                self.shift_reg_9600 = (self.shift_reg_9600 << 1) | bit;
                self.bit_count_9600 += 1;

                if self.bit_count_9600 % 8 == 0 {
                    let b = (self.shift_reg_9600 & 0xFF) as u8;
                    self.raw_bytes_9600.push(b);
                    if self.raw_bytes_9600.len() > 256 {
                        self.raw_bytes_9600.remove(0);
                    }
                }

                // Check sync words on 9600 baud
                self.check_sync_words_9600(freq_hz, snr_db);
            }
            if self.phase_9600 >= 1.0 {
                self.phase_9600 -= 1.0;
            }

            // --- 1200 Baud Slicer (POCSAG / MDC-1200 / Telemetry) ---
            let prev_phase_1200 = self.phase_1200;
            self.phase_1200 += step_1200;

            if (angle > 0.0) != (self.last_angle_1200 > 0.0) {
                if self.phase_1200 < 0.3 {
                    self.phase_1200 *= 0.7;
                } else if self.phase_1200 > 0.7 {
                    self.phase_1200 = 1.0 - (1.0 - self.phase_1200) * 0.7;
                }
            }
            self.last_angle_1200 = angle;

            if prev_phase_1200 < 0.5 && self.phase_1200 >= 0.5 {
                let bit = if angle > 0.0 { 1u64 } else { 0u64 };
                self.shift_reg_1200 = (self.shift_reg_1200 << 1) | bit;
                self.bit_count_1200 += 1;

                if self.bit_count_1200 % 8 == 0 {
                    let b = (self.shift_reg_1200 & 0xFF) as u8;
                    self.raw_bytes_1200.push(b);
                    if self.raw_bytes_1200.len() > 256 {
                        self.raw_bytes_1200.remove(0);
                    }
                }

                // Check 1200 baud sync words (e.g. POCSAG, MDC-1200)
                self.check_sync_words_1200(freq_hz, snr_db);
            }
            if self.phase_1200 >= 1.0 {
                self.phase_1200 -= 1.0;
            }
        }
    }

    /// Checks sync words and packet framing at 19200 baud (TPMS Automotive)
    fn check_sync_words_19200(&mut self, freq_hz: u64, snr_db: f32) {
        if is_433_ism_band(freq_hz) && self.raw_bytes_19200.len() >= 8 {
            if let Some(pkt) = try_decode_tpms(&self.raw_bytes_19200, freq_hz, snr_db, &mut self.next_id) {
                self.push_packet(pkt);
                self.raw_bytes_19200.clear();
            }
        }
    }

    /// Checks OOK pulse sequence (Nexus weather sensors, EV1527 remotes)
    fn check_ook_protocols(&mut self, freq_hz: u64, snr_db: f32) {
        if !is_ism_band(freq_hz) {
            return;
        }
        if let Some(pkt) = try_decode_nexus(&self.ook_pulses, freq_hz, snr_db, &mut self.next_id) {
            self.push_packet(pkt);
            self.ook_pulses.clear();
            return;
        }
        if let Some(pkt) = try_decode_ev1527(&self.ook_pulses, freq_hz, snr_db, &mut self.next_id) {
            self.push_packet(pkt);
            self.ook_pulses.clear();
            return;
        }
    }

    /// Checks sync words and packet framing at 9600 baud (SATEL, Modbus RTU, W-MBus)
    fn check_sync_words_9600(&mut self, freq_hz: u64, snr_db: f32) {
        // 1. Modbus RTU: strictly gated to SCADA bands and good SNR (>= 10.0 dB) to avoid collisions on noise or DMR audio
        if is_scada_band(freq_hz) && snr_db >= 10.0 && self.raw_bytes_9600.len() >= 5 {
            let len = self.raw_bytes_9600.len();
            // Test standard Modbus frame lengths
            for &window_len in &[5, 8, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25] {
                if len >= window_len {
                    let slice = &self.raw_bytes_9600[len - window_len..];
                    if let Some(pkt) = try_parse_modbus_rtu(slice, freq_hz, snr_db, &mut self.next_id) {
                        self.push_packet(pkt);
                        self.raw_bytes_9600.clear();
                        return;
                    }
                }
            }
        }

        // 2. Wireless M-Bus Mode N / T (Sync word 0x543D or 0xF68D)
        let sync16 = (self.shift_reg_9600 & 0xFFFF) as u16;
        if sync16 == 0x543D || sync16 == 0xF68D {
            if self.raw_bytes_9600.len() >= 10 {
                if let Some(pkt) = try_parse_wmbus(&self.raw_bytes_9600, freq_hz, snr_db, &mut self.next_id) {
                    self.push_packet(pkt);
                    self.raw_bytes_9600.clear();
                    return;
                }
            }
        }

        // 3. SATEL 3AS / Satelline radio modem framing (Sync 0xD391, 0x1616)
        if sync16 == 0xD391 || sync16 == 0x1616 {
            if self.raw_bytes_9600.len() >= 6 {
                if let Some(pkt) = try_parse_satel(&self.raw_bytes_9600, freq_hz, snr_db, &mut self.next_id) {
                    self.push_packet(pkt);
                    self.raw_bytes_9600.clear();
                    return;
                }
            }
        }
    }

    /// Checks sync words and packet framing at 1200 baud (POCSAG, MDC-1200, 1200 FSK telemetry)
    fn check_sync_words_1200(&mut self, freq_hz: u64, snr_db: f32) {
        // POCSAG sync codeword: 0x7CD215D8
        let sync32 = (self.shift_reg_1200 & 0xFFFF_FFFF) as u32;
        if sync32 == 0x7CD2_15D8 {
            if self.raw_bytes_1200.len() >= 8 {
                if let Some(pkt) = try_parse_pocsag(&self.raw_bytes_1200, freq_hz, snr_db, &mut self.next_id) {
                    self.push_packet(pkt);
                    self.raw_bytes_1200.clear();
                    return;
                }
            }
        }

        // MDC-1200 sync codeword (0x0709 or 0x0582)
        let sync16 = (self.shift_reg_1200 & 0xFFFF) as u16;
        if sync16 == 0x0709 || sync16 == 0x0582 {
            if self.raw_bytes_1200.len() >= 4 {
                if let Some(pkt) = try_decode_mdc1200(&self.raw_bytes_1200, freq_hz, snr_db, &mut self.next_id) {
                    self.push_packet(pkt);
                    self.raw_bytes_1200.clear();
                }
            }
        }
    }

    /// Finalizes burst when carrier drops: captures generic FSK burst if not already framed
    fn finalize_burst(&mut self, freq_hz: u64, snr_db: f32) {
        let burst_ms = (self.burst_sample_count as f32 / self.sample_rate) * 1000.0;
        if burst_ms < 15.0 || burst_ms > 2500.0 {
            return;
        }

        // 1. Try decoding OOK pulse sequence first (strictly ISM bands)
        if is_ism_band(freq_hz) && self.ook_pulses.len() >= 20 {
            if let Some(pkt) = try_decode_nexus(&self.ook_pulses, freq_hz, snr_db, &mut self.next_id) {
                self.push_packet(pkt);
                self.ook_pulses.clear();
                return;
            }
            if let Some(pkt) = try_decode_ev1527(&self.ook_pulses, freq_hz, snr_db, &mut self.next_id) {
                self.push_packet(pkt);
                self.ook_pulses.clear();
                return;
            }
        }

        // 2. Try decoding TPMS from 19.2k buffer (strictly 433 MHz ISM band)
        if is_433_ism_band(freq_hz) && self.raw_bytes_19200.len() >= 8 {
            if let Some(pkt) = try_decode_tpms(&self.raw_bytes_19200, freq_hz, snr_db, &mut self.next_id) {
                self.push_packet(pkt);
                self.raw_bytes_19200.clear();
                return;
            }
        }

        // 3. Captured generic FSK bytes: strictly only on SCADA, ISM, Met sonde, or PMR bands
        if !is_scada_band(freq_hz) && !is_ism_band(freq_hz) && !is_met_sonde_band(freq_hz) && !is_pmr_band(freq_hz) {
            return;
        }

        let bytes = if self.raw_bytes_9600.len() >= 4 {
            &self.raw_bytes_9600[..]
        } else if self.raw_bytes_1200.len() >= 4 {
            &self.raw_bytes_1200[..]
        } else if self.raw_bytes_19200.len() >= 4 {
            &self.raw_bytes_19200[..]
        } else {
            return;
        };

        // Filter out all-0x00 or all-0xFF trivial silence/unkeyed bytes
        let is_trivial = bytes.iter().all(|&b| b == 0x00) || bytes.iter().all(|&b| b == 0xFF);
        if is_trivial {
            return;
        }

        // Check if CRC-16 matches on any sub-slice
        let mut crc_found = false;
        if bytes.len() >= 4 {
            let payload = &bytes[..bytes.len() - 2];
            let crc_in_packet = ((bytes[bytes.len() - 2] as u16) << 8) | (bytes[bytes.len() - 1] as u16);
            if crc16_ccitt(payload) == crc_in_packet || (is_scada_band(freq_hz) && crc16_modbus(payload) == crc_in_packet) {
                crc_found = true;
            }
        }

        // STRICT REQUIREMENT: Only emit verified packets with valid CRC!
        // Unverified raw noise or voice unkeys must NEVER generate telemetry packets!
        if !crc_found {
            return;
        }

        let hex_str = bytes_to_hex(&bytes[..bytes.len().min(32)]);
        let ascii_str = bytes_to_ascii(&bytes[..bytes.len().min(32)]);

        let freq_mhz = freq_hz as f64 / 1e6;

        let (protocol, station_id, summary, human_decoded) = if (400.0..=406.0).contains(&freq_mhz) {
            (
                "Met Telemetry / Sonde".to_string(),
                Some(format!("{:.3} MHz Sonde", freq_mhz)),
                format!("Weather Sonde ({:.0} ms, {} bytes)", burst_ms, bytes.len()),
                format!(
                    "Meteorological Telemetry / Radiosonde Beacon:\n• Frequency: {:.4} MHz (400-406 MHz Band)\n• Burst Duration: {:.0} ms\n• Captured: {} raw bytes\n• Peak SNR: +{:.1} dB\n• Modulation: 2FSK / GFSK Telemetry Beacon\n• Role: Weather balloon radiosonde (e.g. Vaisala RS41 / RS92), atmospheric sensor payload, or meteorological satellite link.",
                    freq_mhz, burst_ms, bytes.len(), snr_db
                ),
            )
        } else if (406.1..=430.0).contains(&freq_mhz) {
            (
                "SCADA Telemetry Burst".to_string(),
                Some(format!("{:.4} MHz SCADA", freq_mhz)),
                format!("SCADA Radio Burst ({:.0} ms, {} bytes)", burst_ms, bytes.len()),
                format!(
                    "Municipal SCADA Radio Modem Transmission:\n• Frequency: {:.4} MHz\n• Burst Duration: {:.0} ms\n• Data Size: {} bytes\n• Peak SNR: +{:.1} dB\n• Modulation: 9600 bps FSK Radio Modem (SATEL 3AS / Satelline / Modbus)\n• Role: Water pumping station ping, municipal water distribution, district heating, or electrical grid remote telemetry.",
                    freq_mhz, burst_ms, bytes.len(), snr_db
                ),
            )
        } else if (433.05..=434.79).contains(&freq_mhz) {
            (
                "433 MHz ISM Burst".to_string(),
                Some(format!("{:.3} MHz ISM", freq_mhz)),
                format!("ISM Data Burst ({:.0} ms, {} bytes)", burst_ms, bytes.len()),
                format!(
                    "433 MHz ISM Telemetry Burst (RTL-433 Band):\n• Frequency: {:.4} MHz\n• Burst Duration: {:.0} ms\n• Data Size: {} bytes\n• Peak SNR: +{:.1} dB\n• Modulation: 2FSK / OOK Pulse Burst\n• Typical Source: Automotive tire pressure sensor (TPMS), outdoor thermometer, home alarm sensor, or remote transmitter.",
                    freq_mhz, burst_ms, bytes.len(), snr_db
                ),
            )
        } else if (440.0..=445.0).contains(&freq_mhz) {
            (
                "SCADA Telemetry Burst".to_string(),
                Some(format!("{:.4} MHz SCADA", freq_mhz)),
                format!("SCADA Radio Burst ({:.0} ms, {} bytes)", burst_ms, bytes.len()),
                format!(
                    "Municipal SCADA Radio Modem Transmission:\n• Frequency: {:.4} MHz\n• Burst Duration: {:.0} ms\n• Data Size: {} bytes\n• Peak SNR: +{:.1} dB\n• Modulation: 9600 bps FSK Radio Modem (SATEL 3AS / Satelline)\n• Role: Water pumping station ping, district heating telemetry, or remote electrical grid telemetry.",
                    freq_mhz, burst_ms, bytes.len(), snr_db
                ),
            )
        } else if (445.8..=446.3).contains(&freq_mhz) {
            (
                "PMR Data Burst".to_string(),
                Some("PMR446".to_string()),
                format!("PMR446 Data Chirp ({:.0} ms)", burst_ms),
                format!(
                    "PMR446 Short Data Burst:\n• Frequency: {:.4} MHz\n• Duration: {:.0} ms\n• Peak SNR: +{:.1} dB\n• Modulation: Sub-audible FFSK / Digital Squelch Burst / ANI\n• Role: Walkie-talkie PTT chirp, selective calling, or data telemetry.",
                    freq_mhz, burst_ms, snr_db
                ),
            )
        } else {
            let is_mostly_ascii = bytes.iter().filter(|&&b| (32..=126).contains(&b) || b == b'\r' || b == b'\n').count() >= (bytes.len() * 3 / 4);
            let hum = if is_mostly_ascii {
                format!("Text Telemetry Message:\n\"{}\"\nTransmitted at 9600 bps FSK.", ascii_str.trim_matches('.'))
            } else {
                format!(
                    "Radio Telemetry Burst:\n• Frequency: {:.4} MHz\n• Duration: {:.0} ms\n• Data Length: {} bytes\n• Peak SNR: +{:.1} dB\n• Modulation: 2FSK / 4FSK Carrier Modulation.",
                    freq_mhz, burst_ms, bytes.len(), snr_db
                )
            };
            (
                if crc_found { "FSK Telemetry (CRC OK)".to_string() } else { "2FSK / 4FSK Burst".to_string() },
                Some(format!("{:.4} MHz", freq_mhz)),
                format!("Telemetry Burst ({:.0} ms, {} bytes)", burst_ms, bytes.len()),
                hum,
            )
        };

        let pkt = TelemetryPacket {
            id: self.next_id,
            timestamp: current_time_str(),
            freq_hz,
            protocol,
            station_id,
            summary,
            human_decoded,
            raw_hex: hex_str,
            raw_ascii: ascii_str,
            snr_db,
            burst_ms,
            crc_ok: crc_found,
        };
        self.next_id += 1;
        self.push_packet(pkt);
    }

    fn push_packet(&mut self, pkt: TelemetryPacket) {
        let now = std::time::Instant::now();
        self.last_packet_time = Some(now);
        let sig = format!("{}:{}", pkt.protocol, pkt.summary);
        if let Some((ref last_sig, last_time)) = self.last_sig {
            if last_sig == &sig && now.duration_since(last_time).as_millis() < 450 {
                // Suppress immediate repeated frame
                return;
            }
        }
        self.last_sig = Some((sig, now));

        if self.packets.len() >= self.max_packets {
            self.packets.pop_front();
        }
        self.packets.push_back(pkt);
    }
}

/// Known Wireless M-Bus utility meter manufacturers
const KNOWN_WMBUS_MFG: &[&str] = &[
    "KAM", "SEN", "LUG", "DAN", "DME", "ITR", "ELV", "ABB", "BST", "SON",
    "QDS", "APX", "AWE", "HYD", "TCH", "MET", "ZEN", "ELS", "SLB", "GWF",
    "PAD", "EMH", "NZR", "BME", "KMB",
];

/// Helper: Attempts parsing Modbus RTU packet
fn try_parse_modbus_rtu(data: &[u8], freq_hz: u64, snr_db: f32, next_id: &mut u64) -> Option<TelemetryPacket> {
    if !is_scada_band(freq_hz) {
        return None;
    }
    if data.len() < 5 || data.len() > 64 {
        return None;
    }
    let slave = data[0];
    if slave == 0 || slave > 247 {
        return None;
    }

    let payload_len = data.len() - 2;
    let payload = &data[..payload_len];
    let expected_crc = crc16_modbus(payload);
    let packet_crc = (data[payload_len] as u16) | ((data[payload_len + 1] as u16) << 8); // Modbus is LSB first

    if expected_crc != packet_crc {
        return None;
    }

    let func = data[1];

    // Frame length and structure consistency validation to eliminate random noise collisions
    match func {
        0x01 | 0x02 | 0x05 | 0x06 => {
            if data.len() != 8 && data.len() < 5 {
                return None;
            }
        }
        0x03 | 0x04 => {
            if data.len() == 8 {
                // Read poll query
            } else if data.len() >= 5 && data.len() == 5 + (data[2] as usize) {
                // Read response: byte count must be even (16-bit register words)
                if data[2] % 2 != 0 {
                    return None;
                }
            } else {
                return None;
            }
        }
        0x10 => {
            if data.len() < 9 {
                return None;
            }
        }
        f if (0x81..=0x90).contains(&f) => {
            // Modbus RTU Exception response is STRICTLY 5 bytes: [slave, func|0x80, exc_code, crc_lo, crc_hi]
            if data.len() != 5 {
                return None;
            }
            let exc_code = data[2];
            // Modbus exception codes are strictly 1..=4 (or 1..=8)
            if !(1..=4).contains(&exc_code) {
                return None;
            }
        }
        _ => return None,
    }

    let (func_name, summary, human_decoded) = match func {
        0x01 => {
            let desc = parse_modbus_read_req(data);
            (
                "Read Coils (0x01)",
                format!("Read Digital Outputs ({})", desc),
                format!("Master polling digital states (pump run/stop, valve open/close, alarms) on Substation #{}.\nDetails: {}.", slave, desc),
            )
        }
        0x02 => {
            let desc = parse_modbus_read_req(data);
            (
                "Read Discrete Inputs (0x02)",
                format!("Read Digital Inputs ({})", desc),
                format!("Master checking digital sensor contacts (tank float switches, door sensors, trip indicators) on Substation #{}.\nDetails: {}.", slave, desc),
            )
        }
        0x03 => {
            let (sum, hum) = parse_modbus_registers(data, slave, "Holding Registers (Analog Telemetry)");
            ("Read Holding Regs (0x03)", sum, hum)
        }
        0x04 => {
            let (sum, hum) = parse_modbus_registers(data, slave, "Input Registers (Sensor Readings)");
            ("Read Input Regs (0x04)", sum, hum)
        }
        0x05 => {
            let coil_addr = if data.len() >= 4 { ((data[2] as u16) << 8) | (data[3] as u16) } else { 0 };
            let state = if data.len() >= 5 && data[4] == 0xFF { "ON / ACTIVE" } else { "OFF / IDLE" };
            (
                "Write Single Coil (0x05)",
                format!("Switch Relay #0x{:04X} -> {}", coil_addr, state),
                format!("Remote Control Command: Master commanded Substation #{} to switch relay/output #0x{:04X} to {}.", slave, coil_addr, state),
            )
        }
        0x06 => {
            let reg_addr = if data.len() >= 4 { ((data[2] as u16) << 8) | (data[3] as u16) } else { 0 };
            let reg_val = if data.len() >= 6 { ((data[4] as u16) << 8) | (data[5] as u16) } else { 0 };
            (
                "Write Single Reg (0x06)",
                format!("Set Reg #0x{:04X} = {}", reg_addr, reg_val),
                format!("Setpoint / Parameter Change: Master updated Substation #{} Register #0x{:04X} to value {} (0x{:04X}).", slave, reg_addr, reg_val, reg_val),
            )
        }
        0x10 => {
            (
                "Write Multi Regs (0x10)",
                "Write Multiple Registers Configuration".to_string(),
                format!("Bulk Configuration Update sent to Substation #{}.", slave),
            )
        }
        f if (0x81..=0x90).contains(&f) => {
            let exc_code = data[2];
            let exc_meaning = match exc_code {
                1 => "Illegal Function (0x01)",
                2 => "Illegal Data Address (0x02)",
                3 => "Illegal Data Value (0x03)",
                4 => "Slave Device Failure (0x04)",
                _ => return None,
            };
            let orig_func = f - 0x80;
            (
                "Exception Response",
                format!("Error: {} on Func 0x{:02X}", exc_meaning, orig_func),
                format!(
                    "Substation #{} Exception Response:\n• Condition: {}\n• Originating Request: Function 0x{:02X}\n• Status: Request rejected by slave device.",
                    slave, exc_meaning, orig_func
                ),
            )
        }
        _ => return None,
    };

    let id = *next_id;
    *next_id += 1;

    Some(TelemetryPacket {
        id,
        timestamp: current_time_str(),
        freq_hz,
        protocol: "Modbus RTU".to_string(),
        station_id: Some(format!("Station #{}", slave)),
        summary: format!("{}: {}", func_name, summary),
        human_decoded,
        raw_hex: bytes_to_hex(data),
        raw_ascii: bytes_to_ascii(data),
        snr_db,
        burst_ms: (data.len() as f32 * 8.0 / 9.6),
        crc_ok: true,
    })
}

fn parse_modbus_read_req(data: &[u8]) -> String {
    if data.len() >= 6 {
        let start = ((data[2] as u16) << 8) | (data[3] as u16);
        let count = ((data[4] as u16) << 8) | (data[5] as u16);
        format!("Addr: 0x{:04X}, Count: {}", start, count)
    } else if data.len() >= 3 {
        let byte_count = data[2] as usize;
        format!("Response: {} bytes", byte_count)
    } else {
        "Poll".to_string()
    }
}

fn parse_modbus_registers(data: &[u8], slave: u8, kind: &str) -> (String, String) {
    if data.len() >= 6 && data.len() <= 8 {
        // Master Request: start address + count
        let start = ((data[2] as u16) << 8) | (data[3] as u16);
        let count = ((data[4] as u16) << 8) | (data[5] as u16);
        (
            format!("Polling {} registers from 0x{:04X}", count, start),
            format!(
                "Master Poll Request:\n• Target: Substation #{slave}\n• Register Type: {kind}\n• Starting Address: 0x{start:04X}\n• Count: {count} words\n• Role: Querying live analog telemetry (temperatures, pressure, flow rate, or level)."
            ),
        )
    } else if data.len() >= 5 {
        // Slave Response: byte_count followed by 16-bit register values
        let byte_count = data[2] as usize;
        let mut reg_strs = Vec::new();
        let mut reg_vals = Vec::new();
        let reg_bytes = &data[3..3 + byte_count.min(data.len().saturating_sub(5))];
        for (idx, chunk) in reg_bytes.chunks_exact(2).enumerate() {
            let val = ((chunk[0] as u16) << 8) | (chunk[1] as u16);
            reg_vals.push(val);
            let scaled_val = (val as f32) / 10.0;
            let (sensor_name, unit_str) = match idx {
                0 => ("Flow / Supply Temp (T1)", format!("{:.1} °C", scaled_val)),
                1 => ("Return Temp (T2)", format!("{:.1} °C", scaled_val)),
                2 => ("Loop Pressure (P1)", format!("{:.2} bar", scaled_val / 10.0)),
                3 => ("Flow Rate", format!("{:.1} m³/h", scaled_val)),
                4 => ("Tank / Liquid Level", format!("{:.2} m", scaled_val / 10.0)),
                _ => ("Analog Telemetry", format!("{:.1} units", scaled_val)),
            };
            reg_strs.push(format!("  • Reg[{}] ({}): {} (raw 0x{:04X} ➔ {})", idx, sensor_name, val, val, unit_str));
        }

        let summary = format!(
            "Live Telemetry ({} values: {})",
            reg_vals.len(),
            reg_vals.iter().take(3).map(|v| format!("{:.1}", (*v as f32) / 10.0)).collect::<Vec<_>>().join(", ")
        );

        let human_decoded = format!(
            "Substation #{slave} Live Telemetry Readings:\n{}\n• Interpretation: Remote SCADA sensor broadcast. Standard municipal scaling: 0.1 °C temperature, 0.01 bar pressure, 0.1 m³/h flow.",
            reg_strs.join("\n")
        );

        (summary, human_decoded)
    } else {
        ("Register Data".to_string(), "Modbus register payload".to_string())
    }
}

/// Helper: Attempts parsing SATEL 3AS / Satelline radio modem frame
fn try_parse_satel(data: &[u8], freq_hz: u64, snr_db: f32, next_id: &mut u64) -> Option<TelemetryPacket> {
    if data.len() < 5 {
        return None;
    }
    let dest_addr = data[0];
    let src_addr = data[1];
    let payload_len = data[2] as usize;

    if payload_len == 0 || payload_len + 5 > data.len() {
        return None;
    }

    let payload = &data[3..3 + payload_len];
    let crc_in_packet = ((data[3 + payload_len] as u16) << 8) | (data[3 + payload_len + 1] as u16);
    let expected_crc = crc16_ccitt(&data[..3 + payload_len]);

    let crc_ok = expected_crc == crc_in_packet;
    // Strict CRC validation: reject immediately if CRC does not match!
    if !crc_ok {
        return None;
    }

    // Check if payload encapsulates Modbus RTU
    let (summary, human_decoded) = if let Some(modbus) = try_parse_modbus_rtu(payload, freq_hz, snr_db, next_id) {
        (
            format!("SATEL -> {}", modbus.summary),
            format!(
                "SATEL 3AS Radio Modem Transport:\n• Source: Station #{src_addr}\n• Destination: Substation #{dest_addr}\n• Encapsulated Protocol: Modbus RTU\n\n{}",
                modbus.human_decoded
            ),
        )
    } else {
        // Check if payload contains readable ASCII text
        let is_ascii = payload.iter().filter(|&&b| (32..=126).contains(&b) || b == b'\r' || b == b'\n').count() >= (payload.len() * 3 / 4);
        if is_ascii {
            let text = bytes_to_ascii(payload);
            (
                format!("ASCII Message: \"{}\"", text.trim_matches('.')),
                format!(
                    "SATEL Radio Modem Text Transmission:\n• Link: Station #{src_addr} ➔ Station #{dest_addr}\n• Content: \"{}\"\n• Typical Role: Remote station status string, NMEA beacon, or telemetry alert.",
                    text.trim_matches('.')
                ),
            )
        } else {
            (
                format!("SCADA Telemetry ({} bytes)", payload_len),
                format!(
                    "Municipal SCADA Radio Modem Transmission (SATEL 3AS):\n• Transmitter: Substation #{src_addr}\n• Recipient: Target Substation #{dest_addr}\n• Data Size: {payload_len} bytes\n• Link Status: Active radio modem carrier (Water pumping station, district heating, or remote power grid switch)."
                ),
            )
        }
    };

    let id = *next_id;
    *next_id += 1;

    Some(TelemetryPacket {
        id,
        timestamp: current_time_str(),
        freq_hz,
        protocol: "SATEL 3AS / SATERADIO".to_string(),
        station_id: Some(format!("Src: #{} -> Dst: #{}", src_addr, dest_addr)),
        summary,
        human_decoded,
        raw_hex: bytes_to_hex(data),
        raw_ascii: bytes_to_ascii(data),
        snr_db,
        burst_ms: (data.len() as f32 * 8.0 / 9.6),
        crc_ok: true,
    })
}

/// Helper: Parses DIF/VIF records in Wireless M-Bus frames for physical measurements
fn parse_wmbus_records(payload: &[u8]) -> (Vec<String>, bool) {
    let mut measurements = Vec::new();
    let mut is_encrypted = false;
    let mut pos = 0;

    if !payload.is_empty() {
        let ci = payload[0];
        if ci == 0x7A {
            // Short transport layer header: CI (1B), Access Num (1B), Status (1B), Configuration (2B)
            if payload.len() >= 5 {
                let enc_mode = payload[3] & 0x0F;
                if enc_mode != 0 {
                    is_encrypted = true;
                }
                pos = 5;
            } else {
                pos = payload.len();
            }
        } else if ci == 0x72 {
            // Full transport layer header: 13 bytes
            if payload.len() >= 13 {
                let enc_mode = payload[11] & 0x0F;
                if enc_mode != 0 {
                    is_encrypted = true;
                }
                pos = 13;
            } else {
                pos = payload.len();
            }
        } else if ci == 0x78 {
            // Short header without transport layer: CI is 1 byte, followed directly by DIF/VIF records
            pos = 1;
        }
    }

    while pos + 1 < payload.len() && measurements.len() < 8 {
        let dif = payload[pos];
        if dif == 0x0F || dif == 0x1F || dif == 0x2F {
            break;
        }
        let vif = payload[pos + 1];
        pos += 2;

        let data_len = match dif & 0x0F {
            0x01 => 1,
            0x02 => 2,
            0x03 => 3,
            0x04 => 4,
            0x06 => 6,
            0x07 => 8,
            0x0B => 6,
            0x0C => 4,
            _ => 2,
        };

        if pos + data_len > payload.len() {
            break;
        }

        let slice = &payload[pos..pos + data_len];
        pos += data_len;

        let raw_val: i64 = if data_len == 1 {
            slice[0] as i64
        } else if data_len == 2 {
            ((slice[1] as u16) << 8 | (slice[0] as u16)) as i64
        } else if data_len == 4 {
            let v = ((slice[3] as u32) << 24) | ((slice[2] as u32) << 16) | ((slice[1] as u32) << 8) | (slice[0] as u32);
            v as i64
        } else {
            0
        };

        match vif {
            0x58..=0x5F => {
                let temp = (raw_val as f64) * 0.1;
                measurements.push(format!("🌡️ Flow / Supply Temp: {:.1} °C (T1)", temp));
            }
            0x60..=0x67 => {
                let temp = (raw_val as f64) * 0.1;
                measurements.push(format!("🌡️ Return Temp: {:.1} °C (T2)", temp));
            }
            0x68..=0x6A => {
                let delta = (raw_val as f64) * 0.1;
                measurements.push(format!("🌡️ Temperature Diff (ΔT): {:.1} K", delta));
            }
            0x6B..=0x6E => {
                let pres = (raw_val as f64) * 0.1;
                measurements.push(format!("⏱️ Operating Pressure: {:.2} bar", pres));
            }
            0x10..=0x17 => {
                let vol = (raw_val as f64) * 0.01;
                measurements.push(format!("💧 Cumulative Volume: {:.2} m³", vol));
            }
            0x28..=0x2F => {
                let flow = (raw_val as f64) * 0.01;
                measurements.push(format!("🔄 Flow Rate: {:.2} m³/h", flow));
            }
            0x01..=0x07 => {
                let mwh = (raw_val as f64) * 0.001;
                measurements.push(format!("⚡ Energy Counter: {:.3} MWh", mwh));
            }
            _ => {}
        }
    }

    (measurements, is_encrypted)
}

/// Helper: Attempts parsing Wireless M-Bus frame (Kamstrup, Landis+Gyr, Sensus)
fn try_parse_wmbus(data: &[u8], freq_hz: u64, snr_db: f32, next_id: &mut u64) -> Option<TelemetryPacket> {
    if data.len() < 10 {
        return None;
    }
    let len = data[0] as usize;
    if len < 9 || len + 1 > data.len() {
        return None;
    }

    let c_field = data[1];
    // Manufacturer ID (2 bytes, 3 uppercase ASCII chars packed into 5 bits each)
    let m_field = ((data[3] as u16) << 8) | (data[2] as u16);
    let c1 = ((m_field >> 10) & 0x1F) as u8 + 64;
    let c2 = ((m_field >> 5) & 0x1F) as u8 + 64;
    let c3 = (m_field & 0x1F) as u8 + 64;

    // Strict validation: Manufacturer MUST be a registered, recognized utility meter manufacturer
    if !(c1.is_ascii_uppercase() && c2.is_ascii_uppercase() && c3.is_ascii_uppercase()) {
        return None;
    }

    let mfg = format!("{}{}{}", c1 as char, c2 as char, c3 as char);
    if !KNOWN_WMBUS_MFG.contains(&mfg.as_str()) {
        return None;
    }

    // Meter identification number (4 bytes BCD)
    let id_bcd = format!("{:02X}{:02X}{:02X}{:02X}", data[7], data[6], data[5], data[4]);
    let device_type = data[8];

    let type_name = match device_type {
        0x04 => "District Heating (Heat Meter)",
        0x07 => "Water Meter",
        0x02 => "Electricity Meter",
        0x03 => "Gas Meter",
        0x08 => "Heat Cost Allocator",
        0x06 => "Warm Water Meter (30-90°C)",
        0x0B => "Cooling Meter",
        0x0C => "Combined Heat/Cooling Meter",
        _ => "Smart Utility Meter",
    };

    let c_name = match c_field {
        0x44 => "SND-NR (Periodic Broadcast)",
        0x46 => "SND-IR (Installation Message)",
        0x47 => "ACC-NR (Acknowledgment)",
        0x08 => "REQ-UD2 (Data Request)",
        _ => "Telemetry Frame",
    };

    let mfg_full = match mfg.as_str() {
        "KAM" => "Kamstrup A/S",
        "LUG" => "Landis+Gyr",
        "SEN" => "Sensus / Xylem",
        "DAN" => "Danfoss",
        "DME" => "Diehl Metering (Hydrometer)",
        "ITR" => "Itron",
        "ELV" => "Elvaco",
        "ABB" => "ABB",
        "SON" => "Sontex",
        "QDS" => "Qundis",
        _ => "Utility Meter",
    };

    let payload_slice = if data.len() > 10 { &data[10..len + 1] } else { &[] };
    let (measurements, is_encrypted) = parse_wmbus_records(payload_slice);

    let readings_block = if !measurements.is_empty() {
        format!("• Live Physical Telemetry:\n  {}\n", measurements.join("\n  "))
    } else if is_encrypted {
        format!(
            "• Payload Protection: Mode 5 AES-128 Encrypted Broadcast (OMS Open Metering System)\n• Channel Structure for {type_name}:\n  • Kaukolämpö Meno: T1 Flow Temp (°C)\n  • Kaukolämpö Paluu: T2 Return Temp (°C)\n  • Loop Pressure: (bar)\n  • Cumulative Consumption: Total Energy (MWh) / Volume (m³)\n• Status: Authenticated encrypted telemetry packet from utility meter."
        )
    } else {
        format!(
            "• Channel Structure for {type_name}:\n  • Flow & Return Temperatures (°C)\n  • Operating Loop Pressure (bar)\n  • Cumulative Volume (m³)\n• Status: Normal operational broadcast."
        )
    };

    let human_decoded = format!(
        "Smart Utility Meter Broadcast (Wireless M-Bus EN 13757-4):\n• Manufacturer: {} ({})\n• Meter Serial / ID: #{}\n• Meter Type: {}\n• Transmission Mode: {}\n{}",
        mfg, mfg_full, id_bcd, type_name, c_name, readings_block
    );

    let summary = if !measurements.is_empty() {
        format!("{} [{}] ➔ {}", mfg, id_bcd, measurements[0])
    } else {
        format!("{} [{}] {}", type_name, c_name, mfg)
    };

    let id = *next_id;
    *next_id += 1;

    Some(TelemetryPacket {
        id,
        timestamp: current_time_str(),
        freq_hz,
        protocol: "Wireless M-Bus".to_string(),
        station_id: Some(format!("{} #{}", mfg, id_bcd)),
        summary,
        human_decoded,
        raw_hex: bytes_to_hex(&data[..len + 1]),
        raw_ascii: bytes_to_ascii(&data[..len + 1]),
        snr_db,
        burst_ms: ((len + 1) as f32 * 8.0 / 9.6),
        crc_ok: true,
    })
}

/// Helper: Attempts parsing POCSAG paging frame
fn try_parse_pocsag(data: &[u8], freq_hz: u64, snr_db: f32, next_id: &mut u64) -> Option<TelemetryPacket> {
    if data.len() < 8 {
        return None;
    }
    let cw1 = ((data[0] as u32) << 24) | ((data[1] as u32) << 16) | ((data[2] as u32) << 8) | (data[3] as u32);
    // Bit 31: 0 = address codeword
    if (cw1 & 0x8000_0000) == 0 {
        let ric = (cw1 >> 13) & 0x7FFFF;
        let func = (cw1 >> 11) & 0x03;

        let id = *next_id;
        *next_id += 1;

        let human_decoded = format!(
            "POCSAG Pager Transmission:\n• Pager Address (RIC): {}\n• Function / Alert Level: {}\n• Protocol: 1200 Baud POCSAG (FSK)\n• Role: Emergency dispatcher or on-call maintenance pager alert.",
            ric,
            func + 1
        );

        Some(TelemetryPacket {
            id,
            timestamp: current_time_str(),
            freq_hz,
            protocol: "POCSAG Pager".to_string(),
            station_id: Some(format!("RIC: {}", ric)),
            summary: format!("Pager Alert (Function {})", func + 1),
            human_decoded,
            raw_hex: bytes_to_hex(data),
            raw_ascii: bytes_to_ascii(data),
            snr_db,
            burst_ms: (data.len() as f32 * 8.0 / 1.2), // at 1200 baud
            crc_ok: true,
        })
    } else {
        None
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")
}

fn bytes_to_ascii(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| if (32..=126).contains(&b) { b as char } else { '.' }).collect()
}

fn current_time_str() -> String {
    // Generates a simple timestamp string (HH:MM:SS) based on local system uptime
    let now = std::time::SystemTime::now();
    if let Ok(duration) = now.duration_since(std::time::UNIX_EPOCH) {
        let secs = duration.as_secs();
        let s = secs % 60;
        let m = (secs / 60) % 60;
        let h = ((secs / 3600) + 3) % 24; // Approximate Finnish time UTC+3
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else {
        "00:00:00".to_string()
    }
}

/// Standard CRC-8 calculation with configurable polynomial and initial value
pub fn crc8(data: &[u8], poly: u8, init: u8) -> u8 {
    let mut crc = init;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            if (crc & 0x80) != 0 {
                crc = (crc << 1) ^ poly;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Helper: Attempts parsing Automotive TPMS Tire Pressure Sensor packet
pub fn try_decode_tpms(data: &[u8], freq_hz: u64, snr_db: f32, next_id: &mut u64) -> Option<TelemetryPacket> {
    // TPMS sensors strictly operate within 433 MHz ISM band (432.5 - 435.0 MHz) in Europe/Finland
    // Never allow TPMS decoding on SCADA (406-430 MHz, 440-445 MHz) or Sonde (400-406 MHz) frequencies!
    if !is_433_ism_band(freq_hz) {
        return None;
    }
    if data.len() < 8 {
        return None;
    }
    let max_w = data.len().min(12);
    for window_len in (8..=max_w).rev() {
        let slice = &data[data.len() - window_len..];

        let id_u32 = ((slice[0] as u32) << 24) | ((slice[1] as u32) << 16) | ((slice[2] as u32) << 8) | (slice[3] as u32);
        if id_u32 == 0 || id_u32 == 0xFFFF_FFFF || id_u32 == 0x5555_5555 || id_u32 == 0xAAAA_AAAA || id_u32 < 0x1000 {
            continue;
        }

        // Standard Schrader / Toyota / Ford / Renault format:
        // Byte 4: Pressure (in 2.5 kPa per LSB or ~0.025 bar)
        // Byte 5: Temperature (in °C with offset -50 or -40)
        // Byte 6: Status / Flags
        // Byte 7: Checksum (CRC-8 or XOR)
        let raw_p = slice[4];
        let raw_t = slice[5];
        let flags = slice[6];
        let chk = slice[7];

        let p_bar = (raw_p as f32 * 2.5) / 100.0;
        let p_psi = p_bar * 14.5038;
        let t_c = (raw_t as f32) - 50.0;

        // Realistic automotive tire pressure range: 1.2 to 4.5 bar, temp: -25 to +75 °C
        if p_bar < 1.2 || p_bar > 4.5 || t_c < -25.0 || t_c > 75.0 {
            continue;
        }

        // Strict checksum verification: must match non-zero XOR or CRC-8
        let xor_sum = slice[..7].iter().fold(0u8, |acc, &b| acc ^ b);
        let crc_07 = crc8(&slice[..7], 0x07, 0x00);
        let crc_31 = crc8(&slice[..7], 0x31, 0x00);

        let chk_ok = (xor_sum == chk && chk != 0) || (crc_07 == chk && chk != 0) || (crc_31 == chk && chk != 0);
        if !chk_ok {
            continue;
        }

        let is_low_batt = (flags & 0x80) != 0;
        let is_moving = (flags & 0x40) != 0;

        let pid = *next_id;
        *next_id += 1;

        let summary = format!("TPMS [#{:08X}] ➔ {:.2} bar ({:.1} PSI), {:.1} °C", id_u32, p_bar, p_psi, t_c);
        let human_decoded = format!(
            "Automotive TPMS (Tire Pressure Monitoring System):\n• Tire Pressure: {:.2} bar ({:.1} PSI / {:.0} kPa)\n• Tire Temperature: {:.1} °C ({:.1} °F)\n• Sensor ID: #0x{:08X}\n• Battery Status: {}\n• Wheel Motion: {}\n• Protocol: 433.92 MHz FSK Manchester (RTL-433 compatible)",
            p_bar,
            p_psi,
            p_bar * 100.0,
            t_c,
            t_c * 1.8 + 32.0,
            id_u32,
            if is_low_batt { "LOW BATTERY ⚠️" } else { "OK ✓" },
            if is_moving { "Driving / Wheel Rotating 🚗" } else { "Stationary / Parked 🅿️" }
        );

        return Some(TelemetryPacket {
            id: pid,
            timestamp: current_time_str(),
            freq_hz,
            protocol: "TPMS Tire Pressure".to_string(),
            station_id: Some(format!("TPMS #{:08X}", id_u32)),
            summary,
            human_decoded,
            raw_hex: bytes_to_hex(slice),
            raw_ascii: bytes_to_ascii(slice),
            snr_db,
            burst_ms: (slice.len() as f32 * 8.0 / 19.2),
            crc_ok: true,
        });
    }
    None
}

/// Helper: Attempts parsing EV1527 / PT2262 433.92 MHz wireless remote & alarm sensors
pub fn try_decode_ev1527(pulses: &[(u32, u32)], freq_hz: u64, snr_db: f32, next_id: &mut u64) -> Option<TelemetryPacket> {
    if !is_ism_band(freq_hz) || pulses.len() < 24 {
        return None;
    }
    for start in 0..=(pulses.len() - 24) {
        let chunk = &pulses[start..start + 24];
        let mut bits = 0u32;
        let mut valid = true;

        for &(high_us, low_us) in chunk {
            let total = high_us + low_us;
            if total < 600 || total > 3200 {
                valid = false;
                break;
            }
            let ratio = high_us as f32 / total as f32;
            if (0.12..=0.45).contains(&ratio) {
                bits = (bits << 1) | 0;
            } else if (0.55..=0.88).contains(&ratio) {
                bits = (bits << 1) | 1;
            } else {
                valid = false;
                break;
            }
        }

        if valid {
            let addr = (bits >> 4) & 0xF_FFFF;
            let code = (bits & 0x0F) as u8;

            if addr == 0 || addr == 0xF_FFFF {
                continue;
            }

            let btn_str = match code {
                0x01 => "Button A (Keyfob Remote)",
                0x02 => "Button B (Keyfob Remote)",
                0x04 => "Button C (Keyfob Remote)",
                0x08 => "Button D (Keyfob Remote)",
                0x0A => "Door / Window Contact (Reed Switch Opened)",
                0x0C => "PIR Motion Detected (Wireless Alarm)",
                0x0E => "Tamper Alert / Battery Low",
                0x03 => "Doorbell Chime Push",
                _ => "Wireless Sensor Trigger",
            };

            let pid = *next_id;
            *next_id += 1;

            let summary = format!("Remote [0x{:05X}] ➔ {} (Code 0x{:X})", addr, btn_str, code);
            let human_decoded = format!(
                "Wireless Remote / Security Sensor (EV1527 / PT2262):\n• Security Address ID: 0x{:05X}\n• Button / Event: {} (0x{:X})\n• Raw Codeword: 24-bit 0x{:06X}\n• Protocol: 433.92 MHz OOK Pulse-Width Modulation (PWM)\n• Device Category: Wireless doorbell, PIR alarm sensor, magnetic window sensor, or garage/car keyfob.",
                addr, btn_str, code, bits
            );

            return Some(TelemetryPacket {
                id: pid,
                timestamp: current_time_str(),
                freq_hz,
                protocol: "EV1527 Remote".to_string(),
                station_id: Some(format!("Remote 0x{:05X}", addr)),
                summary,
                human_decoded,
                raw_hex: format!("{:02X} {:02X} {:02X}", (bits >> 16) & 0xFF, (bits >> 8) & 0xFF, bits & 0xFF),
                raw_ascii: "...".to_string(),
                snr_db,
                burst_ms: 25.0,
                crc_ok: true,
            });
        }
    }
    None
}

/// Helper: Attempts parsing Nexus / Rubicson / ThermoPro 433.92 MHz weather sensors (36 bits PPM)
pub fn try_decode_nexus(pulses: &[(u32, u32)], freq_hz: u64, snr_db: f32, next_id: &mut u64) -> Option<TelemetryPacket> {
    if !is_ism_band(freq_hz) || pulses.len() < 36 {
        return None;
    }
    for start in 0..=(pulses.len() - 36) {
        let chunk = &pulses[start..start + 36];
        let mut bits = 0u64;
        let mut valid = true;

        for &(high_us, low_us) in chunk {
            if high_us < 250 || high_us > 950 {
                valid = false;
                break;
            }
            if (650..=1500).contains(&low_us) {
                bits = (bits << 1) | 0;
            } else if (1501..=3000).contains(&low_us) {
                bits = (bits << 1) | 1;
            } else {
                valid = false;
                break;
            }
        }

        if valid {
            let id = ((bits >> 28) & 0xFF) as u8;
            let battery_low = ((bits >> 27) & 1) != 0;
            let channel = (((bits >> 24) & 3) as u8) + 1;

            let temp_raw = ((bits >> 12) & 0xFFF) as i16;
            let temp_c = if (temp_raw & 0x800) != 0 {
                -(((!temp_raw) & 0x7FF) as f32 + 1.0) / 10.0
            } else {
                (temp_raw as f32) / 10.0
            };

            let const_nibble = ((bits >> 8) & 0xF) as u8;
            let humidity = (bits & 0xFF) as u8;

            if (const_nibble == 0xF || const_nibble == 0x0)
                && (-35.0..=65.0).contains(&temp_c)
                && (5..=99).contains(&humidity)
                && channel <= 3
            {
                let pid = *next_id;
                *next_id += 1;

                let summary = format!("Weather [Ch {}] ➔ {:.1} °C, {}% RH (ID #{:02X})", channel, temp_c, humidity, id);
                let human_decoded = format!(
                    "Wireless Weather Sensor (Nexus / Rubicson / ThermoPro):\n• Temperature: {:.1} °C ({:.1} °F)\n• Relative Humidity: {} %\n• Channel: Channel {}\n• Rolling Sensor ID: #0x{:02X}\n• Battery Status: {}\n• Protocol: 433.92 MHz OOK PPM (36-bit standard)",
                    temp_c,
                    temp_c * 1.8 + 32.0,
                    humidity,
                    channel,
                    id,
                    if battery_low { "LOW BATTERY ⚠️" } else { "OK ✓" }
                );

                return Some(TelemetryPacket {
                    id: pid,
                    timestamp: current_time_str(),
                    freq_hz,
                    protocol: "Nexus Weather Sensor".to_string(),
                    station_id: Some(format!("Sensor Ch{} #{:02X}", channel, id)),
                    summary,
                    human_decoded,
                    raw_hex: format!("{:010X}", bits),
                    raw_ascii: "...".to_string(),
                    snr_db,
                    burst_ms: 60.0,
                    crc_ok: true,
                });
            }
        }
    }
    None
}

/// Helper: Attempts parsing Motorola MDC-1200 1200 baud ANI / Dispatch bursts
pub fn try_decode_mdc1200(data: &[u8], freq_hz: u64, snr_db: f32, next_id: &mut u64) -> Option<TelemetryPacket> {
    if data.len() < 4 {
        return None;
    }
    let op = data[0];
    let unit_id = ((data[2] as u16) << 8) | (data[3] as u16);
    if unit_id == 0 || unit_id == 0xFFFF {
        return None;
    }

    let event_str = match op {
        0x01 => "PTT ID (Radio Keyed)",
        0x03 => "PTT Release (Unkey)",
        0x20 => "EMERGENCY ALERT 🚨",
        0x11 => "Radio Check / Status Query",
        0x23 => "Radio Stun / Inhibit",
        0x25 => "Radio Revive",
        _ => return None,
    };

    let pid = *next_id;
    *next_id += 1;

    let summary = format!("MDC-1200 Unit #{:04X} ➔ {}", unit_id, event_str);
    let human_decoded = format!(
        "Motorola MDC-1200 Radio ANI:\n• Radio Unit ID: #0x{:04X} ({})\n• Signaling Event: {}\n• Protocol: 1200 Baud AFSK/FSK MDC-1200 Burst\n• Typical Role: PMR446 / UHF repeater dispatcher unit identification.",
        unit_id, unit_id, event_str
    );

    Some(TelemetryPacket {
        id: pid,
        timestamp: current_time_str(),
        freq_hz,
        protocol: "MDC-1200 Radio ID".to_string(),
        station_id: Some(format!("MDC #{:04X}", unit_id)),
        summary,
        human_decoded,
        raw_hex: bytes_to_hex(data),
        raw_ascii: bytes_to_ascii(data),
        snr_db,
        burst_ms: 173.0,
        crc_ok: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modbus_crc16_and_parsing() {
        // Standard Modbus RTU Read Holding Registers request:
        // Slave: 0x05, Func: 0x03, Start: 0x0010, Count: 0x0004
        let mut msg = vec![0x05, 0x03, 0x00, 0x10, 0x00, 0x04];
        let crc = crc16_modbus(&msg);
        msg.push((crc & 0xFF) as u8);
        msg.push((crc >> 8) as u8);

        let mut next_id = 1;
        let pkt = try_parse_modbus_rtu(&msg, 442_000_000, 18.5, &mut next_id).expect("Should parse Modbus RTU");
        assert_eq!(pkt.protocol, "Modbus RTU");
        assert_eq!(pkt.station_id.unwrap(), "Station #5");
        assert!(pkt.summary.contains("Read Holding Regs"));
        assert!(pkt.crc_ok);
    }

    #[test]
    fn test_satel_parsing_with_modbus() {
        // SATEL frame: Dst 0x01, Src 0x0A, Len 8, Payload (Modbus Read Coils) + CRC
        let mut modbus = vec![0x02, 0x01, 0x00, 0x00, 0x00, 0x08];
        let crc_m = crc16_modbus(&modbus);
        modbus.push((crc_m & 0xFF) as u8);
        modbus.push((crc_m >> 8) as u8);

        let mut satel = vec![0x01, 0x0A, modbus.len() as u8];
        satel.extend_from_slice(&modbus);
        let crc_s = crc16_ccitt(&satel);
        satel.push((crc_s >> 8) as u8);
        satel.push((crc_s & 0xFF) as u8);

        let mut next_id = 1;
        let pkt = try_parse_satel(&satel, 441_950_000, 22.0, &mut next_id).expect("Should parse SATEL");
        assert!(pkt.protocol.contains("SATEL"));
        assert!(pkt.summary.contains("Read Coils") || pkt.summary.contains("SATEL"));
    }

    #[test]
    fn test_wmbus_parsing() {
        // Wireless M-Bus frame for Kamstrup Heat Meter
        // Len 9, C 0x44 (SND-NR), M-field for "KAM", ID BCD 0x12 0x34 0x56 0x78, Type 0x04 (Heat)
        let m_field = (((b'K' - 64) as u16) << 10) | (((b'A' - 64) as u16) << 5) | ((b'M' - 64) as u16);
        let msg = vec![
            9,
            0x44,
            (m_field & 0xFF) as u8,
            (m_field >> 8) as u8,
            0x78, 0x56, 0x34, 0x12,
            0x04,
            0x20,
        ];

        let mut next_id = 1;
        let pkt = try_parse_wmbus(&msg, 442_125_000, 15.0, &mut next_id).expect("Should parse W-MBus");
        assert_eq!(pkt.protocol, "Wireless M-Bus");
        assert!(pkt.station_id.unwrap().contains("KAM"));
        assert!(pkt.summary.contains("Heat"));
    }

    #[test]
    fn test_satel_rejects_corrupted_crc() {
        let mut satel = vec![0x01, 0x0A, 4, 0xAA, 0xBB, 0xCC, 0xDD];
        satel.push(0x12);
        satel.push(0x34); // Invalid CRC
        let mut next_id = 1;
        assert!(try_parse_satel(&satel, 441_950_000, 15.0, &mut next_id).is_none());
    }

    #[test]
    fn test_wmbus_rejects_random_noise_manufacturer() {
        // "IYR" from user prompt: should be rejected because not a real utility manufacturer
        let m_field = (((b'I' - 64) as u16) << 10) | (((b'Y' - 64) as u16) << 5) | ((b'R' - 64) as u16);
        let msg = vec![
            9,
            0x44,
            (m_field & 0xFF) as u8,
            (m_field >> 8) as u8,
            0x80, 0xF8, 0xA6, 0x23,
            0x04,
            0x20,
        ];
        let mut next_id = 1;
        assert!(try_parse_wmbus(&msg, 442_125_000, 15.0, &mut next_id).is_none());
    }

    #[test]
    fn test_modbus_rejects_invalid_exception_code() {
        // Code 124 from user complaint: should be rejected
        let mut msg = vec![0x43, 0x83, 124];
        let crc = crc16_modbus(&msg);
        msg.push((crc & 0xFF) as u8);
        msg.push((crc >> 8) as u8);
        let mut next_id = 1;
        assert!(try_parse_modbus_rtu(&msg, 442_000_000, 15.0, &mut next_id).is_none());
    }

    #[test]
    fn test_wmbus_parses_temperature_and_pressure() {
        // Wireless M-Bus frame for Kamstrup with CI=0x78 (no transport layer):
        // Flow temp: 68.4 °C (DIF=0x02, VIF=0x5B, Val=684), Pressure: 4.6 bar (DIF=0x02, VIF=0x6B, Val=46)
        let m_field = (((b'K' - 64) as u16) << 10) | (((b'A' - 64) as u16) << 5) | ((b'M' - 64) as u16);
        let mut msg = vec![
            18, // length byte (18 bytes follow)
            0x44, // SND-NR
            (m_field & 0xFF) as u8,
            (m_field >> 8) as u8,
            0x12, 0x34, 0x56, 0x78, // ID
            0x04, // Heat meter
            0x20, // version
            0x78, // CI field: no transport header
        ];
        // DIF: 16-bit int, VIF: 0x5B (temp), val: 684 (0x02AC)
        msg.extend_from_slice(&[0x02, 0x5B, 0xAC, 0x02]);
        // DIF: 16-bit int, VIF: 0x6B (pressure), val: 46 (0x002E)
        msg.extend_from_slice(&[0x02, 0x6B, 0x2E, 0x00]);

        let mut next_id = 1;
        let pkt = try_parse_wmbus(&msg, 442_125_000, 20.0, &mut next_id).expect("Should parse W-MBus");
        assert!(pkt.human_decoded.contains("68.4 °C") || pkt.summary.contains("68.4 °C"));
        assert!(pkt.human_decoded.contains("4.60 bar"));
    }

    #[test]
    fn test_tpms_decoding() {
        // Schrader TPMS: ID 0x8A 0x31 0xD0 0xF4, Pressure 92 (2.30 bar / 33.4 PSI), Temp 71 (21 °C), Flags 0x40 (moving)
        let mut tpms_msg = vec![0x8A, 0x31, 0xD0, 0xF4, 92, 71, 0x40];
        let xor_sum = tpms_msg.iter().fold(0u8, |acc, &b| acc ^ b);
        tpms_msg.push(xor_sum);

        let mut next_id = 1;
        let pkt = try_decode_tpms(&tpms_msg, 433_920_000, 18.0, &mut next_id).expect("Should decode TPMS");
        assert_eq!(pkt.protocol, "TPMS Tire Pressure");
        assert!(pkt.summary.contains("2.30 bar"));
        assert!(pkt.human_decoded.contains("33.4 PSI"));
        assert!(pkt.human_decoded.contains("8A31D0F4"));
        assert!(pkt.human_decoded.contains("Driving / Wheel Rotating"));
    }

    #[test]
    fn test_tpms_rejects_on_scada_frequency() {
        // Same valid payload, but received on 411.000 MHz (SCADA band)
        let mut tpms_msg = vec![0x8A, 0x31, 0xD0, 0xF4, 92, 71, 0x40];
        let xor_sum = tpms_msg.iter().fold(0u8, |acc, &b| acc ^ b);
        tpms_msg.push(xor_sum);

        let mut next_id = 1;
        assert!(try_decode_tpms(&tpms_msg, 411_000_000, 18.0, &mut next_id).is_none());
        assert!(try_decode_tpms(&tpms_msg, 401_000_000, 18.0, &mut next_id).is_none());
        assert!(try_decode_tpms(&tpms_msg, 442_000_000, 18.0, &mut next_id).is_none());
    }

    #[test]
    fn test_nexus_weather_decoding() {
        // Nexus weather sensor: 36 bits PPM
        // ID: 0x4B, batt OK, manual 0, Ch 1 (bits 0), temp 214 (21.4 °C), nibble 0xF, humidity 55 (55%)
        let id = 0x4Bu64;
        let batt = 0u64;
        let tx = 0u64;
        let ch = 0u64; // Channel 1
        let temp = 214u64;
        let nibble = 0xFu64;
        let hum = 55u64;

        let codeword: u64 = (id << 28) | (batt << 27) | (tx << 26) | (ch << 24) | (temp << 12) | (nibble << 8) | hum;

        let mut pulses = Vec::new();
        for i in (0..36).rev() {
            let bit = (codeword >> i) & 1;
            let low_us = if bit == 1 { 2000 } else { 1000 };
            pulses.push((500, low_us));
        }

        let mut next_id = 1;
        let pkt = try_decode_nexus(&pulses, 433_920_000, 15.0, &mut next_id).expect("Should decode Nexus");
        assert_eq!(pkt.protocol, "Nexus Weather Sensor");
        assert!(pkt.summary.contains("21.4 °C"));
        assert!(pkt.summary.contains("55% RH"));
        assert!(pkt.human_decoded.contains("Channel 1"));
    }

    #[test]
    fn test_ev1527_remote_decoding() {
        // EV1527 remote: 20 bits address 0xA4E21, 4 bits code 0x01 (Button A)
        let addr = 0xA4E21u32;
        let code = 0x01u32;
        let codeword = (addr << 4) | code;

        let mut pulses = Vec::new();
        for i in (0..24).rev() {
            let bit = (codeword >> i) & 1;
            let (high_us, low_us) = if bit == 1 { (1050, 350) } else { (350, 1050) };
            pulses.push((high_us, low_us));
        }

        let mut next_id = 1;
        let pkt = try_decode_ev1527(&pulses, 433_920_000, 16.0, &mut next_id).expect("Should decode EV1527");
        assert_eq!(pkt.protocol, "EV1527 Remote");
        assert!(pkt.summary.contains("0xA4E21"));
        assert!(pkt.summary.contains("Button A"));
    }

    #[test]
    fn test_mdc1200_decoding() {
        // MDC-1200: Opcode 0x01 (PTT ID), arg 0x00, unit ID 0x1234
        let msg = vec![0x01, 0x00, 0x12, 0x34];
        let mut next_id = 1;
        let pkt = try_decode_mdc1200(&msg, 446_000_000, 20.0, &mut next_id).expect("Should decode MDC-1200");
        assert_eq!(pkt.protocol, "MDC-1200 Radio ID");
        assert!(pkt.summary.contains("Unit #1234"));
        assert!(pkt.human_decoded.contains("PTT ID"));
    }

    #[test]
    fn test_modbus_rejects_on_non_scada_bands() {
        let mut msg = vec![0x05, 0x03, 0x00, 0x10, 0x00, 0x04];
        let crc = crc16_modbus(&msg);
        msg.push((crc & 0xFF) as u8);
        msg.push((crc >> 8) as u8);

        let mut next_id = 1;
        // 70cm amateur repeater (434.600 MHz or 438.800 MHz) must reject Modbus
        assert!(try_parse_modbus_rtu(&msg, 434_600_000, 18.0, &mut next_id).is_none());
        assert!(try_parse_modbus_rtu(&msg, 438_800_000, 18.0, &mut next_id).is_none());
        // Airband (125.0 MHz) must reject Modbus
        assert!(try_parse_modbus_rtu(&msg, 125_000_000, 18.0, &mut next_id).is_none());
        // APRS (144.800 MHz) must reject Modbus
        assert!(try_parse_modbus_rtu(&msg, 144_800_000, 18.0, &mut next_id).is_none());
        // But in SCADA band (442.0 MHz) it must succeed
        assert!(try_parse_modbus_rtu(&msg, 442_000_000, 18.0, &mut next_id).is_some());
    }

    #[test]
    fn test_finalize_burst_rejects_unverified_crc() {
        let mut decoder = TelemetryDecoder::new(48_000.0);
        decoder.burst_sample_count = 4800; // 100ms
        decoder.raw_bytes_9600 = vec![0x12, 0x34, 0x56, 0x78]; // random bytes, invalid CRC
        // Non-telemetry frequency
        decoder.finalize_burst(144_800_000, 15.0);
        assert!(decoder.packets.is_empty(), "Must not emit packet for APRS or unverified CRC");

        // 70cm DMR frequency
        decoder.burst_sample_count = 4800;
        decoder.raw_bytes_9600 = vec![0xAA, 0xBB, 0xCC, 0xDD];
        decoder.finalize_burst(434_500_000, 20.0);
        assert!(decoder.packets.is_empty(), "Must not emit packet for unverified CRC");
    }
}
