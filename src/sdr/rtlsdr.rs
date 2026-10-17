use super::SdrDevice;
use crossbeam_channel::Sender;
use libloading::{Library, Symbol};
use num_complex::Complex32;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

type RtlSdrReadAsyncCb = extern "C" fn(buf: *mut u8, len: u32, ctx: *mut c_void);

struct RtlContext {
    sender: Sender<Vec<Complex32>>,
}

extern "C" fn rtlsdr_rx_callback(buf: *mut u8, len: u32, ctx: *mut c_void) {
    unsafe {
        if buf.is_null() || len == 0 || ctx.is_null() {
            return;
        }

        let ctx_ptr = ctx as *mut RtlContext;
        let slice = std::slice::from_raw_parts(buf, len as usize);

        let num_samples = slice.len() / 2;
        let mut buffer = Vec::with_capacity(num_samples);

        for chunk in slice.chunks_exact(2) {
            let i = (chunk[0] as f32 - 127.5) / 128.0;
            let q = (chunk[1] as f32 - 127.5) / 128.0;
            buffer.push(Complex32::new(i, q));
        }

        let _ = (*ctx_ptr).sender.try_send(buffer);
    }
}

pub struct RtlSdrDevice {
    lib: Arc<Library>,
    device_ptr: *mut c_void,
    running: Arc<AtomicBool>,
    worker_handle: Option<thread::JoinHandle<()>>,
    frequency_hz: u64,
    sample_rate: u32,
    gain: u32,
}

unsafe impl Send for RtlSdrDevice {}

impl RtlSdrDevice {
    pub fn new() -> Result<Self, String> {
        let lib = super::load_sdr_library("rtlsdr.dll")?;

        let mut device_ptr: *mut c_void = std::ptr::null_mut();

        unsafe {
            type RtlOpenFn = unsafe extern "C" fn(dev: *mut *mut c_void, index: u32) -> i32;
            let open_fn: Symbol<RtlOpenFn> = lib
                .get(b"rtlsdr_open")
                .map_err(|e| format!("Symbol rtlsdr_open not found: {}", e))?;

            let res = open_fn(&mut device_ptr, 0);
            if res != 0 || device_ptr.is_null() {
                return Err(format!("rtlsdr_open failed with code: {}", res));
            }
        }

        Ok(Self {
            lib: Arc::new(lib),
            device_ptr,
            running: Arc::new(AtomicBool::new(false)),
            worker_handle: None,
            frequency_hz: 122_500_000,
            sample_rate: 2_048_000,
            gain: 200,
        })
    }
}

impl SdrDevice for RtlSdrDevice {
    fn name(&self) -> &str {
        "RTL-SDR"
    }

    fn set_frequency(&mut self, freq_hz: u64) -> Result<(), String> {
        self.frequency_hz = freq_hz;
        if self.device_ptr.is_null() {
            return Ok(());
        }

        unsafe {
            type RtlSetFreqFn = unsafe extern "C" fn(dev: *mut c_void, freq: u32) -> i32;
            let set_freq: Symbol<RtlSetFreqFn> = self
                .lib
                .get(b"rtlsdr_set_center_freq")
                .map_err(|e| e.to_string())?;
            let res = set_freq(self.device_ptr, freq_hz as u32);
            if res != 0 {
                return Err(format!("rtlsdr_set_center_freq failed: {}", res));
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
            type RtlSetRateFn = unsafe extern "C" fn(dev: *mut c_void, rate: u32) -> i32;
            let set_rate: Symbol<RtlSetRateFn> = self
                .lib
                .get(b"rtlsdr_set_sample_rate")
                .map_err(|e| e.to_string())?;
            let res = set_rate(self.device_ptr, rate_hz);
            if res != 0 {
                return Err(format!("rtlsdr_set_sample_rate failed: {}", res));
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
            type RtlSetGainModeFn = unsafe extern "C" fn(dev: *mut c_void, manual: i32) -> i32;
            if let Ok(set_mode) = self.lib.get::<RtlSetGainModeFn>(b"rtlsdr_set_tuner_gain_mode") {
                set_mode(self.device_ptr, 1);
            }

            type RtlSetGainFn = unsafe extern "C" fn(dev: *mut c_void, gain: i32) -> i32;
            if let Ok(set_gain) = self.lib.get::<RtlSetGainFn>(b"rtlsdr_set_tuner_gain") {
                set_gain(self.device_ptr, (gain * 10) as i32);
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

        unsafe {
            type RtlResetBufFn = unsafe extern "C" fn(dev: *mut c_void) -> i32;
            if let Ok(reset_buf) = self.lib.get::<RtlResetBufFn>(b"rtlsdr_reset_buffer") {
                reset_buf(self.device_ptr);
            }
        }

        let lib = self.lib.clone();
        let dev_ptr = self.device_ptr as usize;
        let running_flag = self.running.clone();
        running_flag.store(true, Ordering::SeqCst);

        let handle = thread::spawn(move || {
            let mut context = Box::new(RtlContext {
                sender: sample_sender,
            });

            unsafe {
                type RtlReadAsyncFn = unsafe extern "C" fn(
                    dev: *mut c_void,
                    cb: RtlSdrReadAsyncCb,
                    ctx: *mut c_void,
                    buf_num: u32,
                    buf_len: u32,
                ) -> i32;

                if let Ok(read_async) = lib.get::<RtlReadAsyncFn>(b"rtlsdr_read_async") {
                    let ctx_ptr = &mut *context as *mut RtlContext as *mut c_void;
                    read_async(dev_ptr as *mut c_void, rtlsdr_rx_callback, ctx_ptr, 16, 32768);
                }
            }
            running_flag.store(false, Ordering::SeqCst);
        });

        self.worker_handle = Some(handle);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), String> {
        if !self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.running.store(false, Ordering::SeqCst);

        unsafe {
            type RtlCancelAsyncFn = unsafe extern "C" fn(dev: *mut c_void) -> i32;
            if let Ok(cancel) = self.lib.get::<RtlCancelAsyncFn>(b"rtlsdr_cancel_async") {
                cancel(self.device_ptr);
            }
        }

        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }

        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for RtlSdrDevice {
    fn drop(&mut self) {
        let _ = self.stop();
        if !self.device_ptr.is_null() {
            unsafe {
                type RtlCloseFn = unsafe extern "C" fn(dev: *mut c_void) -> i32;
                if let Ok(close_fn) = self.lib.get::<RtlCloseFn>(b"rtlsdr_close") {
                    close_fn(self.device_ptr);
                }
            }
        }
    }
}
