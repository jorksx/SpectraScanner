use super::SdrDevice;
use crossbeam_channel::Sender;
use libloading::{Library, Symbol};
use num_complex::Complex32;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[repr(C)]
pub struct AirspyTransfer {
    pub device: *mut c_void,
    pub ctx: *mut c_void,
    pub samples: *mut c_void,
    pub sample_count: i32,
    pub dropped_samples: u64,
    pub sample_type: i32,
}

type AirspyCallback = extern "C" fn(transfer: *mut AirspyTransfer) -> i32;

struct AirspyContext {
    sender: Sender<Vec<Complex32>>,
}

extern "C" fn airspy_rx_callback(transfer: *mut AirspyTransfer) -> i32 {
    unsafe {
        if transfer.is_null() || (*transfer).samples.is_null() {
            return 0;
        }

        let ctx_ptr = (*transfer).ctx as *mut AirspyContext;
        if ctx_ptr.is_null() {
            return 0;
        }

        let count = (*transfer).sample_count as usize;
        if count == 0 {
            return 0;
        }

        // Float32 IQ format: interleaved [I0, Q0, I1, Q1, ...]
        let raw_floats = std::slice::from_raw_parts((*transfer).samples as *const f32, count * 2);
        let mut buffer = Vec::with_capacity(count);

        for chunk in raw_floats.chunks_exact(2) {
            buffer.push(Complex32::new(chunk[0], chunk[1]));
        }

        let _ = (*ctx_ptr).sender.try_send(buffer);
    }
    0
}

pub struct AirspyDevice {
    lib: Arc<Library>,
    device_ptr: *mut c_void,
    running: Arc<AtomicBool>,
    context: Option<Box<AirspyContext>>,
    frequency_hz: u64,
    sample_rate: u32,
    gain: u32,
}

// Safety: Airspy device handle is managed via thread-safe callbacks
unsafe impl Send for AirspyDevice {}

impl AirspyDevice {
    pub fn new() -> Result<Self, String> {
        let lib = super::load_sdr_library("airspy.dll")?;

        let mut device_ptr: *mut c_void = std::ptr::null_mut();

        unsafe {
            type AirspyInitFn = unsafe extern "C" fn() -> i32;
            if let Ok(init_fn) = lib.get::<AirspyInitFn>(b"airspy_init") {
                init_fn();
            }

            type AirspyOpenFn = unsafe extern "C" fn(device: *mut *mut c_void) -> i32;
            let open_fn: Symbol<AirspyOpenFn> = lib
                .get(b"airspy_open")
                .map_err(|e| format!("Symbol airspy_open not found: {}", e))?;

            let res = open_fn(&mut device_ptr);
            if res != 0 || device_ptr.is_null() {
                let err_desc = match res {
                    -1 => "AIRSPY_ERROR_INVALID_PARAM",
                    -2 => "AIRSPY_ERROR_NOT_FOUND (Laitetta ei löydy, tarkista USB)",
                    -3 => "AIRSPY_ERROR_BUSY (Laite varattu toiselle ohjelmalle esim. SDR#)",
                    -4 => "AIRSPY_ERROR_NO_MEM",
                    -5 => "AIRSPY_ERROR_LIBUSB",
                    -6 => "AIRSPY_ERROR_THREAD",
                    -7 => "AIRSPY_ERROR_STREAMING_THREAD_ERR",
                    -8 => "AIRSPY_ERROR_STREAMING_STOPPED",
                    -9 => "AIRSPY_ERROR_OTHER",
                    _ => "Tuntematon virhe",
                };
                return Err(format!("airspy_open epäonnistui: {} (koodi {})", err_desc, res));
            }

            // Set Float32 IQ sample type (0 = AIRSPY_SAMPLE_FLOAT32_IQ)
            type AirspySetSampleTypeFn =
                unsafe extern "C" fn(device: *mut c_void, sample_type: i32) -> i32;
            if let Ok(set_type) = lib.get::<AirspySetSampleTypeFn>(b"airspy_set_sample_type") {
                set_type(device_ptr, 0);
            }
        }

        Ok(Self {
            lib: Arc::new(lib),
            device_ptr,
            running: Arc::new(AtomicBool::new(false)),
            context: None,
            frequency_hz: 122_500_000,
            sample_rate: 10_000_000,
            gain: 15,
        })
    }
}

impl SdrDevice for AirspyDevice {
    fn name(&self) -> &str {
        "Airspy R2"
    }

    fn set_frequency(&mut self, freq_hz: u64) -> Result<(), String> {
        self.frequency_hz = freq_hz;
        if self.device_ptr.is_null() {
            return Ok(());
        }

        unsafe {
            type AirspySetFreqFn = unsafe extern "C" fn(device: *mut c_void, freq_hz: u32) -> i32;
            let set_freq: Symbol<AirspySetFreqFn> = self
                .lib
                .get(b"airspy_set_freq")
                .map_err(|e| e.to_string())?;
            let res = set_freq(self.device_ptr, freq_hz as u32);
            if res != 0 {
                return Err(format!("airspy_set_freq failed: {}", res));
            }
        }
        Ok(())
    }

