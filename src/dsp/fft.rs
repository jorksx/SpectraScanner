use num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::f32::consts::PI;
use std::sync::Arc;

pub struct FftProcessor {
    fft: Arc<dyn Fft<f32>>,
    size: usize,
    window: Vec<f32>,
    scratch: Vec<Complex32>,
}

impl FftProcessor {
    pub fn new(size: usize) -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(size);

        // Precompute Blackman-Harris window
        let mut window = Vec::with_capacity(size);
        let a0 = 0.35875;
        let a1 = 0.48829;
        let a2 = 0.14128;
        let a3 = 0.01168;

        for i in 0..size {
            let theta = 2.0 * PI * (i as f32) / ((size - 1) as f32);
            let w = a0 - a1 * theta.cos() + a2 * (2.0 * theta).cos() - a3 * (3.0 * theta).cos();
            window.push(w);
        }

        Self {
            fft,
            size,
            window,
            scratch: vec![Complex32::default(); size],
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// Computes power spectrum in dBFS, centered (zero frequency in the middle)
    pub fn compute_spectrum(&mut self, samples: &[Complex32], out_db: &mut [f32]) {
        assert_eq!(samples.len(), self.size);
        assert_eq!(out_db.len(), self.size);

        // Apply window
        for i in 0..self.size {
            self.scratch[i] = samples[i] * self.window[i];
        }

        // In-place FFT
        self.fft.process(&mut self.scratch);

        // Compute power with FFT shift
        let half = self.size / 2;
        let norm = 1.0 / (self.size as f32);

        for i in 0..self.size {
            let shifted_idx = (i + half) % self.size;
            let c = self.scratch[shifted_idx] * norm;
            let pwr = c.norm_sqr().max(1e-12);
            out_db[i] = 10.0 * pwr.log10();
        }

        // Suppress central hardware DC offset spike (smoothly interpolate across DC notch window)
        let center_bin = half;
        if center_bin >= 3 && center_bin + 3 < self.size {
            let left = out_db[center_bin - 3];
            let right = out_db[center_bin + 3];
            for k in 1..=5 {
                let frac = k as f32 / 6.0;
                out_db[center_bin - 3 + k] = left * (1.0 - frac) + right * frac;
            }
        }

        // Edge roll-off guard: extend outer bins so analog filter cutoff does not plummet into math clamp
        let guard = (self.size / 64).max(8);
        for i in 0..guard {
            out_db[i] = out_db[guard];
            out_db[self.size - 1 - i] = out_db[self.size - 1 - guard];
        }
    }

    /// Computes power spectrum averaged across multiple consecutive windows.
    /// This drastically suppresses noise variance (by sqrt(N)) within a single batch,
    /// so the spectrum is immediately calibrated and steady upon retuning.
    pub fn compute_averaged_spectrum(&mut self, samples: &[Complex32], out_db: &mut [f32], max_windows: usize) {
        let n_win = (samples.len() / self.size).min(max_windows).max(1);
        if n_win == 1 {
            self.compute_spectrum(&samples[..self.size], out_db);
            return;
        }

        let mut sum_pwr = vec![0.0f32; self.size];
        let mut temp = vec![0.0f32; self.size];

        for w in 0..n_win {
            let start = w * self.size;
            let end = start + self.size;
            self.compute_spectrum(&samples[start..end], &mut temp);
            for i in 0..self.size {
                let pwr = 10.0f32.powf(temp[i] * 0.1);
                sum_pwr[i] += pwr;
            }
        }

        let norm = 1.0 / (n_win as f32);
        for i in 0..self.size {
            let avg_pwr = sum_pwr[i] * norm;
            out_db[i] = 10.0 * avg_pwr.max(1e-12).log10();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fft_frequency_mapping() {
        let size = 2048;
        let mut fft = FftProcessor::new(size);
        let sample_rate = 10_000_000.0f32;
        let test_offset = 1_000_000.0f32; // +1 MHz
        let mut samples = vec![Complex32::default(); size];

        for i in 0..size {
            let t = i as f32 / sample_rate;
            let phase = 2.0 * PI * test_offset * t;
            samples[i] = Complex32::new(phase.cos(), phase.sin());
        }

        let mut out_db = vec![0.0f32; size];
        fft.compute_spectrum(&samples, &mut out_db);

        // Find peak index
        let (peak_idx, peak_val) = out_db
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();

        // Expected bin: center is size/2 = 1024.
        // +1 MHz offset with 10 MHz BW = + (1M / 10M) * 2048 = +204.8 bins -> 1024 + 205 = 1229
        let expected_idx = 1024 + ((test_offset / sample_rate) * size as f32).round() as usize;
        println!("Peak found at bin {} with value {:.1} dBFS (expected ~{})", peak_idx, peak_val, expected_idx);
        assert!((peak_idx as isize - expected_idx as isize).abs() <= 1);
    }
}

