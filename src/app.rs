use crate::hw::{DisplayMode, DriveStatus, HwActivity};

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
            HeadSelection::Both  => HeadSelection::Head0,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            HeadSelection::Head0 => "0",
            HeadSelection::Head1 => "1",
            HeadSelection::Both  => "BOTH (0+1)",
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
        let crc_errors = if is_ok { 0 } else { expected_count.saturating_sub(ok_count) };
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
            quality_pct: if is_ok { 99 } else { 50 },
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
}

/// Application state wrapper
pub struct App {
    pub status: DriveStatus,
    pub view_mode: ViewMode,
    pub head_selection: HeadSelection,
    pub drive_unit: u8,
    pub last_pass_h0: Option<DiagnosticPass>,
    pub last_pass_h1: Option<DiagnosticPass>,
}

impl App {
    pub fn new() -> Self {
        Self::with_drive_unit(0)
    }

    pub fn with_drive_unit(drive_unit: u8) -> Self {
        let unit = drive_unit.min(1);
        let status = DriveStatus {
            drive_unit: unit,
            unit_id: unit,
            ..Default::default()
        };
        Self {
            status,
            view_mode: ViewMode::Normal,
            head_selection: HeadSelection::default(),
            drive_unit: unit,
            last_pass_h0: None,
            last_pass_h1: None,
        }
    }

    pub fn handle_hw_message(&mut self, status: DriveStatus) {
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
        self.drive_unit = if self.drive_unit == 0 { 1 } else { 0 };
        self.status.drive_unit = self.drive_unit;
        self.status.unit_id = self.drive_unit;
    }

    pub fn set_drive_unit(&mut self, unit: u8) {
        self.drive_unit = unit.min(1);
        self.status.drive_unit = self.drive_unit;
        self.status.unit_id = self.drive_unit;
    }

    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::ToggleDriveUnit => self.toggle_drive_unit(),
            Action::Analyze | Action::StartAnalysis => {
                self.status.analyzing = true;
                self.status.mode = DisplayMode::Analyze;
                self.status.motor_on = true;
                self.status.activity = HwActivity::ReadingAnalyzing;
            }
        }
    }

    pub fn record_pass(&mut self, pass: DiagnosticPass) {
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
    fn test_track_mismatch_triggers_warning_audio_event() {
        use crate::audio::{evaluate_alignment_audio_event, AudioEvent};

        let mut status = DriveStatus {
            track: 75,
            target_track: 75,
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
    fn test_crc_error_prevents_positive_radar_tone() {
        use crate::audio::{evaluate_alignment_audio_event, AudioEvent};

        let mut status = DriveStatus {
            track: 40,
            target_track: 40,
            head_select: HeadSelection::Both,
            ..Default::default()
        };

        // Head 0 has 1 CRC error
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

        // CRC error must NOT trigger PerfectAlignment, but OffTrackOrCrcError
        assert_eq!(event, Some(AudioEvent::OffTrackOrCrcError));
    }
}


