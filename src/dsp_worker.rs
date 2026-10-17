use crate::audio::AudioEngine;
use crate::config::{Bookmark, DemodMode, ScannerConfig};
use crate::dsp::{DynamicNoiseFloor, FftProcessor};
use crate::scanner::vfo::VfoState;
use crate::scanner::{ScannerEngine, ScannerState};
use crate::sdr::airspy::AirspyDevice;
use crate::sdr::mock::MockDevice;
use crate::sdr::rtlsdr::RtlSdrDevice;
use crate::sdr::{DeviceSelection, SdrDevice};
use crossbeam_channel::{bounded, Receiver, Sender};
use num_complex::Complex32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone)]
#[allow(dead_code)]
pub struct VfoDisplayInfo {
    pub id: usize,
    pub freq_hz: u64,
    pub name: String,
    pub mode: DemodMode,
    pub state: VfoState,
    pub snr_db: f32,
    pub vad_score: f32,
    pub active: bool,
    pub is_out_of_band: bool,
    pub muted: bool,
    pub volume: f32,
    pub delay_remaining_sec: f32,
    pub dmr_info: Option<crate::dsp::DmrDisplayInfo>,
    pub latest_telemetry_info: Option<String>,
    pub audio_waveform: [f32; 32],
    pub squelch_open: bool,
}

#[derive(Clone)]
pub struct DisplaySnapshot {
    pub raw_spectrum: Vec<f32>,
    pub pseudo_spectrum: Vec<f32>,
    pub noise_baseline: Vec<f32>,
    pub scan_cursor_hz: u64,
    pub vfos: Vec<VfoDisplayInfo>,
    pub center_freq_hz: u64,
    pub sample_rate_hz: u32,
    pub status_message: String,
    pub device_running: bool,
    pub active_range: String,
    #[allow(dead_code)]
    pub latest_aprs: Option<crate::dsp::AprsPacket>,
    pub aprs_packets: Vec<crate::dsp::AprsPacket>,
    #[allow(dead_code)]
    pub latest_dmr: Option<crate::dsp::DmrPacket>,
    pub dmr_packets: Vec<crate::dsp::DmrPacket>,
    #[allow(dead_code)]
    pub latest_telemetry: Option<crate::dsp::TelemetryPacket>,
    pub telemetry_packets: Vec<crate::dsp::TelemetryPacket>,
    #[allow(dead_code)]
    pub latest_air: Option<crate::dsp::AirPacket>,
    pub air_packets: Vec<crate::dsp::AirPacket>,
    #[allow(dead_code)]
    pub latest_ais: Option<crate::dsp::AisPacket>,
    pub ais_packets: Vec<crate::dsp::AisPacket>,
    pub current_audio_device: String,
    pub blacklist: Vec<u64>,
    pub vdl2_burst_stats: Option<(u32, f32, f32)>,
}

#[allow(dead_code)]
pub enum DspCommand {
    SetAudioDevice(String),
    SetCenterFreq(u64),
    SetSquelch(f32),
    SetVad(bool, f32),
    SetScanRange(u64, u64, u32),
    SetScannerState(ScannerState),
    PinMostActive,
    TogglePinVfo(usize),
    SkipFrequency(u64, u64),
    SetManualVfo(usize, u64, DemodMode, String),
    TuneDataVfo(usize, u64, DemodMode, String),
    TuneMultiDataVfos(Vec<(u64, DemodMode, String)>),
    TuneMultiChannels(Vec<(u64, DemodMode, String, bool)>),
    SetVfoMute(usize, bool),
    SetVfoVolume(usize, f32),
    SetVfoMode(usize, DemodMode),
    SetMasterVolume(f32),
    SwitchDevice(DeviceSelection),
    SetRfGain(u32),
    SetScanProfile {
        name: String,
        center_freq_hz: u64,
        scan_start_hz: u64,
        scan_end_hz: u64,
        scan_step_hz: u32,
        mode: DemodMode,
    },
    SetRangeEnabled {
        name: String,
        enabled: bool,
    },
    SetMaxVfos(usize),
    SetHangoverMs(u64),
    CloseVfo(usize),
    ClearHistory,
    SetVfoComment(usize, String),
    AddBlacklist(u64),
    RemoveBlacklist(u64),
    AddBookmark(Bookmark),
    RemoveBookmark(u64),
    SetVfoEditing(Option<usize>),
    SetQth(f64, f64),
    RestoreConfiguredBands {
        active_range: String,
    },
}

pub struct DspController {
    pub cmd_tx: Sender<DspCommand>,
    pub display_rx: Receiver<DisplaySnapshot>,
    pub _running: Arc<AtomicBool>,
}

impl DspController {
    pub fn spawn(initial_cfg: ScannerConfig) -> Self {
        let (cmd_tx, cmd_rx) = bounded::<DspCommand>(128);
        let (display_tx, display_rx) = bounded::<DisplaySnapshot>(2);
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        thread::Builder::new()
            .name("SDR-DSP-Worker".to_string())
            .spawn(move || {
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_dsp_worker(initial_cfg, cmd_rx, display_tx, running_clone);
                }));
                if let Err(e) = res {
                    eprintln!("CRITICAL: SDR-DSP-Worker thread crashed: {:?}", e);
                    let err_msg = if let Some(s) = e.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = e.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "Unknown panic in DSP worker".to_string()
                    };
                    let _ = std::fs::write("dsp_worker_crash.log", format!("Crash at {:?}: {}", std::time::SystemTime::now(), err_msg));
                }
            })
            .expect("Failed to spawn DSP worker thread");

        Self {
            cmd_tx,
            display_rx,
            _running: running,
        }
    }
}

struct DisconnectedDevice {
    name: String,
}

