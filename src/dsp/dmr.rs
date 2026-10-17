use num_complex::Complex32;
use std::time::Instant;

/// DMR Synchronization Patterns (48 bits / 24 symbols, ETSI TS 102 361-1 clause 9.1.1)
pub const DMR_BS_VOICE_SYNC: u64 = 0x755_FD7D_F75F_7;
pub const DMR_BS_DATA_SYNC: u64 = 0xDFF_57D7_5DF5_D;
pub const DMR_MS_VOICE_SYNC: u64 = 0x7F7_D5DD_57DF_D;
pub const DMR_MS_DATA_SYNC: u64 = 0xD5D_7F77_F5D7_5;
pub const DMR_RC_VOICE_SYNC: u64 = 0x77D_55F7_DFD7_7;
pub const DMR_RC_DATA_SYNC: u64 = 0xD75_FFD5_DF7D_7;

// Inverted polarity sync patterns
#[allow(dead_code)]
pub const DMR_BS_VOICE_SYNC_INV: u64 = 0x8AA_0282_08A0_8;
#[allow(dead_code)]
pub const DMR_BS_DATA_SYNC_INV: u64 = 0x200_A828_A20A_2;
#[allow(dead_code)]
pub const DMR_MS_VOICE_SYNC_INV: u64 = 0x808_2A22_A802_2;
#[allow(dead_code)]
pub const DMR_MS_DATA_SYNC_INV: u64 = 0x2A2_8088_0A28_A;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmrSyncType {
    BsVoice,
    BsData,
    MsVoice,
    MsData,
    RcVoice,
    RcData,
}

