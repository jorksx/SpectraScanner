pub mod airspy;
pub mod mock;
pub mod rtlsdr;

use crossbeam_channel::Sender;
use libloading::Library;
use num_complex::Complex32;
use std::path::{Path, PathBuf};

pub trait SdrDevice: Send {
    fn name(&self) -> &str;
    fn set_frequency(&mut self, freq_hz: u64) -> Result<(), String>;
    fn set_sample_rate(&mut self, rate_hz: u32) -> Result<(), String>;
    fn set_gain(&mut self, gain: u32) -> Result<(), String>;
    fn start(&mut self, sample_sender: Sender<Vec<Complex32>>) -> Result<(), String>;
    fn stop(&mut self) -> Result<(), String>;
    fn is_running(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSelection {
    AirspyR2,
    RtlSdr,
    SimulatedRf,
}

impl DeviceSelection {
    pub fn name(&self) -> &'static str {
        match self {
            DeviceSelection::AirspyR2 => "Airspy R2",
            DeviceSelection::RtlSdr => "RTL-SDR",
            DeviceSelection::SimulatedRf => "Test Generator (Simulated)",
        }
    }
}

#[cfg(windows)]
fn set_dll_directory(dir: &Path) {
    if let Ok(abs) = dir.canonicalize() {
        use std::os::windows::ffi::OsStrExt;
        let mut wide: Vec<u16> = abs.as_os_str().encode_wide().collect();
        wide.push(0);
        unsafe {
            extern "system" {
                fn SetDllDirectoryW(lpPathName: *const u16) -> i32;
            }
            SetDllDirectoryW(wide.as_ptr());
        }
    }
}

/// Robust DLL loader that searches executable dir, cwd, subdirectories, and registers
/// the DLL directory with Windows so dependent DLLs (e.g. libusb-1.0.dll, pthreadVC3.dll)
/// are always resolved automatically.
pub fn load_sdr_library(dll_name: &str) -> Result<Library, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // 1. Directory of running executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join(dll_name));
            candidates.push(exe_dir.join("lib").join(dll_name));
            if let Some(parent) = exe_dir.parent() {
                candidates.push(parent.join(dll_name));
                candidates.push(parent.join("lib").join(dll_name));
                if let Some(grandparent) = parent.parent() {
                    candidates.push(grandparent.join(dll_name));
                    candidates.push(grandparent.join("lib").join(dll_name));
                }
            }
        }
    }

    // 2. Current working directory
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(dll_name));
        candidates.push(cwd.join("lib").join(dll_name));
    }

    // 3. Relative path fallbacks
    candidates.push(PathBuf::from(dll_name));
    candidates.push(PathBuf::from("lib").join(dll_name));

    let mut last_err = None;
    for path in &candidates {
        if path.exists() {
            #[cfg(windows)]
            if let Some(parent) = path.parent() {
                set_dll_directory(parent);
            }
            unsafe {
                match Library::new(path) {
                    Ok(lib) => return Ok(lib),
                    Err(e) => last_err = Some(format!("Error loading {}: {}", path.display(), e)),
                }
            }
        }
    }

    // Fallback: system PATH
    unsafe {
        match Library::new(dll_name) {
            Ok(lib) => Ok(lib),
            Err(e) => {
                let detail = last_err.unwrap_or_else(|| e.to_string());
                Err(format!("Could not load {}: {}", dll_name, detail))
            }
        }
    }
}