impl SdrDevice for DisconnectedDevice {
    fn name(&self) -> &str {
        &self.name
    }
    fn set_frequency(&mut self, _freq_hz: u64) -> Result<(), String> {
        Ok(())
    }
    fn set_sample_rate(&mut self, _rate_hz: u32) -> Result<(), String> {
        Ok(())
    }
    fn set_gain(&mut self, _gain: u32) -> Result<(), String> {
        Ok(())
    }
    fn start(&mut self, _sample_sender: Sender<Vec<Complex32>>) -> Result<(), String> {
        Ok(())
    }
    fn stop(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn is_running(&self) -> bool {
        false
    }
}

fn run_dsp_worker(
    mut cfg: ScannerConfig,
    cmd_rx: Receiver<DspCommand>,
    display_tx: Sender<DisplaySnapshot>,
    running: Arc<AtomicBool>,
) {
    let (sample_tx, sample_rx) = bounded::<Vec<Complex32>>(64);

    let dev_opt = if cfg.audio_device == "Default (Windows System Output)" {
        None
    } else {
        Some(cfg.audio_device.as_str())
    };
    let (mut audio, mut current_audio_device) = match AudioEngine::new_with_device(dev_opt, 48_000) {
        Ok((a, name)) => (Some(a), name),
        Err(e) => {
            eprintln!("Failed to open preferred audio device '{}': {}, trying default", cfg.audio_device, e);
            match AudioEngine::new_with_device(None, 48_000) {
                Ok((a, name)) => (Some(a), name),
                Err(e2) => {
                    eprintln!("Failed to open default audio device: {}", e2);
                    (None, "No Audio Device".to_string())
                }
            }
        }
    };
    let mut master_volume = 1.0f32;

    let fft_size = 2048;
    let mut fft = FftProcessor::new(fft_size);
    let mut noise_floor = DynamicNoiseFloor::new(fft_size, 25);
    let mut engine = ScannerEngine::new(&cfg);

    let mut raw_spectrum = vec![-100.0; fft_size];
    let mut smooth_spectrum = vec![-100.0; fft_size];
    let mut pseudo_spectrum = vec![0.0; fft_size];
    let mut warm_up_frames = 1usize;
    let mut audio_buffer = Vec::with_capacity(4096);

    let mut aprs_decoder = crate::dsp::AprsDecoder::new(48_000.0);
    let mut aprs_ddc = crate::dsp::DigitalDownConverter::new(cfg.sample_rate as f32, 48_000.0, 16_000.0);
    let mut aprs_ddc_buf = Vec::with_capacity(2048);
    let mut last_aprs_center_freq = 0u64;
    let mut last_aprs_sample_rate = 0u32;

    let mut adsb_decoder = crate::dsp::AdsbDecoder::new(cfg.sample_rate as f32);
    let mut direct_adsb_packets: Vec<crate::dsp::AirPacket> = Vec::new();

    let mut ais_decoder_a = crate::dsp::AisDecoder::new(48_000.0);
    let mut ais_decoder_b = crate::dsp::AisDecoder::new(48_000.0);
    let mut ais_ddc_a = crate::dsp::DigitalDownConverter::new(cfg.sample_rate as f32, 48_000.0, 25_000.0);
    let mut ais_ddc_b = crate::dsp::DigitalDownConverter::new(cfg.sample_rate as f32, 48_000.0, 25_000.0);
    let mut ais_ddc_buf_a = Vec::with_capacity(2048);
    let mut ais_ddc_buf_b = Vec::with_capacity(2048);
    let mut direct_ais_packets: Vec<crate::dsp::AisPacket> = Vec::new();
    let mut last_ais_center_freq_a = 0u64;
    let mut last_ais_center_freq_b = 0u64;
    let mut last_ais_sample_rate = 0u32;

    let mut current_selection = DeviceSelection::AirspyR2;
    let mut last_reconnect_attempt = Instant::now();

    // Initial physical Airspy connect - NEVER silently fall back to fake mock device!
    let (mut device, mut status_message): (Box<dyn SdrDevice>, String) = match AirspyDevice::new() {
        Ok(mut airspy) => {
            let _ = airspy.set_sample_rate(cfg.sample_rate);
            let _ = airspy.set_frequency(cfg.center_freq_hz);
            let _ = airspy.set_gain(cfg.rf_gain);
            match airspy.start(sample_tx.clone()) {
                Ok(_) => {
                    let msg = format!("Airspy R2 ({:.1} MSPS) ● RUNNING", cfg.sample_rate as f64 / 1e6);
                    (Box::new(airspy), msg)
                }
                Err(e) => {
                    let msg = format!("Airspy R2 error: {}", e);
                    (Box::new(DisconnectedDevice { name: "Airspy R2 (Disconnected)".to_string() }), msg)
                }
            }
        }
        Err(e) => {
            let msg = format!("Airspy R2: {}", e);
            (Box::new(DisconnectedDevice { name: "Airspy R2 (Disconnected)".to_string() }), msg)
        }
    };

    let mut last_display_time = Instant::now();
    let mut last_range_hop = Instant::now();
    let mut settling_until = Instant::now() + Duration::from_millis(40);
    let display_interval = Duration::from_millis(25); // ~40 FPS for display
    let mut current_temp_scan: Option<String> = None;

    while running.load(Ordering::Relaxed) {
        // 1. Process all pending GUI commands
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                DspCommand::SetAudioDevice(name) => {
                    cfg.audio_device = name.clone();
                    let dev_opt = if name == "Default (Windows System Output)" {
                        None
                    } else {
                        Some(name.as_str())
                    };
                    match AudioEngine::new_with_device(dev_opt, 48_000) {
                        Ok((new_audio, actual_name)) => {
                            audio = Some(new_audio);
                            current_audio_device = actual_name;
                        }
                        Err(e) => {
                            eprintln!("Failed to switch to audio device '{}': {}", name, e);
                            if let Ok((fallback_audio, fallback_name)) = AudioEngine::new_with_device(None, 48_000) {
                                audio = Some(fallback_audio);
                                current_audio_device = fallback_name;
                            }
                        }
                    }
                }
                DspCommand::SetCenterFreq(f) => {
                    cfg.center_freq_hz = f;
                    let _ = device.set_frequency(f);
                    while sample_rx.try_recv().is_ok() {}
                    settling_until = Instant::now() + Duration::from_millis(40);
                    warm_up_frames = 1;
                    noise_floor.reset();
                    for vfo in &mut engine.vfos {
                        let freq = vfo.freq_hz;
                        let name = vfo.name.clone();
                        vfo.retune(freq, f, name);
                    }
                }
                DspCommand::SetVfoEditing(opt) => {
                    engine.editing_vfo_id = opt;
                }
                DspCommand::SetSquelch(sq) => {
                    cfg.squelch_snr_db = sq;
                }
                DspCommand::SetVad(enabled, thresh) => {
                    cfg.vad_enabled = enabled;
                    cfg.vad_threshold = thresh;
                }
                DspCommand::SetScanRange(start, end, step) => {
                    cfg.scan_start_hz = start;
                    cfg.scan_end_hz = end;
                    cfg.scan_step_hz = step;
                }
                DspCommand::SetScannerState(st) => {
                    engine.set_scanner_state(st);
                }
                DspCommand::PinMostActive => {
                    engine.pin_most_active();
                }
                DspCommand::TogglePinVfo(id) => {
                    engine.toggle_pin_vfo(id);
                }
                DspCommand::CloseVfo(id) => {
                    engine.close_vfo(id);
                }
                DspCommand::ClearHistory => {
                    engine.clear_history();
                }
                DspCommand::SetVfoComment(id, comment) => {
                    engine.set_vfo_comment(id, comment.clone());
                    if let Some(vfo) = engine.vfos.iter().find(|v| v.id == id) {
                        let trimmed = comment.trim();
                        if !trimmed.is_empty() {
                            let bm = Bookmark {
                                freq_hz: vfo.freq_hz,
                                name: trimmed.to_string(),
                                mode: vfo.mode,
                                bandwidth_hz: 12_500,
                            };
                            cfg.bookmarks.insert(vfo.freq_hz, bm);
                        }
                    }
                }
                DspCommand::AddBookmark(bm) => {
                    cfg.bookmarks.insert(bm.freq_hz, bm);
                }
                DspCommand::RemoveBookmark(f) => {
                    cfg.bookmarks.remove(&f);
                }
                DspCommand::AddBlacklist(f) => {
                    cfg.add_blacklist(f);
                    engine.add_blacklist(f);
                }
                DspCommand::RemoveBlacklist(f) => {
                    cfg.remove_blacklist(f);
                    engine.remove_blacklist(f);
                }
                DspCommand::SkipFrequency(f, dur) => {
                    engine.skip_frequency(f, dur);
                }
                DspCommand::SetManualVfo(idx, f, mode, name) => {
                    let half_rate = (cfg.sample_rate / 2) as u64;
                    let min_f = cfg.center_freq_hz.saturating_sub(half_rate);
                    let max_f = cfg.center_freq_hz + half_rate;
                    if f < min_f || f > max_f {
                        cfg.center_freq_hz = f;
                        let _ = device.set_frequency(f);
                        while sample_rx.try_recv().is_ok() {}
                        settling_until = Instant::now() + Duration::from_millis(40);
                        warm_up_frames = 1;
                        noise_floor.reset();
                    }
                    engine.set_manual_vfo(idx, f, mode, name, &cfg);
                }
                DspCommand::TuneDataVfo(idx, f, mode, name) => {
                    let half_rate = (cfg.sample_rate / 2) as u64;
                    let min_f = cfg.center_freq_hz.saturating_sub(half_rate);
                    let max_f = cfg.center_freq_hz + half_rate;
                    if f < min_f || f > max_f {
                        cfg.center_freq_hz = f;
                        let _ = device.set_frequency(f);
                        while sample_rx.try_recv().is_ok() {}
                        settling_until = Instant::now() + Duration::from_millis(40);
                        warm_up_frames = 1;
                        noise_floor.reset();
                    }
                    engine.tune_data_vfo(idx, f, mode, name, &cfg);
                }
                DspCommand::TuneMultiDataVfos(reqs) => {
                    if !reqs.is_empty() {
                        let has_1090 = reqs.iter().any(|r| r.0 >= 1_080_000_000 && r.0 <= 1_100_000_000);
                        let target_center = if has_1090 {
                            1_090_000_000
                        } else {
                            let min_f = reqs.iter().map(|r| r.0).min().unwrap();
                            let max_f = reqs.iter().map(|r| r.0).max().unwrap();
                            let span = max_f.saturating_sub(min_f);
                            let usable_bw = ((cfg.sample_rate as f64) * 0.90) as u64;

                            if span <= usable_bw {
                                (min_f + max_f) / 2
                            } else {
                                reqs.last().map(|r| r.0).unwrap_or(min_f)
                            }
                        };

                        let half_rate = (cfg.sample_rate / 2) as u64;
                        let cur_min = cfg.center_freq_hz.saturating_sub(half_rate);
                        let cur_max = cfg.center_freq_hz + half_rate;

                        let all_fit_current = !has_1090 && reqs.iter().all(|r| r.0 >= cur_min && r.0 <= cur_max);
                        if !all_fit_current || has_1090 || (target_center != cfg.center_freq_hz) {
                            cfg.center_freq_hz = target_center;
                            let _ = device.set_frequency(target_center);
                            while sample_rx.try_recv().is_ok() {}
                            settling_until = Instant::now() + Duration::from_millis(40);
                            warm_up_frames = 1;
                            noise_floor.reset();
                        }

                        engine.tune_multi_data_vfos(&reqs, &cfg);
                    }
                }
                DspCommand::TuneMultiChannels(reqs) => {
                    if !reqs.is_empty() {
                        let has_1090 = reqs.iter().any(|r| r.0 >= 1_080_000_000 && r.0 <= 1_100_000_000);
                        let target_center = if has_1090 {
                            1_090_000_000
                        } else {
                            let min_f = reqs.iter().map(|r| r.0).min().unwrap();
                            let max_f = reqs.iter().map(|r| r.0).max().unwrap();
                            let span = max_f.saturating_sub(min_f);
                            let usable_bw = ((cfg.sample_rate as f64) * 0.90) as u64;

                            if span <= usable_bw {
                                (min_f + max_f) / 2
                            } else {
                                reqs.last().map(|r| r.0).unwrap_or(min_f)
                            }
                        };

                        let half_rate = (cfg.sample_rate / 2) as u64;
                        let cur_min = cfg.center_freq_hz.saturating_sub(half_rate);
                        let cur_max = cfg.center_freq_hz + half_rate;

                        let all_fit_current = !has_1090 && reqs.iter().all(|r| r.0 >= cur_min && r.0 <= cur_max);
                        if !all_fit_current || has_1090 || (target_center != cfg.center_freq_hz) {
                            cfg.center_freq_hz = target_center;
                            let _ = device.set_frequency(target_center);
                            while sample_rx.try_recv().is_ok() {}
                            settling_until = Instant::now() + Duration::from_millis(40);
                            warm_up_frames = 1;
                            noise_floor.reset();
                        }

                        engine.tune_multi_channels(&reqs, &cfg);
                    }
                }
                DspCommand::SetQth(lat, lon) => {
                    adsb_decoder.set_receiver_qth((lat, lon));
                    for vfo in &mut engine.vfos {
                        vfo.air_decoder.set_qth((lat, lon));
                    }
                }
                DspCommand::SetVfoMute(id, m) => {
                    if let Some(vfo) = engine.vfos.iter_mut().find(|v| v.id == id) {
                        vfo.muted = m;
                    }
                }
                DspCommand::SetVfoVolume(id, vol) => {
                    if let Some(vfo) = engine.vfos.iter_mut().find(|v| v.id == id) {
                        vfo.volume = vol;
                    }
                }
                DspCommand::SetVfoMode(id, mode) => {
                    if let Some(vfo) = engine.vfos.iter_mut().find(|v| v.id == id) {
                        vfo.set_mode(mode);
                    }
                }
                DspCommand::SetMasterVolume(vol) => {
                    master_volume = vol;
                }
                DspCommand::SetRfGain(g) => {
                    cfg.rf_gain = g;
                    let _ = device.set_gain(g);
                }
                DspCommand::SetScanProfile {
                    name,
                    center_freq_hz,
                    scan_start_hz,
                    scan_end_hz,
                    scan_step_hz,
                    mode,
                } => {
                    let is_configured = cfg.ranges.iter().any(|r| r.name == name);
                    if is_configured {
                        if let Some(r) = cfg.ranges.iter_mut().find(|r| r.name == name) {
                            r.center_freq_hz = center_freq_hz;
                            r.scan_start_hz = scan_start_hz;
                            r.scan_end_hz = scan_end_hz;
                            r.scan_step_hz = scan_step_hz;
                            r.mode = mode;
                            r.enabled = true;
                        }
                        current_temp_scan = None;
                    } else {
                        // Temporary scan profile (Airband scan, ISM scan, Radiosonde scan from popups):
                        // NEVER permanently push into cfg.ranges!
                        current_temp_scan = Some(name.clone());
                    }

                    // Purge any temporary profiles that may have leaked into cfg.ranges
                    cfg.ranges.retain(|r| !crate::config::is_temporary_scan_profile(&r.name));

                    cfg.active_range = name;
                    cfg.center_freq_hz = center_freq_hz;
                    cfg.scan_start_hz = scan_start_hz;
                    cfg.scan_end_hz = scan_end_hz;
                    cfg.scan_step_hz = scan_step_hz;
                    let _ = device.set_frequency(center_freq_hz);
                    while sample_rx.try_recv().is_ok() {}
                    settling_until = Instant::now() + Duration::from_millis(40);
                    warm_up_frames = 1;

                    // Unpin any leftover manual VFOs so the scanner is immediately free to sweep the new band
                    engine.unpin_all_manual_vfos();
                    engine.switch_range(scan_start_hz, scan_end_hz, center_freq_hz, scan_step_hz, mode);
                    last_range_hop = Instant::now();
                    noise_floor.reset();
                }
                DspCommand::RestoreConfiguredBands { active_range } => {
                    current_temp_scan = None;
                    cfg.ranges.retain(|r| !crate::config::is_temporary_scan_profile(&r.name));

                    // Reset any open-squelch data VFOs or pinned VFOs back to Idle
                    for vfo in &mut engine.vfos {
                        if vfo.squelch_open || vfo.state == VfoState::Pinned {
                            vfo.squelch_open = false;
                            vfo.state = VfoState::Idle;
                            vfo.active = false;
                            vfo.muted = false;
                            vfo.snr_db = 0.0;
                        }
                    }

                    let target = cfg.ranges.iter().find(|r| r.name == active_range)
                        .or_else(|| cfg.ranges.iter().find(|r| r.enabled))
                        .or_else(|| cfg.ranges.first())
                        .cloned();

                    if let Some(target) = target {
                        let usable_bw = ((cfg.sample_rate as f64) * 0.85).round() as u64;
                        let target_is_wide = (target.scan_end_hz - target.scan_start_hz) > usable_bw;
                        let target_center = if target_is_wide {
                            target.scan_start_hz + (usable_bw / 2)
                        } else {
                            target.center_freq_hz
                        };

                        cfg.active_range = target.name.clone();
                        cfg.center_freq_hz = target_center;
                        cfg.scan_start_hz = target.scan_start_hz;
                        cfg.scan_end_hz = target.scan_end_hz;
                        cfg.scan_step_hz = target.scan_step_hz;

                        let _ = device.set_frequency(target_center);
                        while sample_rx.try_recv().is_ok() {}
                        settling_until = Instant::now() + Duration::from_millis(40);
                        warm_up_frames = 1;

                        engine.switch_range(
                            target.scan_start_hz,
                            target.scan_end_hz,
                            target_center,
                            target.scan_step_hz,
                            target.mode,
                        );
                        noise_floor.reset();
                    }

                    engine.set_scanner_state(ScannerState::Paused);
                    last_range_hop = Instant::now();
                }
                DspCommand::SetRangeEnabled { name, enabled } => {
                    if let Some(r) = cfg.ranges.iter_mut().find(|r| r.name == name) {
                        r.enabled = enabled;
                    }
                    if !enabled && cfg.active_range == name {
                        // User unchecked the active band: hop immediately to first enabled band!
                        if let Some(target) = cfg.ranges.iter().find(|r| r.enabled).cloned() {
                            let usable_bw = ((cfg.sample_rate as f64) * 0.85).round() as u64;
                            let target_is_wide = (target.scan_end_hz - target.scan_start_hz) > usable_bw;
                            let target_center = if target_is_wide {
                                target.scan_start_hz + (usable_bw / 2)
                            } else {
                                target.center_freq_hz
                            };
                            cfg.active_range = target.name.clone();
                            cfg.center_freq_hz = target_center;
                            cfg.scan_start_hz = target.scan_start_hz;
                            cfg.scan_end_hz = target.scan_end_hz;
                            cfg.scan_step_hz = target.scan_step_hz;
                            let _ = device.set_frequency(target_center);
                            while sample_rx.try_recv().is_ok() {}
                            settling_until = Instant::now() + Duration::from_millis(40);
                            warm_up_frames = 1;
                            engine.switch_range(
                                target.scan_start_hz,
                                target.scan_end_hz,
                                target_center,
                                target.scan_step_hz,
                                target.mode,
                            );
                            last_range_hop = Instant::now();
                            noise_floor.reset();
                        }
                    }
                }
                DspCommand::SetMaxVfos(max) => {
                    cfg.max_vfos = max;
                    engine.set_max_vfos(max);
                }
                DspCommand::SetHangoverMs(ms) => {
                    cfg.hangover_ms = ms;
                }
                DspCommand::SwitchDevice(selection) => {
                    let _ = device.stop();
                    current_selection = selection;
                    match selection {
                        DeviceSelection::AirspyR2 => {
                            cfg.sample_rate = 10_000_000;
                            match AirspyDevice::new() {
                                Ok(mut airspy) => {
                                    let _ = airspy.set_sample_rate(cfg.sample_rate);
                                    let _ = airspy.set_frequency(cfg.center_freq_hz);
                                    let _ = airspy.set_gain(cfg.rf_gain);
                                    if airspy.start(sample_tx.clone()).is_ok() {
                                        status_message = format!("Airspy R2 ({:.1} MSPS) ● RUNNING", cfg.sample_rate as f64 / 1e6);
                                        device = Box::new(airspy);
                                    } else {
                                        status_message = "Airspy R2 käynnistys epäonnistui".to_string();
                                        device = Box::new(DisconnectedDevice { name: "Airspy R2 (Virhe)".to_string() });
                                    }
                                }
                                Err(e) => {
                                    status_message = format!("Airspy R2: {}", e);
                                    device = Box::new(DisconnectedDevice { name: "Airspy R2 (Ei yhteyttä)".to_string() });
                                }
                            }
                        }
                        DeviceSelection::RtlSdr => {
                            cfg.sample_rate = 2_048_000;
                            match RtlSdrDevice::new() {
                                Ok(mut rtl) => {
                                    let _ = rtl.set_sample_rate(cfg.sample_rate);
                                    let _ = rtl.set_frequency(cfg.center_freq_hz);
                                    let _ = rtl.set_gain(cfg.rf_gain);
                                    if rtl.start(sample_tx.clone()).is_ok() {
                                        status_message = format!("RTL-SDR ({:.2} MSPS) ● RUNNING", cfg.sample_rate as f64 / 1e6);
                                        device = Box::new(rtl);
                                    } else {
                                        status_message = "RTL-SDR start failed".to_string();
                                        device = Box::new(DisconnectedDevice { name: "RTL-SDR (Error)".to_string() });
                                    }
                                }
                                Err(e) => {
                                    status_message = format!("RTL-SDR: {}", e);
                                    device = Box::new(DisconnectedDevice { name: "RTL-SDR (Disconnected)".to_string() });
                                }
                            }
                        }
                        DeviceSelection::SimulatedRf => {
                            cfg.sample_rate = 2_500_000;
                            let mut mock = MockDevice::new();
                            let _ = mock.set_frequency(cfg.center_freq_hz);
                            let _ = mock.start(sample_tx.clone());
                            device = Box::new(mock);
                            status_message = "Test Generator (Simulated RF) ● RUNNING".to_string();
                        }
                    }
                }
            }
        }

