#![allow(dead_code)]

use std::collections::HashMap;
use std::time::Instant;

/// Represents a decoded aviation ACARS or VDL2 telemetry / position packet
#[derive(Clone, Debug, PartialEq)]
pub struct AirPacket {
    pub freq_hz: u64,
    pub timestamp: String,
    pub callsign: String,
    pub tail: String,
    pub airline: Option<String>,
    pub aircraft_type: Option<String>,
    pub origin: Option<String>,
    pub destination: Option<String>,
    pub route: Option<String>,
    pub altitude_ft: Option<i32>,
    pub speed_kts: Option<u32>,
    pub heading_deg: Option<f32>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub waypoint: Option<String>,
    pub message_type: String,
    pub human_summary: String,
    pub raw_text: String,
    pub signal_snr_db: f32,
}

/// Represents an aircraft tracked in memory with its latest state and flight path
#[derive(Clone, Debug)]
pub struct AirStation {
    pub tail: String,
    pub callsign: String,
    pub airline: Option<String>,
    pub aircraft_type: Option<String>,
    pub route: Option<String>,
    pub origin: Option<String>,
    pub destination: Option<String>,
    pub altitude_ft: Option<i32>,
    pub speed_kts: Option<u32>,
    pub heading_deg: Option<f32>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub last_waypoint: Option<String>,
    pub last_seen: Instant,
    pub last_timestamp: String,
    pub packet_count: u32,
    pub latest_message: String,
    pub latest_summary: String,
    pub track_history: Vec<(f64, f64)>,
}

/// Major international and Finnish airport lookup table (ICAO code -> Friendly name)
pub fn resolve_airport(icao: &str) -> Option<&'static str> {
    match icao.to_uppercase().as_str() {
        "EFHK" => Some("Helsinki-Vantaa"),
        "EFTP" => Some("Tampere-Pirkkala"),
        "EFTU" => Some("Turku"),
        "EFOU" => Some("Oulu"),
        "EFRO" => Some("Rovaniemi"),
        "EFVA" => Some("Vaasa"),
        "EFKU" => Some("Kuopio"),
        "EFJO" => Some("Joensuu"),
        "EFJY" => Some("Jyväskylä"),
        "EFIV" => Some("Ivalo"),
        "EFKT" => Some("Kittilä"),
        "EFKS" => Some("Kuusamo"),
        "EFMA" => Some("Mariehamn"),
        "EFPO" => Some("Pori"),
        "EFSA" => Some("Savonlinna"),
        "EFMI" => Some("Mikkeli"),
        "EETN" => Some("Tallinn Lennart Meri"),
        "EVRA" => Some("Riga"),
        "EYVI" => Some("Vilnius"),
        "ESSA" => Some("Stockholm Arlanda"),
        "ESGG" => Some("Gothenburg Landvetter"),
        "ENGM" => Some("Oslo Gardermoen"),
        "EKCH" => Some("Copenhagen Kastrup"),
        "EGLL" => Some("London Heathrow"),
        "EGKK" => Some("London Gatwick"),
        "EGSS" => Some("London Stansted"),
        "EDDF" => Some("Frankfurt am Main"),
        "EDDM" => Some("Munich"),
        "EDDB" => Some("Berlin Brandenburg"),
        "EHAM" => Some("Amsterdam Schiphol"),
        "LFPG" => Some("Paris Charles de Gaulle"),
        "LEMD" => Some("Madrid Barajas"),
        "LEBL" => Some("Barcelona El Prat"),
        "LIRF" => Some("Rome Fiumicino"),
        "LOWW" => Some("Vienna"),
        "LSZH" => Some("Zurich"),
        "EPWA" => Some("Warsaw Chopin"),
        "LKPR" => Some("Prague"),
        "KJFK" => Some("New York JFK"),
        "KORD" => Some("Chicago O'Hare"),
        "KLAX" => Some("Los Angeles"),
        "VHHH" => Some("Hong Kong"),
        "RJAA" => Some("Tokyo Narita"),
        "RJTT" => Some("Tokyo Haneda"),
        "WSSS" => Some("Singapore Changi"),
        "OMDB" => Some("Dubai"),
        "OTHH" => Some("Doha"),
        "LTFM" => Some("Istanbul"),
        _ => None,
    }
}

/// Airline designator prefix lookup (IATA/ICAO -> Full airline name)
pub fn resolve_airline(prefix: &str) -> Option<&'static str> {
    let p = prefix.to_uppercase();
    if p.starts_with("AY") || p.starts_with("FIN") {
        Some("Finnair")
    } else if p.starts_with("SK") || p.starts_with("SAS") {
        Some("SAS Scandinavian Airlines")
    } else if p.starts_with("LH") || p.starts_with("DLH") {
        Some("Lufthansa")
    } else if p.starts_with("BA") || p.starts_with("BAW") {
        Some("British Airways")
    } else if p.starts_with("AF") || p.starts_with("AFR") {
        Some("Air France")
    } else if p.starts_with("KL") || p.starts_with("KLM") {
        Some("KLM Royal Dutch Airlines")
    } else if p.starts_with("DY") || p.starts_with("NAX") {
        Some("Norwegian Air Shuttle")
    } else if p.starts_with("W6") || p.starts_with("WZZ") {
        Some("Wizz Air")
    } else if p.starts_with("FR") || p.starts_with("RYR") {
        Some("Ryanair")
    } else if p.starts_with("TK") || p.starts_with("THY") {
        Some("Turkish Airlines")
    } else if p.starts_with("LX") || p.starts_with("SWR") {
        Some("Swiss International")
    } else if p.starts_with("OS") || p.starts_with("AUA") {
        Some("Austrian Airlines")
    } else if p.starts_with("BT") || p.starts_with("BTI") {
        Some("airBaltic")
    } else if p.starts_with("FI") || p.starts_with("ICE") {
        Some("Icelandair")
    } else if p.starts_with("QR") || p.starts_with("QTR") {
        Some("Qatar Airways")
    } else if p.starts_with("EK") || p.starts_with("UAE") {
        Some("Emirates")
    } else if p.starts_with("FN") || p.starts_with("FINN") {
        Some("Finnish Air Force")
    } else if p.starts_with("RV") || p.starts_with("RAJA") {
        Some("Finnish Border Guard")
    } else {
        None
    }
}

/// Aircraft country of registration lookup
pub fn resolve_country(tail: &str) -> &'static str {
    let t = tail.to_uppercase();
    if t.starts_with("OH-") || t.starts_with(".OH-") {
        "Finland"
    } else if t.starts_with("SE-") || t.starts_with(".SE-") {
        "Sweden"
    } else if t.starts_with("LN-") || t.starts_with(".LN-") {
        "Norway"
    } else if t.starts_with("OY-") || t.starts_with(".OY-") {
        "Denmark"
    } else if t.starts_with("ES-") || t.starts_with(".ES-") {
        "Estonia"
    } else if t.starts_with("YL-") || t.starts_with(".YL-") {
        "Latvia"
    } else if t.starts_with("LY-") || t.starts_with(".LY-") {
        "Lithuania"
    } else if t.starts_with("D-") || t.starts_with(".D-") {
        "Germany"
    } else if t.starts_with("F-") || t.starts_with(".F-") {
        "France"
    } else if t.starts_with("G-") || t.starts_with(".G-") {
        "United Kingdom"
    } else if t.starts_with("PH-") || t.starts_with(".PH-") {
        "Netherlands"
    } else if t.starts_with("OO-") || t.starts_with(".OO-") {
        "Belgium"
    } else if t.starts_with("HB-") || t.starts_with(".HB-") {
        "Switzerland"
    } else if t.starts_with("OE-") || t.starts_with(".OE-") {
        "Austria"
    } else if t.starts_with("EI-") || t.starts_with(".EI-") || t.starts_with("EJ-") {
        "Ireland"
    } else if t.starts_with("SP-") || t.starts_with(".SP-") {
        "Poland"
    } else if t.starts_with("TC-") || t.starts_with(".TC-") {
        "Turkey"
    } else if t.starts_with("N") || t.starts_with(".N") {
        "United States"
    } else {
        "International"
    }
}

/// Decodes standard ACARS label codes to human-readable English explanations
pub fn resolve_label(label: &str) -> &'static str {
    match label.trim() {
        "10" => "Out of Gate",
        "15" => "Airborne / Takeoff",
        "16" => "Touchdown / Landing",
        "17" => "In Gate / Arrival",
        "5D" => "Position Report",
        "H1" => "Flight Plan / Airline Ops",
        "RA" => "Route Analysis / Winds",
        "Q0" => "ACARS Link Ping",
        "SQ" => "Squawk Update",
        "WX" => "Weather Observation (PIREP)",
        "80" => "Engine Health Telemetry",
        "B6" => "CPDLC ATC Clearance",
        "SA" => "METAR / TAF Request",
        _ => "Aviation Data",
    }
}

