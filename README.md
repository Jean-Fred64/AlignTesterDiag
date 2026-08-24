# 💾 AlignTesterDiag 🛠️

[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-v0.2.0--alpha-blue.svg)]()
[![TUI](https://img.shields.io/badge/ui-Ratatui%20%2B%20Crossterm-blue.svg)](https://ratatui.rs)
[![Hardware](https://img.shields.io/badge/hardware-Greaseweazle%20v4-green.svg)](https://github.com/keirf/greaseweazle)
[![Architecture](https://img.shields.io/badge/concurrency-100%25%20Non--Blocking-brightgreen.svg)]()
[![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE)

**AlignTesterDiag** is a high-performance, non-blocking, modern terminal user interface (TUI) diagnostics and calibration suite for floppy disk drives. Interfacing directly with a **Greaseweazle USB flux controller**, it bridges Dave Dunfield's classic **ImageDisk (`IMD.C`)** diagnostic methodologies with modern sub-microsecond magnetic flux DPLL decoding and real-time audio-visual feedback.

---

## 🌟 Key Highlights

- ⚡ **100% Non-Blocking Architecture (~60 Hz TUI):** Strict decoupling between the interactive Ratatui/Crossterm rendering loop, the USB hardware acquisition worker thread, and a dedicated real-time audio thread via lock-free `crossbeam-channel` queues. 64 KB extended serial block buffers eliminate micro-blocking system calls and USB CDC latency. 171 automated unit tests with strict Clippy compliance (`cargo clippy -- -D warnings`).
- 💾 **High-Precision Low-Level Format Engine & MFM Synthesizer (`CMD_WRITE_FLUX`):**
  - **Ultra-Fast Formatter Pipeline (~35s Fast / ~70s Full Verify):** Direct index-synchronized flux emission with optional 1-revolution instant DPLL read-after-write verification per track (toggled with <kbd>V</kbd> in format modal).
  - **Custom Track Range (`TrackRange`) & Full Disk:** Flexible format targeting via Single Track (<kbd>T</kbd>), Custom Range (<kbd>R</kbd>), or Full Disk (<kbd>D</kbd>).
  - **Integrated Modal Navigation:** Live track stepping (<kbd>+</kbd>/<kbd>-</kbd>, <kbd>[</kbd>/<kbd>]</kbd>, <kbd>←</kbd>/<kbd>→</kbd>, or Mouse Wheel `ScrollUp`/`ScrollDown`), head toggle (<kbd>H</kbd>), and total cylinder count adjustment (<kbd>PgUp</kbd>/<kbd>PgDn</kbd> or <kbd>↑</kbd>/<kbd>↓</kbd>).
  - **Interactive Range Editor (`RangeEditModal`):** Dual-bound modal editor with exclusive <kbd>Tab</kbd> field switching (`Start` / `End`), numeric typing (<kbd>0</kbd>–<kbd>9</kbd>), incremental adjustment (<kbd>+</kbd>/<kbd>-</kbd>, <kbd>↑</kbd>/<kbd>↓</kbd>, Mouse Wheel), and strict validation (`start <= end < max_tracks`).
  - **Explicit Confirmation Safety Lock (`[y/N]`):** All destructive operations require explicit user confirmation (<kbd>Y</kbd> to confirm, <kbd>N</kbd> / <kbd>Enter</kbd> / <kbd>Esc</kbd> to cancel, with `N` as default).
  - **72 MHz Pulse Timing:** Translates synthesized MFM bitstreams into cycle-accurate 72 MHz interval ticks ($2T, 3T, 4T$).
  - **Write Pre-Compensation:** Applies $\pm 125\text{ ns}$ ($\approx 9$ ticks @ 72 MHz) timing shifts on inner tracks ($> 40$) to counteract magnetic peak shift.
  - **Hardware Index Cueing:** Synchronizes flux emission to the physical index pulse (`cue_at_index = 1`) with controlled splice overlap.
  - **Read-After-Write Verification:** Automatically reads back each formatted track, verifies 100% expected sector presence, 0 CRC errors, and $Q \ge 85\%$ quality with up to 2 automatic retries.
  - **Dynamic Track Override:** Interactive track adjustment supporting standard 40/80 tracks up to 42/84 tracks with physical cylinder tracking.
  - **Write-Protect Guard:** Hardware Pin 28 (`WPROT`) validation before any carriage movement or flux emission.
- 🧲 **Hardware Low-Level DC Erase Engine (`CMD_ERASE_FLUX`):**
  - **Continuous Magnetic Neutralization:** Asserts raw continuous write gate without flux transitions for $\ge 1.1$ revolutions after index pulse to permanently wipe and degauss magnetic tracks.
  - **Interactive Erase Modal (<kbd>E</kbd>):** Single-track (<kbd>T</kbd>), custom track range (<kbd>R</kbd>), or full-disk dual-head (<kbd>D</kbd>) DC erasure with interactive track count bounds (40/42 for 48 TPI, 80/84 for 96/135 TPI).
  - **Integrated Navigation & Range Editing:** Track adjustment (<kbd>+</kbd>/<kbd>-</kbd>, <kbd>[</kbd>/<kbd>]</kbd>, arrows, mouse wheel), head toggle (<kbd>H</kbd>), and custom range editor (<kbd>R</kbd>).
  - **Explicit Confirmation Safety Lock (`[y/N]`):** Prompts for explicit confirmation before activating the erase gate (<kbd>Y</kbd> to proceed, <kbd>N</kbd> / <kbd>Enter</kbd> / <kbd>Esc</kbd> to cancel).
  - **Hardware Write-Protect Safety:** Proactively queries Pin 28 before seeking or activating write gate.
- 🕹️ **Multi-System Retro Platform Support (IBM PC, Amiga, Atari ST, Amstrad CPC):**
  - **Amiga (Paula MFM):** Décodage bit-level *even/odd*, checksums 32-bit XOR, 11 secteurs/piste en DD (880 Ko) et 22 secteurs en HD (1.76 Mo).
  - **Atari ST (WD1772):** Support des formats étendus 9, 10 et 11 secteurs (jusqu'à 880 Ko) et pistes 80 à 82.
  - **Amstrad CPC (µPD765 / 3"):** Formats DATA (`0xC1..0xC9`), SYSTEM (`0x41..0x49`) et CPM.
- 🎯 **Real-Time Mechanical Alignment Radar:** Continuous on-track vs. off-track sector validation with live percentage gauge calculation, detecting misaligned head carriages, tracking errors, and invalid track IDs.
- 🔊 **Dynamic Pitch Acoustic Variometer:** Auditory feedback inspired by glider variometers. Continuously modulates multi-tier audio frequency (**1500 Hz – 2200 Hz** for nominal alignment $\ge 95\%$, **600 Hz – 1400 Hz** for marginal tracking $70\text{--}94\%$, **250 Hz – 500 Hz** continuous tone for severe misalignment $< 70\%$), with instantaneous **180 Hz pulsed warning buzz** on cross-track mismatch and **150 Hz hum** on zero decoded sectors.
- ⏱️ **72 MHz Spindle Tachometer & Live RPM Jitter Gauge:** Sub-microsecond revolution interval timing extracted directly from hardware index pulses. Computes instant RPM, 10-revolution rolling average, peak-to-peak jitter ($\Delta\text{RPM}$, $\pm\Delta\%$), and displays a 21-character visual centering gauge.
- 🔄 **Dual-Head ("Both" Mode) Acquisition:** Alternating Head 0 & Head 1 acquisition with automated cross-track divergence detection ($T_{\text{H0}} \ne T_{\text{H1}}$) and consolidated dual-head health scoring.
- 🔌 **Universal Drive Support (26-Pin FFC & 34-Pin Shugart/PC):** Motor-gated seek with 15 ms electronic wake-up delay (`STEPPER_WAKEUP_DELAY_MS`) to eliminate stepper motor driver lockups on slim laptop mechanisms (e.g. TEAC FD-05HG). Dynamic drive unit selection (`A:`/`B:` for IBM PC, `DS0`..`DS3` for Shugart).
- 📊 **Standard & Verbose Live Stream Views:** Color-coded segmented ribbon blocks representing sector-by-sector IDAM/DAM integrity, Gap0 timing in microseconds, phosphor decay animation, and interleave ratio.
- 🛑 **Panic Reset & Safe State Protection:** Instant motor power cut, flux buffer flush, and Greaseweazle bus re-initialization on `Backspace` or `Esc`.

---

## 📸 Interface Preview

![AlignTesterDiag Diagnostic Screen](Medias/AlignTesterDiag_screenshot_beta%200.2.jpg)

*Live track stepping, density autodetection, spindle RPM monitoring, and continuous MFM sector integrity verification in real-time.*

---

## 🚀 Quick Start

### Option A: Pre-compiled Windows Binary (Recommended for Users)

1. Download the latest `aligntester-diag-windows-x64.zip` from the **[Releases](../../releases)** tab.
2. Extract the archive into any local folder.
3. Connect your **Greaseweazle v4 / v4.1** and floppy drive via USB.
4. **Launch via Terminal (Important):** Run the executable inside a terminal window (**Windows Terminal**, **PowerShell**, or **Command Prompt**) rather than double-clicking it from the file explorer. This ensures proper ANSI color rendering, full-size TUI layout, and readable error logs if a COM port issue occurs.

```powershell
# Navigate to your extracted folder and run (auto-detects the Greaseweazle port):
.\aligntester-diag.exe

# Or specify the COM port and Drive unit manually (IBM PC mode, Drive A:):
.\aligntester-diag.exe COM3 --drive 0

# Or launch directly in Shugart bus mode for native Amiga / Atari / Commodore drives:
.\aligntester-diag.exe COM3 --shugart --drive 0
```

### Option B: Build from Source (Developers & Linux / macOS)

### 1. Prerequisites
- **Rust Toolchain:** Stable Rust (2021 edition or newer) & Cargo ([rustup.rs](https://rustup.rs)).
- **Hardware:** Greaseweazle v4 (or compatible v4.1 hardware) connected via USB to a 3.5" or 5.25" floppy drive.
- **Reference Diskette:** A known-good, formatted 3.5" (720K DD / 1.44M HD) or 5.25" (360K DD / 1.2M HD) floppy diskette.

### 2. Build from Source
```bash
git clone https://github.com/your-username/AlignTesterDiag.git
cd AlignTesterDiag
cargo build --release
```

### 3. Execution
Auto-detect connected Greaseweazle device on Drive 0:
```bash
cargo run --release
```

Specify COM port, Drive Unit, and Bus Type explicitly:
```bash
# Windows (IBM PC mode, Drive A:)
cargo run --release -- COM3 --drive 0

# Linux / macOS (IBM PC mode, Drive B:)
cargo run --release -- /dev/ttyACM0 --drive 1

# Native Amiga / Shugart Drive (DS0..DS3)
cargo run --release -- --shugart --drive 0
```

---

## ⌨️ Interactive Keybindings Quick Reference

### Main Screen Commands

| Key | Action | Description |
|:---:|:---|:---|
| <kbd>?</kbd> / <kbd>F1</kbd> | **Help Modal** | Open interactive help modal overlay |
| <kbd>A</kbd> | **Analyze** | Start continuous real-time track alignment analysis |
| <kbd>D</kbd> | **Read Data** | Read and test sector CRC integrity across the cylinder |
| <kbd>E</kbd> | **Erase Modal** | Open Low-Level Hardware DC Erase Modal (`[T]` Track, `[R]` Range, `[D]` Disk, `[Esc]` Cancel) |
| <kbd>F</kbd> | **Format Modal** | Open Low-Level Track & Disk Formatter Modal (`[T]` Track, `[R]` Range, `[D]` Disk, `[V]` Verify, `[Esc]` Cancel) |
| <kbd>L</kbd> | **Live RPM** | Live RPM tachometer & jitter stability test |
| <kbd>P</kbd> | **Preset (Hardware & Format)** | Cycle unified hardware & format presets (`Pc35Hd` ➔ `Pc35Dd` ➔ `Pc525Hd` ➔ `Pc525DdOnHd` ➔ `Pc525Dd` ➔ `Amiga35Dd` ➔ `Atari35Dd` ➔ `Cpc30Data`), atomically configuring bus, stepping rate, DPLL clock & nominal bitrate |
| <kbd>S</kbd> | **Toggle Step Rate** | Toggle Single (1:1) / Double (2:1) step mode (48/96 TPI, for reading 40-track media on 80-track drives) |
| <kbd>T</kbd> | **Toggle Bus Type** | Switch interface bus mode: `IBM PC (0x01)` ➔ `Shugart (0x02)` (auto-resets unit to 0 when entering PC mode from DS2/DS3) |
| <kbd>B</kbd> | **Beep (Audio Radar)** | Toggle acoustic pitch variometer on / off |
| <kbd>H</kbd> | **Toggle Head** | Cycle head selection: `Head 0` ➔ `Head 1` ➔ `BOTH (0+1)` |
| <kbd>U</kbd> | **Toggle Drive Unit** | Switch active drive: `Drive 0 (A:)` ➔ `Drive 1 (B:)` in IBM PC mode, or cycle `Unit 0 (DS0)` ➔ `Unit 1 (DS1)` ➔ `Unit 2 (DS2)` ➔ `Unit 3 (DS3)` in Shugart mode |
| <kbd>M</kbd> | **Toggle Motor** | Manually assert / negate spindle motor power line |
| <kbd>V</kbd> | **Toggle Verbose** | Switch between Standard and Detailed Verbose Stream views |
| <kbd>R</kbd> | **Recalibrate** | Recalibrate carriage to Track 0 and return to current track |
| <kbd>Z</kbd> | **Zero Track** | Direct single seek step return to Track 0 |
| <kbd>+</kbd> / <kbd>→</kbd> / <kbd>▲</kbd> / `ScrollUp` | **Step Forward (+1)** | Step carriage forward by +1 track (up to Track 83) |
| <kbd>-</kbd> / <kbd>←</kbd> / <kbd>▼</kbd> / `ScrollDown` | **Step Backward (-1)** | Step carriage backward by -1 track (down to Track 0) |
| <kbd>0</kbd>–<kbd>8</kbd> | **Direct Decade Seek** | Direct seek jump to decade tracks (0, 10, 20 ... 80) |
| <kbd>9</kbd> | **Overtrack Seek** | Direct seek jump to physical limit (Track 83) |
| <kbd>Esc</kbd> | **Stop / Cancel** | Stop spindle motor, flush pending buffers, dismiss active modal |
| <kbd>Backspace</kbd> | **Panic Reset** | Emergency instant motor cut and hardware re-initialization |
| <kbd>Q</kbd> / <kbd>X</kbd> / <kbd>Ctrl+C</kbd> | **Exit** | Clean shutdown (cuts spindle motor, disables LEDs, exits raw mode) |

### Modal & Range Navigation Controls

| Context | Controls | Function |
|:---|:---|:---|
| **Format / Erase Modals** | <kbd>T</kbd> | Execute operation on **Current Track** only (Head 0 or Head 1) |
| | <kbd>R</kbd> | Open **Custom Track Range Editor** (`RangeEditModal`) |
| | <kbd>D</kbd> | Execute operation on **Entire Disk** (Dual-Head, 00..max) |
| | <kbd>H</kbd> | Switch physical target head (`Head 0` $\leftrightarrow$ `Head 1`) |
| | <kbd>+</kbd> / <kbd>-</kbd>, <kbd>[</kbd> / <kbd>]</kbd>, <kbd>←</kbd> / <kbd>→</kbd>, `Scroll` | Step target cylinder for single-track operations |
| | <kbd>PgUp</kbd> / <kbd>PgDn</kbd>, <kbd>↑</kbd> / <kbd>↓</kbd> | Adjust total disk tracks (40/42 for 48 TPI, 80/84 for 96/135 TPI) |
| | <kbd>V</kbd> *(Format only)* | Toggle **Read-After-Write Verify** (`ON` ~70s / `OFF` ~35s) |
| | <kbd>Esc</kbd> / <kbd>Q</kbd> / <kbd>X</kbd> | Close modal and cancel |
| **Range Editor (`RangeEditModal`)** | <kbd>Tab</kbd> | Switch active input field (`Start Track` $\leftrightarrow$ `End Track`) |
| | <kbd>0</kbd>–<kbd>9</kbd> | Type track number directly |
| | <kbd>+</kbd> / <kbd>-</kbd>, <kbd>↑</kbd> / <kbd>↓</kbd>, <kbd>←</kbd> / <kbd>→</kbd>, `Scroll` | Increment / decrement active bound |
| | <kbd>Backspace</kbd> | Clear / backspace active field digits |
| | <kbd>Enter</kbd> | Validate track range and arm execution |
| | <kbd>Esc</kbd> | Cancel range edit and return to parent modal |
| **Confirmation Prompt (`[y/N]`)** | <kbd>Y</kbd> / <kbd>y</kbd> | **Confirm & Execute** destructive format/erase action |
| | <kbd>N</kbd> / <kbd>n</kbd>, <kbd>Enter</kbd>, <kbd>Esc</kbd> | **Abort** confirmation and return to modal (Safe default) |

---

## 💻 CLI Options & Syntax

```text
aligntester-diag [PORT] [OPTIONS]

ARGUMENTS:
  [PORT]                  Serial port name (e.g. COM3, /dev/ttyACM0). Auto-detected if omitted.

OPTIONS:
  -p, --preset <preset>      Select hardware & format preset:
                             - pc35hd: 3.5" HD (1.44M, PC Bus, Step 1:1, DPLL 500 kbps @ 300 RPM)
                             - pc35dd: 3.5" DD (720K, PC Bus, Step 1:1, DPLL 250 kbps @ 300 RPM)
                             - pc525hd: 5.25" HD (1.2M, PC Bus, Step 1:1, DPLL 500 kbps @ 360 RPM)
                             - pc525ddonhd: 5.25" DD on HD Drive (360K, PC Bus, Step 2:1, DPLL 300 kbps @ 360 RPM)
                             - pc525dd: 5.25" DD on DD Drive (360K, PC Bus, Step 1:1, DPLL 250 kbps @ 300 RPM)
                             - amiga: Amiga 3.5" (880K, Shugart Bus, Step 1:1, DPLL 250 kbps @ 300 RPM)
                             - atari: Atari 3.5" (720K, PC Bus, Step 1:1, DPLL 250 kbps @ 300 RPM)
                             - cpc: Amstrad CPC 3.0" (178K, Shugart Bus, Step 1:1, DPLL 250 kbps @ 300 RPM)
                             Default: pc35hd
  -d, --drive <0-3>          Select target drive unit (0..1 for IBM PC, 0..3 for Shugart). Default: 0
      --drive=<0-3>          Alternative key-value syntax for drive unit selection
  -b, --bus <pc|shugart>     Select floppy interface bus type (pc | shugart). Default: pc
      --bus=<pc|shugart>     Alternative key-value syntax for bus type selection
      --shugart              Shorthand flag for Shugart bus mode (Amiga straight cable)
  -s, --step <single|double> Select step mode (single 1:1 for 96/135 TPI | double 2:1 for 48 TPI). Default: single
      --step=<single|double> Alternative key-value syntax for step mode selection
      --double-step          Shorthand flag for Double Step 2:1 mode (48 TPI media)
      --port <PORT>          Serial port connected to Greaseweazle
  -h, --help                 Print help information
  -v, -V, --version          Print version information
```

---

## 📖 Comprehensive Technical Documentation

For an exhaustive technical breakdown of the DPLL flux recovery engine, Greaseweazle low-level protocol opcodes, electromechanical timing constants, acoustic modulation equations, and multi-system retro formats, please refer to:

👉 **[Complete Technical Documentation (`documentation.md`)](documentation.md)**

---

## 📁 Repository Structure

```text
AlignTesterDiag/
├── Cargo.toml                 # Manifest & dependencies (ratatui, crossterm, serialport, crossbeam-channel)
├── README.md                  # Quick start guide, keybindings reference & showcase
├── README.txt                 # Plain-text distribution notes & cheat sheet
├── documentation.md           # Complete unified technical specification & manual
├── src/
│   ├── main.rs                # Application entrypoint, CLI argument parser, main event loop
│   ├── app.rs                 # State management, Action dispatcher, Dual-head metrics consolidation
│   ├── audio.rs               # Real-time sound worker thread, dynamic pitch calculation, platform beeps
│   ├── ui.rs                  # TUI components, styled ribbon spans, centering gauges, track ruler
│   └── hw/
│       ├── mod.rs             # Greaseweazle USB communication, DPLL, MFM decoding, hardware timings
│       ├── format.rs          # Low-level MFM track synthesis, CRC-16 table, pulse timing & format engine
│       └── protocol.rs        # Greaseweazle binary protocol opcodes, ACK codes, packet builders
└── export/
    ├── README.md              # Distribution showcase & quick reference
    ├── README.txt             # Plain-text distribution notes & cheat sheet
    ├── documentation.md       # Complete unified technical specification & manual
    └── Medias/                # Asset & screenshot directory
```

---

## 📄 License & Credits

- Copyright (C) 2026 Mr JeAn-FReD 🇫🇷
- **Heritage:** Inspired by Dave Dunfield's **ImageDisk (`IMD`)** and Keir Fraser's **Greaseweazle**.
- **License:** Distributed under the terms of the [GNU General Public License v3.0 (GPL-3.0)](LICENSE).

