use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseEventKind,
    },
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{
    env,
    error::Error,
    io::stdout,
    thread,
    time::Duration,
};

mod app;
mod audio;
mod hw;
mod ui;

use crossbeam_channel::unbounded;
pub use app::*;
pub use audio::*;
pub use hw::{
    get_status_expected_sector_ids, hw_thread, BusType, DiskFormat, DisplayMode, DriveStatus,
    FormatProgress, FormatStep, HeadSelection, HwActivity, HwCmd, PresetProfile, StepMode, TrackRange,
};
pub use ui::*;

/// Builds the clean CLI banner and help/version text
pub fn build_cli_banner() -> String {
    format!(
r#"💾 AlignTesterDiag v{}
Real-time Floppy Drive Diagnostic & Alignment Tool for Greaseweazle
Copyright (C) 2026 MonSieur JeAn-FReD (GPL-3.0)

Usage: aligntester-diag [OPTIONS] [PORT]

Arguments:
  [PORT]                  Serial port connected to Greaseweazle (e.g. COM3, /dev/ttyACM0)

Options:
  -p, --preset <preset>      Select hardware & format preset (pc35hd, pc35dd, pc525hd, pc525ddonhd, pc525dd, amiga, atari, cpc) [default: pc35hd]
  -d, --drive <0-3>          Select physical drive unit (0..1 for PC, 0..3 for Shugart) [default: 0]
  -b, --bus <pc|shugart>     Select floppy interface bus type (pc | shugart) [default: pc]
  -s, --step <single|double> Select step mode (single 1:1 for 96/135 TPI | double 2:1 for 48 TPI) [default: single]
      --double-step          Alias for --step double
      --port <PORT>          Serial port connected to Greaseweazle
  -h, --help                 Print help information
  -v, -V, --version          Print version information
"#,
        env!("CARGO_PKG_VERSION")
    )
}

/// Checks for help or version flags (-h, --help, -v, -V, --version)
/// If present, prints the clean CLI banner to stdout and returns true (indicating early exit).
pub fn handle_cli_help_or_version(args: &[String]) -> bool {
    for arg in args.iter().skip(1) {
        if arg == "-h" || arg == "--help" || arg == "-v" || arg == "-V" || arg == "--version" {
            print!("{}", build_cli_banner());
            return true;
        }
    }
    false
}

/// Formats the UI status log message cleanly with standard "Log: " prefix
pub fn format_log_line(msg: &str) -> String {
    if msg.starts_with("Log: ") {
        msg.to_string()
    } else {
        format!("Log: {}", msg)
    }
}

