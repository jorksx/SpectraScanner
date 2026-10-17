use super::SdrDevice;
use crossbeam_channel::Sender;
use num_complex::Complex32;
use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub struct MockDevice {
    running: Arc<AtomicBool>,
    worker_handle: Option<thread::JoinHandle<()>>,
    frequency_hz: Arc<AtomicU64>,
    sample_rate: u32,
    gain: u32,
}

impl MockDevice {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            worker_handle: None,
            frequency_hz: Arc::new(AtomicU64::new(122_500_000)),
            sample_rate: 2_500_000,
            gain: 15,
        }
    }
}

impl SdrDevice for MockDevice {
    fn name(&self) -> &str {
        "Test Generator (Simulated)"
    }

    fn set_frequency(&mut self, freq_hz: u64) -> Result<(), String> {
        self.frequency_hz.store(freq_hz, Ordering::Relaxed);
        Ok(())
    }

    fn set_sample_rate(&mut self, rate_hz: u32) -> Result<(), String> {
        self.sample_rate = rate_hz;
        Ok(())
    }

    fn set_gain(&mut self, gain: u32) -> Result<(), String> {
        self.gain = gain;
        Ok(())
    }

    fn start(&mut self, sample_sender: Sender<Vec<Complex32>>) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        let running_flag = self.running.clone();
        running_flag.store(true, Ordering::SeqCst);

        let freq_atomic = self.frequency_hz.clone();
        let sample_rate = self.sample_rate as f32;

        let handle = thread::spawn(move || {
            let chunk_size = 16384;
            let mut phase_noise = 0.0f32;
            let mut phase_sig1 = 0.0f32;
            let mut phase_sig2 = 0.0f32;
            let mut phase_spur = 0.0f32;
            let mut audio_phase = 0.0f32;

            let start_time = Instant::now();

            while running_flag.load(Ordering::SeqCst) {
                let loop_start = Instant::now();
                let elapsed = start_time.elapsed().as_secs_f32();
                let center_freq = freq_atomic.load(Ordering::Relaxed) as f64;

                // Signal 1 at 118.100 MHz (AM Voice, transmits for 4s, pauses for 3s)
                let sig1_active = (elapsed % 7.0) < 4.0;
                let offset1 = (118_100_000f64 - center_freq) as f32;
                let dphase1 = 2.0 * PI * offset1 / sample_rate;

                // Signal 2 at 121.300 MHz (AM Voice, transmits for 5s, pauses for 4s)
                let sig2_active = ((elapsed + 3.0) % 9.0) < 5.0;
                let offset2 = (121_300_000f64 - center_freq) as f32;
                let dphase2 = 2.0 * PI * offset2 / sample_rate;

                // Unmodulated spur at 125.000 MHz (Constant carrier - NO voice)
                let offset_spur = (125_000_000f64 - center_freq) as f32;
                let dphase_spur = 2.0 * PI * offset_spur / sample_rate;

                let mut buffer = Vec::with_capacity(chunk_size);

                for _ in 0..chunk_size {
                    // Simulated audio modulation (Voice formants: ~750 Hz and ~1200 Hz)
                    audio_phase += 2.0 * PI * 800.0 / sample_rate;
                    if audio_phase > 2.0 * PI {
                        audio_phase -= 2.0 * PI;
                    }
                    let voice_mod = 0.5 + 0.4 * audio_phase.sin();

                    // Simple pseudo-random thermal noise
                    phase_noise = (phase_noise * 1103515245.0 + 12345.0) % 2147483648.0;
                    let n_i = ((phase_noise / 2147483648.0) - 0.5) * 0.005;
                    let n_q = (((phase_noise * 3.14159) / 2147483648.0).fract() - 0.5) * 0.005;

                    let mut i = n_i;
                    let mut q = n_q;

                    // Add Signal 1 (AM Voice)
                    if sig1_active {
                        phase_sig1 += dphase1;
                        let amp1 = 0.08 * voice_mod;
                        i += amp1 * phase_sig1.cos();
                        q += amp1 * phase_sig1.sin();
                    }

                    // Add Signal 2 (AM Voice)
                    if sig2_active {
                        phase_sig2 += dphase2;
                        let amp2 = 0.06 * voice_mod;
                        i += amp2 * phase_sig2.cos();
                        q += amp2 * phase_sig2.sin();
                    }

                    // Add Spur (Dead carrier - should be filtered by VAD!)
                    phase_spur += dphase_spur;
                    let amp_spur = 0.05;
                    i += amp_spur * phase_spur.cos();
                    q += amp_spur * phase_spur.sin();

                    buffer.push(Complex32::new(i, q));
                }

                if sample_sender.send(buffer).is_err() {
                    break;
                }

                let chunk_duration = Duration::from_secs_f32(chunk_size as f32 / sample_rate);
                let proc_duration = loop_start.elapsed();
                if chunk_duration > proc_duration {
                    thread::sleep(chunk_duration - proc_duration);
                }
            }
        });

        self.worker_handle = Some(handle);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), String> {
        if !self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}
