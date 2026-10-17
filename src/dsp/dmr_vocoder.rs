/// AMBE+2 Vocoder and DMR Voice Burst De-Interleaver
///
/// Implements bit de-interleaving per ETSI TS 102 361-2 Clause B.2.1
/// and interfaces with mbelib for AMBE 3600x2450 speech synthesis.

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MbeParms {
    pub w0: f32,
    pub l_val: i32,
    pub k_val: i32,
    pub vl: [i32; 57],
    pub ml: [f32; 57],
    pub log2ml: [f32; 57],
    pub phil: [f32; 57],
    pub psil: [f32; 57],
    pub gamma: f32,
    pub un: i32,
    pub repeat: i32,
}

impl Default for MbeParms {
    fn default() -> Self {
        Self {
            w0: 0.09378,
            l_val: 0,
            k_val: 0,
            vl: [0; 57],
            ml: [0.0; 57],
            log2ml: [0.0; 57],
            phil: [0.0; 57],
            psil: [0.0; 57],
            gamma: 0.0,
            un: 0,
            repeat: 0,
        }
    }
}

extern "C" {
    fn mbe_initMbeParms(
        cur_mp: *mut MbeParms,
        prev_mp: *mut MbeParms,
        prev_mp_enhanced: *mut MbeParms,
    );

    fn mbe_processAmbe3600x2450Framef(
        aout_buf: *mut f32,
        errs: *mut i32,
        errs2: *mut i32,
        err_str: *mut i8,
        ambe_fr: *mut [i8; 24],
        ambe_d: *mut i8,
        cur_mp: *mut MbeParms,
        prev_mp: *mut MbeParms,
        prev_mp_enhanced: *mut MbeParms,
        uvquality: i32,
    );
}

/// DMR AMBE Interleave Schedule Matrices (ETSI TS 102 361-2 Clause B.2.1)
pub const DMR_RW: [usize; 36] = [
    0, 1, 0, 1, 0, 1,
    0, 1, 0, 1, 0, 1,
    0, 1, 0, 1, 0, 1,
    0, 1, 0, 1, 0, 2,
    0, 2, 0, 2, 0, 2,
    0, 2, 0, 2, 0, 2,
];

pub const DMR_RX: [usize; 36] = [
    23, 10, 22, 9, 21, 8,
    20, 7, 19, 6, 18, 5,
    17, 4, 16, 3, 15, 2,
    14, 1, 13, 0, 12, 10,
    11, 9, 10, 8, 9, 7,
    8, 6, 7, 5, 6, 4,
];

pub const DMR_RY: [usize; 36] = [
    0, 2, 0, 2, 0, 2,
    0, 2, 0, 3, 0, 3,
    1, 3, 1, 3, 1, 3,
    1, 3, 1, 3, 1, 3,
    1, 3, 1, 3, 1, 3,
    1, 3, 1, 3, 1, 3,
];

pub const DMR_RZ: [usize; 36] = [
    5, 3, 4, 2, 3, 1,
    2, 0, 1, 13, 0, 12,
    22, 11, 21, 10, 20, 9,
    19, 8, 18, 7, 17, 6,
    16, 5, 15, 4, 14, 3,
    13, 2, 12, 1, 11, 0,
];

pub struct DmrVocoder {
    cur_mp: MbeParms,
    prev_mp: MbeParms,
    prev_mp_enhanced: MbeParms,
    gain: f32,
    max_history: [f32; 25],
    max_idx: usize,
    audio_temp_8k: [f32; 160],
}

impl DmrVocoder {
    pub fn new() -> Self {
        let mut cur_mp = MbeParms::default();
        let mut prev_mp = MbeParms::default();
        let mut prev_mp_enhanced = MbeParms::default();

        unsafe {
            mbe_initMbeParms(&mut cur_mp, &mut prev_mp, &mut prev_mp_enhanced);
        }

        Self {
            cur_mp,
            prev_mp,
            prev_mp_enhanced,
            gain: 25.0,
            max_history: [0.0; 25],
            max_idx: 0,
            audio_temp_8k: [0.0; 160],
        }
    }

    pub fn reset(&mut self) {
        unsafe {
            mbe_initMbeParms(&mut self.cur_mp, &mut self.prev_mp, &mut self.prev_mp_enhanced);
        }
        self.gain = 25.0;
        self.max_history.fill(0.0);
        self.max_idx = 0;
        self.audio_temp_8k.fill(0.0);
    }