        // Automatic reconnect retry if physical device is disconnected
        if !device.is_running() && last_reconnect_attempt.elapsed() > Duration::from_millis(2000) {
            last_reconnect_attempt = Instant::now();
            match current_selection {
                DeviceSelection::AirspyR2 => {
                    if let Ok(mut airspy) = AirspyDevice::new() {
                        let _ = airspy.set_sample_rate(cfg.sample_rate);
                        let _ = airspy.set_frequency(cfg.center_freq_hz);
                        let _ = airspy.set_gain(cfg.rf_gain);
                        if airspy.start(sample_tx.clone()).is_ok() {
                            status_message = format!("Airspy R2 ({:.1} MSPS) ● RUNNING", cfg.sample_rate as f64 / 1e6);
                            device = Box::new(airspy);
                        }
                    }
                }
                DeviceSelection::RtlSdr => {
                    if let Ok(mut rtl) = RtlSdrDevice::new() {
                        let _ = rtl.set_sample_rate(cfg.sample_rate);
                        let _ = rtl.set_frequency(cfg.center_freq_hz);
                        let _ = rtl.set_gain(cfg.rf_gain);
                        if rtl.start(sample_tx.clone()).is_ok() {
                            status_message = format!("RTL-SDR ({:.2} MSPS) ● RUNNING", cfg.sample_rate as f64 / 1e6);
                            device = Box::new(rtl);
                        }
                    }
                }
                _ => {}
            }
        }