/// Parses coordinates from standard ACARS and VDL2 position reporting strings
/// Handles ARINC 620 degrees + decimal minutes, decimal degrees, and degrees + minutes:
/// Examples:
/// "POSN N60189E024582,350,1205,RUMAX" -> (60.315°N, 24.970°E, 35000 ft, RUMAX)
/// "N60.317/E024.963" -> (60.317°N, 24.963°E)
/// "6018N 02458E" -> (60.300°N, 24.967°E)
/// "POS01/N6018.9/E02458.2/FL360"
pub fn parse_acars_position(text: &str) -> (Option<f64>, Option<f64>, Option<i32>, Option<String>) {
    let mut lat = None;
    let mut lon = None;
    let mut alt = None;
    let mut waypoint = None;

    // Helper to parse latitude substring:
    // e.g. "N60189" -> 60 deg 18.9 min -> 60 + 18.9/60 = 60.315
    //      "N6018"  -> 60 deg 18 min   -> 60 + 18/60 = 60.300
    //      "N60.317" -> 60.317
    //      "6018N"  -> 60 deg 18 min   -> 60 + 18/60 = 60.300
    let parse_lat_str = |s: &str| -> Option<f64> {
        let clean = s.trim();
        if clean.is_empty() {
            return None;
        }
        let is_south = clean.starts_with('S') || clean.ends_with('S');
        let num_str = clean.trim_matches(|c: char| c == 'N' || c == 'S' || c == '/' || c == ',');
        if num_str.contains('.') {
            // Decimal degrees (e.g. 60.317) or decimal minutes (e.g. 6018.9)
            if let Ok(v) = num_str.parse::<f64>() {
                if (0.0..=90.0).contains(&v) {
                    return Some(if is_south { -v } else { v });
                } else if (1000.0..=9000.0).contains(&v) {
                    let deg = (v / 100.0).floor();
                    let min = v - (deg * 100.0);
                    let val = deg + (min / 60.0);
                    return Some(if is_south { -val } else { val });
                }
            }
        } else {
            // Integer string, e.g. "601854" (6 digits DMS: 60 deg, 18 min, 54 sec), "60189" (5 digits: 60 deg, 18.9 min) or "6018" (4 digits: 60 deg, 18 min)
            if num_str.len() == 6 {
                if let (Ok(deg), Ok(min), Ok(sec)) = (num_str[..2].parse::<f64>(), num_str[2..4].parse::<f64>(), num_str[4..].parse::<f64>()) {
                    if (0.0..=90.0).contains(&deg) && (0.0..60.0).contains(&min) && (0.0..60.0).contains(&sec) {
                        let val = deg + (min / 60.0) + (sec / 3600.0);
                        return Some(if is_south { -val } else { val });
                    }
                }
            } else if num_str.len() == 5 {
                if let (Ok(deg), Ok(tenth_min)) = (num_str[..2].parse::<f64>(), num_str[2..].parse::<f64>()) {
                    let min = tenth_min / 10.0;
                    if (0.0..=90.0).contains(&deg) && (0.0..60.0).contains(&min) {
                        let val = deg + (min / 60.0);
                        return Some(if is_south { -val } else { val });
                    }
                }
            } else if num_str.len() == 4 {
                if let (Ok(deg), Ok(min)) = (num_str[..2].parse::<f64>(), num_str[2..].parse::<f64>()) {
                    if (0.0..=90.0).contains(&deg) && (0.0..60.0).contains(&min) {
                        let val = deg + (min / 60.0);
                        return Some(if is_south { -val } else { val });
                    }
                }
            }
        }
        None
    };

    // Helper to parse longitude substring:
    // e.g. "E0245812" -> 24 deg 58 min 12 sec -> 24 + 58/60 + 12/3600 = 24.970
    //      "E024582"  -> 24 deg 58.2 min -> 24 + 58.2/60 = 24.970
    //      "E02458"   -> 24 deg 58 min   -> 24 + 58/60 = 24.967
    //      "E024.963" or "E24.963" -> 24.963
    //      "02458E"   -> 24 deg 58 min
    let parse_lon_str = |s: &str| -> Option<f64> {
        let clean = s.trim();
        if clean.is_empty() {
            return None;
        }
        let is_west = clean.starts_with('W') || clean.ends_with('W');
        let num_str = clean.trim_matches(|c: char| c == 'E' || c == 'W' || c == '/' || c == ',');
        if num_str.contains('.') {
            if let Ok(v) = num_str.parse::<f64>() {
                if (0.0..=180.0).contains(&v) {
                    return Some(if is_west { -v } else { v });
                } else if (1000.0..=18000.0).contains(&v) {
                    let deg = (v / 100.0).floor();
                    let min = v - (deg * 100.0);
                    let val = deg + (min / 60.0);
                    return Some(if is_west { -val } else { val });
                }
            }
        } else {
            // Integer string, e.g. "0245812" (7 digits DMS), "024582" (6 digits: 024 deg, 58.2 min) or "24582" (5 digits: 24 deg, 58.2 min)
            if num_str.len() == 7 {
                if let (Ok(deg), Ok(min), Ok(sec)) = (num_str[..3].parse::<f64>(), num_str[3..5].parse::<f64>(), num_str[5..].parse::<f64>()) {
                    if (0.0..=180.0).contains(&deg) && (0.0..60.0).contains(&min) && (0.0..60.0).contains(&sec) {
                        let val = deg + (min / 60.0) + (sec / 3600.0);
                        return Some(if is_west { -val } else { val });
                    }
                }
            } else if num_str.len() == 6 {
                if let (Ok(deg), Ok(tenth_min)) = (num_str[..3].parse::<f64>(), num_str[3..].parse::<f64>()) {
                    let min = tenth_min / 10.0;
                    if (0.0..=180.0).contains(&deg) && (0.0..60.0).contains(&min) {
                        let val = deg + (min / 60.0);
                        return Some(if is_west { -val } else { val });
                    }
                }
            } else if num_str.len() == 5 {
                if let (Ok(deg), Ok(min)) = (num_str[..3].parse::<f64>(), num_str[3..].parse::<f64>()) {
                    if (0.0..=180.0).contains(&deg) && (0.0..60.0).contains(&min) {
                        let val = deg + (min / 60.0);
                        return Some(if is_west { -val } else { val });
                    }
                } else if let (Ok(deg), Ok(tenth_min)) = (num_str[..2].parse::<f64>(), num_str[2..].parse::<f64>()) {
                    let min = tenth_min / 10.0;
                    if (0.0..=180.0).contains(&deg) && (0.0..60.0).contains(&min) {
                        let val = deg + (min / 60.0);
                        return Some(if is_west { -val } else { val });
                    }
                }
            } else if num_str.len() == 4 {
                if let (Ok(deg), Ok(min)) = (num_str[..2].parse::<f64>(), num_str[2..].parse::<f64>()) {
                    if (0.0..=180.0).contains(&deg) && (0.0..60.0).contains(&min) {
                        let val = deg + (min / 60.0);
                        return Some(if is_west { -val } else { val });
                    }
                }
            }
        }
        None
    };

    for tok in text.split(|c: char| c.is_whitespace() || c == ',' || c == '/') {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }

        // Combined token: e.g. "N60189E024582" or "N60.317E024.963"
        if (t.starts_with('N') || t.starts_with('S')) && (t.contains('E') || t.contains('W')) && t.len() >= 8 {
            if let Some(e_idx) = t.find('E').or_else(|| t.find('W')) {
                let lat_part = &t[..e_idx];
                let lon_part = &t[e_idx..];
                if lat.is_none() {
                    lat = parse_lat_str(lat_part);
                }
                if lon.is_none() {
                    lon = parse_lon_str(lon_part);
                }
            }
        } else if (t.starts_with('N') || t.starts_with('S') || t.ends_with('N') || t.ends_with('S')) && t.len() >= 4 {
            if lat.is_none() {
                lat = parse_lat_str(t);
            }
        } else if (t.starts_with('E') || t.starts_with('W') || t.ends_with('E') || t.ends_with('W')) && t.len() >= 4 {
            if lon.is_none() {
                lon = parse_lon_str(t);
            }
        } else if t.starts_with("FL") && t.len() >= 4 {
            if let Ok(lvl) = t[2..].parse::<i32>() {
                alt = Some(lvl * 100);
            }
        } else if t.len() == 3 && t.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(lvl) = t.parse::<i32>() {
                if (50..=450).contains(&lvl) && alt.is_none() {
                    alt = Some(lvl * 100);
                }
            }
        } else if t.len() == 5 && t.chars().all(|c| c.is_ascii_uppercase()) {
            if t != "POSN" && t != "METAR" && t != "ACARS" && t != "SPEED" && t != "WINDS" && waypoint.is_none() {
                waypoint = Some(t.to_string());
            }
        }
    }

    (lat, lon, alt, waypoint)
}

/// Validates whether an aircraft registration matches recognized national prefixes
pub fn is_valid_registration(token: &str) -> bool {
    let t = token.trim_start_matches('.').to_uppercase();
    if t.len() < 4 || t.len() > 8 {
        return false;
    }
    // A registration cannot contain decimal dots, slashes, or commas
    if t.contains('.') || t.contains('/') || t.contains(',') {
        return false;
    }
    // US registrations start with N: must be N + 1-5 chars where first char is 1-9 (e.g. N12345, N700AF)
    if t.starts_with('N') {
        let rest = &t[1..];
        if rest.len() >= 4 && rest.chars().all(|c| c.is_ascii_digit()) {
            return false; // Coordinate like N6018 or N60189
        }
        if let Some(first) = rest.chars().next() {
            return ('1'..='9').contains(&first) && rest.len() <= 5 && rest.chars().all(|c| c.is_ascii_alphanumeric());
        }
        return false;
    }
    const PREFIXES: &[&str] = &[
        "OH-", "SE-", "LN-", "OY-", "ES-", "YL-", "LY-", "D-", "F-", "G-", "PH-",
        "OO-", "HB-", "OE-", "EI-", "EJ-", "SP-", "TC-", "TF-", "9H-", "CS-", "EC-",
        "I-",
    ];
    for &p in PREFIXES {
        if t.starts_with(p) {
            let rest = &t[p.len()..];
            if rest.len() >= 2 && rest.len() <= 4 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                return true;
            }
        }
    }
    false
}

/// Validates whether a callsign matches recognized commercial or military airline prefixes
pub fn is_valid_callsign(token: &str) -> bool {
    let t = token.to_uppercase();
    if t.len() < 4 || t.len() > 8 {
        return false;
    }
    const AIRLINE_PREFIXES: &[&str] = &[
        "AY", "FIN", "SK", "SAS", "LH", "DLH", "BA", "BAW", "AF", "AFR", "KL", "KLM",
        "DY", "NAX", "NOZ", "W6", "WZZ", "FR", "RYR", "TK", "THY", "LX", "SWR", "OS",
        "AUA", "BT", "BTI", "FI", "ICE", "QR", "QTR", "EK", "UAE", "EW", "EWG", "U2",
        "EZY", "FN", "FINN", "RV", "RAJA",
    ];
    for &p in AIRLINE_PREFIXES {
        if t.starts_with(p) {
            let rest = &t[p.len()..];
            // Legitimate flight number: 1 to 4 characters, first character MUST be a digit ('0'..='9')
            if let Some(first) = rest.chars().next() {
                if ('0'..='9').contains(&first) && rest.len() <= 5 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return true;
                }
            }
        }
    }
    false
}

