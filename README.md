# 💾 AlignTesterDiag 🛠️

[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-v1.0.0-blue.svg)]()
[![TUI](https://img.shields.io/badge/ui-Ratatui%20%2B%20Crossterm-blue.svg)](https://ratatui.rs)
[![Hardware](https://img.shields.io/badge/hardware-Greaseweazle%20v4-green.svg)](https://github.com/keirf/greaseweazle)
[![Architecture](https://img.shields.io/badge/concurrency-100%25%20Non--Blocking-brightgreen.svg)]()
[![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE)

**AlignTesterDiag** is a high-performance, non-blocking, modern terminal user interface (TUI) diagnostics, formatting, and calibration suite for floppy disk drives. Interfacing directly with a **Greaseweazle USB flux controller**, it bridges Dave Dunfield's classic **ImageDisk (`IMD.C`)** diagnostic methodologies with modern sub-microsecond magnetic flux DPLL decoding, native multi-system retro hardware synthesis (IBM PC, Commodore Amiga Paula, Atari ST, Amstrad CPC), and real-time audio-visual feedback.

---

## 🌟 Key Highlights

- ⚡ **100% Non-Blocking Architecture (~60 Hz TUI):** Strict decoupling between the interactive Ratatui/Crossterm rendering loop, the USB hardware acquisition worker thread, and a dedicated real-time audio thread via lock-free `crossbeam-channel` queues. 64 KB extended serial block buffers eliminate micro-blocking system calls and USB CDC latency. **198 automated unit tests** with strict Clippy compliance (`cargo clippy -- -D warnings`).
- 🦁 **Native Commodore Amiga Paula Engine & Asynchronous Stream Synthesis:**
  - **Asynchronous Continuous Track Writing (`cue_at_index = false`):** Paula hardware writes tracks continuously as a circular MFM stream as soon as requested, without waiting for the physical index hole (`cue_at_index = false` for AmigaDOS vs. `cue_at_index = true` for index-aligned PC / Atari / CPC formats).
  - **Clean Paula Encoding (No Artificial Lead-In):** Exact Amiga track layout without artificial zero padding, generating 11 consecutive sectors with double sync words `0x44894489`, header info, 16-byte OS label, 32-bit XOR header checksum, 512-byte payload split into 128 even longwords + 128 odd longwords, 32-bit XOR data checksum, and 1 byte of MFM inter-sector gap (`0x00`).
  - **Seamless Over-Write Splice Loop (~108,000 MFM bits):** Encodes $\approx 1.08$ to $1.13$ revolutions ($\approx 216\text{ ms}$ @ 250 kbps) ensuring complete track erasure and a clean magnetic splice loop across drive RPM variations (295–305 RPM).
  - **100% Hardware Validation:** Tested and validated on physical Commodore Amiga 500 hardware running Amiga Test Kit (`........... (11/11 okay)`).
- 💾 **High-Precision Low-Level Format Engine & MFM Synthesizer (`CMD_WRITE_FLUX`):**
  - **Ultra-Fast Formatter Pipeline (~35s Fast / ~70s Full Verify):** Direct flux emission with optional 1-revolution instant DPLL read-after-write verification per track (toggled with <kbd>V</kbd> in format modal).
  - **24H Precision Progress Statistics:** Real-time timestamped tracking during multi-track operations:
    - *In Progress:* `Start: HH:MM:SS | Now: HH:MM:SS | Est. End: HH:MM:SS`
    - *Completed:* `Completed Successfully | Total Duration: HH:MM:SS`
  - **OS-Ready Filesystem Initialization Mode (<kbd>S</kbd>):** Instant injection of valid boot sectors, file allocation structures, and root directories tailored to the active profile:
    - **PC DOS FAT12:** Valid BPB standard (OEM `MSDOS5.0`), FAT1 & FAT2 tables with Media Descriptors (`0xF0` for 1.44M HD, `0xF9` for 720K DD / 1.2M 5.25" HD, `0xFD` for 360K DD), clean root directory, and `0x55AA` boot signature.
    - **Atari ST TOS:** Standard BPB with valid 16-bit word boot sector checksum (`sum == 0x1234`).
    - **Commodore Amiga OFS (`DOS\0`):** Valid Bootblock (Blocks 0 & 1, 32-bit circular carry checksum), RootBlock (Block 880, hash table & checksum), and BitmapBlock (Block 881, allocation map & checksum).
    - **Amstrad CPC AMSDOS:** CP/M standard directory catalog initialized (Track 0, sectors `0xC1..0xC4` filled with `0xE5`).
    - **Blank Mode (`Raw 0xE5`):** Low-level blank sectors filled with unformatted `0xE5` byte pattern.
  - **Tri-State Head Selection (<kbd>H</kbd>):** Dedicated head targeting (`Both (Dual-Head)` ➔ `Head 0 only` ➔ `Head 1 only` ➔ `Both`) applied across Single Track (<kbd>T</kbd>), Custom Range (<kbd>R</kbd>), and Full Disk (<kbd>D</kbd>) operations, with dynamic pass calculation: `total_passes = range.count() * heads.len()`.
  - **Custom Track Range (`TrackRange`) & Full Disk:** Flexible format targeting via Single Track (<kbd>T</kbd>), Custom Range (<kbd>R</kbd>), or Full Disk (<kbd>D</kbd>).
  - **Integrated Modal Navigation & Preset Cycling:** Live track stepping (<kbd>+</kbd>/<kbd>-</kbd>, <kbd>[</kbd>/<kbd>]</kbd>, <kbd>←</kbd>/<kbd>→</kbd>, or Mouse Wheel `ScrollUp`/`ScrollDown`), head cycle (<kbd>H</kbd>), preset cycle (<kbd>P</kbd>) with automatic geometry clamping, and total cylinder count adjustment (<kbd>PgUp</kbd>/<kbd>PgDn</kbd> or <kbd>↑</kbd>/<kbd>↓</kbd>).
  - **Interactive Range Editor (`RangeEditModal`):** Dual-bound modal editor with exclusive <kbd>Tab</kbd> field switching (`Start` / `End`), numeric typing (<kbd>0</kbd>–<kbd>9</kbd>), incremental adjustment (<kbd>+</kbd>/<kbd>-</kbd>, <kbd>↑</kbd>/<kbd>↓</kbd>, Mouse Wheel), head toggle (<kbd>H</kbd>), and strict validation (`start <= end < max_tracks`).
  - **Explicit Confirmation Safety Lock (`[y/N]`):** All destructive operations require explicit user confirmation (<kbd>Y</kbd> to confirm, <kbd>N</kbd> / <kbd>Enter</kbd> / <kbd>Esc</kbd> to cancel, with `N` as default).
  - **72 MHz Pulse Timing:** Translates synthesized MFM bitstreams into cycle-accurate 72 MHz interval ticks ($2T, 3T, 4T$).
  - **Write Pre-Compensation:** Applies $\pm 125\text{ ns}$ ($\approx 9$ ticks @ 72 MHz) timing shifts on inner tracks ($> 40$) to counteract magnetic peak shift.
  - **Read-After-Write Verification:** Automatically reads back each formatted track, verifies 100% expected sector presence, 0 CRC errors, and $Q \ge 85\%$ quality with up to 2 automatic retries.
  - **Dynamic Track Override:** Interactive track adjustment supporting standard 40/80 tracks up to 42/84 tracks with physical cylinder tracking.
  - **Write-Protect Guard:** Hardware Pin 28 (`WPROT`) validation before any carriage movement or flux emission.
- 🧲 **Hardware Low-Level DC Erase Engine (`CMD_ERASE_FLUX`):**
  - **Continuous Magnetic Neutralization:** Asserts raw continuous write gate without flux transitions for $\ge 1.1$ revolutions after index pulse to permanently wipe and degauss magnetic tracks.
  - **Interactive Erase Modal (<kbd>E</kbd>):** Single-track (<kbd>T</kbd>), custom track range (<kbd>R</kbd>), or full-disk (<kbd>D</kbd>) DC erasure with tri-state head targeting (<kbd>H</kbd>), preset cycling (<kbd>P</kbd>), 24H timing stats, and interactive track count bounds (40/42 for 48 TPI, 80/84 for 96/135 TPI).
- 🕹️ **Multi-System Retro Platform Diagnostics:**
  - **Amiga (Paula MFM):** Bit-level *even/odd* decoding, 32-bit XOR checksums, strict 11-sector DD ribbon `[ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (11/11 OK)`, and multi-revolution CRC repair.
  - **Atari ST (WD1772):** Support for 9, 10, and 11 sector layouts (up to 880 KB) and extended tracks (80 to 82).
  - **Amstrad CPC (µPD765 / 3"):** DATA (`0xC1..0xC9`), SYSTEM (`0x41..0x49`), and CP/M formats.
  - **IBM PC (NEC µPD765 / Intel 82077AA):** 720K DD, 1.44M HD, 360K DD, and 1.2M HD formats.
- ⏱️ **72 MHz Multi-Mode Spindle Tachometer & Live RPM Centering Gauge:**
  - **Hardware Pin 8 Index Measurement:** Direct hardware timing via inter-index flux summation.
  - **Software DPLL Sync Fallback:** Reconstructed spindle speed from MFM sync pulse intervals for unindexed drives or missing index pulses.
  - **Dual Mode & Differential ($\Delta\text{RPM}$):** Real-time side-by-side comparison of HW Index and SW Sync.
  - **Contextual <kbd>I</kbd> Key:** Toggles tachometer measurement modes in Live RPM (<kbd>L</kbd>), or toggles track details in standard views.
  - **Visual Jitter Gauge:** 10-revolution rolling average, peak-to-peak jitter ($\Delta\text{RPM}$, $\pm\Delta\%$), and 21-character visual centering gauge (300.0 RPM / 360.0 RPM).
- 🎯 **Real-Time Mechanical Alignment Radar:** Continuous on-track vs. off-track sector validation with live percentage gauge calculation, detecting misaligned head carriages, tracking errors, and invalid track IDs.
- 🔊 **Dynamic Pitch Acoustic Variometer:** Auditory feedback inspired by glider variometers. Continuously modulates multi-tier audio frequency (**1500 Hz – 2200 Hz** for nominal alignment $\ge 95\%$, **600 Hz – 1400 Hz** for marginal tracking $70\text{--}94\%$, **250 Hz – 500 Hz** continuous tone for severe misalignment $< 70\%$), with instantaneous **180 Hz pulsed warning buzz** on cross-track mismatch and **150 Hz hum** on zero decoded sectors.
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
- **Reference Diskette:** A known-good, formatted 3.5" (720K DD / 1.44M HD / 880K Amiga) or 5.25" (360K DD / 1.2M HD) floppy diskette.

### 2. Build from Source
```bash
git clone https://github.com/Jean-Fred64/AlignTesterDiag.git
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
| <kbd>F</kbd> | **Format Modal** | Open Low-Level Track & Disk Formatter Modal (`[T]` Track, `[R]` Range, `[D]` Disk, `[S]` FS Mode, `[V]` Verify, `[Esc]` Cancel) |
| <kbd>L</kbd> | **Live RPM** | Live RPM tachometer & jitter stability test |
| <kbd>I</kbd> | **Index / RPM Mode Toggle** | Contextual: Cycle RPM measurement mode in Live RPM (`HW Pin 8` ➔ `SW Sync` ➔ `Dual`), or toggle track info in standard view |
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
| **Format / Erase Modals** | <kbd>T</kbd> | Execute operation on **Current Track** only (Head 0, Head 1, or Both) |
| | <kbd>R</kbd> | Open **Custom Track Range Editor** (`RangeEditModal`) |
| | <kbd>D</kbd> | Execute operation on **Entire Disk** (Tracks 00..max, Head 0, Head 1, or Both) |
| | <kbd>H</kbd> | Cycle physical target head (`Both` ➔ `Head 0` ➔ `Head 1` ➔ `Both`) |
| | <kbd>P</kbd> | Cycle active **Preset Profile** (auto-adjusts geometry, bitrate, RPM target, and clamps tracks) |
| | <kbd>S</kbd> *(Format only)* | Toggle **System FS Init** (`Blank (Raw 0xE5)` $\leftrightarrow$ `OS-Ready (Boot & Root FS)`) |
| | <kbd>V</kbd> *(Format only)* | Toggle **Read-After-Write Verify** (`ON` ~70s / `OFF` ~35s) |
| | <kbd>+</kbd> / <kbd>-</kbd>, <kbd>[</kbd> / <kbd>]</kbd>, <kbd>←</kbd> / <kbd>→</kbd>, `Scroll` | Step target cylinder for single-track operations |
| | <kbd>PgUp</kbd> / <kbd>PgDn</kbd>, <kbd>↑</kbd> / <kbd>↓</kbd> | Adjust total disk tracks (40/42 for 48 TPI, 80/84 for 96/135 TPI) |
| | <kbd>Esc</kbd> / <kbd>Q</kbd> / <kbd>X</kbd> | Close modal and cancel |
| **Range Editor (`RangeEditModal`)** | <kbd>Tab</kbd> | Switch active input field (`Start Track` $\leftrightarrow$ `End Track`) |
| | <kbd>0</kbd>–<kbd>9</kbd> | Type track number directly |
| | <kbd>+</kbd> / <kbd>-</kbd>, <kbd>↑</kbd> / <kbd>↓</kbd>, <kbd>←</kbd> / <kbd>→</kbd>, `Scroll` | Increment / decrement active bound |
| | <kbd>H</kbd> | Cycle target head (`Both` ➔ `Head 0` ➔ `Head 1` ➔ `Both`, dynamic passes calculation) |
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
├── Cargo.lock                 # Deterministic dependency lockfile
├── LICENSE                    # GNU General Public License v3.0 (GPL-3.0)
├── README.md                  # Quick start guide, keybindings reference & showcase
├── README.txt                 # Plain-text distribution notes & cheat sheet
├── documentation.md           # Complete unified technical specification & manual
├── Medias/                    # Showcase screenshots and visual assets
└── src/
    ├── main.rs                # Application entrypoint, CLI argument parser, main event loop
    ├── app.rs                 # State management, Action dispatcher, Dual-head metrics consolidation
    ├── audio.rs               # Real-time sound worker thread, dynamic pitch calculation, platform beeps
    ├── ui.rs                  # TUI components, styled ribbon spans, centering gauges, track ruler
    └── hw/
        ├── mod.rs             # Greaseweazle USB communication, DPLL, MFM decoding, hardware timings
        ├── format.rs          # Low-level MFM track synthesis, CRC-16 table, pulse timing & format engine
        ├── fs.rs              # OS-Ready filesystem payload synthesizer (DOS FAT12, Atari TOS, Amiga OFS, CP/M)
        └── protocol.rs        # Greaseweazle binary protocol opcodes, ACK codes, packet builders
```

---

## 📄 License & Credits

- Copyright (C) 2026 MonSieur JeAn-FReD 🇫🇷
- **Author:** MonSieur JeAn-FReD (`https://github.com/Jean-Fred64`)
- **Heritage:** Inspired by Dave Dunfield's **ImageDisk (`IMD`)** and Keir Fraser's **Greaseweazle**.
- **License:** Distributed under the terms of the [GNU General Public License v3.0 (GPL-3.0)](LICENSE).
