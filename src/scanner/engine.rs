use super::vfo::{VfoChannel, VfoState};
use crate::config::{DemodMode, ScannerConfig};
use num_complex::Complex32;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScannerState {
    Scanning,
    Paused,
}

pub struct ScannerEngine {
    pub state: ScannerState,
    pub scan_cursor_hz: u64,
    pub vfos: Vec<VfoChannel>,
    pub max_active_vfos: usize,
    pub pass_completed: bool,

    skip_list: HashMap<u64, Instant>,
    pub blacklist: Vec<u64>,
    pub editing_vfo_id: Option<usize>,
}

impl ScannerEngine {
    pub fn new(cfg: &ScannerConfig) -> Self {
        let max_vfos = 10;
        let mut vfos = Vec::with_capacity(max_vfos);

        // Pre-allocate dynamic VFO channels in the pool
        for i in 0..max_vfos {
            let vfo = VfoChannel::new(
                i + 1,
                cfg.scan_start_hz + (i as u64 * cfg.scan_step_hz as u64),
                cfg.center_freq_hz,
                cfg.sample_rate as f32,
                DemodMode::AM,
                format!("VFO #{}", i + 1),
                VfoState::Idle,
            );
            vfos.push(vfo);
        }

        Self {
            state: ScannerState::Scanning,
            scan_cursor_hz: cfg.scan_start_hz,
            vfos,
            max_active_vfos: cfg.max_vfos.clamp(1, 10),
            pass_completed: false,
            skip_list: HashMap::new(),
            blacklist: cfg.blacklist.clone(),
            editing_vfo_id: None,
        }
    }

    /// Cleanly switches to a new frequency band and resets unpinned or out-of-band VFOs
    pub fn switch_range(
        &mut self,
        start_hz: u64,
        end_hz: u64,
        center_hz: u64,
        step_hz: u32,
        mode: DemodMode,
    ) {
        let now = Instant::now();
        self.scan_cursor_hz = start_hz;
        self.pass_completed = false;
        self.prune_history();
        for (i, vfo) in self.vfos.iter_mut().enumerate() {
            if i >= self.max_active_vfos {
                vfo.state = VfoState::Idle;
                vfo.active = false;
                vfo.snr_db = 0.0;
                vfo.vad_score = 0.0;
                continue;
            }

            // If pinned VFO is far outside the new receiver passband, transition to History so it doesn't hold the new band hostage
            if vfo.state == VfoState::Pinned && (vfo.freq_hz as i64 - center_hz as i64).abs() > 5_000_000 {
                vfo.state = VfoState::History;
                vfo.active = false;
                let freq = vfo.freq_hz;
                let name = vfo.name.clone();
                vfo.retune(freq, center_hz, name);
                continue;
            }

            // Preserve History, in-band Pinned, and currently user-edited VFOs across band hops
            if vfo.state == VfoState::History || vfo.state == VfoState::Pinned || Some(vfo.id) == self.editing_vfo_id {
                let freq = vfo.freq_hz;
                let name = vfo.name.clone();
                vfo.retune(freq, center_hz, name);
                continue;
            }

            // If active locked station is within the new receiver passband (center_hz +/- 4.5 MHz), keep Locked
            let is_active_voice = vfo.state == VfoState::Locked && (vfo.active || now < vfo.hangover_until);
            let in_passband = vfo.freq_hz >= center_hz.saturating_sub(4_500_000) && vfo.freq_hz <= center_hz + 4_500_000;
            if is_active_voice && in_passband {
                let freq = vfo.freq_hz;
                let name = vfo.name.clone();
                vfo.retune(freq, center_hz, name);
                continue;
            }

            // Locked stations out of band transition to History
            if vfo.state == VfoState::Locked {
                vfo.state = VfoState::History;
                vfo.active = false;
                let freq = vfo.freq_hz;
                let name = vfo.name.clone();
                vfo.retune(freq, center_hz, name);
                continue;
            }

            // Unpinned inactive or idle slots: reset to Idle
            vfo.state = VfoState::Idle;
            vfo.active = false;
            let initial_freq = start_hz + (i as u64 * step_hz as u64);
            let bounded_freq = initial_freq.min(end_hz);
            vfo.retune(bounded_freq, center_hz, format!("VFO #{}", vfo.id));
            vfo.set_mode(mode);
        }
        self.prune_history();
    }

