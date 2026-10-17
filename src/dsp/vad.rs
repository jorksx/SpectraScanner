use std::f32::consts::PI;

pub struct VoiceActivityDetector {
    bp_state_1: f32,
    bp_state_2: f32,
    bp_b0: f32,
    bp_b2: f32,
    bp_a1: f32,
    bp_a2: f32,
    env_peak: f32,
    env_trough: f32,
    running_vad_score: f32,
    audio_sample_rate: f32,
    speech_frames_count: u32,
    rms_history: [f32; 8],
    rms_history_idx: usize,
    is_tone_buzz: bool,
    decim_buf: Vec<f32>,
}

impl VoiceActivityDetector {
    pub fn new(audio_sample_rate: f32) -> Self {
        // 2nd order Butterworth Bandpass filter for human voice: 300 Hz to 3000 Hz
        let f_low = 300.0f32;
        let f_high = 3000.0f32;
        let f_center = (f_low * f_high).sqrt();
        let q = f_center / (f_high - f_low);

        let w0 = 2.0 * PI * f_center / audio_sample_rate;
        let alpha = w0.sin() / (2.0 * q);

        let b0 = alpha;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * w0.cos();
        let a2 = 1.0 - alpha;

        Self {
            bp_state_1: 0.0,
            bp_state_2: 0.0,
            bp_b0: b0 / a0,
            bp_b2: b2 / a0,
            bp_a1: a1 / a0,
            bp_a2: a2 / a0,
            env_peak: 0.0,
            env_trough: 0.0,
            running_vad_score: 0.0,
            audio_sample_rate,
            speech_frames_count: 0,
            rms_history: [0.0; 8],
            rms_history_idx: 0,
            is_tone_buzz: false,
            decim_buf: Vec::with_capacity(512),
        }
    }

    pub fn is_tone_or_buzz(&self) -> bool {
        self.is_tone_buzz
    }