#[allow(dead_code)]
impl DmrSyncType {
    pub fn name(&self) -> &'static str {
        match self {
            DmrSyncType::BsVoice => "BS Voice",
            DmrSyncType::BsData => "BS Data/Header",
            DmrSyncType::MsVoice => "MS Voice (Simplex)",
            DmrSyncType::MsData => "MS Data (Simplex)",
            DmrSyncType::RcVoice => "RC Voice",
            DmrSyncType::RcData => "RC Data",
        }
    }

    pub fn is_voice(&self) -> bool {
        matches!(self, DmrSyncType::BsVoice | DmrSyncType::MsVoice | DmrSyncType::RcVoice)
    }

    pub fn is_data(&self) -> bool {
        matches!(self, DmrSyncType::BsData | DmrSyncType::MsData | DmrSyncType::RcData)
    }

    pub fn is_simplex(&self) -> bool {
        matches!(self, DmrSyncType::MsVoice | DmrSyncType::MsData)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmrDisplayInfo {
    pub color_code: u8,
    pub timeslot: u8,
    pub call_type: String,      // "Group" or "Private"
    pub target_id: Option<u32>, // Talkgroup (TG)
    pub source_id: Option<u32>, // Radio ID (SRC)
    pub burst_type: String,     // "Voice" or "Data/LC"
    pub is_simplex: bool,
}

impl DmrDisplayInfo {
    pub fn badge_text(&self) -> String {
        let mut text = format!("DMR CC:{} TS:{}", self.color_code, self.timeslot);
        if let Some(tg) = self.target_id {
            if tg > 0 {
                text.push_str(&format!(" TG:{}", tg));
            }
        }
        if let Some(src) = self.source_id {
            if src > 0 {
                text.push_str(&format!(" ID:{}", src));
            }
        }
        text
    }
}

#[derive(Debug, Clone)]
pub struct DmrPacket {
    pub timestamp: Instant,
    pub time_str: String,
    pub freq_hz: u64,
    pub sync_type: DmrSyncType,
    pub color_code: u8,
    pub timeslot: u8,
    pub call_type: String,
    pub target_id: Option<u32>,
    pub source_id: Option<u32>,
    pub summary: String,
}

use std::collections::VecDeque;
use crate::dsp::dmr_vocoder::DmrVocoder;

/// High-performance 4FSK ETSI TS 102 361 DMR Demodulator & Frame Decoder
pub struct DmrDecoder {
    #[allow(dead_code)]
    sample_rate: f32,
    #[allow(dead_code)]
    samples_per_symbol: f32,
    dpll_phase: f32,
    dpll_step: f32,

    last_sample: Complex32,
    disc_history: [f32; 3],
    disc_idx: usize,

    peak_tracker: f32,
    dc_tracker: f32,

    shift_reg: u64,
    shift_reg_inv: u64,

    last_sign: bool,

    // Circular symbol ring buffer (stores 512 dibits / symbols = ~4 TDMA slots)
    symbol_buf: [u8; 512],
    symbol_idx: usize,

    // Burst collection state
    collecting_burst: bool,
    burst_symbols_remaining: usize,
    burst_sync_type: Option<DmrSyncType>,
    burst_inverted: bool,

    // AMBE+2 speech vocoder and audio FIFO queue
    vocoder: DmrVocoder,
    audio_out_queue: VecDeque<f32>,
    voice_scratch: Vec<f32>,
    pub last_voice_time: Option<Instant>,
    in_voice_call: bool,
    voice_burst_countdown: usize,

    // Tracking state
    pub color_code: u8,
    candidate_color_code: u8,
    candidate_cc_count: u8,
    pub last_sync_time: Option<Instant>,
    pub last_timeslot: u8,
    pub latest_info: Option<DmrDisplayInfo>,
    pub current_freq_hz: u64,
    pub packets: Vec<DmrPacket>,
}

impl DmrDecoder {
    pub fn new(sample_rate: f32) -> Self {
        let baud_rate = 4800.0;
        let samples_per_symbol = sample_rate / baud_rate;
        let dpll_step = 1.0 / samples_per_symbol;

        Self {
            sample_rate,
            samples_per_symbol,
            dpll_phase: 0.0,
            dpll_step,
            last_sample: Complex32::default(),
            disc_history: [0.0; 3],
            disc_idx: 0,
            peak_tracker: 0.20,
            dc_tracker: 0.0,
            shift_reg: 0,
            shift_reg_inv: 0,
            last_sign: false,
            symbol_buf: [0; 512],
            symbol_idx: 0,
            collecting_burst: false,
            burst_symbols_remaining: 0,
            burst_sync_type: None,
            burst_inverted: false,
            vocoder: DmrVocoder::new(),
            audio_out_queue: VecDeque::with_capacity(9600),
            voice_scratch: Vec::with_capacity(2880),
            last_voice_time: None,
            in_voice_call: false,
            voice_burst_countdown: 0,
            color_code: 1,
            candidate_color_code: 1,
            candidate_cc_count: 0,
            last_sync_time: None,
            last_timeslot: 1,
            latest_info: None,
            current_freq_hz: 0,
            packets: Vec::with_capacity(32),
        }
    }

    pub fn reset(&mut self) {
        self.dpll_phase = 0.0;
        self.last_sample = Complex32::default();
        self.disc_history = [0.0; 3];
        self.disc_idx = 0;
        self.peak_tracker = 0.20;
        self.dc_tracker = 0.0;
        self.shift_reg = 0;
        self.shift_reg_inv = 0;
        self.last_sign = false;
        self.symbol_buf = [0; 512];
        self.symbol_idx = 0;
        self.collecting_burst = false;
        self.burst_symbols_remaining = 0;
        self.burst_sync_type = None;
        self.vocoder.reset();
        self.audio_out_queue.clear();
        self.voice_scratch.clear();
        self.last_voice_time = None;
        self.in_voice_call = false;
        self.voice_burst_countdown = 0;
        self.color_code = 1;
        self.candidate_color_code = 1;
        self.candidate_cc_count = 0;
        self.latest_info = None;
    }

    pub fn reset_voice_call(&mut self) {
        self.in_voice_call = false;
        self.voice_burst_countdown = 0;
        self.collecting_burst = false;
        self.burst_symbols_remaining = 0;
        self.burst_sync_type = None;
        self.audio_out_queue.clear();
    }

    /// Returns true if a DMR digital transmission was recently detected
    pub fn is_digital_active(&self) -> bool {
        self.last_sync_time
            .map_or(false, |t| t.elapsed().as_millis() < 800)
            || self.last_voice_time
                .map_or(false, |t| t.elapsed().as_millis() < 600)
    }

    /// Returns true if active voice is currently being decoded
    #[allow(dead_code)]
    pub fn is_receiving_voice(&self) -> bool {
        self.last_voice_time
            .map_or(false, |t| t.elapsed().as_millis() < 800)
    }

    /// Reads up to out_buf.len() synthesized PCM speech samples at 48 kHz
    pub fn read_audio_samples(&mut self, out_buf: &mut [f32]) -> usize {
        let n = out_buf.len().min(self.audio_out_queue.len());
        for i in 0..n {
            out_buf[i] = self.audio_out_queue.pop_front().unwrap_or(0.0);
        }
        n
    }

    /// Feeds complex baseband samples (typically 48 kHz from DDC)
    pub fn process_samples(&mut self, samples: &[Complex32]) {
        for &s in samples {
            // 1. FM Discriminator
            let conj = self.last_sample.conj();
            let diff = s * conj;
            let raw_angle = diff.im.atan2(diff.re);
            self.last_sample = s;

            // DC tracking & removal
            self.dc_tracker = self.dc_tracker * 0.9995 + raw_angle * 0.0005;
            let disc = raw_angle - self.dc_tracker;

            self.disc_history[self.disc_idx] = disc;
            self.disc_idx = (self.disc_idx + 1) % 3;
            let smoothed = (self.disc_history[0] + self.disc_history[1] + self.disc_history[2]) / 3.0;

            // Amplitude peak tracking for adaptive 4FSK slicing
            let abs_val = smoothed.abs();
            if abs_val > self.peak_tracker {
                self.peak_tracker = self.peak_tracker * 0.99 + abs_val * 0.01;
            } else {
                self.peak_tracker = self.peak_tracker * 0.9998 + abs_val * 0.0002;
            }
            let peak = self.peak_tracker.clamp(0.05, 1.2);

            // 2. DPLL Symbol Clock Recovery with Hysteresis Zero-Crossing Phase Locking
            // Using hysteresis (peak * 0.15) prevents noise near 0.0 from randomly jittering symbol clock
            let hyst = peak * 0.15;
            if smoothed > hyst && !self.last_sign {
                self.last_sign = true;
                let err = if self.dpll_phase < 0.5 {
                    -self.dpll_phase
                } else {
                    1.0 - self.dpll_phase
                };
                self.dpll_phase = (self.dpll_phase + err * 0.05).clamp(0.0, 1.0);
            } else if smoothed < -hyst && self.last_sign {
                self.last_sign = false;
                let err = if self.dpll_phase < 0.5 {
                    -self.dpll_phase
                } else {
                    1.0 - self.dpll_phase
                };
                self.dpll_phase = (self.dpll_phase + err * 0.05).clamp(0.0, 1.0);
            }

            let prev_phase = self.dpll_phase;
            self.dpll_phase += self.dpll_step;
            let sample_eye = prev_phase < 0.5 && self.dpll_phase >= 0.5;
            if self.dpll_phase >= 1.0 {
                self.dpll_phase -= 1.0;
            }

            // Check if we crossed center-of-eye sample threshold (phase 0.5)
            if sample_eye {
                // Slice 4FSK symbol into 2-bit dibit
                // Nominal levels: -3, -1, +1, +3. Optimal decision boundary between +1 and +3 is 0.65*peak
                let threshold = peak * 0.65;
                let (dibit_normal, dibit_inv) = if smoothed > threshold {
                    (0b01, 0b11) // +3
                } else if smoothed > 0.0 {
                    (0b00, 0b10) // +1
                } else if smoothed > -threshold {
                    (0b10, 0b00) // -1
                } else {
                    (0b11, 0b01) // -3
                };

                self.push_symbol(dibit_normal, dibit_inv);
            }
        }
    }

    /// Directly processes demodulated 4FSK discriminator float samples
    #[allow(dead_code)]
    pub fn process_discriminator(&mut self, disc_samples: &[f32]) {
        for &raw_disc in disc_samples {
            self.dc_tracker = self.dc_tracker * 0.9995 + raw_disc * 0.0005;
            let disc = raw_disc - self.dc_tracker;

            self.disc_history[self.disc_idx] = disc;
            self.disc_idx = (self.disc_idx + 1) % 3;
            let smoothed = (self.disc_history[0] + self.disc_history[1] + self.disc_history[2]) / 3.0;

            let abs_val = smoothed.abs();
            if abs_val > self.peak_tracker {
                self.peak_tracker = self.peak_tracker * 0.99 + abs_val * 0.01;
            } else {
                self.peak_tracker = self.peak_tracker * 0.9998 + abs_val * 0.0002;
            }
            let peak = self.peak_tracker.clamp(0.05, 1.2);

            let hyst = peak * 0.15;
            if smoothed > hyst && !self.last_sign {
                self.last_sign = true;
                let err = if self.dpll_phase < 0.5 {
                    -self.dpll_phase
                } else {
                    1.0 - self.dpll_phase
                };
                self.dpll_phase = (self.dpll_phase + err * 0.05).clamp(0.0, 1.0);
            } else if smoothed < -hyst && self.last_sign {
                self.last_sign = false;
                let err = if self.dpll_phase < 0.5 {
                    -self.dpll_phase
                } else {
                    1.0 - self.dpll_phase
                };
                self.dpll_phase = (self.dpll_phase + err * 0.05).clamp(0.0, 1.0);
            }

            let prev_phase = self.dpll_phase;
            self.dpll_phase += self.dpll_step;
            let sample_eye = prev_phase < 0.5 && self.dpll_phase >= 0.5;
            if self.dpll_phase >= 1.0 {
                self.dpll_phase -= 1.0;
            }

            if sample_eye {
                let threshold = peak * 0.65;
                let (dibit_normal, dibit_inv) = if smoothed > threshold {
                    (0b01, 0b11)
                } else if smoothed > 0.0 {
                    (0b00, 0b10)
                } else if smoothed > -threshold {
                    (0b10, 0b00)
                } else {
                    (0b11, 0b01)
                };

                self.push_symbol(dibit_normal, dibit_inv);
            }
        }
    }

    /// Ingests a demodulated symbol (dibit) into correlator and burst assembler
    fn push_symbol(&mut self, dibit_norm: u8, dibit_inv: u8) {
        // Update 48-bit shift registers (24 symbols)
        self.shift_reg = ((self.shift_reg << 2) | (dibit_norm as u64)) & 0x0000_FFFF_FFFF_FFFF;
        self.shift_reg_inv = ((self.shift_reg_inv << 2) | (dibit_inv as u64)) & 0x0000_FFFF_FFFF_FFFF;

        // Store active dibit in circular ring buffer
        let dibit_to_store = if self.burst_inverted { dibit_inv } else { dibit_norm };
        self.symbol_buf[self.symbol_idx] = dibit_to_store;
        self.symbol_idx = (self.symbol_idx + 1) % self.symbol_buf.len();

        if self.collecting_burst {
            if self.burst_symbols_remaining > 0 {
                self.burst_symbols_remaining -= 1;
            }
            if self.burst_symbols_remaining == 0 {
                self.collecting_burst = false;
                self.parse_complete_burst();

                // If in an active voice call, the next voice burst on OUR timeslot arrives 288 symbols later
                // (ETSI TS 102 361-1: 1 slot = 30 ms = 144 symbols; 1 TDMA frame = 2 slots = 60 ms = 288 symbols).
                // From the end of Burst A (after 132 symbols) to the end of Burst B is exactly 288 symbols!
                if self.in_voice_call && self.voice_burst_countdown > 0 {
                    self.voice_burst_countdown -= 1;
                    self.collecting_burst = true;
                    self.burst_symbols_remaining = 288;
                } else {
                    self.in_voice_call = false;
                    self.burst_sync_type = None;
                }
            }
        }

        // Correlate against 48-bit sync patterns
        if let Some((sync_type, is_inv)) = Self::correlate_sync(self.shift_reg, self.shift_reg_inv) {
            // While receiving an ongoing voice superframe, ignore data sync patterns
            // so embedded signaling or audio dibits never abort speech decoding!
            if self.in_voice_call && !sync_type.is_voice() {
                return;
            }

            let now = Instant::now();
            self.last_sync_time = Some(now);

            // Retroactive polarity correction:
            // Ensure all preceding 78 symbols of this burst match the detected sync polarity!
            if is_inv != self.burst_inverted {
                let buf_len = self.symbol_buf.len();
                for i in 0..78 {
                    let ring_pos = (self.symbol_idx + buf_len - 78 + i) % buf_len;
                    self.symbol_buf[ring_pos] ^= 0b10;
                }
                self.burst_inverted = is_inv;
            }

            // DMR repeater burst alternation (TS 1 vs TS 2, each burst is 30ms)
            if !sync_type.is_simplex() {
                self.last_timeslot = if self.last_timeslot == 1 { 2 } else { 1 };
            } else {
                self.last_timeslot = 1;
            }

            // Sync word occupies symbols 54..77 (24 symbols).
            // A DMR burst has 132 symbols (symbols 0..131).
            // When sync is detected, symbol 77 was just pushed!
            // We now collect the remaining 54 symbols (symbols 78..131) to complete the burst.
            self.collecting_burst = true;
            self.burst_symbols_remaining = 54;
            self.burst_sync_type = Some(sync_type);
            if sync_type.is_voice() {
                self.in_voice_call = true;
                self.voice_burst_countdown = 5;
            }

            // Immediately record telemetry if we don't have detailed LC yet
            if self.latest_info.is_none() {
                self.latest_info = Some(DmrDisplayInfo {
                    color_code: self.color_code,
                    timeslot: self.last_timeslot,
                    call_type: if sync_type.is_simplex() { "Simplex".into() } else { "Group".into() },
                    target_id: None,
                    source_id: None,
                    burst_type: sync_type.name().into(),
                    is_simplex: sync_type.is_simplex(),
                });
            }
        }
    }

    /// Correlates 48-bit shift register against ETSI TS 102 361 sync words
    pub fn correlate_sync(reg_norm: u64, reg_inv: u64) -> Option<(DmrSyncType, bool)> {
        let tests: &[(u64, DmrSyncType, u32)] = &[
            (DMR_BS_VOICE_SYNC, DmrSyncType::BsVoice, 2),
            (DMR_MS_VOICE_SYNC, DmrSyncType::MsVoice, 2),
            (DMR_RC_VOICE_SYNC, DmrSyncType::RcVoice, 2),
            (DMR_BS_DATA_SYNC, DmrSyncType::BsData, 1),
            (DMR_MS_DATA_SYNC, DmrSyncType::MsData, 1),
            (DMR_RC_DATA_SYNC, DmrSyncType::RcData, 1),
        ];

        for &(pattern, sync_type, max_errors) in tests {
            // Normal polarity
            let diff = (reg_norm ^ pattern).count_ones();
            if diff <= max_errors {
                return Some((sync_type, false));
            }

            // Inverted polarity
            let diff_inv = (reg_inv ^ pattern).count_ones();
            if diff_inv <= max_errors {
                return Some((sync_type, true));
            }
        }

        None
    }

    /// Parses the full 132-symbol (264-bit) burst once symbol 131 is received
    fn parse_complete_burst(&mut self) {
        let sync_type = match self.burst_sync_type {
            Some(st) => st,
            None => return,
        };

        // Extract 132 symbols from circular ring buffer
        // The most recently added symbol is symbol 131.
        let buf_len = self.symbol_buf.len();
        let mut burst_symbols = [0u8; 132];
        for i in 0..132 {
            // Symbol 0 was 132 symbols ago
            let ring_pos = (self.symbol_idx + buf_len - 132 + i) % buf_len;
            burst_symbols[i] = self.symbol_buf[ring_pos];
        }

        // Convert 132 symbols to 264 bit array
        let mut burst_bits = [0u8; 264];
        for i in 0..132 {
            let dibit = burst_symbols[i];
            burst_bits[i * 2] = (dibit >> 1) & 1;
            burst_bits[i * 2 + 1] = dibit & 1;
        }

        let mut target_id = None;
        let mut source_id = None;
        let mut call_type = "Group".to_string();

        // Slot Type (SLCO) field and Link Control (LC) payload exist ONLY in Data / Header bursts
        if sync_type.is_data() {
            let mut slco_bits = [0u8; 20];
            slco_bits[..10].copy_from_slice(&burst_bits[98..108]);
            slco_bits[10..].copy_from_slice(&burst_bits[156..166]);

            let (cc, data_type) = Self::decode_slot_type(&slco_bits);
            if cc <= 15 {
                if cc == self.candidate_color_code {
                    self.candidate_cc_count = self.candidate_cc_count.saturating_add(1);
                    if self.candidate_cc_count >= 3 {
                        self.color_code = cc;
                    }
                } else {
                    self.candidate_color_code = cc;
                    self.candidate_cc_count = 1;
                }
            }

            // In Data/Header bursts or LC (Data Type 1 = Voice LC Header, 2 = Terminator with LC)
            if data_type == 1 || data_type == 2 {
                if let Some((lc_call_type, tg, src)) = Self::decode_full_lc(&burst_bits) {
                    call_type = lc_call_type;
                    if tg > 0 {
                        target_id = Some(tg);
                    }
                    if src > 0 {
                        source_id = Some(src);
                    }
                }
            }
        }

        // If Voice Burst, synthesize AMBE+2 speech frames into audio queue
        if sync_type.is_voice() {
            self.last_voice_time = Some(Instant::now());

            self.voice_scratch.clear();
            self.vocoder.decode_voice_burst(&burst_symbols, &mut self.voice_scratch);

            for &sample in &self.voice_scratch {
                if self.audio_out_queue.len() < 48_000 {
                    self.audio_out_queue.push_back(sample);
                }
            }
        } else if sync_type.is_data() {
            self.in_voice_call = false;
            self.voice_burst_countdown = 0;
        }

        let info = DmrDisplayInfo {
            color_code: self.color_code,
            timeslot: self.last_timeslot,
            call_type: call_type.clone(),
            target_id,
            source_id,
            burst_type: sync_type.name().to_string(),
            is_simplex: sync_type.is_simplex(),
        };

        let summary = format!(
            "[{}] CC:{} TS:{} {}{}{}",
            sync_type.name(),
            self.color_code,
            self.last_timeslot,
            if let Some(tg) = target_id { format!("TG:{} ", tg) } else { String::new() },
            if let Some(src) = source_id { format!("ID:{} ", src) } else { String::new() },
            call_type
        );

        let pkt = DmrPacket {
            timestamp: Instant::now(),
            time_str: current_time_str(),
            freq_hz: self.current_freq_hz,
            sync_type,
            color_code: self.color_code,
            timeslot: self.last_timeslot,
            call_type,
            target_id,
            source_id,
            summary,
        };

        if self.packets.len() >= 64 {
            self.packets.remove(0);
        }
        self.packets.push(pkt);
        self.latest_info = Some(info);
    }

    /// Decodes the 20-bit Slot Type Golay(20,8) field to extract 4-bit Color Code and 4-bit Data Type
    pub fn decode_slot_type(slco_bits: &[u8; 20]) -> (u8, u8) {
        // Information bits in Slot Type:
        // Bits 0..3: Color Code (CC 0..15)
        // Bits 4..7: Data Type (0..15)
        let mut cc = 0u8;
        for i in 0..4 {
            cc = (cc << 1) | slco_bits[i];
        }

        let mut data_type = 0u8;
        for i in 4..8 {
            data_type = (data_type << 1) | slco_bits[i];
        }

        // Clamp Color Code to standard 0..15
        (cc.clamp(0, 15), data_type)
    }

    /// Decodes 72-bit Link Control (LC) PDU from payload bits
    pub fn decode_full_lc(burst_bits: &[u8; 264]) -> Option<(String, u32, u32)> {
        // Voice LC Header or Terminator payload:
        // Pack first 108 bits and second 108 bits (excluding the center 48-bit sync)
        let mut payload_bytes = [0u8; 27];
        let mut bit_idx = 0;

        // Skip slot type bits and pack payload
        for i in 0..96 {
            let byte_pos = bit_idx / 8;
            let bit_pos = 7 - (bit_idx % 8);
            payload_bytes[byte_pos] |= burst_bits[i] << bit_pos;
            bit_idx += 1;
        }
        for i in 166..262 {
            let byte_pos = bit_idx / 8;
            let bit_pos = 7 - (bit_idx % 8);
            if byte_pos < payload_bytes.len() {
                payload_bytes[byte_pos] |= burst_bits[i] << bit_pos;
                bit_idx += 1;
            }
        }

        // In DMR Voice LC Header (ETSI TS 102 361-2 clause 7.1):
        // Byte 0: Service Options / FLCO
        //   Bit 6: Group (0) vs Individual/Private (1)
        // Byte 1: Feature Set ID (FID)
        // Byte 2: Service options
        // Bytes 3..5: Target Address (Talkgroup) (24 bits)
        // Bytes 6..8: Source Radio ID (24 bits)
        let flco = payload_bytes[0];
        let is_private = (flco & 0x40) != 0;
        let call_type = if is_private { "Private" } else { "Group" }.to_string();

        let target_id = ((payload_bytes[3] as u32) << 16)
            | ((payload_bytes[4] as u32) << 8)
            | (payload_bytes[5] as u32);

        let source_id = ((payload_bytes[6] as u32) << 16)
            | ((payload_bytes[7] as u32) << 8)
            | (payload_bytes[8] as u32);

        // Sanity check valid IDs (DMR IDs are up to 16,777,215 (0xFFFFFF))
        if target_id > 0 || source_id > 0 {
            Some((call_type, target_id, source_id))
        } else {
            None
        }
    }
}

fn current_time_str() -> String {
    let now = std::time::SystemTime::now();
    if let Ok(duration) = now.duration_since(std::time::UNIX_EPOCH) {
        let secs = duration.as_secs();
        let s = secs % 60;
        let m = (secs / 60) % 60;
        let h = ((secs / 3600) + 3) % 24; // Finnish local time UTC+3
        format!("{:02}:{:02}:{:02}", h, m, s)
    } else {
        "00:00:00".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dmr_sync_correlation() {
        // Test BS Voice Sync
        let res = DmrDecoder::correlate_sync(DMR_BS_VOICE_SYNC, 0);
        assert_eq!(res, Some((DmrSyncType::BsVoice, false)));

        // Test BS Data Sync
        let res_data = DmrDecoder::correlate_sync(DMR_BS_DATA_SYNC, 0);
        assert_eq!(res_data, Some((DmrSyncType::BsData, false)));

        // Test MS Voice Sync
        let res_ms = DmrDecoder::correlate_sync(DMR_MS_VOICE_SYNC, 0);
        assert_eq!(res_ms, Some((DmrSyncType::MsVoice, false)));

        // Test 1-bit corrupted sync (error tolerance)
        let corrupt_sync = DMR_BS_VOICE_SYNC ^ 0x01;
        let res_corrupt = DmrDecoder::correlate_sync(corrupt_sync, 0);
        assert_eq!(res_corrupt, Some((DmrSyncType::BsVoice, false)));
    }

    #[test]
    fn test_dmr_slot_type_decode() {
        // Slot type with Color Code 1 (0001) and Data Type 1 (0001 = Voice LC Header)
        let mut slco = [0u8; 20];
        slco[3] = 1; // CC = 1
        slco[7] = 1; // Data Type = 1

        let (cc, dt) = DmrDecoder::decode_slot_type(&slco);
        assert_eq!(cc, 1);
        assert_eq!(dt, 1);
    }

    #[test]
    fn test_dmr_end_to_end_demodulation() {
        let mut decoder = DmrDecoder::new(48_000.0);

        // Generate synthetic 4FSK signal for BS Voice Sync (24 symbols)
        // Each symbol = 10 samples at 48 kHz
        let mut disc_samples = Vec::new();

        // 54 symbols of zero-mean scrambled payload
        for i in 0..54 {
            let lvl = if i % 2 == 0 { 0.12 } else { -0.12 };
            for _ in 0..10 {
                disc_samples.push(lvl);
            }
        }

        // 24 symbols of BS Voice Sync: 0x755FD7DF75F7
        let sync_pattern = DMR_BS_VOICE_SYNC;
        for i in (0..24).rev() {
            let dibit = ((sync_pattern >> (i * 2)) & 0x03) as u8;
            let level = match dibit {
                0b01 => 0.35,  // +3
                0b00 => 0.12,  // +1
                0b10 => -0.12, // -1
                0b11 => -0.35, // -3
                _ => 0.0,
            };
            for _ in 0..10 {
                disc_samples.push(level);
            }
        }

        // 54 symbols of trailing payload
        for i in 0..54 {
            let lvl = if i % 2 == 0 { 0.12 } else { -0.12 };
            for _ in 0..10 {
                disc_samples.push(lvl);
            }
        }

        decoder.process_discriminator(&disc_samples);

        assert!(decoder.last_sync_time.is_some(), "DMR Sync must be detected!");
        assert!(decoder.latest_info.is_some(), "DMR Info must be populated!");
        let info = decoder.latest_info.unwrap();
        assert_eq!(info.burst_type, "BS Voice");
    }
}