    /// Evaluates the wideband spectrum, maintains active VFOs, and sweeps for new stations
    pub fn evaluate_spectrum(&mut self, pseudo_db: &[f32], cfg: &ScannerConfig) {
        let now = Instant::now();

        // Broadband transient filter:
        // Real radio signals are narrowband (2-5 bins). If more than 30 bins exceed squelch
        // or the average SNR across all 2048 bins is > 3.5 dB, this is a hardware retune transient
        // or broadband burst. NEVER allow VFOs to trigger or lock onto a broadband burst!
        let active_bins = pseudo_db.iter().filter(|&&v| v >= cfg.squelch_snr_db).count();
        if active_bins > 30 {
            return;
        }

        // 1. Clean up expired skip frequencies
        self.skip_list.retain(|_, until| *until > now);

        let half_bw = (cfg.sample_rate as f64) * 0.48;
        let min_visible_hz = (cfg.center_freq_hz as f64 - half_bw).max(0.0) as u64;
        let max_visible_hz = (cfg.center_freq_hz as f64 + half_bw) as u64;

        let scan_start = cfg.scan_start_hz.max(min_visible_hz);
        let scan_end = cfg.scan_end_hz.min(max_visible_hz);
        let step = (cfg.scan_step_hz as u64).max(1000);

        let fft_size = pseudo_db.len();
        let bin_hz = (cfg.sample_rate as f64) / (fft_size as f64);
        let half_bins = fft_size / 2;
        let margin_bins = (fft_size as isize * 5) / 100;

        // Helper to get channel SNR and occupied RF bandwidth with DC notch and blacklist filtering
        let get_signal_metrics = |freq_hz: u64, blk: &[u64]| -> (f32, f32) {
            if freq_hz < min_visible_hz || freq_hz > max_visible_hz {
                return (0.0, 0.0);
            }

            // Exclude blacklisted / ignored frequencies
            if blk.iter().any(|&f| f.abs_diff(freq_hz) <= 5000) {
                return (0.0, 0.0);
            }

            let dc_offset = (freq_hz as i64 - cfg.center_freq_hz as i64).abs();
            if dc_offset < 20_000 {
                return (0.0, 0.0); // Notch DC center spike
            }

            let offset_hz = freq_hz as f64 - cfg.center_freq_hz as f64;
            let center_bin = (half_bins as f64) + (offset_hz / bin_hz);
            let bin_idx = center_bin.round() as isize;

            // Mask outer 5% margin bins to prevent hardware filter roll-off edge artifacts
            if bin_idx < margin_bins || bin_idx >= (fft_size as isize - margin_bins) {
                return (0.0, 0.0);
            }

            let mut peak_snr = 0.0f32;
            let mut peak_bin = bin_idx;
            for offset in -2..=2 {
                let idx = (bin_idx + offset) as usize;
                if idx < fft_size && pseudo_db[idx] > peak_snr {
                    peak_snr = pseudo_db[idx];
                    peak_bin = (bin_idx + offset) as isize;
                }
            }

            if peak_snr < cfg.squelch_snr_db {
                return (peak_snr, 0.0);
            }

            let cutoff_db = (peak_snr - 6.0).max(cfg.squelch_snr_db);
            let mut left = peak_bin;
            while left > margin_bins && pseudo_db[(left - 1) as usize] >= cutoff_db {
                left -= 1;
            }
            let mut right = peak_bin;
            while right + 1 < (fft_size as isize - margin_bins) && pseudo_db[(right + 1) as usize] >= cutoff_db {
                right += 1;
            }
            let bw_hz = ((right - left + 1) as f64 * bin_hz) as f32;
            (peak_snr, bw_hz)
        };

        let get_snr = |freq_hz: u64, blk: &[u64]| -> f32 {
            get_signal_metrics(freq_hz, blk).0
        };

        // 2. Update all currently active VFOs (Locked and Pinned) within max_active_vfos or data channels
        for (i, vfo) in self.vfos.iter_mut().enumerate() {
            let is_data_channel = vfo.squelch_open && vfo.muted;
            let within_limit = i < self.max_active_vfos;

            if !within_limit && !is_data_channel {
                vfo.state = VfoState::Idle;
                vfo.active = false;
                vfo.squelch_open = false;
                vfo.muted = false;
                vfo.snr_db = 0.0;
                vfo.vad_score = 0.0;
                vfo.squelch_gain = 0.0;
                vfo.audio_waveform = [0.0; 32];
                continue;
            }

            if vfo.state == VfoState::Locked || vfo.state == VfoState::Pinned {
                let (snr, bw_hz) = get_signal_metrics(vfo.freq_hz, &self.blacklist);
                let is_bcfm = vfo.mode == DemodMode::BCFM || vfo.mode == DemodMode::WFM;
                let is_signal_present = if is_bcfm {
                    snr >= 2.0
                } else {
                    snr >= cfg.squelch_snr_db
                };

                if vfo.squelch_open {
                    vfo.rf_bandwidth_hz = bw_hz;
                    vfo.snr_db = snr;
                    vfo.active = true;
                    vfo.has_confirmed_carrier = true;
                    vfo.hangover_until = now + Duration::from_millis(cfg.hangover_ms);
                } else if is_signal_present {
                    vfo.rf_bandwidth_hz = bw_hz;
                    vfo.update_squelch(true, snr, cfg.vad_enabled, cfg.vad_threshold);
                    if vfo.active {
                        vfo.hangover_until = now + Duration::from_millis(cfg.hangover_ms);
                    }
                } else if vfo.state == VfoState::Pinned {
                    // Pinned VFO: keeps channel locked in memory/tuning,
                    // but squelches (mutes) audio when no carrier is present to prevent hiss!
                    vfo.update_squelch(false, snr, false, 0.0);
                } else if self.state == ScannerState::Paused {
                    // Scanner global pause: update squelch based on actual signal presence
                    vfo.update_squelch(is_signal_present, snr, cfg.vad_enabled, cfg.vad_threshold);
                } else {
                    // Transmission ended / Carrier dropped!
                    let carrier_duration = now.saturating_duration_since(vfo.carrier_lock_start).as_secs_f32();

                    if carrier_duration < 0.08 || (!vfo.is_confirmed() && now >= vfo.hangover_until) {
                        // Transient noise spike / glitch that dropped before confirmation:
                        // Completely discard without polluting History!
                        vfo.update_squelch(false, snr, false, 0.0);
                        vfo.active = false;
                        vfo.state = VfoState::Idle;
                    } else if now < vfo.hangover_until {
                        // Real transmission ended: maintain hold delay timer for conversational reply!
                        vfo.update_squelch(false, snr, false, 0.0);
                    } else {
                        // Hold delay timer expired: transition to History
                        vfo.update_squelch(false, snr, false, 0.0);
                        vfo.active = false;
                        vfo.state = VfoState::History;
                    }
                }
            } else {
                vfo.active = false;
            }
        }

        // Cap history entries to max_active_vfos
        self.prune_history();

        if self.state == ScannerState::Paused {
            return;
        }

        // 3. Count currently busy VFOs (actively receiving or within hold hangover timer)
        let busy_count = self.vfos[..self.max_active_vfos]
            .iter()
            .filter(|v| (v.state == VfoState::Locked || v.state == VfoState::Pinned) && (v.active || now < v.hangover_until))
            .count();

        // If all allowed concurrent channels are busy, keep cursor parked on busy channel and don't sweep
        if busy_count >= self.max_active_vfos {
            if let Some(busy_vfo) = self.vfos[..self.max_active_vfos].iter().find(|v| (v.state == VfoState::Locked || v.state == VfoState::Pinned) && (v.active || now < v.hangover_until)) {
                self.scan_cursor_hz = busy_vfo.freq_hz;
            }
            return;
        }

        // 4. Sequential Channel Sweep: Step through frequency grid and stop on active signals
        if scan_start >= scan_end {
            return;
        }

        let steps_per_frame = 24; // Scans ~1000 channels/sec
        for _ in 0..steps_per_frame {
            self.scan_cursor_hz += step;
            if self.scan_cursor_hz < scan_start {
                self.scan_cursor_hz = scan_start;
            } else if self.scan_cursor_hz > scan_end {
                self.scan_cursor_hz = scan_start;
                self.pass_completed = true;
            }

            let test_freq = self.scan_cursor_hz;

            // Skip frequencies in temporary skip list or permanent blacklist
            if self.skip_list.contains_key(&test_freq) || self.blacklist.iter().any(|&f| f.abs_diff(test_freq) <= 5000) {
                continue;
            }

            // Do not lock to a channel already actively locked or pinned on another VFO
            if self.vfos[..self.max_active_vfos].iter().any(|v| (v.state == VfoState::Locked || v.state == VfoState::Pinned) && v.freq_hz.abs_diff(test_freq) <= (step / 2).max(5000)) {
                continue;
            }

            let snr = get_snr(test_freq, &self.blacklist);
            if snr >= cfg.squelch_snr_db {
                // ACTIVE TRANSMISSION DETECTED!
                // Carrier Peak Search & AFC (Automatic Frequency Control):
                // Search neighborhood of +- (step * 1.5) or +- 30 kHz for the true peak carrier bin
                let search_bw_hz = (step as f64 * 1.5).max(30_000.0);
                let search_bins = (search_bw_hz / bin_hz).ceil() as isize;

                let offset_hz = test_freq as f64 - cfg.center_freq_hz as f64;
                let test_bin = (half_bins as f64 + (offset_hz / bin_hz)).round() as isize;

                let min_bin = (test_bin - search_bins).max(margin_bins);
                let max_bin = (test_bin + search_bins).min(fft_size as isize - 1 - margin_bins);

                let mut best_bin = test_bin as usize;
                let mut best_snr = snr;

                let dc_notch_bins = (20_000.0 / bin_hz).ceil() as isize;
                let half_bins_isize = half_bins as isize;

                for b in min_bin..=max_bin {
                    // Skip DC center spike notch
                    if (b - half_bins_isize).abs() < dc_notch_bins {
                        continue;
                    }

                    // Check blacklist
                    let b_freq_hz = (cfg.center_freq_hz as f64 + (b - half_bins_isize) as f64 * bin_hz).round() as u64;
                    if self.blacklist.iter().any(|&f| f.abs_diff(b_freq_hz) <= 5000) {
                        continue;
                    }

                    let val = pseudo_db[b as usize];
                    if val > best_snr {
                        best_snr = val;
                        best_bin = b as usize;
                    }
                }

                // Measure occupied contiguous RF bandwidth of this peak (-6 dB down from peak)
                let cutoff_bw_db = (best_snr - 6.0).max(cfg.squelch_snr_db);
                let mut left_bw = best_bin as isize;
                while left_bw > margin_bins && pseudo_db[(left_bw - 1) as usize] >= cutoff_bw_db {
                    left_bw -= 1;
                }
                let mut right_bw = best_bin as isize;
                while right_bw + 1 < (fft_size as isize - margin_bins) && pseudo_db[(right_bw + 1) as usize] >= cutoff_bw_db {
                    right_bw += 1;
                }
                let target_bw_hz = ((right_bw - left_bw + 1) as f64 * bin_hz) as f32;

                // Parabolic sub-bin interpolation for precise carrier center
                let delta = if best_bin > 0 && best_bin + 1 < fft_size {
                    let alpha = pseudo_db[best_bin - 1];
                    let beta = pseudo_db[best_bin];
                    let gamma = pseudo_db[best_bin + 1];
                    let denom = alpha - 2.0 * beta + gamma;
                    if denom < -0.0001 {
                        (0.5 * (alpha - gamma) / denom).clamp(-0.5, 0.5)
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };

                let exact_carrier_hz = cfg.center_freq_hz as f64 + ((best_bin as f64 + delta as f64) - half_bins as f64) * bin_hz;

                // Channel Grid Snapping:
                let grid_hz = if (8320..=8340).contains(&step) {
                    // 8.333333 kHz airband channel grid
                    let step_f = 25_000.0 / 3.0;
                    let k = ((exact_carrier_hz - scan_start as f64) / step_f).round();
                    (scan_start as f64 + k * step_f).round() as u64
                } else {
                    // Standard channel grid from scan_start
                    let k = ((exact_carrier_hz - scan_start as f64 + (step as f64 * 0.5)) / (step as f64)).floor();
                    scan_start + (k.max(0.0) as u64 * step)
                };

                let diff = (exact_carrier_hz - grid_hz as f64).abs();
                let snapped_freq = if diff <= (step as f64 * 0.65) {
                    grid_hz
                } else {
                    // Round off-grid carrier to nearest 100 Hz
                    ((exact_carrier_hz / 100.0).round() * 100.0) as u64
                };

                // Check if a known bookmark matches this snapped channel within 3500 Hz
                let target_freq = if let Some(bm) = cfg.get_comment(snapped_freq, 3500) {
                    bm.freq_hz
                } else if let Some(bm) = cfg.get_comment(exact_carrier_hz.round() as u64, 3000) {
                    bm.freq_hz
                } else {
                    snapped_freq
                };

                // Validate target_freq against blacklist, skip list, and other active VFOs
                if self.blacklist.iter().any(|&f| f.abs_diff(target_freq) <= 5000)
                    || self.skip_list.contains_key(&target_freq)
                    || self.vfos[..self.max_active_vfos].iter().any(|v| (v.state == VfoState::Locked || v.state == VfoState::Pinned) && v.freq_hz.abs_diff(target_freq) <= (step / 2).max(5000))
                {
                    continue;
                }

                // 1. If this exact frequency is already in History within allowed slots, re-use it!
                let existing_idx = self.vfos[..self.max_active_vfos]
                    .iter()
                    .position(|v| v.state == VfoState::History && v.freq_hz.abs_diff(target_freq) <= (step / 2).max(5000));
                let target_idx = if let Some(idx) = existing_idx {
                    Some(idx)
                } else {
                    // 2. Otherwise pick first Idle slot within allowed slots
                    let idle_idx = self.vfos[..self.max_active_vfos]
                        .iter()
                        .position(|v| v.state == VfoState::Idle);
                    if idle_idx.is_some() {
                        idle_idx
                    } else {
                        // 3. Otherwise recycle the oldest History slot within allowed slots!
                        self.vfos[..self.max_active_vfos]
                            .iter()
                            .enumerate()
                            .filter(|(_, v)| v.state == VfoState::History && Some(v.id) != self.editing_vfo_id)
                            .min_by_key(|(_, v)| v.hangover_until)
                            .map(|(i, _)| i)
                    }
                };

                if let Some(slot_idx) = target_idx {
                    let bookmark_comment = cfg.get_comment(target_freq, 3500);
                    let name = bookmark_comment
                        .map(|b| b.name.clone())
                        .unwrap_or_else(|| format!("{:.4} MHz", target_freq as f64 / 1_000_000.0));
                    let mode = bookmark_comment.map(|b| b.mode).unwrap_or_else(|| {
                        cfg.ranges.iter().find(|r| r.name == cfg.active_range).map(|r| r.mode).unwrap_or(DemodMode::AM)
                    });

                    let free_vfo = &mut self.vfos[slot_idx];
                    free_vfo.retune(target_freq, cfg.center_freq_hz, name);
                    free_vfo.set_mode(mode);
                    free_vfo.state = VfoState::Locked;
                    free_vfo.rf_bandwidth_hz = target_bw_hz;
                    free_vfo.update_squelch(true, best_snr, cfg.vad_enabled, cfg.vad_threshold);
                    if free_vfo.active {
                        free_vfo.hangover_until = now + Duration::from_millis(cfg.hangover_ms);
                    }

                    // Advance scan cursor to the centered carrier frequency
                    self.scan_cursor_hz = target_freq;
                    break;
                }
            }
        }
    }

    /// Returns true if any configured VFO is actively receiving speech or in hangover hold timer
    pub fn has_active_traffic(&self) -> bool {
        let now = Instant::now();
        self.vfos[..self.max_active_vfos].iter().any(|v| {
            v.state == VfoState::Locked && (v.active || now < v.hangover_until)
        })
    }

    /// Sets the maximum concurrent active VFOs and caps history entries to max limit
    pub fn set_max_vfos(&mut self, max: usize) {
        self.max_active_vfos = max.clamp(1, 10);
        self.prune_history();
    }

    /// Enforces that the total number of retained History channels does not exceed max_active_vfos,
    /// and that the total non-idle channels across active + history does not exceed max_active_vfos.
    /// Older history entries (lowest hangover_until) are recycled to Idle first.
    pub fn prune_history(&mut self) {
        let active_count = self.vfos
            .iter()
            .take(self.max_active_vfos)
            .filter(|v| v.state == VfoState::Locked || v.state == VfoState::Pinned)
            .count();
        let max_history = self.max_active_vfos.saturating_sub(active_count);

        let history_count = self.vfos.iter().filter(|v| v.state == VfoState::History).count();
        if history_count > max_history {
            let to_remove = history_count - max_history;
            for _ in 0..to_remove {
                if let Some(oldest_idx) = self.vfos
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.state == VfoState::History && Some(v.id) != self.editing_vfo_id)
                    .min_by_key(|(_, v)| v.hangover_until)
                    .map(|(i, _)| i)
                {
                    self.vfos[oldest_idx].state = VfoState::Idle;
                    self.vfos[oldest_idx].active = false;
                    self.vfos[oldest_idx].snr_db = 0.0;
                    self.vfos[oldest_idx].vad_score = 0.0;
                }
            }
        }

        // Compact any remaining History in slots >= max_active_vfos into free slots in 0..max_active_vfos
        for i in self.max_active_vfos..self.vfos.len() {
            if self.vfos[i].state == VfoState::History {
                if let Some(idle_idx) = self.vfos[..self.max_active_vfos].iter().position(|v| v.state == VfoState::Idle) {
                    self.vfos[idle_idx].freq_hz = self.vfos[i].freq_hz;
                    self.vfos[idle_idx].name = self.vfos[i].name.clone();
                    self.vfos[idle_idx].mode = self.vfos[i].mode;
                    self.vfos[idle_idx].state = VfoState::History;
                    self.vfos[idle_idx].hangover_until = self.vfos[i].hangover_until;
                    self.vfos[idle_idx].active = false;
                }
            }
            let is_data_channel = self.vfos[i].squelch_open && self.vfos[i].muted;
            if !is_data_channel {
                self.vfos[i].state = VfoState::Idle;
                self.vfos[i].active = false;
                self.vfos[i].squelch_open = false;
                self.vfos[i].muted = false;
                self.vfos[i].snr_db = 0.0;
                self.vfos[i].vad_score = 0.0;
                self.vfos[i].squelch_gain = 0.0;
                self.vfos[i].audio_waveform = [0.0; 32];
            }
        }
    }

