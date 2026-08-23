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

### 1.2 Thread Communication Enums

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
| `0x07` | `CMD_READ_FLUX` | `[0x07, 0x08, 0x00, 0x00, 0x00, 0x00, rev_low, rev_high]` | Stream | Stream raw magnetic flux transition timings |
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

### 6.3 Low-Latency Audio Queue
The audio thread processes signals from `crossbeam_channel::Receiver<AudioEvent>`. Under high revolution speeds, intermediate queued beeps are drained to ensure real-time responsiveness:
```rust
while let Ok(mut event) = rx.recv() {
    while let Ok(newer) = rx.try_recv() {
        event = newer; // Drain backlog to eliminate acoustic lag
    }
    play_audio_event(event);
}
```

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

### 7.2 21-Character Visual Centering Gauge
The UI renders an intuitive centering gauge depicting deviation from the nominal 300.0 RPM target:

```text
[----|----▼----|----]  <-- Nominal Target (300.0 RPM +/- 0.5% - Light Green)
[----|---▼|----|----]  <-- Moderate Drift (297.0 RPM +/- 1.0% - Yellow)
[▼---|----|----|----]  <-- Severe Under-speed (< 295.5 RPM - Red)
[----|----|----|---▼]  <-- Severe Over-speed (> 304.5 RPM - Red)
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

### 8.3 Hardware Safeguards & Read-After-Write Verification

1. **Hardware Write-Protect Query:** Queries floppy Pin 28 (`WPROT`) before seeking or emitting flux. If asserted, the operation is immediately rejected and logged.
2. **Index-Synchronized Writing:** Emits `CMD_WRITE_FLUX` with `cue_at_index = 1` and streams RLE flux packets.
3. **Automated Verification Pass:** Immediately reads back the written track via `CMD_READ_FLUX` to verify:
   - $100\%$ presence of expected sectors.
   - $0$ CRC errors on headers and data fields.
   - Quality score $Q \ge 90\%$.
4. **Auto-Retry:** Re-attempts format up to 2 times upon verification failure before flagging a track error.

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

### 8.1 Visual Components
1. **Top Header Banner:**
   - **Branding & Port Badge:** Clean title banner spanning top border: ` 💾 AlignTesterDiag v{VERSION} ` on the left, active Greaseweazle COM port `[ Port: {PORT_NAME} ]` (e.g. `[ Port: COM3 ]`, `[ Port: /dev/ttyACM0 ]`) on the right.
   - **Drive & Track:** Active unit (`A:` / `B:` in IBM PC mode, `DS0`..`DS3` in Shugart mode), Density (`500k HD` / `250k DD`), Cylinder (`T40`), Head (`H0`, `H1`, `HB(H0)`).
   - **Access Flags:** `Flags: [-wRz-]` (`w` = Cyan bold when write enabled / WP negated; `-` = Dark gray when write protected; `R` = Recalibrate yellow bold; `z` = Zero track yellow bold).
   - **Write Protect Badge:** `WP: PROTECTED` (Yellow) vs. `WP: WRITE-ENABLED` (Cyan).
   - **Sector Map:** Numbered badges highlighting sector availability and CRC errors in red.
   - **Track Ruler (0–83):** 84-character ruler highlighting carriage travel with solid white block.
2. **Left Navigation Panel:**
   - Hardware signal states (`UNIT : Drive 0 (A:)` / `Unit 0 (DS0)`, `BUS : IBM PC / Shugart`, `TRK0`, `INDEX`, `MOT`, `WPROT`, `RPM`, `BEEP`, `VERB`).
   - Comprehensive keyboard shortcut legend.
3. **Right Diagnostic Stream Panel:**
   - **Single-Head Live Stream:** Sliding history scroll (up to 13 passes) with rotating spinner (`▸ [/] `) on the latest capture line, and TrueColor phosphor decay interpolation transitioning smoothly from bright green-white (`Rgb(210, 255, 210)`) to standard green (`Rgb(0, 180, 0)`) over ~220 ms.
   - **Dual-Head ("Both" Mode):** Dedicated 2-line fixed persistent view (Head 0 and Head 1) with active pointer `► ` indicating the head currently being sampled.
   - Real-time segmented ribbons:
     - 🟩 `■ ` (Light Green): Valid sector read with correct IDAM and CRC.
     - 🟥 `■ ` (Light Red): Sector read with CRC corruption.
     - 🟨 `■ ` (Yellow): Special mark (`NO-DAM`, `DEL-DAM`).
     - 🟧 `■ ` (Orange): Misaligned sector claiming a different track ID (`MISALIGNED`).
     - ⬜ `░ ` (Dark Gray): Missing sector.
4. **Interactive Help Modal (<kbd>?</kbd> / <kbd>F1</kbd>):**
   - Centered overlay modal dialog featuring a double-line border and dark slate background, presenting version, author attribution, and the full interactive keybinding cheat sheet.

### 8.2 Keyboard Shortcuts & Interactive Commands

All keyboard inputs are captured in raw mode via Crossterm non-blocking polling (`event::poll(Duration::from_millis(15))`) and mapped into strongly-typed `HwCmd` messages sent across the unbounded channel to the hardware thread.

| Key / Shortcut | Function & Action Name | Target Subsystem | Detailed Technical Description |
|:---|:---|:---:|:---|
| <kbd>?</kbd> / <kbd>F1</kbd> | **Toggle Interactive Help Modal** | `UI` | Opens/closes the full-screen centered interactive help modal overlay displaying all keybindings, version, author attribution, and license details. |
| <kbd>A</kbd> / <kbd>a</kbd> | **Real-Time Track Analysis** | `UI` & `Hardware I/O` | Dispatches `HwCmd::Analyze` and `Action::Analyze`. Spins up spindle motor if stopped (enforcing `SPIN_UP_DELAY_MS` 350 ms delay), samples physical `/WRTPRT` pin, sets `DisplayMode::Analyze` and `HwActivity::ReadingAnalyzing`. Initiates continuous raw flux capture (`CMD_READ_FLUX`), real-time DPLL MFM decoding, mechanical alignment percentage calculation, and acoustic variometer evaluation. |
| <kbd>D</kbd> / <kbd>d</kbd> | **Read Sector Data & CRC Test** | `Hardware I/O` | Dispatches `HwCmd::ReadData`. Purges UART buffer, asserts spindle motor power (`CMD_MOTOR 1`), queries `/WRTPRT`, and enters `DisplayMode::ReadData`. Continuously decodes IDAM/DAM headers and verifies 512-byte sector data integrity against CCITT CRC-16 checksums, updating the sector ribbon and summary metrics. |
| <kbd>Esc</kbd> | **Safe Stop / Motor OFF** | `UI` & `Hardware I/O` | Dispatches `HwCmd::Stop` and `Action::Stop` (or dismisses Help modal if open). Immediately turns off spindle motor (`CMD_MOTOR 0`), halts continuous flux capture, resets mode to `DisplayMode::None`, sets activity to `HwActivity::Stopped`, and invalidates transient sector buffers. Establishes safe mechanical state for inserting or swapping diskettes. |
| <kbd>Backspace</kbd> / `\x08` / `\u{8}` | **Emergency Panic Reset** | `Hardware I/O` | Dispatches `HwCmd::PanicReset`. Instantly drains all pending command queues in `rx_cmd`, purges all UART buffers, cuts motor power (`CMD_MOTOR 0`), deselects head (`CMD_HEAD 0`), re-initializes Greaseweazle bus (`CMD_SET_BUS_TYPE 1`), re-selects drive unit, and resets internal state to `[IDLE / READY]`. |
| <kbd>Z</kbd> / <kbd>z</kbd> | **Zero Track (Return to Cyl 00)** | `Hardware I/O` | Dispatches `HwCmd::ZeroTrack`. Performs motor-gated seek to Track 00 (`CMD_SEEK 0`) with 3000 ms optical stop timeout (`SEEK_TRK0_TIMEOUT_MS`). Enforces 200 ms mechanical shock dissipation delay (`RECALIBRATE_WAIT_MS`), resets Track 0 flag (`TRK0=ON`), re-queries write-protect pin, purges input UART buffers, and resumes active diagnostic mode if previously analyzing. |
| <kbd>R</kbd> / <kbd>r</kbd> | **Recalibrate & Seek Return** | `Hardware I/O` | Dispatches `HwCmd::RecalibrateSeek`. Saves origin cylinder, issues motor-gated seek to physical Track 00 (`CMD_SEEK 0`), pauses for 60 ms dwell time (`DWELL_TIME_TRK0_MS`) against the stop, then seeks directly back to origin cylinder with 30 ms vibration dampening (`HEAD_SETTLE_TIME_MS`). Clears lead-screw backlash and motor step slip. (If already at Track 00, performs a 2-track step-out clearance cycle `Seek(2)` $\rightarrow$ `Seek(0)`). |
| <kbd>+</kbd> / <kbd>=</kbd> / <kbd>▲ Up</kbd> / <kbd>► Right</kbd> | **Step Track +1 (Outward)** | `Hardware I/O` | Dispatches `HwCmd::Seek(track + 1)`. Steps head carriage outward by +1 cylinder (clamped at physical limit Track 83). Uses `perform_motor_gated_seek()` with electronic stepper wake-up if motor is stopped, clears prior track flux cache, enforces 30 ms head settling time (`HEAD_SETTLE_TIME_MS`), and queries `/WRTPRT`. |
| <kbd>-</kbd> / <kbd>_</kbd> / <kbd>▼ Down</kbd> / <kbd>◄ Left</kbd> | **Step Track -1 (Inward)** | `Hardware I/O` | Dispatches `HwCmd::Seek(track - 1)`. Steps head carriage inward by -1 cylinder (clamped at physical limit Track 00). Uses motor-gated stepping, enforces 30 ms damping delay (`HEAD_SETTLE_TIME_MS`), purges residual UART flux frames, and updates `TRK0` indicator if cylinder 00 is reached. |
| <kbd>0</kbd> .. <kbd>8</kbd> | **Direct Decade Track Jump** | `Hardware I/O` | Dispatches `HwCmd::Seek(digit * 10)`. Immediately seeks to cylinder 00, 10, 20, 30, 40, 50, 60, 70, or 80. Dynamically scales serial seek timeout ($T = 1200\text{ ms} + \vert\Delta T\vert \times 25\text{ ms}$), dampens mechanical carriage vibration for 30 ms, clears sector history, and re-locks DPLL stream. |
| <kbd>9</kbd> | **Overtrack Limit Jump (Track 83)** | `Hardware I/O` | Dispatches `HwCmd::Seek(90 \rightarrow \min 83)`. Steps carriage to the maximum physical overtrack boundary (Cylinder 83) to verify overtracking headroom, head carriage clearance, and upper limit mechanical stop safety. |
| <kbd>H</kbd> / <kbd>h</kbd> | **Toggle Physical Head** | `Hardware I/O` | Dispatches `HwCmd::ToggleHead`. Cycles head selection: `Head 0` (Side 0 / Lower) $\rightarrow$ `Head 1` (Side 1 / Upper) $\rightarrow$ `Both (0+1)` (Alternating Dual-Head Mode). Transmits `CMD_HEAD` (0x03), applies 1 ms electronic preamplifier settle delay (`HEAD_SWITCH_SETTLE_MS`), clears transient sector logs, and resets per-head diagnostic passes. |
| <kbd>U</kbd> / <kbd>u</kbd> | **Toggle Drive Unit Selection** | `UI` & `Hardware I/O` | Dispatches `HwCmd::ToggleDriveUnit` and `Action::ToggleDriveUnit`. In **IBM PC mode**, alternates active drive between `Drive 0 (A:)` and `Drive 1 (B:)`. In **Shugart mode**, cycles through the 4 physical units `Unit 0 (DS0)` ➔ `Unit 1 (DS1)` ➔ `Unit 2 (DS2)` ➔ `Unit 3 (DS3)`. Safely shuts down motor on old drive, deselects bus (`CMD_DESELECT 0x0D`), selects new drive unit (`CMD_SELECT 0x0C`), performs motor-gated recalibration to Track 00 (`CMD_SEEK 0`), queries write-protect status, and resets UI metrics. |
| <kbd>L</kbd> / <kbd>l</kbd> | **Live RPM & Tachometer Test** | `Hardware I/O` & `UI` | Dispatches `HwCmd::MeasureRpm`. Toggles live motor tachometer mode (`DisplayMode::RpmMeasure`, `HwActivity::MeasuringRpm`). Spins up spindle motor if stopped, continuously captures index pulse intervals over 72 MHz hardware timer, tracks instantaneous RPM, computes 10-revolution rolling average and peak-to-peak flutter/jitter, and updates the 21-slot visual centering gauge. Interruptible by any seek or mode key. |
| <kbd>M</kbd> / <kbd>m</kbd> | **Force Motor Toggle ON / OFF** | `Hardware I/O` | Dispatches `HwCmd::ToggleMotor`. Manually toggles spindle motor power (`CMD_MOTOR = 1 / 0`) with bus and unit re-assertion, preserving current cylinder, head selection, sector map, and rolling average RPM (non-destructive state toggle). |
| <kbd>B</kbd> / <kbd>b</kbd> | **Toggle Acoustic Variometer** | `Audio` & `UI` | Dispatches `HwCmd::ToggleBeep`. Toggles real-time acoustic alignment radar audio feedback. When enabled (`BEEP : ON (Radar)`), the sound worker thread synthesizes multi-tier pitch-modulated tones ($1500\text{ Hz} \le f \le 2200\text{ Hz}$ for nominal, $600\text{ Hz} \le f \le 1400\text{ Hz}$ for marginal, $250\text{ Hz} \le f \le 500\text{ Hz}$ continuous for severe, and $180\text{ Hz}$ pulsed buzz for cross-track mismatch). |
| <kbd>V</kbd> / <kbd>v</kbd> | **Toggle Verbose Stream Mode** | `UI` & `Hardware I/O` | Dispatches `HwCmd::ToggleVerbose`. Switches stream display between Standard mode (compact graphical sector blocks `[ ■ ■ ■ ]`) and Verbose History mode (detailed microsecond cell timings, DPLL phase drift, instantaneous RPM per revolution, and individual error causes). |
| <kbd>Q</kbd> / <kbd>q</kbd> / <kbd>X</kbd> / <kbd>x</kbd> / <kbd>Ctrl</kbd>+<kbd>C</kbd> | **Clean Application Exit** | `UI` & `Hardware I/O` | Dispatches `HwCmd::Exit`. Gracefully terminates background worker threads, shuts down spindle motor, deselects drive unit (`CMD_DESELECT`), tri-states interface bus (`CMD_SET_BUS_TYPE 0`), lowers DTR/RTS lines, restores terminal raw mode, and exits the application cleanly. |
| <kbd>P</kbd> / <kbd>p</kbd> | **Cycle Hardware & Format Preset** | `UI` & `Hardware I/O` | Dispatches `HwCmd::CyclePreset` and `Action::CyclePreset`. Atomically cycles through standard hardware & format presets: `Pc35Hd` (3.5" HD, 1.44M, PC Bus, Step 1:1, 500 kbps @ 300 RPM) ➔ `Pc35Dd` (3.5" DD, 720K, PC Bus, Step 1:1, 250 kbps @ 300 RPM) ➔ `Pc525Hd` (5.25" HD, 1.2M, PC Bus, Step 1:1, 500 kbps @ 360 RPM) ➔ `Pc525DdOnHd` (5.25" DD on HD Drive, 360K, PC Bus, Step 2:1, DPLL 300 kbps @ 360 RPM) ➔ `Pc525Dd` (5.25" DD on DD Drive, 360K, PC Bus, Step 1:1, 250 kbps @ 300 RPM) ➔ `Amiga35Dd` (Amiga 3.5", 880K, Shugart Bus, Step 1:1, 250 kbps @ 300 RPM) ➔ `Atari35Dd` (Atari 3.5", 720K, PC Bus, Step 1:1, 250 kbps @ 300 RPM) ➔ `Cpc30Data` (Amstrad CPC 3.0", 178K, Shugart Bus, Step 1:1, 250 kbps @ 300 RPM). Automatically syncs DPLL nominal window, bus mode, and step rate. |
| <kbd>F</kbd> / <kbd>f</kbd> | **Low-Level Track & Disk Formatter** | `UI` & `Hardware I/O` | Opens the Low-Level Format Confirmation Modal. Dispatches `HwCmd::FormatTrack` on <kbd>T</kbd> (formats current track only) or `HwCmd::FormatDisk` on <kbd>D</kbd> (formats entire disk cylinder-by-cylinder across both heads) using cycle-accurate 72 MHz pulse synthesis, write pre-compensation, physical index cueing, and read-after-write CRC verification. |
| <kbd>I</kbd> / <kbd>i</kbd> | **Track Flux Imaging Utility** | `Hardware I/O` *(Reserved)* | Reserved shortcut for raw multi-revolution flux imaging and flux-level surface degradation heatmaps in Roadmap Phase 4. |
| <kbd>S</kbd> / <kbd>s</kbd> | **Toggle Step Rate (Single 1:1 / Double 2:1 for 48/96 TPI)** | `UI` & `Hardware I/O` | Dispatches `HwCmd::ToggleStepMode` and `Action::ToggleStepMode`. Alternates head carriage stepping rate between Single Step 1:1 (native 96/135 TPI drives, 0..83 cylinders) and Double Step 2:1 (48 TPI media on 96/135 TPI mechanics, multiplying physical seek cylinders by 2 to map 40-41 logical tracks T00..T40 onto physical cylinders 0..82). Automatically bounds active track to the maximum logical limit (83 in Single, 41 in Double). |
| <kbd>T</kbd> / <kbd>t</kbd> | **Toggle Bus Type (IBM PC <-> Shugart)** | `UI` & `Hardware I/O` | Dispatches `HwCmd::ToggleBusType` and `Action::ToggleBusType`. Toggles floppy interface between IBM PC bus (`0x01`) and Shugart standard bus (`0x02`, e.g. Amiga / Atari / Commodore / CPC native drives). Dynamically updates pinout configuration and unit drive selection (automatically resetting the active unit to 0 if switching back to PC mode while on DS2 or DS3). |
| <kbd>W</kbd> / <kbd>w</kbd> | **Write Sector Integrity Test** | `Hardware I/O` *(Reserved)* | Reserved shortcut for non-destructive sector rewrite and magnetic surface test patterns in Roadmap Phase 3. |

---

## 10. Automated Test & Verification Suite

AlignTesterDiag includes an exhaustive **157-test automated unit test suite** built directly into Cargo (100% success rate).

### 10.1 Automated Unit Test Harness
Run the full test suite using Cargo:
```bash
cargo test
```

The 157 unit tests provide complete coverage across all subsystems:
- Low-Level MFM track synthesizer, altered sync drops `0xA1*` -> `0x4489`, CRC16-CCITT static table validation, 72 MHz pulse timing translation for 250k/300k/500k, write pre-compensation ($\pm 125\text{ ns}$ on tracks $> 40$), Greaseweazle RLE flux decoding roundtrip, and Amiga Paula even/odd encoding & decoding roundtrips (`src/hw/format.rs`).
- State transitions on `HwCmd` and `Action` dispatches (`src/app.rs`, `src/hw/mod.rs`).
- Hardware & format presets lifecycle, cycling, and CLI argument parsing (`src/hw/protocol.rs`, `src/app.rs`, `src/main.rs`).
- Dynamic drive unit cycling across bus types (`0..1` for IBM PC, `0..3` for Shugart) and automatic unit fallback (`src/app.rs`, `src/hw/mod.rs`, `src/main.rs`).
- Step rate / Double-step mode translation, track bounds clamping, and physical cylinder calculation (`src/hw/protocol.rs`, `src/hw/mod.rs`, `src/app.rs`, `src/main.rs`).
- DPLL adaptive tolerance ($\pm 25\%$ on DD rates, $\pm 10\%$ on HD rates), parasitic noise filtering (< 1.5 µs), 300 kbps decoding for 360K on HD drives, RMS phase jitter, and frequency adaptation (`src/hw/mod.rs`).
- Multi-tier variometer pitch bounds ($1500 \text{ Hz} \le f \le 2200 \text{ Hz}$, $600 \text{ Hz} \le f \le 1400 \text{ Hz}$, $250 \text{ Hz} \le f \le 500 \text{ Hz}$) and mismatch warning tone (180 Hz) (`src/audio.rs`).
- Spindle tachometer rolling average windowing, sub-microsecond interval conversion, and jitter calculations (`src/hw/mod.rs`).
- Multi-system retro encoding & decoding engines (Amiga Paula even/odd bit deinterleaving & 32-bit XOR checksums, Atari ST 9/10/11 sectors & overtracks, Amstrad CPC DATA/SYSTEM sector ID formats) (`src/hw/mod.rs`, `src/app.rs`, `src/ui.rs`).
- Dual-head "Both" mode consolidation, active pointer tracking, and mismatch handling (`src/app.rs`, `src/hw/mod.rs`, `src/ui.rs`).
- Format confirmation modal formatting, shortcut styling (`[T]` / `[D]` / `[Esc]`), progress bar calculation, and right panel format diagnostics (`src/ui.rs`, `src/main.rs`).
- TUI ribbon span coloring, phosphor decay interpolation, spinner animation, zero-allocation ruler line, and dynamic text generation (`src/ui.rs`).
- Hardware protocol packet encoders, bus modes, step modes, and opcode definitions (`src/hw/protocol.rs`).
- CLI argument parsing (`--preset`, `-p`, `--port`, `--drive`, `--bus`, `--step`, `--double-step`, `--shugart`, short & key-value syntax) and auto-detection fallback (`src/main.rs`).

---

## 11. Technical Roadmap & Multi-System Future Support

```text
AlignTesterDiag Roadmap
├── ✅ Phase 1: Core TUI, Greaseweazle Driver, DPLL Engine & Audio Variometer
├── 🔄 Phase 2: Mechanical Diagnostics (Endurance Seek, Random Seek, Head Cleaning)
├── ✅ Phase 3: High-Precision Low-Level MFM Formatter & Synthesizer (CMD_WRITE_FLUX)
└── ✅ Phase 4: Retro Multi-System Encodings (Atari ST, Amiga Paula, Amstrad CPC)
```

<details>
<summary>🕹️ <b>1. Atari ST Format Support (WD1772 MFM — 250 kbps)</b></summary>

- **Standard Geometry:** 9 sectors/track (720 KB double-sided, 360 KB single-sided).
- **Overformatted / Extended:** Support for 10 and 11 sectors/track (Twister / Fastcopy, 800 KB – 880 KB) and extended tracks (80 to 82).
- **Timing:** Adaptation for short $Gap_3$ formats.
</details>

<details>
<summary>🦁 <b>2. Amiga Format Support (Paula MFM — 250 kbps / 500 kbps)</b></summary>

- **Amiga Sync Word:** `0x44894489` repeated twice.
- **Split Even/Odd MFM Decoding:** Paula writes data by splitting bytes into even and odd bit arrays across the track buffer. The decoder reconstructs bytes via interleaved bitwise OR.
- **AmigaDOS Geometry:** 11 sectors/track (512 bytes/sector, 880 KB DD / 1.76 MB HD).
- **Checksum:** 32-bit odd/even XOR checksum calculation.
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

## 📄 License & Credits


- Copyright (C) 2026 Mr JeAn-FReD 🇫🇷
- **Heritage:** Inspired by Dave Dunfield's **ImageDisk (`IMD`)** and Keir Fraser's **Greaseweazle**.
- **License:** Distributed under the terms of the [GNU General Public License v3.0 (GPL-3.0)](LICENSE).
