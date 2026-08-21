use crate::hw::{BusType, DiskFormat, DisplayMode, DriveStatus, HwActivity};

/// Three-state head selection mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeadSelection {
    #[default]
    Head0,
    Head1,
    Both,
}

impl HeadSelection {
    pub fn toggle_next(&self) -> Self {
        match self {
            HeadSelection::Head0 => HeadSelection::Head1,
            HeadSelection::Head1 => HeadSelection::Both,
            HeadSelection::Both => HeadSelection::Head0,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            HeadSelection::Head0 => "0",
            HeadSelection::Head1 => "1",
            HeadSelection::Both => "BOTH (0+1)",
        }
    }
}

/// Application level view mode
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Standard / Normal clean continuous scroll with segmented bar
    #[default]
    Normal,
    /// Detailed verbose history mode with timing, PLL, RPM, and ribbon
    Verbose,
}

/// Represents the result and formatted metrics of a completed diagnostic pass on a physical head
#[derive(Clone, Debug, PartialEq)]
pub struct DiagnosticPass {
    pub track: u8,
    pub track_id: u8,
    pub head: u8,
    pub bitrate: u16,
    pub line_standard: String,
    pub line_verbose: String,
    pub ok_count: u8,
    pub valid_sectors: u8,
    pub expected_count: u8,
    pub crc_errors: u8,
    pub quality_pct: u8,
    pub is_ok: bool,
}

impl DiagnosticPass {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        track: u8,
        head: u8,
        bitrate: u16,
        line_standard: String,
        line_verbose: String,
        ok_count: u8,
        expected_count: u8,
        is_ok: bool,
    ) -> Self {
        let crc_errors = if is_ok {
            0
        } else {
            expected_count.saturating_sub(ok_count)
        };
        let quality_pct = if is_ok {
            99
        } else if expected_count > 0 {
            ((ok_count as f32 / expected_count as f32) * 100.0)
                .round()
                .clamp(0.0, 100.0) as u8
        } else {
            50
        };
        Self {
            track,
            track_id: track,
            head,
            bitrate,
            line_standard,
            line_verbose,
            ok_count,
            valid_sectors: ok_count,
            expected_count,
            crc_errors,
            quality_pct,
            is_ok,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_details(
        track: u8,
        head: u8,
        bitrate: u16,
        line_standard: String,
        line_verbose: String,
        ok_count: u8,
        expected_count: u8,
        crc_errors: u8,
        quality_pct: u8,
        is_ok: bool,
    ) -> Self {
        Self {
            track,
            track_id: track,
            head,
            bitrate,
            line_standard,
            line_verbose,
            ok_count,
            valid_sectors: ok_count,
            expected_count,
            crc_errors,
            quality_pct,
            is_ok,
        }
    }
}

/// Consolidated metrics calculated from dual heads in Both mode
#[derive(Clone, Debug, PartialEq)]
pub struct BothModeMetrics {
    pub alignment_pct: f32,
    pub total_expected: u32,
    pub total_ok: u32,
    pub total_off_track: u32,
    pub off_track_details: String,
    pub total_crc_err: u32,
    pub crc_integrity_pct: f32,
    pub is_degraded: bool,
}

/// User action executable by the application
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ToggleDriveUnit,
    Analyze,
    StartAnalysis,
    Stop,
    ToggleMotor,
    SetMotor(bool),
    PanicReset,
    CycleDiskFormat,
    SetDiskFormat(DiskFormat),
    ToggleBusType,
    SetBusType(BusType),
}

/// Application state wrapper
pub struct App {
    pub status: DriveStatus,
    pub motor_on: bool,
    pub view_mode: ViewMode,
    pub head_selection: HeadSelection,
    pub drive_unit: u8,
    pub last_pass_h0: Option<DiagnosticPass>,
    pub last_pass_h1: Option<DiagnosticPass>,
    pub last_capture_instant: std::time::Instant,
    pub stream_spinner_idx: usize,
    pub show_help: bool,
    pub disk_format: DiskFormat,
    pub bus_type: BusType,
}

impl App {
    pub fn new() -> Self {
        Self::with_config(0, BusType::IbmPc)
    }

    pub fn with_drive_unit(drive_unit: u8) -> Self {
        Self::with_config(drive_unit, BusType::IbmPc)
    }