/// Parses an ACARS or VDL2 payload string into a clean, human-readable AirPacket
pub fn parse_air_message(raw_msg: &str, freq_hz: u64, snr_db: f32) -> Option<AirPacket> {
    let clean = raw_msg.trim();
    if clean.is_empty() {
        return None;
    }

    // Extract tail / registration (often formatted as .OH-LWA or OH-LWA)
    let mut tail = String::new();
    let mut callsign = String::new();
    let mut origin = None;
    let mut destination = None;
    let mut label: Option<&str> = None;

    for token in clean.split(|c: char| c.is_whitespace() || c == '/' || c == ',') {
        let t = token.trim();
        if is_valid_registration(t) {
            tail = t.trim_start_matches('.').to_string();
        } else if is_valid_callsign(t) {
            callsign = t.to_string();
        } else if t.starts_with("ICAO:") && t.len() >= 7 {
            callsign = t.to_string();
        } else if t.len() == 4 && t.starts_with('E') && t.chars().all(|c| c.is_ascii_uppercase()) {
            if let Some(apt) = resolve_airport(t) {
                if origin.is_none() {
                    origin = Some(format!("{} ({})", t, apt));
                } else if destination.is_none() {
                    destination = Some(format!("{} ({})", t, apt));
                }
            }
        } else if t.len() == 2 && ["10", "15", "16", "17", "5D", "H1", "RA", "Q0", "WX", "80", "B6"].contains(&t) {
            label = Some(t);
        }
    }

    // STRICT REJECTION: If neither a valid registration nor a valid airline callsign is found,
    // do NOT accept arbitrary noise/scrambled tokens like "o8zH" or "U6UI"!
    if tail.is_empty() && callsign.is_empty() {
        return None;
    }

    let (lat, lon, alt, waypoint) = parse_acars_position(clean);
    let airline = resolve_airline(&callsign).or_else(|| resolve_airline(&tail)).map(|s| s.to_string());
    let route = match (&origin, &destination) {
        (Some(o), Some(d)) => Some(format!("{} ➔ {}", o, d)),
        _ => None,
    };

    // Designate message type truthfully:
    // Only designate as "Position Report" if coordinates were actually parsed!
    let msg_type = if lat.is_some() && lon.is_some() {
        "Position Report".to_string()
    } else if let Some(lbl) = label {
        if lbl == "5D" {
            "Position Update".to_string()
        } else {
            resolve_label(lbl).to_string()
        }
    } else {
        "Aviation Data Link".to_string()
    };

    // Compose crystal-clear human-readable English summary without clutter
    let summary = {
        let airl = airline.as_deref().unwrap_or("Aircraft");
        let flight_ident = if !callsign.is_empty() { format!("flight {}", callsign) } else { format!("tail {}", tail) };
        let reg = if !tail.is_empty() && tail != callsign { format!(" ({})", tail) } else { String::new() };

        let mut parts = vec![format!("{} {}{}: {}", airl, flight_ident, reg, msg_type)];
        if let Some(r) = &route {
            parts.push(format!("Route: {}", r));
        }
        if let Some(a) = alt {
            parts.push(format!("Alt: {} ft (FL{})", a, a / 100));
        }
        if let Some(wp) = &waypoint {
            parts.push(format!("Waypoint: {}", wp));
        }
        if let (Some(la), Some(lo)) = (lat, lon) {
            parts.push(format!("Position: {:.4}°N, {:.4}°E", la, lo));
        }
        parts.join(" | ")
    };

    let now_str = {
        let now = std::time::SystemTime::now();
        let dur = now.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap_or_default();
        let secs = dur.as_secs();
        let hours = (secs / 3600) % 24;
        let mins = (secs / 60) % 60;
        let s = secs % 60;
        format!("{:02}:{:02}:{:02}", hours, mins, s)
    };

    Some(AirPacket {
        freq_hz,
        timestamp: now_str,
        callsign: if callsign.is_empty() { tail.clone() } else { callsign },
        tail,
        airline,
        aircraft_type: None,
        origin,
        destination,
        route,
        altitude_ft: alt,
        speed_kts: None,
        heading_deg: None,
        latitude: lat,
        longitude: lon,
        waypoint,
        message_type: msg_type,
        human_summary: summary,
        raw_text: clean.to_string(),
        signal_snr_db: snr_db,
    })
}

use rustfft::num_complex::Complex32;

/// Parses a 24-bit ICAO aircraft address from AVLC DLC address octets (ICAO Doc 9776 / ARINC 631)
pub fn parse_dlc_addr(buf: &[u8]) -> u32 {
    if buf.len() < 4 {
        return 0;
    }
    let raw = ((buf[0] as u32) >> 1)
        | ((buf[1] as u32) << 6)
        | ((buf[2] as u32) << 13)
        | (((buf[3] as u32) & 0xfe) << 20);
    raw.reverse_bits() >> 4
}

/// Resolves 24-bit ICAO address allocation blocks to nation of registry
pub fn resolve_icao_country(icao: u32) -> &'static str {
    match icao {
        0x460000..=0x467FFF => "Finland",
        0x4A8000..=0x4AFFFF => "Sweden",
        0x478000..=0x47FFFF => "Norway",
        0x458000..=0x45FFFF => "Denmark",
        0x511000..=0x5113FF => "Estonia",
        0x502C00..=0x502FFF => "Latvia",
        0x503C00..=0x503FFF => "Lithuania",
        0x3C0000..=0x3FFFFF => "Germany",
        0x380000..=0x3BFFFF => "France",
        0x400000..=0x43FFFF => "United Kingdom",
        0x4CA000..=0x4CAFFF => "Ireland",
        0x480000..=0x487FFF => "Netherlands",
        0x448000..=0x44FFFF => "Belgium",
        0x4B8000..=0x4BFFFF => "Switzerland",
        0x440000..=0x447FFF => "Austria",
        0x498000..=0x49FFFF => "Poland",
        0x4B0000..=0x4B7FFF => "Turkey",
        0x501000..=0x5013FF => "Iceland",
        0x4D0000..=0x4D03FF => "Malta",
        0x490000..=0x497FFF => "Portugal",
        0x340000..=0x37FFFF => "Spain",
        0x300000..=0x33FFFF => "Italy",
        0xA00000..=0xAFFFFF => "United States",
        0xC00000..=0xC3FFFF => "Canada",
        0x06A000..=0x06A3FF => "Qatar",
        0x896000..=0x896FFF => "United Arab Emirates",
        _ => "International",
    }
}

/// Resolves standard national registration prefix from 24-bit ICAO address
pub fn resolve_icao_reg_prefix(icao: u32) -> &'static str {
    match icao {
        0x460000..=0x467FFF => "OH-",
        0x4A8000..=0x4AFFFF => "SE-",
        0x478000..=0x47FFFF => "LN-",
        0x458000..=0x45FFFF => "OY-",
        0x511000..=0x5113FF => "ES-",
        0x502C00..=0x502FFF => "YL-",
        0x503C00..=0x503FFF => "LY-",
        0x3C0000..=0x3FFFFF => "D-",
        0x380000..=0x3BFFFF => "F-",
        0x400000..=0x43FFFF => "G-",
        0x4CA000..=0x4CAFFF => "EI-",
        0x480000..=0x487FFF => "PH-",
        0x448000..=0x44FFFF => "OO-",
        0x4B8000..=0x4BFFFF => "HB-",
        0x440000..=0x447FFF => "OE-",
        0x498000..=0x49FFFF => "SP-",
        0x4B0000..=0x4B7FFF => "TC-",
        0x501000..=0x5013FF => "TF-",
        0x4D0000..=0x4D03FF => "9H-",
        0x490000..=0x497FFF => "CS-",
        0x340000..=0x37FFFF => "EC-",
        0x300000..=0x33FFFF => "I-",
        0xA00000..=0xAFFFFF => "N",
        0xC00000..=0xC3FFFF => "C-",
        0x06A000..=0x06A3FF => "A7-",
        0x896000..=0x896FFF => "A6-",
        _ => "",
    }
}

/// Validates whether a 24-bit ICAO address falls into a recognized civilian national allocation
pub fn is_valid_icao_aircraft(icao: u32) -> bool {
    matches!(
        icao,
        0x460000..=0x467FFF // Finland (OH-)
        | 0x4A8000..=0x4AFFFF // Sweden (SE-)
        | 0x478000..=0x47FFFF // Norway (LN-)
        | 0x458000..=0x45FFFF // Denmark (OY-)
        | 0x511000..=0x5113FF // Estonia (ES-)
        | 0x502C00..=0x502FFF // Latvia (YL-)
        | 0x503C00..=0x503FFF // Lithuania (LY-)
        | 0x3C0000..=0x3FFFFF // Germany (D-)
        | 0x380000..=0x3BFFFF // France (F-)
        | 0x400000..=0x43FFFF // United Kingdom (G-)
        | 0x4CA000..=0x4CAFFF // Ireland (EI-)
        | 0x480000..=0x487FFF // Netherlands (PH-)
        | 0x448000..=0x44FFFF // Belgium (OO-)
        | 0x4B8000..=0x4BFFFF // Switzerland (HB-)
        | 0x440000..=0x447FFF // Austria (OE-)
        | 0x498000..=0x49FFFF // Poland (SP-)
        | 0x4B0000..=0x4B7FFF // Turkey (TC-)
        | 0x501000..=0x5013FF // Iceland (TF-)
        | 0x4D0000..=0x4D03FF // Malta (9H-)
        | 0x490000..=0x497FFF // Portugal (CS-)
        | 0x340000..=0x37FFFF // Spain (EC-)
        | 0x300000..=0x33FFFF // Italy (I-)
        | 0xA00000..=0xAFFFFF // United States (N)
        | 0xC00000..=0xC3FFFF // Canada (C-)
        | 0x06A000..=0x06A3FF // Qatar (A7-)
        | 0x896000..=0x896FFF // United Arab Emirates (A6-)
    )
}