    fn set_sample_rate(&mut self, rate_hz: u32) -> Result<(), String> {
        self.sample_rate = rate_hz;
        if self.device_ptr.is_null() {
            return Ok(());
        }

        unsafe {
            type AirspySetSampleRateFn =
                unsafe extern "C" fn(device: *mut c_void, samplerate: u32) -> i32;
            let set_sr: Symbol<AirspySetSampleRateFn> = self
                .lib
                .get(b"airspy_set_samplerate")
                .map_err(|e| e.to_string())?;
            let res = set_sr(self.device_ptr, rate_hz);
            if res != 0 {
                return Err(format!("airspy_set_samplerate failed: {}", res));
            }
        }
        Ok(())
    }

    fn set_gain(&mut self, gain: u32) -> Result<(), String> {
        self.gain = gain;
        if self.device_ptr.is_null() {
            return Ok(());
        }

        unsafe {
            type AirspySetLinearityGainFn =
                unsafe extern "C" fn(device: *mut c_void, gain: u8) -> i32;
            if let Ok(set_gain) = self
                .lib
                .get::<AirspySetLinearityGainFn>(b"airspy_set_linearity_gain")
            {
                let g = (gain.min(21)) as u8;
                set_gain(self.device_ptr, g);
            }
        }
        Ok(())
    }

    fn start(&mut self, sample_sender: Sender<Vec<Complex32>>) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.set_sample_rate(self.sample_rate)?;
        self.set_frequency(self.frequency_hz)?;
        self.set_gain(self.gain)?;

        let mut context = Box::new(AirspyContext {
            sender: sample_sender,
        });

        unsafe {
            type AirspyStartRxFn = unsafe extern "C" fn(
                device: *mut c_void,
                callback: AirspyCallback,
                ctx: *mut c_void,
            ) -> i32;

            let start_rx: Symbol<AirspyStartRxFn> = self
                .lib
                .get(b"airspy_start_rx")
                .map_err(|e| e.to_string())?;

            let ctx_ptr = &mut *context as *mut AirspyContext as *mut c_void;
            let res = start_rx(self.device_ptr, airspy_rx_callback, ctx_ptr);
            if res != 0 {
                return Err(format!("airspy_start_rx failed with code: {}", res));
            }
        }

        self.context = Some(context);
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), String> {
        if !self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        unsafe {
            type AirspyStopRxFn = unsafe extern "C" fn(device: *mut c_void) -> i32;
            if let Ok(stop_rx) = self.lib.get::<AirspyStopRxFn>(b"airspy_stop_rx") {
                stop_rx(self.device_ptr);
            }
        }

        self.running.store(false, Ordering::SeqCst);
        self.context = None;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for AirspyDevice {
    fn drop(&mut self) {
        let _ = self.stop();
        if !self.device_ptr.is_null() {
            unsafe {
                type AirspyCloseFn = unsafe extern "C" fn(device: *mut c_void) -> i32;
                if let Ok(close_fn) = self.lib.get::<AirspyCloseFn>(b"airspy_close") {
                    close_fn(self.device_ptr);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_airspy_hardware_probe() {
        if let Ok(mut device) = AirspyDevice::new() {
            let (tx, rx) = crossbeam_channel::bounded(16);
            if device.start(tx).is_ok() {
                if let Ok(batch) = rx.recv_timeout(std::time::Duration::from_millis(500)) {
                    assert!(!batch.is_empty(), "Airspy returned non-empty sample batch");
                    let fft_size = 2048;
                    let mut fft = crate::dsp::FftProcessor::new(fft_size);
                    let mut avg_raw = vec![-100.0f32; fft_size];
                    let mut pseudo = vec![0.0f32; fft_size];
                    let mut noise_floor = crate::dsp::DynamicNoiseFloor::new(fft_size, 25);

                    fft.compute_averaged_spectrum(&batch, &mut avg_raw, 8);
                    let raw_mean = avg_raw.iter().sum::<f32>() / (fft_size as f32);
                    let raw_min = avg_raw.iter().copied().fold(f32::INFINITY, f32::min);
                    let raw_max = avg_raw.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                    noise_floor.process(&avg_raw, &mut pseudo);

                    let mean_p = pseudo.iter().sum::<f32>() / (fft_size as f32);
                    println!("Airspy batch: raw_mean={:.1} dB, raw_min={:.1} dB, raw_max={:.1} dB, pseudo_mean={:.1} dB", raw_mean, raw_min, raw_max, mean_p);
                    assert!(mean_p <= 5.0, "Initial 8-window pseudo mean should be <= 5.0 dB: {}", mean_p);

                    // Test retune to 145 MHz
                    let _ = device.set_frequency(145_000_000);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    while rx.try_recv().is_ok() {}

                    if let Ok(new_batch) = rx.recv_timeout(std::time::Duration::from_millis(500)) {
                        fft.compute_averaged_spectrum(&new_batch, &mut avg_raw, 8);
                        noise_floor.reset();
                        noise_floor.process(&avg_raw, &mut pseudo);
                        let retune_mean = pseudo.iter().sum::<f32>() / (fft_size as f32);
                        assert!(retune_mean <= 5.0, "Retune 8-window pseudo mean should be <= 5.0 dB: {}", retune_mean);
                    }
                }
                let _ = device.stop();
            }
        }
    }
}
