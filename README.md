# SpectraScanner

**SpectraScanner** is a high-speed, multi-VFO tactical Software Defined Radio (SDR) scanner and digital signal analysis platform written in Rust.

Featuring real-time parallel DDC (Digital Down-Conversion) architecture, SpectraScanner simultaneously tracks, demodulates, and monitors multiple signals across wide instantaneous bandwidths (up to 10 MHz on Airspy, 2.4 MHz on RTL-SDR). It integrates deep protocol decoders for aviation (ADS-B / VDL2), maritime (AIS), amateur radio (APRS), digital voice (DMR), and industrial SCADA radio modems onto interactive tactical maps and spectrum displays.

---

## Key Features

### ⚡ Parallel Multi-VFO Architecture
- **Simultaneous Reception**: Run multiple independent VFOs (1 to 10 concurrent channels) within the SDR capture window.
- **Independent Demodulators**: Each active VFO features its own DDC, channel filter, squelch, AGC, audio router, and digital decoder.
- **Fast Sweeping & Scanning**: High-rate scanning engine with configurable step sizes, automatic noise floor tracking, and hit recording.

### 📻 Supported Modulation Modes
- **AM**: Airband voice communications with fast audio AGC.
- **NFM**: Narrowband FM (12.5 kHz / 25 kHz) for marine, PMR446, commercial business radio, and amateur radio.
- **WFM & BCFM**: Broadcast Wideband FM with 50µs / 75µs de-emphasis filtering and high-fidelity downsampling.
- **USB & LSB**: Single Sideband (SSB) with Hilbert transform analytic filtering.

### 🛰️ Integrated Protocol Decoders & Tactical Maps
- **✈️ Air Traffic (ADS-B & VDL2 / ACARS)**:
  - ADS-B Mode-S (1090 MHz) & VDL Mode 2 (136.975 MHz) receiver.
  - 10 MSPS pulse demodulation with CPR local/global coordinate decoding, altitude, velocity, and ICAO database resolution.
  - Interactive radar map with snail-tail flight tracks, callsigns, and distance/bearing from your home QTH.
- **🚢 Marine Traffic (AIS)**:
  - Dual-frequency simultaneous monitoring on AIS 1 (161.975 MHz) and AIS 2 (162.025 MHz).
  - High-precision GMSK 9600 baud demodulation with 32-bit DPLL clock recovery, NRZI decoding, and HDLC bit-unstuffing.
  - Decodes Class A and Class B position reports, MMSI country resolution, vessel types, speed, course, and navigational status.
- **📡 APRS (Automatic Packet Reporting System - 144.800 MHz)**:
  - Bell 202 1200 baud AFSK demodulation and AX.25 frame extraction.
  - Tactical 2D GPS station map, path digipeater analysis, and station telemetry logs.
- **📻 DMR (Digital Mobile Radio / MOTOTRBO)**:
  - 2-slot TDMA 4FSK 9600 baud demodulator with CACH sync and burst frame timing.
  - Decodes talkgroup IDs, caller IDs, and color codes.
  - Integrated open-source AMBE+2 vocoder synthesis (`mbelib`) for digital voice decoding.
- **⚙️ SCADA & Radio Modem Inspector**:
  - Telemetry parser for SATEL 3AS radio modems (9600/19200 baud) with Modbus RTU payload inspection.
  - Wireless M-Bus (EN 13757-4 Mode T/C) smart metering telegrams.
  - POCSAG pager message decoding.
  - Sub-GHz ISM decoders (TPMS tire sensors, Nexus weather stations, EV1527 remotes).

### 🖥️ Tactical User Interface
- **Dynamic Pseudospectrum**: Proprietary noise floor subtraction algorithm that highlights signals buried near the noise floor.
- **Offline-Capable Slippy Map**: High-performance raster map supporting Tactical Dark, OpenStreetMap, ESRI World Imagery, and CartoDB Dark Matter.
- **Multi-Monitor DPI-Awareness**: Custom Windows manifest and system DPI locking to prevent scaling jitter across multi-monitor setups.

---

## Supported Hardware

- **Airspy R2 / Airspy Mini**: 10 MSPS or 2.5 MSPS high-dynamic-range sampling.
- **RTL-SDR (v3, v4, Generic RTL2832U)**: 2.4 MSPS sampling.
- **Auto-Detection**: Dynamic runtime library loader (`lib/` or system PATH).

---

## Building from Source

### Prerequisites
- **Rust Toolchain**: Stable Rust (2021 edition).
- **C Compiler (Windows)**: GCC (e.g. from `w64devkit` or MSYS2 MinGW-w64) for compiling embedded C decoders (`mbelib`).

### Compilation
Clone the repository and build using Cargo:

```bash
git clone https://github.com/jorksx/SpectraScanner.git
cd SpectraScanner
cargo build --release
```

The compiled executable will be located at:
```
target/release/spectrascanner.exe
```

---

## Quick Start on Windows

1. Ensure the required SDR driver DLLs (`airspy.dll`, `rtlsdr.dll`, `libusb-1.0.dll`, `pthreadVC3.dll`) are present in `lib/` (included in repository).
2. Run the application:
   ```cmd
   run.bat
   ```
   Or launch `spectrascanner.exe` directly.
3. Select your SDR hardware from the top device selector, set your desired frequency or choose a preset band (AIR, HAM 2m, Marine, Broadcast FM, etc.), and hit **START**.
4. Click on signals in the waterfall to tune VFOs or enable the scanner to search for activity automatically.

---

## License

This project is licensed under the MIT License - see the repository for details.
External components:
- `mbelib` (C) Copyright (c) 2010 by mbelib authors.
