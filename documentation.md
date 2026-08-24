# 💾 AlignTesterDiag v0.2.0-alpha — Comprehensive Technical Documentation & Architecture Manual

Welcome to the definitive technical documentation for **AlignTesterDiag**, an ultra-responsive, non-blocking terminal user interface (TUI) diagnostics and calibration platform for floppy disk drives connected via the **Greaseweazle USB flux controller**.

---

## 📑 Table of Contents

1. [System Architecture & Concurrency Model](#1-system-architecture--concurrency-model)
2. [Greaseweazle Hardware Protocol & Interfacing](#2-greaseweazle-hardware-protocol--interfacing)
3. [Electromechanical Timings & Drive Compatibility](#3-electromechanical-timings--drive-compatibility)
4. [Magnetic Flux Processing & DPLL MFM Decoder](#4-magnetic-flux-processing--dpll-mfm-decoder)
5. [Real-Time Alignment Diagnostic Engine](#5-real-time-alignment-diagnostic-engine)
6. [Acoustic Variometer & Alignment Radar](#6-acoustic-variometer--alignment-radar)
7. [High-Precision Spindle Tachometer & Live RPM Jitter Engine](#7-high-precision-spindle-tachometer--live-rpm-jitter-engine)
8. [High-Precision Low-Level Format Engine & MFM Synthesizer](#8-high-precision-low-level-format-engine--mfm-synthesizer)
9. [User Interface, Visual Indicators & Controls](#9-user-interface-visual-indicators--controls)
10. [Automated Test & Verification Suite](#10-automated-test--verification-suite)
11. [Technical Roadmap & Multi-System Future Support](#11-technical-roadmap--multi-system-future-support)

---

## 1. System Architecture & Concurrency Model

AlignTesterDiag is engineered around a **100% non-blocking, multi-threaded architecture** designed to maintain a consistent ~60 Hz terminal rendering framerate while continuously capturing raw flux transitions over USB and synthesizing pitch-modulated audio feedback.


### 1.1 Concurrency Topology
The application executes across three decoupled OS threads:

1. **Main UI & Render Thread (`src/main.rs`, `src/ui.rs`):**
   - Drives the **Ratatui** and **Crossterm** TUI engine.
   - Drains incoming status updates from the hardware worker via `rx_status.try_recv()`.
   - Polls keyboard input with a 15 ms timeout slice (`crossterm::event::poll(Duration::from_millis(15))`).
   - Translates key presses into strongly-typed `HwCmd` messages sent to `tx_cmd`.
2. **Hardware I/O & Decoding Thread (`src/hw/mod.rs`):**
   - Owns the USB CDC serial connection to the Greaseweazle hardware (`serialport`).
   - Manages drive selection, motor power sequencing, track seeking, and raw flux stream captures.
   - Executes the software Digital Phase-Locked Loop (DPLL) and MFM sector decoding pipeline.
   - Publishes periodic `DriveStatus` snapshots via `tx_status`.
   - Dispatches `AudioEvent` notifications to the audio thread via `tx_audio`.
3. **Real-Time Sound Worker Thread (`src/audio.rs`):**
   - Listens on a dedicated unbounded channel for `AudioEvent` signals.
   - Automatically drains intermediate backlog events to eliminate latency and acoustic lag.
   - Issues non-blocking Win32 `Beep()` or POSIX terminal bell pulses.

### 1.2 Thread Communication Enums & Structures

```rust
pub enum HwCmd {
    SelectUnit(u8),
    ToggleDriveUnit,
    SetMotor(bool),
    ToggleMotor,
    MeasureRpm,
    Seek(u8),
    RecalibrateSeek,
    ZeroTrack,
    SetHead(u8),
    SetHeadSelection(HeadSelection),
    ToggleHead,
    Analyze,
    StartAnalysis,
    ReadData,
    ToggleVerbose,
    ToggleBeep,
    SetVerbose(bool),
    SetBeep(bool),
    FormatTrack { track: u8, head_sel: HeadSelection, verify: bool, fs_mode: FsInitMode },
    FormatDisk { range: TrackRange, head_sel: HeadSelection, verify: bool, fs_mode: FsInitMode },
    EraseTrack { track: u8, head_sel: HeadSelection },
    EraseDisk { range: TrackRange, head_sel: HeadSelection },
    Stop,
    PanicReset,
    SetDiskFormat(DiskFormat),
    CycleDiskFormat,
    SetBusType(BusType),
    ToggleBusType,
    SetStepMode(StepMode),
    ToggleStepMode,
    CyclePreset,
    SetPreset(PresetProfile),
    Exit,
}

/// Filesystem payload initialization mode for formatting
pub enum FsInitMode {
    /// Raw unformatted sectors filled with standard byte 0xE5
    Blank,
    /// OS-ready initialization with valid boot sector, FAT/OFS/CPM structures, and root directory
    OsReady,
}

/// Floppy head selection mode for analysis, formatting and erasing
pub enum HeadSelection {
    Head0,
    Head1,
    Both,
}

/// Explicit confirmation state for destructive actions (Format / Erase)
pub enum PendingConfirmation {
    FormatTrack { track: u8, head_sel: HeadSelection, verify: bool, fs_mode: FsInitMode, preset: PresetProfile },
    FormatDisk { range: TrackRange, head_sel: HeadSelection, verify: bool, fs_mode: FsInitMode, preset: PresetProfile },
    FormatRange { range: TrackRange, head_sel: HeadSelection, verify: bool, fs_mode: FsInitMode, preset: PresetProfile },
    EraseTrack { track: u8, head_sel: HeadSelection },
    EraseDisk { range: TrackRange, head_sel: HeadSelection },
    EraseRange { range: TrackRange, head_sel: HeadSelection },
}

/// Modal editor context and field selection
pub enum RangeModalKind {
    Format,
    Erase,
}

pub enum RangeField {
    Start,
    End,
}

/// Bounded contiguous track range specification
pub struct TrackRange {
    pub start: u8,
    pub end: u8,
}
```

---

## 2. Greaseweazle Hardware Protocol & Interfacing

AlignTesterDiag interfaces directly with the Greaseweazle firmware over USB CDC Virtual COM port at 115,200 baud. It supports automated device discovery and communicates using the native Greaseweazle binary protocol.

### 2.1 Auto-Discovery
During startup, `find_greaseweazle()` scans all available serial ports for USB Vendor/Product IDs:
- **VID:** `0x1209` (Generic Open Source Hardware)
- **PID:** `0x4D22` (Greaseweazle v4) / `0x4D69` (Greaseweazle v4.1)

If USB enumeration fails, the engine falls back to testing standard serial paths (`COM2`, `COM10`, `/dev/ttyACM0`, `/dev/ttyS2`) by asserting DTR/RTS control lines.

### 2.2 Protocol Opcode Reference

All commands follow the Greaseweazle frame format: `[CMD_OPCODE, FRAME_LENGTH, ARGS...]`. The controller responds with a 2-byte header `[CMD_ECHO, STATUS]` followed by optional payload bytes.

| Opcode (Hex) | Command Name | Arguments | Response Payload | Purpose |
|:---|:---|:---|:---|:---|
| `0x00` | `CMD_GET_INFO` | `[0x00, 0x03, 0x00]` | 32 bytes | Query firmware version, hardware model, sample clock frequency |
| `0x0E` | `CMD_SET_BUS_TYPE` | `[0x0E, 0x03, bus_type]` | 0 bytes | Configure interface pinout (`0x01` = IBM PC standard, `0x02` = Shugart standard) |
| `0x0C` | `CMD_SELECT` | `[0x0C, 0x03, unit]` | 0 bytes | Assert drive select line (`0` = Drive A / DS0, `1` = Drive B / DS1, `2` = DS2, `3` = DS3) |
| `0x0D` | `CMD_DESELECT` | `[0x0D, 0x02]` | 0 bytes | Deselect all drive units / release interface bus |
| `0x06` | `CMD_MOTOR` | `[0x06, 0x04, unit, state]` | 0 bytes | Control spindle motor (`1` = ON, `0` = OFF) |
| `0x02` | `CMD_SEEK` | `[0x02, 0x03, cyl]` | 0 bytes | Step head carriage to logical cylinder (`0` to `83`) |
| `0x03` | `CMD_HEAD` | `[0x03, 0x03, head]` | 0 bytes | Select physical head (`0` = Lower / Side 0, `1` = Upper / Side 1) |
| `0x07` | `CMD_READ_FLUX` | `[0x07, 0x08, 0x00, 0x00, 0x00, 0x00, rev_low, rev_high]` | Stream | Stream raw magnetic flux transition timings (64 KB extended reception buffer) |
| `0x08` | `CMD_WRITE_FLUX` | `[0x08, 0x04, cue_at_index, terminate_at_index]` | Status ACK | Write raw magnetic flux stream synchronized to index pulse |
| `0x11` | `CMD_ERASE_FLUX` | `[0x11, 0x06, ticks_le_u32]` | Status ACK | Assert continuous neutral write gate without flux transitions for specified sample ticks ($\ge 1.1$ revs) |
| `0x14` | `CMD_GET_PIN` | `[0x14, 0x03, pin_num]` | 1 byte | Read logic level of physical connector pin |

<details>
<summary>🔍 <b>Hardware Pin Monitoring Details (Active-Low Signals)</b></summary>

Floppy interface signals utilize active-low open-collector logic ($0 = \text{Asserted / Low}$, $1 = \text{Negated / High}$). AlignTesterDiag monitors:

- **Pin 28 (`/WRTPRT`):** Write Protect sensor. Sampled via `CMD_GET_PIN (0x14, 28)`. Low indicates the diskette write-protect notch is open.
- **Pin 26 (`/TRK00`):** Optical Track 0 sensor. Read via `DriveStatus`. Indicates carriage is resting at the physical base stop.
- **Pin 8 (`/INDEX`):** Spindle index pulse sensor. Emits one low pulse per revolution.
</details>

---

## 3. Electromechanical Timings & Drive Compatibility

Floppy disk drives incorporate physical stepper motors, lead screws, and rotating mechanical spindles subject to inertia and vibration. AlignTesterDiag enforces strict timing constants to ensure reliable operation across legacy 34-pin PC drives and 26-pin slim notebook drives.

```text
[Motor Start] ──(350 ms)──> [Spindle Stabilized @ 300 RPM]
[Step Pulse]  ──(15-30 ms)─> [Vibration Dampened] ──> [Flux Read]
[Head Switch] ──(1 ms)────> [Preamplifier Settled] ──> [Flux Read]
```

### 3.1 Electromechanical Timing Constants

| Constant | Value | Description & Technical Rationale |
|:---|:---:|:---|
| `STEPPER_WAKEUP_DELAY_MS` | `15 ms` | **Stepper Driver Electronic Wake-up Delay:** On 26-pin FFC drives (e.g. TEAC FD-05HG with adapter), the stepper motor driver IC is powered down when the spindle motor signal is negated. This delay ensures power rail stabilization before issuing step pulses. |
| `HEAD_SETTLE_TIME_MS` | `30 ms` | **Head Settling & Damping Delay:** Mechanical carriage dampening time after a multi-track seek to eliminate head bounce and vibration before reading flux. |
| `FORMAT_HEAD_SETTLE_MS` | `100 ms` | **Format Track Step Settle Delay:** Robust mechanical carriage dampening time after single-track stepping during low-level format operations. |
| `FORMAT_HEAD_SWITCH_SETTLE_MS` | `50 ms` | **Format Head Switch Settle Delay:** Preamplifier stabilization delay when switching between Head 0 and Head 1 during format operations on the same cylinder. |
| `HEAD_SWITCH_SETTLE_MS` | `1 ms` | **Electronic Head Switch Settle Time:** Preamplifier stabilization delay when switching between Head 0 and Head 1. |
| `SPIN_UP_DELAY_MS` | `350 ms` | **Motor Spin-Up Delay:** Time required for the DC brushless spindle motor to accelerate from 0 to 300 RPM and lock index synchronization. |
| `RECALIBRATE_WAIT_MS` | `200 ms` | **Track 0 Shock Dissipation Delay:** One full disk revolution (~200 ms) allowed after optical stop recalibration to dissipate mechanical shock before MFM decoding. |
| `DWELL_TIME_TRK0_MS` | `60 ms` | **Dwell Time at Track 0:** Carriage stabilization pause at the physical stop during recalibrate-and-return sequences. |
| `SEEK_TRK0_TIMEOUT_MS` | `3000 ms` | **Track 0 Seek Timeout:** Maximum guard window during optical base stop homing operations. |
| `DEFAULT_SERIAL_TIMEOUT_MS` | `1000 ms` | **USB Flux Read Timeout:** Maximum allowable communication window for flux packet reception. |
| `ACK_GUARD_TIMEOUT_MS` | `500 ms` | **Hardware ACK Guard Timeout:** Response timeout on low-level command acknowledgments. |
| `SYNC_DELAY_MS` | `30 ms` | **Bus Protocol Synchronization Pause:** Short stabilization delay during bus mode re-sync. |

### 3.2 Motor-Gated Seek Operation
When stepping tracks while the motor is stopped, issuing immediate step pulses will fail on 26-pin drives whose stepper drivers are powered off. `perform_motor_gated_seek()` resolves this:
1. Temporarily asserts motor power (`CMD_MOTOR = 1`).
2. Waits `STEPPER_WAKEUP_DELAY_MS` (15 ms) for driver IC stabilization.
3. Issues `CMD_SEEK` to the target cylinder.
4. Restores motor state to OFF.

If the motor is already running (e.g., in Analyze or Tachometer mode), the seek executes immediately without extra latency.

---

## 4. Magnetic Flux Processing & DPLL MFM Decoder

AlignTesterDiag implements a high-performance software **Digital Phase-Locked Loop (DPLL)** and **Modified Frequency Modulation (MFM)** decoding engine capable of recovering clean bitstreams from noisy analog flux reversals.

### 4.1 Greaseweazle Flux Encoding
Greaseweazle transmits flux transitions as a stream of byte-encoded timer ticks measured against its internal **72 MHz sample clock**:
- Byte `1..=249`: A flux reversal occurred $N$ ticks after the previous event.
- Byte `0xFA`: Extension marker adding $+250$ ticks without a flux reversal.

$$\text{Interval (seconds)} = \frac{\text{Total Ticks}}{72{,}000{,}000}$$

### 4.2 Software DPLL Architecture
The software DPLL (`SoftwarePll`, `pll_flux_to_mfm_bits`) dynamically tracks spindle speed fluctuations and instantaneous phase jitter:

```mermaid
flowchart LR
    A[Raw Flux Ticks] --> B[Parasitic Pulse Filter < 1.5 µs]
    B --> C[Accumulate Ticks in Phase Acc]
    C --> D{Ticks >= Clock / 2 ?}
    D -- No --> C
    D -- Yes --> E[Emit Bit '0']
    E --> F[Subtract Clock Window]
    F --> D
    D -- Reversal Event --> G[Emit Bit '1']
    G --> H[Adjust Phase & Period]
    H --> I[Clamp Clock to Adaptive Tolerance]
```

$$\text{Clock}_{\text{nom}} = \frac{72{,}000{,}000}{2 \times \text{Bitrate (bps)}} \implies \begin{cases} 72.0 \text{ ticks} & \text{for 500 kbps (HD)} \\ 120.0 \text{ ticks} & \text{for 300 kbps (DD @ 360 RPM, Pc525DdOnHd)} \\ 144.0 \text{ ticks} & \text{for 250 kbps (DD @ 300 RPM)} \end{cases}$$

#### Adaptive Clamping Tolerance & Noise Filtering:
- **High Density (500 kbps / 72.0 ticks):** Nominal $\pm 10\%$ tolerance ($\text{Clock}_{\min} = 64.8$, $\text{Clock}_{\max} = 79.2$, $\text{Phase Adj} = 0.60$) for dense, low-jitter flux cells.
- **Double Density (250 kbps & 300 kbps / $\ge 100.0$ ticks):** Extended $\pm 25\%$ tolerance ($\text{Clock}_{\min} = \text{Clock}_{\text{nom}} \times 0.75$, $\text{Clock}_{\max} = \text{Clock}_{\text{nom}} \times 1.25$, $\text{Phase Adj} = 0.65$) to absorb heavy spindle flutter and phase jitter on legacy 5.25" and 3.5" media.
- **Parasitic Noise Filtering (< 1.5 µs / 108 ticks @ 72 MHz):** In DD modes ($\text{Clock}_{\text{nom}} \ge 100.0$ ticks), pulses under 108 ticks generated by 48 TPI fringe track noise are accumulated and merged into subsequent flux transitions.

Phase adjustment ($\alpha$) and period adjustment ($\beta = 0.05$) adapt the clock window on every valid flux pulse:
$$\text{Clock} \leftarrow \text{Clock} + \Delta\text{Ticks} \times \beta$$

### 4.3 Automated Density Detection
By evaluating average flux transition intervals across the initial revolution, the engine classifies disk density automatically:
- **Average Cell $\le 240$ ticks:** High Density (HD, 500 kbps, 18 sectors/track @ 300 RPM).
- **Average Cell $> 240$ ticks:** Double Density (DD, 250 kbps, 9 sectors/track @ 300 RPM).

### 4.4 MFM Address Marks & CRC-16 CCITT
MFM encoding uses clock bits to separate data bits. Special synchronization words violate standard MFM clock rules to create unique framing markers.

```text
MFM Sync Word: 0x4489 (Decodes to 0xA1 with missing clock transition between bits 4 and 5)
```

| Marker | Sync Sequence | Mark Byte | Description |
|:---|:---|:---:|:---|
| `IAM` | `0xC2C2C2` | `0xFC` | **Index Address Mark:** Marks the start of track flux. |
| `IDAM` | `0xA1A1A1` (`0x448944894489`) | `0xFE` | **ID Address Mark:** Precedes cylinder, head, sector, size code header. |
| `DAM` | `0xA1A1A1` (`0x448944894489`) | `0xFB` | **Data Address Mark:** Precedes 512-byte sector data payload. |
| `DDAM` | `0xA1A1A1` (`0x448944894489`) | `0xF8` | **Deleted Data Address Mark:** Marks sector as deleted/bad. |

#### CRC-16 CCITT Calculation
Both sector headers and data payloads are validated using the standard CCITT polynomial:

$$P(x) = x^{16} + x^{12} + x^5 + 1 \quad (\text{Polynomial: } \mathtt{0x1021}, \text{ Initial Seed: } \mathtt{0xFFFF})$$

The calculation includes the three `0xA1` sync bytes:
```rust
fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}
```

---

## 5. Real-Time Alignment Diagnostic Engine

The diagnostic engine continuously measures head positioning accuracy relative to the magnetic track centerline, identifying track slippage, misaligned head carriages, and azimuth errors.

### 5.1 Alignment Metric Calculation
Alignment quality is calculated from the ratio of successfully decoded sectors matching the target cylinder vs. expected sectors:

$$\text{Mechanical Alignment (\%)} = \left( \frac{\sum \text{Valid Sectors on Target Track}}{\text{Total Expected Sectors}} \right) \times 100$$

- **≥ 95% (Green):** Nominal factory alignment.
- **70% – 94% (Yellow):** Degraded alignment / marginal tracking.
- **< 70% (Red):** Severe mechanical misalignment or corrupt track.

### 5.2 Single-Head vs. Dual-Head ("Both" Mode) Operation
Users can toggle head acquisition mode using the <kbd>H</kbd> key:

1. **Head 0 / Head 1 Modes:** Continuously samples the selected physical head, displaying a sliding history stream of the latest 12–13 revolution passes.
2. **"Both" Mode (Dual-Head Consolidated):** Alternates head selection on every revolution pass (`Head 0` $\leftrightarrow$ `Head 1`), presenting a dedicated **2-line persistent display**:
   - **Line 1:** Head 0 status ribbon and metrics.
   - **Line 2:** Head 1 status ribbon and metrics.
   - An active cursor pointer (`► `) indicates which head is currently being sampled.

### 5.3 Cross-Track Divergence Detection
When operating in "Both" mode, if Head 0 reads Track $N$ while Head 1 reads Track $N \pm 1$ (due to physical carriage skew or split head alignment), the engine immediately triggers:
- Diagnostic Flag: `MISMATCH: Track X on Head 0, Track Y on Head 1`
- Alignment penalty: Alignment score is reduced to $50\%$ or lower.
- Orange/Red ribbon highlighting on misaligned segments.
- Immediate **180 Hz pulsed warning buzz** from the audio variometer.

---

## 6. Acoustic Variometer & Alignment Radar

To allow technicians to align floppy drive head carriages without constantly looking at the screen, AlignTesterDiag includes a real-time **Multi-Tier Acoustic Variometer** inspired by soaring flight instrumentation.

### 6.1 Multi-Tier Dynamic Frequency Modulation
The acoustic engine dynamically calculates pitch across three continuous quality tiers ($Q\%$):

$$\text{Pitch}(Q\%) = \begin{cases}
1500 + \left(\frac{Q\% - 95}{5}\right) \times 700 \text{ Hz} & \text{for } 95\% \le Q\% \le 100\% \text{ (Nominal Factory Alignment)} \\[6pt]
600 + \left(\frac{Q\% - 70}{24}\right) \times 800 \text{ Hz} & \text{for } 70\% \le Q\% \le 94\% \text{ (Marginal Tracking)} \\[6pt]
250 + \left(\frac{Q\%}{69}\right) \times 250 \text{ Hz} & \text{for } Q\% < 70\% \text{ (Severe Misalignment — never silenced)}
\end{cases}$$

```text
 100% Quality ──> 2200 Hz (C#7)  ──┐
  95% Quality ──> 1500 Hz (G6)   ──┴─ [Tier 1: Nominal Factory Alignment]
  94% Quality ──> 1400 Hz (F6)   ──┐
  70% Quality ──>  600 Hz (D5)   ──┴─ [Tier 2: Marginal Tracking]
  69% Quality ──>  500 Hz (B4)   ──┐
   0% Quality ──>  250 Hz (B3)   ──┴─ [Tier 3: Severe Misalignment (Continuous Low Tone)]
```

### 6.2 Sonic Signature Mapping

| Audio Event | Frequency / Pattern | Duration | Acoustic Profile & Trigger Condition |
|:---|:---:|:---:|:---|
| **Nominal Alignment** | `1500 Hz – 2200 Hz` | `40 ms` | High clean tone. Dynamic pitch tracking signal quality $95\% \le Q\% \le 100\%$. |
| **Marginal Tracking** | `600 Hz – 1400 Hz` | `40 ms` | Medium tone. Dynamic pitch tracking signal quality $70\% \le Q\% \le 94\%$. |
| **Severe Misalignment** | `250 Hz – 500 Hz` | `40 ms` | Low continuous tone ($Q\% < 70\%$). Never muted, providing non-zero auditory guidance. |
| **Track Mismatch** | `180 Hz` pulsed buzz | `2x 50 ms` (15 ms gap) | Low dissonant alarm. Triggered on cross-track divergence ($T_{\text{H0}} \ne T_{\text{H1}}$) or carriage off-target ($T_{\text{read}} \ne T_{\text{target}}$). |
| **Zero Decoded Sectors** | `150 Hz` | `40 ms` | Low-frequency warning hum. Triggered when zero valid sectors or IDAM headers are detected. |
Inspired by glider variometers, the real-time sound thread (`src/audio.rs`) synthesizes continuous pitch-modulated auditory feedback to guide head alignment adjustments without looking at the screen:

- **Nominal Alignment ($\ge 95\%$):** Clean high-frequency tone ($1500\text{ Hz} \le f \le 2200\text{ Hz}$).
- **Marginal Tracking ($70\text{--}94\%$):** Medium-frequency tone ($600\text{ Hz} \le f \le 1400\text{ Hz}$).
- **Severe Misalignment ($< 70\%$):** Low continuous tone ($250\text{ Hz} \le f \le 500\text{ Hz}$).
- **Cross-Track Mismatch:** Instantaneous double-pulsed 180 Hz warning buzz ($2 \times 50\text{ ms}$).
- **Zero Decoded Sectors:** 150 Hz warning hum (40 ms).

---

## 7. High-Precision Spindle Tachometer & Live RPM Jitter Engine

Accessed via the <kbd>L</kbd> key, the Spindle Tachometer measures disk rotational speed directly from hardware index pulses at **72 MHz sub-microsecond resolution**.

### 7.1 Mathematical Model & Jitter Calculation
The controller captures timer ticks between successive index pulses:

$$\text{Revolution Period } (ms) = \frac{\Delta\text{Ticks}}{72{,}000}$$

$$\text{Instant RPM} = \frac{60{,}000}{\text{Revolution Period } (ms)}$$

To filter high-frequency motor flutter without concealing mechanical drift, AlignTesterDiag computes:
- **Rolling Average ($\text{Avg RPM}$):** 10-revolution sliding window average.
- **Peak-to-Peak Speed Jitter ($\Delta\text{RPM}$):** $(\text{Max RPM} - \text{Min RPM}) / 2$.
- **Percentage Jitter ($\pm\Delta\%$):** $\left( \frac{\text{Max RPM} - \text{Min RPM}}{2 \times \text{Avg RPM}} \right) \times 100$.

### 7.2 21-Character Visual Centering Gauge & Nominal RPM Targets

Nominal spindle speeds by drive type & preset:
- **360.0 RPM:** 5.25" High Density formats (`Pc525Hd` 1.2M and `Pc525DdOnHd` 360K DD on HD drive).
- **300.0 RPM:** All 3.5" formats (PC 720K/1.44M, Amiga 880K, Atari 720K), 3.0" CPC, and native 5.25" DD drives (`Pc525Dd`).

```text
[----|----▼----|----]  <-- Nominal Target (300.0 / 360.0 RPM +/- 0.5% - Light Green)
[----|---▼|----|----]  <-- Moderate Drift (-1.0% - Yellow)
[▼---|----|----|----]  <-- Severe Under-speed (< -1.5% - Red)
[----|----|----|---▼]  <-- Severe Over-speed (> +1.5% - Red)
```

### 7.3 Motor Health Classification Matrix

| Jitter Range ($\pm\Delta\%$) | Rating | Health Assessment |
|:---|:---:|:---|
| $\le \pm 0.20\%$ | `★★★★★` | **EXCELLENT STABILITY:** Direct-drive quartz-locked precision. |
| $\le \pm 0.50\%$ | `★★★★☆` | **GOOD STABILITY:** Nominal belt-drive performance. |
| $\le \pm 1.00\%$ | `★★★☆☆` | **ACCEPTABLE STABILITY:** Usable; minor spindle belt wear or dry bearings. |
| $> \pm 1.00\%$ | `★☆☆☆☆` | **UNSTABLE MOTOR SPEED:** Defective drive belt, failing motor controller, or excessive friction. |

---

## 8. High-Precision Low-Level Format Engine & MFM Synthesizer

Accessed via the <kbd>F</kbd> key, the Low-Level Format Engine enables bit-accurate track and whole-disk magnetic formatting directly through Greaseweazle's `CMD_WRITE_FLUX` (0x08) protocol command.

### 8.1 Zero-Allocation MFM Synthesizer Architecture (`src/hw/format.rs`)

1. **Static Conversion Tables:**
   - **MFM Clock/Data Encoding:** Static $2 \times 256$ entry lookup table (`MFM_ENCODE_TABLE`) encoding data bit $1 \to 01$, bit $0$ (after $0$) $\to 10$, bit $0$ (after $1$) $\to 00$.
   - **Altered Sync Drops:** Generates missing clock sync words `0xA1*` (clock transition drop $0x0A$ instead of $0x0E$, producing MFM word `0x4489`) for IDAM (`0xFE`) and DAM (`0xFB`).
   - **CRC16-CCITT Accelerator:** Precomputed 256-word lookup table (`const CRC16_TABLE`) using polynomial `0x1021` and initial seed `0xFFFF`.

2. **Supported Format Preset Layouts:**
   - **IBM PC 1.44M HD:** 18 sectors/track (512B, Gap1=80, Gap2=22, Gap3=84, fill=0xE5, 500 kbps @ 300 RPM).
   - **IBM PC 720K DD:** 9 sectors/track (512B, Gap1=32, Gap2=22, Gap3=54, fill=0xE5, 250 kbps @ 300 RPM).
   - **IBM PC 1.2M 5.25" HD:** 15 sectors/track (512B, Gap1=80, Gap2=22, Gap3=84, fill=0xE5, 500 kbps @ 360 RPM).
   - **IBM PC 360K (HD Drive):** 9 sectors/track (512B, Gap1=32, Gap2=22, Gap3=54, fill=0xE5, 300 kbps @ 360 RPM, Step 2:1).
   - **IBM PC 360K (DD Drive):** 9 sectors/track (512B, Gap1=32, Gap2=22, Gap3=54, fill=0xE5, 250 kbps @ 300 RPM, Step 1:1).
   - **AmigaDOS DD (880K):** 11 sectors/track (512B, split even/odd Paula, 0x4489 sync, 32-bit XOR checksum, 250 kbps).
   - **Atari ST & Amstrad CPC Data:** 9 sectors/track (512B, 250 kbps).

### 8.2 72 MHz Flux Translation & Write Pre-Compensation

1. **Cycle-Accurate Timings:**
   - $500\text{ kbps} \implies 1T = 72\text{ ticks}, 2T = 144\text{ ticks}, 3T = 216\text{ ticks}, 4T = 288\text{ ticks}$.
   - $300\text{ kbps} \implies 1T = 120\text{ ticks}, 2T = 240\text{ ticks}, 3T = 360\text{ ticks}, 4T = 480\text{ ticks}$.
   - $250\text{ kbps} \implies 1T = 144\text{ ticks}, 2T = 288\text{ ticks}, 3T = 432\text{ ticks}, 4T = 576\text{ ticks}$.

2. **Write Pre-Compensation on Inner Cylinders ($> 40$):**
   - Applies $\pm 125\text{ ns}$ ($\approx 9\text{ ticks}$ @ 72 MHz) shifts to counteract magnetic peak shift:
     * $2T$ followed by $\ge 3T \implies$ shifted **EARLY** ($-9\text{ ticks}$ on $2T$, $+9\text{ ticks}$ on $\ge 3T$).
     * $\ge 3T$ followed by $2T \implies$ shifted **LATE** ($+9\text{ ticks}$ on $\ge 3T$, $-9\text{ ticks}$ on $2T$).
     * Symmetrical intervals ($2T-2T$ or $\ge 3T - \ge 3T$) have zero pre-compensation shift.

### 8.3 Filesystem Payload Synthesizer & OS-Ready Mode (`src/hw/fs.rs`)

When formatting in **OS-Ready mode** (toggled with <kbd>S</kbd> in `FormatModal`), the synthesizer injects valid logical filesystem structures into the raw MFM track payload:

```text
Low-Level Format (CMD_WRITE_FLUX)
 ├── [S] FsInitMode::Blank   ──► Raw 0xE5 unformatted byte pattern across all sectors
 └── [S] FsInitMode::OsReady ──► Logical OS Boot & Root Filesystem Injection:
       ├── DOS FAT12 (PC)    ──► Sector 0 BPB (MSDOS5.0), FAT1/FAT2 with Media Desc, Root Dir, 0x55AA
       ├── Atari ST TOS      ──► TOS BPB, 16-bit Boot Checksum (sum == 0x1234), FAT1/FAT2
       ├── AmigaDOS OFS      ──► Bootblock (DOS\0, 32-bit checksum), RootBlock (880), BitmapBlock (881)
       └── Amstrad CPC       ──► CP/M standard directory catalogue (Track 0, Sec 0xC1..0xC4 -> 0xE5)
```

1. **IBM PC DOS FAT12 Generation:**
   - **Cylinder 0, Head 0, Sector 1 (LBA 0):** Full BIOS Parameter Block (BPB) with OEM `MSDOS5.0`, jump instruction `EB 3C 90`, geometry headers, media descriptor, volume ID, label `NO NAME    `, file system type `FAT12   `, and standard boot message terminated by `0x55AA`.
   - **Media Descriptors:** `0xF0` for 1.44M 3.5" HD, `0xF9` for 720K 3.5" DD & 1.2M 5.25" HD, `0xFD` for 360K 5.25" DD.
   - **FAT 1 & FAT 2 Tables:** Pre-initialized with media descriptor byte and cluster 0/1 end-of-chain markers (`[media_desc, 0xFF, 0xFF]`).
   - **Root Directory:** Initialized clean with 0x00 entries.

2. **Atari ST TOS FAT12 Generation:**
   - **LBA 0 Boot Sector:** TOS BPB with branch `0x60 0x38`, OEM `ALIGND`, 2 heads, 9 sectors/track, 5 sectors/FAT, 112 root entries, Media Descriptor `0xF9`.
   - **16-bit Boot Checksum:** Calculated such that the 16-bit big-endian sum across all 256 words equals `0x1234`, allowing Atari TOS to recognize the diskette as bootable.

3. **Commodore AmigaDOS OFS Generation (`DOS\0`):**
   - **Blocks 0 & 1 (Boot Block, 1024 bytes):** OFS type signature `DOS\0`, RootBlock pointer `880`, with 32-bit circular carry addition checksum.
   - **Block 880 (RootBlock at Cyl 40, Head 0, Sec 0):** Primary type `T_HEADER = 2`, hash table size 72, bitmap valid flag `0xFFFFFFFF`, bitmap block pointer `881`, disk name `Empty`, secondary type `ST_ROOT = 1`, and 32-bit sum-to-zero checksum.
   - **Block 881 (BitmapBlock at Cyl 40, Head 0, Sec 1):** 55 allocation longwords marking blocks 0, 1, 880, and 881 as allocated (`0` bits) and all other blocks free (`1` bits), with 32-bit sum-to-zero checksum.

4. **Amstrad CPC AMSDOS / CP/M:**
   - CP/M Data Catalogue initialized on Track 0 (sectors `0xC1..0xC4` filled with standard `0xE5` empty directory entries).

### 8.4 Tri-State Head Targeting & Pass Projections

The formatter and eraser feature a dedicated tri-state head selector toggled via <kbd>H</kbd> (`Both (Dual-Head)` ➔ `Head 0 only` ➔ `Head 1 only` ➔ `Both`):
- **Single Track (<kbd>T</kbd>):** Formats/erases Head 0, Head 1, or both heads on the targeted cylinder.
- **Custom Range (<kbd>R</kbd>):** Iterates through `start..=end` cylinders on selected heads.
- **Full Disk (<kbd>D</kbd>):** Formats/erases cylinders `00..max` on selected heads.
- **Dynamic Pass Formula:**
  $$\text{Total Passes} = (\text{End} - \text{Start} + 1) \times \text{Heads Count}$$
  Where $\text{Heads Count} = 2$ for `Both`, or $1$ for `Head 0` / `Head 1`.

### 8.5 Interactive Modal Controls & Preset Cycling

- **Preset Cycling (<kbd>P</kbd>):** Cycles active profile directly inside `FormatModal` and `EraseModal`, automatically re-configuring nominal bitrate, target RPM (300/360), standard track limits (40/80), and clamping cylinder targets.
- **Dynamic Track Count Override:** Interactive track adjustment supporting standard 40/80 tracks up to 42/84 tracks with physical cylinder tracking (<kbd>PgUp</kbd>/<kbd>PgDn</kbd> or <kbd>↑</kbd>/<kbd>↓</kbd>).
- **Read-After-Write Verify Toggle (<kbd>V</kbd>):** Switches between fast format (~35s for 80 tracks dual-head) and verified format (~70s with 1-revolution instant DPLL readback).
- **Explicit Safety Confirmation Lock (`PendingConfirmation [y/N]`):** Prompts for explicit <kbd>Y</kbd> to execute and defaults to safe abort on <kbd>N</kbd>, <kbd>Enter</kbd>, or <kbd>Esc</kbd>.

---

## 9. User Interface, Visual Indicators & Controls

The interface is divided into three primary functional zones: Top Header, Left Control Menu, and Right Diagnostic Panel.

```text
┌─ AlignTesterDiag v0.2.0-alpha ─────────────────────────────────────────────────────────────────── [ Port: COM3 ] ──┐
│ A: 500k HD    T40  H0     Flags: [-wRz-]   WP: WRITE-ENABLED     18x512  27  84         ► [READING / ANALYZING /]        │
│  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15 16 17 18                                                                   │
│ 0....+....1....+....2....+....3....+....4....+....5....+....6....+....7....+....8...                                   │
├───────────────────────────────┬─────────────────────────────────────────────────────────────────────────────────────────┤
│ Insert formatted              │ === REAL-TIME ALIGNMENT ANALYSIS & DIAGNOSTICS ===                                      │
│ diskette                      │                                                                                         │
│                               │ ► Mechanical Alignment    : 100.0%  [ ████████████████████ ]                            │
│ UNIT : Drive 0 (A:)           │ ► Requested Track Sectors : 18 sectors                                                  │
│ STAT : READ/ANALYZ /          │ ► Off-Track Sectors       : 0 (NONE (Perfect))                                          │
│ TRK0 : OFF                    │ ► CRC Integrity Check     : 100% OK (0 errors)                                          │
│ INDEX: ON                     │                                                                                         │
│ MOT  : ON                     │ --- Read Sectors Stream (Standard Mode) ---                                             │
│ WPROT: OFF                    │   T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)             │
│ RPM  : 300.1 RPM              │ ► T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)             │
│ BEEP : ON (Radar)             │                                                                                         │
│ VERB : OFF                    │ Log: Reading track 40 head 0...                                                         │
└───────────────────────────────┴─────────────────────────────────────────────────────────────────────────────────────────┘
```

### 9.1 Visual Components & Overlay Modals
1. **Top Header Banner:**
   - **Branding & Port Badge:** Clean title banner spanning top border: ` 💾 AlignTesterDiag v{VERSION} ` on the left, active Greaseweazle COM port `[ Port: {PORT_NAME} ]` on the right.
   - **Drive & Track:** Active unit (`A:` / `B:` in IBM PC mode, `DS0`..`DS3` in Shugart mode), Density (`500k HD` / `250k DD`), Cylinder (`T40`), Head (`H0`, `H1`, `HB(H0)`).
   - **Access Flags:** `Flags: [-wRz-]` (`w` = Cyan bold when write enabled; `-` = Dark gray when write protected; `R` = Recalibrate; `z` = Zero track).
   - **Write Protect Badge:** `WP: PROTECTED` (Yellow) vs. `WP: WRITE-ENABLED` (Cyan).
   - **Track Ruler (0–83):** 84-character ruler highlighting carriage travel with solid white block.
2. **Left Navigation Panel:** Hardware status flags, motor state, tachometer RPM, and shortcut legend.
3. **Right Diagnostic Stream Panel:** Real-time sector ribbons (Green OK, Red CRC, Yellow DAM, Orange Misaligned, Gray Missing) and phosphor decay animation.
4. **Interactive Help Modal (<kbd>?</kbd> / <kbd>F1</kbd>):** Full-screen keybindings and license overlay.
5. **Interactive Format & Erase Modals (<kbd>F</kbd> / <kbd>E</kbd>):** Parameter configuration with <kbd>S</kbd> (FS Init), <kbd>P</kbd> (Preset), <kbd>H</kbd> (Head), <kbd>V</kbd> (Verify), <kbd>T</kbd> (Track), <kbd>R</kbd> (Range), <kbd>D</kbd> (Disk).
6. **Custom Range Editor Modal (`RangeEditModal`):** Dual-field numeric editor with <kbd>Tab</kbd> switching, <kbd>H</kbd> head targeting, pass counter, and boundary validation.
7. **Pending Confirmation Overlay (`[y/N]`):** High-visibility safety confirmation gate.

### 9.2 Keyboard Shortcuts & Interactive Commands

#### Main Screen Keybindings

| Key / Shortcut | Function & Action Name | Target Subsystem | Detailed Technical Description |
|:---|:---|:---:|:---|
| <kbd>?</kbd> / <kbd>F1</kbd> | **Toggle Interactive Help Modal** | `UI` | Opens/closes the full-screen centered interactive help modal overlay. |
| <kbd>A</kbd> / <kbd>a</kbd> | **Real-Time Track Analysis** | `UI` & `Hardware I/O` | Starts continuous DPLL flux capture, alignment score, and acoustic variometer. |
| <kbd>D</kbd> / <kbd>d</kbd> | **Read Sector Data & CRC Test** | `Hardware I/O` | Reads and verifies sector CCITT CRC-16 integrity across cylinder. |
| <kbd>E</kbd> / <kbd>e</kbd> | **Low-Level Hardware DC Erase Modal** | `UI` & `Hardware I/O` | Opens DC Erase modal (`[T]` Track, `[R]` Range, `[D]` Disk, `[P]` Preset, `[H]` Head). |
| <kbd>F</kbd> / <kbd>f</kbd> | **Low-Level Track & Disk Formatter Modal** | `UI` & `Hardware I/O` | Opens Format modal (`[T]` Track, `[R]` Range, `[D]` Disk, `[S]` FS Mode, `[P]` Preset, `[H]` Head, `[V]` Verify). |
| <kbd>Esc</kbd> | **Safe Stop / Motor OFF** | `UI` & `Hardware I/O` | Immediately stops spindle motor, halts acquisition, dismisses active modal. |
| <kbd>Backspace</kbd> | **Emergency Panic Reset** | `Hardware I/O` | Drains queues, purges UART, cuts motor, re-initializes Greaseweazle bus. |
| <kbd>Z</kbd> / <kbd>z</kbd> | **Zero Track (Return to Cyl 00)** | `Hardware I/O` | Motor-gated seek to Track 00 with recoil dissipation delay. |
| <kbd>R</kbd> / <kbd>r</kbd> | **Recalibrate & Seek Return** | `Hardware I/O` | Seeks to Track 00 to clear backlash, then returns to origin cylinder. |
| <kbd>+</kbd> / <kbd>→</kbd> / <kbd>▲</kbd> / `ScrollUp` | **Step Track +1 (Outward)** | `Hardware I/O` | Steps head carriage outward by +1 cylinder (up to 83). |
| <kbd>-</kbd> / <kbd>←</kbd> / <kbd>▼</kbd> / `ScrollDown` | **Step Track -1 (Inward)** | `Hardware I/O` | Steps head carriage inward by -1 cylinder (down to 0). |
| <kbd>0</kbd> .. <kbd>8</kbd> | **Direct Decade Track Jump** | `Hardware I/O` | Seeks directly to Track 0, 10, 20 .. 80. |
| <kbd>9</kbd> | **Overtrack Limit Jump (Track 83)** | `Hardware I/O` | Steps carriage to physical limit (Track 83). |
| <kbd>H</kbd> / <kbd>h</kbd> | **Toggle Physical Head** | `Hardware I/O` | Cycles head selection: `Head 0` ➔ `Head 1` ➔ `Both (0+1)`. |
| <kbd>U</kbd> / <kbd>u</kbd> | **Toggle Drive Unit Selection** | `UI` & `Hardware I/O` | Switches drive unit: `A:`/`B:` (PC) or `DS0`..`DS3` (Shugart). |
| <kbd>L</kbd> / <kbd>l</kbd> | **Live RPM & Tachometer Test** | `Hardware I/O` & `UI` | Measures 72 MHz index intervals, RPM rolling average, and jitter gauge. |
| <kbd>M</kbd> / <kbd>m</kbd> | **Force Motor Toggle ON / OFF** | `Hardware I/O` | Manually asserts/negates spindle motor power line. |
| <kbd>B</kbd> / <kbd>b</kbd> | **Toggle Acoustic Variometer** | `Audio` & `UI` | Toggles dynamic pitch acoustic variometer on / off. |
| <kbd>V</kbd> / <kbd>v</kbd> | **Toggle Verbose Stream Mode** | `UI` & `Hardware I/O` | Toggles between Standard block view and Verbose timing stream. |
| <kbd>P</kbd> / <kbd>p</kbd> | **Cycle Hardware & Format Preset** | `UI` & `Hardware I/O` | Cycles presets (`Pc35Hd` ➔ `Pc35Dd` ➔ `Pc525Hd` ➔ `Pc525DdOnHd` ➔ `Pc525Dd` ➔ `Amiga35Dd` ➔ `Atari35Dd` ➔ `Cpc30Data`). |
| <kbd>S</kbd> / <kbd>s</kbd> | **Toggle Step Rate** | `UI` & `Hardware I/O` | Toggles Single (1:1) / Double (2:1) step mode for 48/96 TPI media. |
| <kbd>T</kbd> / <kbd>t</kbd> | **Toggle Bus Type** | `UI` & `Hardware I/O` | Toggles IBM PC (`0x01`) and Shugart (`0x02`) interface pinout. |
| <kbd>Q</kbd> / <kbd>X</kbd> / <kbd>Ctrl+C</kbd> | **Clean Application Exit** | `UI` & `Hardware I/O` | Cuts motor, restores raw mode, and exits gracefully. |

#### Modal-Specific Controls & Subsystem Navigation

| Context | Shortcut / Key | Function & Action |
|:---|:---|:---|
| **Format Modal (<kbd>F</kbd>)** | <kbd>S</kbd> / <kbd>s</kbd> | Toggle **System FS Init** (`Blank (Raw 0xE5)` $\leftrightarrow$ `OS-Ready (Boot & Root FS)`) |
| | <kbd>P</kbd> / <kbd>p</kbd> | Cycle active **Preset Profile** (auto-adjusts geometry, RPM, bitrate, and clamps tracks) |
| | <kbd>H</kbd> / <kbd>h</kbd> | Cycle target head (`Both (Dual-Head)` ➔ `Head 0 only` ➔ `Head 1 only` ➔ `Both`) |
| | <kbd>V</kbd> / <kbd>v</kbd> | Toggle Read-After-Write verify mode (`ON` ~70s / `OFF` ~35s) |
| | <kbd>T</kbd> / <kbd>t</kbd> | Arm single-track format for active cylinder and selected head(s) |
| | <kbd>R</kbd> / <kbd>r</kbd> | Open interactive custom track range editor (`RangeEditModal`) |
| | <kbd>D</kbd> / <kbd>d</kbd> | Arm full-disk batch format (`00..max`) for selected head(s) |
| | <kbd>+</kbd> / <kbd>-</kbd>, <kbd>[</kbd> / <kbd>]</kbd>, <kbd>←</kbd> / <kbd>→</kbd>, `Scroll` | Step active cylinder target |
| | <kbd>PgUp</kbd> / <kbd>PgDn</kbd>, <kbd>↑</kbd> / <kbd>↓</kbd> | Adjust total disk tracks (40/42 for 48 TPI, 80/84 for 96/135 TPI) |
| | <kbd>Esc</kbd> / <kbd>Q</kbd> / <kbd>X</kbd> | Close modal and cancel |
| **Erase Modal (<kbd>E</kbd>)** | <kbd>P</kbd> / <kbd>p</kbd> | Cycle active **Preset Profile** |
| | <kbd>H</kbd> / <kbd>h</kbd> | Cycle target head (`Both` ➔ `Head 0` ➔ `Head 1` ➔ `Both`) |
| | <kbd>T</kbd> / <kbd>t</kbd> | Arm single-track DC erase for active cylinder and selected head(s) |
| | <kbd>R</kbd> / <kbd>r</kbd> | Open interactive custom track range editor (`RangeEditModal`) |
| | <kbd>D</kbd> / <kbd>d</kbd> | Arm full-disk batch DC erase (`00..max`) for selected head(s) |
| | <kbd>+</kbd> / <kbd>-</kbd>, <kbd>[</kbd> / <kbd>]</kbd>, <kbd>←</kbd> / <kbd>→</kbd>, `Scroll` | Step active cylinder target |
| | <kbd>PgUp</kbd> / <kbd>PgDn</kbd>, <kbd>↑</kbd> / <kbd>↓</kbd> | Adjust total disk tracks (40/42 for 48 TPI, 80/84 for 96/135 TPI) |
| | <kbd>Esc</kbd> / <kbd>Q</kbd> / <kbd>X</kbd> | Close modal and cancel |
| **Range Editor (`RangeEditModal`)** | <kbd>Tab</kbd> | Switch active editing field (`Start Track` $\leftrightarrow$ `End Track`) |
| | <kbd>0</kbd>–<kbd>9</kbd> | Type track numeric digits directly into the active field |
| | <kbd>+</kbd> / <kbd>-</kbd>, <kbd>↑</kbd> / <kbd>↓</kbd>, <kbd>←</kbd> / <kbd>→</kbd>, `Scroll` | Increment / decrement active bound |
| | <kbd>H</kbd> / <kbd>h</kbd> | Cycle target head (`Both` ➔ `Head 0` ➔ `Head 1`, dynamic pass calculation) |
| | <kbd>Backspace</kbd> | Delete last digit or clear active field |
| | <kbd>Enter</kbd> | Validate track range bounds and arm batch execution |
| | <kbd>Esc</kbd> | Cancel range editing and return to parent modal |
| **Confirmation Prompt (`[y/N]`)** | <kbd>Y</kbd> / <kbd>y</kbd> | **Confirm and Execute:** Dispatches `HwCmd` and closes modal |
| | <kbd>N</kbd> / <kbd>n</kbd>, <kbd>Enter</kbd>, <kbd>Esc</kbd> | **Cancel Prompt:** Clears pending confirmation and returns safely to modal |

---

## 10. Automated Test & Verification Suite

AlignTesterDiag includes an exhaustive **171-test automated unit test suite** built directly into Cargo (100% success rate), maintaining strict zero-warning Clippy compliance (`cargo clippy -- -D warnings`).

### 10.1 Automated Unit Test Harness
Run the full test suite using Cargo:
```bash
cargo test
```

The 171 unit tests provide complete coverage across all subsystems:
- **Filesystem Synthesizer & OS-Ready Generation:** Valid DOS BPB generation (OEM `MSDOS5.0`, 1.44M/720K/1.2M/360K media descriptors `0xF0`/`0xF9`/`0xFD`, FAT tables, boot signature `0x55AA`), Atari ST TOS BPB and 16-bit boot checksum verification (`sum == 0x1234`), Commodore Amiga OFS Bootblock checksum calculation (32-bit circular carry), RootBlock at block 880 (hash table & checksum), BitmapBlock at block 881 (allocation map & checksum), and CP/M catalogue layout (`src/hw/fs.rs`).
- **Low-Level MFM Synthesizer & Flux Timing:** MFM track synthesis, altered sync drops `0xA1*` -> `0x4489`, CRC16-CCITT static table validation, 72 MHz pulse timing translation for 250k/300k/500k, write pre-compensation ($\pm 125\text{ ns}$ on tracks $> 40$), Greaseweazle RLE flux decoding roundtrip, and Amiga Paula even/odd encoding & decoding roundtrips (`src/hw/format.rs`).
- **Hardware DC Erase Engine (`CMD_ERASE_FLUX`):** 6-byte packet builders, write-protect Pin 28 pre-checks, and multi-track erase loop execution (`src/hw/protocol.rs`, `src/hw/mod.rs`).
- **Tri-State Head Selection & Pass Projections:** `HeadSelection` cycling (`Both` ➔ `Head 0` ➔ `Head 1` ➔ `Both`), dynamic pass projection calculation (`total_passes = range.count() * heads.len()`), and modal rendering (`src/hw/protocol.rs`, `src/app.rs`, `src/ui.rs`, `src/main.rs`).
- **Confirmation Prompts & Safety Lock:** `PendingConfirmation` formatting, prompt string generation with OS-Ready tags, and safe default abort (`src/app.rs`, `src/main.rs`).
- **Range Editor Modal Lifecycle:** `RangeEditModal` field navigation via `Tab`, numeric parsing, boundary clamping, and error validation (`src/app.rs`, `src/main.rs`).
- **Hardware & Format Presets:** Presets lifecycle, preset cycling (<kbd>P</kbd>), 360 RPM nominal target for 5.25" HD, automatic track clamping, and CLI argument parsing (`src/hw/protocol.rs`, `src/app.rs`, `src/main.rs`).
- **Drive Unit Cycling & Bus Modes:** IBM PC `0..1` and Shugart `0..3` unit transitions, automatic fallback, and pinout reconfiguration (`src/app.rs`, `src/hw/mod.rs`, `src/main.rs`).
- **Step Rate Translation:** Single 1:1 and Double 2:1 mode clamping and physical cylinder mapping (`src/hw/protocol.rs`, `src/hw/mod.rs`, `src/app.rs`, `src/main.rs`).
- **DPLL Phase Decoding & Jitter Tolerance:** Adaptive tolerance ($\pm 25\%$ DD, $\pm 10\%$ HD), noise filtering (< 1.5 µs), 300 kbps decoding for 360K on HD drives, and frequency tracking (`src/hw/mod.rs`).
- **Acoustic Variometer Evaluation:** Pitch tiers ($1500\text{--}2200\text{ Hz}$, $600\text{--}1400\text{ Hz}$, $250\text{--}500\text{ Hz}$) and mismatch tone (180 Hz) (`src/audio.rs`).
- **Spindle Tachometer:** Sub-microsecond interval conversion, rolling average windowing, and jitter computation (`src/hw/mod.rs`).
- **Multi-System Retro Decoders:** Amiga Paula even/odd bit deinterleaving & 32-bit XOR checksums, Atari ST 9/10/11 sectors, Amstrad CPC DATA/SYSTEM sector ID formats (`src/hw/mod.rs`, `src/app.rs`, `src/ui.rs`).
- **UI Rendering & Ribbon Visuals:** Segmented ribbon coloring, TrueColor phosphor decay interpolation, spinner animation, zero-allocation ruler line, and modal styling (`src/ui.rs`).

---

## 11. Technical Roadmap & Multi-System Future Support

```text
AlignTesterDiag Roadmap
├── ✅ Phase 1: Core TUI, Greaseweazle Driver, DPLL Engine & Audio Variometer
├── 🔄 Phase 2: Mechanical Diagnostics (Endurance Seek, Random Seek, Head Cleaning)
├── ✅ Phase 3: High-Precision Low-Level MFM Formatter & Synthesizer (CMD_WRITE_FLUX)
│   ├── ✅ Low-Level Track & Disk Flux Synthesis & Verification
│   ├── ✅ OS-Ready Filesystem Initialization (DOS FAT12, Atari ST TOS, AmigaDOS OFS, CP/M)
│   └── ✅ Tri-State Head Targeting (Both / Head 0 / Head 1)
└── ✅ Phase 4: Retro Multi-System Encodings (Atari ST, Amiga Paula, Amstrad CPC)
```

<details>
<summary>🕹️ <b>1. Atari ST Format Support (WD1772 MFM — 250 kbps)</b></summary>

- **Standard Geometry:** 9 sectors/track (720 KB double-sided, 360 KB single-sided).
- **Overformatted / Extended:** Support for 10 and 11 sectors/track (Twister / Fastcopy, 800 KB – 880 KB) and extended tracks (80 to 82).
- **Boot Sector Checksum:** Valid 16-bit word checksum (`sum == 0x1234`) for OS-Ready auto-booting.
</details>

<details>
<summary>🦁 <b>2. Amiga Format Support (Paula MFM — 250 kbps / 500 kbps)</b></summary>

- **Amiga Sync Word:** `0x44894489` repeated twice.
- **Split Even/Odd MFM Decoding & Encoding:** Paula writes data by splitting bytes into even and odd bit arrays across the track buffer.
- **AmigaDOS Geometry:** 11 sectors/track (512 bytes/sector, 880 KB DD / 1.76 MB HD).
- **Filesystem Structures:** BootBlock 0 & 1 with circular carry checksum, RootBlock (880), and BitmapBlock (881).
</details>

<details>
<summary>📼 <b>3. Amstrad CPC & 3-Inch Compact Floppy Drives</b></summary>

- **CPC Sector Numbering:** Data format sectors `0xC1`–`0xC9`, System format sectors `0x41`–`0x49`.
- **3-Inch Drive Geometry:** 40 tracks single/double-sided reversible.
- **Signal Handling:** Proper interpretation of the physical `READY` line on Panasonic/Matsushita mechanisms (EME-156 / EME-216).
</details>

<details>
<summary>💾 <b>4. Interactive Low-Level Formatter (Key <kbd>F</kbd>)</b></summary>

- Full track MFM pattern synthesis (`IAM 0xC2`, `IDAM 0xA1`, `DAM 0xFB`).
- Configurable fill bytes (`0xE5`, `0xF6`, `0x00`), magnetic gaps ($Gap_1$, $Gap_2$, $Gap_3$, $Gap_4$), interleave ratios (1:1, 1:2, 1:3), and cylinder skew.
- Bulk erase mode without flux transitions for complete disk degaussing.
</details>

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
│       ├── fs.rs              # OS-Ready filesystem payload synthesizer (DOS FAT12, Atari TOS, Amiga OFS, CP/M)
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