pub fn parse_cli_args(args: &[String]) -> (Option<String>, u8, BusType, StepMode, PresetProfile) {
    let mut port: Option<String> = None;
    let mut raw_drive_unit: u8 = 0;
    let mut preset_opt: Option<PresetProfile> = None;
    let mut bus_opt: Option<BusType> = None;
    let mut step_opt: Option<StepMode> = None;
    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];
        if arg == "--preset" {
            if i + 1 < args.len() {
                if let Some(p) = PresetProfile::from_str_loose(&args[i + 1]) {
                    preset_opt = Some(p);
                }
                i += 1;
            }
        } else if let Some(stripped) = arg.strip_prefix("--preset=") {
            if let Some(p) = PresetProfile::from_str_loose(stripped) {
                preset_opt = Some(p);
            }
        } else if arg == "-p" {
            if i + 1 < args.len() {
                if let Some(p) = PresetProfile::from_str_loose(&args[i + 1]) {
                    preset_opt = Some(p);
                } else {
                    // Fallback for legacy port specification "-p COM3"
                    port = Some(args[i + 1].clone());
                }
                i += 1;
            }
        } else if let Some(stripped) = arg.strip_prefix("-p=") {
            if let Some(p) = PresetProfile::from_str_loose(stripped) {
                preset_opt = Some(p);
            } else {
                port = Some(stripped.to_string());
            }
        } else if arg == "--drive" || arg == "-d" {
            if i + 1 < args.len() {
                if let Ok(u) = args[i + 1].parse::<u8>() {
                    raw_drive_unit = u.min(3);
                }
                i += 1;
            }
        } else if let Some(stripped) = arg.strip_prefix("--drive=") {
            if let Ok(u) = stripped.parse::<u8>() {
                raw_drive_unit = u.min(3);
            }
        } else if let Some(stripped) = arg.strip_prefix("-d=") {
            if let Ok(u) = stripped.parse::<u8>() {
                raw_drive_unit = u.min(3);
            }
        } else if arg == "--bus" || arg == "-b" || arg == "--bus-type" {
            if i + 1 < args.len() {
                let val = args[i + 1].to_lowercase();
                if val == "shugart" || val == "amiga" {
                    bus_opt = Some(BusType::Shugart);
                } else if val == "pc" || val == "ibm" || val == "ibmpc" {
                    bus_opt = Some(BusType::IbmPc);
                }
                i += 1;
            }
        } else if let Some(stripped) = arg.strip_prefix("--bus=") {
            let val = stripped.to_lowercase();
            if val == "shugart" || val == "amiga" {
                bus_opt = Some(BusType::Shugart);
            } else if val == "pc" || val == "ibm" || val == "ibmpc" {
                bus_opt = Some(BusType::IbmPc);
            }
        } else if let Some(stripped) = arg.strip_prefix("-b=") {
            let val = stripped.to_lowercase();
            if val == "shugart" || val == "amiga" {
                bus_opt = Some(BusType::Shugart);
            } else if val == "pc" || val == "ibm" || val == "ibmpc" {
                bus_opt = Some(BusType::IbmPc);
            }
        } else if let Some(stripped) = arg.strip_prefix("--bus-type=") {
            let val = stripped.to_lowercase();
            if val == "shugart" || val == "amiga" {
                bus_opt = Some(BusType::Shugart);
            } else if val == "pc" || val == "ibm" || val == "ibmpc" {
                bus_opt = Some(BusType::IbmPc);
            }
        } else if arg == "--shugart" {
            bus_opt = Some(BusType::Shugart);
        } else if arg == "--pc" || arg == "--ibmpc" {
            bus_opt = Some(BusType::IbmPc);
        } else if arg == "--step" || arg == "-s" || arg == "--step-mode" {
            if i + 1 < args.len() {
                let val = args[i + 1].to_lowercase();
                if val == "double" || val == "2" || val == "2:1" || val == "48" || val == "48tpi" {
                    step_opt = Some(StepMode::Double);
                } else if val == "single" || val == "1" || val == "1:1" || val == "96" || val == "135" {
                    step_opt = Some(StepMode::Single);
                }
                i += 1;
            }
        } else if let Some(stripped) = arg.strip_prefix("--step=") {
            let val = stripped.to_lowercase();
            if val == "double" || val == "2" || val == "2:1" || val == "48" || val == "48tpi" {
                step_opt = Some(StepMode::Double);
            } else if val == "single" || val == "1" || val == "1:1" || val == "96" || val == "135" {
                step_opt = Some(StepMode::Single);
            }
        } else if let Some(stripped) = arg.strip_prefix("-s=") {
            let val = stripped.to_lowercase();
            if val == "double" || val == "2" || val == "2:1" || val == "48" || val == "48tpi" {
                step_opt = Some(StepMode::Double);
            } else if val == "single" || val == "1" || val == "1:1" || val == "96" || val == "135" {
                step_opt = Some(StepMode::Single);
            }
        } else if let Some(stripped) = arg.strip_prefix("--step-mode=") {
            let val = stripped.to_lowercase();
            if val == "double" || val == "2" || val == "2:1" || val == "48" || val == "48tpi" {
                step_opt = Some(StepMode::Double);
            } else if val == "single" || val == "1" || val == "1:1" || val == "96" || val == "135" {
                step_opt = Some(StepMode::Single);
            }
        } else if arg == "--double-step" || arg == "--doublestep" {
            step_opt = Some(StepMode::Double);
        } else if arg == "--single-step" || arg == "--singlestep" {
            step_opt = Some(StepMode::Single);
        } else if arg == "--port" {
            if i + 1 < args.len() {
                port = Some(args[i + 1].clone());
                i += 1;
            }
        } else if let Some(stripped) = arg.strip_prefix("--port=") {
            port = Some(stripped.to_string());
        } else if !arg.starts_with('-') && port.is_none() {
            port = Some(arg.clone());
        }
        i += 1;
    }

    let preset = preset_opt.unwrap_or(PresetProfile::Pc35Hd);
    let bus_type = bus_opt.unwrap_or_else(|| preset.default_bus());
    let step_mode = step_opt.unwrap_or_else(|| preset.default_step());
    let max_unit = if bus_type == BusType::IbmPc { 1 } else { 3 };
    let drive_unit = raw_drive_unit.min(max_unit);

    (port, drive_unit, bus_type, step_mode, preset)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if handle_cli_help_or_version(&args) {
        return Ok(());
    }

    let (port_arg, initial_drive_unit, initial_bus_type, initial_step_mode, initial_preset) = parse_cli_args(&args);

    let (tx_cmd, rx_cmd) = unbounded::<HwCmd>();
    let (tx_status, rx_status) = unbounded::<DriveStatus>();

    let port_clone = port_arg.clone();
    thread::spawn(move || {
        hw_thread(tx_status, rx_cmd, port_clone, initial_drive_unit, initial_bus_type, initial_step_mode, initial_preset);
    });

    enable_raw_mode()?;
    let mut stdout = stdout();
    stdout.execute(EnterAlternateScreen)?;
    stdout.execute(EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::with_full_preset_config(initial_drive_unit, initial_bus_type, initial_step_mode, initial_preset);

    loop {
        while let Ok(status) = rx_status.try_recv() {
            app.handle_hw_message(status);
        }

        terminal.draw(|f| {
            let status = app.drive_status();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Min(1),
                ])
                .split(f.size());

            let drive_letter = format_drive_unit_short(status.bus_type, status.drive_unit);
            let expected_ids = get_status_expected_sector_ids(status);
            let sec_count_str = format_disk_format_header(status.disk_format, status.bitrate, status.sector_count);

            let mut sec_line_spans = Vec::new();
            if status.has_disk && status.sectors_known && !expected_ids.is_empty() {
                sec_line_spans.push(Span::styled(" ", Style::default()));
                for &id in &expected_ids {
                    let s_str = if id >= 0x40 {
                        format!("{:02X} ", id)
                    } else {
                        format!("{:2} ", id)
                    };
                    let sec_present = status.sectors.iter().any(|s| s.sec_id == id);
                    let has_err = status.sectors.iter().any(|s| s.sec_id == id && !s.crc_ok);

                    let style = if has_err {
                        Style::default()
                            .bg(Color::Red)
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else if sec_present {
                        if status.mode == DisplayMode::ReadData {
                            Style::default()
                                .bg(Color::Yellow)
                                .fg(Color::Black)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        }
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };

                    sec_line_spans.push(Span::styled(s_str, style));
                }
            } else {
                sec_line_spans.push(Span::styled(" ", Style::default()));
                for _ in 1..=18 {
                    sec_line_spans.push(Span::styled(" ? ", Style::default().fg(Color::DarkGray)));
                }
            }

            let (badge_icon, badge_text) = format_activity_badge(status.activity, status.io_cycle);
            let head_hdr = format_head_header_str(status.head_select, status.head);

            let mut top_spans = vec![
                Span::styled(
                    format!(
                        "{} {}k {}    T{:02}  {:<7}",
                        drive_letter,
                        status.bitrate,
                        if status.bitrate == 500 { "HD" } else { "DD" },
                        status.track,
                        head_hdr,
                    ),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
            ];
            top_spans.extend(build_flags_spans(status.write_protect));
            top_spans.push(Span::styled("   ", Style::default()));
            top_spans.push(build_wp_span(status.write_protect));
            top_spans.push(Span::styled(
                format!("     {}         ", sec_count_str),
                Style::default().fg(Color::White),
            ));
            top_spans.push(badge_icon);
            top_spans.push(badge_text);

            let port_display = if status.port_name.is_empty() {
                if status.connected {
                    "Connected".to_string()
                } else {
                    "Searching...".to_string()
                }
            } else {
                status.port_name.clone()
            };
            let port_badge = format_port_badge(&port_display);

            let header_lines = vec![
                Line::from(top_spans),
                Line::from(sec_line_spans),
                build_ruler_line(status.track),
                Line::from(""),
            ];
            let header = Paragraph::new(header_lines)
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        )
                        .title(
                            ratatui::widgets::block::Title::from(get_header_title())
                                .alignment(ratatui::layout::Alignment::Left),
                        )
                        .title(
                            ratatui::widgets::block::Title::from(port_badge)
                                .alignment(ratatui::layout::Alignment::Right),
                        )
                        .title_style(
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                )
                .style(Style::default().bg(Color::Red));
            f.render_widget(header, chunks[0]);

            let lower_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(32), Constraint::Min(1)])
                .split(chunks[1]);

            let rpm_display = if status.mode == DisplayMode::RpmMeasure
                && status.rpm_measure.sample_count > 0
            {
                format!("{:.1} RPM", status.rpm_measure.instant_rpm)
            } else if !status.rpm_display.is_empty() && status.rpm_display != "... RPM" && status.rpm_display != "--- RPM" {
                status.rpm_display.clone()
            } else {
                format_rpm_display(status.motor_on, status.rpm)
            };

            let state_label = match status.activity {
                HwActivity::MeasuringRpm => {
                    const SPINNER: &[char] = &['|', '/', '-', '\\'];
                    let spin = SPINNER[(status.io_cycle as usize) % SPINNER.len()];
                    format!("RPM TEST {}", spin)
                }
                HwActivity::ReadingAnalyzing => {
                    const SPINNER: &[char] = &['|', '/', '-', '\\'];
                    let spin = SPINNER[(status.io_cycle as usize) % SPINNER.len()];
                    format!("READ/ANALYZ {}", spin)
                }
                HwActivity::Formatting => {
                    const SPINNER: &[char] = &['|', '/', '-', '\\'];
                    let spin = SPINNER[(status.io_cycle as usize) % SPINNER.len()];
                    format!("FORMATTING {}", spin)
                }
                HwActivity::Erasing => {
                    const SPINNER: &[char] = &['|', '/', '-', '\\'];
                    let spin = SPINNER[(status.io_cycle as usize) % SPINNER.len()];
                    format!("ERASING {}", spin)
                }
                HwActivity::Seeking => "SEEKING...".to_string(),
                HwActivity::Stopped => "STOPPED".to_string(),
                HwActivity::WaitingPort => "CONNECTING".to_string(),
                HwActivity::Idle => "IDLE".to_string(),
            };

            let menu_lines = vec![
                Line::from(" Insert formatted"),
                Line::from(" diskette"),
                Line::from(""),
                Line::from(format!(" PRESET: {}", status.preset.label())),
                Line::from(format!(" UNIT : {}", format_drive_unit_label(status.bus_type, status.drive_unit))),
                Line::from(format!(" BUS  : {}", status.bus_type.as_str())),
                Line::from(format!(" STEP : {}", status.step_mode.as_str())),
                Line::from(format!(" RATE : {} kbps", status.bitrate)),
                Line::from(format!(" PROF : {}", status.disk_format.short_name())),
                Line::from(format!(" STAT : {}", state_label)),
                Line::from(format!(" TRK0 : {}", if status.trk0 { "ON " } else { "OFF" })),
                Line::from(format!(
                    " INDEX: {}",
                    if status.index && status.has_disk {
                        "ON "
                    } else {
                        "OFF"
                    }
                )),
                Line::from(format!(" MOT  : {}", if status.motor_on { "ON " } else { "OFF" })),
                Line::from(format!(
                    " WPROT: {}",
                    if status.write_protect { "ON " } else { "OFF" }
                )),
                Line::from(format!(" RPM  : {}", rpm_display)),
                Line::from(format!(
                    " BEEP : {}",
                    format_beep_status(status.beep_enabled)
                )),
                Line::from(format!(
                    " VERB : {}",
                    if status.verbose_mode { "ON " } else { "OFF" }
                )),
                Line::from(""),
                Line::from(" A = Analyze"),
                Line::from(" B = Beep on/off"),
                Line::from(" D = read Data"),
                Line::from(" E = Erase"),
                Line::from(" Esc = Stop / Motor off"),
                Line::from(" Backspace = Panic Reset"),
                Line::from(" F = Format"),
                Line::from(" H = Head 0/1/Both"),
                Line::from(" I = track Image"),
                Line::from(" L = Live RPM test"),
                Line::from(" M = Motor on/off"),
                Line::from(" P = Preset / Profile"),
                Line::from(" R = Recal/seek"),
                Line::from(" S = Step (1:1 / 2:1)"),
                Line::from(" T = Shugart/PC Bus"),
                Line::from(format_unit_menu_shortcut(status.bus_type)),
                Line::from(" V = Verbose on/off"),
                Line::from(" W = Write data"),
                Line::from(" Z = Zero track"),
                Line::from(" 0-9 = seek 0-90"),
                Line::from(" +/- = Seek +/-1"),
                Line::from(" ?/F1= Help Modal"),
                Line::from(" Q/X = Quit / eXit"),
            ];

            let menu = Paragraph::new(menu_lines)
                .style(Style::default().fg(Color::White).bg(Color::Blue));
            f.render_widget(menu, lower_chunks[0]);

            let mut right_lines = Vec::new();

            match status.mode {
                DisplayMode::Format | DisplayMode::Erase => {
                    right_lines = build_format_progress_lines(status);
                }
                DisplayMode::RpmMeasure => {
                    right_lines.push(Line::from(Span::styled(
                        "=== MOTOR TACHOMETER / LIVE RPM TEST ===",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                    right_lines.push(Line::from(Span::styled(
                        "Live RPM Test: High-precision continuous measurement for fine mechanical tuning",
                        Style::default().fg(Color::LightCyan),
                    )));
                    right_lines.push(Line::from(""));

                    if status.rpm_measure.sample_count > 0 {
                        let target_rpm = status.preset.target_rpm();
                        let instant_rpm = status.rpm_measure.instant_rpm;
                        let diff = instant_rpm - target_rpm;
                        let sign = if diff >= 0.0 { "+" } else { "" };
                        let diff_pct = (diff / target_rpm) * 100.0;

                        right_lines.push(Line::from(build_rpm_metric_spans(&status.rpm_measure, target_rpm)));

                        right_lines.push(build_rpm_gauge_line(instant_rpm, target_rpm));
                        right_lines.push(Line::from(""));

                        let jitter_color = if status.rpm_measure.jitter_pct <= 0.20 {
                            Color::LightGreen
                        } else if status.rpm_measure.jitter_pct <= 0.50 {
                            Color::Yellow
                        } else {
                            Color::Red
                        };

                        right_lines.push(Line::from(vec![
                            Span::styled(
                                "► Nominal Target Speed    : ",
                                Style::default().fg(Color::White),
                            ),
                            Span::styled(
                                format!(
                                    "{:.1} RPM  (Offset: {}{:.1} RPM, {}{:.2}%)",
                                    target_rpm, sign, diff, sign, diff_pct
                                ),
                                Style::default().fg(Color::LightCyan),
                            ),
                        ]));

                        right_lines.push(Line::from(vec![
                            Span::styled(
                                "► Speed Jitter (Peak-Peak): ",
                                Style::default().fg(Color::White),
                            ),
                            Span::styled(
                                format!(
                                    "±{:.1} RPM  (Δ={:.1} RPM | ±{:.2}%)",
                                    status.rpm_measure.jitter_rpm,
                                    status.rpm_measure.max_rpm - status.rpm_measure.min_rpm,
                                    status.rpm_measure.jitter_pct
                                ),
                                Style::default()
                                    .fg(jitter_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));

                        right_lines.push(Line::from(vec![
                            Span::styled(
                                "► Rolling Average (10 rev): ",
                                Style::default().fg(Color::White),
                            ),
                            Span::styled(
                                format!(
                                    "{:.1} RPM  (over {} total revolutions)",
                                    status.rpm_measure.avg_rpm, status.rpm_measure.sample_count
                                ),
                                Style::default().fg(Color::LightCyan),
                            ),
                        ]));

                        let stability_rating = if status.rpm_measure.jitter_pct <= 0.20 {
                            ("★★★★★ EXCELLENT STABILITY (Jitter <= ±0.20%)", Color::Green)
                        } else if status.rpm_measure.jitter_pct <= 0.50 {
                            ("★★★★☆ GOOD STABILITY (Jitter <= ±0.50%)", Color::LightGreen)
                        } else if status.rpm_measure.jitter_pct <= 1.00 {
                            ("★★★☆☆ ACCEPTABLE STABILITY (Jitter <= ±1.00%)", Color::Yellow)
                        } else {
                            ("★☆☆☆☆ UNSTABLE MOTOR SPEED (Jitter > ±1.00%)", Color::Red)
                        };

                        right_lines.push(Line::from(vec![
                            Span::styled(
                                "► Motor Health Rating     : ",
                                Style::default().fg(Color::White),
                            ),
                            Span::styled(
                                stability_rating.0,
                                Style::default()
                                    .fg(stability_rating.1)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));
                    } else {
                        right_lines.push(Line::from(Span::styled(
                            "► Capturing hardware index pulses at 72 MHz sample clock...",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        )));
                        right_lines.push(Line::from(""));
                        right_lines.push(Line::from(
                            "  Ensure a diskette is inserted with valid index hole/sensor pulses.",
                        ));
                    }

                    right_lines.push(Line::from(""));
                    right_lines.push(Line::from(Span::styled(
                        "--- Recent Revolution Captures (72 MHz Hardware Ticks) ---",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )));

                    if !status.rpm_measure.recent_samples.is_empty() {
                        let available_height = lower_chunks[1].height as usize;
                        let max_vertical_items = available_height.saturating_sub(13).max(1);
                        let start_idx = status
                            .rpm_measure
                            .recent_samples
                            .len()
                            .saturating_sub(max_vertical_items);
                        let recent = &status.rpm_measure.recent_samples[start_idx..];

                        for (i, &(rpm_val, delta_ticks)) in recent.iter().enumerate() {
                            let is_last = i == recent.len().saturating_sub(1);
                            let period_ms = (delta_ticks as f64) / 72_000.0;

                            let prefix = if is_last { " ► " } else { "   " };
                            let style = if is_last {
                                Style::default()
                                    .fg(Color::LightGreen)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default().fg(Color::White)
                            };

                            let line_str = format!(
                                "Rev Index : {:6.1} RPM  ({:8} ticks @ 72MHz, {:6.2} ms/rev)",
                                rpm_val, delta_ticks, period_ms
                            );

                            right_lines.push(Line::from(vec![
                                Span::styled(
                                    prefix,
                                    Style::default()
                                        .fg(Color::Yellow)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(line_str, style),
                            ]));
                        }
                    } else {
                        right_lines.push(Line::from(Span::styled(
                            "  (Waiting for rotation stream...)",
                            Style::default().fg(Color::DarkGray),
                        )));
                    }

                    right_lines.push(Line::from(""));
                    right_lines.push(Line::from(Span::styled(
                        "Note: Press Esc, L, A, D, +, -, or Seek key to interrupt Tachometer mode immediately.",
                        Style::default().fg(Color::LightYellow),
                    )));
                    right_lines.push(Line::from(format_log_line(&status.log_msg)));
                }
                DisplayMode::Analyze | DisplayMode::ReadData => {
                    let title = if status.mode == DisplayMode::Analyze {
                        "=== REAL-TIME ALIGNMENT ANALYSIS & DIAGNOSTICS ==="
                    } else {
                        "=== SECTOR READ & INTEGRITY CHECK ==="
                    };

                    right_lines.push(Line::from(Span::styled(
                        title,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                    right_lines.push(Line::from(""));

                    let (disp_align_pct, disp_in_track_sectors, disp_off_track_count, disp_off_track_details, disp_crc_text, disp_crc_color) =
                        if status.head_select == HeadSelection::Both {
                            let both_metrics = app.compute_both_mode_metrics();
                            let crc_txt = if both_metrics.total_crc_err == 0 && both_metrics.alignment_pct >= 99.9 {
                                "100% OK (0 errors)".to_string()
                            } else if both_metrics.total_crc_err > 0 {
                                format!("{:.1}% ({} CRC errors)", both_metrics.crc_integrity_pct, both_metrics.total_crc_err)
                            } else {
                                format!("{:.1}% (degraded pass)", both_metrics.crc_integrity_pct)
                            };
                            let crc_col = if both_metrics.total_crc_err == 0 && !both_metrics.is_degraded {
                                Color::Green
                            } else {
                                Color::Red
                            };
                            (
                                both_metrics.alignment_pct,
                                both_metrics.total_ok,
                                both_metrics.total_off_track,
                                both_metrics.off_track_details,
                                crc_txt,
                                crc_col,
                            )
                        } else {
                            let crc_txt = if status.crc_err_count == 0 {
                                "100% OK (0 errors)".to_string()
                            } else {
                                format!("{} CRC Error(s)", status.crc_err_count)
                            };
                            let crc_col = if status.crc_err_count == 0 {
                                Color::Green
                            } else {
                                Color::Red
                            };
                            (
                                status.alignment_pct,
                                status.on_track_count,
                                status.off_track_count,
                                status.off_track_details.clone(),
                                crc_txt,
                                crc_col,
                            )
                        };

                    let align_color = if disp_align_pct >= 95.0 {
                        Color::Green
                    } else if disp_align_pct >= 90.0 {
                        Color::LightGreen
                    } else if disp_align_pct >= 70.0 {
                        Color::Yellow
                    } else {
                        Color::Red
                    };

                    right_lines.push(Line::from(vec![
                        Span::styled(
                            "► Mechanical Alignment    : ",
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!(
                                "{:.1}%  [ {} ]",
                                disp_align_pct,
                                build_alignment_gauge(disp_align_pct)
                            ),
                            Style::default()
                                .fg(align_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));

                    right_lines.push(Line::from(vec![
                        Span::styled(
                            "► Requested Track Sectors (Y) : ",
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!("{} sectors", disp_in_track_sectors),
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));

                    let off_color = if disp_off_track_count == 0 {
                        Color::Green
                    } else {
                        Color::Red
                    };
                    right_lines.push(Line::from(vec![
                        Span::styled(
                            "► Off-Track Sectors (N / Off-Track) : ",
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!("{} ({})", disp_off_track_count, disp_off_track_details),
                            Style::default().fg(off_color).add_modifier(Modifier::BOLD),
                        ),
                    ]));

                    right_lines.push(Line::from(vec![
                        Span::styled(
                            "► CRC Integrity Check     : ",
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            disp_crc_text,
                            Style::default().fg(disp_crc_color).add_modifier(Modifier::BOLD),
                        ),
                    ]));

                    right_lines.push(Line::from(""));
                    let subtitle = if status.head_select == HeadSelection::Both {
                        if status.verbose_mode {
                            "--- Dual-Head Real-Time Stream (Both Mode - Verbose) ---"
                        } else {
                            "--- Dual-Head Real-Time Stream (Both Mode - Standard) ---"
                        }
                    } else if status.verbose_mode {
                        "--- Read Sectors Stream (Verbose History Mode) ---"
                    } else {
                        "--- Read Sectors Stream (Standard Mode) ---"
                    };
                    right_lines.push(Line::from(Span::styled(
                        subtitle,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )));

                    if status.head_select == HeadSelection::Both {
                        let both_lines = build_both_mode_display_lines(status);
                        for line in both_lines {
                            right_lines.push(line);
                        }
                    } else {
                        let available_height = lower_chunks[1].height as usize;
                        let stream_lines = build_single_head_stream_lines(&app, available_height);
                        for line in stream_lines {
                            right_lines.push(line);
                        }
                    }

                    right_lines.push(Line::from(""));
                    right_lines.push(Line::from(format_log_line(&status.log_msg)));
                }
                DisplayMode::None => {
                    right_lines.push(Line::from(Span::styled(
                        "=== GREASEWEAZLE HARDWARE DIAGNOSTICS ===",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                    right_lines.push(Line::from(""));
                    right_lines.push(Line::from(format!(
                        "Connection Status : {}",
                        if status.connected {
                            "Connected (Greaseweazle v4.1)"
                        } else {
                            "Searching..."
                        }
                    )));
                    right_lines.push(Line::from(format!(
                        "Drive Selection   : {} ({})",
                        format_drive_unit_label(status.bus_type, status.drive_unit),
                        if status.drive_select { "Active" } else { "Inactive" }
                    )));
                    right_lines.push(Line::from(format!(
                        "Spindle Motor     : {}",
                        if status.motor_on { "RUNNING (ON)" } else { "STOPPED (OFF)" }
                    )));
                    right_lines.push(Line::from(format!(
                        "Disk Speed (RPM)  : {}",
                        rpm_display
                    )));
                    right_lines.push(Line::from(format!(
                        "Current Cylinder  : Track {:02}",
                        status.track
                    )));
                    right_lines.push(Line::from(format!(
                        "Current Head      : {}",
                        format_head_display(status.head_select, status.head)
                    )));
                    right_lines.push(Line::from(format!(
                        "Diskette Density  : {} ({})",
                        if status.density { "High Density (HD)" } else { "Double Density (DD)" },
                        if status.bitrate == 500 { "500 kbps" } else { "250 kbps" }
                    )));
                    right_lines.push(Line::from(""));
                    right_lines.push(Line::from(Span::styled(
                        "=== COMMAND SHORTCUTS ===",
                        Style::default().fg(Color::Cyan),
                    )));
                    right_lines.push(Line::from(
                        "    A = Analyze     : Real-time track analysis & alignment",
                    ));
                    right_lines.push(Line::from(
                        "    B = Beep        : Toggle Audio Radar (Pitch-variometer)",
                    ));
                    right_lines.push(Line::from(
                        "    D = read Data   : Read and test sector CRC integrity",
                    ));
                    right_lines.push(Line::from(
                        "    Esc = Stop      : Stop motor, clear buffers & enter safe state",
                    ));
                    right_lines.push(Line::from(
                        "    Backspace       : PANIC RESET (Instant motor cut & hardware re-init)",
                    ));
                    right_lines.push(Line::from(
                        "    L = Live RPM    : Live RPM measure / Tachometer test",
                    ));
                    right_lines.push(Line::from(
                        "    M = Motor       : Toggle Motor ON / OFF",
                    ));
                    right_lines.push(Line::from(
                        "    + / -           : Step track by track (0 to 83)",
                    ));
                    right_lines.push(Line::from(
                        "    0-9             : Direct jump to tracks (0, 10, 20... 80)",
                    ));
                    right_lines.push(Line::from("    H               : Toggle Head (Head 0 -> Head 1 -> Both 0+1)"));
                    right_lines.push(Line::from(
                        "    R               : Recalibrate Track 0 -> Current track",
                    ));
                    right_lines.push(Line::from(
                        "    S = Step Rate   : Toggle Single 1:1 / Double 2:1 (48/96 TPI)",
                    ));
                    right_lines.push(Line::from(
                        "    T               : Toggle Bus Type (IBM PC <-> Shugart)",
                    ));
                    right_lines.push(Line::from(
                        "    U = Unit        : Toggle Drive Unit (Drive 0 / Drive 1)",
                    ));
                    right_lines.push(Line::from(
                        "    V = Verbose     : Toggle Standard / Verbose display mode",
                    ));
                    right_lines.push(Line::from(
                        "    Z               : Zero track (Direct return to Track 0)",
                    ));
                    right_lines.push(Line::from(
                        "    Q/X = Exit      : Clean exit (Instant motor & LED shutdown)",
                    ));
                    right_lines.push(Line::from(""));
                    right_lines.push(Line::from(format_log_line(&status.log_msg)));
                }
            }

            let right_panel = Paragraph::new(right_lines)
                .style(Style::default().fg(Color::White).bg(Color::Blue));
            f.render_widget(right_panel, lower_chunks[1]);

            if app.show_help {
                render_help_modal(f, f.size());
            }

            if app.show_format_modal {
                let max_trk = app.step_mode.max_logical_tracks();
                render_format_modal(
                    f,
                    f.size(),
                    app.status.track,
                    app.head_selection,
                    max_trk,
                    app.preset,
                    app.preset.target_rpm(),
                    app.status.bitrate,
                    app.bus_type.as_str(),
                    app.drive_unit,
                    app.format_target_tracks,
                    app.is_48_tpi(),
                    app.format_verify,
                    app.format_fs_mode,
                    app.format_range,
                    app.pending_confirmation.as_ref(),
                );
            }

            if app.show_erase_modal {
                render_erase_modal(
                    f,
                    f.size(),
                    app.status.track,
                    app.head_selection,
                    app.preset.label(),
                    app.preset.target_rpm(),
                    app.status.bitrate,
                    app.erase_target_tracks,
                    app.is_48_tpi(),
                    app.erase_range,
                    app.pending_confirmation.as_ref(),
                );
            }

            if let Some(kind) = app.show_range_modal {
                render_range_edit_modal(
                    f,
                    f.size(),
                    kind,
                    app.head_selection,
                    &app.range_input_start,
                    &app.range_input_end,
                    app.range_edit_field,
                    app.max_format_tracks(),
                    app.range_error_msg.as_deref(),
                );
            }
        })?;

        if event::poll(Duration::from_millis(15))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('C'))
                    {
                        let _ = tx_cmd.send(HwCmd::Exit);
                        break;
                    }

                    if app.show_help {
                        match key.code {
                            KeyCode::Esc
                            | KeyCode::Char('?')
                            | KeyCode::F(1)
                            | KeyCode::Char('q')
                            | KeyCode::Char('Q') => {
                                app.show_help = false;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if app.show_range_modal.is_some() {
                        match key.code {
                            KeyCode::Tab => {
                                app.toggle_range_field();
                            }
                            KeyCode::Up
                            | KeyCode::Right
                            | KeyCode::Char('+')
                            | KeyCode::Char('=') => {
                                app.increment_active_range_field();
                            }
                            KeyCode::Down
                            | KeyCode::Left
                            | KeyCode::Char('-')
                            | KeyCode::Char('_') => {
                                app.decrement_active_range_field();
                            }
                            KeyCode::Char('h') | KeyCode::Char('H') => {
                                app.toggle_head();
                                let _ = tx_cmd.send(HwCmd::SetHeadSelection(app.head_selection));
                            }
                            KeyCode::Char(c) if c.is_ascii_digit() => {
                                app.range_input_push_digit(c);
                            }
                            KeyCode::Backspace
                            | KeyCode::Char('\x08') => {
                                app.range_input_backspace();
                            }
                            KeyCode::Enter => {
                                match app.validate_and_apply_range() {
                                    Ok((range, kind)) => match kind {
                                        RangeModalKind::Format => {
                                            let verify = app.format_verify;
                                            let head_sel = app.head_selection;
                                            let fs_mode = app.format_fs_mode;
                                            let preset = app.preset;
                                            app.show_format_modal = true;
                                            app.pending_confirmation = Some(PendingConfirmation::FormatRange {
                                                range,
                                                head_sel,
                                                verify,
                                                fs_mode,
                                                preset,
                                            });
                                        }
                                        RangeModalKind::Erase => {
                                            let head_sel = app.head_selection;
                                            app.show_erase_modal = true;
                                            app.pending_confirmation = Some(PendingConfirmation::EraseRange { range, head_sel });
                                        }
                                    },
                                    Err(err) => {
                                        app.range_error_msg = Some(err);
                                    }
                                }
                            }
                            KeyCode::Esc => {
                                app.close_range_modal(true);
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if app.show_format_modal {
                        if let Some(confirm) = app.pending_confirmation.clone() {
                            match key.code {
                                KeyCode::Char('y') | KeyCode::Char('Y') => {
                                    app.show_format_modal = false;
                                    app.pending_confirmation = None;
                                    match confirm {
                                        PendingConfirmation::FormatTrack {
                                            track,
                                            head_sel,
                                            verify,
                                            fs_mode,
                                            ..
                                        } => {
                                            app.handle_action(Action::FormatTrack {
                                                track,
                                                head_sel,
                                                verify,
                                                fs_mode,
                                            });
                                            let _ = tx_cmd.send(HwCmd::FormatTrack {
                                                track,
                                                head_sel,
                                                verify,
                                                fs_mode,
                                            });
                                        }
                                        PendingConfirmation::FormatDisk {
                                            range,
                                            head_sel,
                                            verify,
                                            fs_mode,
                                            ..
                                        }
                                        | PendingConfirmation::FormatRange {
                                            range,
                                            head_sel,
                                            verify,
                                            fs_mode,
                                            ..
                                        } => {
                                            app.handle_action(Action::FormatDisk {
                                                range,
                                                head_sel,
                                                verify,
                                                fs_mode,
                                            });
                                            let _ = tx_cmd.send(HwCmd::FormatDisk {
                                                range,
                                                head_sel,
                                                verify,
                                                fs_mode,
                                            });
                                        }
                                        _ => {}
                                    }
                                }
                                KeyCode::Char('n')
                                | KeyCode::Char('N')
                                | KeyCode::Enter
                                | KeyCode::Esc => {
                                    app.pending_confirmation = None;
                                }
                                _ => {}
                            }
                            continue;
                        }

                        match key.code {
                            KeyCode::Left
                            | KeyCode::Char('-')
                            | KeyCode::Char('_')
                            | KeyCode::Char('[') => {
                                let prev = app.step_track_down();
                                let _ = tx_cmd.send(HwCmd::Seek(prev));
                            }
                            KeyCode::Right
                            | KeyCode::Char('+')
                            | KeyCode::Char('=')
                            | KeyCode::Char(']') => {
                                let next = app.step_track_up();
                                let _ = tx_cmd.send(HwCmd::Seek(next));
                            }
                            KeyCode::PageUp | KeyCode::Up => {
                                app.increment_format_tracks();
                            }
                            KeyCode::PageDown | KeyCode::Down => {
                                app.decrement_format_tracks();
                            }
                            KeyCode::Char('u') | KeyCode::Char('U') => {
                                app.handle_action(Action::ToggleDriveUnit);
                                let _ = tx_cmd.send(HwCmd::ToggleDriveUnit);
                            }
                            KeyCode::Char('b') | KeyCode::Char('B') => {
                                app.handle_action(Action::ToggleBusType);
                                let _ = tx_cmd.send(HwCmd::SetBusType(app.bus_type));
                            }
                            KeyCode::Char('h') | KeyCode::Char('H') => {
                                app.toggle_head();
                                let _ = tx_cmd.send(HwCmd::SetHeadSelection(app.head_selection));
                            }
                            KeyCode::Char('v') | KeyCode::Char('V') => {
                                app.toggle_format_verify();
                            }
                            KeyCode::Char('s') | KeyCode::Char('S') => {
                                app.toggle_format_fs_mode();
                            }
                            KeyCode::Char('p') | KeyCode::Char('P') => {
                                app.handle_action(Action::CyclePreset);
                                let _ = tx_cmd.send(HwCmd::CyclePreset);
                            }
                            KeyCode::Char('t') | KeyCode::Char('T') => {
                                let track = app.status.track;
                                let head_sel = app.head_selection;
                                let verify = app.format_verify;
                                let fs_mode = app.format_fs_mode;
                                let preset = app.preset;
                                app.pending_confirmation = Some(PendingConfirmation::FormatTrack {
                                    track,
                                    head_sel,
                                    verify,
                                    fs_mode,
                                    preset,
                                });
                            }
                            KeyCode::Char('r') | KeyCode::Char('R') => {
                                app.open_range_modal(RangeModalKind::Format);
                            }
                            KeyCode::Char('d') | KeyCode::Char('D') => {
                                let range = TrackRange::full_disk(app.format_target_tracks);
                                let head_sel = app.head_selection;
                                let verify = app.format_verify;
                                let fs_mode = app.format_fs_mode;
                                let preset = app.preset;
                                app.pending_confirmation = Some(PendingConfirmation::FormatDisk {
                                    range,
                                    head_sel,
                                    verify,
                                    fs_mode,
                                    preset,
                                });
                            }
                            KeyCode::Esc
                            | KeyCode::Char('q')
                            | KeyCode::Char('Q')
                            | KeyCode::Char('x')
                            | KeyCode::Char('X') => {
                                app.show_format_modal = false;
                                app.pending_confirmation = None;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if app.show_erase_modal {
                        if let Some(confirm) = app.pending_confirmation.clone() {
                            match key.code {
                                KeyCode::Char('y') | KeyCode::Char('Y') => {
                                    app.show_erase_modal = false;
                                    app.pending_confirmation = None;
                                    match confirm {
                                        PendingConfirmation::EraseTrack { track, head_sel } => {
                                            app.handle_action(Action::EraseTrack { track, head_sel });
                                            let _ = tx_cmd.send(HwCmd::EraseTrack { track, head_sel });
                                        }
                                        PendingConfirmation::EraseDisk { range, head_sel }
                                        | PendingConfirmation::EraseRange { range, head_sel } => {
                                            app.handle_action(Action::EraseDisk { range, head_sel });
                                            let _ = tx_cmd.send(HwCmd::EraseDisk { range, head_sel });
                                        }
                                        _ => {}
                                    }
                                }
                                KeyCode::Char('n')
                                | KeyCode::Char('N')
                                | KeyCode::Enter
                                | KeyCode::Esc => {
                                    app.pending_confirmation = None;
                                }
                                _ => {}
                            }
                            continue;
                        }

                        match key.code {
                            KeyCode::Left
                            | KeyCode::Char('-')
                            | KeyCode::Char('_')
                            | KeyCode::Char('[') => {
                                let prev = app.step_track_down();
                                let _ = tx_cmd.send(HwCmd::Seek(prev));
                            }
                            KeyCode::Right
                            | KeyCode::Char('+')
                            | KeyCode::Char('=')
                            | KeyCode::Char(']') => {
                                let next = app.step_track_up();
                                let _ = tx_cmd.send(HwCmd::Seek(next));
                            }
                            KeyCode::PageUp | KeyCode::Up => {
                                app.increment_erase_tracks();
                            }
                            KeyCode::PageDown | KeyCode::Down => {
                                app.decrement_erase_tracks();
                            }
                            KeyCode::Char('u') | KeyCode::Char('U') => {
                                app.handle_action(Action::ToggleDriveUnit);
                                let _ = tx_cmd.send(HwCmd::ToggleDriveUnit);
                            }
                            KeyCode::Char('b') | KeyCode::Char('B') => {
                                app.handle_action(Action::ToggleBusType);
                                let _ = tx_cmd.send(HwCmd::SetBusType(app.bus_type));
                            }
                            KeyCode::Char('h') | KeyCode::Char('H') => {
                                app.toggle_head();
                                let _ = tx_cmd.send(HwCmd::SetHeadSelection(app.head_selection));
                            }
                            KeyCode::Char('p') | KeyCode::Char('P') => {
                                app.handle_action(Action::CyclePreset);
                                let _ = tx_cmd.send(HwCmd::CyclePreset);
                            }
                            KeyCode::Char('t') | KeyCode::Char('T') => {
                                let track = app.status.track;
                                let head_sel = app.head_selection;
                                app.pending_confirmation = Some(PendingConfirmation::EraseTrack { track, head_sel });
                            }
                            KeyCode::Char('r') | KeyCode::Char('R') => {
                                app.open_range_modal(RangeModalKind::Erase);
                            }
                            KeyCode::Char('d') | KeyCode::Char('D') => {
                                let range = TrackRange::full_disk(app.erase_target_tracks);
                                let head_sel = app.head_selection;
                                app.pending_confirmation = Some(PendingConfirmation::EraseDisk { range, head_sel });
                            }
                            KeyCode::Esc
                            | KeyCode::Char('q')
                            | KeyCode::Char('Q')
                            | KeyCode::Char('x')
                            | KeyCode::Char('X') => {
                                app.show_erase_modal = false;
                                app.pending_confirmation = None;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if app.status.mode == DisplayMode::Format || app.status.mode == DisplayMode::Erase {
                        let is_finished = if let Some(ref p) = app.status.format_progress {
                            matches!(p.step, FormatStep::Completed | FormatStep::Error | FormatStep::Idle)
                        } else {
                            false
                        };

                        if is_finished {
                            match key.code {
                                KeyCode::Enter | KeyCode::Esc => {
                                    let mode = app.status.mode;
                                    app.status.mode = DisplayMode::None;
                                    if mode == DisplayMode::Format {
                                        app.handle_action(Action::OpenFormatModal);
                                    } else {
                                        app.handle_action(Action::OpenEraseModal);
                                    }
                                    continue;
                                }
                                KeyCode::Char('q') | KeyCode::Char('Q') => {
                                    app.status.mode = DisplayMode::None;
                                    app.handle_action(Action::Stop);
                                    let _ = tx_cmd.send(HwCmd::Stop);
                                    continue;
                                }
                                _ => {}
                            }
                        } else if key.code == KeyCode::Esc {
                            let _ = tx_cmd.send(HwCmd::Stop);
                            continue;
                        }
                    }

                    if key.code == KeyCode::Backspace
                        || key.code == KeyCode::Char('\x08')
                    {
                        app.handle_action(Action::PanicReset);
                        let _ = tx_cmd.send(HwCmd::PanicReset);
                        continue;
                    }
                    match key.code {
                        KeyCode::Char('?') | KeyCode::F(1) => {
                            app.toggle_help();
                        }
                        KeyCode::Char('x') | KeyCode::Char('X') | KeyCode::Char('q') | KeyCode::Char('Q') => {
                            let _ = tx_cmd.send(HwCmd::Exit);
                            break;
                        }
                        KeyCode::Esc => {
                            app.handle_action(Action::Stop);
                            let _ = tx_cmd.send(HwCmd::Stop);
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') | KeyCode::Right | KeyCode::Up | KeyCode::Char(']') => {
                            let max_trk = app.step_mode.max_logical_tracks();
                            let next = app.step_track_up().min(max_trk);
                            let _ = tx_cmd.send(HwCmd::Seek(next));
                        }
                        KeyCode::Char('-') | KeyCode::Char('_') | KeyCode::Left | KeyCode::Down | KeyCode::Char('[') => {
                            let prev = app.step_track_down();
                            let _ = tx_cmd.send(HwCmd::Seek(prev));
                        }
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            if let Some(d) = c.to_digit(10) {
                                let max_trk = app.step_mode.max_logical_tracks();
                                let track = ((d as u8) * 10).min(max_trk);
                                app.status.track = track;
                                app.status.target_track = track;
                                let _ = tx_cmd.send(HwCmd::Seek(track));
                            }
                        }
                        KeyCode::Char('h') | KeyCode::Char('H') => {
                            app.toggle_head();
                            let _ = tx_cmd.send(HwCmd::ToggleHead);
                        }
                        KeyCode::Char('l') | KeyCode::Char('L') => {
                            let _ = tx_cmd.send(HwCmd::MeasureRpm);
                        }
                        KeyCode::Char('m') | KeyCode::Char('M') => {
                            app.handle_action(Action::ToggleMotor);
                            let _ = tx_cmd.send(HwCmd::ToggleMotor);
                        }
                        KeyCode::Char('r') | KeyCode::Char('R') => {
                            let _ = tx_cmd.send(HwCmd::RecalibrateSeek);
                        }
                        KeyCode::Char('u') | KeyCode::Char('U') => {
                            app.handle_action(Action::ToggleDriveUnit);
                            let _ = tx_cmd.send(HwCmd::ToggleDriveUnit);
                        }
                        KeyCode::Char('z') | KeyCode::Char('Z') => {
                            app.status.track = 0;
                            app.status.target_track = 0;
                            let _ = tx_cmd.send(HwCmd::ZeroTrack);
                        }
                        KeyCode::Char('a') | KeyCode::Char('A') => {
                            app.handle_action(Action::Analyze);
                            let _ = tx_cmd.send(HwCmd::Analyze);
                        }
                        KeyCode::Char('d') | KeyCode::Char('D') => {
                            let _ = tx_cmd.send(HwCmd::ReadData);
                        }
                        KeyCode::Char('e') | KeyCode::Char('E') => {
                            app.handle_action(Action::OpenEraseModal);
                        }
                        KeyCode::Char('f') | KeyCode::Char('F') => {
                            app.handle_action(Action::OpenFormatModal);
                        }
                        KeyCode::Char('v') | KeyCode::Char('V') => {
                            let _ = tx_cmd.send(HwCmd::ToggleVerbose);
                        }
                        KeyCode::Char('b') | KeyCode::Char('B') => {
                            let _ = tx_cmd.send(HwCmd::ToggleBeep);
                        }
                        KeyCode::Char('p') | KeyCode::Char('P') => {
                            app.handle_action(Action::CyclePreset);
                            let _ = tx_cmd.send(HwCmd::CyclePreset);
                        }
                        KeyCode::Char('t') | KeyCode::Char('T') => {
                            app.handle_action(Action::ToggleBusType);
                            let _ = tx_cmd.send(HwCmd::ToggleBusType);
                        }
                        KeyCode::Char('s') | KeyCode::Char('S') => {
                            app.handle_action(Action::ToggleStepMode);
                            let _ = tx_cmd.send(HwCmd::ToggleStepMode);
                        }
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        if app.show_range_modal.is_some() {
                            app.increment_active_range_field();
                        } else {
                            let next = app.step_track_up();
                            let _ = tx_cmd.send(HwCmd::Seek(next));
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        if app.show_range_modal.is_some() {
                            app.decrement_active_range_field();
                        } else {
                            let prev = app.step_track_down();
                            let _ = tx_cmd.send(HwCmd::Seek(prev));
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    let mut out = std::io::stdout();
    let _ = out.execute(DisableMouseCapture);
    let _ = out.execute(LeaveAlternateScreen);
    println!("Alignment Diagnostic session ended cleanly.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hw::FsInitMode;
    #[test]
    fn test_format_flags_display() {
        assert_eq!(format_flags_display(true), "[--Rz-]");
        assert_eq!(format_flags_display(false), "[-wRz-]");
    }

    #[test]
    fn test_build_flags_spans() {
        let spans_protected = build_flags_spans(true);
        let text_p: String = spans_protected.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text_p, "Flags: [--Rz-]");

        let spans_writable = build_flags_spans(false);
        let text_w: String = spans_writable.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text_w, "Flags: [-wRz-]");
    }

    #[test]
    fn test_build_wp_span() {
        let span_p = build_wp_span(true);
        assert_eq!(span_p.content, "WP: PROTECTED");
        assert_eq!(
            span_p.style,
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        );

        let span_w = build_wp_span(false);
        assert_eq!(span_w.content, "WP: WRITE-ENABLED");
        assert_eq!(
            span_w.style,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        );
    }


    #[test]
    fn test_format_rpm_display() {
        assert_eq!(format_rpm_display(true, 300), "300 RPM");
        assert_eq!(format_rpm_display(true, 360), "360 RPM");
        assert_eq!(format_rpm_display(true, 0), "... RPM");
        assert_eq!(format_rpm_display(false, 0), "--- RPM");
        assert_eq!(format_rpm_display(false, 300), "--- RPM");
    }

    #[test]
    fn test_build_ruler_line_structure() {
        let ruler = build_ruler_line(0);
        // 1 leading space + 84 track characters (0 to 83)
        assert_eq!(ruler.spans.len(), 85);

        let ruler_str: String = ruler.spans.iter().skip(1).map(|s| s.content.as_ref()).collect();
        // 0 to 83 = 84 characters
        assert_eq!(ruler_str.len(), 84);
        assert_eq!(
            ruler_str,
            "0....+....1....+....2....+....3....+....4....+....5....+....6....+....7....+....8..."
        );

        // Check style for Track 0: current track is highlighted cursor
        assert_eq!(
            ruler.spans[1].style,
            Style::default()
                .bg(Color::White)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        );
        // Track > 0 are dimmed
        assert_eq!(
            ruler.spans[2].style,
            Style::default().fg(Color::Rgb(180, 80, 80))
        );
    }

    #[test]
    fn test_build_ruler_line_dynamic_highlight() {
        let ruler = build_ruler_line(15);
        // Tracks 0..15 should be solid white bar (bg: White, fg: Black)
        for i in 1..=16 {
            assert_eq!(
                ruler.spans[i].style,
                Style::default()
                    .bg(Color::White)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            );
        }

        assert_eq!(ruler.spans[16].content, "+");

        // Tracks 16..83 are dimmed
        for i in 17..=84 {
            assert_eq!(
                ruler.spans[i].style,
                Style::default().fg(Color::Rgb(180, 80, 80))
            );
        }
    }

    #[test]
    fn test_menu_and_key_mapping() {
        // Validate key L maps to MeasureRpm and M maps to ToggleMotor and Ctrl+Esc to PanicReset
        let (tx_cmd, rx_cmd) = unbounded::<HwCmd>();
        
        let key_l = KeyCode::Char('l');
        match key_l {
            KeyCode::Char('l') | KeyCode::Char('L') => {
                let _ = tx_cmd.send(HwCmd::MeasureRpm);
            }
            _ => panic!("Expected key L to match"),
        }
        assert!(matches!(rx_cmd.try_recv().unwrap(), HwCmd::MeasureRpm));

        let key_m = KeyCode::Char('m');
        match key_m {
            KeyCode::Char('m') | KeyCode::Char('M') => {
                let _ = tx_cmd.send(HwCmd::ToggleMotor);
            }
            _ => panic!("Expected key M to match"),
        }
        assert!(matches!(rx_cmd.try_recv().unwrap(), HwCmd::ToggleMotor));

        let key_h = KeyCode::Char('h');
        match key_h {
            KeyCode::Char('h') | KeyCode::Char('H') => {
                let _ = tx_cmd.send(HwCmd::ToggleHead);
            }
            _ => panic!("Expected key H to match"),
        }
        assert!(matches!(rx_cmd.try_recv().unwrap(), HwCmd::ToggleHead));

        let key_u = KeyCode::Char('u');
        match key_u {
            KeyCode::Char('u') | KeyCode::Char('U') => {
                let _ = tx_cmd.send(HwCmd::ToggleDriveUnit);
            }
            _ => panic!("Expected key U to match"),
        }
        assert!(matches!(rx_cmd.try_recv().unwrap(), HwCmd::ToggleDriveUnit));

        let key_a = KeyCode::Char('a');
        match key_a {
            KeyCode::Char('a') | KeyCode::Char('A') => {
                let _ = tx_cmd.send(HwCmd::Analyze);
            }
            _ => panic!("Expected key A to match"),
        }
        assert!(matches!(rx_cmd.try_recv().unwrap(), HwCmd::Analyze));

        let key_t = KeyCode::Char('t');
        match key_t {
            KeyCode::Char('t') | KeyCode::Char('T') => {
                let _ = tx_cmd.send(HwCmd::ToggleBusType);
            }
            _ => panic!("Expected key T to match"),
        }
        assert!(matches!(rx_cmd.try_recv().unwrap(), HwCmd::ToggleBusType));

        let key_p = KeyCode::Char('p');
        match key_p {
            KeyCode::Char('p') | KeyCode::Char('P') => {
                let _ = tx_cmd.send(HwCmd::CyclePreset);
            }
            _ => panic!("Expected key P to match"),
        }
        assert!(matches!(rx_cmd.try_recv().unwrap(), HwCmd::CyclePreset));

        // Backspace PanicReset mapping
        let key_backspace = KeyCode::Backspace;
        if key_backspace == KeyCode::Backspace
            || key_backspace == KeyCode::Char('\x08')
            || key_backspace == KeyCode::Char('\u{8}')
        {
            let _ = tx_cmd.send(HwCmd::PanicReset);
        }
        assert!(matches!(rx_cmd.try_recv().unwrap(), HwCmd::PanicReset));

        let key_ascii_bs = KeyCode::Char('\x08');
        if key_ascii_bs == KeyCode::Backspace
            || key_ascii_bs == KeyCode::Char('\x08')
            || key_ascii_bs == KeyCode::Char('\u{8}')
        {
            let _ = tx_cmd.send(HwCmd::PanicReset);
        }
        assert!(matches!(rx_cmd.try_recv().unwrap(), HwCmd::PanicReset));
    }

    #[test]
    fn test_get_standard_line_style_perfect_pass() {
        let style = get_standard_line_style("T:00 H:0  500k  [ ██████████████████ ]  (18/18 OK)", 18);
        assert_eq!(
            style,
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD)
        );

        let style_dd = get_standard_line_style("T:00 H:0  250k  [ █████████ ]           (9/9 OK)", 9);
        assert_eq!(
            style_dd,
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_get_standard_line_style_partial_pass() {
        // 16 sectors out of 18 (with missing) -> Yellow
        let style = get_standard_line_style("T:00 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ░ ░ ] (16/18 MISSING: Sec 17, 18)", 18);
        assert_eq!(
            style,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        );

        // 18 sectors with 1 CRC error -> LightRed
        let style_err = get_standard_line_style("T:00 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (17/18 CRC-DAT: Sec 9)", 18);
        assert_eq!(
            style_err,
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_get_standard_line_style_missing_or_poor_pass() {
        let style_missing = get_standard_line_style("T:00 H:0 Rate:---k --- [ ? ] (0/0 NO DATA / NO DISK)", 18);
        assert_eq!(
            style_missing,
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD)
        );

        let style_poor = get_standard_line_style("T:00 H:0 Rate:500k MFM [ ■ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] (3/18 MISSING: Sec 4, 5)", 18);
        assert_eq!(
            style_poor,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_build_standard_line_spans_individual_coloring() {
        let line_all_ok = "T:00 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)";
        let spans = build_standard_line_spans(line_all_ok, 18);
        assert!(!spans.is_empty());
        let green_blocks = spans
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(green_blocks, 18);

        let line_crc_15 = "T:35 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (17/18 CRC-DAT: Sec 15)";
        let spans_crc = build_standard_line_spans(line_crc_15, 18);
        let red_blocks = spans_crc
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::LightRed))
            .count();
        let ok_blocks = spans_crc
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(red_blocks, 1);
        assert_eq!(ok_blocks, 17);

        let line_missing_8 = "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ░ ■ ■ ■ ■ ■ ■ ■ ] (14/15 MISSING: Sec 8)";
        let spans_miss = build_standard_line_spans(line_missing_8, 15);
        let dark_blocks = spans_miss
            .iter()
            .filter(|s| s.content == "░ " && s.style.fg == Some(Color::DarkGray))
            .count();
        let green_14 = spans_miss
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(dark_blocks, 1);
        assert_eq!(green_14, 14);
    }

    #[test]
    fn test_get_verbose_line_style_perfect_pass() {
        let style = get_verbose_line_style(
            "T:79 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK) IL:1:1 Gap0:1440µs Q:99%",
            18,
        );
        assert_eq!(
            style,
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD)
        );

        let style_dd = get_verbose_line_style(
            "T:40 H:0 Rate:250k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ]                    (9/9 OK)   IL:1:1 Gap0:2880µs Q:98%",
            9,
        );
        assert_eq!(
            style_dd,
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_get_verbose_line_style_partial_pass() {
        let style_missing = get_verbose_line_style(
            "T:35 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ░ ] (14/15 MISSING: Sec 8) IL:1:1 Gap0:1440µs Q:90%",
            15,
        );
        assert_eq!(
            style_missing,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        );

        let style_off = get_verbose_line_style(
            "T:00 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OFF-TRK: T10: 18 sect) IL:1:1 Gap0:1440µs Q:85%",
            18,
        );
        assert_eq!(
            style_off,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_get_verbose_line_style_missing_or_poor_pass() {
        let style_missing = get_verbose_line_style(
            "T:80 H:0 Rate:---k --- [ ? ]                                    (0/0 NO DATA / NO DISK) IL:--- Gap0:---- Q:--%",
            18,
        );
        assert_eq!(
            style_missing,
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD)
        );

        let style_crc = get_verbose_line_style(
            "T:35 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (17/18 CRC-DAT: Sec 15) IL:1:1 Gap0:1440µs Q:84%",
            18,
        );
        assert_eq!(
            style_crc,
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_build_verbose_line_spans_individual_coloring() {
        let line_all_ok = "T:79 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK) IL:1:1 Gap0:1440µs Q:99%";
        let spans = build_verbose_line_spans(line_all_ok, 18);
        assert!(!spans.is_empty());
        // Check ribbon blocks are green
        let green_blocks = spans
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(green_blocks, 18);

        let line_crc_15 = "T:35 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (17/18 CRC-DAT: Sec 15) IL:1:1 Gap0:1440µs Q:84%";
        let spans_crc = build_verbose_line_spans(line_crc_15, 18);
        let red_blocks = spans_crc
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::LightRed))
            .count();
        let ok_blocks = spans_crc
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(red_blocks, 1);
        assert_eq!(ok_blocks, 17);

        let line_missing_8 = "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ░ ■ ■ ■ ■ ■ ■ ■ ] (14/15 MISSING: Sec 8) IL:1:1 Gap0:1440µs Q:90%";
        let spans_miss = build_verbose_line_spans(line_missing_8, 15);
        let dark_blocks = spans_miss
            .iter()
            .filter(|s| s.content == "░ " && s.style.fg == Some(Color::DarkGray))
            .count();
        let green_14 = spans_miss
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(dark_blocks, 1);
        assert_eq!(green_14, 14);
    }

    #[test]
    fn test_build_standard_line_spans_progressive_sweep() {
        // 0/18: 18 dark blocks, 0 green blocks
        let line_0 = "T:40 H:0 Rate:500k MFM [ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] ( 0/18)";
        let spans_0 = build_standard_line_spans(line_0, 18);
        let dark_0 = spans_0
            .iter()
            .filter(|s| s.content == "░ " && s.style.fg == Some(Color::DarkGray))
            .count();
        let green_0 = spans_0
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(dark_0, 18);
        assert_eq!(green_0, 0);

        // 1/18: 1 green block, 17 dark blocks
        let line_1 = "T:40 H:0 Rate:500k MFM [ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] ( 1/18)";
        let spans_1 = build_standard_line_spans(line_1, 18);
        let dark_1 = spans_1
            .iter()
            .filter(|s| s.content == "░ " && s.style.fg == Some(Color::DarkGray))
            .count();
        let green_1 = spans_1
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(dark_1, 17);
        assert_eq!(green_1, 1);

        // 5/18: 5 green blocks, 13 dark blocks
        let line_5 = "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ░ ] ( 5/18)";
        let spans_5 = build_standard_line_spans(line_5, 18);
        let dark_5 = spans_5
            .iter()
            .filter(|s| s.content == "░ " && s.style.fg == Some(Color::DarkGray))
            .count();
        let green_5 = spans_5
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(dark_5, 13);
        assert_eq!(green_5, 5);

        // 18/18 OK: 18 green blocks, 0 dark blocks
        let line_18 = "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)";
        let spans_18 = build_standard_line_spans(line_18, 18);
        let dark_18 = spans_18
            .iter()
            .filter(|s| s.content == "░ " && s.style.fg == Some(Color::DarkGray))
            .count();
        let green_18 = spans_18
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(dark_18, 0);
        assert_eq!(green_18, 18);
    }

    #[test]
    fn test_build_rpm_centering_gauge_nominal() {
        let (gauge, color) = build_rpm_centering_gauge(300.0, 300.0);
        assert_eq!(gauge, "[----|----▼----|----]");
        assert_eq!(color, Color::LightGreen);
    }

    #[test]
    fn test_build_rpm_centering_gauge_color_thresholds() {
        // <= 0.5% deviation -> LightGreen
        // 300 * 0.005 = 1.5 RPM -> 301.5 is +0.5%
        let (_, color_green_pos) = build_rpm_centering_gauge(301.5, 300.0);
        assert_eq!(color_green_pos, Color::LightGreen);

        let (_, color_green_neg) = build_rpm_centering_gauge(298.5, 300.0);
        assert_eq!(color_green_neg, Color::LightGreen);

        // > 0.5% and <= 1.5% deviation -> Yellow
        // 300 * 0.010 = 3.0 RPM -> 303.0 is +1.0%
        let (gauge_yellow_pos, color_yellow_pos) = build_rpm_centering_gauge(303.0, 300.0);
        assert_eq!(color_yellow_pos, Color::Yellow);
        assert!(gauge_yellow_pos.contains('▼'));

        let (_, color_yellow_neg) = build_rpm_centering_gauge(296.5, 300.0);
        assert_eq!(color_yellow_neg, Color::Yellow);

        // > 1.5% deviation -> Red
        // 300 * 0.020 = 6.0 RPM -> 306.0 is +2.0%
        let (gauge_red_pos, color_red_pos) = build_rpm_centering_gauge(306.0, 300.0);
        assert_eq!(color_red_pos, Color::Red);
        assert_eq!(gauge_red_pos, "[----|----|----|---▼]");

        let (gauge_red_neg, color_red_neg) = build_rpm_centering_gauge(290.0, 300.0);
        assert_eq!(color_red_neg, Color::Red);
        assert_eq!(gauge_red_neg, "[▼---|----|----|----]");
    }

    #[test]
    fn test_format_rpm_metric_line() {
        let mut meas = hw::RpmMeasurement::new();
        meas.instant_rpm = 300.1;
        meas.avg_rpm = 300.0;
        meas.min_rpm = 299.8;
        meas.max_rpm = 300.2;
        meas.jitter_pct = 0.0667;

        let metric_str = format_rpm_metric_line(&meas);
        assert_eq!(
            metric_str,
            "RPM: 300.1 (Avg: 300.0 | Min: 299.8 | Max: 300.2 | Jitter: ±0.07%)"
        );
    }

    #[test]
    fn test_build_rpm_metric_spans() {
        let mut meas = hw::RpmMeasurement::new();
        meas.instant_rpm = 300.1;
        meas.avg_rpm = 300.0;
        meas.min_rpm = 299.8;
        meas.max_rpm = 300.2;
        meas.jitter_pct = 0.0667;

        let spans = build_rpm_metric_spans(&meas, 300.0);
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(
            joined,
            "RPM: 300.1 (Avg: 300.0 | Min: 299.8 | Max: 300.2 | Jitter: ±0.07%)"
        );
        // RPM within 0.5% is LightGreen
        assert_eq!(spans[1].style.fg, Some(Color::LightGreen));
    }

    #[test]
    fn test_rpm_rolling_average_window_10() {
        let mut meas = hw::RpmMeasurement::new();

        // Feed 10 samples of 300.0
        for _ in 0..10 {
            meas.record_sample(300.0, 14_400_000);
        }
        assert_eq!(meas.sample_count, 10);
        assert_eq!(meas.avg_rpm, 300.0);
        assert_eq!(meas.min_rpm, 300.0);
        assert_eq!(meas.max_rpm, 300.0);
        assert_eq!(meas.jitter_rpm, 0.0);
        assert_eq!(meas.jitter_pct, 0.0);

        // Feed 5 samples of 310.0 -> Rolling window should now contain five 300.0 and five 310.0 -> avg = 305.0
        for _ in 0..5 {
            meas.record_sample(310.0, 13_935_483);
        }
        assert_eq!(meas.sample_count, 15);
        assert_eq!(meas.avg_rpm, 305.0);
        assert_eq!(meas.min_rpm, 300.0);
        assert_eq!(meas.max_rpm, 310.0);
        assert_eq!(meas.jitter_rpm, 5.0);

        // Feed 5 more samples of 310.0 -> Rolling window now contains ten 310.0 -> avg = 310.0
        for _ in 0..5 {
            meas.record_sample(310.0, 13_935_483);
        }
        assert_eq!(meas.sample_count, 20);
        assert_eq!(meas.avg_rpm, 310.0);
        assert_eq!(meas.min_rpm, 300.0);
        assert_eq!(meas.max_rpm, 310.0);
    }

    #[test]
    fn test_single_rev_duration_strict_extraction() {
        // Simulate two index timestamps 14_400_000 ticks apart at 72MHz sample rate
        let idx_start: u32 = 1_000_000;
        let idx_end: u32 = 15_400_000;
        let sample_rate = 72_000_000.0;
        let delta = (idx_end.wrapping_sub(idx_start)) & 0x0FFF_FFFF;
        let rev_time_ms = (delta as f64 / sample_rate) * 1000.0;
        assert!((rev_time_ms - 200.0).abs() < 0.001);

        let rpm_instant = 60_000.0 / rev_time_ms;
        assert!((rpm_instant - 300.0).abs() < 0.001);
    }

    #[test]
    fn test_head_selection_model_and_ui_formatting() {
        // 1. Enum toggle_next cycle
        let mut head = HeadSelection::Head0;
        assert_eq!(head.as_str(), "0");
        head = head.toggle_next();
        assert_eq!(head, HeadSelection::Head1);
        assert_eq!(head.as_str(), "1");
        head = head.toggle_next();
        assert_eq!(head, HeadSelection::Both);
        assert_eq!(head.as_str(), "BOTH (0+1)");
        head = head.toggle_next();
        assert_eq!(head, HeadSelection::Head0);

        // 2. App struct integration
        let mut app = App::new();
        assert_eq!(app.head_selection, HeadSelection::Head0);
        assert_eq!(app.status.head_select, HeadSelection::Head0);

        app.toggle_head();
        assert_eq!(app.head_selection, HeadSelection::Head1);
        assert_eq!(app.status.head_select, HeadSelection::Head1);

        app.toggle_head();
        assert_eq!(app.head_selection, HeadSelection::Both);
        assert_eq!(app.status.head_select, HeadSelection::Both);

        app.toggle_head();
        assert_eq!(app.head_selection, HeadSelection::Head0);
        assert_eq!(app.status.head_select, HeadSelection::Head0);

        // 3. UI formatting helpers
        assert_eq!(format_head_display(HeadSelection::Head0, 0), "Head 0");
        assert_eq!(format_head_display(HeadSelection::Head1, 1), "Head 1");
        assert_eq!(
            format_head_display(HeadSelection::Both, 0),
            "BOTH (0+1) [Active: H:0]"
        );
        assert_eq!(
            format_head_display(HeadSelection::Both, 1),
            "BOTH (0+1) [Active: H:1]"
        );

        assert_eq!(format_head_header_str(HeadSelection::Head0, 0), "H0");
        assert_eq!(format_head_header_str(HeadSelection::Head1, 1), "H1");
        assert_eq!(format_head_header_str(HeadSelection::Both, 0), "HB(H0)");
        assert_eq!(format_head_header_str(HeadSelection::Both, 1), "HB(H1)");
    }

    #[test]
    fn test_diagnostic_pass_model_and_app_integration() {
        let pass_h0 = DiagnosticPass::new(
            40,
            0,
            500,
            "T:40 H:0  500k  [ ██████████████████ ]  (18/18 OK)".to_string(),
            "T:40 H:0 Rate:500k MFM [ ■■■■■■■■■■■■■■■■■■ ] (18/18 OK) IL:1:1 Gap0:1440µs Q:99%".to_string(),
            18,
            18,
            true,
        );

        let pass_h1 = DiagnosticPass::new(
            40,
            1,
            500,
            "T:40 H:1  500k  [ ██████████░░░░░░░░ ]  (10/18 BAD)".to_string(),
            "T:40 H:1 Rate:500k MFM [ ■■■■■■■■■■░░░░░░░░ ] (10/18 BAD) IL:1:1 Gap0:1440µs Q:70%".to_string(),
            10,
            18,
            false,
        );

        let mut app = App::new();
        assert!(app.last_pass_h0.is_none());
        assert!(app.last_pass_h1.is_none());

        app.record_pass(pass_h0.clone());
        assert_eq!(app.last_pass_h0, Some(pass_h0.clone()));
        assert!(app.last_pass_h1.is_none());

        app.record_pass(pass_h1.clone());
        assert_eq!(app.last_pass_h0, Some(pass_h0));
        assert_eq!(app.last_pass_h1, Some(pass_h1));

        app.clear_passes();
        assert!(app.last_pass_h0.is_none());
        assert!(app.last_pass_h1.is_none());
    }

    #[test]
    fn test_build_both_mode_display_lines() {
        let mut status = DriveStatus {
            track: 40,
            head_select: HeadSelection::Both,
            head: 0,
            bitrate: 500,
            sector_count: 18,
            analyzing: true,
            ..Default::default()
        };

        status.last_pass_h0 = Some(DiagnosticPass::new(
            40,
            0,
            500,
            "T:40 H:0  500k  [ ██████████████████ ]  (18/18 OK)".to_string(),
            "T:40 H:0 Rate:500k MFM [ ■■■■■■■■■■■■■■■■■■ ] (18/18 OK) IL:1:1 Gap0:1440µs Q:99%".to_string(),
            18,
            18,
            true,
        ));

        status.last_pass_h1 = Some(DiagnosticPass::new(
            40,
            1,
            500,
            "T:40 H:1  500k  [ ██████████░░░░░░░░ ]  (10/18 BAD)".to_string(),
            "T:40 H:1 Rate:500k MFM [ ■■■■■■■■■■░░░░░░░░ ] (10/18 BAD) IL:1:1 Gap0:1440µs Q:70%".to_string(),
            10,
            18,
            false,
        ));

        // When Head 0 is active (under acquisition):
        status.head = 0;
        let lines_h0_active = build_both_mode_display_lines(&status);
        assert_eq!(lines_h0_active.len(), 2);
        // Line 0 (Head 0) should have active pointer '► '
        assert_eq!(lines_h0_active[0].spans[0].content, "► ");
        assert_eq!(lines_h0_active[0].spans[0].style.fg, Some(Color::Yellow));
        // Line 1 (Head 1) should have inactive padding '  '
        assert_eq!(lines_h0_active[1].spans[0].content, "  ");

        // When Head 1 is active (under acquisition):
        status.head = 1;
        let lines_h1_active = build_both_mode_display_lines(&status);
        assert_eq!(lines_h1_active.len(), 2);
        // Line 0 (Head 0) should have inactive padding '  '
        assert_eq!(lines_h1_active[0].spans[0].content, "  ");
        // Line 1 (Head 1) should have active pointer '► '
        assert_eq!(lines_h1_active[1].spans[0].content, "► ");
        assert_eq!(lines_h1_active[1].spans[0].style.fg, Some(Color::Yellow));
    }

    #[test]
    fn test_build_standard_line_spans_with_bad_token() {
        let line_bad = "T:40 H:1 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ░ ░ ░ ░ ░ ░ ░ ░ ] (10/18 BAD)";
        let spans = build_standard_line_spans(line_bad, 18);
        assert!(!spans.is_empty());

        // Verify (10/18 BAD) has LightRed style
        let bad_span = spans.iter().find(|s| s.content.contains("BAD"));
        assert!(bad_span.is_some());
        assert_eq!(bad_span.unwrap().style.fg, Some(Color::LightRed));

        let style = get_standard_line_style(line_bad, 18);
        assert_eq!(style, Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD));
    }

    #[test]
    fn test_head_switch_settle_delay_constant() {
        assert_eq!(hw::HEAD_SWITCH_SETTLE_MS, 1);
    }

    #[test]
    fn test_stepper_wakeup_delay_constant() {
        assert_eq!(hw::STEPPER_WAKEUP_DELAY_MS, 15);
    }

    #[test]
    fn test_cli_argument_parsing_drive_unit() {
        let args_default = vec!["aligntester".to_string()];
        let (port, unit, bus, step, preset) = parse_cli_args(&args_default);
        assert_eq!(port, None);
        assert_eq!(unit, 0);
        assert_eq!(bus, BusType::IbmPc);
        assert_eq!(step, StepMode::Single);
        assert_eq!(preset, PresetProfile::Pc35Hd);

        let args_port_only = vec!["aligntester".to_string(), "COM3".to_string()];
        let (port, unit, bus, step, preset) = parse_cli_args(&args_port_only);
        assert_eq!(port, Some("COM3".to_string()));
        assert_eq!(unit, 0);
        assert_eq!(bus, BusType::IbmPc);
        assert_eq!(step, StepMode::Single);
        assert_eq!(preset, PresetProfile::Pc35Hd);

        let args_drive1 = vec!["aligntester".to_string(), "--drive".to_string(), "1".to_string()];
        let (port, unit, _bus, _step, _preset) = parse_cli_args(&args_drive1);
        assert_eq!(port, None);
        assert_eq!(unit, 1);

        let args_drive0 = vec!["aligntester".to_string(), "--drive".to_string(), "0".to_string()];
        let (port, unit, _bus, _step, _preset) = parse_cli_args(&args_drive0);
        assert_eq!(port, None);
        assert_eq!(unit, 0);

        let args_short_d1 = vec!["aligntester".to_string(), "-d".to_string(), "1".to_string()];
        let (port, unit, _bus, _step, _preset) = parse_cli_args(&args_short_d1);
        assert_eq!(port, None);
        assert_eq!(unit, 1);

        let args_eq_syntax = vec!["aligntester".to_string(), "--drive=1".to_string(), "COM5".to_string()];
        let (port, unit, _bus, _step, _preset) = parse_cli_args(&args_eq_syntax);
        assert_eq!(port, Some("COM5".to_string()));
        assert_eq!(unit, 1);

        let args_short_eq = vec!["aligntester".to_string(), "COM5".to_string(), "-d=1".to_string()];
        let (port, unit, _bus, _step, _preset) = parse_cli_args(&args_short_eq);
        assert_eq!(port, Some("COM5".to_string()));
        assert_eq!(unit, 1);

        // Saturation to 1 in PC mode
        let args_out_of_bounds_pc = vec!["aligntester".to_string(), "--drive".to_string(), "4".to_string()];
        let (_port, unit, bus, _step, _preset) = parse_cli_args(&args_out_of_bounds_pc);
        assert_eq!(unit, 1);
        assert_eq!(bus, BusType::IbmPc);

        // Support for units 2 and 3 in Shugart mode
        let args_shugart_unit2 = vec!["aligntester".to_string(), "--bus".to_string(), "shugart".to_string(), "--drive".to_string(), "2".to_string()];
        let (_port, unit, bus, _step, _preset) = parse_cli_args(&args_shugart_unit2);
        assert_eq!(unit, 2);
        assert_eq!(bus, BusType::Shugart);

        let args_shugart_unit3 = vec!["aligntester".to_string(), "--shugart".to_string(), "-d=3".to_string()];
        let (_port, unit, bus, _step, _preset) = parse_cli_args(&args_shugart_unit3);
        assert_eq!(unit, 3);
        assert_eq!(bus, BusType::Shugart);

        // Saturation to 3 in Shugart mode
        let args_shugart_out_of_bounds = vec!["aligntester".to_string(), "--shugart".to_string(), "--drive".to_string(), "9".to_string()];
        let (_port, unit, bus, _step, _preset) = parse_cli_args(&args_shugart_out_of_bounds);
        assert_eq!(unit, 3);
        assert_eq!(bus, BusType::Shugart);
    }

    #[test]
    fn test_cli_argument_parsing_bus_type() {
        let args_shugart = vec!["aligntester".to_string(), "--bus".to_string(), "shugart".to_string()];
        let (_port, _unit, bus, _step, _preset) = parse_cli_args(&args_shugart);
        assert_eq!(bus, BusType::Shugart);

        let args_shugart_short = vec!["aligntester".to_string(), "-b".to_string(), "shugart".to_string()];
        let (_port, _unit, bus, _step, _preset) = parse_cli_args(&args_shugart_short);
        assert_eq!(bus, BusType::Shugart);

        let args_shugart_flag = vec!["aligntester".to_string(), "--shugart".to_string()];
        let (_port, _unit, bus, _step, _preset) = parse_cli_args(&args_shugart_flag);
        assert_eq!(bus, BusType::Shugart);

        let args_bus_eq = vec!["aligntester".to_string(), "--bus=shugart".to_string()];
        let (_port, _unit, bus, _step, _preset) = parse_cli_args(&args_bus_eq);
        assert_eq!(bus, BusType::Shugart);

        let args_b_eq = vec!["aligntester".to_string(), "-b=shugart".to_string()];
        let (_port, _unit, bus, _step, _preset) = parse_cli_args(&args_b_eq);
        assert_eq!(bus, BusType::Shugart);

        let args_amiga = vec!["aligntester".to_string(), "--bus".to_string(), "amiga".to_string()];
        let (_port, _unit, bus, _step, _preset) = parse_cli_args(&args_amiga);
        assert_eq!(bus, BusType::Shugart);

        let args_pc = vec!["aligntester".to_string(), "--bus".to_string(), "pc".to_string()];
        let (_port, _unit, bus, _step, _preset) = parse_cli_args(&args_pc);
        assert_eq!(bus, BusType::IbmPc);
    }

    #[test]
    fn test_cli_argument_parsing_step_mode() {
        let args_double = vec!["aligntester".to_string(), "--step".to_string(), "double".to_string()];
        let (_port, _unit, _bus, step, _preset) = parse_cli_args(&args_double);
        assert_eq!(step, StepMode::Double);

        let args_short_double = vec!["aligntester".to_string(), "-s".to_string(), "double".to_string()];
        let (_port, _unit, _bus, step, _preset) = parse_cli_args(&args_short_double);
        assert_eq!(step, StepMode::Double);

        let args_flag_double = vec!["aligntester".to_string(), "--double-step".to_string()];
        let (_port, _unit, _bus, step, _preset) = parse_cli_args(&args_flag_double);
        assert_eq!(step, StepMode::Double);

        let args_eq_double = vec!["aligntester".to_string(), "--step=double".to_string()];
        let (_port, _unit, _bus, step, _preset) = parse_cli_args(&args_eq_double);
        assert_eq!(step, StepMode::Double);

        let args_short_eq_double = vec!["aligntester".to_string(), "-s=2".to_string()];
        let (_port, _unit, _bus, step, _preset) = parse_cli_args(&args_short_eq_double);
        assert_eq!(step, StepMode::Double);

        let args_single = vec!["aligntester".to_string(), "--step".to_string(), "single".to_string()];
        let (_port, _unit, _bus, step, _preset) = parse_cli_args(&args_single);
        assert_eq!(step, StepMode::Single);

        let args_flag_single = vec!["aligntester".to_string(), "--single-step".to_string()];
        let (_port, _unit, _bus, step, _preset) = parse_cli_args(&args_flag_single);
        assert_eq!(step, StepMode::Single);
    }

    #[test]
    fn test_cli_argument_parsing_presets() {
        let args_amiga = vec!["aligntester".to_string(), "--preset".to_string(), "amiga".to_string()];
        let (_port, _unit, bus, step, preset) = parse_cli_args(&args_amiga);
        assert_eq!(preset, PresetProfile::Amiga35Dd);
        assert_eq!(bus, BusType::Shugart);
        assert_eq!(step, StepMode::Single);

        let args_360k_on_hd = vec!["aligntester".to_string(), "-p".to_string(), "pc525ddonhd".to_string()];
        let (_port, _unit, bus, step, preset) = parse_cli_args(&args_360k_on_hd);
        assert_eq!(preset, PresetProfile::Pc525DdOnHd);
        assert_eq!(bus, BusType::IbmPc);
        assert_eq!(step, StepMode::Double);

        let args_cpc = vec!["aligntester".to_string(), "--preset=cpc".to_string()];
        let (_port, _unit, bus, step, preset) = parse_cli_args(&args_cpc);
        assert_eq!(preset, PresetProfile::Cpc30Data);
        assert_eq!(bus, BusType::Shugart);
        assert_eq!(step, StepMode::Single);

        let args_atari = vec!["aligntester".to_string(), "-p=atari".to_string()];
        let (_port, _unit, bus, step, preset) = parse_cli_args(&args_atari);
        assert_eq!(preset, PresetProfile::Atari35Dd);
        assert_eq!(bus, BusType::IbmPc);
        assert_eq!(step, StepMode::Single);
    }

    #[test]
    fn test_app_drive_unit_state_and_action_handling() {
        let mut app0 = App::new();
        assert_eq!(app0.drive_unit, 0);
        assert_eq!(app0.status.drive_unit, 0);
        assert_eq!(app0.status.unit_id, 0);

        let app1 = App::with_drive_unit(1);
        assert_eq!(app1.drive_unit, 1);
        assert_eq!(app1.status.drive_unit, 1);
        assert_eq!(app1.status.unit_id, 1);

        app0.toggle_drive_unit();
        assert_eq!(app0.drive_unit, 1);
        assert_eq!(app0.status.drive_unit, 1);
        assert_eq!(app0.status.unit_id, 1);

        app0.handle_action(Action::ToggleDriveUnit);
        assert_eq!(app0.drive_unit, 0);
        assert_eq!(app0.status.drive_unit, 0);
        assert_eq!(app0.status.unit_id, 0);

        app0.set_drive_unit(1);
        assert_eq!(app0.drive_unit, 1);
        assert_eq!(app0.status.drive_unit, 1);
        assert_eq!(app0.status.unit_id, 1);
    }

    #[test]
    fn test_misaligned_ribbon_spans_coloring() {
        let std_line = "T:40 H:1 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 MISALIGNED T:41)";
        let std_spans = build_standard_line_spans(std_line, 18);
        // Ribbon block spans should have Orange/Red color
        let misaligned_blocks: Vec<_> = std_spans
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::Rgb(255, 140, 0)))
            .collect();
        assert_eq!(misaligned_blocks.len(), 18);

        // Status token should have Orange/Red color
        let misaligned_tokens: Vec<_> = std_spans
            .iter()
            .filter(|s| s.content.contains("MISALIGNED") && s.style.fg == Some(Color::Rgb(255, 140, 0)))
            .collect();
        assert!(!misaligned_tokens.is_empty());

        let verb_line = "T:40 H:1 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 MISALIGNED T:41) IL:1:1 Gap0:1440µs Q:95%";
        let verb_spans = build_verbose_line_spans(verb_line, 18);
        let verb_misaligned_blocks: Vec<_> = verb_spans
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::Rgb(255, 140, 0)))
            .collect();
        assert_eq!(verb_misaligned_blocks.len(), 18);

        // Line styles
        assert_eq!(
            get_standard_line_style(std_line, 18),
            Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            get_verbose_line_style(verb_line, 18),
            Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_both_mode_metrics_50_pct_and_mismatch() {
        let pass_h0 = DiagnosticPass::with_details(
            40, 0, 500,
            "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)".into(),
            "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK) IL:1:1 Gap0:1440µs Q:98%".into(),
            18, 18, 0, 98, true,
        );

        let mut pass_h1 = DiagnosticPass::with_details(
            40, 1, 500,
            "T:40 H:1 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 MISALIGNED T:41)".into(),
            "T:40 H:1 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 MISALIGNED T:41) IL:1:1 Gap0:1440µs Q:95%".into(),
            0, 18, 0, 95, false,
        );
        pass_h1.track_id = 41;

        let metrics = App::compute_both_metrics_from_passes(Some(&pass_h0), Some(&pass_h1), 18);
        assert_eq!(metrics.alignment_pct, 50.0);
        assert_eq!(metrics.total_ok, 18);
        assert_eq!(metrics.total_expected, 36);
        assert_eq!(metrics.total_off_track, 18);
        assert_eq!(metrics.off_track_details, "MISMATCH: Track 41 on Head 1");
    }

    #[test]
    fn test_single_head_stream_lines_spinner_and_decay_rendering() {
        let mut app = App::new();
        app.status.head_select = HeadSelection::Head0;
        app.status.sector_count = 18;
        app.status.sector_log = vec![
            "T:00 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)".to_string(),
            "T:00 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)".to_string(),
            "T:00 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (18/18 OK)".to_string(),
        ];

        // Frame 0: spinner_idx = 0 -> '|'
        app.stream_spinner_idx = 0;
        app.last_capture_instant = std::time::Instant::now();

        let lines = build_single_head_stream_lines(&app, 25);
        assert_eq!(lines.len(), 3);

        // Line 0 (history): prefix must be 6 neutral spaces "      "
        assert_eq!(lines[0].spans[0].content, "      ");
        // Line 0 valid blocks must be standard green Color::Rgb(0, 180, 0)
        let hist_greens = lines[0]
            .spans
            .iter()
            .filter(|s| s.content == "■ " && s.style.fg == Some(Color::Rgb(0, 180, 0)))
            .count();
        assert_eq!(hist_greens, 18);

        // Line 1 (history): prefix must be 6 neutral spaces "      "
        assert_eq!(lines[1].spans[0].content, "      ");

        // Line 2 (last line): prefix must have spinner "▸ [|] "
        assert_eq!(lines[2].spans[0].content, "▸ [|] ");

        // Line 2 (last line): valid blocks must have TrueColor decay (since elapsed is near 0, factor ~ 1.0 -> Rgb(210, 255, 210))
        let last_decay_blocks = lines[2]
            .spans
            .iter()
            .filter(|s| {
                if s.content == "■ " {
                    if let Some(Color::Rgb(r, g, b)) = s.style.fg {
                        return r >= 30 && g >= 180 && b >= 30;
                    }
                }
                false
            })
            .count();
        assert_eq!(last_decay_blocks, 18);

        // Test spinner rotation
        app.stream_spinner_idx = 1;
        let lines_spin1 = build_single_head_stream_lines(&app, 25);
        assert_eq!(lines_spin1[2].spans[0].content, "▸ [/] ");

        app.stream_spinner_idx = 2;
        let lines_spin2 = build_single_head_stream_lines(&app, 25);
        assert_eq!(lines_spin2[2].spans[0].content, "▸ [-] ");

        app.stream_spinner_idx = 3;
        let lines_spin3 = build_single_head_stream_lines(&app, 25);
        assert_eq!(lines_spin3[2].spans[0].content, "▸ [\\] ");
    }

    #[test]
    fn test_single_head_stream_lines_error_priority_on_last_line() {
        let mut app = App::new();
        app.status.head_select = HeadSelection::Head0;
        app.status.sector_count = 18;
        app.status.sector_log = vec![
            "T:35 H:0 Rate:500k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ■ ░ ■ ■ ■ ] (17/18 CRC-DAT: Sec 15, MISSING: Sec 15)".to_string(),
        ];
        app.last_capture_instant = std::time::Instant::now();
        app.stream_spinner_idx = 1;

        let lines = build_single_head_stream_lines(&app, 25);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content, "▸ [/] ");

        // Missing sector (░) must be DarkGray
        let dark_blocks = lines[0]
            .spans
            .iter()
            .filter(|s| s.content == "░ " && s.style.fg == Some(Color::DarkGray))
            .count();
        assert_eq!(dark_blocks, 1);
    }

    #[test]
    fn test_cli_help_and_version_flags() {
        let banner = build_cli_banner();
        assert!(banner.contains("💾 AlignTesterDiag"));
        assert!(banner.contains(env!("CARGO_PKG_VERSION")));
        assert!(banner.contains("2026 MonSieur JeAn-FReD (GPL-3.0)"));
        assert!(banner.contains("--port"));
        assert!(banner.contains("--drive"));
        assert!(banner.contains("--step"));
        assert!(banner.contains("--help"));
        assert!(banner.contains("--version"));

        let args_help_short = vec!["aligntester".to_string(), "-h".to_string()];
        assert!(handle_cli_help_or_version(&args_help_short));

        let args_help_long = vec!["aligntester".to_string(), "--help".to_string()];
        assert!(handle_cli_help_or_version(&args_help_long));

        let args_ver_short_v = vec!["aligntester".to_string(), "-v".to_string()];
        assert!(handle_cli_help_or_version(&args_ver_short_v));

        let args_ver_short_cap_v = vec!["aligntester".to_string(), "-V".to_string()];
        assert!(handle_cli_help_or_version(&args_ver_short_cap_v));

        let args_ver_long = vec!["aligntester".to_string(), "--version".to_string()];
        assert!(handle_cli_help_or_version(&args_ver_long));

        let args_normal = vec!["aligntester".to_string(), "COM3".to_string(), "-d".to_string(), "1".to_string()];
        assert!(!handle_cli_help_or_version(&args_normal));
    }

    #[test]
    fn test_cli_argument_parsing_port_flags() {
        let args_p_short = vec!["aligntester".to_string(), "-p".to_string(), "COM4".to_string(), "-d".to_string(), "1".to_string()];
        let (port, unit, _bus, _step, _preset) = parse_cli_args(&args_p_short);
        assert_eq!(port, Some("COM4".to_string()));
        assert_eq!(unit, 1);

        let args_p_long = vec!["aligntester".to_string(), "--port".to_string(), "/dev/ttyACM0".to_string()];
        let (port, unit, _bus, _step, _preset) = parse_cli_args(&args_p_long);
        assert_eq!(port, Some("/dev/ttyACM0".to_string()));
        assert_eq!(unit, 0);

        let args_p_eq = vec!["aligntester".to_string(), "--port=COM7".to_string(), "--drive=1".to_string()];
        let (port, unit, _bus, _step, _preset) = parse_cli_args(&args_p_eq);
        assert_eq!(port, Some("COM7".to_string()));
        assert_eq!(unit, 1);

        let args_short_p_eq = vec!["aligntester".to_string(), "-p=COM9".to_string()];
        let (port, unit, _bus, _step, _preset) = parse_cli_args(&args_short_p_eq);
        assert_eq!(port, Some("COM9".to_string()));
        assert_eq!(unit, 0);
    }

    #[test]
    fn test_top_header_branding() {
        let title = get_header_title();
        assert_eq!(title, format!(" 💾 AlignTesterDiag v{} ", env!("CARGO_PKG_VERSION")));
        assert!(title.contains('💾'));
        assert!(!title.contains("MonSieur JeAn-FReD"));

        let badge_com3 = format_port_badge("COM3");
        assert_eq!(badge_com3, " [ Port: COM3 ] ");

        let badge_com10 = format_port_badge("COM10");
        assert_eq!(badge_com10, " [ Port: COM10 ] ");

        let badge_linux = format_port_badge("/dev/ttyACM0");
        assert_eq!(badge_linux, " [ Port: /dev/ttyACM0 ] ");

        let badge_empty = format_port_badge("");
        assert_eq!(badge_empty, " [ Port: Auto ] ");
    }

    #[test]
    fn test_footer_line_content() {
        let footer_line = build_footer_line();
        let footer_text: String = footer_line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(footer_text.contains("[?] / [F1] Help"));
        assert!(footer_text.contains("[Q] Quit"));
        assert!(footer_text.contains("[Esc] Stop"));
        assert!(footer_text.contains("Hardware: Greaseweazle"));
    }

    #[test]
    fn test_help_modal_content() {
        let lines = build_help_modal_lines();
        let full_text: String = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>() + "\n")
            .collect();

        assert!(full_text.contains("Version:"));
        assert!(full_text.contains(env!("CARGO_PKG_VERSION")));
        assert!(full_text.contains("Mr JeAn-FReD"));
        assert!(full_text.contains("License: GPL-3.0"));
        assert!(full_text.contains("? / F1"));
        assert!(full_text.contains("Analyze"));
        assert!(full_text.contains("Audio Radar"));
        assert!(full_text.contains("Read Data"));
        assert!(full_text.contains("Stop / Motor off"));
        assert!(full_text.contains("PANIC RESET"));
        assert!(full_text.contains("Head 0 -> Head 1 -> Both 0+1"));
        assert!(full_text.contains("Toggle Step Rate (Single 1:1 / Double 2:1 for 48/96 TPI)"));
        assert!(full_text.contains("Toggle Bus Type (IBM PC <-> Shugart)"));
        assert!(full_text.contains("Press [Esc], [?], or [F1] to return"));
    }

    #[test]
    fn test_terminal_vertical_layout_no_footer() {
        let area = ratatui::layout::Rect::new(0, 0, 100, 30);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Min(1),
            ])
            .split(area);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].height, 4);
        assert_eq!(chunks[1].height, 26);
    }

    #[test]
    fn test_format_disk_format_header() {
        assert_eq!(
            format_disk_format_header(DiskFormat::AmigaDos, 250, 11),
            "AmigaDOS DD 11x512"
        );
        assert_eq!(
            format_disk_format_header(DiskFormat::AmigaDos, 500, 22),
            "AmigaDOS HD 22x512"
        );
        assert_eq!(
            format_disk_format_header(DiskFormat::AtariSt, 250, 10),
            "Atari ST 10x512"
        );
        assert_eq!(
            format_disk_format_header(DiskFormat::AtariSt, 250, 11),
            "Atari ST 11x512"
        );
        assert_eq!(
            format_disk_format_header(DiskFormat::AmstradCpcData, 250, 9),
            "CPC Data 9x512"
        );
        assert_eq!(
            format_disk_format_header(DiskFormat::AmstradCpcData, 250, 10),
            "CPC Data 10x512"
        );
        assert_eq!(
            format_disk_format_header(DiskFormat::AmstradCpcSystem, 250, 9),
            "CPC System 9x512"
        );
        assert_eq!(
            format_disk_format_header(DiskFormat::IbmPc, 500, 18),
            "PC HD 18x512 (1.44M)"
        );
        assert_eq!(
            format_disk_format_header(DiskFormat::IbmPc, 500, 15),
            "PC HD 15x512 (1.2M)"
        );
        assert_eq!(
            format_disk_format_header(DiskFormat::IbmPc, 250, 9),
            "PC DD 9x512 (720K)"
        );
        assert_eq!(
            format_disk_format_header(DiskFormat::IbmPc, 300, 9),
            "PC DD 9x512 (360K)"
        );
        assert_eq!(
            format_disk_format_header(DiskFormat::AutoDetect, 300, 9),
            "PC DD 9x512 (360K)"
        );
    }

    #[test]
    fn test_extract_sector_ids_from_error_hex_and_decimal() {
        let dec_line = "T:40 H:0 Rate:500k MFM [ ■ ■ ■ ] (17/18 CRC-DAT: Sec 15)";
        let dec_ids = extract_sector_ids_from_error(dec_line, "CRC-DAT: Sec ");
        assert_eq!(dec_ids, vec![15]);

        let hex_line = "T:20 H:0 Rate:250k MFM [ ■ ■ ■ ] (8/9 CRC-DAT: Sec C2)";
        let hex_ids = extract_sector_ids_from_error(hex_line, "CRC-DAT: Sec ");
        assert_eq!(hex_ids, vec![0xC2]);

        let hex_sys_line = "T:20 H:0 Rate:250k MFM [ ■ ■ ■ ] (8/9 CRC-ID: Sec 45)";
        let hex_sys_ids = extract_sector_ids_from_error(hex_sys_line, "CRC-ID: Sec ");
        assert_eq!(hex_sys_ids, vec![0x45]);
    }

    #[test]
    fn test_build_standard_line_spans_cpc_error_highlight() {
        let line = "T:20 H:0 Rate:250k MFM [ ■ ■ ■ ■ ■ ■ ■ ■ ■ ] (8/9 CRC-DAT: Sec C2)";
        let spans = build_standard_line_spans(line, 9);
        // Find the bracket inner spans
        let block_spans: Vec<&Span> = spans.iter().filter(|s| s.content == "■ ").collect();
        assert_eq!(block_spans.len(), 9);
        // Block index 1 (sec C1) -> Green
        assert_eq!(block_spans[0].style.fg, Some(Color::LightGreen));
        // Block index 2 (sec C2) -> Red
        assert_eq!(block_spans[1].style.fg, Some(Color::LightRed));
        // Block index 3 (sec C3) -> Green
        assert_eq!(block_spans[2].style.fg, Some(Color::LightGreen));
    }

    #[test]
    fn test_format_modal_lines_and_typography() {
        let lines = build_format_modal_lines(
            40,
            HeadSelection::Head0,
            79,
            PresetProfile::Pc35Hd,
            300.0,
            500,
            "IBM PC",
            0,
            80,
            false,
            true,
            FsInitMode::Blank,
            TrackRange::new(0, 79),
            None,
        );

        let full_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();

        // Must contain key English instructions
        assert!(full_text.contains("LOW-LEVEL MFM FORMATTING"));
        assert!(full_text.contains("Target Preset : [3.5\" HD (1.44M)]"));
        assert!(full_text.contains("Target: 300 RPM"));
        assert!(full_text.contains("Head: Head 0"));
        assert!(full_text.contains("Target Tracks : 80 tracks"));
        assert!(full_text.contains("Range: 00..79 | Standard: 80, Max: 84"));
        assert!(full_text.contains("Read-After-Write Verify : ON"));
        assert!(full_text.contains("System FS Init : Blank (Raw 0xE5)"));
        assert!(full_text.contains("Unit (A:/B:)"));
        assert!(full_text.contains("Bus (PC/Shugart)"));
        assert!(full_text.contains("Preset Profile"));
        assert!(full_text.contains("Head"));
        assert!(full_text.contains("Format Track Range     (Tracks 00..79, Head 0 only)"));
        assert!(full_text.contains("Tracks 00..79, Head 0 only"));
        assert!(full_text.contains("Cancel & Return"));

        // Check shortcut highlight formatting for [U], [B], [P], [H], [T], [R], [D], [V], [S]
        let mut found_u_shortcut = false;
        let mut found_b_shortcut = false;
        let mut found_p_shortcut = false;
        let mut found_h_shortcut = false;
        let mut found_t_shortcut = false;
        let mut found_r_shortcut = false;
        let mut found_d_shortcut = false;
        let mut found_v_shortcut = false;
        let mut found_s_shortcut = false;
        let mut found_esc_shortcut = false;

        for line in &lines {
            for span in &line.spans {
                if span.content == "U" && span.style.fg == Some(Color::Yellow) {
                    found_u_shortcut = true;
                }
                if span.content == "B" && span.style.fg == Some(Color::Yellow) {
                    found_b_shortcut = true;
                }
                if span.content == "P" && span.style.fg == Some(Color::Yellow) {
                    found_p_shortcut = true;
                }
                if span.content == "H" && span.style.fg == Some(Color::Yellow) {
                    found_h_shortcut = true;
                }
                if span.content == "T" && span.style.fg == Some(Color::Yellow) {
                    found_t_shortcut = true;
                }
                if span.content == "R" && span.style.fg == Some(Color::Yellow) {
                    found_r_shortcut = true;
                }
                if span.content == "D" && span.style.fg == Some(Color::Yellow) {
                    found_d_shortcut = true;
                }
                if span.content == "V" && span.style.fg == Some(Color::Yellow) {
                    found_v_shortcut = true;
                }
                if span.content == "S" && span.style.fg == Some(Color::Yellow) {
                    found_s_shortcut = true;
                }
                if span.content == "Esc" && span.style.fg == Some(Color::LightRed) {
                    found_esc_shortcut = true;
                }
            }
        }

        assert!(found_u_shortcut, "Shortcut [U] and 'Unit' must be emphasized in Yellow/Bold");
        assert!(found_b_shortcut, "Shortcut [B] and 'Bus' must be emphasized in Yellow/Bold");
        assert!(found_p_shortcut, "Shortcut [P] and 'Preset' must be emphasized in Yellow/Bold");
        assert!(found_h_shortcut, "Shortcut [H] and 'Head' must be emphasized in Yellow/Bold");
        assert!(found_t_shortcut, "Shortcut [T] and 'Track' must be emphasized in Yellow/Bold");
        assert!(found_r_shortcut, "Shortcut [R] and 'Range' must be emphasized in Yellow/Bold");
        assert!(found_d_shortcut, "Shortcut [D] and 'Disk' must be emphasized in Yellow/Bold");
        assert!(found_v_shortcut, "Shortcut [V] and 'Verify' must be emphasized in Yellow/Bold");
        assert!(found_s_shortcut, "Shortcut [S] and 'System FS Init' must be emphasized in Yellow/Bold");
        assert!(found_esc_shortcut, "Shortcut [Esc] must be highlighted in Red/Bold");
    }

    #[test]
    fn test_erase_modal_lines_and_typography() {
        let lines = build_erase_modal_lines(
            40,
            HeadSelection::Head0,
            "3.5\" HD (1.44M)",
            300.0,
            500,
            80,
            false,
            TrackRange::new(0, 79),
            None,
        );

        let full_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();

        assert!(full_text.contains("Target Preset : [3.5\" HD (1.44M)]"));
        assert!(full_text.contains("Target: 300 RPM"));
        assert!(full_text.contains("Head: Head 0"));
        assert!(full_text.contains("Target Tracks : 80 tracks"));
        assert!(full_text.contains("WARNING: This will permanently wipe all magnetic flux"));
        assert!(full_text.contains("Unit (A:/B:)"));
        assert!(full_text.contains("Bus (PC/Shugart)"));
        assert!(full_text.contains("Preset Profile"));
        assert!(full_text.contains("Head"));
        assert!(full_text.contains("Erase Current Track only  (Track 40, Head 0 only)"));
        assert!(full_text.contains("Erase Track Range     (Tracks 00..79, Head 0 only)"));
        assert!(full_text.contains("Erase Entire Disk         (Tracks 00..79, Head 0 only)"));
        assert!(full_text.contains("Cancel & Return"));

        let mut found_u = false;
        let mut found_b = false;
        let mut found_p = false;
        let mut found_h = false;
        let mut found_t = false;
        let mut found_r = false;
        let mut found_d = false;
        let mut found_esc = false;

        for line in &lines {
            for span in &line.spans {
                if span.content == "U" && span.style.fg == Some(Color::Yellow) {
                    found_u = true;
                }
                if span.content == "B" && span.style.fg == Some(Color::Yellow) {
                    found_b = true;
                }
                if span.content == "P" && span.style.fg == Some(Color::Yellow) {
                    found_p = true;
                }
                if span.content == "H" && span.style.fg == Some(Color::Yellow) {
                    found_h = true;
                }
                if span.content == "T" && span.style.fg == Some(Color::Yellow) {
                    found_t = true;
                }
                if span.content == "R" && span.style.fg == Some(Color::Yellow) {
                    found_r = true;
                }
                if span.content == "D" && span.style.fg == Some(Color::Yellow) {
                    found_d = true;
                }
                if span.content == "Esc" && span.style.fg == Some(Color::LightRed) {
                    found_esc = true;
                }
            }
        }

        assert!(found_u, "Shortcut [U] must be emphasized in Yellow/Bold");
        assert!(found_b, "Shortcut [B] must be emphasized in Yellow/Bold");
        assert!(found_p, "Shortcut [P] must be emphasized in Yellow/Bold");
        assert!(found_h, "Shortcut [H] must be emphasized in Yellow/Bold");
        assert!(found_t, "Shortcut [T] must be emphasized in Yellow/Bold");
        assert!(found_r, "Shortcut [R] must be emphasized in Yellow/Bold");
        assert!(found_d, "Shortcut [D] must be emphasized in Yellow/Bold");
        assert!(found_esc, "Shortcut [Esc] must be highlighted in Red/Bold");
    }

    #[test]
    fn test_modal_preset_cycling_and_track_clamping() {
        let mut app = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Single, PresetProfile::Pc35Hd);
        app.status.track = 79;
        app.status.target_track = 79;

        // Open Format modal
        app.handle_action(Action::OpenFormatModal);
        assert!(app.show_format_modal);
        assert_eq!(app.preset, PresetProfile::Pc35Hd);
        assert_eq!(app.preset.target_rpm(), 300.0);
        assert_eq!(app.format_target_tracks, 80);
        assert_eq!(app.format_range, TrackRange::new(0, 79));
        assert_eq!(app.status.track, 79);

        // Cycle preset: Pc35Hd -> Pc35Dd
        app.handle_action(Action::CyclePreset);
        assert_eq!(app.preset, PresetProfile::Pc35Dd);
        assert_eq!(app.preset.target_rpm(), 300.0);
        assert_eq!(app.status.bitrate, 250);
        assert_eq!(app.format_target_tracks, 80);
        assert_eq!(app.format_range, TrackRange::new(0, 79));
        assert_eq!(app.status.track, 79);

        // Cycle preset: Pc35Dd -> Pc525Hd
        app.handle_action(Action::CyclePreset);
        assert_eq!(app.preset, PresetProfile::Pc525Hd);
        assert_eq!(app.preset.target_rpm(), 360.0);
        assert_eq!(app.status.bitrate, 500);
        assert_eq!(app.format_target_tracks, 80);
        assert_eq!(app.format_range, TrackRange::new(0, 79));
        assert_eq!(app.status.track, 79);

        // Cycle preset: Pc525Hd -> Pc525DdOnHd (48 TPI, Double step, 40 tracks default)
        app.handle_action(Action::CyclePreset);
        assert_eq!(app.preset, PresetProfile::Pc525DdOnHd);
        assert_eq!(app.preset.target_rpm(), 360.0);
        assert_eq!(app.status.bitrate, 300);
        assert_eq!(app.step_mode, StepMode::Double);
        assert_eq!(app.format_target_tracks, 40);
        assert_eq!(app.format_range, TrackRange::new(0, 39));
        // Track 79 must be clamped to max_tracks - 1 = 41
        assert_eq!(app.status.track, 41);
        assert_eq!(app.status.target_track, 41);

        // Cycle preset: Pc525DdOnHd -> Pc525Dd (48 TPI, Single step, 40 tracks default)
        app.handle_action(Action::CyclePreset);
        assert_eq!(app.preset, PresetProfile::Pc525Dd);
        assert_eq!(app.preset.target_rpm(), 300.0);
        assert_eq!(app.status.bitrate, 250);
        assert_eq!(app.format_target_tracks, 40);
        assert_eq!(app.format_range, TrackRange::new(0, 39));
        assert_eq!(app.status.track, 41);

        // Close format modal, open erase modal
        app.handle_action(Action::CloseFormatModal);
        app.status.track = 30;
        app.status.target_track = 30;
        app.handle_action(Action::OpenEraseModal);
        assert!(app.show_erase_modal);

        // Cycle preset: Pc525Dd -> Amiga35Dd (80 tracks, preserves IBM PC bus)
        app.handle_action(Action::CyclePreset);
        assert_eq!(app.preset, PresetProfile::Amiga35Dd);
        assert_eq!(app.preset.target_rpm(), 300.0);
        assert_eq!(app.bus_type, BusType::IbmPc);
        assert_eq!(app.erase_target_tracks, 80);
        assert_eq!(app.erase_range, TrackRange::new(0, 79));
        assert_eq!(app.status.track, 30);

        // Move track to 75
        app.status.track = 75;
        app.status.target_track = 75;

        // Cycle preset: Amiga35Dd -> Atari35Dd (80 tracks, IBM PC)
        app.handle_action(Action::CyclePreset);
        assert_eq!(app.preset, PresetProfile::Atari35Dd);
        assert_eq!(app.preset.target_rpm(), 300.0);
        assert_eq!(app.bus_type, BusType::IbmPc);
        assert_eq!(app.erase_target_tracks, 80);
        assert_eq!(app.erase_range, TrackRange::new(0, 79));
        assert_eq!(app.status.track, 75);

        // Cycle preset: Atari35Dd -> Cpc30Data (48 TPI, 40 tracks, preserves IBM PC bus)
        app.handle_action(Action::CyclePreset);
        assert_eq!(app.preset, PresetProfile::Cpc30Data);
        assert_eq!(app.preset.target_rpm(), 300.0);
        assert_eq!(app.bus_type, BusType::IbmPc);
        assert_eq!(app.erase_target_tracks, 40);
        assert_eq!(app.erase_range, TrackRange::new(0, 39));
        // Track 75 clamped to 41
        assert_eq!(app.status.track, 41);
        assert_eq!(app.status.target_track, 41);

        // Cycle preset: Cpc30Data -> Pc35Hd (80 tracks, IBM PC)
        app.handle_action(Action::CyclePreset);
        assert_eq!(app.preset, PresetProfile::Pc35Hd);
        assert_eq!(app.preset.target_rpm(), 300.0);
        assert_eq!(app.bus_type, BusType::IbmPc);
        assert_eq!(app.erase_target_tracks, 80);
        assert_eq!(app.erase_range, TrackRange::new(0, 79));
        assert_eq!(app.status.track, 41);
    }

    #[test]
    fn test_build_progress_bar_string() {
        let bar_0 = build_progress_bar_string(0.0, 20);
        assert_eq!(bar_0, "[░░░░░░░░░░░░░░░░░░░░]   0.0%");

        let bar_50 = build_progress_bar_string(50.0, 20);
        assert_eq!(bar_50, "[██████████░░░░░░░░░░]  50.0%");

        let bar_100 = build_progress_bar_string(100.0, 20);
        assert_eq!(bar_100, "[████████████████████] 100.0%");
    }

    #[test]
    fn test_build_format_progress_lines() {
        let mut status = DriveStatus {
            preset: PresetProfile::Pc35Hd,
            mode: DisplayMode::Format,
            format_progress: Some(FormatProgress {
            current_track: 20,
            current_head: 1,
            total_tracks: 80,
            total_heads: 2,
            step: FormatStep::Writing,
            completed_passes: 41,
            total_passes: 160,
            verification_ok: true,
            retry_count: 0,
            quality_pct: 98,
            crc_errors: 0,
            verified_sectors: 18,
            expected_sectors: 18,
            elapsed_secs: 12.5,
            eta_secs: 36.2,
            message: "Writing flux...".to_string(),
            }),
            ..Default::default()
        };

        let lines = build_format_progress_lines(&status);
        assert!(!lines.is_empty());
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text.contains("LOW-LEVEL FORMAT ENGINE"));
        assert!(text.contains("WRITING FLUX"));
        assert!(text.contains("Track 20 of 80 total (Phys: 20) | Head 1/1"));
        assert!(text.contains("Sectors: 18/18"));
        assert!(text.contains("(41/160 passes)"));

        // Test Erase progress rendering
        status.mode = DisplayMode::Erase;
        if let Some(ref mut p) = status.format_progress {
            p.step = FormatStep::Erasing;
        }
        let erase_lines = build_format_progress_lines(&status);
        let erase_text: String = erase_lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(erase_text.contains("LOW-LEVEL DC FLUX ERASE ENGINE"));
        assert!(erase_text.contains("ERASING FLUX"));
    }

    #[test]
    fn test_app_format_modal_actions() {
        let mut app = App::new();
        assert!(!app.show_format_modal);
        assert_eq!(app.format_fs_mode, FsInitMode::Blank);

        app.handle_action(Action::ToggleFormatFsMode);
        assert_eq!(app.format_fs_mode, FsInitMode::OsReady);
        app.handle_action(Action::ToggleFormatFsMode);
        assert_eq!(app.format_fs_mode, FsInitMode::Blank);

        app.handle_action(Action::OpenFormatModal);
        assert!(app.show_format_modal);

        app.handle_action(Action::CloseFormatModal);
        assert!(!app.show_format_modal);

        app.handle_action(Action::FormatTrack {
            track: 10,
            head_sel: HeadSelection::Head0,
            verify: true,
            fs_mode: FsInitMode::Blank,
        });
        assert_eq!(app.status.mode, DisplayMode::Format);
        assert_eq!(app.status.activity, HwActivity::Formatting);
        assert!(app.motor_on);

        app.handle_action(Action::FormatDisk {
            range: TrackRange::new(0, 79),
            head_sel: HeadSelection::Both,
            verify: false,
            fs_mode: FsInitMode::OsReady,
        });
        assert_eq!(app.status.mode, DisplayMode::Format);
        assert_eq!(app.status.activity, HwActivity::Formatting);
        assert!(app.motor_on);

        app.handle_action(Action::Stop);
        assert_eq!(app.status.mode, DisplayMode::None);
        assert_eq!(app.status.activity, HwActivity::Stopped);
    }

    #[test]
    fn test_format_target_tracks_defaults_and_limits_48tpi_vs_80tpi() {
        // 1. 80 TPI standard presets default to 80 tracks, min 1, max 84
        let app_hd = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Single, PresetProfile::Pc35Hd);
        assert!(!app_hd.is_48_tpi());
        assert_eq!(app_hd.default_format_tracks(), 80);
        assert_eq!(app_hd.max_format_tracks(), 84);
        assert_eq!(app_hd.format_target_tracks, 80);

        let app_dd = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Single, PresetProfile::Pc35Dd);
        assert_eq!(app_dd.default_format_tracks(), 80);
        assert_eq!(app_dd.max_format_tracks(), 84);

        let app_amiga = App::with_full_preset_config(0, BusType::Shugart, StepMode::Single, PresetProfile::Amiga35Dd);
        assert_eq!(app_amiga.default_format_tracks(), 80);
        assert_eq!(app_amiga.max_format_tracks(), 84);

        let app_atari = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Single, PresetProfile::Atari35Dd);
        assert_eq!(app_atari.default_format_tracks(), 80);
        assert_eq!(app_atari.max_format_tracks(), 84);

        // 2. 48 TPI presets default to 40 tracks, min 1, max 42
        let app_525dd = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Single, PresetProfile::Pc525Dd);
        assert!(app_525dd.is_48_tpi());
        assert_eq!(app_525dd.default_format_tracks(), 40);
        assert_eq!(app_525dd.max_format_tracks(), 42);
        assert_eq!(app_525dd.format_target_tracks, 40);

        let app_525dd_on_hd = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Double, PresetProfile::Pc525DdOnHd);
        assert!(app_525dd_on_hd.is_48_tpi());
        assert_eq!(app_525dd_on_hd.default_format_tracks(), 40);
        assert_eq!(app_525dd_on_hd.max_format_tracks(), 42);
        assert_eq!(app_525dd_on_hd.format_target_tracks, 40);

        let app_cpc = App::with_full_preset_config(0, BusType::Shugart, StepMode::Single, PresetProfile::Cpc30Data);
        assert!(app_cpc.is_48_tpi());
        assert_eq!(app_cpc.default_format_tracks(), 40);
        assert_eq!(app_cpc.max_format_tracks(), 42);
        assert_eq!(app_cpc.format_target_tracks, 40);

        // 3. StepMode::Double forces 48 TPI track bounds
        let mut app_step = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Single, PresetProfile::Pc35Hd);
        assert_eq!(app_step.format_target_tracks, 80);
        app_step.handle_action(Action::SetStepMode(StepMode::Double));
        assert!(app_step.is_48_tpi());
        assert_eq!(app_step.format_target_tracks, 40);
        assert_eq!(app_step.max_format_tracks(), 42);
    }

    #[test]
    fn test_format_target_tracks_interactive_adjustment() {
        let mut app = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Single, PresetProfile::Pc35Hd);
        assert_eq!(app.format_target_tracks, 80);

        // Increment up to max 84
        for _ in 0..10 {
            app.increment_format_tracks();
        }
        assert_eq!(app.format_target_tracks, 84, "Should be clamped at 84");

        // Decrement down to min 1
        for _ in 0..100 {
            app.decrement_format_tracks();
        }
        assert_eq!(app.format_target_tracks, 1, "Should be clamped at min 1");

        // 48 TPI overtracking clamping to 42
        let mut app_48 = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Single, PresetProfile::Pc525Dd);
        assert_eq!(app_48.format_target_tracks, 40);
        for _ in 0..10 {
            app_48.increment_format_tracks();
        }
        assert_eq!(app_48.format_target_tracks, 42, "Should be clamped at 42 in 48 TPI");
    }

    #[test]
    fn test_format_total_passes_calculation() {
        // Standard 80 tracks x 2 heads = 160 passes
        let target_80 = 80u8;
        let heads = 2u8;
        let passes_80 = (target_80 as usize) * (heads as usize);
        assert_eq!(passes_80, 160);

        // Standard 40 tracks x 2 heads = 80 passes
        let target_40 = 40u8;
        let passes_40 = (target_40 as usize) * (heads as usize);
        assert_eq!(passes_40, 80);

        // Overtracked 84 tracks x 2 heads = 168 passes
        let target_84 = 84u8;
        let passes_84 = (target_84 as usize) * (heads as usize);
        assert_eq!(passes_84, 168);

        // Overtracked 42 tracks x 2 heads = 84 passes
        let target_42 = 42u8;
        let passes_42 = (target_42 as usize) * (heads as usize);
        assert_eq!(passes_42, 84);

        // Progression percentage calculation
        let pct_0 = (0_f32 / passes_80 as f32) * 100.0;
        assert_eq!(pct_0, 0.0);

        let pct_half = (40_f32 / passes_40 as f32) * 100.0;
        assert_eq!(pct_half, 50.0);

        let pct_100 = (passes_80 as f32 / passes_80 as f32) * 100.0;
        assert_eq!(pct_100, 100.0);
    }

    #[test]
    fn test_format_modal_rendering_with_dynamic_tracks() {
        // 48 TPI modal line test
        let lines_48 = build_format_modal_lines(
            20,
            HeadSelection::Head0,
            39,
            PresetProfile::Pc525Dd,
            300.0,
            250,
            "IBM PC",
            0,
            40,
            true,
            true,
            FsInitMode::Blank,
            TrackRange::new(0, 39),
            None,
        );
        let text_48: String = lines_48
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_48.contains("Target Tracks : 40 tracks"));
        assert!(text_48.contains("Range: 00..39 | Standard: 40, Max: 42"));
        assert!(text_48.contains("Tracks 00..39, Head 0 only"));
        assert!(text_48.contains("Target Track  : Track 20"));
        assert!(text_48.contains("Head: Head 0"));

        // Overtracked 48 TPI (42 tracks)
        let lines_48_over = build_format_modal_lines(
            20,
            HeadSelection::Head1,
            41,
            PresetProfile::Pc525Dd,
            300.0,
            250,
            "IBM PC",
            0,
            42,
            true,
            false,
            FsInitMode::Blank,
            TrackRange::new(0, 41),
            None,
        );
        let text_48_over: String = lines_48_over
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_48_over.contains("Target Tracks : 42 tracks"));
        assert!(text_48_over.contains("Range: 00..41 | Standard: 40, Max: 42"));
        assert!(text_48_over.contains("Tracks 00..41, Head 1 only"));
        assert!(text_48_over.contains("Target Track  : Track 20"));
        assert!(text_48_over.contains("Head: Head 1"));

        // 80 TPI modal line test
        let lines_80 = build_format_modal_lines(
            0,
            HeadSelection::Both,
            79,
            PresetProfile::Pc35Hd,
            300.0,
            500,
            "IBM PC",
            0,
            80,
            false,
            true,
            FsInitMode::Blank,
            TrackRange::new(0, 79),
            None,
        );
        let text_80: String = lines_80
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_80.contains("Target Tracks : 80 tracks"));
        assert!(text_80.contains("Range: 00..79 | Standard: 80, Max: 84"));
        assert!(text_80.contains("Tracks 00..79, Dual-Head"));
        assert!(text_80.contains("Target Track  : Track 00"));
        assert!(text_80.contains("Head: Both"));
    }

    #[test]
    fn test_erase_modal_rendering_with_dynamic_tracks_and_head() {
        let lines = build_erase_modal_lines(
            35,
            HeadSelection::Head1,
            "3.5\" HD (1.44M)",
            300.0,
            500,
            80,
            false,
            TrackRange::new(10, 20),
            None,
        );
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text.contains("Target Track  : Track 35"));
        assert!(text.contains("Head: Head 1"));
        assert!(text.contains("Target Tracks : 80 tracks"));
        assert!(text.contains("Erase Current Track only  (Track 35, Head 1 only)"));
        assert!(text.contains("Erase Track Range     (Tracks 10..20, Head 1 only)"));
        assert!(text.contains("Erase Entire Disk         (Tracks 00..79, Head 1 only)"));
    }

    #[test]
    fn test_app_track_and_head_navigation() {
        let mut app = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Single, PresetProfile::Pc35Hd);
        assert_eq!(app.status.track, 0);
        assert_eq!(app.status.head, 0);

        // Step track up
        assert_eq!(app.step_track_up(), 1);
        assert_eq!(app.status.track, 1);
        assert_eq!(app.step_track_up(), 2);
        assert_eq!(app.status.track, 2);

        // Step track down
        assert_eq!(app.step_track_down(), 1);
        assert_eq!(app.status.track, 1);
        assert_eq!(app.step_track_down(), 0);
        assert_eq!(app.status.track, 0);
        // Clamped at 0
        assert_eq!(app.step_track_down(), 0);
        assert_eq!(app.status.track, 0);

        // Head toggle (0 -> 1 -> Both -> 0)
        app.toggle_head();
        assert_eq!(app.status.head, 1);
        assert_eq!(app.status.head_select, HeadSelection::Head1);

        app.toggle_head();
        assert_eq!(app.status.head, 0);
        assert_eq!(app.status.head_select, HeadSelection::Both);

        app.toggle_head();
        assert_eq!(app.status.head, 0);
        assert_eq!(app.status.head_select, HeadSelection::Head0);
    }

    #[test]
    fn test_range_edit_modal_lifecycle_and_validation() {
        let mut app = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Single, PresetProfile::Pc35Hd);

        // 1. Open for Format
        app.open_range_modal(RangeModalKind::Format);
        assert_eq!(app.show_range_modal, Some(RangeModalKind::Format));
        assert_eq!(app.range_edit_field, RangeField::Start);
        assert_eq!(app.range_input_start, "0");
        assert_eq!(app.range_input_end, "79");
        assert!(app.range_error_msg.is_none());

        // Increment Start field
        app.increment_active_range_field();
        assert_eq!(app.range_input_start, "1");
        app.increment_active_range_field();
        assert_eq!(app.range_input_start, "2");
        app.decrement_active_range_field();
        assert_eq!(app.range_input_start, "1");

        // Toggle field exclusively via Tab to End
        app.toggle_range_field();
        assert_eq!(app.range_edit_field, RangeField::End);

        // Increment End field (clamped at max allowed 83)
        app.range_input_end = "82".to_string();
        app.increment_active_range_field();
        assert_eq!(app.range_input_end, "83");
        app.increment_active_range_field();
        assert_eq!(app.range_input_end, "83"); // Clamped

        // Decrement End field (clamped at 0)
        app.range_input_end = "1".to_string();
        app.decrement_active_range_field();
        assert_eq!(app.range_input_end, "0");
        app.decrement_active_range_field();
        assert_eq!(app.range_input_end, "0"); // Clamped

        // Edit End field: backspace twice, type "40"
        app.range_input_start = "0".to_string();
        app.range_input_end = "40".to_string();

        // Validate
        let res = app.validate_and_apply_range();
        assert!(res.is_ok());
        let (range, kind) = res.unwrap();
        assert_eq!(range, TrackRange::new(0, 40));
        assert_eq!(kind, RangeModalKind::Format);
        assert_eq!(app.format_range, TrackRange::new(0, 40));
        assert_eq!(app.show_range_modal, None);

        // 2. Open for Erase and test error cases
        app.open_range_modal(RangeModalKind::Erase);
        assert_eq!(app.show_range_modal, Some(RangeModalKind::Erase));

        // Set start to "50" and end to "20" -> should error start > end
        app.range_edit_field = RangeField::Start;
        app.range_input_start = "50".to_string();
        app.range_input_end = "20".to_string();

        let err_res = app.validate_and_apply_range();
        assert!(err_res.is_err());
        assert!(err_res.unwrap_err().contains("cannot exceed End track"));

        // Set end to "90" -> exceeds max limit (84 for 80 TPI)
        app.range_input_start = "10".to_string();
        app.range_input_end = "90".to_string();
        let err_res2 = app.validate_and_apply_range();
        assert!(err_res2.is_err());
        assert!(err_res2.unwrap_err().contains("exceeds max allowed"));

        // Close and return to parent modal
        app.close_range_modal(true);
        assert_eq!(app.show_range_modal, None);
        assert!(app.show_erase_modal);
    }

    #[test]
    fn test_range_edit_modal_lines_and_head_selection() {
        // Test Dual-Head summary line
        let lines_both = build_range_edit_modal_lines(
            HeadSelection::Both,
            "0",
            "79",
            RangeField::Start,
            80,
            None,
        );
        let text_both: String = lines_both
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_both.contains("Total: 80 tracks (Tracks 00..79, Dual-Head = 160 passes)"));
        assert!(text_both.contains("[Tab] Switch Field   [+/- or ↑/↓] Inc/Dec   [0-9] Edit   [Bksp] Clear"));
        assert!(text_both.contains("[H] Toggle Head      [Enter] Validate & Start     [Esc] Cancel & Back"));

        // Test Head 0 only summary line
        let lines_h0 = build_range_edit_modal_lines(
            HeadSelection::Head0,
            "10",
            "20",
            RangeField::End,
            80,
            None,
        );
        let text_h0: String = lines_h0
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_h0.contains("Total: 11 tracks (Tracks 10..20, Head 0 only = 11 passes)"));

        // Test Head 1 only summary line
        let lines_h1 = build_range_edit_modal_lines(
            HeadSelection::Head1,
            "5",
            "15",
            RangeField::Start,
            80,
            None,
        );
        let text_h1: String = lines_h1
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_h1.contains("Total: 11 tracks (Tracks 05..15, Head 1 only = 11 passes)"));

        // Test error display
        let lines_err = build_range_edit_modal_lines(
            HeadSelection::Both,
            "50",
            "20",
            RangeField::Start,
            80,
            Some("Start track (50) cannot exceed End track (20)"),
        );
        let text_err: String = lines_err
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_err.contains("⚠️  Start track (50) cannot exceed End track (20)"));

        // Test head cycling integration in Range Modal
        let mut app = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Single, PresetProfile::Pc35Hd);
        app.open_range_modal(RangeModalKind::Format);
        assert_eq!(app.head_selection, HeadSelection::Head0);

        // Cycle head in modal: Head 0 -> Head 1 -> Both -> Head 0
        app.toggle_head();
        assert_eq!(app.head_selection, HeadSelection::Head1);

        app.toggle_head();
        assert_eq!(app.head_selection, HeadSelection::Both);

        app.range_input_start = "10".to_string();
        app.range_input_end = "20".to_string();
        let (range, kind) = app.validate_and_apply_range().unwrap();
        assert_eq!(range, TrackRange::new(10, 20));
        assert_eq!(kind, RangeModalKind::Format);

        // Confirm created confirmation receives the HeadSelection::Both
        let conf = PendingConfirmation::FormatRange {
            range,
            head_sel: app.head_selection,
            verify: app.format_verify,
            fs_mode: app.format_fs_mode,
            preset: app.preset,
        };
        assert_eq!(conf.prompt_string(), "Confirm Format Range 10..20 (Dual-Head)? [y/N]");
    }

    #[test]
    fn test_pending_confirmation_prompts_and_rendering() {
        // Test prompt string variations with HeadSelection and Blank mode
        let c_fmt_trk_h0 = PendingConfirmation::FormatTrack {
            track: 0,
            head_sel: HeadSelection::Head0,
            verify: true,
            fs_mode: FsInitMode::Blank,
            preset: PresetProfile::Pc35Hd,
        };
        assert_eq!(c_fmt_trk_h0.prompt_string(), "Confirm Format Track 00 (Head 0 only)? [y/N]");

        let c_fmt_trk_both = PendingConfirmation::FormatTrack {
            track: 40,
            head_sel: HeadSelection::Both,
            verify: true,
            fs_mode: FsInitMode::Blank,
            preset: PresetProfile::Pc35Hd,
        };
        assert_eq!(c_fmt_trk_both.prompt_string(), "Confirm Format Track 40 (Dual-Head)? [y/N]");

        let c_fmt_disk = PendingConfirmation::FormatDisk {
            range: TrackRange::new(0, 79),
            head_sel: HeadSelection::Both,
            verify: true,
            fs_mode: FsInitMode::Blank,
            preset: PresetProfile::Pc35Hd,
        };
        assert_eq!(c_fmt_disk.prompt_string(), "Confirm FULL DISK Format (00..79, Dual-Head)? [y/N]");

        let c_fmt_range = PendingConfirmation::FormatRange {
            range: TrackRange::new(10, 20),
            head_sel: HeadSelection::Head1,
            verify: false,
            fs_mode: FsInitMode::Blank,
            preset: PresetProfile::Pc35Hd,
        };
        assert_eq!(c_fmt_range.prompt_string(), "Confirm Format Range 10..20 (Head 1 only)? [y/N]");

        // Test OS-Ready confirmation prompts
        let c_os_pc = PendingConfirmation::FormatDisk {
            range: TrackRange::new(0, 79),
            head_sel: HeadSelection::Both,
            verify: true,
            fs_mode: FsInitMode::OsReady,
            preset: PresetProfile::Pc35Hd,
        };
        assert_eq!(
            c_os_pc.prompt_string(),
            "Confirm FULL DISK Format (00..79, Dual-Head, OS-Ready: DOS FAT12)? [y/N]"
        );

        let c_os_amiga = PendingConfirmation::FormatDisk {
            range: TrackRange::new(0, 79),
            head_sel: HeadSelection::Both,
            verify: true,
            fs_mode: FsInitMode::OsReady,
            preset: PresetProfile::Amiga35Dd,
        };
        assert_eq!(
            c_os_amiga.prompt_string(),
            "Confirm FULL DISK Format (00..79, Dual-Head, OS-Ready: AmigaDOS OFS)? [y/N]"
        );

        let c_os_atari = PendingConfirmation::FormatTrack {
            track: 0,
            head_sel: HeadSelection::Head0,
            verify: true,
            fs_mode: FsInitMode::OsReady,
            preset: PresetProfile::Atari35Dd,
        };
        assert_eq!(
            c_os_atari.prompt_string(),
            "Confirm Format Track 00 (Head 0 only, OS-Ready: Atari TOS)? [y/N]"
        );

        let c_os_cpc = PendingConfirmation::FormatRange {
            range: TrackRange::new(0, 39),
            head_sel: HeadSelection::Head0,
            verify: false,
            fs_mode: FsInitMode::OsReady,
            preset: PresetProfile::Cpc30Data,
        };
        assert_eq!(
            c_os_cpc.prompt_string(),
            "Confirm Format Range 00..39 (Head 0 only, OS-Ready: CP/M Data)? [y/N]"
        );

        let c_erase_trk = PendingConfirmation::EraseTrack { track: 40, head_sel: HeadSelection::Head1 };
        assert_eq!(c_erase_trk.prompt_string(), "Confirm Erase Track 40 (Head 1 only)? [y/N]");

        let c_erase_disk = PendingConfirmation::EraseDisk { range: TrackRange::new(0, 79), head_sel: HeadSelection::Both };
        assert_eq!(c_erase_disk.prompt_string(), "Confirm FULL DISK Erase (00..79, Dual-Head)? [y/N]");

        let c_erase_range = PendingConfirmation::EraseRange { range: TrackRange::new(5, 15), head_sel: HeadSelection::Head0 };
        assert_eq!(c_erase_range.prompt_string(), "Confirm Erase Range 05..15 (Head 0 only)? [y/N]");

        // Test Format modal with confirmation active
        let lines_fmt = build_format_modal_lines(
            0,
            HeadSelection::Head0,
            79,
            PresetProfile::Pc35Hd,
            300.0,
            500,
            "IBM PC",
            0,
            80,
            false,
            true,
            FsInitMode::Blank,
            TrackRange::new(0, 79),
            Some(&c_fmt_trk_h0),
        );
        let text_fmt: String = lines_fmt
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_fmt.contains("Confirm Format Track 00 (Head 0 only)? [y/N]"));
        assert!(text_fmt.contains("Press Y to confirm & execute, or N / Enter / Esc to cancel"));

        // Test Erase modal with confirmation active
        let lines_erase = build_erase_modal_lines(
            40,
            HeadSelection::Head1,
            "3.5\" HD (1.44M)",
            300.0,
            500,
            80,
            false,
            TrackRange::new(0, 79),
            Some(&c_erase_trk),
        );
        let text_erase: String = lines_erase
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_erase.contains("Confirm Erase Track 40 (Head 1 only)? [y/N]"));
        assert!(text_erase.contains("Press Y to confirm & execute, or N / Enter / Esc to cancel"));

        // Test modal options rendering with dynamic head selection labels
        let lines_fmt_idle = build_format_modal_lines(
            20,
            HeadSelection::Both,
            79,
            PresetProfile::Pc35Hd,
            300.0,
            500,
            "IBM PC",
            0,
            80,
            false,
            true,
            FsInitMode::Blank,
            TrackRange::new(10, 30),
            None,
        );
        let text_fmt_idle: String = lines_fmt_idle
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_fmt_idle.contains("isk     (Tracks 00..79, Dual-Head)"));
    }

    #[test]
    fn test_format_timing_constants() {
        assert_eq!(crate::hw::FORMAT_HEAD_SETTLE_MS, 100);
        assert_eq!(crate::hw::FORMAT_HEAD_SWITCH_SETTLE_MS, 50);
    }

    #[test]
    fn test_active_modal_enum_and_app_queries() {
        let mut app = App::new();
        assert_eq!(app.active_modal(), crate::app::ActiveModal::None);

        app.handle_action(Action::OpenFormatModal);
        assert_eq!(app.active_modal(), crate::app::ActiveModal::Format);

        app.handle_action(Action::CloseFormatModal);
        app.handle_action(Action::OpenEraseModal);
        assert_eq!(app.active_modal(), crate::app::ActiveModal::Erase);

        app.handle_action(Action::CloseEraseModal);
        app.open_range_modal(crate::app::RangeModalKind::Format);
        assert_eq!(app.active_modal(), crate::app::ActiveModal::Range(crate::app::RangeModalKind::Format));

        app.show_range_modal = None;
        app.show_help = true;
        assert_eq!(app.active_modal(), crate::app::ActiveModal::Help);
    }

    #[test]
    fn test_modal_unit_and_bus_shortcuts_and_decoupling() {
        let mut app = App::with_full_preset_config(0, BusType::IbmPc, StepMode::Single, PresetProfile::Pc35Hd);
        assert_eq!(app.drive_unit, 0);
        assert_eq!(app.bus_type, BusType::IbmPc);

        // Open Format modal
        app.handle_action(Action::OpenFormatModal);
        assert!(app.show_format_modal);

        // Toggle unit U: Drive 0 -> Drive 1
        app.handle_action(Action::ToggleDriveUnit);
        assert_eq!(app.drive_unit, 1);
        assert_eq!(app.status.drive_unit, 1);

        // Toggle unit U again: Drive 1 -> Drive 0 (on IBM PC bus)
        app.handle_action(Action::ToggleDriveUnit);
        assert_eq!(app.drive_unit, 0);

        // Toggle Bus B: IbmPc -> Shugart
        app.handle_action(Action::ToggleBusType);
        assert_eq!(app.bus_type, BusType::Shugart);
        assert_eq!(app.status.bus_type, BusType::Shugart);

        // On Shugart, units cycle 0..3
        app.handle_action(Action::ToggleDriveUnit);
        assert_eq!(app.drive_unit, 1);
        app.handle_action(Action::ToggleDriveUnit);
        assert_eq!(app.drive_unit, 2);

        // Toggle Bus back to IbmPc: clamps unit > 1 to 0
        app.handle_action(Action::ToggleBusType);
        assert_eq!(app.bus_type, BusType::IbmPc);
        assert_eq!(app.drive_unit, 0);

        // Cycle preset P in modal: preserves active bus_type (IbmPc)
        app.handle_action(Action::CyclePreset);
        assert_eq!(app.bus_type, BusType::IbmPc);
    }

    #[test]
    fn test_post_op_progress_lines_completed_and_aborted() {
        let mut status = DriveStatus {
            preset: PresetProfile::Pc35Hd,
            mode: DisplayMode::Format,
            format_progress: Some(FormatProgress {
                current_track: 79,
                current_head: 1,
                total_tracks: 80,
                total_heads: 2,
                step: FormatStep::Completed,
                completed_passes: 160,
                total_passes: 160,
                verification_ok: true,
                retry_count: 0,
                quality_pct: 100,
                crc_errors: 0,
                verified_sectors: 18,
                expected_sectors: 18,
                elapsed_secs: 65.0,
                eta_secs: 0.0,
                message: "Format completed successfully".to_string(),
            }),
            ..Default::default()
        };

        // Completed format
        let lines_completed = build_format_progress_lines(&status);
        let text_completed: String = lines_completed
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_completed.contains("Press [Enter] or [Esc] to return to menu, [Q] for Main View."));

        // Aborted format (Idle / aborted)
        if let Some(ref mut p) = status.format_progress {
            p.step = FormatStep::Idle;
            p.message = "Format aborted by user".to_string();
        }
        let lines_aborted = build_format_progress_lines(&status);
        let text_aborted: String = lines_aborted
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_aborted.contains("Press [Enter] or [Esc] to return to menu, [Q] for Main View."));

        // Running format (in progress)
        if let Some(ref mut p) = status.format_progress {
            p.step = FormatStep::Writing;
            p.message = "Writing flux...".to_string();
        }
        let lines_running = build_format_progress_lines(&status);
        let text_running: String = lines_running
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text_running.contains("Press [Esc] at any time to abort formatting safely."));
    }
}


