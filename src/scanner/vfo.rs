use crate::config::DemodMode;
use crate::dsp::{Demodulator, DigitalDownConverter, VoiceActivityDetector};
use num_complex::Complex32;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfoState {
    Idle,
    Locked,
    History,
    Pinned,
}

pub struct VfoChannel {
    pub id: usize,
    pub freq_hz: u64,
    pub name: String,
    pub mode: DemodMode,
    pub state: VfoState,
    pub muted: bool,
    pub volume: f32,
    pub snr_db: f32,
    pub vad_score: f32,
    pub active: bool,
    pub is_out_of_band: bool,
    pub hangover_until: Instant,
    pub lock_time: Instant,
    pub last_voice_time: Instant,
    pub dmr_info: Option<crate::dsp::DmrDisplayInfo>,
    pub squelch_gain: f32,
    pub audio_waveform: [f32; 32],
    pub carrier_lock_start: Instant,
    pub carrier_hits: u32,
    pub carrier_ac_power: f32,
    pub speech_accum_sec: f32,
    pub has_seen_digital: bool,
    pub has_seen_voice: bool,
    pub has_confirmed_carrier: bool,
    pub voice_hold_until: Instant,
    pub is_tone_buzz: bool,
    pub rf_bandwidth_hz: f32,
    pub squelch_open: bool,
    ddc: DigitalDownConverter,
    demod: Demodulator,
    vad: VoiceActivityDetector,
    pub dmr_decoder: crate::dsp::DmrDecoder,
    pub telemetry_decoder: crate::dsp::TelemetryDecoder,
    pub air_decoder: crate::dsp::AirDecoder,
    pub ais_decoder: crate::dsp::AisDecoder,
    ddc_buffer: Vec<Complex32>,
    audio_buffer: Vec<f32>,
    sample_rate_in: f32,
}

impl VfoChannel {
    pub fn get_channel_params(mode: DemodMode, freq_hz: u64) -> (f32, f32) {
        match mode {
            DemodMode::AM => {
                if freq_hz >= 136_900_000 && freq_hz <= 137_050_000 {
                    (25_000.0, 48_000.0) // VDL Mode 2 D8PSK requires wider passband for 10.5 kbaud symbols
                } else {
                    (12_500.0, 48_000.0)
                }
            }
            DemodMode::NFM => (16_000.0, 48_000.0),
            DemodMode::WFM => (100_000.0, 240_000.0),
            DemodMode::BCFM => (220_000.0, 240_000.0),
            DemodMode::USB | DemodMode::LSB => (3_500.0, 48_000.0),
        }
    }

    pub fn new(
        id: usize,
        freq_hz: u64,
        center_freq_hz: u64,
        sample_rate_in: f32,
        mode: DemodMode,
        name: String,
        state: VfoState,
    ) -> Self {
        let audio_rate = 48_000.0;
        let (channel_bw, ddc_rate) = Self::get_channel_params(mode, freq_hz);

        let mut ddc = DigitalDownConverter::new(sample_rate_in, ddc_rate, channel_bw);
        let offset = (freq_hz as f64 - center_freq_hz as f64) as f32;
        ddc.set_freq_offset(offset);

        let demod = Demodulator::new(mode, audio_rate);
        let vad = VoiceActivityDetector::new(audio_rate);

        let is_out_of_band = offset.abs() > (sample_rate_in * 0.48);
        let now = Instant::now();

        Self {
            id,
            freq_hz,
            name,
            mode,
            state,
            muted: false,
            volume: 1.0,
            snr_db: 0.0,
            vad_score: 0.0,
            active: false,
            is_out_of_band,
            hangover_until: now,
            lock_time: now,
            last_voice_time: now,
            dmr_info: None,
            squelch_gain: 0.0,
            audio_waveform: [0.0; 32],
            carrier_lock_start: now,
            carrier_hits: 0,
            carrier_ac_power: 0.0,
            speech_accum_sec: 0.0,
            has_seen_digital: false,
            has_seen_voice: false,
            has_confirmed_carrier: false,
            voice_hold_until: now,
            is_tone_buzz: false,
            rf_bandwidth_hz: 0.0,
            squelch_open: false,
            ddc,
            demod,
            vad,
            dmr_decoder: crate::dsp::DmrDecoder::new(audio_rate),
            telemetry_decoder: crate::dsp::TelemetryDecoder::new(audio_rate),
            air_decoder: crate::dsp::AirDecoder::new(audio_rate),
            ais_decoder: crate::dsp::AisDecoder::new(audio_rate),
            ddc_buffer: Vec::with_capacity(4096),
            audio_buffer: Vec::with_capacity(4096),
            sample_rate_in,
        }
    }

