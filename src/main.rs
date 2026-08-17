use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
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
pub use hw::{hw_thread, DisplayMode, DriveStatus, HwActivity, HwCmd};
pub use ui::*;

/// Builds the clean CLI banner and help/version text
pub fn build_cli_banner() -> String {
    format!(
r#"💾 AlignTesterDiag v{}
Real-time Floppy Drive Diagnostic & Alignment Tool for Greaseweazle
Copyright (C) 2026 MonSieur JeAn-FReD (GPL-3.0)

Usage: aligntester-diag [OPTIONS] [PORT]

Arguments:
  [PORT]              Serial port connected to Greaseweazle (e.g. COM3, /dev/ttyACM0)

Options:
  -d, --drive <0|1>   Select physical drive unit (0 for Drive A:, 1 for Drive B:) [default: 0]
  -p, --port <PORT>   Serial port connected to Greaseweazle
  -h, --help          Print help information
  -v, -V, --version   Print version information
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

pub fn parse_cli_args(args: &[String]) -> (Option<String>, u8) {
    let mut port = None;
    let mut drive_unit: u8 = 0;
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--drive" || arg == "-d" {
            if i + 1 < args.len() {
                if let Ok(val) = args[i + 1].parse::<u8>() {
                    drive_unit = val.min(1);
                }
                i += 1;
            }
        } else if let Some(stripped) = arg.strip_prefix("--drive=") {
            if let Ok(val) = stripped.parse::<u8>() {
                drive_unit = val.min(1);
            }
        } else if let Some(stripped) = arg.strip_prefix("-d=") {
            if let Ok(val) = stripped.parse::<u8>() {
                drive_unit = val.min(1);
            }
        } else if arg == "--port" || arg == "-p" {
            if i + 1 < args.len() {
                port = Some(args[i + 1].clone());
                i += 1;
            }
        } else if let Some(stripped) = arg.strip_prefix("--port=") {
            port = Some(stripped.to_string());
        } else if let Some(stripped) = arg.strip_prefix("-p=") {
            port = Some(stripped.to_string());
        } else if !arg.starts_with('-') && port.is_none() {
            port = Some(arg.clone());
        }
        i += 1;
    }
    (port, drive_unit)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if handle_cli_help_or_version(&args) {
        return Ok(());
    }
    let (port_name, drive_unit) = parse_cli_args(&args);

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let (tx_status, rx_status) = unbounded::<DriveStatus>();
    let (tx_cmd, rx_cmd) = unbounded::<HwCmd>();

    let port_arg = port_name.clone();
    thread::spawn(move || {
        hw_thread(tx_status, rx_cmd, port_arg, drive_unit);
    });

    let mut app = App::with_drive_unit(drive_unit);
    if let Some(ref p) = port_name {
        app.status.port_name = p.clone();
    }

    loop {
        while let Ok(msg) = rx_status.try_recv() {
            app.handle_hw_message(msg);
        }
        let status = &app.status;

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Min(1),
                ])
                .split(f.size());

            let drive_letter = if status.drive_unit == 0 { "A:" } else { "B:" };
            let max_sec = status.sector_count;

            let sec_count_str = if status.sectors_known && status.sector_count > 0 {
                format!("{:2}x512", status.sector_count)
            } else {
                String::from(" ?x512")
            };

            let mut sec_line_spans = Vec::new();
            if status.has_disk && status.sectors_known && max_sec > 0 {
                sec_line_spans.push(Span::styled(" ", Style::default()));
                for i in 1..=max_sec {
                    let s_str = format!("{:2} ", i);
                    let sec_present = status.sectors.iter().any(|s| s.sec_id == i);
                    let has_err = status.sectors.iter().any(|s| s.sec_id == i && !s.crc_ok);

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
                    sec_line_spans.push(Span::styled(" ? ", Style::default().fg(Color::Yellow)));
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
                format!("     {}  27  84         ", sec_count_str),
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
                HwActivity::Seeking => "SEEKING...".to_string(),
                HwActivity::Stopped => "STOPPED".to_string(),
                HwActivity::WaitingPort => "CONNECTING".to_string(),
                HwActivity::Idle => "IDLE".to_string(),
            };

            let menu_lines = vec![
                Line::from(" Insert formatted"),
                Line::from(" diskette"),
                Line::from(""),
                Line::from(format!(" UNIT : Drive {} ({})", status.drive_unit, if status.drive_unit == 0 { "A:" } else { "B:" })),
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
                Line::from(" Esc = Stop / Motor off"),
                Line::from(" Backspace = Panic Reset"),
                Line::from(" F = Format"),
                Line::from(" H = Head 0/1/Both"),
                Line::from(" I = track Image"),
                Line::from(" L = Live RPM test"),
                Line::from(" M = Motor on/off"),
                Line::from(" P = fmt Parms"),
                Line::from(" R = Recal/seek"),
                Line::from(" S = Step S/D"),
                Line::from(" U = Unit (Drive 0/1)"),
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
                        let target_rpm = 300.0f64;
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
                    right_lines.push(Line::from(format!("Log: {}", status.log_msg)));
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
                    right_lines.push(Line::from(format!("Log: {}", status.log_msg)));
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
                        "Drive Selection   : Drive {} ({}) ({})",
                        status.drive_unit,
                        if status.drive_unit == 0 { "A:" } else { "B:" },
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
                    right_lines.push(Line::from(format!("Log: {}", status.log_msg)));
                }
            }

            let right_panel = Paragraph::new(right_lines)
                .style(Style::default().fg(Color::White).bg(Color::Blue));
            f.render_widget(right_panel, lower_chunks[1]);

            if app.show_help {
                render_help_modal(f, f.size());
            }
        })?;

        if event::poll(Duration::from_millis(15))? {
            if let Event::Key(key) = event::read()? {
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

                if key.code == KeyCode::Backspace
                    || key.code == KeyCode::Char('\x08')
                    || key.code == KeyCode::Char('\u{8}')
                {
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
                    KeyCode::Char('+') | KeyCode::Char('=') | KeyCode::Right | KeyCode::Up => {
                        let _ = tx_cmd.send(HwCmd::Seek(app.status.track.saturating_add(1)));
                    }
                    KeyCode::Char('-') | KeyCode::Char('_') | KeyCode::Left | KeyCode::Down => {
                        let _ = tx_cmd.send(HwCmd::Seek(app.status.track.saturating_sub(1)));
                    }
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        let track = (c.to_digit(10).unwrap() as u8) * 10;
                        let _ = tx_cmd.send(HwCmd::Seek(track));
                    }
                    KeyCode::Char('h') | KeyCode::Char('H') => {
                        let _ = tx_cmd.send(HwCmd::ToggleHead);
                    }
                    KeyCode::Char('l') | KeyCode::Char('L') => {
                        let _ = tx_cmd.send(HwCmd::MeasureRpm);
                    }
                    KeyCode::Char('m') | KeyCode::Char('M') => {
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
                        let _ = tx_cmd.send(HwCmd::ZeroTrack);
                    }
                    KeyCode::Char('a') | KeyCode::Char('A') => {
                        app.handle_action(Action::Analyze);
                        let _ = tx_cmd.send(HwCmd::Analyze);
                    }
                    KeyCode::Char('d') | KeyCode::Char('D') => {
                        let _ = tx_cmd.send(HwCmd::ReadData);
                    }
                    KeyCode::Char('v') | KeyCode::Char('V') => {
                        let _ = tx_cmd.send(HwCmd::ToggleVerbose);
                    }
                    KeyCode::Char('b') | KeyCode::Char('B') => {
                        let _ = tx_cmd.send(HwCmd::ToggleBeep);
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    println!("Alignment Diagnostic session ended cleanly.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let (port, unit) = parse_cli_args(&args_default);
        assert_eq!(port, None);
        assert_eq!(unit, 0);

        let args_port_only = vec!["aligntester".to_string(), "COM3".to_string()];
        let (port, unit) = parse_cli_args(&args_port_only);
        assert_eq!(port, Some("COM3".to_string()));
        assert_eq!(unit, 0);

        let args_drive1 = vec!["aligntester".to_string(), "--drive".to_string(), "1".to_string()];
        let (port, unit) = parse_cli_args(&args_drive1);
        assert_eq!(port, None);
        assert_eq!(unit, 1);

        let args_drive0 = vec!["aligntester".to_string(), "--drive".to_string(), "0".to_string()];
        let (port, unit) = parse_cli_args(&args_drive0);
        assert_eq!(port, None);
        assert_eq!(unit, 0);

        let args_short_d1 = vec!["aligntester".to_string(), "-d".to_string(), "1".to_string()];
        let (port, unit) = parse_cli_args(&args_short_d1);
        assert_eq!(port, None);
        assert_eq!(unit, 1);

        let args_eq_syntax = vec!["aligntester".to_string(), "--drive=1".to_string(), "COM5".to_string()];
        let (port, unit) = parse_cli_args(&args_eq_syntax);
        assert_eq!(port, Some("COM5".to_string()));
        assert_eq!(unit, 1);

        let args_short_eq = vec!["aligntester".to_string(), "COM5".to_string(), "-d=1".to_string()];
        let (port, unit) = parse_cli_args(&args_short_eq);
        assert_eq!(port, Some("COM5".to_string()));
        assert_eq!(unit, 1);

        // Saturation to 1
        let args_out_of_bounds = vec!["aligntester".to_string(), "--drive".to_string(), "4".to_string()];
        let (_port, unit) = parse_cli_args(&args_out_of_bounds);
        assert_eq!(unit, 1);
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
        let (port, unit) = parse_cli_args(&args_p_short);
        assert_eq!(port, Some("COM4".to_string()));
        assert_eq!(unit, 1);

        let args_p_long = vec!["aligntester".to_string(), "--port".to_string(), "/dev/ttyACM0".to_string()];
        let (port, unit) = parse_cli_args(&args_p_long);
        assert_eq!(port, Some("/dev/ttyACM0".to_string()));
        assert_eq!(unit, 0);

        let args_p_eq = vec!["aligntester".to_string(), "--port=COM7".to_string(), "--drive=1".to_string()];
        let (port, unit) = parse_cli_args(&args_p_eq);
        assert_eq!(port, Some("COM7".to_string()));
        assert_eq!(unit, 1);

        let args_short_p_eq = vec!["aligntester".to_string(), "-p=COM9".to_string()];
        let (port, unit) = parse_cli_args(&args_short_p_eq);
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
        assert!(full_text.contains("MonSieur JeAn-FReD"));
        assert!(full_text.contains("License: GPL-3.0"));
        assert!(full_text.contains("? / F1"));
        assert!(full_text.contains("Analyze"));
        assert!(full_text.contains("Audio Radar"));
        assert!(full_text.contains("Read Data"));
        assert!(full_text.contains("Stop / Motor off"));
        assert!(full_text.contains("PANIC RESET"));
        assert!(full_text.contains("Head 0 -> Head 1 -> Both 0+1"));
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
}