        // 2. Receive RF sample batches from hardware SDR
        let mut sample_batch: Option<Vec<Complex32>> = None;
        let mut batches_count = 0;

        // Drain up to 16 batches per loop cycle to maintain real-time throughput
        let aprs_freq_hz: u64 = 144_800_000;
        let half_rate = (cfg.sample_rate / 2) as u64;
        let min_sdr_f = cfg.center_freq_hz.saturating_sub(half_rate);
        let max_sdr_f = cfg.center_freq_hz + half_rate;
        let aprs_in_band = aprs_freq_hz >= min_sdr_f && aprs_freq_hz <= max_sdr_f;

        let ais_1_freq: u64 = 161_975_000;
        let ais_2_freq: u64 = 162_025_000;
        let ais_1_in_band = ais_1_freq >= min_sdr_f && ais_1_freq <= max_sdr_f;
        let ais_2_in_band = ais_2_freq >= min_sdr_f && ais_2_freq <= max_sdr_f;

        if cfg.center_freq_hz != last_aprs_center_freq || cfg.sample_rate != last_aprs_sample_rate {
            last_aprs_center_freq = cfg.center_freq_hz;
            last_aprs_sample_rate = cfg.sample_rate;
            if aprs_in_band {
                aprs_ddc = crate::dsp::DigitalDownConverter::new(cfg.sample_rate as f32, 48_000.0, 16_000.0);
                let offset_hz = aprs_freq_hz as f64 - cfg.center_freq_hz as f64;
                aprs_ddc.set_freq_offset(offset_hz as f32);
            }
        }

