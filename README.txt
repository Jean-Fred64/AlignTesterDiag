======================================================================
  AlignTesterDiag - Floppy Drive Alignment & Diagnostic Tool
  Version: 0.2.0-alpha (Windows x64)
  Author: MonSieur JeAn-FReD
======================================================================

PREREQUISITES & HARDWARE
----------------------------------------------------------------------
1. Greaseweazle v4 or v4.1 hardware connected via USB.
2. 3.5", 5.25", or 26-pin slim laptop floppy drive properly cabled and powered.
3. A known-good, formatted standard test diskette (720K DD or 1.44M HD).
4. Recommended terminal: Windows Terminal, PowerShell, or standard cmd.exe.

--------------------------------------------------------------------------------
1. OVERVIEW
--------------------------------------------------------------------------------
AlignTesterDiag is a high-performance, non-blocking, modern terminal user
interface (TUI) diagnostics and calibration platform for floppy disk drives.
Interfacing directly with a Greaseweazle USB flux controller, it provides:
- Real-time MFM sector decoding via software DPLL.
- Live mechanical alignment percentage radar.
- Glider-inspired dynamic acoustic variometer with multi-tier pitch modulation.
- Sub-microsecond 72 MHz spindle tachometer with RPM jitter & centering gauge.
- Multi-system retro support: IBM PC, Amiga DOS, Atari ST, Amstrad CPC.
- Universal bus mode support: IBM PC (Drive A:/B:) & Shugart (DS0..DS3).

2. QUICK START
--------------------------------------------------------------------------------
A. Pre-compiled Windows Binary:
   1. Extract 'aligntester-diag-windows-x64.zip'.
   2. Open Windows Terminal, PowerShell, or Command Prompt.
   3. Run:
      .\aligntester-diag.exe

   Or specify port and drive unit explicitly:
      .\aligntester-diag.exe COM3 --drive 0

   For native Amiga / Atari / Shugart drives:
      .\aligntester-diag.exe COM3 --shugart --drive 0

B. Build from Source (Cargo):
   cargo build --release

   Run with auto-detection:
      cargo run --release

   Run with custom parameters:
      cargo run --release -- COM3 --drive 0
      cargo run --release -- /dev/ttyACM0 --drive 1
      cargo run --release -- --shugart --drive 0

--------------------------------------------------------------------------------
3. COMMAND-LINE OPTIONS & SYNTAX
--------------------------------------------------------------------------------
Usage: aligntester-diag [PORT] [OPTIONS]

ARGUMENTS:
  [PORT]                  Serial port connected to Greaseweazle (e.g. COM3,
                          /dev/ttyACM0). Auto-detected if omitted.

OPTIONS:
  -d, --drive <0-3>          Select target drive unit:
                             - 0..1 for IBM PC mode (0 = Drive A:, 1 = Drive B:)
                             - 0..3 for Shugart mode (DS0, DS1, DS2, DS3)
                             [default: 0]
      --drive=<0-3>          Alternative key-value syntax for drive unit
  -b, --bus <pc|shugart>     Select floppy interface bus type (pc | shugart)
                             [default: pc]
      --bus=<pc|shugart>     Alternative key-value syntax for bus type
      --shugart              Shorthand flag for Shugart bus mode (Amiga straight cable)
  -s, --step <single|double> Select step mode (single 1:1 for 96/135 TPI | double 2:1 for 48 TPI)
                             [default: single]
      --step=<single|double> Alternative key-value syntax for step mode
      --double-step          Shorthand flag for Double Step 2:1 mode (48 TPI media)
  -p, --port <PORT>          Serial port connected to Greaseweazle
  -h, --help                 Print help information
  -v, -V, --version          Print version information

--------------------------------------------------------------------------------
4. INTERACTIVE KEYBINDINGS CHEAT SHEET
--------------------------------------------------------------------------------
Key / Shortcut       Action & Technical Description
------------------   -----------------------------------------------------------
? / F1               Help Modal       : Toggle full-screen help modal overlay
A                    Analyze          : Continuous real-time track alignment test
D                    Read Data        : Read and verify sector CCITT CRC-16 integrity
L                    Live RPM         : 72 MHz spindle tachometer & jitter stability
P                    Format Profile   : Cycle machine format (Auto, PC, Amiga, Atari, CPC)
S                    Step Rate        : Toggle Single (1:1) / Double (2:1) step mode (48/96 TPI)
T                    Toggle Bus Type  : Switch bus mode: IBM PC (0x01) <-> Shugart (0x02)
                                        (auto-resets unit to 0 when entering PC mode from DS2/DS3)
B                    Audio Radar      : Toggle dynamic pitch acoustic variometer ON/OFF
H                    Toggle Head      : Cycle head: Head 0 -> Head 1 -> BOTH (0+1)
U                    Toggle Drive     : In IBM PC mode : Drive 0 (A:) <-> Drive 1 (B:)
                                        In Shugart mode: Unit 0 (DS0) -> Unit 1 (DS1) ->
                                                         Unit 2 (DS2) -> Unit 3 (DS3)
M                    Toggle Motor     : Manually assert/negate spindle motor power
V                    Toggle Verbose   : Switch Standard / Verbose history stream
R                    Recalibrate      : Recalibrate carriage to Track 0 and return
Z                    Zero Track       : Direct single seek step return to Track 0
+ / = / Up / Right   Step Forward     : Step head carriage +1 track (up to Track 83)
- / _ / Down / Left  Step Backward    : Step head carriage -1 track (down to Track 0)
0 .. 8               Decade Seek      : Direct seek jump to Track 0, 10, 20 .. 80
9                    Overtrack Seek   : Direct seek jump to physical limit (Track 83)
Esc                  Stop             : Stop spindle motor, flush buffers, dismiss help
Backspace            Panic Reset      : Emergency instant motor cut & bus re-init
Q / X / Ctrl+C       Exit             : Clean shutdown (cuts motor & exits raw mode)

--------------------------------------------------------------------------------
5. ACOUSTIC VARIOMETER TIER MAPPING
--------------------------------------------------------------------------------
- Nominal Alignment (>= 95%): 1500 Hz – 2200 Hz (High clean tone)
- Marginal Tracking (70% - 94%): 600 Hz – 1400 Hz (Medium tone)
- Severe Misalignment (< 70%): 250 Hz – 500 Hz (Low continuous tone)
- Track Mismatch (Divergence): 180 Hz pulsed warning buzz (2x 50 ms)
- Zero Decoded Sectors: 150 Hz warning hum (40 ms)

--------------------------------------------------------------------------------
6. AUTOMATED TEST SUITE
--------------------------------------------------------------------------------
AlignTesterDiag features an automated test suite with 132 unit tests (100% pass rate).
Run with:
  cargo test

--------------------------------------------------------------------------------
7. REPOSITORY & DOCUMENTATION
--------------------------------------------------------------------------------
GitHub Repository: https://github.com/Jean-Fred64/AlignTesterDiag
Full Documentation: See documentation.md
License: GNU General Public License v3.0 (See LICENSE.txt)
Copyright (C) 2026 MonSieur JeAn-FReD