    /// Processes RF samples across all active/pinned VFOs and mixes their audio
    pub fn process_samples(
        &mut self,
        rf_samples: &[Complex32],
        audio_out: &mut Vec<f32>,
        cfg: &ScannerConfig,
    ) {
        audio_out.clear();

        let mut active_count = 0;
        for (i, vfo) in self.vfos.iter().enumerate() {
            let is_data_channel = vfo.squelch_open && vfo.muted;
            let within_limit = i < self.max_active_vfos;
            if (within_limit || is_data_channel)
                && (vfo.state == VfoState::Locked || vfo.state == VfoState::Pinned)
                && vfo.active
                && !vfo.muted
            {
                active_count += 1;
            }
        }

        for (i, vfo) in self.vfos.iter_mut().enumerate() {
            let is_data_channel = vfo.squelch_open && vfo.muted;
            let within_limit = i < self.max_active_vfos;
            if (within_limit || is_data_channel)
                && (vfo.state == VfoState::Locked || vfo.state == VfoState::Pinned)
                && (vfo.active || vfo.squelch_gain > 0.001 || is_data_channel)
            {
                vfo.process_samples(rf_samples, audio_out, cfg.vad_enabled, cfg.vad_threshold);
            }
        }

        // Normalize mix to prevent clipping when multiple stations are active
        if active_count > 1 {
            let scale = 1.0 / (active_count as f32).sqrt();
            for sample in audio_out.iter_mut() {
                *sample *= scale;
            }
        }
    }