/// Resolves a 24-bit ICAO aircraft address to registration, operating airline, and aircraft model
pub fn resolve_icao_aircraft_info(icao: u32) -> Option<(&'static str, &'static str, &'static str)> {
    match icao {
        // Finnair Airbus A320-214 fleet
        0x461F62 => Some(("OH-LXA", "Finnair", "Airbus A320-214")),
        0x461F63 => Some(("OH-LXB", "Finnair", "Airbus A320-214")),
        0x461F64 => Some(("OH-LXC", "Finnair", "Airbus A320-214")),
        0x461F65 => Some(("OH-LXD", "Finnair", "Airbus A320-214")),
        0x461F66 => Some(("OH-LXE", "Finnair", "Airbus A320-214")),
        0x461F67 => Some(("OH-LXF", "Finnair", "Airbus A320-214")),
        0x461F68 => Some(("OH-LXG", "Finnair", "Airbus A320-214")),
        0x461F69 => Some(("OH-LXH", "Finnair", "Airbus A320-214")),
        0x461F6A => Some(("OH-LXI", "Finnair", "Airbus A320-214")),
        0x461F6B => Some(("OH-LXJ", "Finnair", "Airbus A320-214")),
        0x461F6C => Some(("OH-LXK", "Finnair", "Airbus A320-214")),

        // Finnair Airbus A321 fleet
        0x461F81 => Some(("OH-LZA", "Finnair", "Airbus A321-211")),
        0x461F82 => Some(("OH-LZB", "Finnair", "Airbus A321-211")),
        0x461F83 => Some(("OH-LZC", "Finnair", "Airbus A321-211")),
        0x461F84 => Some(("OH-LZD", "Finnair", "Airbus A321-211")),
        0x461F85 => Some(("OH-LZE", "Finnair", "Airbus A321-231")),
        0x461F86 => Some(("OH-LZF", "Finnair", "Airbus A321-231")),
        0x461F87 => Some(("OH-LZG", "Finnair", "Airbus A321-231")),
        0x461F88 => Some(("OH-LZH", "Finnair", "Airbus A321-231")),
        0x461F89 => Some(("OH-LZI", "Finnair", "Airbus A321-231")),
        0x461F8A => Some(("OH-LZJ", "Finnair", "Airbus A321-231")),
        0x461F8B => Some(("OH-LZK", "Finnair", "Airbus A321-231")),
        0x461F8C => Some(("OH-LZL", "Finnair", "Airbus A321-231")),
        0x461F8D => Some(("OH-LZM", "Finnair", "Airbus A321-231")),
        0x461F8E => Some(("OH-LZN", "Finnair", "Airbus A321-231")),
        0x461F8F => Some(("OH-LZO", "Finnair", "Airbus A321-231")),
        0x461F90 => Some(("OH-LZP", "Finnair", "Airbus A321-231")),
        0x461F91 => Some(("OH-LZR", "Finnair", "Airbus A321-231")),
        0x461F92 => Some(("OH-LZS", "Finnair", "Airbus A321-231")),
        0x461F93 => Some(("OH-LZT", "Finnair", "Airbus A321-231")),
        0x461F94 => Some(("OH-LZU", "Finnair", "Airbus A321-231")),

        // Finnair Airbus A319-112 fleet
        0x461F71 => Some(("OH-LVA", "Finnair", "Airbus A319-112")),
        0x461F72 => Some(("OH-LVB", "Finnair", "Airbus A319-112")),
        0x461F73 => Some(("OH-LVC", "Finnair", "Airbus A319-112")),
        0x461F74 => Some(("OH-LVD", "Finnair", "Airbus A319-112")),
        0x461F75 => Some(("OH-LVE", "Finnair", "Airbus A319-112")),

        // Finnair Airbus A350-941 flagship fleet
        0x461F48 => Some(("OH-LWA", "Finnair", "Airbus A350-941")),
        0x461F49 => Some(("OH-LWB", "Finnair", "Airbus A350-941")),
        0x461F4A => Some(("OH-LWC", "Finnair", "Airbus A350-941")),
        0x461F4B => Some(("OH-LWD", "Finnair", "Airbus A350-941")),
        0x461F4C => Some(("OH-LWE", "Finnair", "Airbus A350-941")),
        0x461F4D => Some(("OH-LWF", "Finnair", "Airbus A350-941")),
        0x461F4E => Some(("OH-LWG", "Finnair", "Airbus A350-941")),
        0x461F4F => Some(("OH-LWH", "Finnair", "Airbus A350-941")),
        0x461F50 => Some(("OH-LWI", "Finnair", "Airbus A350-941")),
        0x461F51 => Some(("OH-LWJ", "Finnair", "Airbus A350-941")),
        0x461F52 => Some(("OH-LWK", "Finnair", "Airbus A350-941")),
        0x461F53 => Some(("OH-LWL", "Finnair", "Airbus A350-941")),
        0x461F54 => Some(("OH-LWM", "Finnair", "Airbus A350-941")),
        0x461F55 => Some(("OH-LWN", "Finnair", "Airbus A350-941")),
        0x461F56 => Some(("OH-LWO", "Finnair", "Airbus A350-941")),
        0x461F57 => Some(("OH-LWP", "Finnair", "Airbus A350-941")),
        0x461F58 => Some(("OH-LWR", "Finnair", "Airbus A350-941")),
        0x461F59 => Some(("OH-LWS", "Finnair", "Airbus A350-941")),

        // Finnair Airbus A330-302 fleet
        0x461F31 => Some(("OH-LTO", "Finnair", "Airbus A330-302")),
        0x461F32 => Some(("OH-LTP", "Finnair", "Airbus A330-302")),
        0x461F33 => Some(("OH-LTR", "Finnair", "Airbus A330-302")),
        0x461F34 => Some(("OH-LTS", "Finnair", "Airbus A330-302")),
        0x461F35 => Some(("OH-LTT", "Finnair", "Airbus A330-302")),
        0x461F36 => Some(("OH-LTU", "Finnair", "Airbus A330-302")),
        0x461F37 => Some(("OH-LTV", "Finnair", "Airbus A330-302")),

        // Nordic Regional Airlines (Norra) / Finnair Embraer ERJ-190 fleet
        0x461E10 => Some(("OH-LKA", "Finnair / Norra", "Embraer ERJ-190")),
        0x461E11 => Some(("OH-LKB", "Finnair / Norra", "Embraer ERJ-190")),
        0x461E12 => Some(("OH-LKC", "Finnair / Norra", "Embraer ERJ-190")),
        0x461E13 => Some(("OH-LKD", "Finnair / Norra", "Embraer ERJ-190")),
        0x461E14 => Some(("OH-LKE", "Finnair / Norra", "Embraer ERJ-190")),
        0x461E15 => Some(("OH-LKF", "Finnair / Norra", "Embraer ERJ-190")),
        0x461E16 => Some(("OH-LKG", "Finnair / Norra", "Embraer ERJ-190")),
        0x461E17 => Some(("OH-LKH", "Finnair / Norra", "Embraer ERJ-190")),
        0x461E18 => Some(("OH-LKI", "Finnair / Norra", "Embraer ERJ-190")),
        0x461E19 => Some(("OH-LKJ", "Finnair / Norra", "Embraer ERJ-190")),
        0x461E1A => Some(("OH-LKK", "Finnair / Norra", "Embraer ERJ-190")),
        0x461E1B => Some(("OH-LKL", "Finnair / Norra", "Embraer ERJ-190")),

        // Air Baltic Airbus A220-300 fleet
        0x502D5B => Some(("YL-ABJ", "Air Baltic", "Airbus A220-300")),
        0x502D44 => Some(("YL-ABC", "Air Baltic", "Airbus A220-300")),
        0x502D89 => Some(("YL-ABU", "Air Baltic", "Airbus A220-300")),
        0x502D50..=0x502D9F => Some(("", "Air Baltic", "Airbus A220-300")),

        _ => None,
    }
}

/// Computes CRC-16-CCITT across an octet buffer (ISO 3309 / ARINC 631 / AVLC FCS)
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0x8408;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFF
}

/// Verifies whether an AVLC frame passes 16-bit Frame Check Sequence (FCS)
pub fn verify_avlc_fcs(frame: &[u8]) -> bool {
    if frame.len() < 10 {
        return false;
    }
    let mut crc: u16 = 0xFFFF;
    for &b in frame {
        crc ^= b as u16;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0x8408;
            } else {
                crc >>= 1;
            }
        }
    }
    crc == 0xF0B8
}

const VDL2_HDR_H: [u32; 5] = [
    0b0000000011111111111110000,
    0b0011111100001111111101000,
    0b1100011100110000111100100,
    0b1101101101010011001100010,
    0b0110100111100101010100001,
];

const VDL2_SYNDTABLE: [u32; 32] = [
    0b0000000000000000000000000,
    0b0000000000000000000000001,
    0b0000000000000000000000010,
    0b0100000000000000000000100,
    0b0000000000000000000000100,
    0b0100000000000000000000010,
    0b1000000000000000000000000,
    0b0100000000000000000000000,
    0b0000000000000000000001000,
    0b0010000000000000000000000,
    0b0001000000000000000000000,
    0b0000100000000000000000000,
    0b0000010000000000000000000,
    0b1000100000000000000000000,
    0b0000001000000000000000000,
    0b0000000100000000000000000,
    0b0000000000000000000010000,
    0b0000000010000000000000000,
    0b0100000000100000000000000,
    0b0000000001000000000000000,
    0b0100000001000000000000000,
    0b0000000000100000000000000,
    0b0000000000010000000000000,
    0b1000000010000000000000000,
    0b0000000000001000000000000,
    0b0000000000000100000000000,
    0b0000000000000010000000000,
    0b0000000000000001000000000,
    0b0000000000000000100000000,
    0b0000000000000000010000000,
    0b0000000000000000001000000,
    0b0000000000000000000100000,
];

pub fn reverse_bits_17(v: u32) -> u32 {
    let mut res = 0u32;
    for i in 0..17 {
        res |= ((v >> i) & 1) << (16 - i);
    }
    res
}

pub fn deinterleave_vdl2(input: &[u8], num_blocks: usize, last_block_len: usize) -> Vec<u8> {
    if num_blocks <= 1 || input.is_empty() {
        return input.to_vec();
    }
    let mut out = vec![vec![0u8; 249]; num_blocks];
    let mut row = 0;
    let mut col = 0;
    for &b in input {
        if row == num_blocks - 1 && col >= last_block_len {
            row = 0;
            col += 1;
        }
        if col >= 249 {
            break;
        }
        out[row][col] = b;
        row += 1;
        if row == num_blocks {
            row = 0;
            col += 1;
        }
    }
    let mut result = Vec::with_capacity(input.len());
    for r in 0..num_blocks {
        let len = if r == num_blocks - 1 { last_block_len } else { 249 };
        result.extend_from_slice(&out[r][..len.min(249)]);
    }
    result
}

pub fn hdlc_unstuff_bits(bits: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut cur_frame_bits = Vec::new();
    let mut ones = 0;
    let mut in_frame = false;

    for &bit in bits {
        if bit == 0 && ones == 5 {
            // Stuffed 0-bit: skip it
            ones = 0;
            continue;
        } else if bit == 1 {
            ones += 1;
            if ones > 6 {
                // Invalid sequence
                in_frame = false;
                cur_frame_bits.clear();
                ones = 0;
                continue;
            }
        }

        if bit == 0 {
            if ones == 6 {
                // Flag 01111110 (0x7E)
                if in_frame {
                    cur_frame_bits.truncate(cur_frame_bits.len().saturating_sub(6));
                    if cur_frame_bits.len() >= 80 {
                        // Pack bits to octets LSB first
                        let mut octets = Vec::with_capacity(cur_frame_bits.len() / 8);
                        for chunk in cur_frame_bits.chunks_exact(8) {
                            let mut byte = 0u8;
                            for (i, &b) in chunk.iter().enumerate() {
                                byte |= b << i;
                            }
                            octets.push(byte);
                        }
                        frames.push(octets);
                    }
                    cur_frame_bits.clear();
                }
                in_frame = true;
                ones = 0;
                continue;
            }
            ones = 0;
        }

        if in_frame {
            cur_frame_bits.push(bit);
        }
    }
    frames
}

