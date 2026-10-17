use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DemodMode {
    AM,
    NFM,
    WFM,
    BCFM,
    USB,
    LSB,
}

impl DemodMode {
    pub fn name(&self) -> &'static str {
        match self {
            DemodMode::AM => "AM",
            DemodMode::NFM => "NFM",
            DemodMode::WFM => "WFM",
            DemodMode::BCFM => "BCFM",
            DemodMode::USB => "USB",
            DemodMode::LSB => "LSB",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub freq_hz: u64,
    pub name: String,
    pub mode: DemodMode,
    pub bandwidth_hz: u32,
}

fn default_range_enabled_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRangeProfile {
    pub name: String,
    pub center_freq_hz: u64,
    pub scan_start_hz: u64,
    pub scan_end_hz: u64,
    pub scan_step_hz: u32,
    pub mode: DemodMode,
    #[serde(default = "default_range_enabled_true")]
    pub enabled: bool,
}

fn default_audio_device() -> String {
    "Default (Windows System Output)".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerConfig {
    pub selected_device: String,
    pub center_freq_hz: u64,
    pub sample_rate: u32,
    pub rf_gain: u32,
    pub scan_start_hz: u64,
    pub scan_end_hz: u64,
    pub scan_step_hz: u32,
    pub squelch_snr_db: f32,
    pub vad_enabled: bool,
    pub vad_threshold: f32,
    pub hangover_ms: u64,
    pub max_vfos: usize,
    pub active_range: String,
    pub ranges: Vec<ScanRangeProfile>,
    pub bookmarks: HashMap<u64, Bookmark>,
    #[serde(default)]
    pub blacklist: Vec<u64>,
    #[serde(default = "default_audio_device")]
    pub audio_device: String,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        let mut bookmarks = HashMap::new();
        let default_list = vec![
            (118_100_000, "EFHK Torni", DemodMode::AM, 25_000),
            (118_600_000, "EFHK Saapuvat", DemodMode::AM, 25_000),
            (119_700_000, "EFHK Lähialue", DemodMode::AM, 25_000),
            (121_300_000, "EFHK Aluelennonjohto", DemodMode::AM, 25_000),
            (121_500_000, "HÄTÄ / Emergency", DemodMode::AM, 25_000),
            (129_150_000, "EFHK ATIS", DemodMode::AM, 25_000),
            (131_250_000, "Lentoyhtiöt Ops", DemodMode::AM, 25_000),
            (1_090_000_000, "ADS-B 1090 MHz", DemodMode::AM, 2_000_000),
            (145_500_000, "2m Kutsu OH", DemodMode::NFM, 12_500),
            (145_700_000, "2m Toistin RAC", DemodMode::NFM, 12_500),
            (156_800_000, "Meri VHF K16 Hätä/Kutsu", DemodMode::NFM, 25_000),
            (156_525_000, "Meri VHF K70 DSC", DemodMode::NFM, 25_000),
            (433_500_000, "70cm Kutsukanava", DemodMode::NFM, 12_500),
            (446_006_250, "PMR446 K1", DemodMode::NFM, 12_500),
            (446_018_750, "PMR446 K2", DemodMode::NFM, 12_500),
            (446_031_250, "PMR446 K3", DemodMode::NFM, 12_500),
            (446_093_750, "PMR446 K8", DemodMode::NFM, 12_500),
            // Traficom Elinkeinoelämän yhteiskäyttökanavat Alue 2 (150 MHz)
            (154_506_250, "Traficom Alue 2 K1", DemodMode::NFM, 12_500),
            (154_518_750, "Traficom Alue 2 K2", DemodMode::NFM, 12_500),
            (154_531_250, "Traficom Alue 2 K3", DemodMode::NFM, 12_500),
            (154_543_750, "Traficom Alue 2 K4", DemodMode::NFM, 12_500),
            (154_556_250, "Traficom Alue 2 K5", DemodMode::NFM, 12_500),
            (154_568_750, "Traficom Alue 2 K6", DemodMode::NFM, 12_500),
            (154_581_250, "Traficom Alue 2 K7", DemodMode::NFM, 12_500),
            (154_593_750, "Traficom Alue 2 K8", DemodMode::NFM, 12_500),
            (154_606_250, "Traficom Alue 2 K9", DemodMode::NFM, 12_500),
            (154_618_750, "Traficom Alue 2 K10", DemodMode::NFM, 12_500),
            (154_631_250, "Traficom Alue 2 K11", DemodMode::NFM, 12_500),
            (154_643_750, "Traficom Alue 2 K12", DemodMode::NFM, 12_500),
            // Traficom Digitaaliset (Alue 2d)
            (154_656_250, "Traficom Alue 2d K1 DMR", DemodMode::NFM, 12_500),
            (154_681_250, "Traficom Alue 2d K2 DMR", DemodMode::NFM, 12_500),
            (154_718_750, "Traficom Alue 2d K3 DMR", DemodMode::NFM, 12_500),
            (154_768_750, "Traficom Alue 2d K4 DMR", DemodMode::NFM, 12_500),
            (154_793_750, "Traficom Alue 2d K5 DMR", DemodMode::NFM, 12_500),
            (154_818_750, "Traficom Alue 2d K6 DMR", DemodMode::NFM, 12_500),
            (154_856_250, "Traficom Alue 2d K7 DMR", DemodMode::NFM, 12_500),
            (154_893_750, "Traficom Alue 2d K8 DMR", DemodMode::NFM, 12_500),
            // Traficom Alue 3 (160 MHz)
            (147_100_000, "Traficom Alue 3 K1", DemodMode::NFM, 25_000),
            (152_050_000, "Traficom Alue 3 K2", DemodMode::NFM, 25_000),
            (152_100_000, "Traficom Alue 3 K3", DemodMode::NFM, 25_000),
            (160_250_000, "Traficom Alue 3 K4", DemodMode::NFM, 25_000),
            (160_275_000, "Traficom Alue 3 K5", DemodMode::NFM, 25_000),
            (160_300_000, "Traficom Alue 3 K6", DemodMode::NFM, 25_000),
            (170_425_000, "Traficom Alue 3 K7", DemodMode::NFM, 25_000),
            (170_450_000, "Traficom Alue 3 K8", DemodMode::NFM, 25_000),
            // Traficom Nostotyö
            (442_850_000, "Traficom Nostotyö K1", DemodMode::NFM, 25_000),
            (442_875_000, "Traficom Nostotyö K2", DemodMode::NFM, 25_000),
            (442_900_000, "Traficom Nostotyö K3", DemodMode::NFM, 25_000),
            (442_925_000, "Traficom Nostotyö K4", DemodMode::NFM, 25_000),
            (442_950_000, "Traficom Nostotyö K5", DemodMode::NFM, 25_000),
            (442_975_000, "Traficom Nostotyö K6", DemodMode::NFM, 25_000),
            // Traficom Alue 5 (450 MHz)
            (443_125_000, "Traficom Alue 5 K1", DemodMode::NFM, 25_000),
            (443_500_000, "Traficom Alue 5 K2", DemodMode::NFM, 25_000),
            (443_550_000, "Traficom Alue 5 K3", DemodMode::NFM, 25_000),
            (443_800_000, "Traficom Alue 5 K4", DemodMode::NFM, 25_000),
            (445_200_000, "Traficom Alue 5 K5", DemodMode::NFM, 25_000),
            (445_675_000, "Traficom Alue 5 K6", DemodMode::NFM, 25_000),
            (458_250_000, "Traficom Alue 5 K7", DemodMode::NFM, 25_000),
            (458_850_000, "Traficom Alue 5 K8", DemodMode::NFM, 25_000),
            (458_900_000, "Traficom Alue 5 K9", DemodMode::NFM, 25_000),
            // Traficom Alue 5d DMR
            (447_006_250, "Traficom Alue 5d K1 DMR", DemodMode::NFM, 12_500),
            (447_056_250, "Traficom Alue 5d K2 DMR", DemodMode::NFM, 12_500),
            (447_081_250, "Traficom Alue 5d K3 DMR", DemodMode::NFM, 12_500),
            (447_156_250, "Traficom Alue 5d K4 DMR", DemodMode::NFM, 12_500),
            (447_181_250, "Traficom Alue 5d K5 DMR", DemodMode::NFM, 12_500),
            (447_206_250, "Traficom Alue 5d K6 DMR", DemodMode::NFM, 12_500),
            (447_231_250, "Traficom Alue 5d K7 DMR", DemodMode::NFM, 12_500),
            (447_281_250, "Traficom Alue 5d K8 DMR", DemodMode::NFM, 12_500),
            // Traficom TETRA DMO
            (416_237_500, "Traficom TETRA DMO 1", DemodMode::NFM, 25_000),
            (426_237_500, "Traficom TETRA DMO 2", DemodMode::NFM, 25_000),
            // RHA68
            (68_100_000, "RHA68 K1", DemodMode::NFM, 25_000),
            (68_300_000, "RHA68 K2", DemodMode::NFM, 25_000),
            (68_425_000, "RHA68 K3", DemodMode::NFM, 25_000),
            (68_525_000, "RHA68 K4", DemodMode::NFM, 25_000),
            (71_025_000, "RHA68 K11 Metsästys", DemodMode::NFM, 25_000),
            (71_050_000, "RHA68 K12 Metsästys", DemodMode::NFM, 25_000),
            (71_100_000, "RHA68 K14 Metsästys", DemodMode::NFM, 25_000),
            (71_375_000, "RHA68 K17 Metsästys", DemodMode::NFM, 25_000),
            (71_425_000, "RHA68 K19 Metsästys", DemodMode::NFM, 25_000),
            (71_475_000, "RHA68 K21 Metsästys", DemodMode::NFM, 25_000),
        ];

        for (freq, name, mode, bw) in default_list {
            bookmarks.insert(
                freq,
                Bookmark {
                    freq_hz: freq,
                    name: name.to_string(),
                    mode,
                    bandwidth_hz: bw,
                },
            );
        }

        let ranges = vec![
            ScanRangeProfile {
                name: "AIR".to_string(),
                center_freq_hz: 122_500_000,
                scan_start_hz: 118_100_000,
                scan_end_hz: 127_000_000,
                scan_step_hz: 25_000,
                mode: DemodMode::AM,
                enabled: true,
            },
            ScanRangeProfile {
                name: "Elinkeino VHF (154M)".to_string(),
                center_freq_hz: 154_700_000,
                scan_start_hz: 154_500_000,
                scan_end_hz: 154_900_000,
                scan_step_hz: 12_500,
                mode: DemodMode::NFM,
                enabled: false,
            },
            ScanRangeProfile {
                name: "Elinkeino VHF (147-171M)".to_string(),
                center_freq_hz: 160_000_000,
                scan_start_hz: 147_000_000,
                scan_end_hz: 171_000_000,
                scan_step_hz: 25_000,
                mode: DemodMode::NFM,
                enabled: false,
            },
            ScanRangeProfile {
                name: "Elinkeino UHF (442-459M)".to_string(),
                center_freq_hz: 450_000_000,
                scan_start_hz: 442_800_000,
                scan_end_hz: 459_000_000,
                scan_step_hz: 12_500,
                mode: DemodMode::NFM,
                enabled: false,
            },
            ScanRangeProfile {
                name: "RHA68 Harraste/Elinkeino".to_string(),
                center_freq_hz: 70_000_000,
                scan_start_hz: 68_000_000,
                scan_end_hz: 71_500_000,
                scan_step_hz: 25_000,
                mode: DemodMode::NFM,
                enabled: false,
            },
            ScanRangeProfile {
                name: "HAM 2m".to_string(),
                center_freq_hz: 145_000_000,
                scan_start_hz: 144_000_000,
                scan_end_hz: 146_000_000,
                scan_step_hz: 12_500,
                mode: DemodMode::NFM,
                enabled: false,
            },
            ScanRangeProfile {
                name: "HAM 70cm".to_string(),
                center_freq_hz: 434_000_000,
                scan_start_hz: 432_000_000,
                scan_end_hz: 438_000_000,
                scan_step_hz: 12_500,
                mode: DemodMode::NFM,
                enabled: false,
            },
            ScanRangeProfile {
                name: "PMR446".to_string(),
                center_freq_hz: 446_100_000,
                scan_start_hz: 446_000_000,
                scan_end_hz: 446_200_000,
                scan_step_hz: 12_500,
                mode: DemodMode::NFM,
                enabled: false,
            },
            ScanRangeProfile {
                name: "Telemetria / SCADA (440-445M)".to_string(),
                center_freq_hz: 442_500_000,
                scan_start_hz: 440_900_000,
                scan_end_hz: 445_000_000,
                scan_step_hz: 12_500,
                mode: DemodMode::NFM,
                enabled: false,
            },
            ScanRangeProfile {
                name: "ADS-B (1090 MHz)".to_string(),
                center_freq_hz: 1_090_000_000,
                scan_start_hz: 1_090_000_000,
                scan_end_hz: 1_090_000_000,
                scan_step_hz: 0,
                mode: DemodMode::AM,
                enabled: false,
            },
            ScanRangeProfile {
                name: "Broadcast FM (87.5-108M)".to_string(),
                center_freq_hz: 100_000_000,
                scan_start_hz: 87_500_000,
                scan_end_hz: 108_000_000,
                scan_step_hz: 100_000,
                mode: DemodMode::BCFM,
                enabled: false,
            },
        ];

        Self {
            selected_device: "Airspy R2".to_string(),
            center_freq_hz: 122_500_000, // Central Airband
            sample_rate: 10_000_000,     // 10 MSPS
            rf_gain: 18,
            scan_start_hz: 118_100_000,
            scan_end_hz: 127_000_000,
            scan_step_hz: 25_000,
            squelch_snr_db: 5.0, // 5.0 dB relative squelch matching MykolaSDR
            vad_enabled: true,
            vad_threshold: 0.35,
            hangover_ms: 1500, // 1.50s hysteresis matching MykolaSDR
            max_vfos: 3,
            active_range: "AIR".to_string(),
            ranges,
            bookmarks,
            blacklist: Vec::new(),
            audio_device: default_audio_device(),
        }
    }
}

fn get_config_file_path() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("scanner_config.json");
            if p.exists() {
                return p;
            }
        }
    }
    let local = std::path::PathBuf::from("scanner_config.json");
    if local.exists() {
        return local;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("scanner_config.json");
        }
    }
    local
}