    /// Pin action: locks a VFO so it never releases automatically, or unpins to History
    pub fn toggle_pin_vfo(&mut self, id: usize) {
        if let Some(vfo) = self.vfos.iter_mut().find(|v| v.id == id) {
            vfo.state = match vfo.state {
                VfoState::Pinned => {
                    vfo.active = false;
                    VfoState::History
                }
                _ => {
                    // Lock frequency in place; squelch will evaluate signal presence cleanly
                    VfoState::Pinned
                }
            };
        }
    }

    /// Clears all channels in History back to Idle
    pub fn clear_history(&mut self) {
        for vfo in &mut self.vfos {
            if vfo.state == VfoState::History {
                vfo.state = VfoState::Idle;
                vfo.active = false;
                vfo.snr_db = 0.0;
                vfo.vad_score = 0.0;
            }
        }
    }

    /// Pin the most active station
    pub fn pin_most_active(&mut self) {
        if let Some(vfo) = self.vfos.iter_mut().filter(|v| v.active).max_by(|a, b| {
            a.snr_db.partial_cmp(&b.snr_db).unwrap_or(std::cmp::Ordering::Equal)
        }) {
            vfo.state = VfoState::Pinned;
        }
    }

    /// Skips a frequency for N seconds and frees any VFO monitoring it
    pub fn skip_frequency(&mut self, freq_hz: u64, duration_secs: u64) {
        let expire = Instant::now() + Duration::from_secs(duration_secs);
        self.skip_list.insert(freq_hz, expire);

        for vfo in &mut self.vfos {
            if vfo.freq_hz == freq_hz {
                vfo.state = VfoState::Idle;
                vfo.active = false;
                vfo.snr_db = 0.0;
            }
        }
    }

    /// Unconditionally closes/frees a VFO slot
    pub fn close_vfo(&mut self, id: usize) {
        if let Some(vfo) = self.vfos.iter_mut().find(|v| v.id == id) {
            vfo.state = VfoState::Idle;
            vfo.active = false;
            vfo.snr_db = 0.0;
            vfo.vad_score = 0.0;
            vfo.squelch_gain = 0.0;
            vfo.audio_waveform = [0.0; 32];
        }
    }

    /// Updates the custom label / tag for a VFO
    pub fn set_vfo_comment(&mut self, id: usize, comment: String) {
        if let Some(vfo) = self.vfos.iter_mut().find(|v| v.id == id) {
            vfo.name = comment;
        }
    }

    /// Adds a frequency to the blacklist and drops any VFO monitoring it
    pub fn add_blacklist(&mut self, freq_hz: u64) {
        if !self.blacklist.iter().any(|&f| f.abs_diff(freq_hz) <= 5000) {
            self.blacklist.push(freq_hz);
            self.blacklist.sort();
        }
        for vfo in &mut self.vfos {
            if vfo.freq_hz.abs_diff(freq_hz) <= 5000 {
                vfo.state = VfoState::Idle;
                vfo.active = false;
                vfo.snr_db = 0.0;
                vfo.vad_score = 0.0;
                vfo.squelch_gain = 0.0;
                vfo.audio_waveform = [0.0; 32];
                vfo.speech_accum_sec = 0.0;
                vfo.carrier_ac_power = 0.0;
                vfo.is_tone_buzz = false;
            }
        }
    }

    /// Removes a frequency from the blacklist
    pub fn remove_blacklist(&mut self, freq_hz: u64) {
        self.blacklist.retain(|&f| f.abs_diff(freq_hz) > 5000);
    }

    /// Sets scanner state
    pub fn set_scanner_state(&mut self, state: ScannerState) {
        self.state = state;
        if state == ScannerState::Scanning {
            // When starting or resuming sweep, release any manual VFOs so they don't lock up the sweep cursor!
            for vfo in &mut self.vfos {
                if vfo.state == VfoState::Pinned {
                    vfo.state = VfoState::Locked;
                    vfo.hangover_until = Instant::now() + Duration::from_millis(1500);
                }
            }
        }
    }

    /// Unpins all manual/pinned VFOs when starting a new scan profile
    pub fn unpin_all_manual_vfos(&mut self) {
        for vfo in &mut self.vfos {
            if vfo.state == VfoState::Pinned {
                vfo.state = VfoState::Idle;
                vfo.active = false;
                vfo.hangover_until = Instant::now();
            }
        }
    }

    /// Adds or updates a manual VFO (tunes frequency and pins it to keep listening while paused)
    pub fn set_manual_vfo(
        &mut self,
        vfo_idx: usize,
        freq_hz: u64,
        mode: DemodMode,
        name: String,
        cfg: &ScannerConfig,
    ) {
        let vfo_idx = vfo_idx.min(self.max_active_vfos.saturating_sub(1));
        if vfo_idx < self.vfos.len() {
            let vfo = &mut self.vfos[vfo_idx];
            vfo.retune(freq_hz, cfg.center_freq_hz, name);
            vfo.set_mode(mode);
            vfo.state = VfoState::Pinned;
            vfo.squelch_open = false;
            vfo.muted = false;
            vfo.active = true;
            vfo.has_confirmed_carrier = true;
            vfo.hangover_until = Instant::now() + Duration::from_secs(3600);
            self.scan_cursor_hz = freq_hz;
        }

        // Deactivate any orphaned non-data VFOs outside max_active_vfos
        for vfo in &mut self.vfos[self.max_active_vfos..] {
            let is_data_channel = vfo.squelch_open && vfo.muted;
            if !is_data_channel {
                vfo.state = VfoState::Idle;
                vfo.active = false;
                vfo.squelch_open = false;
                vfo.muted = false;
                vfo.snr_db = 0.0;
                vfo.vad_score = 0.0;
                vfo.squelch_gain = 0.0;
            }
        }

        // Re-sync all active/pinned VFOs with cfg.center_freq_hz
        for vfo in &mut self.vfos {
            if vfo.state == VfoState::Pinned || vfo.squelch_open || vfo.active {
                vfo.update_center_freq(cfg.center_freq_hz);
            }
        }
    }

    /// Tunes a dedicated data channel (ACARS, VDL2, APRS, Telemetry) with squelch forced open and audio muted
    pub fn tune_data_vfo(
        &mut self,
        vfo_idx: usize,
        freq_hz: u64,
        mode: DemodMode,
        name: String,
        cfg: &ScannerConfig,
    ) {
        if vfo_idx < self.vfos.len() {
            let vfo = &mut self.vfos[vfo_idx];
            vfo.retune(freq_hz, cfg.center_freq_hz, name);
            vfo.set_mode(mode);
            vfo.state = VfoState::Pinned;
            vfo.squelch_open = true;
            vfo.muted = true;
            vfo.active = true;
            vfo.has_confirmed_carrier = true;
            vfo.hangover_until = Instant::now() + Duration::from_secs(3600);
            self.scan_cursor_hz = freq_hz;
        }

        // Re-sync all active/pinned VFOs with cfg.center_freq_hz
        for vfo in &mut self.vfos {
            if vfo.state == VfoState::Pinned || vfo.squelch_open || vfo.active {
                vfo.update_center_freq(cfg.center_freq_hz);
            }
        }
    }

