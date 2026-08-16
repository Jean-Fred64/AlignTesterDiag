use crate::hw::DriveStatus;

/// Application level view mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// Standard / Normal clean continuous scroll with segmented bar
    Normal,
    /// Detailed verbose history mode with timing, PLL, RPM, and ribbon
    Verbose,
}

impl Default for ViewMode {
    fn default() -> Self {
        Self::Normal
    }
}

/// Application state wrapper
pub struct App {
    pub status: DriveStatus,
    pub view_mode: ViewMode,
}

impl App {
    pub fn new() -> Self {
        Self {
            status: DriveStatus::default(),
            view_mode: ViewMode::Normal,
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
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
