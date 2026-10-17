pub struct DynamicNoiseFloor {
    size: usize,
    radius: usize,
    temporal_alpha: f32,
    noise_floor_baseline: Vec<f32>,
    temp_min: Vec<f32>,
    is_initialized: bool,
}

impl DynamicNoiseFloor {
    pub fn new(size: usize, radius_bins: usize) -> Self {
        Self {
            size,
            radius: radius_bins,
            temporal_alpha: 0.15, // Fast adaptive tracking
            noise_floor_baseline: vec![-100.0; size],
            temp_min: vec![-100.0; size],
            is_initialized: false,
        }
    }

    /// Resets the baseline state (called on band switch or frequency hop)
    pub fn reset(&mut self) {
        self.is_initialized = false;
    }

    /// Calculates Dynamic Noise Floor and returns (pseudospectrum, noise_baseline)
    pub fn process(&mut self, spectrum_db: &[f32], pseudo_out: &mut [f32]) {
        assert_eq!(spectrum_db.len(), self.size);
        assert_eq!(pseudo_out.len(), self.size);

        let r = self.radius;

        // Step 1: 1D Morphological Erosion (running minimum)
        for i in 0..self.size {
            let start = i.saturating_sub(r);
            let end = (i + r + 1).min(self.size);
            let mut min_val = f32::INFINITY;
            for &val in &spectrum_db[start..end] {
                if val < min_val {
                    min_val = val;
                }
            }
            self.temp_min[i] = min_val;
        }

        // Step 2: 1D Morphological Dilation (running maximum of minimums)
        // to form the morphological opening baseline
        let is_first_run = !self.is_initialized;

        for i in 0..self.size {
            let start = i.saturating_sub(r);
            let end = (i + r + 1).min(self.size);
            let mut max_val = f32::NEG_INFINITY;
            for &val in &self.temp_min[start..end] {
                if val > max_val {
                    max_val = val;
                }
            }

            // Step 3: Temporal IIR smoothing to eliminate frame flutter
            // (on first frame initialize immediately to avoid transient delay)
            let smoothed = if is_first_run {
                max_val
            } else {
                (1.0 - self.temporal_alpha) * self.noise_floor_baseline[i]
                    + self.temporal_alpha * max_val
            };
            self.noise_floor_baseline[i] = smoothed;

            // Step 4: Calibrated Pseudospectrum
            // Morphological opening baseline aligns background noise at ~0 dB
            let snr = spectrum_db[i] - smoothed;
            pseudo_out[i] = snr.max(0.0);
        }

        self.is_initialized = true;
    }

    pub fn baseline(&self) -> &[f32] {
        &self.noise_floor_baseline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deep_noise_floor_does_not_shelf_or_diverge() {
        let size = 2048;
        let mut dnf = DynamicNoiseFloor::new(size, 25);
        let mut pseudo = vec![0.0f32; size];

        // Simulate low RF gain where ADC/thermal noise floor drops to -105 dBFS
        let deep_noise = vec![-105.0f32; size];

        for _ in 0..10 {
            dnf.process(&deep_noise, &mut pseudo);
        }

        // Pseudospectrum noise floor must sit around <= 0.0 dB (not blown up to +15..+30 dB!)
        for (i, &snr) in pseudo.iter().enumerate() {
            assert!(
                snr <= 0.5,
                "Bin {} with deep noise floor has excessive SNR: {} dB (shelf bug)",
                i,
                snr
            );
        }

        // Test with a +20 dB carrier signal at bin 500 (-85 dBFS)
        let mut signal_spectrum = deep_noise.clone();
        signal_spectrum[500] = -85.0;
        dnf.process(&signal_spectrum, &mut pseudo);

        assert!(
            pseudo[500] >= 10.0,
            "Carrier signal at bin 500 should have clear positive SNR: {} dB",
            pseudo[500]
        );
    }
}