    /// Decodes a single 72-bit (36 dibit) AMBE frame into 160 samples @ 8 kHz
    pub fn decode_frame_8k(&mut self, ambe_fr: &[[i8; 24]; 4], out_8k: &mut [f32; 160]) -> bool {
        let mut errs = 0i32;
        let mut errs2 = 0i32;
        let mut err_str = [0i8; 64];
        let mut ambe_d = [0i8; 49];

        unsafe {
            mbe_processAmbe3600x2450Framef(
                self.audio_temp_8k.as_mut_ptr(),
                &mut errs,
                &mut errs2,
                err_str.as_mut_ptr(),
                ambe_fr.as_ptr() as *mut [i8; 24],
                ambe_d.as_mut_ptr(),
                &mut self.cur_mp,
                &mut self.prev_mp,
                &mut self.prev_mp_enhanced,
                3, // uvquality
            );
        }

        // AGC (Automatic Gain Control)
        let mut frame_max: f32 = 0.0;
        for &s in &self.audio_temp_8k {
            let a = s.abs();
            if a > frame_max {
                frame_max = a;
            }
        }

        self.max_history[self.max_idx] = frame_max;
        self.max_idx = (self.max_idx + 1) % 25;

        let mut hist_max: f32 = 0.0;
        for &m in &self.max_history {
            if m > hist_max {
                hist_max = m;
            }
        }

        let target_gain = if hist_max > 0.0 {
            (28_000.0 / hist_max).clamp(2.0, 50.0)
        } else {
            25.0
        };

        // Smooth gain transition
        self.gain = self.gain * 0.95 + target_gain * 0.05;

        // Scale to normalized audio [-1.0, 1.0]
        let scale = self.gain / 32_768.0;
        for i in 0..160 {
            let val = self.audio_temp_8k[i] * scale;
            out_8k[i] = val.clamp(-0.95, 0.95);
        }

        errs < 4
    }

    /// De-interleaves and decodes all 3 voice frames from a 132-symbol DMR Voice Burst
    /// and appends 2880 upsampled samples @ 48 kHz to `out_audio_48k`.
    pub fn decode_voice_burst(
        &mut self,
        burst_symbols: &[u8; 132],
        out_audio_48k: &mut Vec<f32>,
    ) {
        // Extract 3 AMBE frames (36 dibits each = 72 bits each)
        // Frame 1: symbols 0..35
        // Frame 2: symbols 36..53 (first 18) and symbols 78..95 (second 18)
        // Frame 3: symbols 96..131
        let mut frames_dibits = [[0u8; 36]; 3];

        // Frame 1
        frames_dibits[0].copy_from_slice(&burst_symbols[0..36]);

        // Frame 2
        frames_dibits[1][0..18].copy_from_slice(&burst_symbols[36..54]);
        frames_dibits[1][18..36].copy_from_slice(&burst_symbols[78..96]);

        // Frame 3
        frames_dibits[2].copy_from_slice(&burst_symbols[96..132]);

        let mut samples_8k = [0.0f32; 160];

        for frame_idx in 0..3 {
            let mut ambe_fr = [[0i8; 24]; 4];

            // De-interleave using ETSI DMR tables
            for i in 0..36 {
                let dibit = frames_dibits[frame_idx][i];
                let bit1 = ((dibit >> 1) & 1) as i8;
                let bit0 = (dibit & 1) as i8;

                ambe_fr[DMR_RW[i]][DMR_RX[i]] = bit1;
                ambe_fr[DMR_RY[i]][DMR_RZ[i]] = bit0;
            }

            self.decode_frame_8k(&ambe_fr, &mut samples_8k);

            // Resample 8 kHz -> 48 kHz (6x interpolation)
            // Each 8 kHz sample expands to 6 samples at 48 kHz
            for i in 0..160 {
                let s0 = samples_8k[i];
                let s1 = if i + 1 < 160 { samples_8k[i + 1] } else { s0 };
                for k in 0..6 {
                    let frac = k as f32 / 6.0;
                    let sample_48k = s0 + frac * (s1 - s0);
                    out_audio_48k.push(sample_48k);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dmr_vocoder_init_and_decode() {
        let mut vocoder = DmrVocoder::new();
        let burst_symbols = [0u8; 132];
        let mut out_audio = Vec::new();

        vocoder.decode_voice_burst(&burst_symbols, &mut out_audio);

        // 3 frames * 160 samples * 6 = 2880 samples @ 48 kHz
        assert_eq!(out_audio.len(), 2880);
        for &s in &out_audio {
            assert!(s.is_finite(), "Vocoder samples must be finite numbers");
            assert!(s.abs() <= 1.0, "Vocoder samples must be within [-1, 1]");
        }
    }

    #[test]
    fn test_dmr_vocoder_stress_random_patterns() {
        let mut vocoder = DmrVocoder::new();
        let mut out_audio = Vec::new();

        for seed in 0..5000 {
            let mut burst_symbols = [0u8; 132];
            for i in 0..132 {
                burst_symbols[i] = ((seed * 37 + i * 19 + (seed ^ i)) % 4) as u8;
            }
            out_audio.clear();
            vocoder.decode_voice_burst(&burst_symbols, &mut out_audio);
            assert_eq!(out_audio.len(), 2880);
        }
    }
}