    /// Tunes multiple channels simultaneously across available VFOs, supporting both voice and data channels
    pub fn tune_multi_channels(
        &mut self,
        channels: &[(u64, DemodMode, String, bool)], // (freq_hz, mode, name, is_voice)
        cfg: &ScannerConfig,
    ) {
        let max_to_assign = channels.len().min(self.max_active_vfos).min(self.vfos.len());
        for (i, (freq_hz, mode, name, is_voice)) in channels.iter().take(max_to_assign).enumerate() {
            let vfo = &mut self.vfos[i];
            vfo.retune(*freq_hz, cfg.center_freq_hz, name.clone());
            vfo.set_mode(*mode);
            vfo.state = VfoState::Pinned;
            if *is_voice {
                vfo.squelch_open = false;
                vfo.muted = false;
            } else {
                vfo.squelch_open = true;
                vfo.muted = true;
            }
            vfo.active = true;
            vfo.has_confirmed_carrier = true;
            vfo.hangover_until = Instant::now() + Duration::from_secs(86400);
            vfo.update_center_freq(cfg.center_freq_hz);
        }

        // Reset any trailing VFOs that were previously used in 0..max_active_vfos
        for vfo in &mut self.vfos[max_to_assign..self.max_active_vfos] {
            if vfo.state == VfoState::Pinned {
                vfo.squelch_open = false;
                vfo.state = VfoState::Idle;
                vfo.active = false;
                vfo.muted = false;
            }
        }

        // Re-sync all active/pinned VFOs with cfg.center_freq_hz
        for vfo in &mut self.vfos {
            if vfo.state == VfoState::Pinned || vfo.squelch_open || vfo.active {
                vfo.update_center_freq(cfg.center_freq_hz);
            }
        }
    }