    /// Evaluates a block of audio samples and returns speech confidence (0.0 to 1.0)
    pub fn evaluate(&mut self, audio: &[f32]) -> f32 {
        if audio.is_empty() {
            return self.running_vad_score;
        }

        let mut total_energy = 0.0f32;
        let mut voice_band_energy = 0.0f32;
        let mut zero_crossing_intervals = Vec::with_capacity(audio.len() / 4);
        let mut last_cross_idx: Option<usize> = None;
        let mut prev_sign = audio[0] >= 0.0;

        for (i, &x) in audio.iter().enumerate() {
            total_energy += x * x;

            // Biquad Bandpass filter (300 - 3000 Hz)
            let y = self.bp_b0 * x + self.bp_b2 * self.bp_state_2
                - self.bp_a1 * self.bp_state_1
                - self.bp_a2 * self.bp_state_2;

            self.bp_state_2 = self.bp_state_1;
            self.bp_state_1 = y;

            voice_band_energy += y * y;

            // Decimate bandpassed audio by 4 (~12 kHz effective rate) for pitch autocorrelation
            if i % 4 == 0 {
                self.decim_buf.push(y);
            }

            let cur_sign = x >= 0.0;
            if cur_sign != prev_sign {
                if let Some(last_i) = last_cross_idx {
                    let interval = (i - last_i) as f32;
                    if interval >= 2.0 {
                        zero_crossing_intervals.push(interval);
                    }
                }
                last_cross_idx = Some(i);
                prev_sign = cur_sign;
            }
        }

        // Maintain circular decimated history buffer
        if self.decim_buf.len() > 512 {
            let overflow = self.decim_buf.len() - 384;
            self.decim_buf.drain(0..overflow);
        }

        let n = audio.len() as f32;
        let rms = (total_energy / n).sqrt();
        let voice_ratio = (voice_band_energy / (total_energy + 1e-7)).clamp(0.0, 1.0);

        // Track short-time RMS history over consecutive frames
        self.rms_history[self.rms_history_idx] = rms;
        self.rms_history_idx = (self.rms_history_idx + 1) % self.rms_history.len();

        let avg_rms: f32 = self.rms_history.iter().sum::<f32>() / self.rms_history.len() as f32;
        let rms_variance: f32 = self.rms_history.iter().map(|&r| {
            let d = r - avg_rms;
            d * d
        }).sum::<f32>() / self.rms_history.len() as f32;

        // Zero-crossing interval statistics (Tonal & Clock Buzz Detector)
        let mut cv = 0.5f32;
        let mut f_cross = 1000.0f32;
        if zero_crossing_intervals.len() >= 6 {
            let k = zero_crossing_intervals.len() as f32;
            let mean_interval: f32 = zero_crossing_intervals.iter().sum::<f32>() / k;
            if mean_interval > 1.0 {
                f_cross = self.audio_sample_rate / (2.0 * mean_interval);
                let var_interval: f32 = zero_crossing_intervals.iter().map(|&iv| {
                    let d = iv - mean_interval;
                    d * d
                }).sum::<f32>() / k;
                cv = var_interval.sqrt() / mean_interval;
            }
        }

        // Tonal / Clock Buzz test:
        // Pure tones, square waves, or USB clock buzz (1 kHz harmonic) have machine-precision zero-crossings
        // (cv < 0.12). Human speech has rich harmonics and turbulent noise where cv is > 0.25!
        // Also high-frequency whistles (f_cross > 450 Hz) or low 50Hz hum (f_cross < 65 Hz) with low jitter
        let is_pure_tone_buzz = (cv < 0.12 && rms > 0.008) || (cv < 0.22 && (f_cross > 450.0 || f_cross < 65.0) && rms > 0.008);
        let is_flat_machine_envelope = rms_variance < 0.00015 && avg_rms > 0.015;

        // Normalized Autocorrelation Function (NACF) for Vocal Cord Pitch Periodicity (80 - 350 Hz)
        let f_decim = self.audio_sample_rate / 4.0;
        let min_pitch_lag = (f_decim / 350.0).floor() as usize; // ~34 at 12 kHz
        let max_pitch_lag = (f_decim / 80.0).ceil() as usize;   // ~150 at 12 kHz
        let mut max_nacf = 0.0f32;
        let mut is_1khz_clock = false;

        let n_corr_pts = 100;
        if self.decim_buf.len() >= max_pitch_lag + n_corr_pts {
            let base_idx = self.decim_buf.len() - max_pitch_lag - n_corr_pts;
            let x_base = &self.decim_buf[base_idx..base_idx + n_corr_pts];
            let mut e0 = 0.0f32;
            for &s in x_base {
                e0 += s * s;
            }

            if e0 > 1e-6 {
                // USB Full-Speed 1 kHz frame clock check (lag 12 at 12 kHz)
                let x_1k = &self.decim_buf[base_idx + 12..base_idx + 12 + n_corr_pts];
                let mut cross_1k = 0.0f32;
                let mut e_1k = 0.0f32;
                for j in 0..n_corr_pts {
                    cross_1k += x_base[j] * x_1k[j];
                    e_1k += x_1k[j] * x_1k[j];
                }
                let denom_1k = (e0 * e_1k).sqrt();
                if denom_1k > 1e-6 && (cross_1k / denom_1k) > 0.65 {
                    is_1khz_clock = true;
                }

                for lag in min_pitch_lag..=max_pitch_lag {
                    let x_lag = &self.decim_buf[base_idx + lag..base_idx + lag + n_corr_pts];
                    let mut cross = 0.0f32;
                    let mut elag = 0.0f32;
                    for j in 0..n_corr_pts {
                        cross += x_base[j] * x_lag[j];
                        elag += x_lag[j] * x_lag[j];
                    }
                    let denom = (e0 * elag).sqrt();
                    if denom > 1e-6 {
                        let nacf = cross / denom;
                        if nacf > max_nacf {
                            max_nacf = nacf;
                        }
                    }
                }
            }
        }

        self.is_tone_buzz = is_pure_tone_buzz || is_flat_machine_envelope || is_1khz_clock;

        // Peak / Trough envelope tracking for syllabic dynamics
        if rms > self.env_peak {
            self.env_peak = rms;
        } else {
            self.env_peak *= 0.96; // Decay
        }

        if rms < self.env_trough || self.env_trough < 1e-5 {
            self.env_trough = rms;
        } else {
            self.env_trough = self.env_trough * 0.98 + rms * 0.02;
        }

        let env_dynamic_range = (self.env_peak - self.env_trough) / (self.env_peak + 1e-5);

        // Speech qualification criteria:
        // 1. RMS above background noise (> 0.006)
        // 2. NOT a stationary tone or USB clock buzz
        // 3. NOT flat stationary machine noise
        // 4. Voice band energy ratio > 0.30
        // 5. Significant cycle-to-cycle variation (cv > 0.16)
        // 6. Vocal cord fundamental pitch periodicity (max_nacf >= 0.40)
        let is_voiced = max_nacf >= 0.40 && !is_pure_tone_buzz && !is_flat_machine_envelope && !is_1khz_clock && voice_ratio > 0.30 && cv > 0.16;

        let instant_score = if rms > 0.006 && is_voiced {
            self.speech_frames_count = (self.speech_frames_count + 1).min(10);
            let dyn_factor = env_dynamic_range.clamp(0.1, 1.0);
            (max_nacf * 0.50 + voice_ratio * 0.25 + dyn_factor * 0.25).clamp(0.0, 1.0)
        } else {
            self.speech_frames_count = self.speech_frames_count.saturating_sub(2);
            0.0
        };

        // Fast attack (45%), smooth release (12%) for speech stability
        if instant_score > self.running_vad_score {
            self.running_vad_score = self.running_vad_score * 0.55 + instant_score * 0.45;
        } else {
            self.running_vad_score = self.running_vad_score * 0.88 + instant_score * 0.12;
        }
        self.running_vad_score.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vad_rejects_1khz_usb_clock_buzz() {
        let mut vad = VoiceActivityDetector::new(48_000.0);
        let sample_rate = 48_000.0;
        let mut score = 0.0;

        // Feed 1 second of pure 1000 Hz square/sine wave (USB frame clock harmonic)
        for _ in 0..40 {
            let mut block = vec![0.0f32; 1024];
            for (i, sample) in block.iter_mut().enumerate() {
                let t = i as f32 / sample_rate;
                // 1 kHz buzz with strong harmonics
                *sample = (2.0 * PI * 1000.0 * t).sin() * 0.5;
            }
            score = vad.evaluate(&block);
        }

        assert!(vad.is_tone_or_buzz(), "1 kHz USB harmonic must be flagged as tone/buzz!");
        assert!(score < 0.15, "1 kHz clock buzz must NOT be recognized as human voice! Score: {}", score);
    }

    #[test]
    fn test_vad_rejects_short_data_chirp_burst() {
        let mut vad = VoiceActivityDetector::new(48_000.0);
        let sample_rate = 48_000.0;

        // Feed silence (carrier)
        for _ in 0..10 {
            let block = vec![0.001f32; 1024];
            vad.evaluate(&block);
        }

        // Feed a single 50ms (2400 samples) high-frequency FSK data burst
        let mut chirp = vec![0.0f32; 2400];
        for (i, sample) in chirp.iter_mut().enumerate() {
            let t = i as f32 / sample_rate;
            *sample = (2.0 * PI * 2200.0 * t).sin() * 0.7;
        }
        let score = vad.evaluate(&chirp);

        assert!(score < 0.25, "Short isolated data chirp must NOT trigger high voice confidence! Score: {}", score);
    }

    #[test]
    fn test_vad_accepts_dynamic_speech() {
        let mut vad = VoiceActivityDetector::new(48_000.0);
        let sample_rate = 48_000.0;
        let mut score = 0.0;

        // Feed 1 second of multi-harmonic voice-like signal (130 Hz pitch + formants at 600, 1200, 2400 Hz)
        // with dynamic syllabic amplitude modulation at 3 Hz
        for frame in 0..40 {
            let mut block = vec![0.0f32; 1024];
            let frame_t = frame as f32 * (1024.0 / sample_rate);
            let syllabic_env = 0.5 + 0.5 * (2.0 * PI * 3.0 * frame_t).sin().abs();

            for (i, sample) in block.iter_mut().enumerate() {
                let t = frame_t + (i as f32 / sample_rate);
                let f0 = (2.0 * PI * 130.0 * t).sin();
                let f1 = 0.6 * (2.0 * PI * 650.0 * t).sin();
                let f2 = 0.4 * (2.0 * PI * 1300.0 * t).sin();
                let f3 = 0.2 * (2.0 * PI * 2200.0 * t).sin();
                *sample = (f0 + f1 + f2 + f3) * 0.3 * syllabic_env;
            }
            score = vad.evaluate(&block);
        }

        assert!(score >= 0.40, "Dynamic multi-harmonic voice must achieve high VAD confidence! Score: {}", score);
    }

    #[test]
    fn test_vad_rejects_broadband_noise_hash() {
        let mut vad = VoiceActivityDetector::new(48_000.0);
        let mut score = 0.0;

        // Feed 1 second of pseudo-random hash/noise (like 450 MHz discriminator hash)
        let mut lcg: u64 = 123456789;
        for _ in 0..40 {
            let mut block = vec![0.0f32; 1024];
            for sample in block.iter_mut() {
                lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1);
                let val = ((lcg >> 33) as f32 / 2147483648.0) - 1.0;
                *sample = val * 0.4;
            }
            score = vad.evaluate(&block);
        }

        assert!(score < 0.20, "Broadband random noise/hash must NOT be recognized as speech! Score: {}", score);
    }
}

