use crossbeam_channel::{Receiver, Sender};
use serialport::SerialPortType;
use std::{
    collections::{HashMap, VecDeque},
    io::Write,
    thread,
    time::{Duration, Instant},
};
use crate::app::{DiagnosticPass, HeadSelection};
use crate::audio::{evaluate_alignment_audio_event, sound_worker, AudioEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayMode {
    None,
    Analyze,
    ReadData,
    RpmMeasure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HwActivity {
    WaitingPort,
    Stopped,
    Seeking,
    ReadingAnalyzing,
    MeasuringRpm,
    Idle,
}

/// Hardware flux reading status for diskette presence & index resilience
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DriveReadStatus {
    #[default]
    Ok,
    NoDiskOrNoIndex,
    IoError,
    Aborted,
}

/// Minimal electronic head switch settle time (1 ms) for SIDE1 selection.
pub const HEAD_SWITCH_SETTLE_MS: u64 = 1;
/// Stepper motor driver electronic wake-up delay (15 ms) for 26-pin FFC drives (e.g. TEAC FD-05HG).
pub const STEPPER_WAKEUP_DELAY_MS: u64 = 15;
/// Electromechanical Head Settle Time (15 ms to 30 ms) after track stepping.
/// Standard floppy disk controller specification requires 15-30 ms for physical head carriage vibration dampening.
pub const HEAD_SETTLE_TIME_MS: u64 = 30;
/// Nominal serial read timeout (1000 ms) for USB flux capture margin.
pub const DEFAULT_SERIAL_TIMEOUT_MS: u64 = 1000;
/// Motor spin-up delay (350 ms) before reading flux to allow spindle to reach 300 RPM and stabilize index.
pub const SPIN_UP_DELAY_MS: u64 = 350;
pub const SYNC_DELAY_MS: u64 = 30;

/// Dwell time at track 0 during recalibration before stepping back to original track.
pub const DWELL_TIME_TRK0_MS: u64 = 60;
/// Guaranteed fixed timeout for SEEK_TRACK0 (optical stop return).
pub const SEEK_TRK0_TIMEOUT_MS: u64 = 3000;

/// Recalibrate / Track 0 stabilization delay: 1 full disk revolution (~200 ms @ 300 RPM / 166.7 ms @ 360 RPM).
/// Allows the mechanical end-stop shock to dissipate and spindle index to stabilize before MFM / PLL decoding.
pub const RECALIBRATE_WAIT_MS: u64 = 200;

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct SectorInfo {
    pub track: u8,
    pub sec_id: u8,
    pub size_code: u8,
    pub status_code: u8,
    pub crc_ok: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RpmMeasurement {
    pub instant_rpm: f64,
    pub min_rpm: f64,
    pub max_rpm: f64,
    pub avg_rpm: f64,
    pub jitter_rpm: f64,
    pub jitter_pct: f64,
    pub sample_count: u32,
    pub recent_samples: Vec<(f64, u32)>,
    pub rolling_window: VecDeque<f64>,
}

impl Default for RpmMeasurement {
    fn default() -> Self {
        Self {
            instant_rpm: 0.0,
            min_rpm: 0.0,
            max_rpm: 0.0,
            avg_rpm: 0.0,
            jitter_rpm: 0.0,
            jitter_pct: 0.0,
            sample_count: 0,
            recent_samples: Vec::new(),
            rolling_window: VecDeque::with_capacity(10),
        }
    }
}

impl RpmMeasurement {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_sample(&mut self, rpm: f64, delta_ticks: u32) {
        self.instant_rpm = (rpm * 10.0).round() / 10.0;
        if self.sample_count == 0 {
            self.min_rpm = self.instant_rpm;
            self.max_rpm = self.instant_rpm;
        } else {
            if self.instant_rpm < self.min_rpm {
                self.min_rpm = self.instant_rpm;
            }
            if self.instant_rpm > self.max_rpm {
                self.max_rpm = self.instant_rpm;
            }
        }

        // Rolling average (window of 10 revolutions) to filter micro-vibrations and flutter
        if self.rolling_window.len() >= 10 {
            self.rolling_window.pop_front();
        }
        self.rolling_window.push_back(self.instant_rpm);
        let sum: f64 = self.rolling_window.iter().sum();
        self.avg_rpm = (sum / (self.rolling_window.len() as f64) * 10.0).round() / 10.0;

        self.jitter_rpm = ((self.max_rpm - self.min_rpm) / 2.0 * 10.0).round() / 10.0;
        self.jitter_pct = if self.avg_rpm > 0.0 {
            ((self.max_rpm - self.min_rpm) / (2.0 * self.avg_rpm)) * 100.0
        } else {
            0.0
        };
        self.sample_count += 1;

        if self.recent_samples.len() >= 20 {
            self.recent_samples.remove(0);
        }
        self.recent_samples.push((self.instant_rpm, delta_ticks));
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Debug)]
pub struct DriveStatus {
    pub trk0: bool,
    pub index: bool,
    pub rpm: u32,
    pub rpm_display: String,
    pub rpm_measure: RpmMeasurement,
    pub track: u8,
    pub target_track: u8,
    pub head_select: HeadSelection,
    pub head: u8,
    pub motor_on: bool,
    pub drive_select: bool,
    pub drive_unit: u8,
    pub unit_id: u8,
    pub write_protect: bool,
    pub write_protected: bool,
    pub density: bool,
    pub bitrate: u16,
    pub sector_count: u8,
    pub sectors_known: bool,
    pub has_disk: bool,
    pub connected: bool,
    pub analyzing: bool,
    pub verbose_mode: bool,
    pub beep_enabled: bool,
    pub mode: DisplayMode,
    pub activity: HwActivity,
    pub io_cycle: u64,
    pub log_msg: String,
    pub sectors: Vec<SectorInfo>,
    pub sector_log: Vec<String>,
    pub sector_log_standard: Vec<String>,
    pub sector_log_verbose: Vec<String>,
    pub on_track_count: u32,
    pub off_track_count: u32,
    pub off_track_details: String,
    pub crc_err_count: u32,
    pub alignment_pct: f32,
    pub in_progress_pass: bool,
    pub last_pass_h0: Option<DiagnosticPass>,
    pub last_pass_h1: Option<DiagnosticPass>,
    pub read_status: DriveReadStatus,
    pub port_name: String,
}

impl Default for DriveStatus {
    fn default() -> Self {
        Self {
            trk0: true,
            index: false,
            rpm: 0,
            rpm_display: String::from("... RPM"),
            rpm_measure: RpmMeasurement::default(),
            track: 0,
            target_track: 0,
            head_select: HeadSelection::Head0,
            head: 0,
            motor_on: false,
            drive_select: false,
            drive_unit: 0,
            unit_id: 0,
            write_protect: true,
            write_protected: true,
            density: true,
            bitrate: 500,
            sector_count: 18,
            sectors_known: false,
            has_disk: true,
            connected: false,
            analyzing: false,
            verbose_mode: false,
            beep_enabled: false,
            mode: DisplayMode::None,
            activity: HwActivity::WaitingPort,
            io_cycle: 0,
            log_msg: String::from("Connecting to Greaseweazle..."),
            sectors: Vec::new(),
            sector_log: Vec::new(),
            sector_log_standard: Vec::new(),
            sector_log_verbose: Vec::new(),
            on_track_count: 0,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            in_progress_pass: false,
            last_pass_h0: None,
            last_pass_h1: None,
            read_status: DriveReadStatus::Ok,
            port_name: String::new(),
        }
    }
}

impl DriveStatus {
    pub fn display_mode(&self) -> DisplayMode {
        self.mode
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
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
    Exit,
}

fn find_greaseweazle() -> Option<String> {
    if let Ok(ports) = serialport::available_ports() {
        for p in ports {
            if let SerialPortType::UsbPort(info) = &p.port_type {
                if info.vid == 0x1209 && (info.pid == 0x4d22 || info.pid == 0x4d69) {
                    return Some(p.port_name);
                }
            }
        }
    }

    for port in &["COM2", "COM10", "/dev/ttyACM0", "/dev/ttyS2"] {
        if let Ok(mut p) = serialport::new(*port, 115_200)
            .timeout(Duration::from_millis(100))
            .open()
        {
            let _ = p.write_data_terminal_ready(true);
            let _ = p.write_request_to_send(true);
            return Some(port.to_string());
        }
    }

    None
}

/// Reads exact number of bytes from serial port with non-blocking chunk reading and global timeout
fn safe_read_exact(
    port: &mut Box<dyn serialport::SerialPort>,
    buf: &mut [u8],
    timeout: Duration,
) -> Result<(), std::io::Error> {
    let start = Instant::now();
    let mut total_read = 0;
    while total_read < buf.len() {
        if start.elapsed() > timeout {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Timeout during safe_read_exact",
            ));
        }
        match port.read(&mut buf[total_read..]) {
            Ok(n) if n > 0 => total_read += n,
            Ok(_) => thread::sleep(Duration::from_millis(1)),
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn gw_send_raw_timeout(
    port: &mut Box<dyn serialport::SerialPort>,
    cmd: &[u8],
    extra_read: usize,
    timeout: Duration,
) -> Result<(u8, Vec<u8>), Box<dyn std::error::Error>> {
    let _ = port.clear(serialport::ClearBuffer::Input);
    port.write_all(cmd)?;
    port.flush()?;
    let mut ack = [0u8; 2];
    safe_read_exact(port, &mut ack, timeout)?;

    let mut extra = Vec::new();
    if ack[1] == 0 && extra_read > 0 {
        extra.resize(extra_read, 0u8);
        safe_read_exact(port, &mut extra, timeout)?;
    }

    Ok((ack[1], extra))
}

fn gw_send_raw(
    port: &mut Box<dyn serialport::SerialPort>,
    cmd: &[u8],
    extra_read: usize,
) -> Result<(u8, Vec<u8>), Box<dyn std::error::Error>> {
    gw_send_raw_timeout(port, cmd, extra_read, Duration::from_millis(100))
}

fn ensure_unit_active(
    port: &mut Box<dyn serialport::SerialPort>,
    unit: u8,
    motor_on: bool,
    head: u8,
) {
    let _ = gw_send_raw(port, &[0x0E, 0x03, 0x01], 0);
    let _ = gw_send_raw(port, &[0x0C, 0x03, unit], 0);
    let _ = gw_send_raw(port, &[0x03, 0x03, head], 0);
    if motor_on {
        let _ = gw_send_raw(port, &[0x06, 0x04, unit, 0x01], 0);
    }
}

fn gw_send_timeout(
    port: &mut Box<dyn serialport::SerialPort>,
    cmd: &[u8],
    extra_read: usize,
    unit: u8,
    motor_on: bool,
    head: u8,
    timeout: Duration,
) -> Result<(u8, Vec<u8>), Box<dyn std::error::Error>> {
    let res = gw_send_raw_timeout(port, cmd, extra_read, timeout);
    match res {
        Ok((7, _)) => {
            // Greaseweazle ACK_BUSY (code 7) -> wait 30 ms, ensure unit active, retry once
            thread::sleep(Duration::from_millis(30));
            ensure_unit_active(port, unit, motor_on, head);
            gw_send_raw_timeout(port, cmd, extra_read, timeout)
        }
        other => other,
    }
}

fn gw_send(
    port: &mut Box<dyn serialport::SerialPort>,
    cmd: &[u8],
    extra_read: usize,
    unit: u8,
    motor_on: bool,
    head: u8,
) -> Result<(u8, Vec<u8>), Box<dyn std::error::Error>> {
    gw_send_timeout(port, cmd, extra_read, unit, motor_on, head, Duration::from_millis(100))
}

/// Performs a motor-gated seek operation.
/// On 26-pin FFC drives (e.g. TEAC FD-05HG with adapter), the stepper motor driver IC is powered down
/// when the spindle motor signal is negated. If motor_on is false, this helper momentarily asserts
/// motor power, waits STEPPER_WAKEUP_DELAY_MS (15 ms) for electronic power stabilization, executes
/// the seek, and then de-asserts motor power.
/// If motor_on is already true (active modes like Analyze, ReadData, Live RPM), no extra packet or delay is added.
pub fn perform_motor_gated_seek(
    port: &mut Box<dyn serialport::SerialPort>,
    unit: u8,
    target_track: u8,
    motor_on: bool,
    head: u8,
    timeout: Duration,
) -> Result<(u8, Vec<u8>), Box<dyn std::error::Error>> {
    let gated = !motor_on;
    if gated {
        let _ = gw_send_raw(port, &[0x06, 0x04, unit, 0x01], 0);
        thread::sleep(Duration::from_millis(STEPPER_WAKEUP_DELAY_MS));
    }

    let res = gw_send_timeout(
        port,
        &[0x02, 0x03, target_track],
        0,
        unit,
        true,
        head,
        timeout,
    );

    if gated {
        let _ = gw_send_raw(port, &[0x06, 0x04, unit, 0x00], 0);
    }

    res
}

/// Queries Write Protect hardware pin (Pin 28 / /WRTPRT) via Greaseweazle Cmd::GetPin (0x14)
/// Floppy pin 28 is active-low: 0 = asserted (write protected), 1 = negated (write enabled)
pub fn query_write_protect(
    port: &mut Box<dyn serialport::SerialPort>,
    unit: u8,
    motor_on: bool,
    head: u8,
) -> Option<bool> {
    match gw_send_timeout(
        port,
        &[0x14, 0x03, 28],
        1,
        unit,
        motor_on,
        head,
        Duration::from_millis(100),
    ) {
        Ok((0, extra)) if !extra.is_empty() => Some(extra[0] == 0),
        _ => None,
    }
}

/// Sets spindle motor state on the specified drive unit
pub fn gw_set_motor(
    port: &mut Box<dyn serialport::SerialPort>,
    unit: u8,
    motor_on: bool,
) -> Result<(u8, Vec<u8>), Box<dyn std::error::Error>> {
    let state = if motor_on { 0x01 } else { 0x00 };
    gw_send_raw_timeout(port, &[0x06, 0x04, unit, state], 0, Duration::from_millis(100))
}

/// Reads raw flux stream directly from Greaseweazle via CMD_READ_FLUX (0x07) without intrusive pre-checks
pub fn gw_read_flux(
    port: &mut Box<dyn serialport::SerialPort>,
    revs: u16,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let _ = port.clear(serialport::ClearBuffer::All);
    let _ = port.set_timeout(Duration::from_millis(DEFAULT_SERIAL_TIMEOUT_MS));

    let b_revs = revs.to_le_bytes();
    let read_cmd = [0x07, 0x08, 0x00, 0x00, 0x00, 0x00, b_revs[0], b_revs[1]];
    port.write_all(&read_cmd)?;
    port.flush()?;

    let mut ack = [0u8; 2];
    safe_read_exact(port, &mut ack, Duration::from_millis(DEFAULT_SERIAL_TIMEOUT_MS))?;

    if ack[1] == 7 {
        // Greaseweazle ACK_BUSY (code 7) -> wait 30 ms, retry once
        thread::sleep(Duration::from_millis(30));
        let _ = port.clear(serialport::ClearBuffer::All);
        port.write_all(&read_cmd)?;
        port.flush()?;
        safe_read_exact(port, &mut ack, Duration::from_millis(DEFAULT_SERIAL_TIMEOUT_MS))?;
    }

    if ack[1] != 0 {
        return Ok(Vec::new());
    }

    let mut dat = Vec::with_capacity(131072);
    let mut tmp_buf = [0u8; 4096];
    let start = Instant::now();

    // Standard serial timeout margin to never cut valid packets
    while start.elapsed() < Duration::from_millis(DEFAULT_SERIAL_TIMEOUT_MS) {
        match port.read(&mut tmp_buf) {
            Ok(n) if n > 0 => {
                dat.extend_from_slice(&tmp_buf[..n]);
                if !dat.is_empty() && dat[dat.len() - 1] == 0 {
                    break;
                }
            }
            Ok(_) => thread::sleep(Duration::from_millis(1)),
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                if !dat.is_empty() {
                    break;
                }
                thread::sleep(Duration::from_millis(1));
            }
            Err(_) => break,
        }
    }

    // GetFluxStatus (0x09)
    let _ = port.write_all(&[0x09, 0x02]);
    let _ = port.flush();
    let mut status_ack = [0u8; 2];
    let _ = safe_read_exact(port, &mut status_ack, Duration::from_millis(50));

    Ok(dat)
}

fn shutdown_drive(port: &mut Box<dyn serialport::SerialPort>, unit: u8) {
    // 1. SET_MOTOR OFF
    let _ = gw_send_raw(port, &[0x06, 0x04, unit, 0x00], 0);
    // 2. DESELECT_UNIT / RELEASE BUS
    let _ = gw_send_raw(port, &[0x0C, 0x03, 0xFF], 0);
    let _ = gw_send_raw(port, &[0x0D, 0x02], 0);
    // 3. SET_BUS OFF / Tri-state buffer
    let _ = gw_send_raw(port, &[0x0E, 0x03, 0x00], 0);
    // Guarantee bytes physically leave UART buffer
    let _ = port.flush();
    // Lower control lines DTR / RTS
    let _ = port.write_data_terminal_ready(false);
    let _ = port.write_request_to_send(false);
}

fn decode_byte_word(w: u16) -> u8 {
    let index = (w & 0x5555) as usize;
    let y = (index + (index >> 1)) & 0x3333;
    let y = (y + (y >> 2)) & 0x0F0F;
    let y = (y + (y >> 4)) & 0x00FF;
    y as u8
}

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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorStatus {
    Ok,
    CrcData,
    CrcId,
    NoDam,
    DelDam,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedSector {
    pub cyl: u8,
    pub head: u8,
    pub sec_id: u8,
    pub size_code: u8,
    pub crc_ok: bool,
    pub status: SectorStatus,
}

impl Default for DecodedSector {
    fn default() -> Self {
        Self {
            cyl: 0,
            head: 0,
            sec_id: 1,
            size_code: 2,
            crc_ok: true,
            status: SectorStatus::Ok,
        }
    }
}

#[allow(dead_code)]
impl DecodedSector {
    pub fn new(cyl: u8, head: u8, sec_id: u8, size_code: u8, crc_ok: bool) -> Self {
        Self {
            cyl,
            head,
            sec_id,
            size_code,
            crc_ok,
            status: if crc_ok {
                SectorStatus::Ok
            } else {
                SectorStatus::CrcData
            },
        }
    }

    pub fn with_status(cyl: u8, head: u8, sec_id: u8, size_code: u8, status: SectorStatus) -> Self {
        let crc_ok = matches!(status, SectorStatus::Ok | SectorStatus::DelDam);
        Self {
            cyl,
            head,
            sec_id,
            size_code,
            crc_ok,
            status,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecodedGwFlux {
    pub flux: Vec<u32>,
    pub index_timestamps: Vec<u32>,
}

/// Decodes Greaseweazle v4 binary packets into flux intervals (ticks) and index timestamps
pub fn decode_gw_flux_with_index(dat: &[u8]) -> DecodedGwFlux {
    let mut flux = Vec::with_capacity(dat.len());
    let mut index_timestamps = Vec::new();
    let mut ticks: u32 = 0;
    let mut idx = 0;
    let len = dat.len();

    while idx < len {
        let i = dat[idx];
        idx += 1;

        if i == 0 || i == 255 {
            if idx >= len {
                break;
            }
            let opcode = dat[idx];
            idx += 1;

            if opcode == 0 {
                // FLUXOP_END (End of stream)
                break;
            } else if opcode == 1 {
                // Index mark (4 bytes timestamp 28-bit)
                if idx + 4 <= len {
                    let b0 = dat[idx] as u32;
                    let b1 = dat[idx + 1] as u32;
                    let b2 = dat[idx + 2] as u32;
                    let b3 = dat[idx + 3] as u32;
                    idx += 4;

                    let ts = ((b0 & 0xFE) >> 1)
                        | (((b1 & 0xFE) >> 1) << 7)
                        | (((b2 & 0xFE) >> 1) << 14)
                        | (((b3 & 0xFE) >> 1) << 21);
                    index_timestamps.push(ts);
                } else {
                    break;
                }
            } else if opcode == 2 {
                // Space mark (4 bytes 28-bit delay)
                if idx + 4 <= len {
                    let b0 = dat[idx] as u32;
                    let b1 = dat[idx + 1] as u32;
                    let b2 = dat[idx + 2] as u32;
                    let b3 = dat[idx + 3] as u32;
                    idx += 4;

                    let val = ((b0 & 0xFE) >> 1)
                        | (((b1 & 0xFE) >> 1) << 7)
                        | (((b2 & 0xFE) >> 1) << 14)
                        | (((b3 & 0xFE) >> 1) << 21);
                    ticks += val;
                } else {
                    break;
                }
            } else if i == 255 {
                // Format 2 extended flux where 255 is the high byte and opcode is the low byte
                let val = 250 + (255 - 250) * 255 + opcode as u32 - 1;
                ticks += val;
                flux.push(ticks);
                ticks = 0;
            }
        } else if (1..250).contains(&i) {
            let val = i as u32;
            ticks += val;
            flux.push(ticks);
            ticks = 0;
        } else {
            // Extended flux 250..254
            if idx >= len {
                break;
            }
            let next_byte = dat[idx] as u32;
            idx += 1;
            let val = 250 + (i as u32 - 250) * 255 + next_byte - 1;
            ticks += val;
            flux.push(ticks);
            ticks = 0;
        }
    }

    DecodedGwFlux {
        flux,
        index_timestamps,
    }
}

/// Decodes Greaseweazle v4 binary packets into flux intervals (ticks)
#[allow(dead_code)]
pub fn decode_gw_flux(dat: &[u8]) -> Vec<u32> {
    decode_gw_flux_with_index(dat).flux
}

/// Computes real RPM values from consecutive hardware index timestamps (72 MHz sample clock)
pub fn calculate_rpm_from_index_timestamps(timestamps: &[u32], sample_freq_hz: f64) -> Vec<u32> {
    let mut rpms = Vec::new();
    if timestamps.len() < 2 {
        return rpms;
    }
    for i in 0..timestamps.len() - 1 {
        let delta = (timestamps[i + 1].wrapping_sub(timestamps[i])) & 0x0FFF_FFFF;
        if delta > 0 {
            let rpm = (sample_freq_hz * 60.0) / (delta as f64);
            let rounded = rpm.round() as u32;
            if (100..=800).contains(&rounded) {
                rpms.push(rounded);
            }
        }
    }
    rpms
}

/// Computes high-precision floating-point RPM values from consecutive hardware index timestamps (72 MHz sample clock)
#[allow(dead_code)]
pub fn calculate_rpm_floats_from_index_timestamps(
    timestamps: &[u32],
    sample_freq_hz: f64,
) -> Vec<f64> {
    let mut rpms = Vec::new();
    if timestamps.len() < 2 {
        return rpms;
    }
    for i in 0..timestamps.len() - 1 {
        let delta = (timestamps[i + 1].wrapping_sub(timestamps[i])) & 0x0FFF_FFFF;
        if delta > 0 {
            let rpm = (sample_freq_hz * 60.0) / (delta as f64);
            if (100.0..=800.0).contains(&rpm) {
                rpms.push(rpm);
            }
        }
    }
    rpms
}

/// Computes RPM and delta ticks by measuring the time interval between two identical MFM address headers (Case B fallback)
pub fn calculate_rpm_from_mfm_headers(flux: &[u32]) -> Option<(u32, u32)> {
    if flux.is_empty() {
        return None;
    }
    for &clock in &[144.0f64, 72.0f64] {
        let bits = pll_flux_to_mfm_bits(flux, clock);
        let sync_48 = [
            false, true, false, false, false, true, false, false, true, false, false, false, true,
            false, false, true, false, true, false, false, false, true, false, false, true, false,
            false, false, true, false, false, true, false, true, false, false, false, true, false,
            false, true, false, false, false, true, false, false, true,
        ];
        let mut first_seen: HashMap<(u8, u8), usize> = HashMap::new();
        let mut i = 0;
        while i + 48 + 7 * 16 <= bits.len() {
            if bits[i..i + 48] == sync_48 {
                let mut hdr = vec![0xA1, 0xA1, 0xA1];
                for byte_idx in 0..7 {
                    let mut w: u16 = 0;
                    for k in 0..16 {
                        w = (w << 1)
                            | (if bits[i + 48 + byte_idx * 16 + k] { 1 } else { 0 });
                    }
                    hdr.push(decode_byte_word(w));
                }

                if hdr[3] == 0xFE {
                    let cyl = hdr[4];
                    let sec_id = hdr[6];
                    let crc_res = crc16_ccitt(&hdr);
                    if crc_res == 0 && (1..=36).contains(&sec_id) {
                        if let Some(&first_pos) = first_seen.get(&(cyl, sec_id)) {
                            let bit_delta = i - first_pos;
                            let delta_ticks = (bit_delta as f64 * clock).round() as u32;
                            if delta_ticks > 0 {
                                let rpm = ((72_000_000.0 * 60.0) / delta_ticks as f64).round() as u32;
                                if (100..=800).contains(&rpm) {
                                    return Some((rpm, delta_ticks));
                                }
                            }
                        } else {
                            first_seen.insert((cyl, sec_id), i);
                        }
                    }
                }
                i += 48 + 7 * 16;
                continue;
            }
            i += 1;
        }
    }
    None
}

/// Calculates adaptive timeout in milliseconds for seek operations based on track distance
pub fn calculate_seek_timeout_ms(current_track: u8, target_track: u8) -> u64 {
    let distance = (target_track as i32 - current_track as i32).unsigned_abs() as u64;
    1200 + (distance * 25)
}

/// Calculates adaptive timeout Duration for seek operations based on track distance
#[allow(dead_code)]
pub fn calculate_seek_timeout(current_track: u8, target_track: u8) -> Duration {
    Duration::from_millis(calculate_seek_timeout_ms(current_track, target_track))
}

/// Greaseweazle-compliant Software Phase-Locked Loop (PLL) for MFM bitstream recovery
#[derive(Clone, Debug)]
pub struct SoftwarePll {
    pub clock_centre: f64,
    pub clock_min: f64,
    pub clock_max: f64,
    pub clock: f64,
    pub phase_accumulator: f64,
    pub phase_adj: f64,
    pub period_adj: f64,
}

impl SoftwarePll {
    pub fn new(clock_ticks: f64) -> Self {
        Self {
            clock_centre: clock_ticks,
            clock_min: clock_ticks * 0.90,
            clock_max: clock_ticks * 1.10,
            clock: clock_ticks,
            phase_accumulator: 0.0,
            phase_adj: 0.60,
            period_adj: 0.05,
        }
    }

    /// Resets the PLL phase accumulator and bitcell period estimator at the beginning of each pass
    pub fn reset(&mut self) {
        self.clock = self.clock_centre;
        self.phase_accumulator = 0.0;
    }

    /// Decodes raw flux intervals into MFM bitstream with clean PLL state initialization
    pub fn decode_flux(&mut self, flux: &[u32]) -> Vec<bool> {
        self.reset();
        let mut bit_array = Vec::with_capacity(flux.len() * 3);

        for &x in flux {
            self.phase_accumulator += x as f64;
            if self.phase_accumulator < self.clock / 2.0 {
                continue;
            }

            let mut zeros = 0;
            loop {
                self.phase_accumulator -= self.clock;
                if self.phase_accumulator < self.clock / 2.0 {
                    break;
                }
                zeros += 1;
                bit_array.push(false);
            }
            bit_array.push(true);

            let new_ticks = self.phase_accumulator * (1.0 - self.phase_adj);
            if zeros <= 3 {
                self.clock += self.phase_accumulator * self.period_adj;
            } else {
                self.clock += (self.clock_centre - self.clock) * self.period_adj;
            }
            self.clock = self.clock.clamp(self.clock_min, self.clock_max);
            self.phase_accumulator = new_ticks;
        }

        bit_array
    }
}

/// Greaseweazle-compliant Phase-Locked Loop (PLL)
pub fn pll_flux_to_mfm_bits(flux: &[u32], clock_ticks: f64) -> Vec<bool> {
    let mut pll = SoftwarePll::new(clock_ticks);
    pll.decode_flux(flux)
}

/// Computes physical PLL quality score (0..=100%) based on RMS Phase Jitter relative to theoretical MFM windows
#[allow(dead_code)]
pub fn calculate_pll_quality(flux: &[u32], clock_ticks: f64) -> u8 {
    calculate_pll_quality_with_crc(flux, clock_ticks, false)
}

/// Computes physical PLL quality score (0..=100%) based on RMS Phase Jitter, with penalty for residual CRC errors:
/// - Phase error per transition: epsilon = |flux_time - bitcell_center| / (bitcell_window / 2.0)
/// - RMS Jitter: sqrt( (1/N) * sum(epsilon^2) )
/// - Base Q: ((1.0 - 2.0 * RMS_jitter) * 100.0).clamp(0.0, 100.0) as u8
/// - CRC penalty: if has_crc_errors, Q.saturating_sub(15)
pub fn calculate_pll_quality_with_crc(flux: &[u32], clock_ticks: f64, has_crc_errors: bool) -> u8 {
    if flux.is_empty() || clock_ticks <= 0.0 {
        return 0;
    }
    let mut sum_sq_err = 0.0f64;
    let mut count = 0usize;
    let half_clock = clock_ticks / 2.0;

    for &x in flux {
        let val = x as f64;
        let n = (val / clock_ticks).round();
        if (2.0..=4.0).contains(&n) {
            let bitcell_center = n * clock_ticks;
            let phase_err = (val - bitcell_center).abs() / half_clock;
            sum_sq_err += phase_err * phase_err;
            count += 1;
        }
    }

    if count == 0 {
        return 0;
    }
    let rms_jitter = (sum_sq_err / (count as f64)).sqrt();
    let q_score = ((1.0 - 2.0 * rms_jitter) * 100.0).clamp(0.0, 100.0).round() as u8;
    if has_crc_errors {
        q_score.saturating_sub(15)
    } else {
        q_score
    }
}

/// Validates and filters Gap0 time in microseconds based on bitrate-specific physical ranges:
/// - 500 kbps (HD): [800 µs, 2200 µs] (Nominal ~1440 µs)
/// - 250 kbps (DD): [1600 µs, 4400 µs] (Nominal ~2880 µs)
/// - 300 kbps: [1300 µs, 3600 µs]
pub fn get_valid_gap0(bitrate: u16, raw_gap0_us: u32) -> Option<u32> {
    let (min_gap, max_gap) = match bitrate {
        500 => (800, 2200),
        250 => (1600, 4400),
        300 => (1300, 3600),
        _ => {
            let nominal = (1440.0 * 500.0 / (bitrate as f64).max(1.0)) as u32;
            (nominal.saturating_sub(nominal * 45 / 100), nominal + nominal * 55 / 100)
        }
    };
    if (min_gap..=max_gap).contains(&raw_gap0_us) {
        Some(raw_gap0_us)
    } else {
        None
    }
}

/// Formats the Gap0 value for display (e.g. `Gap0:1440µs` or `Gap0:----`)
#[allow(dead_code)]
pub fn format_gap0_field(gap0_us: Option<u32>) -> String {
    if let Some(gap0) = gap0_us {
        format!("Gap0:{:4}µs", gap0)
    } else {
        String::from("Gap0:----")
    }
}

/// Detects physical interleave from the sequence of decoded sector IDs
pub fn detect_interleave(sectors: &[DecodedSector], expected_count: u8) -> Option<String> {
    if sectors.is_empty() {
        return None;
    }
    if sectors.len() < 2 {
        return Some(String::from("1:1"));
    }

    let n = if expected_count > 0 {
        expected_count as i32
    } else {
        18
    };

    let mut delta_counts: HashMap<i32, usize> = HashMap::new();
    for i in 0..sectors.len() - 1 {
        let s0 = sectors[i].sec_id as i32;
        let s1 = sectors[i + 1].sec_id as i32;
        let mut delta = s1 - s0;
        if delta <= 0 {
            delta += n;
        }
        if (1..=n).contains(&delta) {
            *delta_counts.entry(delta).or_insert(0) += 1;
        }
    }

    let mode_delta = delta_counts
        .into_iter()
        .max_by_key(|&(_, cnt)| cnt)
        .map(|(d, _)| d)
        .unwrap_or(1);

    Some(format!("1:{}", mode_delta))
}

/// Computes relative jitter % between consecutive revolutions
pub fn calculate_jitter_pct(index_timestamps: &[u32]) -> Option<f64> {
    if index_timestamps.len() >= 3 {
        let delta0 = (index_timestamps[1].wrapping_sub(index_timestamps[0])) & 0x0FFF_FFFF;
        let delta1 = (index_timestamps[2].wrapping_sub(index_timestamps[1])) & 0x0FFF_FFFF;
        if delta0 > 0 && delta1 > 0 {
            let diff = (delta1 as f64 - delta0 as f64).abs();
            let jitter = (diff / delta0 as f64) * 100.0;
            return Some((jitter * 10.0).round() / 10.0);
        }
    }
    None
}

pub fn decode_idam_sectors_from_bits(bits: &[bool]) -> Vec<DecodedSector> {
    let mut sectors = Vec::new();
    let sync_48 = [
        false, true, false, false, false, true, false, false, true, false, false, false, true,
        false, false, true, false, true, false, false, false, true, false, false, true, false,
        false, false, true, false, false, true, false, true, false, false, false, true, false,
        false, true, false, false, false, true, false, false, true,
    ];

    let mut i = 0;
    while i + 48 + 7 * 16 <= bits.len() {
        if bits[i..i + 48] == sync_48 {
            let mut hdr = vec![0xA1, 0xA1, 0xA1];
            for byte_idx in 0..7 {
                let mut w: u16 = 0;
                for k in 0..16 {
                    w = (w << 1)
                        | (if bits[i + 48 + byte_idx * 16 + k] {
                            1
                        } else {
                            0
                        });
                }
                hdr.push(decode_byte_word(w));
            }

            if hdr[3] == 0xFE {
                let cyl = hdr[4];
                let head = hdr[5];
                let sec_id = hdr[6];
                let size_code = hdr[7];
                let crc_res = crc16_ccitt(&hdr);
                let crc_ok = crc_res == 0;

                let mut status = if crc_ok {
                    SectorStatus::Ok
                } else {
                    SectorStatus::CrcId
                };

                // If IDAM is valid, check subsequent DAM (within ~60 bytes / 960 bits)
                if crc_ok {
                    let search_limit = (i + 48 + 7 * 16 + 1000).min(bits.len());
                    let mut dam_pos = i + 48 + 7 * 16;

                    while dam_pos + 48 + 16 <= search_limit {
                        if bits[dam_pos..dam_pos + 48] == sync_48 {
                            let mut dam_w: u16 = 0;
                            for k in 0..16 {
                                dam_w = (dam_w << 1)
                                    | (if bits[dam_pos + 48 + k] { 1 } else { 0 });
                            }
                            let mark_byte = decode_byte_word(dam_w);
                            if mark_byte == 0xFB || mark_byte == 0xF8 {
                                let sector_size = (128usize) << (size_code.min(4) as usize);
                                let needed_bits = dam_pos + 48 + (1 + sector_size + 2) * 16;
                                if needed_bits <= bits.len() {
                                    let mut data_buf = vec![0xA1, 0xA1, 0xA1, mark_byte];
                                    for b_idx in 0..(sector_size + 2) {
                                        let mut dw: u16 = 0;
                                        for k in 0..16 {
                                            dw = (dw << 1)
                                                | (if bits[dam_pos + 48 + 16 + b_idx * 16 + k] {
                                                    1
                                                } else {
                                                    0
                                                });
                                        }
                                        data_buf.push(decode_byte_word(dw));
                                    }
                                    let data_crc = crc16_ccitt(&data_buf);
                                    if data_crc == 0 {
                                        status = if mark_byte == 0xF8 {
                                            SectorStatus::DelDam
                                        } else {
                                            SectorStatus::Ok
                                        };
                                    } else {
                                        status = SectorStatus::CrcData;
                                    }
                                }
                                break;
                            }
                        }
                        dam_pos += 1;
                    }
                }

                let final_crc_ok = matches!(status, SectorStatus::Ok | SectorStatus::DelDam);

                if (1..=36).contains(&sec_id)
                    && size_code <= 4
                    && !sectors
                        .iter()
                        .any(|s: &DecodedSector| s.sec_id == sec_id && s.cyl == cyl)
                {
                    sectors.push(DecodedSector {
                        cyl,
                        head,
                        sec_id,
                        size_code,
                        crc_ok: final_crc_ok,
                        status,
                    });
                }
            }
            i += 48 + 7 * 16;
            continue;
        }
        i += 1;
    }
    sectors
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TrackAnalysisResult {
    pub has_disk: bool,
    pub bitrate: u16,
    pub sector_count: u8,
    pub sectors_known: bool,
    pub sectors: Vec<DecodedSector>,
    pub on_track_count: u32,
    pub off_track_count: u32,
    pub off_track_details: String,
    pub crc_err_count: u32,
    pub alignment_pct: f32,
    pub index_timestamps: Vec<u32>,
    pub instant_rpms: Vec<u32>,
    pub rev_time_ms: f64,
    pub rpm_instant: Option<f64>,
    pub jitter_pct: Option<f64>,
    pub gap0_us: Option<u32>,
    pub pll_quality_pct: Option<u8>,
    pub interleave: Option<String>,
    pub read_status: DriveReadStatus,
}

impl Default for TrackAnalysisResult {
    fn default() -> Self {
        Self {
            has_disk: false,
            bitrate: 500,
            sector_count: 0,
            sectors_known: false,
            sectors: Vec::new(),
            on_track_count: 0,
            off_track_count: 0,
            off_track_details: String::from("NONE"),
            crc_err_count: 0,
            alignment_pct: 0.0,
            index_timestamps: Vec::new(),
            instant_rpms: Vec::new(),
            rev_time_ms: 0.0,
            rpm_instant: None,
            jitter_pct: None,
            gap0_us: None,
            pll_quality_pct: None,
            interleave: None,
            read_status: DriveReadStatus::Ok,
        }
    }
}

fn read_motor_rpm_diagnostic(
    port: &mut Box<dyn serialport::SerialPort>,
    unit: u8,
    motor_on: bool,
    head: u8,
) -> Result<DecodedGwFlux, Box<dyn std::error::Error>> {
    match gw_read_flux(port, 2) {
        Ok(dat) => {
            let decoded_gw = decode_gw_flux_with_index(&dat);
            Ok(decoded_gw)
        }
        Err(e) => {
            let _ = port.clear(serialport::ClearBuffer::All);
            let _ = gw_send_raw(port, &[0x00, 0x03, 0x00], 32);
            ensure_unit_active(port, unit, motor_on, head);
            Err(e)
        }
    }
}

/// Initializes a new progressive scan pass on the active line (`►`)
pub fn start_revolution_progress(
    status: &mut DriveStatus,
    tx_status: &crossbeam_channel::Sender<DriveStatus>,
    track: u8,
    head: u8,
    bitrate: u16,
    expected_count: u8,
) {
    status.has_disk = true;
    status.bitrate = bitrate;
    status.density = bitrate == 500;
    status.sector_count = expected_count;
    status.sectors_known = true;
    status.track = track;
    status.head = head;
    status.in_progress_pass = true;

    // Reset sectors array for the new revolution so top bar starts empty
    status.sectors.clear();

    // Standard ribbon with all empty blocks
    let empty_vec: Vec<&str> = (0..expected_count).map(|_| "░").collect();
    let blocks_std = empty_vec.join(" ");
    let raw_ribbon_std = format!("[ {} ]", blocks_std);
    let ribbon_col_std = format!("{:<39}", raw_ribbon_std);
    let status_str_std = format!("({:>2}/{})", 0, expected_count);
    let standard_line = format!(
        "T:{:02} H:{} Rate:{}k MFM {} {}",
        track, head, bitrate, ribbon_col_std, status_str_std
    );

    // Verbose ribbon with all empty blocks
    let blocks_verb = empty_vec.join(" ");
    let raw_ribbon_verb = format!("[ {} ]", blocks_verb);
    let ribbon_col_verb = format!("{:<39}", raw_ribbon_verb);
    let status_str_verb = format!(" ({:>2}/{})", 0, expected_count);
    let verbose_line = format!(
        "T:{:02} H:{} Rate:{}k MFM {} {} IL:--- Gap0:---- Q:--%",
        track, head, bitrate, ribbon_col_verb, status_str_verb
    );

    status.sector_log_standard.push(standard_line);
    status.sector_log_verbose.push(verbose_line);

    if status.sector_log_standard.len() > 50 {
        let excess = status.sector_log_standard.len() - 50;
        status.sector_log_standard.drain(0..excess);
    }
    if status.sector_log_verbose.len() > 50 {
        let excess = status.sector_log_verbose.len() - 50;
        status.sector_log_verbose.drain(0..excess);
    }

    status.sector_log = if status.verbose_mode {
        status.sector_log_verbose.clone()
    } else {
        status.sector_log_standard.clone()
    };

    let _ = tx_status.send(status.clone());
}

/// Updates the progressive scan pass on the active line (`►`) sector by sector
#[allow(clippy::too_many_arguments)]
pub fn update_revolution_progress(
    status: &mut DriveStatus,
    tx_status: &crossbeam_channel::Sender<DriveStatus>,
    track: u8,
    head: u8,
    bitrate: u16,
    expected_count: u8,
    sectors_found: usize,
    last_sector: Option<&DecodedSector>,
) {
    if let Some(sec) = last_sector {
        if !status.sectors.iter().any(|s| s.sec_id == sec.sec_id && s.track == sec.cyl) {
            status.sectors.push(SectorInfo {
                track: sec.cyl,
                sec_id: sec.sec_id,
                size_code: sec.size_code,
                status_code: if sec.crc_ok { 0 } else { 1 },
                crc_ok: sec.crc_ok,
            });
        }
    }

    let filled = sectors_found.min(expected_count as usize);
    let empty = (expected_count as usize).saturating_sub(filled);

    let mut blocks_vec = Vec::with_capacity(expected_count as usize);
    blocks_vec.extend(std::iter::repeat_n("■", filled));
    blocks_vec.extend(std::iter::repeat_n("░", empty));

    let blocks_str = blocks_vec.join(" ");
    let raw_ribbon = format!("[ {} ]", blocks_str);
    let ribbon_col = format!("{:<39}", raw_ribbon);

    let status_str_std = if filled == expected_count as usize {
        format!("({}/{} OK)", expected_count, expected_count)
    } else {
        format!("({:>2}/{})", filled, expected_count)
    };

    let standard_line = format!(
        "T:{:02} H:{} Rate:{}k MFM {} {}",
        track, head, bitrate, ribbon_col, status_str_std
    );

    let status_str_verb = if filled == expected_count as usize {
        if expected_count == 9 {
            String::from("(9/9 OK)  ")
        } else {
            format!("({}/{} OK)", expected_count, expected_count)
        }
    } else {
        format!(" ({:>2}/{})", filled, expected_count)
    };
    let verbose_line = format!(
        "T:{:02} H:{} Rate:{}k MFM {} {} IL:--- Gap0:---- Q:--%",
        track, head, bitrate, ribbon_col, status_str_verb
    );

    if let Some(last) = status.sector_log_standard.last_mut() {
        *last = standard_line;
    }
    if let Some(last) = status.sector_log_verbose.last_mut() {
        *last = verbose_line;
    }

    status.sector_log = if status.verbose_mode {
        status.sector_log_verbose.clone()
    } else {
        status.sector_log_standard.clone()
    };

    let _ = tx_status.send(status.clone());
}

fn read_and_decode_track_diagnostic(
    port: &mut Box<dyn serialport::SerialPort>,
    expected_cyl: u8,
    status: &mut DriveStatus,
    tx_status: &crossbeam_channel::Sender<DriveStatus>,
) -> TrackAnalysisResult {
    let pass_start = std::time::Instant::now();

    if let Ok(dat) = gw_read_flux(port, 3) {
        if !dat.is_empty() {
            let decoded_gw = decode_gw_flux_with_index(&dat);
            let instant_rpms =
                calculate_rpm_from_index_timestamps(&decoded_gw.index_timestamps, 72_000_000.0);

            let sample_rate = 72_000_000.0;
            let (rev_time_ms, rpm_instant) = if decoded_gw.index_timestamps.len() >= 2 {
                let idx_start = decoded_gw.index_timestamps[0];
                let idx_end = decoded_gw.index_timestamps[1];
                let delta = (idx_end.wrapping_sub(idx_start)) & 0x0FFF_FFFF;
                let rev_ms = (delta as f64 / sample_rate) * 1000.0;
                let rpm = if rev_ms > 0.0 { 60_000.0 / rev_ms } else { 0.0 };
                (rev_ms, if rpm > 0.0 { Some(rpm) } else { None })
            } else if let Some((rpm, delta_ticks)) = calculate_rpm_from_mfm_headers(&decoded_gw.flux) {
                let rev_ms = (delta_ticks as f64 / sample_rate) * 1000.0;
                (rev_ms, Some(rpm as f64))
            } else {
                (200.0, None)
            };

            let jitter_pct = calculate_jitter_pct(&decoded_gw.index_timestamps);

            if dat.len() < 100 || decoded_gw.flux.len() < 100 || decoded_gw.index_timestamps.is_empty() {
                start_revolution_progress(status, tx_status, expected_cyl, status.head, 500, 18);
                return TrackAnalysisResult {
                    has_disk: false,
                    bitrate: 500,
                    sector_count: 0,
                    sectors_known: false,
                    sectors: Vec::new(),
                    on_track_count: 0,
                    off_track_count: 0,
                    off_track_details: String::from("NONE"),
                    crc_err_count: 0,
                    alignment_pct: 0.0,
                    index_timestamps: decoded_gw.index_timestamps,
                    instant_rpms,
                    rev_time_ms,
                    rpm_instant,
                    jitter_pct,
                    gap0_us: None,
                    pll_quality_pct: None,
                    interleave: None,
                    read_status: DriveReadStatus::NoDiskOrNoIndex,
                };
            }

            let flux = decoded_gw.flux;

            // Decode DD 250k (144.0 ticks) then fallback HD 500k (72.0 ticks)
            let bits_dd = pll_flux_to_mfm_bits(&flux, 144.0);
            let mut sectors = decode_idam_sectors_from_bits(&bits_dd);
            let mut bitrate = 250;
            let mut clock = 144.0;
            let mut active_bits = &bits_dd;

            let bits_hd;
            if sectors.is_empty() {
                bits_hd = pll_flux_to_mfm_bits(&flux, 72.0);
                let sec_hd = decode_idam_sectors_from_bits(&bits_hd);
                if !sec_hd.is_empty() {
                    sectors = sec_hd;
                    bitrate = 500;
                    clock = 72.0;
                    active_bits = &bits_hd;
                }
            }

            let sectors_known = !sectors.is_empty();
            let sector_count = if sectors_known {
                if bitrate == 500 {
                    if sectors.len() > 9 {
                        sectors.len() as u8
                    } else {
                        18
                    }
                } else if !sectors.is_empty() {
                    sectors.len() as u8
                } else {
                    9
                }
            } else {
                0
            };

            let expected_count = if bitrate == 250 {
                if sector_count > 9 { sector_count } else { 9 }
            } else if sector_count == 15 || (sectors.iter().map(|s| s.sec_id).max().unwrap_or(0) == 15 && sector_count <= 15) {
                15
            } else if sector_count > 18 {
                sector_count
            } else {
                18
            };

            // 1. Initial state at start of revolution (0/expected_count)
            start_revolution_progress(
                status,
                tx_status,
                expected_cyl,
                status.head,
                bitrate,
                expected_count,
            );

            // 2. Incremental progressive emission as each sector is validated at native CPU speed
            if !sectors.is_empty() {
                for (idx, sec) in sectors.iter().enumerate() {
                    update_revolution_progress(
                        status,
                        tx_status,
                        expected_cyl,
                        status.head,
                        bitrate,
                        expected_count,
                        idx + 1,
                        Some(sec),
                    );
                }
            }

            let sync_48 = [
                false, true, false, false, false, true, false, false, true, false, false, false, true,
                false, false, true, false, true, false, false, false, true, false, false, true, false,
                false, false, true, false, false, true, false, true, false, false, false, true, false,
                false, true, false, false, false, true, false, false, true,
            ];
            let first_idam_bit = active_bits.windows(48).position(|w| w == sync_48);
            let gap0_us = first_idam_bit.and_then(|b_idx| {
                let raw_us = (b_idx as f64 * clock / 72.0).round() as u32;
                get_valid_gap0(bitrate, raw_us)
            });

            let has_crc_errs = sectors.iter().any(|s| !s.crc_ok);
            let pll_quality_pct = if sectors_known {
                Some(calculate_pll_quality_with_crc(&flux, clock, has_crc_errs))
            } else {
                None
            };
            let interleave = detect_interleave(&sectors, sector_count);

            let mut on_track: u32 = 0;
            let mut off_track: u32 = 0;
            let mut crc_errs: u32 = 0;
            let mut wrong_cylinders: HashMap<u8, u32> = HashMap::new();

            for s in &sectors {
                if !s.crc_ok {
                    crc_errs += 1;
                }
                if s.cyl == expected_cyl {
                    if s.crc_ok {
                        on_track += 1;
                    }
                } else {
                    off_track += 1;
                    *wrong_cylinders.entry(s.cyl).or_insert(0) += 1;
                }
            }

            let alignment_pct = if expected_count > 0 {
                (on_track as f32 / expected_count as f32) * 100.0
            } else {
                0.0
            };

            let off_track_details = if off_track == 0 {
                String::from("NONE (Perfect)")
            } else if wrong_cylinders.len() == 1 && on_track == 0 {
                let (&cyl, _) = wrong_cylinders.iter().next().unwrap();
                format!("MISALIGNED T:{:02}", cyl)
            } else {
                let mut parts = Vec::new();
                for (cyl, cnt) in wrong_cylinders {
                    parts.push(format!("T{:02}: {} sect", cyl, cnt));
                }
                parts.join(", ")
            };

            return TrackAnalysisResult {
                has_disk: true,
                bitrate,
                sector_count,
                sectors_known,
                sectors,
                on_track_count: on_track,
                off_track_count: off_track,
                off_track_details,
                crc_err_count: crc_errs,
                alignment_pct,
                index_timestamps: decoded_gw.index_timestamps,
                instant_rpms,
                rev_time_ms,
                rpm_instant,
                jitter_pct,
                gap0_us,
                pll_quality_pct,
                interleave,
                read_status: DriveReadStatus::Ok,
            };
        }
    }

    let pass_duration_ms = pass_start.elapsed().as_secs_f64() * 1000.0;
    let _ = port.clear(serialport::ClearBuffer::All);
    TrackAnalysisResult {
        has_disk: false,
        bitrate: 500,
        sector_count: 0,
        sectors_known: false,
        sectors: Vec::new(),
        on_track_count: 0,
        off_track_count: 0,
        off_track_details: String::from("NONE"),
        crc_err_count: 0,
        alignment_pct: 0.0,
        index_timestamps: Vec::new(),
        instant_rpms: Vec::new(),
        rev_time_ms: pass_duration_ms,
        rpm_instant: None,
        jitter_pct: None,
        gap0_us: None,
        pll_quality_pct: None,
        interleave: None,
        read_status: DriveReadStatus::NoDiskOrNoIndex,
    }
}

/// Formats a single pass for Verbose horizontal / vertical history mode:
/// T:<track> H:<head> Rate:<bitrate>k MFM <ribbon> <status> <IL> <Gap0> <Quality>
/// e.g. "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK) IL:1:1 Gap0:1440µs Q:98%"
pub fn build_verbose_pass_line(
    track: u8,
    head: u8,
    diag: &TrackAnalysisResult,
) -> String {
    if !diag.has_disk || !diag.sectors_known || diag.sectors.is_empty() {
        let ribbon_col = format!("{:<39}", "[ ? ]");
        let status_col = "(0/0 NO DATA / NO DISK)";
        return format!(
            "T:{:02} H:{} Rate:---k --- {} {} IL:--- Gap0:---- Q:--%",
            track, head, ribbon_col, status_col
        );
    }

    let expected_count = if diag.bitrate == 250 {
        if diag.sector_count > 9 { diag.sector_count } else { 9 }
    } else if diag.bitrate == 500 {
        if diag.sector_count == 15 || (diag.sectors.iter().map(|s| s.sec_id).max().unwrap_or(0) == 15 && diag.sector_count <= 15) {
            15
        } else if diag.sector_count > 18 {
            diag.sector_count
        } else {
            18
        }
    } else if diag.sector_count > 0 {
        if diag.sector_count <= 9 {
            9
        } else if diag.sector_count <= 15 {
            15
        } else {
            18
        }
    } else {
        18
    };

    let mut blocks_vec = Vec::with_capacity(expected_count as usize);
    let mut ok_count: u32 = 0;
    let mut crc_dat_secs: Vec<u8> = Vec::new();
    let mut crc_id_secs: Vec<u8> = Vec::new();
    let mut no_dam_secs: Vec<u8> = Vec::new();
    let mut del_dam_secs: Vec<u8> = Vec::new();
    let mut missing_secs: Vec<u8> = Vec::new();

    for sec_id in 1..=expected_count {
        if let Some(sec) = diag.sectors.iter().find(|s| s.sec_id == sec_id && s.cyl == track) {
            match sec.status {
                SectorStatus::Ok => {
                    ok_count += 1;
                    blocks_vec.push("■");
                }
                SectorStatus::CrcData => {
                    crc_dat_secs.push(sec_id);
                    blocks_vec.push("■");
                }
                SectorStatus::CrcId => {
                    crc_id_secs.push(sec_id);
                    blocks_vec.push("■");
                }
                SectorStatus::NoDam => {
                    no_dam_secs.push(sec_id);
                    blocks_vec.push("■");
                }
                SectorStatus::DelDam => {
                    del_dam_secs.push(sec_id);
                    blocks_vec.push("■");
                }
                SectorStatus::Missing => {
                    missing_secs.push(sec_id);
                    blocks_vec.push("░");
                }
            }
        } else if diag.sectors.iter().any(|s| s.sec_id == sec_id) {
            blocks_vec.push("■");
        } else {
            missing_secs.push(sec_id);
            blocks_vec.push("░");
        }
    }

    let raw_ribbon = format!("[ {} ]", blocks_vec.join(" "));
    let ribbon_col = format!("{:<39}", raw_ribbon);

    let status_str = if diag.off_track_count > 0 && diag.off_track_details.starts_with("MISALIGNED") {
        format!("({}/{} {})", diag.sectors.len(), expected_count, diag.off_track_details)
    } else if ok_count == expected_count as u32 && diag.crc_err_count == 0 && diag.off_track_count == 0 {
        if expected_count == 9 {
            String::from("(9/9 OK)  ")
        } else {
            format!("({}/{} OK)", expected_count, expected_count)
        }
    } else if !crc_dat_secs.is_empty() {
        let list: Vec<String> = crc_dat_secs.iter().map(|s| s.to_string()).collect();
        format!("({}/{} CRC-DAT: Sec {})", ok_count, expected_count, list.join(", "))
    } else if !crc_id_secs.is_empty() {
        let list: Vec<String> = crc_id_secs.iter().map(|s| s.to_string()).collect();
        format!("({}/{} CRC-ID: Sec {})", ok_count, expected_count, list.join(", "))
    } else if !no_dam_secs.is_empty() {
        let list: Vec<String> = no_dam_secs.iter().map(|s| s.to_string()).collect();
        format!("({}/{} NO-DAM: Sec {})", ok_count, expected_count, list.join(", "))
    } else if !del_dam_secs.is_empty() {
        let list: Vec<String> = del_dam_secs.iter().map(|s| s.to_string()).collect();
        format!("({}/{} DEL-DAM: Sec {})", ok_count, expected_count, list.join(", "))
    } else if !missing_secs.is_empty() {
        let list: Vec<String> = missing_secs.iter().map(|s| s.to_string()).collect();
        format!("({}/{} MISSING: Sec {})", ok_count, expected_count, list.join(", "))
    } else if diag.off_track_count > 0 {
        format!("({}/{} OFF-TRK: {})", ok_count, expected_count, diag.off_track_details)
    } else if ok_count < expected_count as u32 {
        format!("({}/{} BAD)", ok_count, expected_count)
    } else {
        format!("({}/{} OK)", ok_count, expected_count)
    };

    let il_str = if let Some(ref il) = diag.interleave {
        format!("IL:{}", il)
    } else if let Some(il) = detect_interleave(&diag.sectors, expected_count) {
        format!("IL:{}", il)
    } else {
        String::from("IL:1:1")
    };

    let gap0_str = if let Some(gap0) = diag.gap0_us {
        format!("Gap0:{:4}µs", gap0)
    } else {
        String::from("Gap0:----")
    };

    let q_str = if let Some(q) = diag.pll_quality_pct {
        format!("Q:{}%", q)
    } else {
        String::from("Q:--%")
    };

    let mut parts = Vec::new();
    parts.push(format!(
        "T:{:02} H:{} Rate:{}k MFM {} {}",
        track, head, diag.bitrate, ribbon_col, status_str
    ));
    parts.push(il_str);
    parts.push(gap0_str);
    parts.push(q_str);

    parts.join(" ")
}

/// Formats a single pass for Standard column mode (Clean segmented bar with strict column alignment)
/// Format: T:<track> H:<head> Rate:<bitrate>k MFM <segmented_bar> <status_counter>
pub fn build_standard_pass_line(
    track: u8,
    head: u8,
    diag: &TrackAnalysisResult,
) -> String {
    if !diag.has_disk || !diag.sectors_known || diag.sectors.is_empty() {
        let ribbon_col = format!("{:<39}", "[ ? ]");
        let status_col = "(0/0 NO DATA / NO DISK)";
        return format!("T:{:02} H:{} Rate:---k --- {} {}", track, head, ribbon_col, status_col);
    }

    let expected_count = if diag.bitrate == 250 {
        if diag.sector_count > 9 { diag.sector_count } else { 9 }
    } else if diag.bitrate == 500 {
        if diag.sector_count == 15 || (diag.sectors.iter().map(|s| s.sec_id).max().unwrap_or(0) == 15 && diag.sector_count <= 15) {
            15
        } else if diag.sector_count > 18 {
            diag.sector_count
        } else {
            18
        }
    } else if diag.sector_count > 0 {
        if diag.sector_count <= 9 {
            9
        } else if diag.sector_count <= 15 {
            15
        } else {
            18
        }
    } else {
        18
    };

    let mut blocks_vec = Vec::with_capacity(expected_count as usize);
    let mut ok_count: u32 = 0;
    let mut crc_dat_secs: Vec<u8> = Vec::new();
    let mut crc_id_secs: Vec<u8> = Vec::new();
    let mut no_dam_secs: Vec<u8> = Vec::new();
    let mut del_dam_secs: Vec<u8> = Vec::new();
    let mut missing_secs: Vec<u8> = Vec::new();

    for sec_id in 1..=expected_count {
        if let Some(sec) = diag.sectors.iter().find(|s| s.sec_id == sec_id && s.cyl == track) {
            match sec.status {
                SectorStatus::Ok => {
                    ok_count += 1;
                    blocks_vec.push("■");
                }
                SectorStatus::CrcData => {
                    crc_dat_secs.push(sec_id);
                    blocks_vec.push("■");
                }
                SectorStatus::CrcId => {
                    crc_id_secs.push(sec_id);
                    blocks_vec.push("■");
                }
                SectorStatus::NoDam => {
                    no_dam_secs.push(sec_id);
                    blocks_vec.push("■");
                }
                SectorStatus::DelDam => {
                    del_dam_secs.push(sec_id);
                    blocks_vec.push("■");
                }
                SectorStatus::Missing => {
                    missing_secs.push(sec_id);
                    blocks_vec.push("░");
                }
            }
        } else if diag.sectors.iter().any(|s| s.sec_id == sec_id) {
            blocks_vec.push("■");
        } else {
            missing_secs.push(sec_id);
            blocks_vec.push("░");
        }
    }

    let raw_ribbon = format!("[ {} ]", blocks_vec.join(" "));
    let ribbon_col = format!("{:<39}", raw_ribbon);

    let status_str = if diag.off_track_count > 0 && diag.off_track_details.starts_with("MISALIGNED") {
        format!("({}/{} {})", diag.sectors.len(), expected_count, diag.off_track_details)
    } else if ok_count == expected_count as u32 && diag.crc_err_count == 0 && diag.off_track_count == 0 {
        format!("({}/{} OK)", expected_count, expected_count)
    } else if !crc_dat_secs.is_empty() {
        let list: Vec<String> = crc_dat_secs.iter().map(|s| s.to_string()).collect();
        format!("({}/{} CRC-DAT: Sec {})", ok_count, expected_count, list.join(", "))
    } else if !crc_id_secs.is_empty() {
        let list: Vec<String> = crc_id_secs.iter().map(|s| s.to_string()).collect();
        format!("({}/{} CRC-ID: Sec {})", ok_count, expected_count, list.join(", "))
    } else if !no_dam_secs.is_empty() {
        let list: Vec<String> = no_dam_secs.iter().map(|s| s.to_string()).collect();
        format!("({}/{} NO-DAM: Sec {})", ok_count, expected_count, list.join(", "))
    } else if !del_dam_secs.is_empty() {
        let list: Vec<String> = del_dam_secs.iter().map(|s| s.to_string()).collect();
        format!("({}/{} DEL-DAM: Sec {})", ok_count, expected_count, list.join(", "))
    } else if !missing_secs.is_empty() {
        let list: Vec<String> = missing_secs.iter().map(|s| s.to_string()).collect();
        format!("({}/{} MISSING: Sec {})", ok_count, expected_count, list.join(", "))
    } else if diag.off_track_count > 0 {
        format!("({}/{} OFF-TRK: {})", ok_count, expected_count, diag.off_track_details)
    } else if ok_count < expected_count as u32 {
        format!("({}/{} BAD)", ok_count, expected_count)
    } else {
        format!("({}/{} OK)", ok_count, expected_count)
    };

    format!(
        "T:{:02} H:{} Rate:{}k MFM {} {}",
        track, head, diag.bitrate, ribbon_col, status_str
    )
}

pub fn process_track_diagnostic(
    status: &mut DriveStatus,
    diag: &TrackAnalysisResult,
    tx_sound: &Sender<AudioEvent>,
) {
    let mut effective_diag = diag.clone();

    let target_track = if status.target_track > 0 {
        status.target_track
    } else {
        status.track
    };
    status.target_track = target_track;
    status.track = target_track;
    status.trk0 = target_track == 0;

    // Determine majority decoded cylinder from sectors if available
    let detected_track_id = if !effective_diag.sectors.is_empty() {
        let mut cyl_counts: HashMap<u8, usize> = HashMap::new();
        for sec in &effective_diag.sectors {
            *cyl_counts.entry(sec.cyl).or_insert(0) += 1;
        }
        cyl_counts
            .into_iter()
            .max_by_key(|&(_, cnt)| cnt)
            .map(|(cyl, _)| cyl)
            .unwrap_or(target_track)
    } else {
        target_track
    };

    // Re-evaluate on_track and off_track statistics against target_track if sectors are present
    if !effective_diag.sectors.is_empty() {
        let mut on_track: u32 = 0;
        let mut off_track: u32 = 0;
        let mut crc_errs: u32 = 0;
        let mut wrong_cylinders: HashMap<u8, u32> = HashMap::new();

        for s in &effective_diag.sectors {
            if !s.crc_ok {
                crc_errs += 1;
            }
            if s.cyl == target_track {
                if s.crc_ok {
                    on_track += 1;
                }
            } else {
                off_track += 1;
                *wrong_cylinders.entry(s.cyl).or_insert(0) += 1;
            }
        }

        effective_diag.on_track_count = on_track;
        effective_diag.off_track_count = off_track;
        effective_diag.crc_err_count = crc_errs;
        let expected_count = if effective_diag.sector_count > 0 {
            effective_diag.sector_count
        } else {
            18
        };
        effective_diag.alignment_pct = if expected_count > 0 {
            (on_track as f32 / expected_count as f32) * 100.0
        } else {
            0.0
        };
        effective_diag.off_track_details = if off_track == 0 {
            String::from("NONE (Perfect)")
        } else if wrong_cylinders.len() == 1 && on_track == 0 {
            let (&cyl, _) = wrong_cylinders.iter().next().unwrap();
            format!("MISALIGNED T:{:02}", cyl)
        } else {
            let mut parts = Vec::new();
            for (cyl, cnt) in wrong_cylinders {
                parts.push(format!("T{:02}: {} sect", cyl, cnt));
            }
            parts.join(", ")
        };
    }

    status.has_disk = effective_diag.has_disk;
    status.bitrate = effective_diag.bitrate;
    status.density = effective_diag.bitrate == 500;
    status.sector_count = effective_diag.sector_count;
    status.sectors_known = effective_diag.sectors_known;
    status.on_track_count = effective_diag.on_track_count;
    status.off_track_count = effective_diag.off_track_count;
    status.off_track_details = effective_diag.off_track_details.clone();
    status.crc_err_count = effective_diag.crc_err_count;
    status.alignment_pct = effective_diag.alignment_pct;
    status.read_status = effective_diag.read_status;

    status.sectors.clear();
    for sec in &effective_diag.sectors {
        status.sectors.push(SectorInfo {
            track: sec.cyl,
            sec_id: sec.sec_id,
            size_code: sec.size_code,
            status_code: if sec.crc_ok { 0 } else { 1 },
            crc_ok: sec.crc_ok,
        });
    }

    let verbose_line = build_verbose_pass_line(target_track, status.head, &effective_diag);
    let standard_line = build_standard_pass_line(target_track, status.head, &effective_diag);

    let ok_count = effective_diag
        .sectors
        .iter()
        .filter(|s| s.crc_ok && s.cyl == target_track)
        .count() as u8;
    let expected_count = effective_diag.sector_count;
    let is_ok = effective_diag.sectors_known
        && !effective_diag.sectors.is_empty()
        && effective_diag.crc_err_count == 0
        && effective_diag.off_track_count == 0
        && ok_count >= expected_count
        && detected_track_id == target_track;

    let quality_pct = effective_diag.pll_quality_pct.unwrap_or(if is_ok { 99 } else { 50 });
    let crc_errors = effective_diag.crc_err_count.min(255) as u8;

    let pass = DiagnosticPass {
        track: target_track,
        track_id: detected_track_id,
        head: status.head,
        bitrate: effective_diag.bitrate,
        line_standard: standard_line.clone(),
        line_verbose: verbose_line.clone(),
        ok_count,
        valid_sectors: ok_count,
        expected_count,
        crc_errors,
        quality_pct,
        is_ok,
    };

    if status.head == 0 {
        status.last_pass_h0 = Some(pass);
    } else {
        status.last_pass_h1 = Some(pass);
    }

    if status.head_select == HeadSelection::Both {
        let both_metrics = crate::app::App::compute_both_metrics_from_passes(
            status.last_pass_h0.as_ref(),
            status.last_pass_h1.as_ref(),
            status.sector_count,
        );
        status.alignment_pct = both_metrics.alignment_pct;
        status.on_track_count = both_metrics.total_ok;
        status.off_track_count = both_metrics.total_off_track;
        status.off_track_details = both_metrics.off_track_details.clone();
        status.crc_err_count = both_metrics.total_crc_err;
    }

    if status.in_progress_pass && !status.sector_log_standard.is_empty() && !status.sector_log_verbose.is_empty() {
        if let Some(last) = status.sector_log_standard.last_mut() {
            *last = standard_line;
        }
        if let Some(last) = status.sector_log_verbose.last_mut() {
            *last = verbose_line;
        }
        status.in_progress_pass = false;
    } else {
        status.sector_log_verbose.push(verbose_line);
        status.sector_log_standard.push(standard_line);
    }

    if status.sector_log_verbose.len() > 50 {
        let excess = status.sector_log_verbose.len() - 50;
        status.sector_log_verbose.drain(0..excess);
    }
    if status.sector_log_standard.len() > 50 {
        let excess = status.sector_log_standard.len() - 50;
        status.sector_log_standard.drain(0..excess);
    }

    status.sector_log = if status.verbose_mode {
        status.sector_log_verbose.clone()
    } else {
        status.sector_log_standard.clone()
    };

    // Alignment radar audio variometer feedback
    if status.beep_enabled {
        if let Some(event) = evaluate_alignment_audio_event(
            status.head_select,
            status.target_track,
            status.head,
            status.last_pass_h0.as_ref(),
            status.last_pass_h1.as_ref(),
            status.sector_count,
        ) {
            let _ = tx_sound.send(event);
        }
    }
}

pub struct RpmSampler {
    samples: VecDeque<u32>,
    max_samples: usize,
}

impl RpmSampler {
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }

    pub fn add_sample(&mut self, rpm: u32) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front();
        }
        self.samples.push_back(rpm);
    }

    pub fn average(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let sum: u32 = self.samples.iter().sum();
        sum / (self.samples.len() as u32)
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

pub fn handle_command(
    port: &mut Box<dyn serialport::SerialPort>,
    status: &mut DriveStatus,
    cmd: HwCmd,
    rx_cmd: &Receiver<HwCmd>,
    tx_status: &Sender<DriveStatus>,
) -> bool {
    let mut should_exit = false;
    match cmd {
        HwCmd::Exit => {
            should_exit = true;
        }
        HwCmd::PanicReset => {
            // 1. Drain pending command channel
            while rx_cmd.try_recv().is_ok() {}

            // 2. Clear all port buffers
            let _ = port.clear(serialport::ClearBuffer::All);

            // 3. Immediate hardware emergency shutdown:
            // - Cut drive motor: MOTOR_OFF (0x06 with state 0)
            // - Deselect head: Head 0 (0x03 with head 0)
            // - Re-sync bus & select unit
            let _ = gw_send_raw(port, &[0x06, 0x04, status.drive_unit, 0x00], 0);
            let _ = gw_send_raw(port, &[0x03, 0x03, 0x00], 0);
            let _ = gw_send_raw(port, &[0x00, 0x03, 0x00], 32);
            let _ = gw_send_raw(port, &[0x0E, 0x03, 0x01], 0);
            let _ = gw_send_raw(port, &[0x0C, 0x03, status.drive_unit], 0);

            // 4. Reset software state cleanly to safe stopped state
            status.motor_on = false;
            status.analyzing = false;
            status.mode = DisplayMode::None;
            status.activity = HwActivity::Stopped;
            status.rpm = 0;
            status.rpm_display = String::from("--- RPM");
            status.index = false;
            status.sectors.clear();
            status.sectors_known = false;
            status.head_select = HeadSelection::Head0;
            status.head = 0;
            status.log_msg = String::from("*** EMERGENCY STOP (BACKSPACE): Motor stopped & hardware reset ***");
            let _ = tx_status.send(status.clone());
        }
        HwCmd::Stop => {
            let _ = port.clear(serialport::ClearBuffer::All);
            let _ = port.set_timeout(Duration::from_millis(100));
            let _ = gw_send_raw(
                port,
                &[0x06, 0x04, status.drive_unit, 0x00],
                0,
            );
            let _ = port.set_timeout(Duration::from_millis(100));
            status.motor_on = false;
            status.analyzing = false;
            if status.mode == DisplayMode::RpmMeasure && status.rpm_measure.sample_count > 0 {
                status.rpm_display = format!("{:.1} RPM", status.rpm_measure.avg_rpm);
            }
            status.mode = DisplayMode::None;
            status.activity = HwActivity::Stopped;
            status.index = false;
            status.log_msg = String::from("Stop / Motor OFF (Safe to change disk)");
            let _ = tx_status.send(status.clone());
        }
        HwCmd::MeasureRpm => {
            let _ = port.clear(serialport::ClearBuffer::All);
            if status.mode == DisplayMode::RpmMeasure {
                let _ = gw_send_raw(port, &[0x00, 0x03, 0x00], 32);
                ensure_unit_active(port, status.drive_unit, status.motor_on, status.head);
                if status.rpm_measure.sample_count > 0 {
                    status.rpm_display = format!("{:.1} RPM", status.rpm_measure.avg_rpm);
                }
                status.mode = DisplayMode::None;
                status.activity = if status.motor_on {
                    HwActivity::Idle
                } else {
                    HwActivity::Stopped
                };
                status.log_msg = String::from("Live RPM test stopped");
            } else {
                if !status.motor_on {
                    let _ = gw_send_raw(port, &[0x0E, 0x03, 0x01], 0);
                    let _ = gw_send_raw(port, &[0x0C, 0x03, status.drive_unit], 0);
                    let _ = gw_send_raw(port, &[0x03, 0x03, status.head], 0);
                    let _ = gw_set_motor(port, status.drive_unit, true);
                    status.motor_on = true;
                    thread::sleep(Duration::from_millis(SPIN_UP_DELAY_MS));
                    let _ = port.clear(serialport::ClearBuffer::Input);
                }
                status.analyzing = false;
                status.mode = DisplayMode::RpmMeasure;
                status.activity = HwActivity::MeasuringRpm;
                status.rpm_measure.clear();
                status.log_msg = String::from("Live RPM Test: High-precision continuous measurement for fine mechanical tuning");
            }
            let _ = tx_status.send(status.clone());
        }
        HwCmd::ToggleMotor => {
            let new_state = !status.motor_on;
            let _ = port.clear(serialport::ClearBuffer::All);
            let _ = gw_send_raw(port, &[0x0E, 0x03, 0x01], 0);
            let _ = gw_send_raw(port, &[0x0C, 0x03, status.drive_unit], 0);
            let _ = gw_send_raw(port, &[0x03, 0x03, status.head], 0);
            let _ = port.clear(serialport::ClearBuffer::Input);
            let res = gw_set_motor(port, status.drive_unit, new_state);

            match res {
                Ok((0, _)) => {
                    status.motor_on = new_state;
                    if !new_state {
                        // Non-destructive stop: keep sectors, track, head, and rpm_display intact
                        status.index = false;
                        status.analyzing = false;
                        if status.mode != DisplayMode::None {
                            if status.mode == DisplayMode::RpmMeasure && status.rpm_measure.sample_count > 0 {
                                status.rpm_display = format!("{:.1} RPM", status.rpm_measure.avg_rpm);
                            }
                            status.mode = DisplayMode::None;
                        }
                        status.activity = HwActivity::Stopped;
                        status.log_msg = String::from("Motor OFF (M key)");
                    } else {
                        status.drive_select = true;
                        status.has_disk = true;
                        status.activity = HwActivity::Idle;
                        status.log_msg = String::from("Motor ON (M key)");
                    }
                }
                Ok((st, _)) => {
                    status.log_msg = format!("Motor Toggle Error (code {})", st);
                }
                Err(e) => {
                    status.log_msg = format!("Motor Toggle I/O Error: {}", e);
                }
            }
            let _ = tx_status.send(status.clone());
        }
        HwCmd::Seek(target_track) => {
            let track = target_track.min(83);
            let old_track = status.track;

            // 1. Immediately set status.track = track and clear status.sectors
            status.target_track = track;
            status.track = track;
            status.trk0 = track == 0;
            status.sectors.clear();
            status.sectors_known = false;
            status.last_pass_h0 = None;
            status.last_pass_h1 = None;
            status.activity = HwActivity::Seeking;
            let _ = tx_status.send(status.clone());

            // 2. Issue motor-gated SEEK(target) command to Greaseweazle and wait for ACK
            let adaptive_timeout = if track == 0 {
                Duration::from_millis(SEEK_TRK0_TIMEOUT_MS)
            } else {
                Duration::from_millis(calculate_seek_timeout_ms(old_track, track))
            };
            let _ = port.set_timeout(adaptive_timeout);

            let res = perform_motor_gated_seek(
                port,
                status.drive_unit,
                track,
                status.motor_on,
                status.head,
                adaptive_timeout,
            );
            let _ = port.set_timeout(Duration::from_millis(100));

            match res {
                Ok((0, _)) => {
                    status.log_msg = format!("Seek -> Track {}", track);
                }
                Ok((st, _)) => {
                    status.log_msg = format!("Seek Error (code {})", st);
                }
                Err(e) => {
                    status.log_msg = format!("Seek I/O Error: {}", e);
                }
            }

            // 3. Mandatory UART input buffer purge to eliminate residual flux samples from previous track
            let _ = port.clear(serialport::ClearBuffer::Input);

            // 4. Apply mechanical stabilization delay of 30 ms (HEAD_SETTLE_TIME_MS)
            thread::sleep(Duration::from_millis(HEAD_SETTLE_TIME_MS));

            // Non-blocking WP query on Seek
            if let Some(wp) = query_write_protect(
                port,
                status.drive_unit,
                status.motor_on,
                status.head,
            ) {
                status.write_protect = wp;
                status.write_protected = wp;
            }

            // Reset sector buffer of the current pass and clear input buffer
            status.sectors.clear();
            status.sectors_known = false;
            let _ = port.clear(serialport::ClearBuffer::Input);

            if status.mode == DisplayMode::RpmMeasure {
                let _ = port.clear(serialport::ClearBuffer::All);
                let _ = gw_send_raw(port, &[0x00, 0x03, 0x00], 32);
                ensure_unit_active(port, status.drive_unit, status.motor_on, status.head);
                if status.rpm_measure.sample_count > 0 {
                    status.rpm_display = format!("{:.1} RPM", status.rpm_measure.avg_rpm);
                }
                status.mode = DisplayMode::None;
                status.activity = if status.motor_on {
                    HwActivity::Idle
                } else {
                    HwActivity::Stopped
                };
            } else {
                status.activity = if status.analyzing {
                    HwActivity::ReadingAnalyzing
                } else if status.motor_on {
                    HwActivity::Idle
                } else {
                    HwActivity::Stopped
                };
            }
            let _ = tx_status.send(status.clone());
        }
        HwCmd::RecalibrateSeek => {
            status.activity = HwActivity::Seeking;
            let _ = tx_status.send(status.clone());

            // 1. Save starting track
            let origin = status.track;

            // 2. Purge input buffer
            let _ = port.clear(serialport::ClearBuffer::Input);

            // 3. Motor-gated seek sequence for recalibration (wakes up stepper if motor is off)
            let gated = !status.motor_on;
            if gated {
                let _ = gw_send_raw(port, &[0x06, 0x04, status.drive_unit, 0x01], 0);
                thread::sleep(Duration::from_millis(STEPPER_WAKEUP_DELAY_MS));
            }

            // 4. Seek to Track 0 (SEEK_TRACK0): timeout 3000 ms, track 0 dwell time 60 ms
            let _ = port.set_timeout(Duration::from_millis(SEEK_TRK0_TIMEOUT_MS));
            let res0 = gw_send_timeout(
                port,
                &[0x02, 0x03, 0x00],
                0,
                status.drive_unit,
                true,
                status.head,
                Duration::from_millis(SEEK_TRK0_TIMEOUT_MS),
            );
            thread::sleep(Duration::from_millis(DWELL_TIME_TRK0_MS));

            // 5. Immediate return to origin track
            if res0.is_ok() {
                if origin > 0 {
                    let t_back = Duration::from_millis(1200 + (origin as u64 * 25));
                    let _ = port.set_timeout(t_back);
                    let res_back = gw_send_timeout(
                        port,
                        &[0x02, 0x03, origin],
                        0,
                        status.drive_unit,
                        true,
                        status.head,
                        t_back,
                    );
                    thread::sleep(Duration::from_millis(HEAD_SETTLE_TIME_MS));
                    match res_back {
                        Ok((0, _)) => {
                            status.target_track = origin;
                            status.track = origin;
                            status.trk0 = origin == 0;
                            status.sectors.clear();
                            status.sectors_known = false;
                            status.last_pass_h0 = None;
                            status.last_pass_h1 = None;
                            status.log_msg = format!(
                                "Recalibrate Track 0 -> Track {}",
                                origin
                            );
                        }
                        Ok((st, _)) => {
                            status.log_msg = format!("Recalibrate Error (code {})", st);
                        }
                        Err(e) => {
                            status.log_msg = format!("Recalibrate I/O Error: {}", e);
                        }
                    }
                } else {
                    // origin == 0: Step-out clearance cycle: Seek(2) -> wait 30 ms -> Seek(0) -> wait 35 ms
                    let _ = port.set_timeout(Duration::from_millis(1250));
                    let _ = gw_send_timeout(
                        port,
                        &[0x02, 0x03, 2],
                        0,
                        status.drive_unit,
                        true,
                        status.head,
                        Duration::from_millis(1200 + (2 * 25)),
                    );
                    thread::sleep(Duration::from_millis(30));

                    let _ = port.set_timeout(Duration::from_millis(SEEK_TRK0_TIMEOUT_MS));
                    let res_zero = gw_send_timeout(
                        port,
                        &[0x02, 0x03, 0],
                        0,
                        status.drive_unit,
                        true,
                        status.head,
                        Duration::from_millis(SEEK_TRK0_TIMEOUT_MS),
                    );
                    thread::sleep(Duration::from_millis(35));

                    match res_zero {
                        Ok((0, _)) => {
                            status.target_track = 0;
                            status.track = 0;
                            status.trk0 = true;
                            status.sectors.clear();
                            status.sectors_known = false;
                            status.log_msg = String::from("Recalibrate Track 0 -> Track 0");
                        }
                        Ok((st, _)) => {
                            status.log_msg = format!("Recalibrate Error (code {})", st);
                        }
                        Err(e) => {
                            status.log_msg = format!("Recalibrate I/O Error: {}", e);
                        }
                    }
                }
            } else {
                match res0 {
                    Ok((st, _)) => {
                        status.log_msg = format!("Recalibrate Track 0 Error (code {})", st);
                    }
                    Err(e) => {
                        status.log_msg = format!("Recalibrate Track 0 I/O Error: {}", e);
                    }
                }
            }
            let _ = port.set_timeout(Duration::from_millis(100));

            if gated {
                let _ = gw_send_raw(port, &[0x06, 0x04, status.drive_unit, 0x00], 0);
            }

            // 6. Purge buffer and emit tx_status.send(status.clone());
            let _ = port.clear(serialport::ClearBuffer::Input);
            status.sectors.clear();
            status.sectors_known = false;

            // Non-blocking WP query on Recalibrate
            if let Some(wp) = query_write_protect(
                port,
                status.drive_unit,
                status.motor_on,
                status.head,
            ) {
                status.write_protect = wp;
                status.write_protected = wp;
            }

            if status.mode == DisplayMode::RpmMeasure {
                let _ = port.clear(serialport::ClearBuffer::All);
                let _ = gw_send_raw(port, &[0x00, 0x03, 0x00], 32);
                ensure_unit_active(port, status.drive_unit, status.motor_on, status.head);
                status.mode = DisplayMode::None;
                status.activity = if status.motor_on {
                    HwActivity::Idle
                } else {
                    HwActivity::Stopped
                };
            } else {
                status.activity = if status.analyzing {
                    HwActivity::ReadingAnalyzing
                } else if status.motor_on {
                    HwActivity::Idle
                } else {
                    HwActivity::Stopped
                };
            }
            let _ = tx_status.send(status.clone());
        }
        HwCmd::ZeroTrack => {
            if status.mode == DisplayMode::RpmMeasure {
                let _ = port.clear(serialport::ClearBuffer::All);
                let _ = gw_send_raw(port, &[0x00, 0x03, 0x00], 32);
                ensure_unit_active(port, status.drive_unit, status.motor_on, status.head);
                if status.rpm_measure.sample_count > 0 {
                    status.rpm_display = format!("{:.1} RPM", status.rpm_measure.avg_rpm);
                }
                status.mode = DisplayMode::None;
                status.activity = if status.motor_on {
                    HwActivity::Idle
                } else {
                    HwActivity::Stopped
                };
            }
            status.activity = HwActivity::Seeking;
            let _ = tx_status.send(status.clone());

            let _ = port.set_timeout(Duration::from_millis(SEEK_TRK0_TIMEOUT_MS));
            let res = perform_motor_gated_seek(
                port,
                status.drive_unit,
                0,
                status.motor_on,
                status.head,
                Duration::from_millis(SEEK_TRK0_TIMEOUT_MS),
            );
            let _ = port.set_timeout(Duration::from_millis(100));

            match res {
                Ok((0, _)) => {
                    status.target_track = 0;
                    status.track = 0;
                    status.trk0 = true;
                    status.sectors.clear();
                    status.sectors_known = false;
                    status.last_pass_h0 = None;
                    status.last_pass_h1 = None;
                    status.log_msg = String::from("Zero Track -> Track 00");
                }
                Ok((st, _)) => {
                    status.log_msg = format!("Zero Track Error (code {})", st);
                }
                Err(e) => {
                    status.log_msg = format!("Zero Track I/O Error: {}", e);
                }
            }

            // Recalibrate / Track 0 stabilization wait
            thread::sleep(Duration::from_millis(RECALIBRATE_WAIT_MS));

            // Non-blocking WP query on ZeroTrack
            if let Some(wp) = query_write_protect(
                port,
                status.drive_unit,
                status.motor_on,
                status.head,
            ) {
                status.write_protect = wp;
                status.write_protected = wp;
            }

            // Reset sector buffer and clear input buffer
            status.sectors.clear();
            status.sectors_known = false;
            let _ = port.clear(serialport::ClearBuffer::Input);

            if status.mode == DisplayMode::RpmMeasure {
                status.mode = DisplayMode::None;
                status.activity = if status.motor_on {
                    HwActivity::Idle
                } else {
                    HwActivity::Stopped
                };
            } else {
                status.activity = if status.analyzing {
                    HwActivity::ReadingAnalyzing
                } else if status.motor_on {
                    HwActivity::Idle
                } else {
                    HwActivity::Stopped
                };
            }
            let _ = tx_status.send(status.clone());
        }
        HwCmd::SetHead(head_val) => {
            let (selection, head) = if head_val > 0 {
                (HeadSelection::Head1, 1)
            } else {
                (HeadSelection::Head0, 0)
            };
            let res = gw_send(
                port,
                &[0x03, 0x03, head],
                0,
                status.drive_unit,
                status.motor_on,
                head,
            );

            match res {
                Ok((0, _)) => {
                    status.head_select = selection;
                    status.head = head;
                    status.sectors.clear();
                    status.sectors_known = false;
                    status.log_msg = format!("Head set -> {}", head);
                }
                Ok((st, _)) => {
                    status.log_msg = format!("Head {} Error (code {})", head, st);
                }
                Err(e) => {
                    status.log_msg = format!("Head I/O Error: {}", e);
                }
            }

            // Head switch settle time (1 ms)
            thread::sleep(Duration::from_millis(HEAD_SWITCH_SETTLE_MS));

            // Reset sector buffer and clear input buffer
            status.sectors.clear();
            status.sectors_known = false;
            let _ = port.clear(serialport::ClearBuffer::Input);
            let _ = tx_status.send(status.clone());
        }
        HwCmd::SetHeadSelection(selection) => {
            status.head_select = selection;
            let target_physical_head = match status.head_select {
                HeadSelection::Head0 => 0,
                HeadSelection::Head1 => 1,
                HeadSelection::Both => 0,
            };
            let res = gw_send(
                port,
                &[0x03, 0x03, target_physical_head],
                0,
                status.drive_unit,
                status.motor_on,
                target_physical_head,
            );

            match res {
                Ok((0, _)) => {
                    status.head = target_physical_head;
                    status.sectors.clear();
                    status.sectors_known = false;
                    status.log_msg = match status.head_select {
                        HeadSelection::Head0 => String::from("Head selection -> Head 0"),
                        HeadSelection::Head1 => String::from("Head selection -> Head 1"),
                        HeadSelection::Both => String::from("Head selection -> BOTH (0+1) [Alternating mode]"),
                    };
                }
                Ok((st, _)) => {
                    status.log_msg = format!("Head Error (code {})", st);
                }
                Err(e) => {
                    status.log_msg = format!("Head I/O Error: {}", e);
                }
            }

            // Head switch settle time (1 ms)
            thread::sleep(Duration::from_millis(HEAD_SWITCH_SETTLE_MS));

            // Reset sector buffer and clear input buffer
            status.sectors.clear();
            status.sectors_known = false;
            let _ = port.clear(serialport::ClearBuffer::Input);
            let _ = tx_status.send(status.clone());
        }
        HwCmd::ToggleHead => {
            status.head_select = status.head_select.toggle_next();
            let target_physical_head = match status.head_select {
                HeadSelection::Head0 => 0,
                HeadSelection::Head1 => 1,
                HeadSelection::Both => 0,
            };
            let res = gw_send(
                port,
                &[0x03, 0x03, target_physical_head],
                0,
                status.drive_unit,
                status.motor_on,
                target_physical_head,
            );

            match res {
                Ok((0, _)) => {
                    status.head = target_physical_head;
                    status.sectors.clear();
                    status.sectors_known = false;
                    status.log_msg = match status.head_select {
                        HeadSelection::Head0 => String::from("Head selection -> Head 0"),
                        HeadSelection::Head1 => String::from("Head selection -> Head 1"),
                        HeadSelection::Both => String::from("Head selection -> BOTH (0+1) [Alternating mode]"),
                    };
                }
                Ok((st, _)) => {
                    status.log_msg = format!("Head Error (code {})", st);
                }
                Err(e) => {
                    status.log_msg = format!("Head I/O Error: {}", e);
                }
            }

            // Head switch settle time (1 ms)
            thread::sleep(Duration::from_millis(HEAD_SWITCH_SETTLE_MS));

            // Reset sector buffer and clear input buffer
            status.sectors.clear();
            status.sectors_known = false;
            let _ = port.clear(serialport::ClearBuffer::Input);
            let _ = tx_status.send(status.clone());
        }
        HwCmd::Analyze | HwCmd::StartAnalysis => {
            // 1. Mandatory immediate purge of all preceding USB packet remnants
            let _ = port.clear(serialport::ClearBuffer::All);

            if !status.motor_on {
                let _ = gw_send_raw(port, &[0x0E, 0x03, 0x01], 0);
                let _ = gw_send_raw(port, &[0x0C, 0x03, status.drive_unit], 0);
                let _ = gw_send_raw(port, &[0x03, 0x03, status.head], 0);
                let _ = gw_set_motor(port, status.drive_unit, true);
                status.motor_on = true;
                status.drive_select = true;
                // Allow physical time for disk to reach synchronous 300 RPM and stabilize index
                thread::sleep(Duration::from_millis(SPIN_UP_DELAY_MS));
                // Clear transient noise generated during motor acceleration
                let _ = port.clear(serialport::ClearBuffer::Input);
            }

            // Non-blocking WP query at start of Analyze
            if let Some(wp) = query_write_protect(
                port,
                status.drive_unit,
                status.motor_on,
                status.head,
            ) {
                status.write_protect = wp;
                status.write_protected = wp;
            }

            let _ = port.clear(serialport::ClearBuffer::Input);
            thread::sleep(Duration::from_millis(SYNC_DELAY_MS));

            status.analyzing = true;
            status.mode = DisplayMode::Analyze;
            status.activity = HwActivity::ReadingAnalyzing;
            status.log_msg = String::from("Analyze: Starting alignment diagnostics...");
            let _ = tx_status.send(status.clone());
        }
        HwCmd::ReadData => {
            // 1. Mandatory immediate purge of all preceding USB packet remnants
            let _ = port.clear(serialport::ClearBuffer::All);

            if !status.motor_on {
                let _ = gw_send_raw(port, &[0x0E, 0x03, 0x01], 0);
                let _ = gw_send_raw(port, &[0x0C, 0x03, status.drive_unit], 0);
                let _ = gw_send_raw(port, &[0x03, 0x03, status.head], 0);
                let _ = gw_set_motor(port, status.drive_unit, true);
                status.motor_on = true;
                status.drive_select = true;
                // Allow physical time for disk to reach synchronous 300 RPM and stabilize index
                thread::sleep(Duration::from_millis(SPIN_UP_DELAY_MS));
                // Clear transient noise generated during motor acceleration
                let _ = port.clear(serialport::ClearBuffer::Input);
            }

            // Non-blocking WP query at start of ReadData
            if let Some(wp) = query_write_protect(
                port,
                status.drive_unit,
                status.motor_on,
                status.head,
            ) {
                status.write_protect = wp;
                status.write_protected = wp;
            }

            let _ = port.clear(serialport::ClearBuffer::Input);
            thread::sleep(Duration::from_millis(SYNC_DELAY_MS));

            status.analyzing = true;
            status.mode = DisplayMode::ReadData;
            status.activity = HwActivity::ReadingAnalyzing;
            status.log_msg = String::from("read Data: Starting sector read...");
            let _ = tx_status.send(status.clone());
        }
        HwCmd::ToggleVerbose => {
            status.verbose_mode = !status.verbose_mode;
            status.sector_log = if status.verbose_mode {
                status.sector_log_verbose.clone()
            } else {
                status.sector_log_standard.clone()
            };
            status.log_msg = format!(
                "Verbose: {}",
                if status.verbose_mode { "ON" } else { "OFF" }
            );
            let _ = tx_status.send(status.clone());
        }
        HwCmd::ToggleBeep => {
            status.beep_enabled = !status.beep_enabled;
            status.log_msg = format!(
                "Beep: {}",
                if status.beep_enabled { "ON" } else { "OFF" }
            );
            let _ = tx_status.send(status.clone());
        }
        HwCmd::SetVerbose(v) => {
            status.verbose_mode = v;
            status.sector_log = if status.verbose_mode {
                status.sector_log_verbose.clone()
            } else {
                status.sector_log_standard.clone()
            };
            status.log_msg = format!(
                "Verbose: {}",
                if status.verbose_mode { "ON" } else { "OFF" }
            );
            let _ = tx_status.send(status.clone());
        }
        HwCmd::SetBeep(b) => {
            status.beep_enabled = b;
            status.log_msg = format!(
                "Beep: {}",
                if status.beep_enabled { "ON" } else { "OFF" }
            );
            let _ = tx_status.send(status.clone());
        }
        HwCmd::SetMotor(on) => {
            if status.motor_on == on {
                let _ = tx_status.send(status.clone());
            } else {
                let _ = port.clear(serialport::ClearBuffer::All);
                let _ = gw_send_raw(port, &[0x0E, 0x03, 0x01], 0);
                let _ = gw_send_raw(port, &[0x0C, 0x03, status.drive_unit], 0);
                let _ = gw_send_raw(port, &[0x03, 0x03, status.head], 0);
                let res = gw_set_motor(port, status.drive_unit, on);

                match res {
                    Ok((0, _)) => {
                        status.motor_on = on;
                        if !on {
                            status.index = false;
                            status.analyzing = false;
                            if status.mode != DisplayMode::None {
                                if status.mode == DisplayMode::RpmMeasure && status.rpm_measure.sample_count > 0 {
                                    status.rpm_display = format!("{:.1} RPM", status.rpm_measure.avg_rpm);
                                }
                                status.mode = DisplayMode::None;
                            }
                            status.activity = HwActivity::Stopped;
                            status.log_msg = String::from("Motor OFF");
                        } else {
                            status.drive_select = true;
                            status.has_disk = true;
                            status.activity = HwActivity::Idle;
                            status.log_msg = String::from("Motor ON");
                        }
                    }
                    Ok((st, _)) => {
                        status.log_msg = format!("Motor Error (code {})", st);
                    }
                    Err(e) => {
                        status.log_msg = format!("Motor I/O Error: {}", e);
                    }
                }
                let _ = tx_status.send(status.clone());
            }
        }
        HwCmd::ToggleDriveUnit => {
            let next_unit = if status.drive_unit == 0 { 1 } else { 0 };
            if status.motor_on {
                let _ = gw_send_raw(port, &[0x06, 0x04, status.drive_unit, 0x00], 0);
            }
            let _ = gw_send_raw(port, &[0x0D, 0x02], 0);
            status.drive_unit = next_unit;
            status.unit_id = next_unit;
            ensure_unit_active(
                port,
                status.drive_unit,
                status.motor_on,
                status.head,
            );
            status.drive_select = true;
            let _ = port.set_timeout(Duration::from_millis(SEEK_TRK0_TIMEOUT_MS));
            let _ = perform_motor_gated_seek(
                port,
                status.drive_unit,
                0,
                status.motor_on,
                status.head,
                Duration::from_millis(SEEK_TRK0_TIMEOUT_MS),
            );
            let _ = port.set_timeout(Duration::from_millis(100));
            thread::sleep(Duration::from_millis(RECALIBRATE_WAIT_MS));

            if let Some(wp) = query_write_protect(
                port,
                status.drive_unit,
                status.motor_on,
                status.head,
            ) {
                status.write_protect = wp;
                status.write_protected = wp;
            }

            status.track = 0;
            status.trk0 = true;
            status.sectors.clear();
            status.sectors_known = false;
            status.last_pass_h0 = None;
            status.last_pass_h1 = None;
            status.mode = DisplayMode::None;
            status.activity = if status.motor_on {
                HwActivity::Idle
            } else {
                HwActivity::Stopped
            };
            status.log_msg = format!(
                "Drive {} ({}) selected & Recalibrated Track 0",
                status.drive_unit,
                if status.drive_unit == 0 { "A:" } else { "B:" }
            );
            let _ = tx_status.send(status.clone());
        }
        HwCmd::SelectUnit(unit) => {
            let target_unit = unit.min(1);
            if target_unit != status.drive_unit {
                if status.motor_on {
                    let _ = gw_send_raw(port, &[0x06, 0x04, status.drive_unit, 0x00], 0);
                }
                let _ = gw_send_raw(port, &[0x0D, 0x02], 0);
                status.drive_unit = target_unit;
                status.unit_id = target_unit;
            }
            ensure_unit_active(
                port,
                status.drive_unit,
                status.motor_on,
                status.head,
            );
            status.drive_select = true;
            let _ = port.set_timeout(Duration::from_millis(SEEK_TRK0_TIMEOUT_MS));
            let _ = perform_motor_gated_seek(
                port,
                status.drive_unit,
                0,
                status.motor_on,
                status.head,
                Duration::from_millis(SEEK_TRK0_TIMEOUT_MS),
            );
            let _ = port.set_timeout(Duration::from_millis(100));
            thread::sleep(Duration::from_millis(RECALIBRATE_WAIT_MS));

            if let Some(wp) = query_write_protect(
                port,
                status.drive_unit,
                status.motor_on,
                status.head,
            ) {
                status.write_protect = wp;
                status.write_protected = wp;
            }

            status.track = 0;
            status.trk0 = true;
            status.sectors.clear();
            status.sectors_known = false;
            status.last_pass_h0 = None;
            status.last_pass_h1 = None;
            status.mode = DisplayMode::None;
            status.activity = if status.motor_on {
                HwActivity::Idle
            } else {
                HwActivity::Stopped
            };
            status.log_msg = format!(
                "Select Unit {} & Recalibrate Track 0",
                status.drive_unit
            );
            let _ = tx_status.send(status.clone());
        }
    }
    should_exit
}

pub fn hw_thread(
    tx_status: Sender<DriveStatus>,
    rx_cmd: Receiver<HwCmd>,
    port_arg: Option<String>,
    initial_drive_unit: u8,
) {
    let mut status = DriveStatus::default();
    if let Some(ref p) = port_arg {
        status.port_name = p.clone();
    }
    status.drive_unit = initial_drive_unit.min(1);
    status.unit_id = status.drive_unit;
    let mut rpm_sampler = RpmSampler::new(4);

    let (tx_sound, rx_sound) = crossbeam_channel::unbounded::<AudioEvent>();
    thread::spawn(move || sound_worker(rx_sound));

    loop {
        let port_name = match &port_arg {
            Some(name) => Some(name.clone()),
            None => find_greaseweazle(),
        };

        let mut should_exit = false;

        if let Some(name) = port_name {
            status.port_name = name.clone();
            match serialport::new(&name, 115_200)
                .timeout(Duration::from_millis(100))
                .open()
            {
                Ok(mut port) => {
                    let _ = port.write_data_terminal_ready(true);
                    let _ = port.write_request_to_send(true);
                    let _ = port.clear(serialport::ClearBuffer::All);

                    // Hardware worker startup sequence (1 to 5)
                    // 1. INFO (0x00)
                    let info_res = gw_send(
                        &mut port,
                        &[0x00, 0x03, 0x00],
                        32,
                        status.drive_unit,
                        status.motor_on,
                        status.head,
                    );
                    if info_res.is_err() {
                        shutdown_drive(&mut port, status.drive_unit);
                        status.connected = false;
                        status.activity = HwActivity::WaitingPort;
                        status.log_msg = format!("Port {} not responding to INFO", name);
                        let _ = tx_status.send(status.clone());
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    status.connected = true;
                    status.log_msg = format!("Connected to {}", name);
                    let _ = tx_status.send(status.clone());

                    // 2. SET_BUS ON (0x0E)
                    let bus_res = gw_send(
                        &mut port,
                        &[0x0E, 0x03, 0x01],
                        0,
                        status.drive_unit,
                        status.motor_on,
                        status.head,
                    );
                    if bus_res.is_err() {
                        shutdown_drive(&mut port, status.drive_unit);
                        status.connected = false;
                        status.activity = HwActivity::WaitingPort;
                        status.log_msg = format!("Port {} bus error", name);
                        let _ = tx_status.send(status.clone());
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    status.log_msg = format!("Bus enabled on {}", name);
                    let _ = tx_status.send(status.clone());

                    // 3. SELECT_UNIT (0x0C)
                    let _ = gw_send(
                        &mut port,
                        &[0x0C, 0x03, status.drive_unit],
                        0,
                        status.drive_unit,
                        status.motor_on,
                        status.head,
                    );
                    let _ = gw_send(
                        &mut port,
                        &[0x03, 0x03, status.head],
                        0,
                        status.drive_unit,
                        status.motor_on,
                        status.head,
                    );
                    status.drive_select = true;
                    status.has_disk = true;
                    status.log_msg = format!("Drive {} selected", status.drive_unit);
                    let _ = tx_status.send(status.clone());

                    // 4. SET_MOTOR ON (0x06)
                    let _ = gw_send(
                        &mut port,
                        &[0x06, 0x04, status.drive_unit, 0x01],
                        0,
                        status.drive_unit,
                        true,
                        status.head,
                    );
                    status.motor_on = true;
                    status.rpm_display = String::from("... RPM");
                    status.log_msg = String::from("Motor ON");
                    let _ = tx_status.send(status.clone());

                    // 5. SEEK 0 (0x02) - Recalibration timeout 3000 ms
                    let _ = port.set_timeout(Duration::from_millis(SEEK_TRK0_TIMEOUT_MS));
                    let _ = gw_send_timeout(
                        &mut port,
                        &[0x02, 0x03, 0x00],
                        0,
                        status.drive_unit,
                        status.motor_on,
                        status.head,
                        Duration::from_millis(SEEK_TRK0_TIMEOUT_MS),
                    );
                    let _ = port.set_timeout(Duration::from_millis(100));

                    // Wait 1 full rotation (200 ms) after initial recalibration
                    thread::sleep(Duration::from_millis(RECALIBRATE_WAIT_MS));

                    // Non-blocking WP query at initial connection
                    if let Some(wp) = query_write_protect(
                        &mut port,
                        status.drive_unit,
                        status.motor_on,
                        status.head,
                    ) {
                        status.write_protect = wp;
                        status.write_protected = wp;
                    }

                    status.track = 0;
                    status.trk0 = true;
                    status.activity = HwActivity::Idle;
                    status.log_msg = format!("Ready on {} - Track 0", name);
                    let _ = tx_status.send(status.clone());

                    let mut last_keepalive = Instant::now();

                    loop {
                        while let Ok(cmd) = rx_cmd.try_recv() {
                            if handle_command(
                                &mut port,
                                &mut status,
                                cmd,
                                &rx_cmd,
                                &tx_status,
                            ) {
                                should_exit = true;
                                break;
                            }
                            last_keepalive = Instant::now();
                        }

                        if should_exit {
                            shutdown_drive(&mut port, status.drive_unit);
                            break;
                        }

                        if status.mode == DisplayMode::RpmMeasure && status.motor_on {
                            status.activity = HwActivity::MeasuringRpm;
                            status.io_cycle = status.io_cycle.wrapping_add(1);

                            match read_motor_rpm_diagnostic(
                                &mut port,
                                status.drive_unit,
                                status.motor_on,
                                status.head,
                            ) {
                                Ok(decoded_gw) => {
                                    let mut recorded = false;
                                    if decoded_gw.index_timestamps.len() >= 2 {
                                        for i in 0..decoded_gw.index_timestamps.len() - 1 {
                                            let t0 = decoded_gw.index_timestamps[i];
                                            let t1 = decoded_gw.index_timestamps[i + 1];
                                            let delta = (t1.wrapping_sub(t0)) & 0x0FFF_FFFF;
                                            if delta > 0 {
                                                let rpm_float = (72_000_000.0 * 60.0) / delta as f64;
                                                let rpm = rpm_float.round() as u32;
                                                if (100..=800).contains(&rpm) {
                                                    status.rpm = rpm;
                                                    status.index = true;
                                                    status.has_disk = true;
                                                    status.rpm_measure.record_sample(rpm_float, delta);
                                                    status.rpm_display = format!("{:.1} RPM", status.rpm_measure.instant_rpm);
                                                    status.log_msg = format!("RPM: {:.1} RPM (Index Pin 8 OK)", status.rpm_measure.instant_rpm);
                                                    rpm_sampler.add_sample(rpm);
                                                    recorded = true;
                                                }
                                            }
                                        }
                                    }
                                    if !recorded {
                                        // Fallback Cas B: MFM sector header timing or No Index
                                        if let Some((rpm_u32, delta)) = calculate_rpm_from_mfm_headers(&decoded_gw.flux) {
                                            let rpm = (72_000_000.0 * 60.0) / (delta as f64);
                                            status.rpm = rpm_u32;
                                            status.index = false;
                                            status.has_disk = true;
                                            status.rpm_measure.record_sample(rpm, delta);
                                            status.rpm_display = format!("{:.1} RPM", status.rpm_measure.instant_rpm);
                                            status.log_msg = format!("RPM: {:.1} RPM (MFM header sync - No index Pin 8)", status.rpm_measure.instant_rpm);
                                            rpm_sampler.add_sample(status.rpm);
                                        } else {
                                            status.rpm = 0;
                                            status.index = false;
                                            status.log_msg = String::from("RPM: No Index pulse detected (Pin 8)");
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = port.clear(serialport::ClearBuffer::All);
                                    let _ = gw_send_raw(&mut port, &[0x00, 0x03, 0x00], 32);
                                    ensure_unit_active(&mut port, status.drive_unit, status.motor_on, status.head);
                                    status.index = false;
                                    status.log_msg = format!("RPM Test I/O Error: {}", e);
                                }
                            }
                            last_keepalive = Instant::now();
                            let _ = tx_status.send(status.clone());
                            thread::sleep(Duration::from_millis(15));
                        } else if status.analyzing
                            && status.motor_on
                        {
                            if status.head_select == HeadSelection::Both {
                                let next_head = if status.head == 0 { 1 } else { 0 };
                                let _ = gw_send(
                                    &mut port,
                                    &[0x03, 0x03, next_head],
                                    0,
                                    status.drive_unit,
                                    status.motor_on,
                                    next_head,
                                );
                                thread::sleep(Duration::from_millis(HEAD_SWITCH_SETTLE_MS));
                                let _ = port.clear(serialport::ClearBuffer::Input);
                                status.head = next_head;
                            }

                            status.activity = HwActivity::ReadingAnalyzing;
                            status.io_cycle = status.io_cycle.wrapping_add(1);

                            let diag =
                                read_and_decode_track_diagnostic(&mut port, status.track, &mut status, &tx_status);
                            for &r in &diag.instant_rpms {
                                rpm_sampler.add_sample(r);
                            }
                            if diag.has_disk && !diag.index_timestamps.is_empty() {
                                let avg = rpm_sampler.average();
                                if avg > 0 {
                                    status.rpm = avg;
                                }
                            }
                            status.index = !diag.index_timestamps.is_empty();

                            process_track_diagnostic(&mut status, &diag, &tx_sound);

                            if status.sectors_known {
                                if status.mode == DisplayMode::ReadData {
                                    status.log_msg = format!(
                                        "read Data: {} Sectors read (Diskette {}k {})",
                                        status.sectors.len(),
                                        diag.bitrate,
                                        if status.density { "1.44M HD" } else { "720K DD" }
                                    );
                                } else {
                                    status.log_msg = format!(
                                        "Analyze: {}k ({}) - {} real sectors (Alignment: {:.1}%)",
                                        diag.bitrate,
                                        if status.density { "1.44M HD" } else { "720K DD" },
                                        status.sectors.len(),
                                        diag.alignment_pct
                                    );
                                }
                            } else if !diag.has_disk || diag.read_status == DriveReadStatus::NoDiskOrNoIndex {
                                status.log_msg = format!("Analyze: Track {} -> No Disk / No Index pulse", status.track);
                            } else {
                                status.log_msg = format!("Analyze: Track {} -> ? (No valid IDAM sector found)", status.track);
                            }

                            if !diag.has_disk || diag.read_status == DriveReadStatus::NoDiskOrNoIndex {
                                thread::sleep(Duration::from_millis(20));
                            }
                            let _ = tx_status.send(status.clone());
                        }

                        if !status.motor_on {
                            status.index = false;
                            if status.activity != HwActivity::Seeking {
                                status.activity = HwActivity::Stopped;
                            }
                        }

                        if last_keepalive.elapsed() >= Duration::from_millis(500) {
                            if status.motor_on {
                                let _ = gw_send(
                                    &mut port,
                                    &[0x03, 0x03, status.head],
                                    0,
                                    status.drive_unit,
                                    status.motor_on,
                                    status.head,
                                );
                            }
                            last_keepalive = Instant::now();
                        }

                        if tx_status.send(status.clone()).is_err() {
                            shutdown_drive(&mut port, status.drive_unit);
                            break;
                        }

                        if !status.analyzing && status.mode != DisplayMode::RpmMeasure {
                            thread::sleep(Duration::from_millis(15));
                        }
                    }

                    shutdown_drive(&mut port, status.drive_unit);
                    status.connected = false;
                    status.activity = HwActivity::WaitingPort;
                    status.rpm = 0;
                    status.index = false;
                    status.log_msg = String::from("Disconnected");
                    let _ = tx_status.send(status.clone());
                }
                Err(e) => {
                    status.connected = false;
                    status.activity = HwActivity::WaitingPort;
                    status.rpm = 0;
                    status.index = false;
                    status.log_msg = format!("Port open error: {}", e);
                    let _ = tx_status.send(status.clone());
                    thread::sleep(Duration::from_millis(100));
                }
            }
        } else {
            status.connected = false;
            status.activity = HwActivity::WaitingPort;
            status.rpm = 0;
            status.index = false;
            status.log_msg = String::from("Searching for Greaseweazle serial port...");
            let _ = tx_status.send(status.clone());
            thread::sleep(Duration::from_millis(100));
        }

        if should_exit {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::calculate_radar_pitch;

    #[test]
    fn test_write_protect_status_fields() {
        let mut status = DriveStatus::default();
        assert!(status.write_protect);
        assert!(status.write_protected);

        status.write_protect = false;
        status.write_protected = false;
        assert!(!status.write_protect);
        assert!(!status.write_protected);
    }

    #[test]
    fn test_toggle_motor_immediate_state_transition() {
        let mut status = DriveStatus {
            motor_on: true,
            activity: HwActivity::Idle,
            ..Default::default()
        };

        // Simulate local immediate toggle
        status.motor_on = !status.motor_on;
        status.activity = HwActivity::Stopped;
        assert!(!status.motor_on);
        assert_eq!(status.activity, HwActivity::Stopped);

        // Toggle back
        status.motor_on = !status.motor_on;
        status.activity = HwActivity::Idle;
        assert!(status.motor_on);
        assert_eq!(status.activity, HwActivity::Idle);
    }


    #[test]
    fn test_drive_status_stop_state() {
        let mut status = DriveStatus {
            motor_on: true,
            analyzing: true,
            mode: DisplayMode::Analyze,
            activity: HwActivity::ReadingAnalyzing,
            rpm: 300,
            index: true,
            track: 40,
            head: 1,
            unit_id: 0,
            log_msg: String::from("Analyzing..."),
            ..Default::default()
        };

        status.motor_on = false;
        status.analyzing = false;
        status.mode = DisplayMode::None;
        status.activity = HwActivity::Stopped;
        status.rpm = 0;
        status.index = false;
        status.sectors.clear();
        status.sectors_known = false;
        status.log_msg = String::from("Stop / Motor OFF (Safe to change disk)");

        assert!(!status.motor_on);
        assert!(!status.analyzing);
        assert_eq!(status.mode, DisplayMode::None);
        assert_eq!(status.activity, HwActivity::Stopped);
        assert_eq!(status.rpm, 0);
        assert!(!status.index);
        assert_eq!(status.track, 40);
        assert_eq!(status.head, 1);
        assert_eq!(status.log_msg, "Stop / Motor OFF (Safe to change disk)");
    }

    #[test]
    fn test_process_track_diagnostic_default_standard_mode() {
        let (tx_sound, rx_sound) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 10,
            target_track: 10,
            head: 0,
            verbose_mode: false,
            beep_enabled: true,
            sector_count: 18,
            ..Default::default()
        };

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 3,
            sectors_known: true,
            sectors: vec![
                DecodedSector::new(10, 0, 1, 2, true),
                DecodedSector::new(10, 0, 2, 2, false),
                DecodedSector::new(9, 0, 3, 2, true),
            ],
            on_track_count: 2,
            off_track_count: 1,
            off_track_details: String::from("T09: 1 sect"),
            crc_err_count: 1,
            alignment_pct: 66.7,
            index_timestamps: vec![1000, 14401000],
            instant_rpms: vec![300],
            rev_time_ms: 200.0,
            ..Default::default()
        };

        process_track_diagnostic(&mut status, &diag, &tx_sound);

        assert_eq!(status.sector_log.len(), 1);
        assert_eq!(status.sector_log_standard.len(), 1);
        assert_eq!(status.sector_log_verbose.len(), 1);
        assert_eq!(status.sector_log_standard[0], "T:10 H:0 Rate:500k MFM [ ■ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] (1/18 CRC-DAT: Sec 2)");
        assert!(status.sector_log_verbose[0].contains("T:10 H:0 Rate:500k MFM [ ■ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ]"));
        assert!(status.sector_log_verbose[0].contains("CRC-DAT: Sec 2"));

        let beeps: Vec<AudioEvent> = rx_sound.try_iter().collect();
        assert_eq!(beeps, vec![AudioEvent::OffTrackOrCrcError]);
    }

    #[test]
    fn test_process_track_diagnostic_default_standard_mode_all_ok() {
        let (tx_sound, rx_sound) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 10,
            target_track: 10,
            head: 0,
            verbose_mode: false,
            beep_enabled: true,
            sector_count: 2,
            ..Default::default()
        };

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 2,
            sectors_known: true,
            sectors: vec![
                DecodedSector::new(10, 0, 1, 2, true),
                DecodedSector::new(10, 0, 2, 2, true),
            ],
            on_track_count: 2,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            index_timestamps: vec![1000, 14401000],
            instant_rpms: vec![300],
            rev_time_ms: 200.1,
            ..Default::default()
        };

        process_track_diagnostic(&mut status, &diag, &tx_sound);

        assert_eq!(status.sector_log.len(), 1);
        assert_eq!(status.sector_log_standard[0], "T:10 H:0 Rate:500k MFM [ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] (2/18 MISSING: Sec 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18)");
        assert!(status.sector_log_verbose[0].contains("T:10 H:0 Rate:500k MFM [ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ]"));
        assert!(!status.sector_log_verbose[0].contains("200.1ms"));
        assert!(status.sector_log_verbose[0].contains("Gap0:----"));

        let beeps: Vec<AudioEvent> = rx_sound.try_iter().collect();
        assert_eq!(beeps, vec![AudioEvent::PerfectAlignment { pitch_hz: calculate_radar_pitch(99) }]);
    }

    #[test]
    fn test_process_track_diagnostic_verbose_mode() {
        let (tx_sound, rx_sound) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 10,
            target_track: 10,
            head: 0,
            verbose_mode: true,
            beep_enabled: true,
            sector_count: 3,
            ..Default::default()
        };

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 3,
            sectors_known: true,
            sectors: vec![
                DecodedSector::new(10, 0, 4, 2, true),
                DecodedSector::new(10, 0, 5, 2, true),
                DecodedSector::new(10, 0, 6, 2, true),
            ],
            on_track_count: 3,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            index_timestamps: vec![1000, 14401000],
            instant_rpms: vec![300],
            rev_time_ms: 200.1,
            ..Default::default()
        };

        process_track_diagnostic(&mut status, &diag, &tx_sound);

        assert_eq!(status.sector_log.len(), 1);
        assert!(status.sector_log[0].contains("T:10 H:0 Rate:500k MFM [ ░ ░ ░ ■ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ]"));
        assert_eq!(status.sector_log_standard[0], "T:10 H:0 Rate:500k MFM [ ░ ░ ░ ■ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] (3/18 MISSING: Sec 1, 2, 3, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18)");

        let beeps: Vec<AudioEvent> = rx_sound.try_iter().collect();
        assert_eq!(beeps, vec![AudioEvent::PerfectAlignment { pitch_hz: calculate_radar_pitch(99) }]);
    }

    #[test]
    fn test_process_track_diagnostic_unknown_sectors() {
        let (tx_sound, rx_sound) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 5,
            target_track: 5,
            head: 1,
            head_select: HeadSelection::Head1,
            verbose_mode: false,
            beep_enabled: true,
            ..Default::default()
        };

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 0,
            sectors_known: false,
            sectors: Vec::new(),
            on_track_count: 0,
            off_track_count: 0,
            off_track_details: String::from("NONE"),
            crc_err_count: 0,
            alignment_pct: 0.0,
            index_timestamps: Vec::new(),
            instant_rpms: Vec::new(),
            rev_time_ms: 0.0,
            ..Default::default()
        };

        process_track_diagnostic(&mut status, &diag, &tx_sound);

        assert_eq!(status.sector_log.len(), 1);
        assert_eq!(status.sector_log_standard[0], "T:05 H:1 Rate:---k --- [ ? ]                                   (0/0 NO DATA / NO DISK)");
        assert_eq!(
            status.sector_log_verbose[0],
            "T:05 H:1 Rate:---k --- [ ? ]                                   (0/0 NO DATA / NO DISK) IL:--- Gap0:---- Q:--%"
        );

        let beeps: Vec<AudioEvent> = rx_sound.try_iter().collect();
        assert_eq!(beeps, vec![AudioEvent::OffTrackOrCrcError]);
    }

    #[test]
    fn test_process_track_diagnostic_verbose_with_crc_error() {
        let (tx_sound, rx_sound) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 10,
            target_track: 10,
            head: 0,
            verbose_mode: true,
            beep_enabled: true,
            sector_count: 2,
            ..Default::default()
        };

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 2,
            sectors_known: true,
            sectors: vec![
                DecodedSector::new(10, 0, 1, 2, false),
                DecodedSector::new(10, 0, 2, 2, true),
            ],
            on_track_count: 2,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 1,
            alignment_pct: 100.0,
            index_timestamps: vec![1000, 14401000],
            instant_rpms: vec![300],
            rev_time_ms: 200.0,
            ..Default::default()
        };

        process_track_diagnostic(&mut status, &diag, &tx_sound);

        assert_eq!(status.sector_log.len(), 1);
        assert!(status.sector_log[0].contains("T:10 H:0 Rate:500k MFM [ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ]"));
        assert!(status.sector_log[0].contains("CRC-DAT: Sec 1"));
        assert_eq!(status.sector_log_standard[0], "T:10 H:0 Rate:500k MFM [ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] (1/18 CRC-DAT: Sec 1)");

        let beeps: Vec<AudioEvent> = rx_sound.try_iter().collect();
        assert_eq!(beeps, vec![AudioEvent::OffTrackOrCrcError]);
    }

    #[test]
    fn test_process_track_diagnostic_beep_disabled() {
        let (tx_sound, rx_sound) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 10,
            target_track: 10,
            head: 0,
            verbose_mode: false,
            beep_enabled: false,
            sector_count: 1,
            ..Default::default()
        };

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 1,
            sectors_known: true,
            sectors: vec![DecodedSector::new(10, 0, 1, 2, true)],
            on_track_count: 1,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            index_timestamps: vec![1000, 14401000],
            instant_rpms: vec![300],
            rev_time_ms: 200.0,
            ..Default::default()
        };

        process_track_diagnostic(&mut status, &diag, &tx_sound);

        let beeps: Vec<AudioEvent> = rx_sound.try_iter().collect();
        assert!(beeps.is_empty());
    }

    #[test]
    fn test_verbose_pass_timing_formatting() {
        let (tx_sound, _rx_sound) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 20,
            head: 0,
            verbose_mode: true,
            ..Default::default()
        };

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 2,
            sectors_known: true,
            sectors: vec![
                DecodedSector::new(20, 0, 1, 2, true),
                DecodedSector::new(20, 0, 2, 2, true),
            ],
            on_track_count: 2,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            index_timestamps: vec![],
            instant_rpms: vec![],
            rev_time_ms: 204.3,
            ..Default::default()
        };

        process_track_diagnostic(&mut status, &diag, &tx_sound);

        assert_eq!(status.sector_log_verbose.len(), 1);
        assert!(status.sector_log_verbose[0].contains("T:20 H:0 Rate:500k MFM [ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ]"));
        assert!(!status.sector_log_verbose[0].contains("204.3ms"));
        assert!(status.sector_log_verbose[0].contains("Gap0:----"));
    }

    #[test]
    fn test_toggle_motor_non_destructive() {
        let mut status = DriveStatus {
            motor_on: true,
            track: 40,
            head: 1,
            rpm: 300,
            rpm_display: String::from("300.0 RPM"),
            sectors: vec![SectorInfo {
                track: 40,
                sec_id: 1,
                size_code: 2,
                status_code: 0,
                crc_ok: true,
            }],
            sectors_known: true,
            ..Default::default()
        };

        // Simulate motor off non-destructive logic
        status.motor_on = false;
        status.index = false;
        status.analyzing = false;
        if status.mode != DisplayMode::None {
            status.mode = DisplayMode::None;
        }
        status.activity = HwActivity::Stopped;

        assert!(!status.motor_on);
        assert_eq!(status.track, 40);
        assert_eq!(status.head, 1);
        assert_eq!(status.rpm_display, "300.0 RPM");
        assert_eq!(status.sectors.len(), 1);
        assert!(status.sectors_known);
    }

    #[test]
    fn test_rpm_average_persistence_across_transitions() {
        let mut status = DriveStatus {
            motor_on: true,
            mode: DisplayMode::RpmMeasure,
            activity: HwActivity::MeasuringRpm,
            ..Default::default()
        };

        status.rpm_measure.record_sample(300.2, 14390000);
        status.rpm_measure.record_sample(299.8, 14410000);

        assert_eq!(status.rpm_measure.sample_count, 2);
        assert!((status.rpm_measure.avg_rpm - 300.0).abs() < 0.1);

        // Transition from RPM test to None when motor stays on
        if status.mode == DisplayMode::RpmMeasure && status.rpm_measure.sample_count > 0 {
            status.rpm_display = format!("{:.1} RPM", status.rpm_measure.avg_rpm);
        }
        status.mode = DisplayMode::None;
        status.activity = HwActivity::Idle;

        assert_eq!(status.rpm_display, "300.0 RPM");
        assert_eq!(status.mode, DisplayMode::None);
        assert_eq!(status.activity, HwActivity::Idle);
    }

    #[test]
    fn test_calculate_rpm_hardware_direct() {
        // 72_000_000 ticks/sec / 14_400_000 ticks = 5.0 rev/sec = 300 RPM
        let timestamps = vec![1_000_000, 15_400_000, 29_800_000];
        let rpms = calculate_rpm_from_index_timestamps(&timestamps, 72_000_000.0);
        assert_eq!(rpms.len(), 2);
        assert_eq!(rpms[0], 300);
        assert_eq!(rpms[1], 300);
    }

    #[test]
    fn test_calculate_rpm_floats_hardware_direct() {
        let timestamps = vec![1_000_000, 15_400_000, 29_800_000];
        let rpms = calculate_rpm_floats_from_index_timestamps(&timestamps, 72_000_000.0);
        assert_eq!(rpms.len(), 2);
        assert!((rpms[0] - 300.0).abs() < 0.001);
        assert!((rpms[1] - 300.0).abs() < 0.001);
    }

    #[test]
    fn test_rpm_measurement_statistics() {
        let mut meas = RpmMeasurement::new();
        meas.record_sample(300.0, 14_400_000);
        meas.record_sample(302.0, 14_300_000);
        meas.record_sample(298.0, 14_500_000);

        assert_eq!(meas.sample_count, 3);
        assert_eq!(meas.min_rpm, 298.0);
        assert_eq!(meas.max_rpm, 302.0);
        assert!((meas.avg_rpm - 300.0).abs() < 0.01);
        assert_eq!(meas.jitter_rpm, 2.0);
        assert!((meas.jitter_pct - ((4.0 / 600.0) * 100.0)).abs() < 0.01);
        assert_eq!(meas.recent_samples.len(), 3);
    }

    #[test]
    fn test_adaptive_seek_timeout_calculation() {
        assert!(calculate_seek_timeout_ms(0, 0) >= 1200);
        assert_eq!(calculate_seek_timeout_ms(0, 0), 1200);
        assert_eq!(calculate_seek_timeout_ms(0, 1), 1225);
        assert!(calculate_seek_timeout_ms(0, 49) >= 2400);
        assert_eq!(calculate_seek_timeout_ms(0, 49), 2425);
        assert!(calculate_seek_timeout_ms(80, 0) >= 3200);
        assert_eq!(calculate_seek_timeout_ms(80, 0), 3200);
        assert_eq!(calculate_seek_timeout_ms(0, 80), 3200);
    }

    #[test]
    fn test_tachometer_transition_and_stop() {
        let mut status = DriveStatus {
            motor_on: true,
            mode: DisplayMode::RpmMeasure,
            activity: HwActivity::MeasuringRpm,
            ..Default::default()
        };

        status.rpm_measure.record_sample(300.5, 14380000);
        assert_eq!(status.rpm_measure.sample_count, 1);

        // Stop via Stop cmd
        status.motor_on = false;
        status.analyzing = false;
        if status.mode == DisplayMode::RpmMeasure && status.rpm_measure.sample_count > 0 {
            status.rpm_display = format!("{:.1} RPM", status.rpm_measure.avg_rpm);
        }
        status.mode = DisplayMode::None;
        status.activity = HwActivity::Stopped;
        status.index = false;

        assert!(!status.motor_on);
        assert_eq!(status.mode, DisplayMode::None);
        assert_eq!(status.activity, HwActivity::Stopped);
        assert_eq!(status.rpm_display, "300.5 RPM");
    }

    #[test]
    fn test_rpm_sampler_smoothing() {
        let mut sampler = RpmSampler::new(3);
        sampler.add_sample(300);
        sampler.add_sample(302);
        sampler.add_sample(298);
        assert_eq!(sampler.average(), 300);

        sampler.add_sample(306);
        assert_eq!(sampler.average(), 302);
    }

    #[test]
    fn test_decode_gw_flux_with_index_packets() {
        // [0x00, 0x01, b0, b1, b2, b3, 0x00, 0x00]
        let raw = vec![
            0x00, 0x01, 0x02, 0x00, 0x00, 0x00, // Index 1: ts = 1
            100, 100,
            0x00, 0x01, 0x82, 0x00, 0x00, 0x00, // Index 2: ts = 65
            0x00, 0x00, // End
        ];
        let decoded = decode_gw_flux_with_index(&raw);
        assert_eq!(decoded.index_timestamps.len(), 2);
        assert_eq!(decoded.index_timestamps[0], 1);
        assert_eq!(decoded.index_timestamps[1], 65);
        assert_eq!(decoded.flux.len(), 2);
        assert_eq!(decoded.flux[0], 100);
        assert_eq!(decoded.flux[1], 100);
    }

    #[test]
    fn test_decode_gw_flux_with_space_packets() {
        // Opcode 2 = Space
        let raw = vec![
            0x00, 0x02, 0x02, 0x00, 0x00, 0x00, // Space +1 tick
            50,
            0x00, 0x00,
        ];
        let decoded = decode_gw_flux_with_index(&raw);
        assert_eq!(decoded.flux.len(), 1);
        assert_eq!(decoded.flux[0], 51);
    }

    #[test]
    fn test_rpm_persistence_while_motor_on() {
        let mut status = DriveStatus {
            motor_on: true,
            mode: DisplayMode::RpmMeasure,
            activity: HwActivity::MeasuringRpm,
            ..Default::default()
        };

        status.rpm_measure.record_sample(300.0, 14400000);
        assert_eq!(status.rpm_measure.sample_count, 1);

        // When switching out of RPM measure mode
        if status.mode == DisplayMode::RpmMeasure && status.rpm_measure.sample_count > 0 {
            status.rpm_display = format!("{:.1} RPM", status.rpm_measure.avg_rpm);
        }
        status.mode = DisplayMode::None;
        status.activity = HwActivity::Idle;

        assert_eq!(status.rpm_display, "300.0 RPM");
        assert_eq!(status.mode, DisplayMode::None);
    }

    #[test]
    fn test_calculate_rpm_case_a_pin8_index() {
        let timestamps = vec![1_000_000, 15_400_000];
        let rpms = calculate_rpm_from_index_timestamps(&timestamps, 72_000_000.0);
        assert_eq!(rpms.len(), 1);
        assert_eq!(rpms[0], 300);
    }

    #[test]
    fn test_calculate_rpm_case_b_mfm_fallback() {
        let flux: Vec<u32> = Vec::new();
        assert!(calculate_rpm_from_mfm_headers(&flux).is_none());
    }

    #[test]
    fn test_rpm_measure_interruption_on_action_keys() {
        let mut status = DriveStatus {
            motor_on: true,
            mode: DisplayMode::RpmMeasure,
            activity: HwActivity::MeasuringRpm,
            ..Default::default()
        };
        status.rpm_measure.record_sample(300.0, 14400000);

        // Interruption by Seek
        if status.mode == DisplayMode::RpmMeasure {
            if status.rpm_measure.sample_count > 0 {
                status.rpm_display = format!("{:.1} RPM", status.rpm_measure.avg_rpm);
            }
            status.mode = DisplayMode::None;
            status.activity = HwActivity::Seeking;
        }

        assert_eq!(status.mode, DisplayMode::None);
        assert_eq!(status.rpm_display, "300.0 RPM");
        assert_eq!(status.activity, HwActivity::Seeking);
    }

    #[test]
    fn test_panic_reset_cmd_structure() {
        let (tx_cmd, rx_cmd) = crossbeam_channel::unbounded::<HwCmd>();
        tx_cmd.send(HwCmd::PanicReset).unwrap();
        assert!(matches!(rx_cmd.try_recv().unwrap(), HwCmd::PanicReset));
    }

    #[test]
    fn test_electromechanical_delays_constants() {
        assert_eq!(HEAD_SWITCH_SETTLE_MS, 1);
        assert_eq!(STEPPER_WAKEUP_DELAY_MS, 15);
        assert_eq!(HEAD_SETTLE_TIME_MS, 30);
        assert_eq!(DEFAULT_SERIAL_TIMEOUT_MS, 1000);
        assert_eq!(SPIN_UP_DELAY_MS, 350);
        assert_eq!(SYNC_DELAY_MS, 30);
        assert_eq!(DWELL_TIME_TRK0_MS, 60);
        assert_eq!(SEEK_TRK0_TIMEOUT_MS, 3000);
        assert_eq!(RECALIBRATE_WAIT_MS, 200);
    }

    #[test]
    fn test_auto_realign_track_from_sector_headers() {
        let (tx_sound, rx_sound) = crossbeam_channel::unbounded();
        // Software state is out-of-sync: status.track = 70, but physical head reads track 10
        let mut status = DriveStatus {
            track: 70,
            target_track: 10,
            head: 0,
            verbose_mode: false,
            beep_enabled: true,
            sector_count: 18,
            ..Default::default()
        };

        // All 18 decoded sectors have cyl: 10
        let sectors_track10: Vec<DecodedSector> = (1..=18)
            .map(|id| DecodedSector::new(10, 0, id, 2, true))
            .collect();

        // Initial diag was decoded with expected_cyl = 70, so all 18 sectors appeared as off-track T10
        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 18,
            sectors_known: true,
            sectors: sectors_track10,
            on_track_count: 0,
            off_track_count: 18,
            off_track_details: String::from("T10: 18 sect"),
            crc_err_count: 0,
            alignment_pct: 0.0,
            index_timestamps: vec![1000, 14401000],
            instant_rpms: vec![300],
            rev_time_ms: 200.0,
            ..Default::default()
        };

        process_track_diagnostic(&mut status, &diag, &tx_sound);

        // 1. status.track is automatically realigned to 10
        assert_eq!(status.track, 10);
        assert!(!status.trk0);

        // 2. Stats are re-evaluated to 100% on-track
        assert_eq!(status.on_track_count, 18);
        assert_eq!(status.off_track_count, 0);
        assert_eq!(status.off_track_details, "NONE (Perfect)");
        assert_eq!(status.alignment_pct, 100.0);

        // 3. Standard and Verbose log lines are rendered for T:10 without off-track markers
        assert_eq!(status.sector_log_standard.len(), 1);
        assert_eq!(
            status.sector_log_standard[0],
            "T:10 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)"
        );
        assert!(status.sector_log_verbose[0].contains("T:10 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)"));

        // 4. Audio confirmation confirms perfect pass
        let beeps: Vec<AudioEvent> = rx_sound.try_iter().collect();
        assert_eq!(beeps, vec![AudioEvent::PerfectAlignment { pitch_hz: calculate_radar_pitch(99) }]);
    }

    #[test]
    fn test_seek_dynamic_track_synchronization_in_analyze_mode() {
        let (tx_sound, _rx_sound) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 0,
            head: 0,
            motor_on: true,
            analyzing: true,
            mode: DisplayMode::Analyze,
            activity: HwActivity::ReadingAnalyzing,
            sector_count: 18,
            ..Default::default()
        };

        // User jumps to track 70 while analyzing
        let target_track: u8 = 70;

        // 1. Seeking state
        status.activity = HwActivity::Seeking;

        // 2. Physical move completes -> immediate logical sync
        status.track = target_track;
        status.trk0 = target_track == 0;
        status.sectors.clear();
        status.sectors_known = false;
        status.log_msg = format!("Seek -> Track {}", target_track);

        assert_eq!(status.track, 70);
        assert!(!status.trk0);
        assert!(status.sectors.is_empty());
        assert!(!status.sectors_known);

        // 3. First revolution read on target track 70
        let sectors_track70: Vec<DecodedSector> = (1..=18)
            .map(|id| DecodedSector::new(70, 0, id, 2, true))
            .collect();

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 18,
            sectors_known: true,
            sectors: sectors_track70,
            on_track_count: 18,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            index_timestamps: vec![1000, 14401000],
            instant_rpms: vec![300],
            rev_time_ms: 200.0,
            ..Default::default()
        };

        process_track_diagnostic(&mut status, &diag, &tx_sound);

        // Verify line is cleanly formatted for T:70 and green (no [T70] off-track artifact)
        assert_eq!(status.sector_log_standard.len(), 1);
        assert_eq!(
            status.sector_log_standard[0],
            "T:70 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)"
        );
        assert_eq!(status.on_track_count, 18);
        assert_eq!(status.off_track_count, 0);
        assert_eq!(status.alignment_pct, 100.0);
    }

    #[test]
    fn test_motor_spin_up_initialization_delays() {
        let mut status = DriveStatus {
            motor_on: false,
            analyzing: false,
            mode: DisplayMode::None,
            activity: HwActivity::Stopped,
            unit_id: 0,
            head: 0,
            ..Default::default()
        };

        // When receiving Analyze or ReadData from cold motor state:
        // 1. Motor & unit activation
        status.motor_on = true;
        status.drive_select = true;
        assert_eq!(SPIN_UP_DELAY_MS, 350);
        assert_eq!(SYNC_DELAY_MS, 30);

        // 2. State transition to Analyze
        status.analyzing = true;
        status.mode = DisplayMode::Analyze;
        status.activity = HwActivity::ReadingAnalyzing;

        assert!(status.motor_on);
        assert!(status.drive_select);
        assert!(status.analyzing);
        assert_eq!(status.mode, DisplayMode::Analyze);
        assert_eq!(status.display_mode(), DisplayMode::Analyze);
        assert_eq!(status.activity, HwActivity::ReadingAnalyzing);
    }

    #[test]
    fn test_start_analysis_synchronous_spinup_sequence() {
        let mut status = DriveStatus {
            motor_on: false,
            analyzing: false,
            mode: DisplayMode::None,
            activity: HwActivity::Stopped,
            drive_unit: 0,
            unit_id: 0,
            head: 0,
            ..Default::default()
        };

        // Simulating HwCmd::StartAnalysis / HwCmd::Analyze from cold motor
        let was_motor_off = !status.motor_on;
        assert!(was_motor_off);

        // 1. Motor activation & drive select
        status.motor_on = true;
        status.drive_select = true;
        // 2. Hardware stabilization delay (350 ms)
        assert_eq!(SPIN_UP_DELAY_MS, 350);
        // 3. State transition to Analyze
        status.analyzing = true;
        status.mode = DisplayMode::Analyze;
        status.activity = HwActivity::ReadingAnalyzing;

        assert!(status.motor_on);
        assert!(status.analyzing);
        assert_eq!(status.mode, DisplayMode::Analyze);
        assert_eq!(status.display_mode(), DisplayMode::Analyze);
        assert_eq!(status.activity, HwActivity::ReadingAnalyzing);
    }

    #[test]
    fn test_software_pll_reset_and_decode() {
        let mut pll = SoftwarePll::new(72.0);
        assert_eq!(pll.clock, 72.0);
        assert_eq!(pll.phase_accumulator, 0.0);

        // Simulate some state deviation
        pll.clock = 75.0;
        pll.phase_accumulator = 12.5;

        // Reset must cleanly restore initial parameters
        pll.reset();
        assert_eq!(pll.clock, 72.0);
        assert_eq!(pll.phase_accumulator, 0.0);

        // Test decoding with synthetic ideal flux (alternating 144 ticks -> 01 01 01)
        let ideal_flux = vec![144, 144, 144, 144];
        let bits = pll.decode_flux(&ideal_flux);
        assert!(!bits.is_empty());
        assert_eq!(pll.clock, 72.0);
        assert_eq!(pll.phase_accumulator, 0.0);
    }

    #[test]
    fn test_analysis_persistence_across_isolated_read_failures() {
        let (tx_sound, _rx_sound) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            motor_on: true,
            analyzing: true,
            mode: DisplayMode::Analyze,
            activity: HwActivity::ReadingAnalyzing,
            drive_unit: 0,
            unit_id: 0,
            head: 0,
            track: 40,
            ..Default::default()
        };

        // An isolated pass fails or has missing index / no disk
        let diag_failed = TrackAnalysisResult {
            has_disk: false,
            bitrate: 500,
            sector_count: 0,
            sectors_known: false,
            sectors: Vec::new(),
            on_track_count: 0,
            off_track_count: 0,
            off_track_details: String::from("NONE"),
            crc_err_count: 0,
            alignment_pct: 0.0,
            index_timestamps: Vec::new(),
            instant_rpms: Vec::new(),
            rev_time_ms: 200.0,
            rpm_instant: None,
            jitter_pct: None,
            gap0_us: None,
            pll_quality_pct: None,
            interleave: None,
            read_status: DriveReadStatus::NoDiskOrNoIndex,
        };

        process_track_diagnostic(&mut status, &diag_failed, &tx_sound);

        // Verify analysis state is strictly preserved
        assert!(status.analyzing);
        assert_eq!(status.mode, DisplayMode::Analyze);
        assert_eq!(status.display_mode(), DisplayMode::Analyze);
        assert!(status.motor_on);
        assert_eq!(status.activity, HwActivity::ReadingAnalyzing);
    }

    #[test]
    fn test_panic_reset_state_transition() {
        let mut status = DriveStatus {
            motor_on: true,
            analyzing: true,
            mode: DisplayMode::Analyze,
            activity: HwActivity::ReadingAnalyzing,
            rpm: 300,
            rpm_display: "300.0 RPM".to_string(),
            head: 1,
            ..DriveStatus::default()
        };

        // Apply Panic Reset state logic
        status.motor_on = false;
        status.analyzing = false;
        status.mode = DisplayMode::None;
        status.activity = HwActivity::Stopped;
        status.rpm = 0;
        status.rpm_display = String::from("--- RPM");
        status.index = false;
        status.sectors.clear();
        status.sectors_known = false;
        status.head = 0;

        assert!(!status.motor_on);
        assert!(!status.analyzing);
        assert_eq!(status.mode, DisplayMode::None);
        assert_eq!(status.activity, HwActivity::Stopped);
        assert_eq!(status.rpm, 0);
        assert_eq!(status.rpm_display, "--- RPM");
        assert_eq!(status.head, 0);
    }

    #[test]
    fn test_verbose_ribbon_format_hd_18_sectors() {
        let sectors: Vec<DecodedSector> = (1..=18)
            .map(|id| DecodedSector::new(79, 0, id, 2, true))
            .collect();

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 18,
            sectors_known: true,
            sectors,
            on_track_count: 18,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            index_timestamps: vec![1000, 14401000, 28802000],
            instant_rpms: vec![300],
            rev_time_ms: 200.1,
            rpm_instant: Some(299.8),
            jitter_pct: Some(0.2),
            gap0_us: Some(1440),
            pll_quality_pct: Some(99),
            interleave: Some("1:1".to_string()),
            read_status: DriveReadStatus::Ok,
        };

        let line = build_verbose_pass_line(79, 0, &diag);
        assert_eq!(
            line,
            "T:79 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK) IL:1:1 Gap0:1440µs Q:99%"
        );
    }

    #[test]
    fn test_verbose_ribbon_format_dd_9_sectors() {
        let sectors: Vec<DecodedSector> = (1..=9)
            .map(|id| DecodedSector::new(40, 0, id, 2, true))
            .collect();

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 250,
            sector_count: 9,
            sectors_known: true,
            sectors,
            on_track_count: 9,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            index_timestamps: vec![1000, 14401000, 28802000],
            instant_rpms: vec![300],
            rev_time_ms: 200.0,
            rpm_instant: Some(300.1),
            jitter_pct: Some(0.1),
            gap0_us: Some(2880),
            pll_quality_pct: Some(98),
            interleave: Some("1:1".to_string()),
            read_status: DriveReadStatus::Ok,
        };

        let line = build_verbose_pass_line(40, 0, &diag);
        assert_eq!(
            line,
            "T:40 H:0 Rate:250k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ]                   (9/9 OK)   IL:1:1 Gap0:2880µs Q:98%"
        );
    }

    #[test]
    fn test_verbose_ribbon_format_hd_15_sectors() {
        let sectors: Vec<DecodedSector> = (1..=15)
            .map(|id| DecodedSector::new(40, 0, id, 2, true))
            .collect();

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 15,
            sectors_known: true,
            sectors,
            on_track_count: 15,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            index_timestamps: vec![1000, 12001000, 24002000],
            instant_rpms: vec![360],
            rev_time_ms: 166.7,
            rpm_instant: Some(360.0),
            jitter_pct: Some(0.2),
            gap0_us: Some(1440),
            pll_quality_pct: Some(97),
            interleave: Some("1:1".to_string()),
            read_status: DriveReadStatus::Ok,
        };

        let line = build_verbose_pass_line(40, 0, &diag);
        assert_eq!(
            line,
            "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ]       (15/15 OK) IL:1:1 Gap0:1440µs Q:97%"
        );
    }

    #[test]
    fn test_verbose_ribbon_format_with_crc_data_error() {
        let sectors: Vec<DecodedSector> = (1..=18)
            .map(|id| {
                if id == 15 {
                    DecodedSector::with_status(35, 0, id, 2, SectorStatus::CrcData)
                } else {
                    DecodedSector::new(35, 0, id, 2, true)
                }
            })
            .collect();

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 18,
            sectors_known: true,
            sectors,
            on_track_count: 18,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 1,
            alignment_pct: 100.0,
            index_timestamps: vec![1000, 14401000],
            instant_rpms: vec![300],
            rev_time_ms: 200.2,
            rpm_instant: Some(299.7),
            jitter_pct: None,
            gap0_us: Some(1440),
            pll_quality_pct: Some(84),
            interleave: Some("1:1".to_string()),
            read_status: DriveReadStatus::Ok,
        };

        let line = build_verbose_pass_line(35, 0, &diag);
        assert_eq!(
            line,
            "T:35 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (17/18 CRC-DAT: Sec 15) IL:1:1 Gap0:1440µs Q:84%"
        );
    }

    #[test]
    fn test_verbose_ribbon_format_unformatted_track() {
        let diag = TrackAnalysisResult {
            has_disk: false,
            bitrate: 500,
            sector_count: 0,
            sectors_known: false,
            sectors: Vec::new(),
            on_track_count: 0,
            off_track_count: 0,
            off_track_details: String::from("NONE"),
            crc_err_count: 0,
            alignment_pct: 0.0,
            index_timestamps: vec![1000, 14401000],
            instant_rpms: vec![300],
            rev_time_ms: 200.3,
            rpm_instant: Some(299.5),
            jitter_pct: None,
            gap0_us: None,
            pll_quality_pct: None,
            interleave: None,
            read_status: DriveReadStatus::NoDiskOrNoIndex,
        };

        let line = build_verbose_pass_line(80, 0, &diag);
        assert_eq!(
            line,
            "T:80 H:0 Rate:---k --- [ ? ]                                   (0/0 NO DATA / NO DISK) IL:--- Gap0:---- Q:--%"
        );
    }

    #[test]
    fn test_verbose_line_no_rpm_fields() {
        let sectors: Vec<DecodedSector> = (1..=18)
            .map(|id| DecodedSector::new(40, 0, id, 2, true))
            .collect();

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 18,
            sectors_known: true,
            sectors,
            on_track_count: 18,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            index_timestamps: vec![1000, 14401000],
            instant_rpms: vec![300],
            rev_time_ms: 200.1,
            rpm_instant: Some(299.8),
            jitter_pct: Some(0.2),
            gap0_us: Some(1440),
            pll_quality_pct: Some(98),
            interleave: Some("1:1".to_string()),
            read_status: DriveReadStatus::Ok,
        };

        let line = build_verbose_pass_line(40, 0, &diag);
        assert!(!line.contains("RPM"), "Verbose line must not contain RPM");
        assert!(!line.contains("ms"), "Verbose line must not contain duration in ms");
        assert!(!line.contains('±'), "Verbose line must not contain jitter +/-");
        assert!(line.contains("Gap0:1440µs"));
        assert!(line.contains("Q:98%"));
        assert!(line.contains("IL:1:1"));
    }

    #[test]
    fn test_adaptive_gap0_bounds() {
        // 500 kbps (HD): [800 µs, 2200 µs]
        assert_eq!(get_valid_gap0(500, 1440), Some(1440));
        assert_eq!(get_valid_gap0(500, 800), Some(800));
        assert_eq!(get_valid_gap0(500, 2200), Some(2200));
        assert_eq!(get_valid_gap0(500, 799), None);
        assert_eq!(get_valid_gap0(500, 2201), None);

        // 250 kbps (DD): [1600 µs, 4400 µs]
        assert_eq!(get_valid_gap0(250, 2880), Some(2880));
        assert_eq!(get_valid_gap0(250, 1600), Some(1600));
        assert_eq!(get_valid_gap0(250, 4400), Some(4400));
        assert_eq!(get_valid_gap0(250, 1599), None);
        assert_eq!(get_valid_gap0(250, 4401), None);

        // 300 kbps: [1300 µs, 3600 µs]
        assert_eq!(get_valid_gap0(300, 2400), Some(2400));
        assert_eq!(get_valid_gap0(300, 1300), Some(1300));
        assert_eq!(get_valid_gap0(300, 3600), Some(3600));
        assert_eq!(get_valid_gap0(300, 1299), None);
        assert_eq!(get_valid_gap0(300, 3601), None);
    }

    #[test]
    fn test_pll_quality_rms_jitter() {
        // Ideal 500k pulses (72 clock ticks = 144, 216, 288 for 2T, 3T, 4T) -> RMS Jitter = 0 -> Q = 100%
        let ideal_flux = vec![144, 144, 216, 288, 144, 216];
        let q_ideal = calculate_pll_quality(&ideal_flux, 72.0);
        assert_eq!(q_ideal, 100);

        // Low jitter (Green tier: Q >= 85)
        let low_jitter_flux = vec![142, 146, 214, 290, 143, 218];
        let q_low = calculate_pll_quality(&low_jitter_flux, 72.0);
        assert!(q_low >= 85);
        assert!(q_low <= 100);

        // Moderate jitter (Yellow tier: 60 <= Q < 85)
        let noisy_flux = vec![136, 152, 210, 296, 140, 220];
        let q_noisy = calculate_pll_quality(&noisy_flux, 72.0);
        assert!(q_noisy < q_low);
        assert!(q_noisy >= 60);

        // With CRC error penalty (-15)
        let q_noisy_crc = calculate_pll_quality_with_crc(&noisy_flux, 72.0, true);
        assert_eq!(q_noisy_crc, q_noisy.saturating_sub(15));
    }

    #[test]
    fn test_live_rpm_statistics_window() {
        let mut meas = RpmMeasurement::new();
        for _ in 0..10 {
            meas.record_sample(300.0, 14_400_000);
        }
        assert_eq!(meas.sample_count, 10);
        assert_eq!(meas.avg_rpm, 300.0);
        assert_eq!(meas.min_rpm, 300.0);
        assert_eq!(meas.max_rpm, 300.0);
        assert_eq!(meas.jitter_rpm, 0.0);
        assert_eq!(meas.jitter_pct, 0.0);

        // Add 5 samples of 310.0 -> window of 10 has 5x 300.0 and 5x 310.0 -> avg = 305.0
        for _ in 0..5 {
            meas.record_sample(310.0, 13_935_483);
        }
        assert_eq!(meas.sample_count, 15);
        assert_eq!(meas.avg_rpm, 305.0);
        assert_eq!(meas.min_rpm, 300.0);
        assert_eq!(meas.max_rpm, 310.0);
        assert_eq!(meas.jitter_rpm, 5.0);
        assert!((meas.jitter_pct - (10.0 / (2.0 * 305.0) * 100.0)).abs() < 0.01);
    }

    #[test]
    fn test_calculate_jitter_pct() {
        let timestamps = vec![1_000_000, 15_400_000, 29_820_000];
        // delta0 = 14_400_000, delta1 = 14_420_000 -> diff = 20_000 / 14_400_000 = ~0.14% -> 0.1%
        let jitter = calculate_jitter_pct(&timestamps);
        assert!(jitter.is_some());
        assert!((jitter.unwrap() - 0.1).abs() < 0.05);
    }

    #[test]
    fn test_progressive_revolution_initial_state() {
        let (tx_status, rx_status) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 40,
            head: 0,
            sector_count: 18,
            ..Default::default()
        };

        start_revolution_progress(&mut status, &tx_status, 40, 0, 500, 18);

        assert!(status.in_progress_pass);
        assert_eq!(status.sector_log_standard.len(), 1);
        assert_eq!(
            status.sector_log_standard[0],
            "T:40 H:0 Rate:500k MFM [ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] ( 0/18)"
        );
        assert!(status.sectors.is_empty());

        let emitted = rx_status.try_recv().unwrap();
        assert_eq!(
            emitted.sector_log_standard[0],
            "T:40 H:0 Rate:500k MFM [ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] ( 0/18)"
        );
    }

    #[test]
    fn test_progressive_revolution_incremental_updates() {
        let (tx_status, rx_status) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 40,
            head: 0,
            sector_count: 18,
            ..Default::default()
        };

        start_revolution_progress(&mut status, &tx_status, 40, 0, 500, 18);
        let _ = rx_status.try_recv();

        // Decode Sector 1
        let sec1 = DecodedSector::new(40, 0, 1, 2, true);
        update_revolution_progress(&mut status, &tx_status, 40, 0, 500, 18, 1, Some(&sec1));
        assert_eq!(status.sector_log_standard.len(), 1);
        assert_eq!(
            status.sector_log_standard[0],
            "T:40 H:0 Rate:500k MFM [ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] ( 1/18)"
        );
        assert_eq!(status.sectors.len(), 1);

        // Decode Sector 5
        let sec5 = DecodedSector::new(40, 0, 5, 2, true);
        update_revolution_progress(&mut status, &tx_status, 40, 0, 500, 18, 5, Some(&sec5));
        assert_eq!(status.sector_log_standard.len(), 1);
        assert_eq!(
            status.sector_log_standard[0],
            "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] ( 5/18)"
        );
        assert_eq!(status.sectors.len(), 2);

        // Decode Sector 18 (all sectors)
        let sec18 = DecodedSector::new(40, 0, 18, 2, true);
        update_revolution_progress(&mut status, &tx_status, 40, 0, 500, 18, 18, Some(&sec18));
        assert_eq!(status.sector_log_standard.len(), 1);
        assert_eq!(
            status.sector_log_standard[0],
            "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)"
        );
    }

    #[test]
    fn test_progressive_revolution_finalize_and_multi_revolution_scroll() {
        let (tx_status, _) = crossbeam_channel::unbounded();
        let (tx_sound, _) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 40,
            head: 0,
            sector_count: 18,
            beep_enabled: false,
            ..Default::default()
        };

        // Revolution 1: Start
        start_revolution_progress(&mut status, &tx_status, 40, 0, 500, 18);
        assert_eq!(status.sector_log_standard.len(), 1);

        // Revolution 1: Decode 18 sectors
        let sectors: Vec<DecodedSector> = (1..=18)
            .map(|id| DecodedSector::new(40, 0, id, 2, true))
            .collect();
        for (i, sec) in sectors.iter().enumerate() {
            update_revolution_progress(&mut status, &tx_status, 40, 0, 500, 18, i + 1, Some(sec));
        }
        assert_eq!(status.sector_log_standard.len(), 1);

        // Revolution 1: Finalize
        let diag1 = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 18,
            sectors_known: true,
            sectors: sectors.clone(),
            on_track_count: 18,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            index_timestamps: vec![1000, 14401000],
            instant_rpms: vec![300],
            rev_time_ms: 200.1,
            ..Default::default()
        };
        process_track_diagnostic(&mut status, &diag1, &tx_sound);

        assert_eq!(status.sector_log_standard.len(), 1);
        assert!(!status.in_progress_pass);
        assert_eq!(
            status.sector_log_standard[0],
            "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)"
        );

        // Revolution 2: Start new revolution
        start_revolution_progress(&mut status, &tx_status, 40, 0, 500, 18);
        assert_eq!(status.sector_log_standard.len(), 2);
        assert!(status.in_progress_pass);

        // Line 0 is frozen in history
        assert_eq!(
            status.sector_log_standard[0],
            "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)"
        );
        // Line 1 is the new active line being swept
        assert_eq!(
            status.sector_log_standard[1],
            "T:40 H:0 Rate:500k MFM [ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] ( 0/18)"
        );
    }

    #[test]
    fn test_process_track_diagnostic_both_mode_consolidation() {
        let (tx_sound, _) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 40,
            head_select: HeadSelection::Both,
            head: 0,
            sector_count: 18,
            ..Default::default()
        };

        // Head 0 pass: 18/18 OK
        let sectors_h0: Vec<DecodedSector> = (1..=18)
            .map(|id| DecodedSector::new(40, 0, id, 2, true))
            .collect();
        let diag_h0 = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 18,
            sectors_known: true,
            sectors: sectors_h0,
            on_track_count: 18,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            ..Default::default()
        };
        process_track_diagnostic(&mut status, &diag_h0, &tx_sound);
        assert!(status.last_pass_h0.is_some());

        // Head 1 pass: 0/18 OK (degraded)
        status.head = 1;
        let diag_h1 = TrackAnalysisResult {
            has_disk: false,
            bitrate: 500,
            sector_count: 0,
            sectors_known: false,
            sectors: Vec::new(),
            on_track_count: 0,
            off_track_count: 0,
            off_track_details: String::from("NONE"),
            crc_err_count: 0,
            alignment_pct: 0.0,
            read_status: DriveReadStatus::NoDiskOrNoIndex,
            ..Default::default()
        };
        process_track_diagnostic(&mut status, &diag_h1, &tx_sound);
        assert!(status.last_pass_h1.is_some());

        // Both mode alignment should immediately be 50.0% (18/36)
        assert_eq!(status.alignment_pct, 50.0);
        assert_eq!(status.on_track_count, 18);
    }

    #[test]
    fn test_head_selection_transitions_and_alternation_both_mode() {
        let mut status = DriveStatus {
            track: 20,
            head_select: HeadSelection::Head0,
            head: 0,
            ..Default::default()
        };

        // 1. Toggle from Head0 -> Head1
        status.head_select = status.head_select.toggle_next();
        assert_eq!(status.head_select, HeadSelection::Head1);

        // 2. Toggle from Head1 -> Both
        status.head_select = status.head_select.toggle_next();
        assert_eq!(status.head_select, HeadSelection::Both);

        // 3. Toggle from Both -> Head0
        status.head_select = status.head_select.toggle_next();
        assert_eq!(status.head_select, HeadSelection::Head0);

        // Test alternating physical head simulation in Both mode
        status.head_select = HeadSelection::Both;
        status.head = 0;

        // Simulated continuous read cycle 1: Head 0 -> alternates to Head 1
        let next_head = if status.head == 0 { 1 } else { 0 };
        status.head = next_head;
        assert_eq!(status.head, 1);

        // Simulated continuous read cycle 2: Head 1 -> alternates back to Head 0
        let next_head = if status.head == 0 { 1 } else { 0 };
        status.head = next_head;
        assert_eq!(status.head, 0);
    }

    #[test]
    fn test_drive_read_status_no_disk_or_no_index_handling() {
        let (tx_sound, _rx_sound) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 10,
            head: 0,
            has_disk: true,
            analyzing: true,
            motor_on: true,
            ..Default::default()
        };

        let diag_no_disk = TrackAnalysisResult {
            has_disk: false,
            bitrate: 500,
            sector_count: 0,
            sectors_known: false,
            sectors: Vec::new(),
            on_track_count: 0,
            off_track_count: 0,
            off_track_details: String::from("NONE"),
            crc_err_count: 0,
            alignment_pct: 0.0,
            index_timestamps: Vec::new(),
            instant_rpms: Vec::new(),
            rev_time_ms: 200.0,
            rpm_instant: None,
            jitter_pct: None,
            gap0_us: None,
            pll_quality_pct: None,
            interleave: None,
            read_status: DriveReadStatus::NoDiskOrNoIndex,
        };

        process_track_diagnostic(&mut status, &diag_no_disk, &tx_sound);

        assert!(!status.has_disk);
        assert!(!status.sectors_known);
        assert_eq!(status.alignment_pct, 0.0);
        assert_eq!(status.on_track_count, 0);
        assert_eq!(status.read_status, DriveReadStatus::NoDiskOrNoIndex);
    }

    #[test]
    fn test_nominal_capture_timing_parameters() {
        assert_eq!(DEFAULT_SERIAL_TIMEOUT_MS, 1000);
        assert_eq!(SPIN_UP_DELAY_MS, 350);
        assert_eq!(STEPPER_WAKEUP_DELAY_MS, 15);
    }

    #[test]
    fn test_read_flux_cmd_packet_structure() {
        let revs: u16 = 3;
        let b_revs = revs.to_le_bytes();
        let read_cmd = [0x07, 0x08, 0x00, 0x00, 0x00, 0x00, b_revs[0], b_revs[1]];
        assert_eq!(read_cmd, [0x07, 0x08, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00]);

        let revs2: u16 = 2;
        let b_revs2 = revs2.to_le_bytes();
        let read_cmd2 = [0x07, 0x08, 0x00, 0x00, 0x00, 0x00, b_revs2[0], b_revs2[1]];
        assert_eq!(read_cmd2, [0x07, 0x08, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00]);
    }

    #[test]
    fn test_track_conformity_single_head_misalignment() {
        let (tx_sound, _rx_sound) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 40,
            target_track: 40,
            head: 0,
            sector_count: 18,
            ..Default::default()
        };

        // All 18 decoded sectors have cyl: 41 (physical head shifted by +1 track)
        let sectors_track41: Vec<DecodedSector> = (1..=18)
            .map(|id| DecodedSector::new(41, 0, id, 2, true))
            .collect();

        let diag = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 18,
            sectors_known: true,
            sectors: sectors_track41,
            on_track_count: 0,
            off_track_count: 18,
            off_track_details: String::from("MISALIGNED T:41"),
            crc_err_count: 0,
            alignment_pct: 0.0,
            index_timestamps: vec![1000, 14401000],
            instant_rpms: vec![300],
            rev_time_ms: 200.0,
            ..Default::default()
        };

        process_track_diagnostic(&mut status, &diag, &tx_sound);

        assert_eq!(status.on_track_count, 0);
        assert_eq!(status.off_track_count, 18);
        assert_eq!(status.off_track_details, "MISALIGNED T:41");
        assert_eq!(status.alignment_pct, 0.0);
        assert_eq!(
            status.sector_log_standard[0],
            "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 MISALIGNED T:41)"
        );
        assert!(status.sector_log_verbose[0].contains("(18/18 MISALIGNED T:41)"));
    }

    #[test]
    fn test_both_mode_dual_head_track_shift_alignment_50_pct() {
        let (tx_sound, _rx_sound) = crossbeam_channel::unbounded();
        let mut status = DriveStatus {
            track: 40,
            target_track: 40,
            head_select: HeadSelection::Both,
            head: 0,
            sector_count: 18,
            ..Default::default()
        };

        // Head 0: 18/18 OK on track 40
        let sectors_h0: Vec<DecodedSector> = (1..=18)
            .map(|id| DecodedSector::new(40, 0, id, 2, true))
            .collect();
        let diag_h0 = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 18,
            sectors_known: true,
            sectors: sectors_h0,
            on_track_count: 18,
            off_track_count: 0,
            off_track_details: String::from("NONE (Perfect)"),
            crc_err_count: 0,
            alignment_pct: 100.0,
            ..Default::default()
        };
        process_track_diagnostic(&mut status, &diag_h0, &tx_sound);

        // Head 1: 18/18 on track 41 (shifted to next track)
        status.head = 1;
        let sectors_h1: Vec<DecodedSector> = (1..=18)
            .map(|id| DecodedSector::new(41, 1, id, 2, true))
            .collect();
        let diag_h1 = TrackAnalysisResult {
            has_disk: true,
            bitrate: 500,
            sector_count: 18,
            sectors_known: true,
            sectors: sectors_h1,
            on_track_count: 0,
            off_track_count: 18,
            off_track_details: String::from("MISALIGNED T:41"),
            crc_err_count: 0,
            alignment_pct: 0.0,
            ..Default::default()
        };
        process_track_diagnostic(&mut status, &diag_h1, &tx_sound);

        // Alignment must be 50.0% and off-track counter must indicate MISMATCH on Head 1
        assert_eq!(status.alignment_pct, 50.0);
        assert_eq!(status.on_track_count, 18);
        assert_eq!(status.off_track_count, 18);
        assert_eq!(status.off_track_details, "MISMATCH: Track 41 on Head 1");
    }
}