impl ScannerConfig {
    pub fn load_or_default() -> Self {
        let path = get_config_file_path();
        let default_cfg = Self::default();
        if path.exists() {
            if let Ok(data) = fs::read_to_string(&path) {
                if let Ok(mut cfg) = serde_json::from_str::<ScannerConfig>(&data) {
                    cfg.max_vfos = cfg.max_vfos.clamp(1, 10);
                    cfg.ranges.retain(|r| !is_temporary_scan_profile(&r.name));
                    let usable_bw = ((cfg.sample_rate as f64) * 0.85).round() as u64;
                    if (cfg.scan_end_hz - cfg.scan_start_hz) > usable_bw {
                        cfg.center_freq_hz = cfg.scan_start_hz + (usable_bw / 2);
                    }
                    return cfg;
                }
            }
        }
        let mut cfg = default_cfg;
        cfg.ranges.retain(|r| !is_temporary_scan_profile(&r.name));
        let usable_bw = ((cfg.sample_rate as f64) * 0.85).round() as u64;
        if (cfg.scan_end_hz - cfg.scan_start_hz) > usable_bw {
            cfg.center_freq_hz = cfg.scan_start_hz + (usable_bw / 2);
        }
        let _ = cfg.save();
        cfg
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = get_config_file_path();
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn is_blacklisted(&self, freq_hz: u64, tolerance_hz: u64) -> bool {
        self.blacklist.iter().any(|&f| f.abs_diff(freq_hz) <= tolerance_hz)
    }

    pub fn add_blacklist(&mut self, freq_hz: u64) {
        if !self.is_blacklisted(freq_hz, 4000) {
            self.blacklist.push(freq_hz);
            self.blacklist.sort();
            let _ = self.save();
        }
    }

    pub fn remove_blacklist(&mut self, freq_hz: u64) {
        self.blacklist.retain(|&f| f.abs_diff(freq_hz) > 4000);
        let _ = self.save();
    }

    pub fn get_comment(&self, freq_hz: u64, tolerance_hz: u64) -> Option<&Bookmark> {
        let max_tol = tolerance_hz.min(3500);
        self.bookmarks
            .values()
            .filter(|bm| bm.freq_hz.abs_diff(freq_hz) <= max_tol)
            .min_by_key(|bm| bm.freq_hz.abs_diff(freq_hz))
    }
}

/// Returns true if a profile name corresponds to a temporary popup sweep rather than a user configured band
pub fn is_temporary_scan_profile(name: &str) -> bool {
    name == "AIRBAND (VHF Ilmailu)"
        || name.contains("MARINE")
        || name.contains("Radiosonde")
        || name.contains("SCADA")
        || name.contains("ISM Band")
        || name.contains("PMR446 Channels")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_or_default() {
        let cfg = ScannerConfig::load_or_default();
        assert!(cfg.ranges.len() >= 8);
        assert!(cfg.ranges.iter().any(|r| r.name.contains("Elinkeino VHF")));
        assert!(cfg.max_vfos >= 1 && cfg.max_vfos <= 10);
    }

    #[test]
    fn test_is_temporary_scan_profile_identification() {
        assert!(is_temporary_scan_profile("AIRBAND (VHF Ilmailu)"));
        assert!(is_temporary_scan_profile("400-406M Radiosondes"));
        assert!(is_temporary_scan_profile("406-430M SCADA Telemetry"));
        assert!(is_temporary_scan_profile("433 MHz ISM Band"));
        assert!(is_temporary_scan_profile("SCADA / Telemetry (440-445M)"));
        assert!(is_temporary_scan_profile("PMR446 Channels 1-16"));

        // User's standard configured bands must NOT be flagged as temporary
        assert!(!is_temporary_scan_profile("AIR"));
        assert!(!is_temporary_scan_profile("HAM 2m"));
        assert!(!is_temporary_scan_profile("HAM 70cm"));
        assert!(!is_temporary_scan_profile("PMR446"));
        assert!(!is_temporary_scan_profile("Elinkeino VHF (154M)"));
        assert!(!is_temporary_scan_profile("RHA68 Harraste/Elinkeino"));
    }
}
