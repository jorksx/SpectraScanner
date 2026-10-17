use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[allow(dead_code)]
pub struct AudioEngine {
    _stream: Stream,
    producer: Producer<f32>,
    is_active: Arc<AtomicBool>,
    pub device_name: String,
    pub output_sample_rate: u32,
}

/// Enumerates all currently available playback devices in Windows
pub fn get_output_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut names = vec!["Default (Windows System Output)".to_string()];
    if let Ok(devices) = host.output_devices() {
        for dev in devices {
            if let Ok(name) = dev.name() {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names
}

impl AudioEngine {
    /// Builds an audio engine for the given device name (or Windows system default)
    /// Automatically detects device native sample rate and resamples from SDR audio rate.
    pub fn new_with_device(device_name: Option<&str>, sdr_audio_rate: u32) -> Result<(Self, String), String> {
        let host = cpal::default_host();

        let (device, actual_name) = match device_name {
            Some(name) if name != "Default (Windows System Output)" => {
                let mut matched = None;
                if let Ok(devices) = host.output_devices() {
                    for dev in devices {
                        if let Ok(dname) = dev.name() {
                            if dname == name {
                                matched = Some((dev, dname));
                                break;
                            }
                        }
                    }
                }
                match matched {
                    Some(m) => m,
                    None => {
                        let def = host
                            .default_output_device()
                            .ok_or_else(|| "No default audio output device found in Windows".to_string())?;
                        let dname = def.name().unwrap_or_else(|_| "Default (Windows Output)".to_string());
                        (def, dname)
                    }
                }
            }
            _ => {
                let def = host
                    .default_output_device()
                    .ok_or_else(|| "No default audio output device found in Windows".to_string())?;
                let dname = def.name().unwrap_or_else(|_| "Default (Windows Output)".to_string());
                (def, dname)
            }
        };

        let supported_config = device
            .default_output_config()
            .map_err(|e| format!("Failed to get audio device config for '{}': {}", actual_name, e))?;

        let channels = supported_config.channels();
        let out_sample_rate = supported_config.sample_rate().0;
        let config = StreamConfig {
            channels,
            sample_rate: supported_config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        // Ring buffer for ~0.50 seconds of audio buffer
        let ring_size = (sdr_audio_rate as usize) / 2;
        let (producer, mut consumer): (Producer<f32>, Consumer<f32>) = RingBuffer::new(ring_size);

        let is_active = Arc::new(AtomicBool::new(true));
        let is_active_clone = is_active.clone();

        let err_fn = |err| eprintln!("Audio stream error: {}", err);

        let step = sdr_audio_rate as f32 / out_sample_rate as f32;
        let mut frac = 0.0f32;
        let mut prev = 0.0f32;
        let mut curr = 0.0f32;

        let num_ch = channels as usize;

        let stream = match supported_config.sample_format() {
            SampleFormat::F32 => {
                if sdr_audio_rate == out_sample_rate {
                    device.build_output_stream(
                        &config,
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            if !is_active_clone.load(Ordering::Relaxed) {
                                data.fill(0.0);
                                return;
                            }
                            let frames = data.len() / num_ch;
                            for frame_idx in 0..frames {
                                let sample = consumer.pop().unwrap_or(0.0);
                                let base = frame_idx * num_ch;
                                for c in 0..num_ch {
                                    data[base + c] = sample;
                                }
                            }
                        },
                        err_fn,
                        None,
                    )
                } else {
                    device.build_output_stream(
                        &config,
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            if !is_active_clone.load(Ordering::Relaxed) {
                                data.fill(0.0);
                                return;
                            }
                            let frames = data.len() / num_ch;
                            for frame_idx in 0..frames {
                                if frac > 4.0 {
                                    frac = 0.0;
                                }
                                while frac >= 1.0 {
                                    prev = curr;
                                    curr = consumer.pop().unwrap_or(0.0);
                                    frac -= 1.0;
                                }
                                let sample = prev + (curr - prev) * frac;
                                frac += step;
                                let base = frame_idx * num_ch;
                                for c in 0..num_ch {
                                    data[base + c] = sample;
                                }
                            }
                        },
                        err_fn,
                        None,
                    )
                }
            }
            SampleFormat::I16 => {
                if sdr_audio_rate == out_sample_rate {
                    device.build_output_stream(
                        &config,
                        move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                            if !is_active_clone.load(Ordering::Relaxed) {
                                data.fill(0);
                                return;
                            }
                            let frames = data.len() / num_ch;
                            for frame_idx in 0..frames {
                                let sample = consumer.pop().unwrap_or(0.0);
                                let sample_i16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                                let base = frame_idx * num_ch;
                                for c in 0..num_ch {
                                    data[base + c] = sample_i16;
                                }
                            }
                        },
                        err_fn,
                        None,
                    )
                } else {
                    device.build_output_stream(
                        &config,
                        move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                            if !is_active_clone.load(Ordering::Relaxed) {
                                data.fill(0);
                                return;
                            }
                            let frames = data.len() / num_ch;
                            for frame_idx in 0..frames {
                                if frac > 4.0 {
                                    frac = 0.0;
                                }
                                while frac >= 1.0 {
                                    prev = curr;
                                    curr = consumer.pop().unwrap_or(0.0);
                                    frac -= 1.0;
                                }
                                let sample = prev + (curr - prev) * frac;
                                frac += step;
                                let sample_i16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                                let base = frame_idx * num_ch;
                                for c in 0..num_ch {
                                    data[base + c] = sample_i16;
                                }
                            }
                        },
                        err_fn,
                        None,
                    )
                }
            }
            _ => return Err(format!("Unsupported audio sample format on device '{}'", actual_name)),
        }
        .map_err(|e| format!("Failed to build audio stream on '{}': {}", actual_name, e))?;

        stream
            .play()
            .map_err(|e| format!("Failed to start playback on '{}': {}", actual_name, e))?;

        let engine = Self {
            _stream: stream,
            producer,
            is_active,
            device_name: actual_name.clone(),
            output_sample_rate: out_sample_rate,
        };

        Ok((engine, actual_name))
    }

    #[allow(dead_code)]
    pub fn new(sample_rate: u32) -> Result<Self, String> {
        Self::new_with_device(None, sample_rate).map(|(e, _)| e)
    }

    pub fn push_samples(&mut self, samples: &[f32], master_volume: f32) {
        if !self.is_active.load(Ordering::Relaxed) {
            return;
        }

        // Perceptual audio volume taper (quadratic curve):
        // At 1% (0.01) -> vol_gain is 0.0001 (-80 dBFS, soft whisper)
        // At 20% (0.20) -> vol_gain is 0.04 (-28 dBFS, quiet background)
        // At 50% (0.50) -> vol_gain is 0.25 (-12 dBFS, standard comfortable listening)
        // At 100% (1.00) -> vol_gain is 1.00 (0 dBFS, full line level)
        // Above 1.0 up to 2.0 -> linear boost up to 2.0 (+6 dB) with soft-knee limiter
        let vol_gain = if master_volume <= 1.0 {
            master_volume * master_volume
        } else {
            1.0 + (master_volume - 1.0)
        };

        for &s in samples {
            let scaled = s * vol_gain * 0.75;
            // Transparent soft-knee limiter preventing harsh digital clipping
            let limited = if scaled.abs() > 0.85 {
                scaled.signum() * (0.85 + 0.14 * ((scaled.abs() - 0.85) / 1.5).tanh())
            } else {
                scaled
            };
            let _ = self.producer.push(limited);
        }
    }
}
