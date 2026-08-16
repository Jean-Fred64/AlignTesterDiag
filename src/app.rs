use crate::hw::DriveStatus;

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
    pub head: u8,
    pub bitrate: u16,
    pub line_standard: String,
    pub line_verbose: String,
    pub ok_count: u8,
    pub expected_count: u8,
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
        Self {
            track,
            head,
            bitrate,
            line_standard,
            line_verbose,
            ok_count,
            expected_count,
            is_ok,
        }
    }
}

/// Application state wrapper
pub struct App {
    pub status: DriveStatus,
    pub view_mode: ViewMode,
    pub head_selection: HeadSelection,
    pub last_pass_h0: Option<DiagnosticPass>,
    pub last_pass_h1: Option<DiagnosticPass>,
}

impl App {
    pub fn new() -> Self {
        Self {
            status: DriveStatus::default(),
            view_mode: ViewMode::Normal,
            head_selection: HeadSelection::default(),
            last_pass_h0: None,
            last_pass_h1: None,
        }
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
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}