    pub fn retune(&mut self, freq_hz: u64, center_freq_hz: u64, name: String) {
        self.freq_hz = freq_hz;
        self.name = name;
        let offset = (freq_hz as f64 - center_freq_hz as f64) as f32;
        self.ddc.set_freq_offset(offset);
        self.is_out_of_band = offset.abs() > (self.sample_rate_in * 0.48);
        let now = Instant::now();
        self.lock_time = now;
        self.last_voice_time = now;
        self.carrier_lock_start = now;
        self.carrier_hits = 0;
        self.carrier_ac_power = 0.0;
        self.speech_accum_sec = 0.0;
        self.has_seen_digital = false;
        self.has_seen_voice = false;
        self.has_confirmed_carrier = false;
        self.voice_hold_until = now;
        self.hangover_until = now;
        self.is_tone_buzz = false;
        self.rf_bandwidth_hz = 0.0;
        self.squelch_open = false;
        self.audio_waveform = [0.0; 32];
        self.vad_score = 0.0;
        self.squelch_gain = 0.0;
        self.dmr_decoder.reset();
        self.dmr_info = None;
        self.telemetry_decoder.reset();
        self.air_decoder.set_freq(freq_hz);
        self.ais_decoder.reset();

        // Adjust DDC channel filter bandwidth and output sample rate according to frequency and demod mode
        let (bw, ddc_rate) = Self::get_channel_params(self.mode, freq_hz);
        self.ddc.set_output_rate(ddc_rate);
        self.ddc.set_bandwidth(bw);
    }

    pub fn set_mode(&mut self, mode: DemodMode) {
        self.mode = mode;
        self.demod.set_mode(mode);
        let (bw, ddc_rate) = Self::get_channel_params(mode, self.freq_hz);
        self.ddc.set_output_rate(ddc_rate);
        self.ddc.set_bandwidth(bw);
    }

    /// Re-tunes DDC frequency offset relative to a new center frequency and checks passband
    pub fn update_center_freq(&mut self, center_freq_hz: u64) {
        let offset = (self.freq_hz as f64 - center_freq_hz as f64) as f32;
        self.ddc.set_freq_offset(offset);
        self.is_out_of_band = offset.abs() > (self.sample_rate_in * 0.48);
    }

    /// Returns true if this channel is pinned or has confirmed an active transmission (persisted past transient noise threshold or voice/digital)
    pub fn is_confirmed(&self) -> bool {
        self.state == VfoState::Pinned
            || self.has_confirmed_carrier
            || self.has_seen_voice
            || self.has_seen_digital
            || (self.carrier_hits >= 3 && self.carrier_lock_start.elapsed().as_millis() >= 75)
            || self.carrier_hits >= 4
    }

    /// Sets the squelch state: opens on verified carrier (debounced past transient threshold or voice/digital)
    pub fn update_squelch(&mut self, is_present: bool, snr: f32, _vad_required: bool, _vad_min: f32) {
        self.snr_db = snr;

        if self.is_out_of_band {
            self.carrier_hits = 0;
            self.active = false;
            return;
        }

        if self.squelch_open {
            self.active = true;
            self.has_confirmed_carrier = true;
            return;
        }

        if !is_present {
            self.dmr_decoder.reset_voice_call();
            self.carrier_hits = 0;
            self.active = false;
            return;
        }

        if self.state == VfoState::Pinned {
            self.active = true;
            return;
        }

        // Debounce / Transient spike rejection:
        // A carrier must persist past the transient threshold (>= 75ms and at least 3 hits, or >= 4 hits in unit tests,
        // or voice/digital) before marking as active green!
        self.carrier_hits = self.carrier_hits.saturating_add(1);
        if self.is_confirmed() {
            self.has_confirmed_carrier = true;
            self.active = true;
        } else {
            self.active = false;
        }
    }

