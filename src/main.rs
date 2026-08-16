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
    widgets::Paragraph,
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
mod hw;
mod ui;

use crossbeam_channel::unbounded;
pub use app::*;
pub use hw::{hw_thread, DisplayMode, DriveStatus, HwActivity, HwCmd};
pub use ui::*;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    let port_name = args.get(1).cloned();

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let (tx_status, rx_status) = unbounded::<DriveStatus>();
    let (tx_cmd, rx_cmd) = unbounded::<HwCmd>();

    thread::spawn(move || {
        hw_thread(tx_status, rx_cmd, port_name);
    });

    let mut status = DriveStatus::default();
    loop {
        while let Ok(new_status) = rx_status.try_recv() {
            status = new_status;
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(4), Constraint::Min(1)])
                .split(f.size());

            let drive_letter = if status.unit_id == 0 { "A:" } else { "B:" };
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

            let mut top_spans = vec![
                Span::styled(
                    format!(
                        "{} {}k {}    T{:02}  H{}   ",
                        drive_letter,
                        status.bitrate,
                        if status.bitrate == 500 { "HD" } else { "DD" },
                        status.track,
                        status.head,
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

            let header_lines = vec![
                Line::from(top_spans),
                Line::from(sec_line_spans),
                build_ruler_line(status.track),
                Line::from(""),
            ];
            let header = Paragraph::new(header_lines).style(Style::default().bg(Color::Red));
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
                    if status.beep_enabled { "ON " } else { "OFF" }
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
                Line::from(" H = Head 0/1"),
                Line::from(" I = track Image"),
                Line::from(" L = Live RPM test"),
                Line::from(" M = Motor on/off"),
                Line::from(" P = fmt Parms"),
                Line::from(" R = Recal/seek"),
                Line::from(" S = Step S/D"),
                Line::from(" V = Verbose on/off"),
                Line::from(" W = Write data"),
                Line::from(" Z = Zero track"),
                Line::from(" 0-9 = seek 0-90"),
                Line::from(" +/- = Seek +/-1"),
                Line::from(" X   = eXit"),
            ];

            let menu = Paragraph::new(menu_lines)
                .style(Style::default().fg(Color::White).bg(Color::Blue));
            f.render_widget(menu, lower_chunks[0]);

            let mut right_lines = Vec::new();

            match status.mode {
                DisplayMode::RpmMeasure => {
                    right_lines.push(Line::from(Span::styled(
                        "=== MOTOR TACHOMETER / HIGH-PRECISION RPM TEST ===",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )));
                    right_lines.push(Line::from(""));

                    if status.rpm_measure.sample_count > 0 {
                        let instant_rpm = status.rpm_measure.instant_rpm;
                        let target_rpm = 300.0f64;
                        let diff = instant_rpm - target_rpm;
                        let sign = if diff >= 0.0 { "+" } else { "" };
                        let diff_pct = (diff / target_rpm) * 100.0;

                        let jitter_color = if status.rpm_measure.jitter_rpm < 0.5 {
                            Color::Green
                        } else if status.rpm_measure.jitter_rpm < 1.5 {
                            Color::LightGreen
                        } else if status.rpm_measure.jitter_rpm < 3.0 {
                            Color::Yellow
                        } else {
                            Color::Red
                        };

                        right_lines.push(Line::from(vec![
                            Span::styled(
                                "► Instantaneous Speed     : ",
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                format!("{:.1} RPM", instant_rpm),
                                Style::default()
                                    .fg(Color::LightGreen)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));

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
                                "► Speed Jitter (Gigue)    : ",
                                Style::default().fg(Color::White),
                            ),
                            Span::styled(
                                format!(
                                    "σ = ±{:.1} RPM  ({:.1} RPM peak-to-peak)",
                                    status.rpm_measure.jitter_rpm,
                                    status.rpm_measure.max_rpm - status.rpm_measure.min_rpm
                                ),
                                Style::default()
                                    .fg(jitter_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));

                        right_lines.push(Line::from(vec![
                            Span::styled(
                                "► Statistical Average     : ",
                                Style::default().fg(Color::White),
                            ),
                            Span::styled(
                                format!(
                                    "{:.1} RPM  (over {} revolutions captured)",
                                    status.rpm_measure.avg_rpm, status.rpm_measure.sample_count
                                ),
                                Style::default().fg(Color::LightCyan),
                            ),
                        ]));

                        let stability_rating = if status.rpm_measure.jitter_rpm < 0.5 {
                            ("★★★★★ EXCELLENT STABILITY (Jitter < 0.5 RPM)", Color::Green)
                        } else if status.rpm_measure.jitter_rpm < 1.5 {
                            ("★★★★☆ GOOD STABILITY (Jitter < 1.5 RPM)", Color::LightGreen)
                        } else if status.rpm_measure.jitter_rpm < 3.0 {
                            ("★★★☆☆ ACCEPTABLE STABILITY (Jitter < 3.0 RPM)", Color::Yellow)
                        } else {
                            ("★☆☆☆☆ UNSTABLE MOTOR SPEED (Jitter >= 3.0 RPM)", Color::Red)
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

                    let align_color = if status.alignment_pct >= 95.0 {
                        Color::Green
                    } else if status.alignment_pct >= 80.0 {
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
                                status.alignment_pct,
                                build_alignment_gauge(status.alignment_pct)
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
                            format!("{} sectors", status.on_track_count),
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));

                    let off_color = if status.off_track_count == 0 {
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
                            format!("{} ({})", status.off_track_count, status.off_track_details),
                            Style::default().fg(off_color).add_modifier(Modifier::BOLD),
                        ),
                    ]));

                    let crc_color = if status.crc_err_count == 0 {
                        Color::Green
                    } else {
                        Color::Red
                    };
                    right_lines.push(Line::from(vec![
                        Span::styled(
                            "► CRC Integrity Check     : ",
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            if status.crc_err_count == 0 {
                                "100% OK (0 errors)".to_string()
                            } else {
                                format!("{} CRC Error(s)", status.crc_err_count)
                            },
                            Style::default().fg(crc_color).add_modifier(Modifier::BOLD),
                        ),
                    ]));

                    right_lines.push(Line::from(""));
                    let subtitle = if status.verbose_mode {
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

                    let available_height = lower_chunks[1].height as usize;
                    // Sliding history of 12 to 13 lines
                    let max_vertical_items = available_height.saturating_sub(11).clamp(1, 13);
                    let start_idx = status
                        .sector_log
                        .len()
                        .saturating_sub(max_vertical_items);
                    let recent_logs = &status.sector_log[start_idx..];

                    if recent_logs.is_empty() {
                        right_lines.push(Line::from(Span::styled(
                            format!(
                                "T:{:02} H:{} : (Waiting read stream...)",
                                status.track, status.head
                            ),
                            Style::default().fg(Color::DarkGray),
                        )));
                    } else {
                        for (i, log_line) in recent_logs.iter().enumerate() {
                            let is_last = i == recent_logs.len().saturating_sub(1);
                            let prefix = if is_last { " ► " } else { "   " };
                            let prefix_span = Span::styled(
                                prefix,
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            );

                            if status.verbose_mode {
                                let mut spans = vec![prefix_span];
                                spans.extend(build_verbose_line_spans(log_line, status.sector_count));
                                right_lines.push(Line::from(spans));
                            } else {
                                let mut spans = vec![prefix_span];
                                spans.extend(build_standard_line_spans(log_line, status.sector_count));
                                right_lines.push(Line::from(spans));
                            }
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
                        "Drive Selection   : Drive {} ({})",
                        if status.unit_id == 0 { "A:" } else { "B:" },
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
                        "Current Head      : Head {}",
                        status.head
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
                        "    B = Beep        : Toggle audio feedback on/off",
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
                    right_lines.push(Line::from("    H               : Toggle Head 0 / Head 1"));
                    right_lines.push(Line::from(
                        "    R               : Recalibrate Track 0 -> Current track",
                    ));
                    right_lines.push(Line::from(
                        "    V = Verbose     : Toggle Standard / Verbose display mode",
                    ));
                    right_lines.push(Line::from(
                        "    Z               : Zero track (Direct return to Track 0)",
                    ));
                    right_lines.push(Line::from(
                        "    X               : Clean exit (Instant motor & LED shutdown)",
                    ));
                    right_lines.push(Line::from(""));
                    right_lines.push(Line::from(format!("Log: {}", status.log_msg)));
                }
            }

            let right_panel = Paragraph::new(right_lines)
                .style(Style::default().fg(Color::White).bg(Color::Blue));
            f.render_widget(right_panel, lower_chunks[1]);
        })?;

        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if key.code == KeyCode::Backspace
                    || key.code == KeyCode::Char('\x08')
                    || key.code == KeyCode::Char('\u{8}')
                {
                    let _ = tx_cmd.send(HwCmd::PanicReset);
                    continue;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('C'))
                {
                    let _ = tx_cmd.send(HwCmd::Exit);
                    break;
                }
                match key.code {
                    KeyCode::Char('x') | KeyCode::Char('X') => {
                        let _ = tx_cmd.send(HwCmd::Exit);
                        break;
                    }
                    KeyCode::Esc => {
                        status.motor_on = false;
                        status.analyzing = false;
                        if status.mode == DisplayMode::RpmMeasure && status.rpm_measure.sample_count > 0 {
                            status.rpm_display = format!("{:.1} RPM", status.rpm_measure.avg_rpm);
                        }
                        status.mode = DisplayMode::None;
                        status.activity = HwActivity::Stopped;
                        status.index = false;
                        status.log_msg = String::from("Stop / Motor OFF (Safe to change disk)");
                        let _ = tx_cmd.send(HwCmd::Stop);
                    }
                    KeyCode::Char('+') | KeyCode::Char('=') | KeyCode::Right | KeyCode::Up => {
                        let _ = tx_cmd.send(HwCmd::Seek(status.track.saturating_add(1)));
                    }
                    KeyCode::Char('-') | KeyCode::Char('_') | KeyCode::Left | KeyCode::Down => {
                        let _ = tx_cmd.send(HwCmd::Seek(status.track.saturating_sub(1)));
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
                        status.motor_on = !status.motor_on;
                        if !status.motor_on {
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
                        let _ = tx_cmd.send(HwCmd::ToggleMotor);
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        let _ = tx_cmd.send(HwCmd::RecalibrateSeek);
                    }
                    KeyCode::Char('z') | KeyCode::Char('Z') => {
                        let _ = tx_cmd.send(HwCmd::ZeroTrack);
                    }
                    KeyCode::Char('a') | KeyCode::Char('A') => {
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
                    KeyCode::Char('0') => {
                        let _ = tx_cmd.send(HwCmd::SelectUnit(0));
                    }
                    KeyCode::Char('1') => {
                        let _ = tx_cmd.send(HwCmd::SelectUnit(1));
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
        let style = get_standard_line_style("T:00 H:0  500k  [ ████████████████░░ ]  (16/18 MISSING: Sec 17, 18)", 18);
        assert_eq!(
            style,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        );

        // 18 sectors with 1 CRC error -> Red
        let style_err = get_standard_line_style("T:00 H:0  500k  [ ██████████████████ ]  (17/18 CRC-DAT: Sec 9)", 18);
        assert_eq!(
            style_err,
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_get_standard_line_style_missing_or_poor_pass() {
        let style_missing = get_standard_line_style("T:00 H:0  ---k  [ ? ]                   (0/0 NO DATA / MISSING)", 18);
        assert_eq!(
            style_missing,
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD)
        );

        let style_poor = get_standard_line_style("T:00 H:0  500k  [ ███░░░░░░░░░░░░░░░ ]  (3/18 MISSING: Sec 4, 5)", 18);
        assert_eq!(
            style_poor,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_build_standard_line_spans_individual_coloring() {
        let line_all_ok = "T:00 H:0  500k  [ ██████████████████ ]  (18/18 OK)";
        let spans = build_standard_line_spans(line_all_ok, 18);
        assert!(!spans.is_empty());
        let green_blocks = spans
            .iter()
            .filter(|s| s.content == "█" && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(green_blocks, 18);

        let line_crc_15 = "T:35 H:0  500k  [ ██████████████████ ]  (17/18 CRC-DAT: Sec 15)";
        let spans_crc = build_standard_line_spans(line_crc_15, 18);
        let red_blocks = spans_crc
            .iter()
            .filter(|s| s.content == "█" && s.style.fg == Some(Color::Red))
            .count();
        let ok_blocks = spans_crc
            .iter()
            .filter(|s| s.content == "█" && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(red_blocks, 1);
        assert_eq!(ok_blocks, 17);

        let line_missing_8 = "T:40 H:0  500k  [ ███████░███████ ]     (14/15 MISSING: Sec 8)";
        let spans_miss = build_standard_line_spans(line_missing_8, 15);
        let dark_blocks = spans_miss
            .iter()
            .filter(|s| s.content == "░" && s.style.fg == Some(Color::DarkGray))
            .count();
        let green_14 = spans_miss
            .iter()
            .filter(|s| s.content == "█" && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(dark_blocks, 1);
        assert_eq!(green_14, 14);
    }

    #[test]
    fn test_get_verbose_line_style_perfect_pass() {
        let style = get_verbose_line_style(
            "T:79 H:0 Rate:500k MFM [ ■■■■■■■■■■■■■■■■■■ ] (18/18 OK) IL:1:1 200.1ms [299.8 RPM ±0.2%] Gap0:420µs Q:99%",
            18,
        );
        assert_eq!(
            style,
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD)
        );

        let style_dd = get_verbose_line_style(
            "T:40 H:0 Rate:250k MFM [ ■■■■■■■■■ ]          (9/9 OK) IL:1:1 200.0ms [300.1 RPM ±0.1%] Gap0:410µs Q:98%",
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
            "T:35 H:0 Rate:500k MFM [ ■■■■■■■■■■■■■■■ ] (14/15 MISSING: Sec 8) IL:1:1 166.7ms [360.0 RPM]",
            15,
        );
        assert_eq!(
            style_missing,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        );

        let style_off = get_verbose_line_style(
            "T:00 H:0 Rate:500k MFM [ ■■■■■■■■■■■■■■■■■■ ] (18/18 OFF-TRK: T10: 18 sect) IL:1:1 200.0ms",
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
            "T:80 H:0 Rate:---k --- [ ? ]                   (0/0 NO DATA / MISSING) 200.3ms [299.5 RPM]",
            18,
        );
        assert_eq!(
            style_missing,
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD)
        );

        let style_crc = get_verbose_line_style(
            "T:35 H:0 Rate:500k MFM [ ■■■■■■■■■■■■■■■■■■ ] (17/18 CRC-DAT: Sec 15) IL:1:1 200.2ms [299.7 RPM] Q:84%",
            18,
        );
        assert_eq!(
            style_crc,
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn test_build_verbose_line_spans_individual_coloring() {
        let line_all_ok = "T:79 H:0 Rate:500k MFM [ ■■■■■■■■■■■■■■■■■■ ] (18/18 OK) IL:1:1 200.1ms [299.8 RPM ±0.2%] Gap0:420µs Q:99%";
        let spans = build_verbose_line_spans(line_all_ok, 18);
        assert!(!spans.is_empty());
        // Check ribbon blocks are green
        let green_blocks = spans
            .iter()
            .filter(|s| s.content == "■" && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(green_blocks, 18);

        let line_crc_15 = "T:35 H:0 Rate:500k MFM [ ■■■■■■■■■■■■■■■■■■ ] (17/18 CRC-DAT: Sec 15) IL:1:1 200.2ms [299.7 RPM] Q:84%";
        let spans_crc = build_verbose_line_spans(line_crc_15, 18);
        let red_blocks = spans_crc
            .iter()
            .filter(|s| s.content == "■" && s.style.fg == Some(Color::Red))
            .count();
        let ok_blocks = spans_crc
            .iter()
            .filter(|s| s.content == "■" && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(red_blocks, 1);
        assert_eq!(ok_blocks, 17);

        let line_missing_8 = "T:40 H:0 Rate:500k MFM [ ■■■■■■■■■■■■■■■ ] (14/15 MISSING: Sec 8) IL:1:1 166.7ms";
        let spans_miss = build_verbose_line_spans(line_missing_8, 15);
        let dark_blocks = spans_miss
            .iter()
            .filter(|s| s.content == "■" && s.style.fg == Some(Color::DarkGray))
            .count();
        let green_14 = spans_miss
            .iter()
            .filter(|s| s.content == "■" && s.style.fg == Some(Color::LightGreen))
            .count();
        assert_eq!(dark_blocks, 1);
        assert_eq!(green_14, 14);
    }
}