/// Aviation Airband Data Decoder: Real-time ACARS (MSK 2400) & VDL Mode 2 (D8PSK 31.5 kbps)
#[derive(Clone)]
pub struct AirDecoder {
    pub packets: Vec<AirPacket>,
    pub saved_stations: HashMap<String, AirStation>,
    sample_rate: f32,
    prev_sample: f32,
    bit_buffer: u64,
    bits_count: usize,
    char_buffer: Vec<u8>,
    last_burst_time: Option<Instant>,
    active_freq_hz: u64,
    // VDL Mode 2 D8PSK burst processor (136.975 MHz)
    vdl2_burst_buf: Vec<Complex32>,
    vdl2_in_burst: bool,
    vdl2_peak_snr: f32,
    pub vdl2_burst_count: u32,
    pub last_vdl2_snr: f32,
    pub last_vdl2_dur_ms: f32,
    pub last_vdl2_burst_time: Option<Instant>,
    // ACARS MSK discriminator state
    msk_delay_buf: [f32; 32],
    msk_delay_idx: usize,
    msk_sample_count: usize,
    msk_cur_byte: u8,
    msk_bit_pos: usize,
    msk_in_frame: bool,
    pub adsb_decoder: crate::dsp::AdsbDecoder,
}

impl AirDecoder {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            packets: Vec::with_capacity(256),
            saved_stations: HashMap::new(),
            sample_rate,
            prev_sample: 0.0,
            bit_buffer: 0,
            bits_count: 0,
            char_buffer: Vec::with_capacity(256),
            last_burst_time: None,
            active_freq_hz: 131_725_000,
            vdl2_burst_buf: Vec::with_capacity(8192),
            vdl2_in_burst: false,
            vdl2_peak_snr: 0.0,
            vdl2_burst_count: 0,
            last_vdl2_snr: 0.0,
            last_vdl2_dur_ms: 0.0,
            last_vdl2_burst_time: None,
            msk_delay_buf: [0.0; 32],
            msk_delay_idx: 0,
            msk_sample_count: 0,
            msk_cur_byte: 0,
            msk_bit_pos: 0,
            msk_in_frame: false,
            adsb_decoder: crate::dsp::AdsbDecoder::new(sample_rate),
        }
    }

    pub fn set_freq(&mut self, freq_hz: u64) {
        self.active_freq_hz = freq_hz;
    }

    pub fn set_qth(&mut self, qth: (f64, f64)) {
        self.adsb_decoder.set_receiver_qth(qth);
    }

    /// Primary DSP entrypoint: processes complex baseband IQ (VDL2 D8PSK) and audio (ACARS MSK)
    pub fn process_samples(&mut self, iq: &[Complex32], audio: &[f32], snr_db: f32) {
        if !iq.is_empty() {
            self.process_vdl2(iq, snr_db);
        }
        if !audio.is_empty() {
            self.process_msk(audio, snr_db);
        }
    }

    /// Extended DSP entrypoint that accepts wideband RF samples (for 1090 MHz ADS-B Mode S PPM)
    pub fn process_samples_with_rf(&mut self, rf: &[Complex32], ddc: &[Complex32], audio: &[f32], snr_db: f32, sample_rate_rf: f32) {
        // 1090 MHz ADS-B Mode S decoding
        if self.active_freq_hz >= 1_080_000_000 && self.active_freq_hz <= 1_100_000_000 {
            let target_samples = if !rf.is_empty() { rf } else { ddc };
            let msgs = self.adsb_decoder.process_iq(target_samples, sample_rate_rf, snr_db);
            for m in msgs {
                let pkt = self.convert_adsb_to_air_packet(&m, snr_db);
                self.ingest_packet(pkt);
            }
            return;
        }

        // Standard VHF processing (VDL2 & ACARS)
        self.process_samples(ddc, audio, snr_db);
    }

    /// Converts a decoded Mode S ADS-B message into a unified AirPacket
    pub fn convert_adsb_to_air_packet(&self, m: &crate::dsp::AdsbMessage, snr_db: f32) -> AirPacket {
        convert_adsb_to_air_packet_with_freq(m, snr_db, self.active_freq_hz)
    }

    /// Legacy audio-only wrapper
    pub fn process_audio(&mut self, samples: &[f32], snr_db: f32) {
        self.process_samples(&[], samples, snr_db);
    }

    /// Real-time VDL Mode 2 (136.975 MHz) burst detection, D8PSK demodulation, and packet decoder
    fn process_vdl2(&mut self, iq: &[Complex32], snr_db: f32) {
        // Measure average power in this baseband block
        let mut power = 0.0f32;
        for &s in iq {
            power += s.norm_sqr();
        }
        let avg_p = power / (iq.len() as f32).max(1.0);

        // VDL2 burst threshold: RF signal present with sufficient SNR
        let is_burst = (snr_db >= 5.0 || avg_p > 0.003) && avg_p > 0.0008;

        if is_burst {
            if !self.vdl2_in_burst {
                self.vdl2_in_burst = true;
                self.vdl2_peak_snr = snr_db;
                self.vdl2_burst_buf.clear();
            } else if snr_db > self.vdl2_peak_snr {
                self.vdl2_peak_snr = snr_db;
            }

            if self.vdl2_burst_buf.len() < 8192 {
                self.vdl2_burst_buf.extend_from_slice(iq);
            }
        } else if self.vdl2_in_burst {
            // Burst has finished!
            self.vdl2_in_burst = false;
            let num_samples = self.vdl2_burst_buf.len();
            let duration_ms = (num_samples as f32 / 48.0).round();

            // Legitimate VDL Mode 2 burst duration: between 8 ms and 220 ms
            if duration_ms >= 8.0 && duration_ms <= 220.0 {
                self.decode_vdl2_burst(duration_ms, self.vdl2_peak_snr);
            }
            self.vdl2_burst_buf.clear();
        }
    }

    /// Demodulates D8PSK symbols and extracts VDL2 telemetry / payload
    fn decode_vdl2_burst(&mut self, duration_ms: f32, snr_db: f32) {
        // Record burst statistics for channel status
        self.vdl2_burst_count += 1;
        self.last_vdl2_snr = snr_db;
        self.last_vdl2_dur_ms = duration_ms;
        self.last_vdl2_burst_time = Some(Instant::now());

        if self.vdl2_burst_buf.len() < 96 {
            return;
        }

        // Preamble Unique Word phase sequence (ICAO Doc 9776 / dumpvdl2)
        const VDL2_PR_PHASE: [f32; 16] = [
            0.0 * std::f32::consts::FRAC_PI_4,
            3.0 * std::f32::consts::FRAC_PI_4,
            -3.0 * std::f32::consts::FRAC_PI_4,
            1.0 * std::f32::consts::FRAC_PI_4,
            1.0 * std::f32::consts::FRAC_PI_4,
            2.0 * std::f32::consts::FRAC_PI_4,
            0.0 * std::f32::consts::FRAC_PI_4,
            4.0 * std::f32::consts::FRAC_PI_4,
            -3.0 * std::f32::consts::FRAC_PI_4,
            4.0 * std::f32::consts::FRAC_PI_4,
            -2.0 * std::f32::consts::FRAC_PI_4,
            3.0 * std::f32::consts::FRAC_PI_4,
            1.0 * std::f32::consts::FRAC_PI_4,
            -2.0 * std::f32::consts::FRAC_PI_4,
            -3.0 * std::f32::consts::FRAC_PI_4,
            0.0 * std::f32::consts::FRAC_PI_4,
        ];

        let sps = 4.5714286f32; // 48000 / 10500 = 32 / 7

        // Helper to interpolate fractional samples
        let interp = |buf: &[Complex32], pos: f32| -> Complex32 {
            if pos <= 0.0 {
                return buf.first().copied().unwrap_or_default();
            }
            let idx = pos as usize;
            if idx + 1 >= buf.len() {
                return buf.last().copied().unwrap_or_default();
            }
            let frac = pos - (idx as f32);
            let s0 = buf[idx];
            let s1 = buf[idx + 1];
            Complex32::new(
                s0.re * (1.0 - frac) + s1.re * frac,
                s0.im * (1.0 - frac) + s1.im * frac,
            )
        };

        // Search for preamble unique word alignment across candidate start sample offsets
        let max_search = (self.vdl2_burst_buf.len().saturating_sub(90)).min(160);
        let mut best_start = 0.0f32;
        let mut best_err = f32::MAX;
        let mut best_freq_off = 0.0f32;
        let mut best_init_phase = 0.0f32;

        for offset in 0..max_search {
            let mut errvec = [0.0f32; 16];
            let mut unwrap = 0.0f32;
            let s0 = interp(&self.vdl2_burst_buf, offset as f32);
            let first_phi = s0.im.atan2(s0.re);
            let mut prev_err = first_phi - VDL2_PR_PHASE[0];
            errvec[0] = prev_err;

            for i in 1..16 {
                let sample_pos = offset as f32 + (i as f32) * sps;
                let s = interp(&self.vdl2_burst_buf, sample_pos);
                let phi = s.im.atan2(s.re);
                let cur_err = phi - VDL2_PR_PHASE[i];
                let diff = cur_err - prev_err;
                prev_err = cur_err;
                if diff > std::f32::consts::PI {
                    unwrap -= 2.0 * std::f32::consts::PI;
                } else if diff < -std::f32::consts::PI {
                    unwrap += 2.0 * std::f32::consts::PI;
                }
                errvec[i] = cur_err + unwrap;
            }

            let mean_y: f32 = errvec.iter().sum::<f32>() / 16.0;
            let mut num = 0.0f32;
            let mut denom = 0.0f32;
            for (k, &y) in errvec.iter().enumerate() {
                let x = k as f32 - 7.5;
                num += x * (y - mean_y);
                denom += x * x;
            }
            let slope = if denom > 0.0 { num / denom } else { 0.0 };
            let intercept = mean_y - slope * 7.5;

            let mut var = 0.0f32;
            for (k, &y) in errvec.iter().enumerate() {
                let fitted = intercept + slope * (k as f32);
                let e = y - fitted;
                var += e * e;
            }
            var /= 16.0;

            if var < best_err {
                best_err = var;
                best_start = offset as f32;
                best_freq_off = slope / sps;
                best_init_phase = intercept;
            }
        }

        // If preamble correlation was weak, fallback to adjacent-sample phase estimate
        if best_err > 1.8 {
            let lead_len = self.vdl2_burst_buf.len().min(128);
            let mut sum_diff = Complex32::new(0.0, 0.0);
            for w in self.vdl2_burst_buf[..lead_len].windows(2) {
                sum_diff += w[1] * Complex32::new(w[0].re, -w[0].im);
            }
            best_freq_off = sum_diff.im.atan2(sum_diff.re);
            best_start = 0.0;
            best_init_phase = 0.0;
        }

        // Derotate baseband samples to eliminate carrier frequency offset
        let mut derotated = Vec::with_capacity(self.vdl2_burst_buf.len());
        for (i, &s) in self.vdl2_burst_buf.iter().enumerate() {
            let phase = -best_freq_off * (i as f32) - best_init_phase;
            let rot = Complex32::new(phase.cos(), phase.sin());
            derotated.push(s * rot);
        }

        // Standard ICAO Doc 9776 / dumpvdl2 Gray mapping
        const GRAYCODE: [u8; 8] = [0, 1, 3, 2, 6, 7, 5, 4];

        // Sample D8PSK symbols starting right AFTER the 16 preamble symbols
        // Preamble is symbols 0..15. Data symbols begin at symbol 16.
        // Differential phase of symbol 16 is relative to symbol 15.
        let mut bits = Vec::new();
        let s_prev = interp(&derotated, best_start + 15.0 * sps);
        let mut prev_phi = s_prev.im.atan2(s_prev.re);

        let mut sym_idx = 16.0f32;
        while best_start + sym_idx * sps < derotated.len() as f32 {
            let s_cur = interp(&derotated, best_start + sym_idx * sps);
            let cur_phi = s_cur.im.atan2(s_cur.re);
            let mut dphi = cur_phi - prev_phi;
            prev_phi = cur_phi;

            if dphi < 0.0 {
                dphi += 2.0 * std::f32::consts::PI;
            } else if dphi >= 2.0 * std::f32::consts::PI {
                dphi -= 2.0 * std::f32::consts::PI;
            }

            let oct = ((dphi / std::f32::consts::FRAC_PI_4).round() as usize) % 8;
            let gray = GRAYCODE[oct];

            // 3 bits per symbol, MSB first
            bits.push((gray >> 2) & 1);
            bits.push((gray >> 1) & 1);
            bits.push(gray & 1);

            sym_idx += 1.0;
        }

        // Descramble using VDL2 standard LFSR polynomial x^15 + x + 1, initial state 0x6959
        let mut lfsr: u16 = 0x6959;
        let mut descrambled = Vec::with_capacity(bits.len());
        for &b in &bits {
            let scrambler_bit = ((lfsr >> 0) ^ (lfsr >> 14)) & 1;
            lfsr = (lfsr >> 1) | ((scrambler_bit) << 14);
            descrambled.push(b ^ (scrambler_bit as u8));
        }

        let mut candidate_frames = Vec::new();

        // Path A: Standard ICAO 25-bit Transmission Header decoding + Matrix Deinterleaving
        if descrambled.len() >= 25 {
            let mut header_word = 0u32;
            for &b in &descrambled[..25] {
                header_word = (header_word << 1) | (b as u32);
            }
            header_word &= 0x003F_FFFF; // 22 bits (17 TRLEN + 5 HDRFECLEN)

            let mut syndrome = 0u32;
            for (i, &h_row) in VDL2_HDR_H.iter().enumerate() {
                let p = (header_word & h_row).count_ones() & 1;
                syndrome |= p << (4 - i);
            }
            header_word ^= VDL2_SYNDTABLE[syndrome as usize];
            header_word >>= 5;
            let datalen_bits = reverse_bits_17(header_word & 0x1FFFF) as usize;

            if datalen_bits >= 80 && datalen_bits <= 16384 {
                let datalen_octets = (datalen_bits + 7) / 8;
                let payload_bits = &descrambled[25..];

                // Pack payload bits to octets (LSB first)
                let mut raw_octets = Vec::with_capacity(payload_bits.len() / 8);
                for chunk in payload_bits.chunks_exact(8) {
                    let mut byte = 0u8;
                    for (i, &bit) in chunk.iter().enumerate() {
                        byte |= bit << i;
                    }
                    raw_octets.push(byte);
                }

                if raw_octets.len() >= datalen_octets {
                    let num_blocks = (datalen_octets + 248) / 249;
                    let last_block_len = if datalen_octets % 249 == 0 { 249 } else { datalen_octets % 249 };
                    let deint = deinterleave_vdl2(&raw_octets[..datalen_octets], num_blocks, last_block_len);

                    // Unpack deinterleaved bytes back to bitstream for HDLC unstuffing
                    let mut deint_bits = Vec::with_capacity(deint.len() * 8);
                    for &byte in &deint {
                        for b in 0..8 {
                            deint_bits.push((byte >> b) & 1);
                        }
                    }
                    deint_bits.truncate(datalen_bits);

                    let frames = hdlc_unstuff_bits(&deint_bits);
                    for f in frames {
                        candidate_frames.push(f);
                    }

                    // Also check deinterleaved octets directly
                    if deint.len() >= 10 {
                        candidate_frames.push(deint);
                    }
                }
            }
        }

        // Path B: Direct HDLC flag unstuffing across the entire descrambled stream (fallback)
        let direct_frames = hdlc_unstuff_bits(&descrambled);
        for f in direct_frames {
            candidate_frames.push(f);
        }

        // Path C: Direct octet packing (LSB first) across full descrambled stream
        let mut direct_octets = Vec::with_capacity(descrambled.len() / 8);
        for chunk in descrambled.chunks_exact(8) {
            let mut byte = 0u8;
            for (i, &bit) in chunk.iter().enumerate() {
                byte |= bit << i;
            }
            direct_octets.push(byte);
        }
        if direct_octets.len() >= 10 {
            candidate_frames.push(direct_octets.clone());
        }

        // Evaluate candidate frames for valid AVLC frames
        let mut found_packet = false;
        for frame in &candidate_frames {
            if frame.len() < 10 {
                continue;
            }

            let fcs_valid = verify_avlc_fcs(frame);
            let dst_val = parse_dlc_addr(&frame[0..4]);
            let src_val = parse_dlc_addr(&frame[4..8]);
            let src_icao = src_val & 0x00FF_FFFF;
            let src_type = (src_val >> 24) & 0x07;
            let src_on_ground = ((src_val >> 27) & 0x01) == 1;

            let dst_icao = dst_val & 0x00FF_FFFF;
            let dst_type = (dst_val >> 24) & 0x07;

            let aircraft_opt = if src_type == 1 && is_valid_icao_aircraft(src_icao) {
                Some((src_icao, !src_on_ground))
            } else if dst_type == 1 && is_valid_icao_aircraft(dst_icao) {
                Some((dst_icao, true))
            } else {
                None
            };

            if let Some((icao, is_airborne)) = aircraft_opt {
                let country = resolve_icao_country(icao);
                let reg_prefix = resolve_icao_reg_prefix(icao);

                let payload_slice = if frame.len() > 10 { &frame[8..frame.len() - 2] } else { &[] };
                let payload_ascii: String = payload_slice
                    .iter()
                    .map(|&b| {
                        let b7 = b & 0x7F;
                        if (32..=126).contains(&b7) { b7 as char } else { ' ' }
                    })
                    .collect();

                let (lat, lon, alt, waypoint) = parse_acars_position(&payload_ascii);

                let mut callsign = String::new();
                let mut tail = String::new();
                let mut airport = None;

                for token in payload_ascii.split(|c: char| c.is_whitespace() || c == '/' || c == ',') {
                    let t = token.trim();
                    if is_valid_callsign(t) {
                        callsign = t.to_string();
                    } else if is_valid_registration(t) {
                        tail = t.trim_start_matches('.').to_string();
                    } else if t.len() == 4 && t.starts_with('E') && t.chars().all(|c| c.is_ascii_uppercase()) {
                        if let Some(apt) = resolve_airport(t) {
                            airport = Some(format!("{} ({})", t, apt));
                        }
                    }
                }

                let mut aircraft_type = None;
                if let Some((known_reg, _known_airl, known_model)) = resolve_icao_aircraft_info(icao) {
                    if tail.is_empty() && !known_reg.is_empty() {
                        tail = known_reg.to_string();
                    }
                    aircraft_type = Some(known_model.to_string());
                }

                // Strictly require either a passed FCS check or verified flight identifiers
                let is_verified = fcs_valid
                    || !callsign.is_empty()
                    || !tail.is_empty()
                    || (lat.is_some() && lon.is_some());

                if is_verified {
                    if callsign.is_empty() {
                        if !tail.is_empty() {
                            callsign = tail.clone();
                        } else {
                            callsign = format!("ICAO:{:06X}", icao);
                        }
                    }
                    if tail.is_empty() && !reg_prefix.is_empty() {
                        tail = format!("{}[{:06X}]", reg_prefix, icao);
                    }

                    let flight_state = if is_airborne { "Airborne" } else { "On Ground" };
                    let mut airline = resolve_airline(&callsign).or_else(|| resolve_airline(&tail)).map(|s| s.to_string());
                    if airline.is_none() {
                        if let Some((_, known_airl, _)) = resolve_icao_aircraft_info(icao) {
                            airline = Some(known_airl.to_string());
                        }
                    }
                    let msg_type = if lat.is_some() && lon.is_some() {
                        "Position Report".to_string()
                    } else if airport.is_some() {
                        "Airport Link".to_string()
                    } else {
                        "Aviation Data Link".to_string()
                    };

                    let summary = {
                        let airl = airline.as_deref().unwrap_or(country);
                        let id_str = if !tail.is_empty() && tail != callsign { format!("flight {} ({})", callsign, tail) } else { format!("flight {}", callsign) };
                        let type_str = aircraft_type.as_deref().map(|t| format!(" • {}", t)).unwrap_or_default();
                        let mut parts = vec![format!("{} {}{}: {} [{}]", airl, id_str, type_str, msg_type, flight_state)];
                        if let Some(apt) = &airport {
                            parts.push(format!("Station: {}", apt));
                        }
                        if let Some(a) = alt {
                            parts.push(format!("Alt: {} ft (FL{})", a, a / 100));
                        }
                        if let Some(wp) = &waypoint {
                            parts.push(format!("Waypoint: {}", wp));
                        }
                        if let (Some(la), Some(lo)) = (lat, lon) {
                            parts.push(format!("Position: {:.4}°N, {:.4}°E", la, lo));
                        }
                        parts.join(" | ")
                    };

                    let now_str = {
                        let now = std::time::SystemTime::now();
                        let dur = now.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap_or_default();
                        let secs = dur.as_secs();
                        let hours = (secs / 3600) % 24;
                        let mins = (secs / 60) % 60;
                        let s = secs % 60;
                        format!("{:02}:{:02}:{:02}", hours, mins, s)
                    };

                    let pkt = AirPacket {
                        freq_hz: self.active_freq_hz,
                        timestamp: now_str,
                        callsign,
                        tail,
                        airline,
                        aircraft_type,
                        origin: airport,
                        destination: None,
                        route: None,
                        altitude_ft: alt,
                        speed_kts: None,
                        heading_deg: None,
                        latitude: lat,
                        longitude: lon,
                        waypoint,
                        message_type: msg_type,
                        human_summary: summary,
                        raw_text: format!("VDL2 AVLC ICAO:{:06X} ({}) {}", icao, country, payload_ascii.trim()),
                        signal_snr_db: snr_db,
                    };

                    self.ingest_packet(pkt);
                    found_packet = true;
                    break;
                }
            }
        }

        // Secondary fallback pass: scan ASCII representations
        if !found_packet {
            let s_direct: String = direct_octets
                .iter()
                .map(|&b| {
                    let b7 = b & 0x7F;
                    if (32..=126).contains(&b7) { b7 as char } else { ' ' }
                })
                .collect();
            if let Some(packet) = parse_air_message(&s_direct, self.active_freq_hz, snr_db) {
                self.ingest_packet(packet);
                found_packet = true;
            }
        }

        // If burst was verified RF data burst, emit burst event for live stream
        if !found_packet {
            let now_str = {
                let now = std::time::SystemTime::now();
                let dur = now.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap_or_default();
                let secs = dur.as_secs();
                let hours = (secs / 3600) % 24;
                let mins = (secs / 60) % 60;
                let s = secs % 60;
                format!("{:02}:{:02}:{:02}", hours, mins, s)
            };
            let burst_pkt = AirPacket {
                freq_hz: self.active_freq_hz,
                timestamp: now_str,
                callsign: "VDL2-BURST".to_string(),
                tail: String::new(),
                airline: None,
                aircraft_type: None,
                origin: None,
                destination: None,
                route: None,
                altitude_ft: None,
                speed_kts: None,
                heading_deg: None,
                latitude: None,
                longitude: None,
                waypoint: None,
                message_type: "RF Data Burst".to_string(),
                human_summary: format!("VDL Mode 2 RF data burst detected: duration {:.0} ms | SNR {:.1} dB", duration_ms, snr_db),
                raw_text: format!("VDL2 BURST dur={:.0}ms snr={:.1}dB", duration_ms, snr_db),
                signal_snr_db: snr_db,
            };
            self.ingest_packet(burst_pkt);
        }
    }

    /// High-performance ACARS MSK (1200 / 2400 Hz) audio subcarrier demodulator and frame decoder
    fn process_msk(&mut self, samples: &[f32], snr_db: f32) {
        if samples.is_empty() {
            return;
        }

        // Optimal 20-sample delay at 48 kHz: yields +/-0.5 product swing between 1200 Hz and 2400 Hz
        let delay_len = 20;
        for &s in samples {
            let delayed = self.msk_delay_buf[self.msk_delay_idx];
            self.msk_delay_buf[self.msk_delay_idx] = s;
            self.msk_delay_idx = (self.msk_delay_idx + 1) % delay_len;

            // Delay and multiply product
            let prod = s * delayed;

            // Zero crossings for MSK discriminator
            if (self.prev_sample <= 0.0 && s > 0.0) || (self.prev_sample >= 0.0 && s < 0.0) {
                self.bit_buffer = (self.bit_buffer << 1) | 1;
            } else {
                self.bit_buffer <<= 1;
            }
            self.prev_sample = s;
            self.bits_count += 1;

            // Baud clock strobing (20 samples per symbol at 2400 baud)
            self.msk_sample_count += 1;
            if self.msk_sample_count >= 20 {
                self.msk_sample_count = 0;
                let bit = if prod < 0.0 { 1u8 } else { 0u8 };
                self.msk_cur_byte |= bit << self.msk_bit_pos;
                self.msk_bit_pos += 1;

                if self.msk_bit_pos == 8 {
                    let ch = self.msk_cur_byte & 0x7F; // 7-bit ASCII
                    self.msk_cur_byte = 0;
                    self.msk_bit_pos = 0;

                    if ch == 0x16 || ch == 0x2B {
                        // Sync preamble detected
                        self.msk_in_frame = true;
                        self.char_buffer.clear();
                    } else if self.msk_in_frame {
                        if (32..=126).contains(&ch) || ch == 0x01 || ch == 0x02 || ch == 0x03 {
                            self.char_buffer.push(ch);
                            if ch == 0x03 || self.char_buffer.len() >= 220 {
                                // ETX or max length: finalize packet
                                self.msk_in_frame = false;
                                let msg: String = self.char_buffer.iter().map(|&c| c as char).collect();
                                if let Some(pkt) = parse_air_message(&msg, self.active_freq_hz, snr_db) {
                                    self.ingest_packet(pkt);
                                }
                                self.char_buffer.clear();
                            }
                        } else if self.char_buffer.len() >= 12 {
                            self.msk_in_frame = false;
                            let msg: String = self.char_buffer.iter().map(|&c| c as char).collect();
                            if let Some(pkt) = parse_air_message(&msg, self.active_freq_hz, snr_db) {
                                self.ingest_packet(pkt);
                            }
                            self.char_buffer.clear();
                        } else {
                            self.msk_in_frame = false;
                            self.char_buffer.clear();
                        }
                    }
                }
            }
        }
    }

    /// Ingests a validated ACARS or VDL2 packet into tracking records
    pub fn ingest_packet(&mut self, packet: AirPacket) {
        if packet.callsign.is_empty() {
            return;
        }

        // Push to packet history log
        self.packets.push(packet.clone());
        if self.packets.len() > 500 {
            self.packets.remove(0);
        }

        // 1-Hour Timeout: Evict any aircraft not heard within the last 3600 seconds
        let now = Instant::now();
        self.saved_stations.retain(|_, st| now.duration_since(st.last_seen).as_secs() <= 3600);

        // Raw RF bursts bypass station tracking
        if packet.callsign == "VDL2-BURST" {
            return;
        }

        // Only register into saved_stations if it represents a verified aircraft:
        let is_verified_plane = is_valid_callsign(&packet.callsign)
            || is_valid_registration(&packet.tail)
            || (packet.latitude.is_some() && packet.longitude.is_some())
            || (!packet.callsign.starts_with("ICAO:") && packet.airline.is_some())
            || packet.message_type.contains("ADS-B");

        if !is_verified_plane {
            return;
        }

        let key = if !packet.tail.is_empty() {
            packet.tail.clone()
        } else {
            packet.callsign.clone()
        };

        if let Some(station) = self.saved_stations.get_mut(&key) {
            station.last_seen = now;
            station.last_timestamp = packet.timestamp.clone();
            station.packet_count += 1;
            station.latest_message = packet.raw_text.clone();
            station.latest_summary = packet.human_summary.clone();
            if packet.altitude_ft.is_some() {
                station.altitude_ft = packet.altitude_ft;
            }
            if packet.speed_kts.is_some() {
                station.speed_kts = packet.speed_kts;
            }
            if packet.heading_deg.is_some() {
                station.heading_deg = packet.heading_deg;
            }
            if let (Some(lat), Some(lon)) = (packet.latitude, packet.longitude) {
                station.latitude = Some(lat);
                station.longitude = Some(lon);
                station.track_history.push((lat, lon));
                if station.track_history.len() > 200 {
                    station.track_history.remove(0);
                }
            }
            if packet.origin.is_some() {
                station.origin = packet.origin.clone();
            }
            if packet.aircraft_type.is_some() {
                station.aircraft_type = packet.aircraft_type.clone();
            }
            if packet.destination.is_some() {
                station.destination = packet.destination.clone();
            }
            if packet.route.is_some() {
                station.route = packet.route.clone();
            }
            if packet.waypoint.is_some() {
                station.last_waypoint = packet.waypoint.clone();
            }
        } else {
            let mut track_history = Vec::new();
            if let (Some(lat), Some(lon)) = (packet.latitude, packet.longitude) {
                track_history.push((lat, lon));
            }
            self.saved_stations.insert(
                key,
                AirStation {
                    tail: packet.tail.clone(),
                    callsign: packet.callsign.clone(),
                    airline: packet.airline.clone(),
                    aircraft_type: packet.aircraft_type.clone(),
                    route: packet.route.clone(),
                    origin: packet.origin.clone(),
                    destination: packet.destination.clone(),
                    altitude_ft: packet.altitude_ft,
                    speed_kts: packet.speed_kts,
                    heading_deg: packet.heading_deg,
                    latitude: packet.latitude,
                    longitude: packet.longitude,
                    last_waypoint: packet.waypoint.clone(),
                    last_seen: Instant::now(),
                    last_timestamp: packet.timestamp.clone(),
                    packet_count: 1,
                    latest_message: packet.raw_text.clone(),
                    latest_summary: packet.human_summary.clone(),
                    track_history,
                },
            );
        }
    }

    pub fn clear(&mut self) {
        self.packets.clear();
        self.saved_stations.clear();
    }
}