    pub fn with_config(drive_unit: u8, bus_type: BusType) -> Self {
        let max_unit = match bus_type {
            BusType::IbmPc => 1,
            BusType::Shugart => 3,
        };
        let unit = drive_unit.min(max_unit);
        let status = DriveStatus {
            drive_unit: unit,
            unit_id: unit,
            bus_type,
            ..Default::default()
        };
        Self {
            status,
            motor_on: false,
            view_mode: ViewMode::Normal,
            head_selection: HeadSelection::default(),
            drive_unit: unit,
            last_pass_h0: None,
            last_pass_h1: None,
            last_capture_instant: std::time::Instant::now(),
            stream_spinner_idx: 0,
            show_help: false,
            disk_format: DiskFormat::AutoDetect,
            bus_type,
        }
    }

    pub fn drive_status(&self) -> &DriveStatus {
        &self.status
    }

    pub fn is_motor_on(&self) -> bool {
        self.motor_on
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn on_sector_packet(&mut self) {
        self.last_capture_instant = std::time::Instant::now();
        self.stream_spinner_idx = (self.stream_spinner_idx + 1) % 4;
    }

    pub fn handle_hw_message(&mut self, status: DriveStatus) {
        let is_sector_packet = status.activity == HwActivity::ReadingAnalyzing
            || status.mode == DisplayMode::Analyze
            || status.mode == DisplayMode::ReadData
            || status.analyzing
            || status.in_progress_pass
            || status.sector_log != self.status.sector_log
            || status.io_cycle != self.status.io_cycle
            || status.last_pass_h0 != self.status.last_pass_h0
            || status.last_pass_h1 != self.status.last_pass_h1;

        if is_sector_packet
            && (status.activity == HwActivity::ReadingAnalyzing
                || status.mode == DisplayMode::Analyze
                || status.mode == DisplayMode::ReadData
                || status.analyzing
                || !status.sector_log.is_empty())
        {
            self.last_capture_instant = std::time::Instant::now();
            self.stream_spinner_idx = (self.stream_spinner_idx + 1) % 4;
        }

        self.motor_on = status.motor_on;
        self.disk_format = status.disk_format;
        self.bus_type = status.bus_type;
        self.status = status;
        self.drive_unit = self.status.drive_unit;
        self.head_selection = self.status.head_select;
        self.view_mode = if self.status.verbose_mode {
            ViewMode::Verbose
        } else {
            ViewMode::Normal
        };
    }

    pub fn toggle_verbose(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Normal => ViewMode::Verbose,
            ViewMode::Verbose => ViewMode::Normal,
        };
        self.status.verbose_mode = self.view_mode == ViewMode::Verbose;
    }

    pub fn is_verbose(&self) -> bool {
        self.view_mode == ViewMode::Verbose
    }

    pub fn toggle_head(&mut self) {
        self.head_selection = self.head_selection.toggle_next();
        self.status.head_select = self.head_selection;
    }

    pub fn toggle_drive_unit(&mut self) {
        self.drive_unit = match self.bus_type {
            BusType::IbmPc => (self.drive_unit + 1) % 2,
            BusType::Shugart => (self.drive_unit + 1) % 4,
        };
        self.status.drive_unit = self.drive_unit;
        self.status.unit_id = self.drive_unit;
    }

    pub fn set_drive_unit(&mut self, unit: u8) {
        let max_unit = match self.bus_type {
            BusType::IbmPc => 1,
            BusType::Shugart => 3,
        };
        self.drive_unit = unit.min(max_unit);
        self.status.drive_unit = self.drive_unit;
        self.status.unit_id = self.drive_unit;
    }

    pub fn format_drive_unit_label(&self) -> String {
        format_drive_unit_label(self.bus_type, self.drive_unit)
    }