        if ais_1_in_band && (cfg.center_freq_hz != last_ais_center_freq_a || cfg.sample_rate != last_ais_sample_rate) {
            last_ais_center_freq_a = cfg.center_freq_hz;
            ais_ddc_a = crate::dsp::DigitalDownConverter::new(cfg.sample_rate as f32, 48_000.0, 25_000.0);
            let offset_hz = ais_1_freq as f64 - cfg.center_freq_hz as f64;
            ais_ddc_a.set_freq_offset(offset_hz as f32);
        }
        if ais_2_in_band && (cfg.center_freq_hz != last_ais_center_freq_b || cfg.sample_rate != last_ais_sample_rate) {
            last_ais_center_freq_b = cfg.center_freq_hz;
            ais_ddc_b = crate::dsp::DigitalDownConverter::new(cfg.sample_rate as f32, 48_000.0, 25_000.0);
            let offset_hz = ais_2_freq as f64 - cfg.center_freq_hz as f64;
            ais_ddc_b.set_freq_offset(offset_hz as f32);
        }
        if cfg.sample_rate != last_ais_sample_rate {
            last_ais_sample_rate = cfg.sample_rate;
        }

        if Instant::now() < settling_until {
            // Physical hardware tuner PLL settling (Airspy R820T2 takes ~60-80 ms + USB FIFO drain)
            // Drain transient samples from queue and stream silence to audio output
            let mut drained_any = false;
            while let Ok(batch) = sample_rx.try_recv() {
                drained_any = true;
                if let Some(a) = &mut audio {
                    audio_buffer.clear();
                    audio_buffer.resize(batch.len(), 0.0);
                    a.push_samples(&audio_buffer, master_volume);
                }
            }
            if !drained_any {
                thread::sleep(Duration::from_millis(2));
            }
            continue;
        }

