use num_complex::Complex32;
use std::f32::consts::PI;

pub struct DigitalDownConverter {
    phase: f64,
    phase_inc: f64,
    nco_phasor: Complex32,
    nco_rot_step: Complex32,
    sample_count: usize,
    sample_rate_in: f32,
    sample_rate_out: f32,
    resample_accum: f32,
    resample_step: f32,
    lp_state_1: Complex32,
    lp_state_2: Complex32,
    lowpass_alpha: f32,
}

impl DigitalDownConverter {
    pub fn new(sample_rate_in: f32, sample_rate_out: f32, channel_bw_hz: f32) -> Self {
        // 2-pole lowpass filter with cutoff at half of channel bandwidth
        let cutoff = (channel_bw_hz * 0.5).max(3000.0);
        let rc = 1.0 / (2.0 * PI * cutoff);
        let dt = 1.0 / sample_rate_in;
        let alpha = dt / (rc + dt);

        Self {
            phase: 0.0,
            phase_inc: 0.0,
            nco_phasor: Complex32::new(1.0, 0.0),
            nco_rot_step: Complex32::new(1.0, 0.0),
            sample_count: 0,
            sample_rate_in,
            sample_rate_out,
            resample_accum: 0.0,
            resample_step: sample_rate_in / sample_rate_out,
            lp_state_1: Complex32::default(),
            lp_state_2: Complex32::default(),
            lowpass_alpha: alpha.clamp(0.0005, 0.5),
        }
    }

    pub fn set_output_rate(&mut self, sample_rate_out: f32) {
        if (self.sample_rate_out - sample_rate_out).abs() > 1.0 {
            self.sample_rate_out = sample_rate_out;
            self.resample_step = self.sample_rate_in / sample_rate_out;
            self.resample_accum = 0.0;
        }
    }

    pub fn set_bandwidth(&mut self, channel_bw_hz: f32) {
        let cutoff = (channel_bw_hz * 0.5).max(3000.0);
        let rc = 1.0 / (2.0 * PI * cutoff);
        let dt = 1.0 / self.sample_rate_in;
        let alpha = dt / (rc + dt);
        self.lowpass_alpha = alpha.clamp(0.0005, 0.5);
    }

    pub fn set_freq_offset(&mut self, offset_hz: f32) {
        let inc = -2.0 * std::f64::consts::PI * (offset_hz as f64) / (self.sample_rate_in as f64);
        self.phase_inc = inc;
        let f32_inc = inc as f32;
        self.nco_rot_step = Complex32::new(f32_inc.cos(), f32_inc.sin());
        self.nco_phasor = Complex32::new(1.0, 0.0);
        self.phase = 0.0;
        self.sample_count = 0;
        self.lp_state_1 = Complex32::default();
        self.lp_state_2 = Complex32::default();
    }

    /// High-performance DDC processing with smooth phasor rotation and clean 2-pole filtering
    #[inline]
    pub fn process(&mut self, input: &[Complex32], output: &mut Vec<Complex32>) {
        output.clear();

        let step = self.resample_step;
        let rot = self.nco_rot_step;
        let alpha = self.lowpass_alpha;
        let inv_alpha = 1.0 - alpha;
        let two_pi = 2.0 * std::f64::consts::PI;

        for &sample in input {
            // Phasor multiplication: 4 multiplies, 0 sin/cos per sample!
            let mixed = sample * self.nco_phasor;
            self.nco_phasor *= rot;

            self.sample_count += 1;
            if self.sample_count >= 256 {
                // Re-align phase from 64-bit precision accumulator (zero glitch, zero drift)
                self.phase = (self.phase + self.phase_inc * 256.0) % two_pi;
                self.nco_phasor = Complex32::new(self.phase.cos() as f32, self.phase.sin() as f32);
                self.sample_count = 0;
            }

            // 2-pole cascaded low-pass filter (-12 dB/octave channel selectivity)
            self.lp_state_1 = self.lp_state_1 * inv_alpha + mixed * alpha;
            self.lp_state_2 = self.lp_state_2 * inv_alpha + self.lp_state_1 * alpha;

            // Decimation to target audio sample rate
            self.resample_accum += 1.0;
            if self.resample_accum >= step {
                self.resample_accum -= step;
                output.push(self.lp_state_2);
            }
        }
    }
}