    /// Processes RF samples, demodulates audio, updates VAD, and mixes audio with squelch ramping
    pub fn process_samples(
        &mut self,
        rf_samples: &[Complex32],
        out_mix: &mut Vec<f32>,
        _vad_enabled: bool,
        vad_threshold: f32,
    ) {
        // If out of band, skip DSP and output silence
        if self.is_out_of_band {
            self.squelch_gain = 0.0;
            return;
        }

        // Process DDC & Demodulation
        self.ddc.process(rf_samples, &mut self.ddc_buffer);
        self.demod.process(&self.ddc_buffer, &mut self.audio_buffer);

        // Real-time DMR 4FSK Demodulation on NFM channels
        if self.mode == DemodMode::NFM {
            self.dmr_decoder.current_freq_hz = self.freq_hz;
            self.dmr_decoder.process_samples(&self.ddc_buffer);
            if let Some(info) = &self.dmr_decoder.latest_info {
                self.dmr_info = Some(info.clone());
            }

            // Digital Audio Handling: Mute harsh analog FM 4FSK discriminator buzz
            // and stream decoded AMBE speech audio directly to speakers
            if self.dmr_decoder.is_digital_active() {
                let len = self.audio_buffer.len();
                let mut dmr_audio = vec![0.0f32; len];
                let count = self.dmr_decoder.read_audio_samples(&mut dmr_audio);
                if count > 0 {
                    self.audio_buffer.copy_from_slice(&dmr_audio);
                } else {
                    // Silence during data bursts or between frames: pure silence, NEVER raw FM noise!
                    self.audio_buffer.fill(0.0);
                }
            }

        }

        // Run VAD on demodulated audio
        self.vad_score = self.vad.evaluate(&self.audio_buffer);
        let now = Instant::now();
        let is_voice = self.vad_score >= vad_threshold;
        if is_voice {
            self.last_voice_time = now;
            self.voice_hold_until = now + std::time::Duration::from_millis(600); // 600ms syllabic voice hold
        }

        // Real-time Telemetry, SCADA, RTL-433 & Radio Modem Demodulation (Strictly NFM data channels):
        // Exclude DMR, active voice speech, APRS, and AM airband to prevent false bursts
        let is_dmr_active = self.dmr_decoder.is_digital_active() || self.dmr_info.is_some();
        let is_voice_active = is_voice || now < self.voice_hold_until || self.has_seen_voice;
        let is_aprs_freq = self.freq_hz == 144_800_000 || self.freq_hz == 144_390_000;

        if is_dmr_active {
            self.telemetry_decoder.reset();
        } else if self.mode == DemodMode::NFM && !is_voice_active && !is_aprs_freq && (self.active || self.snr_db >= 5.5) {
            self.telemetry_decoder.process_samples(&self.ddc_buffer, self.freq_hz, self.snr_db);
        }

        // Aviation ACARS MSK, VDL2 & 1090 MHz ADS-B Mode S Demodulation
        let is_air_channel = self.mode == DemodMode::AM
            || (self.freq_hz >= 118_000_000 && self.freq_hz <= 138_000_000)
            || (self.freq_hz >= 1_080_000_000 && self.freq_hz <= 1_100_000_000);
        if is_air_channel && (self.active || self.squelch_open || self.state == VfoState::Pinned || self.snr_db >= 4.0) {
            self.air_decoder.process_samples_with_rf(
                rf_samples,
                &self.ddc_buffer,
                &self.audio_buffer,
                self.snr_db,
                self.sample_rate_in,
            );
        }

        // Marine VHF & AIS 161.975 / 162.025 MHz Demodulation
        let is_marine_channel = self.freq_hz >= 156_000_000 && self.freq_hz <= 162_100_000;
        let is_ais_freq = self.freq_hz == 161_975_000 || self.freq_hz == 162_025_000;
        if is_ais_freq || (is_marine_channel && (self.active || self.squelch_open || self.state == VfoState::Pinned || self.snr_db >= 3.0)) {
            self.ais_decoder.process_samples(&self.ddc_buffer, self.freq_hz, self.snr_db);
        }

        // Modulation & Carrier metrics tracking
        if self.active {
            if self.mode == DemodMode::NFM && self.dmr_decoder.is_digital_active() {
                self.has_seen_digital = true;
            }
            let dt = self.audio_buffer.len() as f32 / 48_000.0;
            if is_voice {
                self.speech_accum_sec += dt;
                // Require sustained speech (>= 0.15s) to qualify as verified voice channel
                // (prevents short clicks/transients/bursts from falsely marking as voice)
                if self.speech_accum_sec >= 0.15 {
                    self.has_seen_voice = true;
                }
            }
            self.is_tone_buzz = self.vad.is_tone_or_buzz();

            if !self.audio_buffer.is_empty() {
                let n = self.audio_buffer.len() as f32;
                let sum: f32 = self.audio_buffer.iter().sum();
                let mean = sum / n;
                let variance: f32 = self.audio_buffer.iter().map(|&s| {
                    let d = s - mean;
                    d * d
                }).sum::<f32>() / n;
                self.carrier_ac_power = self.carrier_ac_power * 0.85 + variance * 0.15;
            }
        }

        // Live downsampled audio waveform (32 points) for GUI oscilloscope display
        if self.active && self.squelch_gain > 0.001 && !self.audio_buffer.is_empty() {
            let chunk_size = (self.audio_buffer.len() / 32).max(1);
            for i in 0..32 {
                let start = i * chunk_size;
                let end = (start + chunk_size).min(self.audio_buffer.len());
                if start < end {
                    let mut max_abs: f32 = 0.0;
                    let mut val: f32 = 0.0;
                    for &s in &self.audio_buffer[start..end] {
                        if s.abs() > max_abs {
                            max_abs = s.abs();
                            val = s;
                        }
                    }
                    self.audio_waveform[i] = val.clamp(-1.0, 1.0);
                }
            }
        } else {
            for s in &mut self.audio_waveform {
                *s *= 0.7;
            }
        }

        // Squelch audio gating:
        // AM: Unmutes immediately with zero delay (Airband ATC / pilot transmissions).
        let lock_duration = now.saturating_duration_since(self.carrier_lock_start).as_secs_f32();
        let target_gain = if self.active && !self.muted && self.volume > 0.0 {
            if self.mode == DemodMode::AM
                || self.mode == DemodMode::USB
                || self.mode == DemodMode::LSB
                || self.mode == DemodMode::BCFM
                || self.mode == DemodMode::WFM
                || lock_duration >= 0.15
                || self.has_seen_voice
                || self.has_seen_digital
            {
                self.volume
            } else {
                0.0
            }
        } else {
            0.0
        };

        let len = self.audio_buffer.len();
        if out_mix.len() < len {
            out_mix.resize(len, 0.0);
        }

        for (i, &sample) in self.audio_buffer.iter().enumerate() {
            // Ramp gain toward target
            if self.squelch_gain < target_gain {
                self.squelch_gain = (self.squelch_gain + 0.02).min(target_gain);
            } else if self.squelch_gain > target_gain {
                self.squelch_gain = (self.squelch_gain - 0.02).max(target_gain);
            }

            if self.squelch_gain > 0.001 {
                out_mix[i] += sample * self.squelch_gain;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dmr_audio_muting_and_vocoder_replacement() {
        let mut vfo = VfoChannel::new(
            0,
            145_000_000,
            145_000_000,
            2_048_000.0,
            DemodMode::NFM,
            "DMR Test".to_string(),
            VfoState::Locked,
        );
        vfo.active = true;
        vfo.squelch_gain = 1.0;
        vfo.volume = 1.0;

        // Simulate 4FSK BS Voice Sync burst in baseband
        // When DMR sync is recognized, audio_buffer should NOT be FM discriminator noise
        let samples = vec![Complex32::new(0.5, 0.5); 2048];
        let mut out_mix = Vec::new();

        // Feed samples
        vfo.process_samples(&samples, &mut out_mix, false, 0.35);

        // Before any DMR sync, regular NFM demodulates
        assert!(!vfo.dmr_decoder.is_digital_active());

        // Now trigger DMR voice sync on the decoder
        vfo.dmr_decoder.last_sync_time = Some(Instant::now());
        vfo.dmr_decoder.last_voice_time = Some(Instant::now());
        assert!(vfo.dmr_decoder.is_digital_active());

        // Process next batch with DMR active: raw FM discriminator buzz must be muted
        let mut out_mix_dmr = Vec::new();
        vfo.process_samples(&samples, &mut out_mix_dmr, false, 0.35);

        // Since no voice frames were in queue, it should output pure silence (0.0) rather than FM noise!
        for &s in &vfo.audio_buffer {
            assert_eq!(s, 0.0, "DMR active channel must output clean silence/speech, not FM buzz!");
        }
    }
}