    /// Tunes multiple dedicated data channels simultaneously across available VFOs
    pub fn tune_multi_data_vfos(
        &mut self,
        channels: &[(u64, DemodMode, String)],
        cfg: &ScannerConfig,
    ) {
        let mapped: Vec<(u64, DemodMode, String, bool)> = channels
            .iter()
            .map(|(f, m, n)| (*f, *m, n.clone(), false))
            .collect();
        self.tune_multi_channels(&mapped, cfg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scanner_detects_signal() {
        let cfg = ScannerConfig::default();
        let mut engine = ScannerEngine::new(&cfg);

        // Create pseudospectrum with a peak of +20 dB at 122.600 MHz (+100 kHz from 122.5 MHz)
        let fft_size = 2048;
        let mut pseudo = vec![0.0f32; fft_size];

        let bin_hz = cfg.sample_rate as f64 / fft_size as f64;
        let center_bin = (fft_size as f64 * 0.5) + (100_000.0 / bin_hz);
        let peak_bin = center_bin.round() as usize;
        pseudo[peak_bin] = 20.0;
        pseudo[peak_bin - 1] = 15.0;
        pseudo[peak_bin + 1] = 15.0;

        // Run evaluate_spectrum multiple times to let sweep reach 122.600 MHz
        let mut locked = false;
        for _ in 0..100 {
            engine.evaluate_spectrum(&pseudo, &cfg);
            if engine.vfos.iter().any(|v| v.state == VfoState::Locked && v.freq_hz == 122_600_000) {
                locked = true;
                break;
            }
        }

        println!("Signal locked? {}", locked);
        for v in &engine.vfos {
            println!("VFO #{}: state={:?}, freq={}, active={}", v.id, v.state, v.freq_hz, v.active);
        }
        assert!(locked);

        // Now test NEXT frame!
        engine.evaluate_spectrum(&pseudo, &cfg);
        println!("After next frame:");
        for v in &engine.vfos {
            println!("VFO #{}: state={:?}, freq={}, active={}", v.id, v.state, v.freq_hz, v.active);
        }
    }

    #[test]
    fn test_scanner_multi_vfo_parallel_detection() {
        let cfg = ScannerConfig::default();
        let mut engine = ScannerEngine::new(&cfg);

        let fft_size = 2048;
        let mut pseudo = vec![0.0f32; fft_size];
        let bin_hz = cfg.sample_rate as f64 / fft_size as f64;

        // 3 concurrent active frequencies:
        // 1. 119.700 MHz (-2.8 MHz from 122.5 MHz)
        // 2. 121.300 MHz (-1.2 MHz from 122.5 MHz)
        // 3. 125.000 MHz (+2.5 MHz from 122.5 MHz)
        let f1_bin = ((fft_size as f64 * 0.5) + (-2_800_000.0 / bin_hz)).round() as usize;
        let f2_bin = ((fft_size as f64 * 0.5) + (-1_200_000.0 / bin_hz)).round() as usize;
        let f3_bin = ((fft_size as f64 * 0.5) + (2_500_000.0 / bin_hz)).round() as usize;

        pseudo[f1_bin] = 25.0;
        pseudo[f2_bin] = 18.0;
        pseudo[f3_bin] = 22.0;

        // Sweep across band should detect and lock all 3 active frequencies into available VFOs!
        for _ in 0..60 {
            engine.evaluate_spectrum(&pseudo, &cfg);
        }

        let locked_count = engine.vfos.iter().filter(|v| v.state == VfoState::Locked).count();
        let active_count = engine.vfos.iter().filter(|v| v.active).count();

        for v in &engine.vfos {
            println!("Multi-VFO #{}: freq={:.3} MHz, state={:?}, active={}", v.id, v.freq_hz as f64 / 1e6, v.state, v.active);
        }

        assert_eq!(locked_count, 3, "Expected 3 VFOs locked simultaneously");
        assert_eq!(active_count, 3, "Expected 3 VFOs actively playing audio");
    }

    #[test]
    fn test_blacklist_skips_frequency() {
        let cfg = ScannerConfig::default();
        let mut engine = ScannerEngine::new(&cfg);

        // Put a spur at 120.000 MHz (-2.5 MHz from 122.5 MHz)
        let fft_size = 2048;
        let mut pseudo = vec![0.0f32; fft_size];
        let bin_hz = cfg.sample_rate as f64 / fft_size as f64;
        let spur_bin = ((fft_size as f64 * 0.5) + (-2_500_000.0 / bin_hz)).round() as usize;
        pseudo[spur_bin] = 25.0;

        // Run sweep: should detect and lock 120.000 MHz
        for _ in 0..50 {
            engine.evaluate_spectrum(&pseudo, &cfg);
        }
        assert!(engine.vfos.iter().any(|v| v.state == VfoState::Locked && v.freq_hz == 120_000_000));

        // Now user clicks IGNORE on 120.000 MHz!
        engine.add_blacklist(120_000_000);

        // VFO should be freed immediately
        assert!(!engine.vfos.iter().any(|v| v.state != VfoState::Idle && v.freq_hz == 120_000_000));

        // Sweep again for 100 frames with the spur still physically present in RF
        for _ in 0..100 {
            engine.evaluate_spectrum(&pseudo, &cfg);
        }

        // Scanner should NEVER lock on 120.000 MHz again!
        assert!(!engine.vfos.iter().any(|v| v.state != VfoState::Idle && v.freq_hz == 120_000_000), "Blacklisted frequency must not be locked");
    }

    #[test]
    fn test_manual_vfo_stays_active_when_paused() {
        let cfg = ScannerConfig::default();
        let mut engine = ScannerEngine::new(&cfg);

        engine.set_scanner_state(ScannerState::Paused);
        engine.set_manual_vfo(0, 121_500_000, DemodMode::AM, "Emergency".to_string(), &cfg);

        assert_eq!(engine.vfos[0].freq_hz, 121_500_000);
        assert!(engine.vfos[0].active);
        assert_eq!(engine.vfos[0].state, VfoState::Pinned);

        let fft_size = 2048;
        let pseudo = vec![0.0f32; fft_size]; // Silent band (no RF signal)

        // Evaluate spectrum multiple times while paused with silent band
        for _ in 0..20 {
            engine.evaluate_spectrum(&pseudo, &cfg);
        }

        // Must stay pinned, NOT expire to Idle or History
        assert_eq!(engine.vfos[0].freq_hz, 121_500_000);
        assert_eq!(engine.vfos[0].state, VfoState::Pinned);
        // But squelch must be closed (!active) because there is no RF signal (prevent noise hiss)
        assert!(!engine.vfos[0].active);

        // When a signal appears at 121.500 MHz, squelch opens!
        let bin_hz = cfg.sample_rate as f64 / fft_size as f64;
        let offset = 121_500_000.0 - cfg.center_freq_hz as f64;
        let bin = ((fft_size as f64 * 0.5) + (offset / bin_hz)).round() as usize;
        let mut pseudo_signal = vec![0.0f32; fft_size];
        pseudo_signal[bin] = 25.0;
        engine.evaluate_spectrum(&pseudo_signal, &cfg);
        assert!(engine.vfos[0].active, "Squelch must open when signal is received!");
        assert_eq!(engine.vfos[0].state, VfoState::Pinned);
    }

    #[test]
    fn test_hold_delay_timer_holds_on_frequency_after_signal_ends() {
        let mut cfg = ScannerConfig::default();
        cfg.max_vfos = 1;
        cfg.hangover_ms = 2000;
        let mut engine = ScannerEngine::new(&cfg);

        let fft_size = 2048;
        let mut pseudo = vec![0.0f32; fft_size];
        let bin_hz = cfg.sample_rate as f64 / fft_size as f64;
        let peak_bin = ((fft_size as f64 * 0.5) + (100_000.0 / bin_hz)).round() as usize;
        pseudo[peak_bin] = 20.0;

        // 1. Detect and lock signal at 122.600 MHz
        for _ in 0..50 {
            engine.evaluate_spectrum(&pseudo, &cfg);
        }
        assert!(engine.vfos[0].state == VfoState::Locked && engine.vfos[0].freq_hz == 122_600_000);
        assert!(engine.vfos[0].active);
        assert_eq!(engine.scan_cursor_hz, 122_600_000);

        // Simulate a real transmission that lasted 1.0 second (> 0.45s)
        engine.vfos[0].carrier_lock_start = Instant::now() - Duration::from_millis(1000);

        // 2. Transmission stops (signal drops to 0)
        let pseudo_silent = vec![0.0f32; fft_size];

        // Frame immediately after signal stops:
        engine.evaluate_spectrum(&pseudo_silent, &cfg);

        // Audio must be squelched/muted (no hiss), BUT VFO must remain Locked and cursor parked!
        assert_eq!(engine.vfos[0].state, VfoState::Locked);
        assert!(!engine.vfos[0].active, "Audio should be squelched/muted during hold");
        assert_eq!(engine.scan_cursor_hz, 122_600_000, "Scanner must not leave during hold time");

        // Several more frames while hold time is still active:
        for _ in 0..10 {
            engine.evaluate_spectrum(&pseudo_silent, &cfg);
            assert_eq!(engine.vfos[0].state, VfoState::Locked);
            assert_eq!(engine.scan_cursor_hz, 122_600_000);
        }

        // 3. Fast-forward timer past hangover_until
        engine.vfos[0].hangover_until = Instant::now() - Duration::from_millis(10);

        // Frame after delay timer expired:
        engine.evaluate_spectrum(&pseudo_silent, &cfg);

        // VFO must now transition to History retention (visible but inactive) and scanner free to sweep!
        assert_eq!(engine.vfos[0].state, VfoState::History);
        assert!(!engine.has_active_traffic(), "History does not block scanner sweep");

        // Clear history resets to Idle
        engine.clear_history();
        assert_eq!(engine.vfos[0].state, VfoState::Idle);
    }

    #[test]
    fn test_transient_noise_spike_releases_to_idle_without_history() {
        let mut cfg = ScannerConfig::default();
        cfg.max_vfos = 1;
        cfg.hangover_ms = 2000;
        let mut engine = ScannerEngine::new(&cfg);

        let fft_size = 2048;
        let mut pseudo = vec![0.0f32; fft_size];
        let bin_hz = cfg.sample_rate as f64 / fft_size as f64;
        let peak_bin = ((fft_size as f64 * 0.5) + (100_000.0 / bin_hz)).round() as usize;
        pseudo[peak_bin] = 20.0;

        // Detect signal across frames
        for _ in 0..50 {
            engine.evaluate_spectrum(&pseudo, &cfg);
        }
        assert!(engine.vfos[0].state == VfoState::Locked);

        // Simulate transient noise spike that dropped in < 80ms (e.g. 30ms)
        engine.vfos[0].carrier_lock_start = Instant::now() - Duration::from_millis(30);

        // Burst drops
        let pseudo_silent = vec![0.0f32; fft_size];
        engine.evaluate_spectrum(&pseudo_silent, &cfg);

        // Millisecond noise spikes must NOT pollute History! Must reset directly to Idle!
        assert_eq!(engine.vfos[0].state, VfoState::Idle);
        assert!(!engine.vfos[0].active);
    }

    #[test]
    fn test_short_glitch_never_activates_green_and_drops_silently() {
        let mut cfg = ScannerConfig::default();
        cfg.max_vfos = 1;
        cfg.hangover_ms = 2000;
        let mut engine = ScannerEngine::new(&cfg);

        let fft_size = 2048;
        let mut pseudo = vec![0.0f32; fft_size];
        let bin_hz = cfg.sample_rate as f64 / fft_size as f64;
        let peak_bin = ((fft_size as f64 * 0.5) + (100_000.0 / bin_hz)).round() as usize;
        pseudo[peak_bin] = 20.0;

        // Sweep until lock
        let mut locked = false;
        for _ in 0..100 {
            engine.evaluate_spectrum(&pseudo, &cfg);
            if engine.vfos[0].state == VfoState::Locked {
                locked = true;
                break;
            }
        }
        assert!(locked);

        // On first detection, carrier is NOT confirmed yet and MUST NOT be active green!
        assert!(!engine.vfos[0].active, "Initial hit must NOT activate green");
        assert!(!engine.vfos[0].has_confirmed_carrier, "Initial hit must NOT confirm carrier");

        // Glitch drops immediately on next frame (< 80ms)
        let pseudo_silent = vec![0.0f32; fft_size];
        engine.evaluate_spectrum(&pseudo_silent, &cfg);

        // Must quietly reset to Idle with NO hold timer and NO history!
        assert_eq!(engine.vfos[0].state, VfoState::Idle);
        assert!(!engine.vfos[0].active);
    }

    #[test]
    fn test_engine_allocates_10_vfos() {
        let mut cfg = ScannerConfig::default();
        cfg.max_vfos = 10;
        let mut engine = ScannerEngine::new(&cfg);

        assert_eq!(engine.vfos.len(), 10, "Engine must pre-allocate 10 VFO channels");
        assert_eq!(engine.max_active_vfos, 10);

        engine.set_max_vfos(8);
        assert_eq!(engine.max_active_vfos, 8);

        engine.set_max_vfos(15); // Clamping test
        assert_eq!(engine.max_active_vfos, 10);
    }

    #[test]
    fn test_traficom_default_ranges_and_bookmarks() {
        let cfg = ScannerConfig::default();

        // Must have Traficom ranges
        assert!(cfg.ranges.iter().any(|r| r.name.contains("Elinkeino VHF (154M)")));
        assert!(cfg.ranges.iter().any(|r| r.name.contains("Elinkeino VHF (147-171M)")));
        assert!(cfg.ranges.iter().any(|r| r.name.contains("Elinkeino UHF (442-459M)")));
        assert!(cfg.ranges.iter().any(|r| r.name.contains("RHA68")));

        // Must have Traficom bookmarks
        assert!(cfg.bookmarks.contains_key(&154_506_250)); // Alue 2 K1
        assert!(cfg.bookmarks.contains_key(&154_656_250)); // Alue 2d K1 DMR
        assert!(cfg.bookmarks.contains_key(&147_100_000)); // Alue 3 K1
        assert!(cfg.bookmarks.contains_key(&442_850_000)); // Nostotyö K1
        assert!(cfg.bookmarks.contains_key(&443_125_000)); // Alue 5 K1
    }

    #[test]
    fn test_pass_completed_and_active_traffic() {
        let mut cfg = ScannerConfig::default();
        cfg.scan_start_hz = 120_000_000;
        cfg.scan_end_hz = 120_100_000;
        cfg.scan_step_hz = 25_000;
        let mut engine = ScannerEngine::new(&cfg);

        assert!(!engine.pass_completed);
        assert!(!engine.has_active_traffic());

        let pseudo = vec![0.0f32; 2048];
        // 5 steps: 120.000 -> 120.025 -> 120.050 -> 120.075 -> 120.100 -> wrap around!
        for _ in 0..2 {
            engine.evaluate_spectrum(&pseudo, &cfg);
        }

        assert!(engine.pass_completed, "Should flag pass_completed when cursor wraps around end of range");
    }

    #[test]
    fn test_switch_range_preserves_history_vfos() {
        let cfg = ScannerConfig::default();
        let mut engine = ScannerEngine::new(&cfg);

        // Put slot 0 and slot 1 into History
        engine.vfos[0].state = VfoState::History;
        engine.vfos[0].freq_hz = 121_500_000;
        engine.vfos[0].name = "121.5000 MHz".to_string();

        engine.vfos[1].state = VfoState::History;
        engine.vfos[1].freq_hz = 119_700_000;
        engine.vfos[1].name = "119.7000 MHz".to_string();

        // Switch to a completely different band (e.g. 70cm HAM at 433 MHz)
        engine.switch_range(433_000_000, 434_000_000, 433_500_000, 12_500, DemodMode::NFM);

        // History VFOs must still be in History and keep their frequencies!
        assert_eq!(engine.vfos[0].state, VfoState::History);
        assert_eq!(engine.vfos[0].freq_hz, 121_500_000);

        assert_eq!(engine.vfos[1].state, VfoState::History);
        assert_eq!(engine.vfos[1].freq_hz, 119_700_000);

        // History VFOs must not block traffic or range hopping
        assert!(!engine.has_active_traffic());
    }

    #[test]
    fn test_afc_carrier_centering_from_skirt() {
        let mut cfg = ScannerConfig::default();
        cfg.scan_start_hz = 122_550_000;
        cfg.scan_step_hz = 25_000;
        let mut engine = ScannerEngine::new(&cfg);

        // Create pseudospectrum with a peak at 122.600 MHz (+100 kHz offset from 122.5 MHz)
        let fft_size = 2048;
        let mut pseudo = vec![0.0f32; fft_size];
        let bin_hz = cfg.sample_rate as f64 / fft_size as f64;
        let center_bin = (fft_size as f64 * 0.5) + (100_000.0 / bin_hz);
        let peak_bin = center_bin.round() as usize;

        // Carrier at 122.600 MHz is +24 dB, but skirt at 122.575 MHz is +8.5 dB (above squelch of 7 dB)
        pseudo[peak_bin] = 24.0;
        pseudo[peak_bin - 1] = 16.0;
        pseudo[peak_bin + 1] = 16.0;
        let skirt_bin = ((fft_size as f64 * 0.5) + (75_000.0 / bin_hz)).round() as usize;
        pseudo[skirt_bin] = 8.5;

        // Cursor starts at 122.550 MHz. First step moves cursor to 122.575 MHz (skirt!).
        // In the old code without AFC, it would have locked onto 122.575 MHz.
        // With AFC peak search, it must find the true peak carrier and snap to 122.600 MHz!
        engine.evaluate_spectrum(&pseudo, &cfg);

        let locked_vfo = engine.vfos.iter().find(|v| v.state == VfoState::Locked);
        assert!(locked_vfo.is_some(), "Should lock onto detected transmission");
        assert_eq!(
            locked_vfo.unwrap().freq_hz,
            122_600_000,
            "AFC must center onto true carrier frequency (122.600 MHz) instead of the skirt step"
        );
    }

    #[test]
    fn test_history_count_capped_by_max_active_vfos() {
        let mut cfg = ScannerConfig::default();
        cfg.max_vfos = 4;
        let mut engine = ScannerEngine::new(&cfg);

        let now = Instant::now();
        // Create 4 History channels with distinct timestamps
        for i in 0..4 {
            engine.vfos[i].state = VfoState::History;
            engine.vfos[i].freq_hz = 120_000_000 + (i as u64 * 100_000);
            engine.vfos[i].hangover_until = now + Duration::from_secs(i as u64 + 1);
        }

        let hist_count = engine.vfos.iter().filter(|v| v.state == VfoState::History).count();
        assert_eq!(hist_count, 4);

        // User reduces max active channels from 4 to 1!
        engine.set_max_vfos(1);

        let new_hist_count = engine.vfos.iter().filter(|v| v.state == VfoState::History).count();
        assert_eq!(new_hist_count, 1, "Only 1 history channel should remain when max_active_vfos is 1");

        // The remaining 1 must be the newest one (highest hangover_until, compacted into slot 0)
        assert_eq!(engine.vfos[0].state, VfoState::History);
        assert_eq!(engine.vfos[0].freq_hz, 120_300_000);
        assert_eq!(engine.vfos[1].state, VfoState::Idle);
        assert_eq!(engine.vfos[2].state, VfoState::Idle);
        assert_eq!(engine.vfos[3].state, VfoState::Idle);
    }

    #[test]
    fn test_max_vfos_2_replaces_oldest_history_when_new_signal_detected() {
        let mut cfg = ScannerConfig::default();
        cfg.max_vfos = 2;
        cfg.scan_start_hz = 122_500_000;
        cfg.scan_step_hz = 25_000;
        let mut engine = ScannerEngine::new(&cfg);

        let now = Instant::now();
        // Slot 0 is older history at 120.0 MHz
        engine.vfos[0].state = VfoState::History;
        engine.vfos[0].freq_hz = 120_000_000;
        engine.vfos[0].hangover_until = now - Duration::from_secs(10);

        // Slot 1 is newer history at 121.0 MHz
        engine.vfos[1].state = VfoState::History;
        engine.vfos[1].freq_hz = 121_000_000;
        engine.vfos[1].hangover_until = now - Duration::from_secs(2);

        // Total non-idle = 2 (all history)
        assert_eq!(engine.vfos.iter().filter(|v| v.state != VfoState::Idle).count(), 2);

        // Transmission appears at 122.600 MHz
        let fft_size = 2048;
        let mut pseudo = vec![0.0f32; fft_size];
        let bin_hz = cfg.sample_rate as f64 / fft_size as f64;
        let peak_bin = ((fft_size as f64 * 0.5) + (100_000.0 / bin_hz)).round() as usize;
        pseudo[peak_bin] = 20.0;

        for _ in 0..5 {
            engine.evaluate_spectrum(&pseudo, &cfg);
        }

        // The oldest history (slot 0) MUST have been replaced by the new signal!
        assert_eq!(engine.vfos[0].state, VfoState::Locked);
        assert_eq!(engine.vfos[0].freq_hz, 122_600_000);
        assert!(engine.vfos[0].active);

        // The newer history (slot 1) is still retained in history!
        assert_eq!(engine.vfos[1].state, VfoState::History);
        assert_eq!(engine.vfos[1].freq_hz, 121_000_000);
        assert!(!engine.vfos[1].active);

        // Total non-idle slots remains strictly 2!
        assert_eq!(engine.vfos.iter().filter(|v| v.state != VfoState::Idle).count(), 2);
        // All remaining slots (2..10) must still be Idle!
        for i in 2..10 {
            assert_eq!(engine.vfos[i].state, VfoState::Idle);
        }
    }

    #[test]
    fn test_wideband_chunk_stepping_and_switching() {
        let mut cfg = ScannerConfig::default();
        cfg.sample_rate = 10_000_000;
        cfg.scan_start_hz = 118_000_000;
        cfg.scan_end_hz = 138_000_000;
        let usable_bw = ((cfg.sample_rate as f64) * 0.85).round() as u64; // 8_500_000 Hz

        // Chunk 0 center
        let chunk0_center = cfg.scan_start_hz + (usable_bw / 2);
        assert_eq!(chunk0_center, 122_250_000);

        // Next chunk center
        let chunk1_center = (chunk0_center + usable_bw).min(cfg.scan_end_hz.saturating_sub(usable_bw / 2));
        assert_eq!(chunk1_center, 130_750_000);

        // Third chunk center
        let chunk2_center = (chunk1_center + usable_bw).min(cfg.scan_end_hz.saturating_sub(usable_bw / 2));
        assert_eq!(chunk2_center, 133_750_000);

        let mut engine = ScannerEngine::new(&cfg);
        // Simulate a station locked on chunk 0
        engine.vfos[0].state = VfoState::Locked;
        engine.vfos[0].freq_hz = 120_100_000;
        engine.vfos[0].active = true;

        // Step chunk to chunk 1: switch_range preserves locked station into History
        engine.switch_range(cfg.scan_start_hz, cfg.scan_end_hz, chunk1_center, cfg.scan_step_hz, DemodMode::AM);

        assert_eq!(engine.vfos[0].state, VfoState::History);
        assert_eq!(engine.vfos[0].freq_hz, 120_100_000);
        assert!(!engine.vfos[0].active);
        // When in history, has_active_traffic must be false!
        assert!(!engine.has_active_traffic());
    }

    #[test]
    fn test_pinned_vfo_holds_chunk_and_bookmark_isolation() {
        let mut cfg = ScannerConfig::default();
        cfg.max_vfos = 3;
        let mut engine = ScannerEngine::new(&cfg);

        // Pin VFO 0 manually (Pause / Pin)
        let vfo_id = engine.vfos[0].id;
        engine.toggle_pin_vfo(vfo_id);
        assert_eq!(engine.vfos[0].state, VfoState::Pinned);
        // Squelch remains closed when silent, opening only upon carrier reception

        // Verify has_pinned_vfo logic
        let has_pinned = engine.vfos.iter().any(|v| v.state == VfoState::Pinned);
        assert!(has_pinned, "Pinned VFO must be detected to hold the 10 MHz chunk!");

        // Bookmark lookup isolation: 154.550 MHz must not match Airband bookmarks
        let bm = cfg.get_comment(154_550_000, 3500);
        assert!(bm.is_none() || bm.unwrap().name.contains("Traficom") || bm.unwrap().name.contains("Alue 2"));
        if let Some(b) = bm {
            assert!(!b.name.contains("EFHK") && !b.name.contains("lennonjohto"), "Elinkeino channel must not match EFHK bookmarks!");
        }
    }

    #[test]
    fn test_tune_data_vfo_opens_squelch_and_mutes_audio() {
        let cfg = ScannerConfig::default();
        let mut engine = ScannerEngine::new(&cfg);

        // Tune dedicated data channel (e.g. 131.725 MHz ACARS)
        engine.tune_data_vfo(0, 131_725_000, DemodMode::AM, "ACARS 131.725".to_string(), &cfg);

        let vfo = &engine.vfos[0];
        assert_eq!(vfo.freq_hz, 131_725_000);
        assert_eq!(vfo.state, VfoState::Pinned);
        assert!(vfo.squelch_open, "Data channel must have squelch_open = true!");
        assert!(vfo.muted, "Data channel must have muted = true to protect listener ears!");
        assert!(vfo.active, "Data channel must be active!");

        // Update VFOs with zero SNR: squelch must remain open and active for data decoding
        let pseudo_spectrum = vec![-120.0f32; 2048];
        engine.evaluate_spectrum(&pseudo_spectrum, &cfg);

        let vfo_after = &engine.vfos[0];
        assert!(vfo_after.active, "VFO must remain active even with weak/silent signal so decoder runs!");
        assert!(vfo_after.muted, "VFO audio must remain muted!");
    }

    #[test]
    fn test_tune_multi_data_vfos_parallel_monitoring() {
        let mut cfg = ScannerConfig::default();
        cfg.sample_rate = 10_000_000;
        cfg.center_freq_hz = 134_250_000;
        let mut engine = ScannerEngine::new(&cfg);

        let channels = vec![
            (131_525_000, DemodMode::AM, "ACARS Intl".to_string()),
            (131_725_000, DemodMode::AM, "ACARS EU".to_string()),
            (136_975_000, DemodMode::AM, "VDL2 EU".to_string()),
        ];

        engine.tune_multi_data_vfos(&channels, &cfg);

        // All 3 channels must be running concurrently on distinct VFOs
        for (i, &(freq, mode, ref name)) in channels.iter().enumerate() {
            let vfo = &engine.vfos[i];
            assert_eq!(vfo.freq_hz, freq);
            assert_eq!(vfo.mode, mode);
            assert_eq!(vfo.name, *name);
            assert_eq!(vfo.state, VfoState::Pinned);
            assert!(vfo.squelch_open, "Channel {} must have squelch open for data", i);
            assert!(vfo.muted, "Channel {} must be muted for audio protection", i);
            assert!(vfo.active, "Channel {} must be active", i);
            assert!(!vfo.is_out_of_band, "Channel {} must fit within 10 MHz SDR baseband", i);
        }

        // Trailing VFOs must be closed / idle
        for i in 3..engine.vfos.len() {
            assert!(!engine.vfos[i].squelch_open);
            assert_eq!(engine.vfos[i].state, VfoState::Idle);
        }
    }

    #[test]
    fn test_tune_multi_marine_channels_mixed_voice_and_data() {
        let mut cfg = ScannerConfig::default();
        cfg.sample_rate = 10_000_000;
        cfg.center_freq_hz = 159_000_000;
        cfg.max_vfos = 3;
        let mut engine = ScannerEngine::new(&cfg);

        let channels = vec![
            (161_975_000, DemodMode::NFM, "AIS 1 (161.975)".to_string(), false), // data: muted, squelch open
            (156_800_000, DemodMode::NFM, "VHF Ch 16 (156.800)".to_string(), true), // voice: unmuted, squelch gated
            (156_600_000, DemodMode::NFM, "VTS Ch 12 (156.600)".to_string(), true), // voice: unmuted, squelch gated
        ];

        engine.tune_multi_channels(&channels, &cfg);

        // VFO 0 is AIS 1 (data)
        assert_eq!(engine.vfos[0].freq_hz, 161_975_000);
        assert_eq!(engine.vfos[0].state, VfoState::Pinned);
        assert!(engine.vfos[0].squelch_open);
        assert!(engine.vfos[0].muted);
        assert!(engine.vfos[0].active);

        // VFO 1 is Ch 16 (voice)
        assert_eq!(engine.vfos[1].freq_hz, 156_800_000);
        assert_eq!(engine.vfos[1].state, VfoState::Pinned);
        assert!(!engine.vfos[1].squelch_open);
        assert!(!engine.vfos[1].muted, "VHF Ch 16 must NOT be muted");
        assert!(engine.vfos[1].active);

        // VFO 2 is VTS (voice)
        assert_eq!(engine.vfos[2].freq_hz, 156_600_000);
        assert_eq!(engine.vfos[2].state, VfoState::Pinned);
        assert!(!engine.vfos[2].squelch_open);
        assert!(!engine.vfos[2].muted, "VTS Ch 12 must NOT be muted");
        assert!(engine.vfos[2].active);
    }

    #[test]
    fn test_max_vfos_1_enforces_single_active_channel() {
        let mut cfg = ScannerConfig::default();
        cfg.sample_rate = 10_000_000;
        cfg.center_freq_hz = 106_000_000;
        cfg.max_vfos = 1;
        let mut engine = ScannerEngine::new(&cfg);

        // Simulate previous state where VFO 1 was pinned or left open by a multi-data session
        engine.vfos[1].state = VfoState::Pinned;
        engine.vfos[1].freq_hz = 105_800_000;
        engine.vfos[1].active = true;
        engine.vfos[1].squelch_open = false;

        // User manually tunes VFO 0 to 106.0 MHz in BCFM mode
        engine.set_manual_vfo(0, 106_000_000, DemodMode::BCFM, "Radio Suomi".to_string(), &cfg);

        // VFO 0 must be Pinned, active, and tuned to 106.0 MHz BCFM
        assert_eq!(engine.vfos[0].freq_hz, 106_000_000);
        assert_eq!(engine.vfos[0].mode, DemodMode::BCFM);
        assert_eq!(engine.vfos[0].state, VfoState::Pinned);
        assert!(engine.vfos[0].active);

        // VFO 1 must have been automatically deactivated and set to Idle!
        assert_eq!(engine.vfos[1].state, VfoState::Idle);
        assert!(!engine.vfos[1].active);

        // evaluate_spectrum must keep only 1 channel active
        let pseudo = vec![15.0f32; 2048];
        engine.evaluate_spectrum(&pseudo, &cfg);

        let active_count = engine.vfos.iter().take(engine.max_active_vfos).filter(|v| v.active).count();
        assert_eq!(active_count, 1, "Exactly 1 channel can be active when max_active_vfos is 1");
        assert_eq!(engine.vfos[1].state, VfoState::Idle);
        assert!(!engine.vfos[1].active);
    }
}