/// Converts a decoded Mode S ADS-B message into a unified AirPacket with explicit frequency
pub fn convert_adsb_to_air_packet_with_freq(m: &crate::dsp::AdsbMessage, snr_db: f32, freq_hz: u64) -> AirPacket {
    let country = resolve_icao_country(m.icao);
    let reg_prefix = resolve_icao_reg_prefix(m.icao);
    let mut tail = format!("{}{:06X}", reg_prefix, m.icao);
    let mut aircraft_type = None;
    let mut airline = None;

    if let Some((known_reg, known_airl, known_model)) = resolve_icao_aircraft_info(m.icao) {
        if !known_reg.is_empty() {
            tail = known_reg.to_string();
        }
        airline = Some(known_airl.to_string());
        aircraft_type = Some(known_model.to_string());
    }

    let callsign = if let Some(ref cs) = m.callsign {
        cs.clone()
    } else if !tail.is_empty() {
        tail.clone()
    } else {
        format!("ICAO:{:06X}", m.icao)
    };

    if airline.is_none() {
        airline = resolve_airline(&callsign).map(|s| s.to_string());
    }

    let now_str = {
        let now = std::time::SystemTime::now();
        let dur = now.duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap_or_default();
        let secs = dur.as_secs();
        let hours = (secs / 3600) % 24;
        let mins = (secs / 60) % 60;
        let s = secs % 60;
        format!("{:02}:{:02}:{:02}", hours, mins, s)
    };

    let summary = {
        let airl = airline.as_deref().unwrap_or(country);
        let id_str = if !tail.is_empty() && tail != callsign { format!("flight {} ({})", callsign, tail) } else { format!("flight {}", callsign) };
        let type_str = aircraft_type.as_deref().map(|t| format!(" • {}", t)).unwrap_or_default();
        let mut parts = vec![format!("{} {}{}: ADS-B 1090 MHz", airl, id_str, type_str)];
        if let Some(a) = m.altitude_ft {
            parts.push(format!("Alt: {} ft (FL{})", a, a / 100));
        }
        if let Some(spd) = m.speed_kts {
            parts.push(format!("Speed: {} kt", spd));
        }
        if let Some(hdg) = m.heading_deg {
            parts.push(format!("Hdg: {:.0}°", hdg));
        }
        if let (Some(la), Some(lo)) = (m.latitude, m.longitude) {
            parts.push(format!("GPS: {:.4}°N, {:.4}°E", la, lo));
        }
        parts.join(" | ")
    };

    AirPacket {
        freq_hz,
        timestamp: now_str,
        callsign,
        tail,
        airline,
        aircraft_type,
        origin: None,
        destination: None,
        route: None,
        altitude_ft: m.altitude_ft,
        speed_kts: m.speed_kts,
        heading_deg: m.heading_deg,
        latitude: m.latitude,
        longitude: m.longitude,
        waypoint: None,
        message_type: "ADS-B Mode S".to_string(),
        human_summary: summary,
        raw_text: format!("ADS-B 1090 ICAO:{:06X} ({})", m.icao, country),
        signal_snr_db: snr_db,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_acars_position_report() {
        let msg = "POSN N60189E024582,350,1205,RUMAX,1218,KUVEN";
        let (lat, lon, alt, wp) = parse_acars_position(msg);
        assert!(lat.is_some(), "Latitude must be parsed");
        assert!(lon.is_some(), "Longitude must be parsed");
        assert_eq!(alt, Some(35000), "FL350 must be 35000 ft");
        assert_eq!(wp, Some("RUMAX".to_string()));

        let lat_val = lat.unwrap();
        let lon_val = lon.unwrap();
        // ARINC 620 degrees + decimal minutes: N60°18.9' = 60.315°N, E024°58.2' = 24.970°E
        assert!((lat_val - 60.315).abs() < 0.01);
        assert!((lon_val - 24.970).abs() < 0.01);
    }

    #[test]
    fn test_parse_acars_dms_and_parity_handling() {
        // 6-digit DMS latitude (60° 18' 54" N) and 7-digit DMS longitude (024° 58' 12" E)
        let msg = "POSN 601854N 0245812E FL380";
        let (lat, lon, alt, _wp) = parse_acars_position(msg);
        assert!(lat.is_some());
        assert!(lon.is_some());
        assert_eq!(alt, Some(38000));
        let lat_v = lat.unwrap();
        let lon_v = lon.unwrap();
        assert!((lat_v - 60.315).abs() < 0.001);
        assert!((lon_v - 24.970).abs() < 0.001);

        // Test parity-encoded ASCII bytes stripping (bit 7 set)
        let raw_with_parity: [u8; 12] = [
            b'A' | 0x80, b'Y' | 0x80, b'1' | 0x80, b'2' | 0x80, b'3' | 0x80, b' ' | 0x80,
            b'E' | 0x80, b'F' | 0x80, b'H' | 0x80, b'K' | 0x80, b' ' | 0x80, b' ' | 0x80,
        ];
        let decoded: String = raw_with_parity
            .iter()
            .map(|&b| {
                let b7 = b & 0x7F;
                if (32..=126).contains(&b7) { b7 as char } else { ' ' }
            })
            .collect();
        assert_eq!(decoded.trim(), "AY123 EFHK");
    }

    #[test]
    fn test_parse_air_message_human_readable() {
        let msg = "/FN AY123 .OH-LWA POSN N60.317/E024.963 FL360 EFHK EGLL RUMAX";
        let pkt = parse_air_message(msg, 131_725_000, 18.5);
        assert!(pkt.is_some());
        let p = pkt.unwrap();
        assert_eq!(p.callsign, "AY123");
        assert_eq!(p.tail, "OH-LWA");
        assert_eq!(p.airline, Some("Finnair".to_string()));
        assert!(p.origin.as_deref().unwrap_or("").contains("Helsinki-Vantaa"));
        assert!(p.destination.as_deref().unwrap_or("").contains("London Heathrow"));
        assert_eq!(p.altitude_ft, Some(36000));
        assert!(p.human_summary.contains("Finnair"));
        assert!(p.human_summary.contains("Helsinki-Vantaa"));
    }

    #[test]
    fn test_resolve_airport_and_airline() {
        assert_eq!(resolve_airport("EFHK"), Some("Helsinki-Vantaa"));
        assert_eq!(resolve_airport("EGLL"), Some("London Heathrow"));
        assert_eq!(resolve_airline("FIN123"), Some("Finnair"));
        assert_eq!(resolve_airline("SAS789"), Some("SAS Scandinavian Airlines"));
        assert_eq!(resolve_airline("DLH456"), Some("Lufthansa"));
        assert_eq!(resolve_country("OH-LWA"), "Finland");
        assert_eq!(resolve_country("SE-DSA"), "Sweden");
    }

    #[test]
    fn test_vdl2_burst_tracking_and_no_phantom_flights() {
        let mut decoder = AirDecoder::new(48_000.0);
        decoder.set_freq(136_975_000);

        // Synthesize a 30 ms VDL2 burst (1440 samples at 48 kHz)
        let mut burst_iq = Vec::new();
        for i in 0..1440 {
            let phase = (i as f32 * 0.25).sin();
            burst_iq.push(Complex32::new(0.08 * phase, 0.08 * (phase + 1.0).cos()));
        }

        // Feed burst
        decoder.process_samples(&burst_iq, &[], 15.5);

        // Followed by silence to mark end of burst
        let silence = vec![Complex32::new(0.0001, 0.0001); 480];
        decoder.process_samples(&silence, &[], 1.0);

        assert!(decoder.vdl2_burst_count >= 1, "Must register RF burst count");
        assert_eq!(decoder.saved_stations.len(), 0, "Raw RF bursts must never pollute saved_stations as phantom aircraft");
    }

    #[test]
    fn test_noise_pseudo_callsigns_rejected() {
        assert!(parse_air_message("o8zH Position Report", 136_975_000, 20.0).is_none());
        assert!(parse_air_message("U6UI Position Report", 136_975_000, 20.0).is_none());
        assert!(parse_air_message("2ODD Position Report", 136_975_000, 20.0).is_none());
        assert!(parse_air_message("QEli Position Report", 136_975_000, 20.0).is_none());
        assert!(parse_air_message("8Nqy Position Report", 136_975_000, 20.0).is_none());
        assert!(parse_air_message("AYxy Position Report", 136_975_000, 20.0).is_none(), "Flight number must have digits");
        assert!(parse_air_message("random garbage text", 136_975_000, 20.0).is_none());
    }

    #[test]
    fn test_message_without_coordinates_not_labeled_position_report() {
        let msg = "AY123 .OH-LWA EFHK EGLL";
        let pkt = parse_air_message(msg, 131_725_000, 15.0).unwrap();
        assert_ne!(pkt.message_type, "Position Report", "Without coordinates, message must not be labeled Position Report");
        assert_eq!(pkt.latitude, None);
        assert_eq!(pkt.longitude, None);
    }

    #[test]
    fn test_parse_dlc_addr_and_country_lookup() {
        assert_eq!(resolve_icao_country(0x4601A2), "Finland");
        assert_eq!(resolve_icao_reg_prefix(0x4601A2), "OH-");
        assert_eq!(resolve_icao_country(0x4A8123), "Sweden");
        assert_eq!(resolve_icao_country(0x478456), "Norway");
        assert_eq!(resolve_icao_country(0x3C1234), "Germany");
        assert_eq!(resolve_icao_country(0x401234), "United Kingdom");

        // Verify parse_dlc_addr on an octet buffer
        let buf = [0xAA, 0x55, 0xAA, 0x54];
        let addr = parse_dlc_addr(&buf);
        assert!(addr > 0, "DLC address must be non-zero");
    }

    #[test]
    fn test_vdl2_burst_packet_stream() {
        let mut decoder = AirDecoder::new(48_000.0);
        decoder.set_freq(136_975_000);

        let burst_pkt = AirPacket {
            freq_hz: 136_975_000,
            timestamp: "17:30:00".to_string(),
            callsign: "VDL2-BURST".to_string(),
            tail: String::new(),
            airline: None,
            aircraft_type: None,
            origin: None,
            destination: None,
            route: None,
            altitude_ft: None,
            speed_kts: None,
            heading_deg: None,
            latitude: None,
            longitude: None,
            waypoint: None,
            message_type: "RF Data Burst".to_string(),
            human_summary: "VDL Mode 2 RF data burst detected".to_string(),
            raw_text: "VDL2 BURST".to_string(),
            signal_snr_db: 18.0,
        };
        decoder.ingest_packet(burst_pkt);
        assert_eq!(decoder.packets.len(), 1, "Burst must appear in packets for live log");
        assert_eq!(decoder.saved_stations.len(), 0, "Burst must not appear in saved_stations");
    }

    #[test]
    fn test_valid_and_invalid_icao_allocations() {
        // Genuine European civilian aircraft allocations
        assert!(is_valid_icao_aircraft(0x4601A2), "Finnish ICAO block must be valid");
        assert!(is_valid_icao_aircraft(0x4A8123), "Swedish ICAO block must be valid");
        assert!(is_valid_icao_aircraft(0x478456), "Norwegian ICAO block must be valid");
        assert!(is_valid_icao_aircraft(0x3C1234), "German ICAO block must be valid");
        assert!(is_valid_icao_aircraft(0x401234), "UK ICAO block must be valid");

        // Noise pseudo-ICAO addresses that appeared in user's burst log must be rejected!
        assert!(!is_valid_icao_aircraft(0x29FD05), "Noise address 29FD05 must be rejected");
        assert!(!is_valid_icao_aircraft(0x2EE9CB), "Noise address 2EE9CB must be rejected");
        assert!(!is_valid_icao_aircraft(0x7DDE85), "Noise address 7DDE85 must be rejected");
        assert!(!is_valid_icao_aircraft(0x000000), "Zero address must be rejected");
        assert!(!is_valid_icao_aircraft(0xFFFFFF), "Broadcast address must be rejected");
    }

    #[test]
    fn test_crc16_ccitt_and_avlc_fcs_verification() {
        let payload = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let crc = crc16_ccitt(&payload);

        // Standard HDLC / AVLC appends CRC (which is already inverted), LSB first
        let fcs_lsb = (crc & 0xFF) as u8;
        let fcs_msb = ((crc >> 8) & 0xFF) as u8;
        let mut frame = payload.to_vec();
        frame.push(fcs_lsb);
        frame.push(fcs_msb);

        assert!(verify_avlc_fcs(&frame), "Frame with correct FCS must pass verify_avlc_fcs");

        // Corrupted frame
        frame[3] ^= 0x01;
        assert!(!verify_avlc_fcs(&frame), "Corrupted frame must fail verify_avlc_fcs");
    }
}