    pub fn format_drive_unit_short(&self) -> &'static str {
        format_drive_unit_short(self.bus_type, self.drive_unit)
    }

    pub fn cycle_disk_format(&mut self) {
        self.disk_format = self.disk_format.cycle_next();
        self.status.disk_format = self.disk_format;
    }

    pub fn set_disk_format(&mut self, format: DiskFormat) {
        self.disk_format = format;
        self.status.disk_format = format;
    }

    pub fn disk_format(&self) -> DiskFormat {
        self.disk_format
    }

    pub fn toggle_bus_type(&mut self) {
        self.bus_type = self.bus_type.toggle();
        if self.bus_type == BusType::IbmPc && self.drive_unit > 1 {
            self.drive_unit = 0;
        }
        self.status.bus_type = self.bus_type;
        self.status.drive_unit = self.drive_unit;
        self.status.unit_id = self.drive_unit;
    }

    pub fn set_bus_type(&mut self, bus: BusType) {
        self.bus_type = bus;
        if self.bus_type == BusType::IbmPc && self.drive_unit > 1 {
            self.drive_unit = 0;
        }
        self.status.bus_type = self.bus_type;
        self.status.drive_unit = self.drive_unit;
        self.status.unit_id = self.drive_unit;
    }

    pub fn bus_type(&self) -> BusType {
        self.bus_type
    }

    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::ToggleDriveUnit => self.toggle_drive_unit(),
            Action::CycleDiskFormat => self.cycle_disk_format(),
            Action::SetDiskFormat(fmt) => self.set_disk_format(fmt),
            Action::ToggleBusType => self.toggle_bus_type(),
            Action::SetBusType(bus) => self.set_bus_type(bus),
            Action::Analyze | Action::StartAnalysis => {
                self.motor_on = true;
                self.status.analyzing = true;
                self.status.mode = DisplayMode::Analyze;
                self.status.motor_on = true;
                self.status.activity = HwActivity::ReadingAnalyzing;
                self.clear_passes();
            }
            Action::Stop => {
                self.motor_on = false;
                self.status.analyzing = false;
                self.status.motor_on = false;
                self.status.mode = DisplayMode::None;
                self.status.activity = HwActivity::Stopped;
                self.status.index = false;
                self.status.log_msg = String::from("Stop / Motor OFF (Safe to change disk)");
            }
            Action::ToggleMotor => {
                self.motor_on = !self.motor_on;
                self.status.motor_on = self.motor_on;
                if !self.motor_on {
                    self.status.index = false;
                    self.status.analyzing = false;
                    if self.status.mode != DisplayMode::None {
                        if self.status.mode == DisplayMode::RpmMeasure
                            && self.status.rpm_measure.sample_count > 0
                        {
                            self.status.rpm_display =
                                format!("{:.1} RPM", self.status.rpm_measure.avg_rpm);
                        }
                        self.status.mode = DisplayMode::None;
                    }
                    self.status.activity = HwActivity::Stopped;
                    self.status.log_msg = String::from("Motor OFF (M key)");
                } else {
                    self.status.drive_select = true;
                    self.status.has_disk = true;
                    self.status.activity = HwActivity::Idle;
                    self.status.log_msg = String::from("Motor ON (M key)");
                }
            }
            Action::SetMotor(on) => {
                self.motor_on = on;
                self.status.motor_on = on;
                if !on {
                    self.status.index = false;
                    self.status.analyzing = false;
                    if self.status.mode != DisplayMode::None {
                        if self.status.mode == DisplayMode::RpmMeasure
                            && self.status.rpm_measure.sample_count > 0
                        {
                            self.status.rpm_display =
                                format!("{:.1} RPM", self.status.rpm_measure.avg_rpm);
                        }
                        self.status.mode = DisplayMode::None;
                    }
                    self.status.activity = HwActivity::Stopped;
                    self.status.log_msg = String::from("Motor OFF (M key)");
                } else {
                    self.status.drive_select = true;
                    self.status.has_disk = true;
                    self.status.activity = HwActivity::Idle;
                    self.status.log_msg = String::from("Motor ON (M key)");
                }
            }
            Action::PanicReset => {
                self.motor_on = false;
                self.clear_passes();
                self.status.motor_on = false;
                self.status.analyzing = false;
                self.status.mode = DisplayMode::None;
                self.status.activity = HwActivity::Idle;
                self.status.rpm = 0;
                self.status.rpm_display = String::from("--- RPM");
                self.status.rpm_measure.clear();
                self.status.index = false;
                self.status.sectors.clear();
                self.status.sectors_known = false;
                self.status.on_track_count = 0;
                self.status.off_track_count = 0;
                self.status.off_track_details = String::from("NONE");
                self.status.crc_err_count = 0;
                self.status.alignment_pct = 0.0;
                self.status.head_select = HeadSelection::Head0;
                self.status.head = 0;
                self.status.last_pass_h0 = None;
                self.status.last_pass_h1 = None;
                self.status.in_progress_pass = false;
                self.status.read_status = crate::hw::DriveReadStatus::Ok;
                self.status.log_msg = String::from("PANIC RESET: Hardware reset & serial buffers purged successfully");
            }
        }
    }

    pub fn record_pass(&mut self, pass: DiagnosticPass) {
        self.last_capture_instant = std::time::Instant::now();
        self.stream_spinner_idx = (self.stream_spinner_idx + 1) % 4;
        if pass.head == 0 {
            self.last_pass_h0 = Some(pass);
        } else {
            self.last_pass_h1 = Some(pass);
        }
    }

    pub fn clear_passes(&mut self) {
        self.last_pass_h0 = None;
        self.last_pass_h1 = None;
    }

    pub fn compute_both_mode_metrics(&self) -> BothModeMetrics {
        Self::compute_both_metrics_from_passes(
            self.last_pass_h0.as_ref().or(self.status.last_pass_h0.as_ref()),
            self.last_pass_h1.as_ref().or(self.status.last_pass_h1.as_ref()),
            self.status.sector_count,
        )
    }

    pub fn compute_both_metrics_from_passes(
        pass_h0: Option<&DiagnosticPass>,
        pass_h1: Option<&DiagnosticPass>,
        fallback_sector_count: u8,
    ) -> BothModeMetrics {
        let expected_per_head = if let Some(p) = pass_h0.filter(|p| p.expected_count > 0) {
            p.expected_count as u32
        } else if let Some(p) = pass_h1.filter(|p| p.expected_count > 0) {
            p.expected_count as u32
        } else if fallback_sector_count > 0 {
            fallback_sector_count as u32
        } else {
            18
        };

        let (h0_ok, h0_exp, h0_off) = pass_h0
            .map(|p| {
                let exp = if p.expected_count > 0 {
                    p.expected_count as u32
                } else {
                    expected_per_head
                };
                let ok = p.ok_count as u32;
                let off = if p.track_id != p.track {
                    exp
                } else {
                    exp.saturating_sub(ok)
                };
                (ok, exp, off)
            })
            .unwrap_or((0, expected_per_head, 0));

        let (h1_ok, h1_exp, h1_off) = pass_h1
            .map(|p| {
                let exp = if p.expected_count > 0 {
                    p.expected_count as u32
                } else {
                    expected_per_head
                };
                let ok = p.ok_count as u32;
                let off = if p.track_id != p.track {
                    exp
                } else {
                    exp.saturating_sub(ok)
                };
                (ok, exp, off)
            })
            .unwrap_or((0, expected_per_head, 0));

        let total_ok = h0_ok + h1_ok;
        let total_expected = (h0_exp + h1_exp).max(1);

        let alignment_pct = if total_expected > 0 {
            (total_ok as f32 / total_expected as f32) * 100.0
        } else {
            0.0
        };

        let total_off_track = match (pass_h0, pass_h1) {
            (Some(p0), Some(p1)) => {
                let mut off = 0;
                if p0.track_id != p0.track {
                    off += p0.expected_count as u32;
                }
                if p1.track_id != p1.track {
                    off += p1.expected_count as u32;
                }
                if off == 0 {
                    h0_off + h1_off
                } else {
                    off
                }
            }
            (Some(p), None) => {
                if p.track_id != p.track {
                    p.expected_count as u32
                } else {
                    h0_off
                }
            }
            (None, Some(p)) => {
                if p.track_id != p.track {
                    p.expected_count as u32
                } else {
                    h1_off
                }
            }
            (None, None) => 0,
        };

        let off_track_details = match (pass_h0, pass_h1) {
            (Some(p0), Some(p1)) if p0.track_id != p0.track && p1.track_id != p1.track => {
                format!("MISMATCH: Track {} on Head 0, Track {} on Head 1", p0.track_id, p1.track_id)
            }
            (Some(_), Some(p1)) if p1.track_id != p1.track => {
                format!("MISMATCH: Track {} on Head 1", p1.track_id)
            }
            (Some(p0), Some(_)) if p0.track_id != p0.track => {
                format!("MISMATCH: Track {} on Head 0", p0.track_id)
            }
            (Some(p0), None) if p0.track_id != p0.track => {
                format!("MISMATCH: Track {} on Head 0", p0.track_id)
            }
            (None, Some(p1)) if p1.track_id != p1.track => {
                format!("MISMATCH: Track {} on Head 1", p1.track_id)
            }
            _ => {
                if total_off_track == 0 {
                    String::from("NONE (Perfect)")
                } else {
                    String::from("OFF-TRK")
                }
            }
        };

        let h0_is_bad = pass_h0.map(|p| !p.is_ok).unwrap_or(false);
        let h1_is_bad = pass_h1.map(|p| !p.is_ok).unwrap_or(false);
        let is_degraded = h0_is_bad || h1_is_bad || alignment_pct < 95.0;

        let total_crc_err = total_expected.saturating_sub(total_ok);
        let crc_integrity_pct = alignment_pct;

        BothModeMetrics {
            alignment_pct,
            total_expected,
            total_ok,
            total_off_track,
            off_track_details,
            total_crc_err,
            crc_integrity_pct,
            is_degraded,
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Formats the drive unit label based on bus type:
/// - IBM PC: Unit 0 -> `Drive 0 (A:)`, Unit 1 -> `Drive 1 (B:)`
/// - Shugart: Unit 0 -> `Unit 0 (DS0)`, Unit 1 -> `Unit 1 (DS1)`, Unit 2 -> `Unit 2 (DS2)`, Unit 3 -> `Unit 3 (DS3)`
pub fn format_drive_unit_label(bus_type: BusType, drive_unit: u8) -> String {
    match bus_type {
        BusType::IbmPc => match drive_unit {
            0 => "Drive 0 (A:)".to_string(),
            _ => "Drive 1 (B:)".to_string(),
        },
        BusType::Shugart => match drive_unit {
            0 => "Unit 0 (DS0)".to_string(),
            1 => "Unit 1 (DS1)".to_string(),
            2 => "Unit 2 (DS2)".to_string(),
            _ => "Unit 3 (DS3)".to_string(),
        },
    }
}

/// Formats the short drive code for top header banner:
/// - IBM PC: `A:` / `B:`
/// - Shugart: `DS0` / `DS1` / `DS2` / `DS3`
pub fn format_drive_unit_short(bus_type: BusType, drive_unit: u8) -> &'static str {
    match bus_type {
        BusType::IbmPc => match drive_unit {
            0 => "A:",
            _ => "B:",
        },
        BusType::Shugart => match drive_unit {
            0 => "DS0",
            1 => "DS1",
            2 => "DS2",
            _ => "DS3",
        },
    }
}

/// Formats the menu shortcut description for drive unit:
/// - IBM PC: ` U = Unit (A: / B:)`
/// - Shugart: ` U = Unit (DS0..DS3)`
pub fn format_unit_menu_shortcut(bus_type: BusType) -> &'static str {
    match bus_type {
        BusType::IbmPc => " U = Unit (A: / B:)",
        BusType::Shugart => " U = Unit (DS0..DS3)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_handle_hw_message_sync() {
        let mut app = App::new();
        assert_eq!(app.drive_unit, 0);
        assert_eq!(app.head_selection, HeadSelection::Head0);
        assert_eq!(app.view_mode, ViewMode::Normal);

        let status = DriveStatus {
            drive_unit: 1,
            head_select: HeadSelection::Both,
            verbose_mode: true,
            ..Default::default()
        };

        app.handle_hw_message(status);

        assert_eq!(app.drive_unit, 1);
        assert_eq!(app.head_selection, HeadSelection::Both);
        assert_eq!(app.view_mode, ViewMode::Verbose);
    }

    #[test]
    fn test_app_handle_action_analyze() {
        let mut app = App::new();
        assert!(!app.status.analyzing);
        assert_eq!(app.status.mode, DisplayMode::None);
        assert!(!app.status.motor_on);

        app.handle_action(Action::Analyze);
        assert!(app.status.analyzing);
        assert_eq!(app.status.mode, DisplayMode::Analyze);
        assert!(app.status.motor_on);
        assert_eq!(app.status.activity, HwActivity::ReadingAnalyzing);

        let mut app2 = App::new();
        app2.handle_action(Action::StartAnalysis);
        assert!(app2.status.analyzing);
        assert_eq!(app2.status.mode, DisplayMode::Analyze);
        assert!(app2.status.motor_on);
        assert_eq!(app2.status.activity, HwActivity::ReadingAnalyzing);
    }

    #[test]
    fn test_app_handle_action_stop() {
        let mut app = App::new();
        app.handle_action(Action::Analyze);
        assert!(app.status.analyzing);
        assert!(app.status.motor_on);
        assert_eq!(app.status.activity, HwActivity::ReadingAnalyzing);

        app.handle_action(Action::Stop);
        assert!(!app.status.analyzing);
        assert!(!app.status.motor_on);
        assert_eq!(app.status.mode, DisplayMode::None);
        assert_eq!(app.status.activity, HwActivity::Stopped);
        assert!(!app.status.index);
    }

    #[test]
    fn test_track_mismatch_triggers_warning_audio_event() {
        use crate::audio::{evaluate_alignment_audio_event, AudioEvent};

        let mut status = DriveStatus {
            track: 75,
            target_track: 75,
            head: 1,
            head_select: HeadSelection::Both,
            ..Default::default()
        };

        // Head 0 read track 75, Head 1 read track 76 -> Track divergence
        let pass_h0 = DiagnosticPass::with_details(
            75, 0, 500,
            "T:75 H:0".into(), "T:75 H:0".into(),
            18, 18, 0, 95, true,
        );
        let pass_h1 = DiagnosticPass::with_details(
            76, 1, 500,
            "T:76 H:1".into(), "T:76 H:1".into(),
            18, 18, 0, 95, true,
        );

        status.last_pass_h0 = Some(pass_h0);
        status.last_pass_h1 = Some(pass_h1);

        let event = evaluate_alignment_audio_event(
            status.head_select,
            status.target_track,
            status.head,
            status.last_pass_h0.as_ref(),
            status.last_pass_h1.as_ref(),
            status.sector_count,
        );

        assert_eq!(event, Some(AudioEvent::TrackMismatch));
    }

    #[test]
    fn test_crc_error_emits_marginal_tracking_tone() {
        use crate::audio::{calculate_radar_pitch, evaluate_alignment_audio_event, AudioEvent};

        let mut status = DriveStatus {
            track: 40,
            target_track: 40,
            head: 0,
            head_select: HeadSelection::Both,
            ..Default::default()
        };

        // Head 0 has 1 CRC error, quality 90%
        let pass_h0 = DiagnosticPass::with_details(
            40, 0, 500,
            "T:40 H:0".into(), "T:40 H:0".into(),
            17, 18, 1, 90, false,
        );
        let pass_h1 = DiagnosticPass::with_details(
            40, 1, 500,
            "T:40 H:1".into(), "T:40 H:1".into(),
            18, 18, 0, 95, true,
        );

        status.last_pass_h0 = Some(pass_h0);
        status.last_pass_h1 = Some(pass_h1);

        let event = evaluate_alignment_audio_event(
            status.head_select,
            status.target_track,
            status.head,
            status.last_pass_h0.as_ref(),
            status.last_pass_h1.as_ref(),
            status.sector_count,
        );

        // Quality 90% is Marginal Tracking -> Medium tone (600 - 1400 Hz)
        let expected_pitch = calculate_radar_pitch(90);
        assert_eq!(event, Some(AudioEvent::AlignmentTone { pitch_hz: expected_pitch }));
        assert_eq!(event, Some(AudioEvent::AlignmentTone { pitch_hz: 1267 }));
    }

    #[test]
    fn test_app_spinner_and_capture_instant_lifecycle() {
        let mut app = App::new();
        assert_eq!(app.stream_spinner_idx, 0);

        let initial_instant = app.last_capture_instant;

        // Simulate sector packet arrival via on_sector_packet
        app.on_sector_packet();
        assert_eq!(app.stream_spinner_idx, 1);
        assert!(app.last_capture_instant >= initial_instant);

        app.on_sector_packet();
        assert_eq!(app.stream_spinner_idx, 2);

        app.on_sector_packet();
        assert_eq!(app.stream_spinner_idx, 3);

        app.on_sector_packet();
        assert_eq!(app.stream_spinner_idx, 0); // Rotates back to 0

        // Simulate sector packet arrival via handle_hw_message
        let mut status = DriveStatus::default();
        status.activity = HwActivity::ReadingAnalyzing;
        status.mode = DisplayMode::Analyze;
        status.sector_log.push("T:00 H:0 Rate:500k MFM [ ■ ■ ■ ] (3/3 OK)".into());

        app.handle_hw_message(status);
        assert_eq!(app.stream_spinner_idx, 1);

        // Simulate record_pass
        let pass = DiagnosticPass::new(
            0, 0, 500,
            "T:00 H:0 [ ■ ] (1/1 OK)".into(),
            "T:00 H:0 [ ■ ] (1/1 OK)".into(),
            1, 1, true,
        );
        app.record_pass(pass);
        assert_eq!(app.stream_spinner_idx, 2);
    }

    #[test]
    fn test_app_help_toggle() {
        let mut app = App::new();
        assert!(!app.show_help);
        app.toggle_help();
        assert!(app.show_help);
        app.toggle_help();
        assert!(!app.show_help);
    }

    #[test]
    fn test_app_toggle_motor_action() {
        let mut app = App::new();
        assert!(!app.motor_on);
        assert!(!app.status.motor_on);
        assert!(!app.is_motor_on());

        // Toggle Motor ON
        app.handle_action(Action::ToggleMotor);
        assert!(app.motor_on);
        assert!(app.status.motor_on);
        assert!(app.is_motor_on());
        assert_eq!(app.status.activity, HwActivity::Idle);
        assert!(app.status.drive_select);
        assert_eq!(app.status.log_msg, "Motor ON (M key)");

        // Toggle Motor OFF
        app.handle_action(Action::ToggleMotor);
        assert!(!app.motor_on);
        assert!(!app.status.motor_on);
        assert!(!app.is_motor_on());
        assert_eq!(app.status.activity, HwActivity::Stopped);
        assert_eq!(app.status.log_msg, "Motor OFF (M key)");

        // Set Motor explicitly
        app.handle_action(Action::SetMotor(true));
        assert!(app.motor_on);
        assert!(app.status.motor_on);
        app.handle_action(Action::SetMotor(false));
        assert!(!app.motor_on);
        assert!(!app.status.motor_on);
    }

    #[test]
    fn test_app_panic_reset_action() {
        let mut app = App::new();
        app.handle_action(Action::Analyze);
        assert!(app.motor_on);
        assert!(app.status.analyzing);
        assert_eq!(app.status.mode, DisplayMode::Analyze);

        let pass = DiagnosticPass::new(
            40, 0, 500,
            "T:40 H:0".into(), "T:40 H:0".into(),
            18, 18, true,
        );
        app.record_pass(pass);
        assert!(app.last_pass_h0.is_some());

        // Trigger PanicReset
        app.handle_action(Action::PanicReset);
        assert!(!app.motor_on);
        assert!(!app.status.motor_on);
        assert!(!app.status.analyzing);
        assert_eq!(app.status.mode, DisplayMode::None);
        assert_eq!(app.status.activity, HwActivity::Idle);
        assert_eq!(app.status.rpm, 0);
        assert_eq!(app.status.rpm_display, "--- RPM");
        assert!(!app.status.index);
        assert!(app.status.sectors.is_empty());
        assert!(!app.status.sectors_known);
        assert_eq!(app.status.head, 0);
        assert_eq!(app.status.head_select, HeadSelection::Head0);
        assert_eq!(app.head_selection, HeadSelection::Head0);
        assert!(app.last_pass_h0.is_none());
        assert!(app.last_pass_h1.is_none());
        assert_eq!(app.status.log_msg, "PANIC RESET: Hardware reset & serial buffers purged successfully");
        assert_eq!(app.drive_status().activity, HwActivity::Idle);
    }

    #[test]
    fn test_app_bus_type_actions_and_config() {
        let mut app = App::with_config(1, BusType::Shugart);
        assert_eq!(app.drive_unit, 1);
        assert_eq!(app.bus_type(), BusType::Shugart);
        assert_eq!(app.status.bus_type, BusType::Shugart);

        app.handle_action(Action::ToggleBusType);
        assert_eq!(app.bus_type(), BusType::IbmPc);
        assert_eq!(app.status.bus_type, BusType::IbmPc);

        app.handle_action(Action::SetBusType(BusType::Shugart));
        assert_eq!(app.bus_type(), BusType::Shugart);
        assert_eq!(app.status.bus_type, BusType::Shugart);
    }

    #[test]
    fn test_app_drive_unit_cycling_pc_mode() {
        let mut app = App::with_config(0, BusType::IbmPc);
        assert_eq!(app.drive_unit, 0);
        assert_eq!(app.format_drive_unit_label(), "Drive 0 (A:)");
        assert_eq!(app.format_drive_unit_short(), "A:");

        app.toggle_drive_unit();
        assert_eq!(app.drive_unit, 1);
        assert_eq!(app.format_drive_unit_label(), "Drive 1 (B:)");
        assert_eq!(app.format_drive_unit_short(), "B:");

        app.toggle_drive_unit();
        assert_eq!(app.drive_unit, 0);
        assert_eq!(app.format_drive_unit_label(), "Drive 0 (A:)");
        assert_eq!(app.format_drive_unit_short(), "A:");
    }

    #[test]
    fn test_app_drive_unit_cycling_shugart_mode() {
        let mut app = App::with_config(0, BusType::Shugart);
        assert_eq!(app.drive_unit, 0);
        assert_eq!(app.format_drive_unit_label(), "Unit 0 (DS0)");
        assert_eq!(app.format_drive_unit_short(), "DS0");

        app.toggle_drive_unit();
        assert_eq!(app.drive_unit, 1);
        assert_eq!(app.format_drive_unit_label(), "Unit 1 (DS1)");
        assert_eq!(app.format_drive_unit_short(), "DS1");

        app.toggle_drive_unit();
        assert_eq!(app.drive_unit, 2);
        assert_eq!(app.format_drive_unit_label(), "Unit 2 (DS2)");
        assert_eq!(app.format_drive_unit_short(), "DS2");

        app.toggle_drive_unit();
        assert_eq!(app.drive_unit, 3);
        assert_eq!(app.format_drive_unit_label(), "Unit 3 (DS3)");
        assert_eq!(app.format_drive_unit_short(), "DS3");

        app.toggle_drive_unit();
        assert_eq!(app.drive_unit, 0);
        assert_eq!(app.format_drive_unit_label(), "Unit 0 (DS0)");
        assert_eq!(app.format_drive_unit_short(), "DS0");
    }

    #[test]
    fn test_app_bus_type_fallback_on_switch() {
        // Switching to PC with Unit 3 resets to 0
        let mut app = App::with_config(3, BusType::Shugart);
        assert_eq!(app.drive_unit, 3);
        app.toggle_bus_type();
        assert_eq!(app.bus_type(), BusType::IbmPc);
        assert_eq!(app.drive_unit, 0);
        assert_eq!(app.status.drive_unit, 0);

        // Switching to PC with Unit 2 resets to 0
        let mut app2 = App::with_config(2, BusType::Shugart);
        assert_eq!(app2.drive_unit, 2);
        app2.set_bus_type(BusType::IbmPc);
        assert_eq!(app2.bus_type(), BusType::IbmPc);
        assert_eq!(app2.drive_unit, 0);
        assert_eq!(app2.status.drive_unit, 0);

        // Switching to PC with Unit 1 stays 1
        let mut app3 = App::with_config(1, BusType::Shugart);
        assert_eq!(app3.drive_unit, 1);
        app3.toggle_bus_type();
        assert_eq!(app3.bus_type(), BusType::IbmPc);
        assert_eq!(app3.drive_unit, 1);
        assert_eq!(app3.status.drive_unit, 1);
    }

    #[test]
    fn test_formatting_drive_labels_and_shortcuts() {
        assert_eq!(format_drive_unit_label(BusType::IbmPc, 0), "Drive 0 (A:)");
        assert_eq!(format_drive_unit_label(BusType::IbmPc, 1), "Drive 1 (B:)");
        assert_eq!(format_drive_unit_label(BusType::Shugart, 0), "Unit 0 (DS0)");
        assert_eq!(format_drive_unit_label(BusType::Shugart, 1), "Unit 1 (DS1)");
        assert_eq!(format_drive_unit_label(BusType::Shugart, 2), "Unit 2 (DS2)");
        assert_eq!(format_drive_unit_label(BusType::Shugart, 3), "Unit 3 (DS3)");

        assert_eq!(format_drive_unit_short(BusType::IbmPc, 0), "A:");
        assert_eq!(format_drive_unit_short(BusType::IbmPc, 1), "B:");
        assert_eq!(format_drive_unit_short(BusType::Shugart, 0), "DS0");
        assert_eq!(format_drive_unit_short(BusType::Shugart, 1), "DS1");
        assert_eq!(format_drive_unit_short(BusType::Shugart, 2), "DS2");
        assert_eq!(format_drive_unit_short(BusType::Shugart, 3), "DS3");

        assert_eq!(format_unit_menu_shortcut(BusType::IbmPc), " U = Unit (A: / B:)");
        assert_eq!(format_unit_menu_shortcut(BusType::Shugart), " U = Unit (DS0..DS3)");
    }
}



