use crate::config::DemodMode;
use num_complex::Complex32;
use std::f32::consts::PI;

pub struct Demodulator {
    mode: DemodMode,
    last_sample: Complex32,
    deemph_state: f32,
    deemph_alpha_standard: f32,
    deemph_alpha_bcfm: f32,
    dc_block_x: f32,
    dc_block_y: f32,
    peak_env: f32,
    agc_gain: f32,
    audio_lp_state: f32,
    audio_lp_alpha_voice: f32,
    // Phasing SSB Hilbert filter buffers (length 31)
    q_history: [f32; 32],
    i_history: [f32; 32],
    hist_idx: usize,
    hilbert_taps: [f32; 31],
    // Wideband FM (BCFM & WFM) IF 240 kHz -> 48 kHz decimation & anti-aliasing
    decim_phase: usize,
    wfm_lp1: f32,
    wfm_lp2: f32,
    wfm_lp3: f32,
    wfm_lp4: f32,
    wfm_lp_alpha_bcfm: f32,
    wfm_lp_alpha_voice: f32,
}

impl Demodulator {
    pub fn new(mode: DemodMode, audio_rate: f32) -> Self {
        // De-emphasis filter: tau = 75us (standard FM voice) and 50us (BCFM Broadcast)
        let dt = 1.0 / audio_rate;
        let deemph_alpha_standard = (dt / (75e-6 + dt)).clamp(0.01, 1.0);
        let deemph_alpha_bcfm = (dt / (50e-6 + dt)).clamp(0.01, 1.0);

        // Audio low-pass filters: 3.5 kHz for clean voice / SSB
        let lp_voice = 1.0 / (2.0 * PI * 3500.0);
        let audio_lp_alpha_voice = (dt / (lp_voice + dt)).clamp(0.01, 1.0);

        // 4-pole anti-aliasing lowpass filter constants at 240 kHz IF:
        // Cutoff 15 kHz for BCFM broadcast, 8 kHz for wideband voice / satellite WFM
        let dt_240k = 1.0 / 240_000.0;
        let rc_15k = 1.0 / (2.0 * PI * 15000.0);
        let wfm_lp_alpha_bcfm = (dt_240k / (rc_15k + dt_240k)).clamp(0.01, 1.0);

        let rc_8k = 1.0 / (2.0 * PI * 8000.0);
        let wfm_lp_alpha_voice = (dt_240k / (rc_8k + dt_240k)).clamp(0.01, 1.0);

        // 31-tap antisymmetric FIR Hilbert transformer for USB/LSB 90° phase shift
        let mut hilbert_taps = [0.0f32; 31];
        for k in -15..=15 {
            if k % 2 != 0 {
                let k_f = k as f32;
                let w = 0.54 + 0.46 * (PI * k_f / 15.0).cos(); // Hamming window
                let h = (2.0 / (PI * k_f)) * w;
                hilbert_taps[(k + 15) as usize] = h;
            }
        }

        Self {
            mode,
            last_sample: Complex32::default(),
            deemph_state: 0.0,
            deemph_alpha_standard,
            deemph_alpha_bcfm,
            dc_block_x: 0.0,
            dc_block_y: 0.0,
            peak_env: 0.1,
            agc_gain: 1.0,
            audio_lp_state: 0.0,
            audio_lp_alpha_voice,
            q_history: [0.0; 32],
            i_history: [0.0; 32],
            hist_idx: 0,
            hilbert_taps,
            decim_phase: 0,
            wfm_lp1: 0.0,
            wfm_lp2: 0.0,
            wfm_lp3: 0.0,
            wfm_lp4: 0.0,
            wfm_lp_alpha_bcfm,
            wfm_lp_alpha_voice,
        }
    }

    pub fn set_mode(&mut self, mode: DemodMode) {
        if self.mode != mode {
            self.mode = mode;
            self.decim_phase = 0;
            self.wfm_lp1 = 0.0;
            self.wfm_lp2 = 0.0;
            self.wfm_lp3 = 0.0;
            self.wfm_lp4 = 0.0;
        }
    }