        while let Ok(batch) = sample_rx.try_recv() {
            // Process audio across all active VFOs
            engine.process_samples(&batch, &mut audio_buffer, &cfg);

            // Stream audio to speakers
            if let Some(a) = &mut audio {
                if warm_up_frames > 0 {
                    for s in &mut audio_buffer {
                        *s = 0.0;
                    }
                }
                a.push_samples(&audio_buffer, master_volume);
            }

            // Real-time APRS 144.800 MHz tap (panic-isolated to guarantee worker audio/waterfall never freezes)
            if aprs_in_band && warm_up_frames == 0 {
                aprs_ddc.process(&batch, &mut aprs_ddc_buf);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    aprs_decoder.process_samples(&aprs_ddc_buf);
                }));
            }

            // Real-time 1090 MHz ADS-B Mode S tap (runs on entire SDR band unconditionally)
            let adsb_in_band = cfg.center_freq_hz >= 1_080_000_000 && cfg.center_freq_hz <= 1_100_000_000;
            if adsb_in_band && warm_up_frames == 0 {
                let msgs = adsb_decoder.process_iq(&batch, cfg.sample_rate as f32, 10.0);
                for m in msgs {
                    let pkt = crate::dsp::convert_adsb_to_air_packet_with_freq(&m, 10.0, cfg.center_freq_hz);
                    direct_adsb_packets.push(pkt);
                    if direct_adsb_packets.len() > 100 {
                        direct_adsb_packets.remove(0);
                    }
                }
            }

            // Real-time Dual-Channel Marine AIS Tap (161.975 & 162.025 MHz)
            if (ais_1_in_band || ais_2_in_band) && warm_up_frames == 0 {
                if ais_1_in_band {
                    ais_ddc_a.process(&batch, &mut ais_ddc_buf_a);
                    let pwr_a: f32 = ais_ddc_buf_a.iter().map(|s| s.norm_sqr()).sum::<f32>() / ais_ddc_buf_a.len().max(1) as f32;
                    let est_snr_a = (10.0 * (pwr_a / 1e-6).max(0.1).log10()).clamp(0.0, 40.0);
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        ais_decoder_a.process_samples(&ais_ddc_buf_a, ais_1_freq, est_snr_a);
                    }));
                    for pkt in ais_decoder_a.packets.drain(..) {
                        direct_ais_packets.push(pkt);
                        if direct_ais_packets.len() > 100 {
                            direct_ais_packets.remove(0);
                        }
                    }
                }
                if ais_2_in_band {
                    ais_ddc_b.process(&batch, &mut ais_ddc_buf_b);
                    let pwr_b: f32 = ais_ddc_buf_b.iter().map(|s| s.norm_sqr()).sum::<f32>() / ais_ddc_buf_b.len().max(1) as f32;
                    let est_snr_b = (10.0 * (pwr_b / 1e-6).max(0.1).log10()).clamp(0.0, 40.0);
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        ais_decoder_b.process_samples(&ais_ddc_buf_b, ais_2_freq, est_snr_b);
                    }));
                    for pkt in ais_decoder_b.packets.drain(..) {
                        direct_ais_packets.push(pkt);
                        if direct_ais_packets.len() > 100 {
                            direct_ais_packets.remove(0);
                        }
                    }
                }
            }

            sample_batch = Some(batch);
            batches_count += 1;
            if batches_count >= 16 {
                break;
            }
        }

        // If no samples arrived yet, sleep briefly to avoid burning 100% spinlock
        if batches_count == 0 {
            thread::sleep(Duration::from_millis(1));
            continue;
        }

        // 3. FFT Spectral Analysis, Display Smoothing & Scanner Evaluation
        if let Some(samples) = sample_batch {
            if samples.len() >= fft.size() {
                fft.compute_averaged_spectrum(&samples, &mut raw_spectrum, 8);

                if warm_up_frames > 0 {
                    smooth_spectrum.copy_from_slice(&raw_spectrum);
                    noise_floor.reset();
                    noise_floor.process(&smooth_spectrum, &mut pseudo_spectrum);
                    warm_up_frames -= 1;
                } else {
                    for i in 0..fft_size {
                        let raw = raw_spectrum[i];
                        let prev = smooth_spectrum[i];
                        if raw > prev {
                            smooth_spectrum[i] = prev * 0.40 + raw * 0.60;
                        } else {
                            smooth_spectrum[i] = prev * 0.85 + raw * 0.15;
                        }
                    }
                }

                noise_floor.process(&smooth_spectrum, &mut pseudo_spectrum);

                // Run high-speed multi-VFO scanner!
                engine.evaluate_spectrum(&pseudo_spectrum, &cfg);

                // Wideband chunk stepping & Multi-Range Sweep Hop logic:
                let usable_bw = ((cfg.sample_rate as f64) * 0.85).round() as u64;
                let current_chunk_max = cfg.center_freq_hz + (usable_bw / 2);

                let dwell_time = last_range_hop.elapsed();
                let has_traffic = engine.has_active_traffic();

                let chunk_finished = engine.pass_completed;
                let band_has_more_chunks = current_chunk_max + (cfg.scan_step_hz as u64 * 4) < cfg.scan_end_hz;

                let enabled_ranges: Vec<usize> = cfg
                    .ranges
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| r.enabled)
                    .map(|(i, _)| i)
                    .collect();

                let has_pinned_vfo = engine.vfos.iter().any(|v| v.state == VfoState::Pinned);

                if engine.state == ScannerState::Scanning {
                    let active_is_enabled = enabled_ranges.iter().any(|&i| cfg.ranges[i].name == cfg.active_range);
                    if !active_is_enabled && current_temp_scan.is_none() {
                        // Ensure active configured range is marked enabled so user's choice is never overridden
                        if let Some(r) = cfg.ranges.iter_mut().find(|r| r.name == cfg.active_range) {
                            r.enabled = true;
                        }
                    } else if has_pinned_vfo {
                        // When any VFO is pinned (paused by user), hold receiver locked on this 10 MHz chunk!
                        // Do not step chunks and do not hop bands. The pinned VFO remains audible and unmuted.
                        // The engine sweeps channels locally within this 10 MHz passband.
                        last_range_hop = Instant::now();
                        engine.pass_completed = false;
                    } else {
                        // Strictly hold receiver while traffic is active! Only advance chunks/bands when idle.
                        let chunk_done = !has_traffic && ((chunk_finished && dwell_time >= Duration::from_millis(150)) || dwell_time >= Duration::from_millis(1500));

                        if chunk_done {
                            if band_has_more_chunks {
                                // Step to next frequency chunk within this wide band (> 8.5-10 MHz)
                                engine.pass_completed = false;
                                last_range_hop = Instant::now();
                                let next_center = (cfg.center_freq_hz + usable_bw).min(cfg.scan_end_hz.saturating_sub(usable_bw / 2));
                                cfg.center_freq_hz = next_center;
                                let _ = device.set_frequency(next_center);
                                while sample_rx.try_recv().is_ok() {}
                                settling_until = Instant::now() + Duration::from_millis(40);
                                warm_up_frames = 1;
                                let cur_mode = engine.vfos.first().map(|v| v.mode).unwrap_or(DemodMode::AM);
                                engine.switch_range(cfg.scan_start_hz, cfg.scan_end_hz, next_center, cfg.scan_step_hz, cur_mode);
                                noise_floor.reset();
                            } else if current_temp_scan.is_none() && enabled_ranges.len() > 1 {
                                // Current configured band has finished all chunks, hop to next enabled band profile!
                                engine.pass_completed = false;

                                let cur_idx_in_enabled = enabled_ranges
                                    .iter()
                                    .position(|&i| cfg.ranges[i].name == cfg.active_range);
                                let next_enabled_slot = match cur_idx_in_enabled {
                                    Some(pos) => (pos + 1) % enabled_ranges.len(),
                                    None => 0,
                                };
                                let target_range_idx = enabled_ranges[next_enabled_slot];
                                let target = cfg.ranges[target_range_idx].clone();

                                let target_is_wide = (target.scan_end_hz - target.scan_start_hz) > usable_bw;
                                let target_center = if target_is_wide {
                                    target.scan_start_hz + (usable_bw / 2)
                                } else {
                                    target.center_freq_hz
                                };

                                cfg.active_range = target.name.clone();
                                cfg.center_freq_hz = target_center;
                                cfg.scan_start_hz = target.scan_start_hz;
                                cfg.scan_end_hz = target.scan_end_hz;
                                cfg.scan_step_hz = target.scan_step_hz;

                                let _ = device.set_frequency(target_center);
                                while sample_rx.try_recv().is_ok() {}
                                settling_until = Instant::now() + Duration::from_millis(40);
                                warm_up_frames = 1;
                                engine.switch_range(
                                    target.scan_start_hz,
                                    target.scan_end_hz,
                                    target_center,
                                    target.scan_step_hz,
                                    target.mode,
                                );
                                last_range_hop = Instant::now();
                                noise_floor.reset();
                            } else if (cfg.scan_end_hz - cfg.scan_start_hz) > usable_bw {
                                // Only 1 band enabled and it reached the end: wrap back to Chunk 0
                                let first_center = cfg.scan_start_hz + (usable_bw / 2);
                                if (cfg.center_freq_hz as i64 - first_center as i64).abs() > (usable_bw as i64 / 4) {
                                    engine.pass_completed = false;
                                    last_range_hop = Instant::now();
                                    cfg.center_freq_hz = first_center;
                                    let _ = device.set_frequency(first_center);
                                    while sample_rx.try_recv().is_ok() {}
                                    settling_until = Instant::now() + Duration::from_millis(40);
                                    warm_up_frames = 1;
                                    let cur_mode = cfg.ranges.iter().find(|r| r.name == cfg.active_range).map(|r| r.mode).unwrap_or(DemodMode::AM);
                                    engine.switch_range(cfg.scan_start_hz, cfg.scan_end_hz, first_center, cfg.scan_step_hz, cur_mode);
                                    noise_floor.reset();
                                }
                            } else {
                                engine.pass_completed = false;
                            }
                        }
                    }
                }
            }
        }

        // 4. Send Display Snapshot to GUI thread at ~40 FPS
        if last_display_time.elapsed() >= display_interval {
            if Instant::now() < settling_until || warm_up_frames > 0 {
                // DO NOT send display snapshot while receiver is retuning or calibrating!
                continue;
            }
            last_display_time = Instant::now();

            let now = Instant::now();
            let vfos_info: Vec<VfoDisplayInfo> = engine
                .vfos
                .iter()
                .map(|v| {
                    let delay_sec = if v.state == VfoState::Locked && !v.active && v.hangover_until > now {
                        (v.hangover_until - now).as_secs_f32()
                    } else {
                        0.0
                    };
                    VfoDisplayInfo {
                        id: v.id,
                        freq_hz: v.freq_hz,
                        name: v.name.clone(),
                        mode: v.mode,
                        state: v.state,
                        snr_db: v.snr_db,
                        vad_score: v.vad_score,
                        active: v.active,
                        is_out_of_band: v.is_out_of_band,
                        muted: v.muted,
                        volume: v.volume,
                        delay_remaining_sec: delay_sec,
                        dmr_info: v.dmr_info.clone(),
                        latest_telemetry_info: if v.dmr_info.is_some() || v.dmr_decoder.is_digital_active() {
                            None
                        } else if let Some(last_pkt) = &v.ais_decoder.latest_packet {
                            Some(format!("AIS: {}", last_pkt.human_summary))
                        } else if let Some(last_time) = v.telemetry_decoder.last_packet_time {
                            if last_time.elapsed() < std::time::Duration::from_secs(4) {
                                v.telemetry_decoder.packets.back().map(|p| format!("{}: {}", p.protocol, p.summary))
                            } else {
                                None
                            }
                        } else {
                            None
                        },
                        audio_waveform: v.audio_waveform,
                        squelch_open: v.squelch_open,
                    }
                })
                .collect();

            let mut latest_dmr = None;
            let mut all_dmr_packets = Vec::new();
            let mut all_telemetry_packets = Vec::new();
            let mut all_air_packets = Vec::new();
            let mut all_ais_packets = Vec::new();
            for v in &mut engine.vfos {
                if let Some(pkt) = v.dmr_decoder.packets.last() {
                    if latest_dmr.is_none() {
                        latest_dmr = Some(pkt.clone());
                    }
                }
                all_dmr_packets.extend(v.dmr_decoder.packets.clone());
                all_telemetry_packets.extend(v.telemetry_decoder.packets.clone());
                all_air_packets.extend(std::mem::take(&mut v.air_decoder.packets));
                all_ais_packets.extend(std::mem::take(&mut v.ais_decoder.packets));
            }
            all_air_packets.extend(std::mem::take(&mut direct_adsb_packets));
            all_ais_packets.extend(std::mem::take(&mut direct_ais_packets));
            all_dmr_packets.sort_by_key(|p| p.timestamp);
            if all_dmr_packets.len() > 32 {
                let start = all_dmr_packets.len() - 32;
                all_dmr_packets = all_dmr_packets[start..].to_vec();
            }

            all_telemetry_packets.sort_by_key(|p| p.id);
            if all_telemetry_packets.len() > 200 {
                let start = all_telemetry_packets.len() - 200;
                all_telemetry_packets = all_telemetry_packets[start..].to_vec();
            }
            let latest_telemetry = all_telemetry_packets.last().cloned();

            all_air_packets.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
            if all_air_packets.len() > 100 {
                let start = all_air_packets.len() - 100;
                all_air_packets = all_air_packets[start..].to_vec();
            }
            let latest_air = all_air_packets.last().cloned();

            all_ais_packets.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
            if all_ais_packets.len() > 100 {
                let start = all_ais_packets.len() - 100;
                all_ais_packets = all_ais_packets[start..].to_vec();
            }
            let latest_ais = all_ais_packets.last().cloned();

            let vdl2_burst_stats = engine
                .vfos
                .iter()
                .find(|v| v.freq_hz == 136_975_000 || v.air_decoder.vdl2_burst_count > 0)
                .map(|v| (v.air_decoder.vdl2_burst_count, v.air_decoder.last_vdl2_snr, v.air_decoder.last_vdl2_dur_ms));

            let snapshot = DisplaySnapshot {
                raw_spectrum: smooth_spectrum.clone(),
                pseudo_spectrum: pseudo_spectrum.clone(),
                noise_baseline: noise_floor.baseline().to_vec(),
                scan_cursor_hz: engine.scan_cursor_hz,
                vfos: vfos_info,
                center_freq_hz: cfg.center_freq_hz,
                sample_rate_hz: cfg.sample_rate,
                status_message: if device.is_running() {
                    format!(
                        "{} ({:.1} MSPS)",
                        device.name(),
                        cfg.sample_rate as f64 / 1e6
                    )
                } else {
                    status_message.clone()
                },
                device_running: device.is_running(),
                active_range: cfg.active_range.clone(),
                latest_aprs: aprs_decoder.latest_packet.clone(),
                aprs_packets: aprs_decoder.packets.clone(),
                latest_dmr,
                dmr_packets: all_dmr_packets,
                latest_telemetry,
                telemetry_packets: all_telemetry_packets,
                latest_air,
                air_packets: all_air_packets,
                latest_ais,
                ais_packets: all_ais_packets,
                current_audio_device: current_audio_device.clone(),
                blacklist: engine.blacklist.clone(),
                vdl2_burst_stats,
            };

            // Non-blocking try_send: drops frame if GUI is currently redrawing
            let _ = display_tx.try_send(snapshot);
        }
    }

    let _ = device.stop();
}