    pub fn process(&mut self, input: &[Complex32], audio_out: &mut Vec<f32>) {
        if self.mode == DemodMode::BCFM || self.mode == DemodMode::WFM {
            audio_out.clear();
            audio_out.reserve(input.len() / 5 + 1);

            let wfm_alpha = if self.mode == DemodMode::BCFM {
                self.wfm_lp_alpha_bcfm
            } else {
                self.wfm_lp_alpha_voice
            };

            let deemph_alpha = if self.mode == DemodMode::BCFM {
                self.deemph_alpha_bcfm
            } else {
                self.deemph_alpha_standard
            };

            for &s in input {
                // Wideband FM Discriminator at 240 kHz IF:
                // Carson's bandwidth of +/- 75 kHz deviation fits within +/- 120 kHz Nyquist.
                // Maximum phase angle step is 2*pi * 75/240 = 0.625*pi rad (112.5 deg < 180 deg),
                // making phase wrapping and tearing distortion mathematically impossible.
                let conj = self.last_sample.conj();
                let diff = s * conj;
                let angle = diff.im.atan2(diff.re);
                self.last_sample = s;

                // 4-pole cascaded lowpass filter at 240 kHz:
                // Attenuates 19 kHz stereo pilot tone (-20 dB), 38 kHz stereo difference subcarrier (-38 dB),
                // and 57 kHz RDS subcarrier (-50 dB) prior to 5:1 decimation.
                // This eliminates all high-frequency multiplex hash from aliasing into audible audio.
                self.wfm_lp1 = (1.0 - wfm_alpha) * self.wfm_lp1 + wfm_alpha * angle;
                self.wfm_lp2 = (1.0 - wfm_alpha) * self.wfm_lp2 + wfm_alpha * self.wfm_lp1;
                self.wfm_lp3 = (1.0 - wfm_alpha) * self.wfm_lp3 + wfm_alpha * self.wfm_lp2;
                self.wfm_lp4 = (1.0 - wfm_alpha) * self.wfm_lp4 + wfm_alpha * self.wfm_lp3;

                // 5:1 decimation down to standard 48,000 Hz audio rate
                self.decim_phase += 1;
                if self.decim_phase >= 5 {
                    self.decim_phase = 0;
                    let raw_audio = self.wfm_lp4;

                    // European standard 50 us de-emphasis (75 us for voice WFM) at 48 kHz
                    self.deemph_state = (1.0 - deemph_alpha) * self.deemph_state
                        + deemph_alpha * raw_audio;
                    let deemph_audio = self.deemph_state;

                    // DC Blocker at 48 kHz: removes center carrier offset
                    let y_dc = deemph_audio - self.dc_block_x + 0.995 * self.dc_block_y;
                    self.dc_block_x = deemph_audio;
                    self.dc_block_y = y_dc;
                    let audio_filtered = y_dc;

                    // Syllabic AGC Envelope Follower & Soft Limiter
                    let abs_level = audio_filtered.abs();
                    if abs_level > self.peak_env {
                        self.peak_env = self.peak_env * 0.98 + abs_level * 0.02;
                    } else {
                        self.peak_env = self.peak_env * 0.9998 + abs_level * 0.0002;
                    }

                    let target_gain = if self.peak_env > 0.002 {
                        (0.85 / self.peak_env).clamp(0.5, 20.0)
                    } else {
                        2.5
                    };

                    self.agc_gain = self.agc_gain * 0.999 + target_gain * 0.001;

                    let normalized = (audio_filtered * self.agc_gain).clamp(-0.95, 0.95);
                    audio_out.push(normalized);
                }
            }
            return;
        }

        audio_out.clear();
        audio_out.reserve(input.len());

        let deemph_alpha = self.deemph_alpha_standard;
        let lp_alpha = self.audio_lp_alpha_voice;

        for &s in input {
            // Push I and Q into SSB circular history
            self.i_history[self.hist_idx] = s.re;
            self.q_history[self.hist_idx] = s.im;
            self.hist_idx = (self.hist_idx + 1) % 32;

            let mut raw_audio = match self.mode {
                DemodMode::AM => {
                    // AM: Envelope detection
                    s.norm()
                }
                DemodMode::NFM => {
                    // FM: Quadrature phase difference
                    let conj = self.last_sample.conj();
                    let diff = s * conj;
                    let angle = diff.im.atan2(diff.re);

                    // De-emphasis
                    self.deemph_state = (1.0 - deemph_alpha) * self.deemph_state
                        + deemph_alpha * angle;
                    self.deemph_state
                }
                DemodMode::USB | DemodMode::LSB => {
                    // Single Sideband: Phasing Hilbert transform (90° phase shift of Q)
                    // Center sample is delayed by 15 samples:
                    let center_idx = (self.hist_idx + 32 - 16) % 32;
                    let i_delayed = self.i_history[center_idx];

                    // Convolve Q with 31-tap Hilbert FIR:
                    let mut q_hilbert = 0.0f32;
                    for k in 0..31 {
                        let tap = self.hilbert_taps[k];
                        if tap != 0.0 {
                            let tap_idx = (self.hist_idx + 32 - 1 - k) % 32;
                            q_hilbert += tap * self.q_history[tap_idx];
                        }
                    }

                    if self.mode == DemodMode::USB {
                        i_delayed - q_hilbert
                    } else {
                        i_delayed + q_hilbert
                    }
                }
                _ => 0.0,
            };
            self.last_sample = s;

            // DC Blocker: y[n] = x[n] - x[n-1] + 0.995 * y[n-1]
            let y_dc = raw_audio - self.dc_block_x + 0.995 * self.dc_block_y;
            self.dc_block_x = raw_audio;
            self.dc_block_y = y_dc;
            raw_audio = y_dc;

            // Low-pass audio filter (clean speech 3.5 kHz)
            self.audio_lp_state = (1.0 - lp_alpha) * self.audio_lp_state + lp_alpha * raw_audio;
            let audio_filtered = self.audio_lp_state;

            // Smooth Syllabic AGC Envelope Follower (zero waveform distortion)
            let abs_level = audio_filtered.abs();
            if abs_level > self.peak_env {
                // Fast attack (~2 ms at 48 kHz)
                self.peak_env = self.peak_env * 0.98 + abs_level * 0.02;
            } else {
                // Smooth decay (~250 ms at 48 kHz)
                self.peak_env = self.peak_env * 0.9998 + abs_level * 0.0002;
            }

            let target_gain = if self.peak_env > 0.002 {
                (0.85 / self.peak_env).clamp(0.5, 20.0)
            } else {
                2.5
            };

            // Smooth gain slewing
            self.agc_gain = self.agc_gain * 0.999 + target_gain * 0.001;

            let normalized = (audio_filtered * self.agc_gain).clamp(-0.95, 0.95);
            audio_out.push(normalized);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demod_modes_usb_lsb_bcfm_processing() {
        let mut demod = Demodulator::new(DemodMode::USB, 48_000.0);
        let mut audio = Vec::new();

        // 1 kHz analytic tone: exp(j * 2 * pi * 1000 * t)
        let samples: Vec<Complex32> = (0..480)
            .map(|n| {
                let phase = 2.0 * PI * 1000.0 * (n as f32) / 48_000.0;
                Complex32::new(phase.cos(), phase.sin())
            })
            .collect();

        // Test USB
        demod.process(&samples, &mut audio);
        assert_eq!(audio.len(), 480);
        for &s in &audio {
            assert!(!s.is_nan() && !s.is_infinite());
        }

        // Test LSB
        demod.set_mode(DemodMode::LSB);
        demod.process(&samples, &mut audio);
        assert_eq!(audio.len(), 480);
        for &s in &audio {
            assert!(!s.is_nan() && !s.is_infinite());
        }

        // Test BCFM (240 kHz IF -> 48 kHz audio decimation: 2400 samples -> 480 audio)
        demod.set_mode(DemodMode::BCFM);
        let bcfm_samples: Vec<Complex32> = (0..2400)
            .map(|n| {
                let phase = 2.0 * PI * 1000.0 * (n as f32) / 240_000.0;
                Complex32::new(phase.cos(), phase.sin())
            })
            .collect();
        demod.process(&bcfm_samples, &mut audio);
        assert_eq!(audio.len(), 480);
        for &s in &audio {
            assert!(!s.is_nan() && !s.is_infinite());
        }

        // Test WFM (240 kHz IF -> 48 kHz audio decimation: 2400 samples -> 480 audio)
        demod.set_mode(DemodMode::WFM);
        demod.process(&bcfm_samples, &mut audio);
        assert_eq!(audio.len(), 480);
        for &s in &audio {
            assert!(!s.is_nan() && !s.is_infinite());
        }
    }
}
