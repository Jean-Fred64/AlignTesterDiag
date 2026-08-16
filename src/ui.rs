use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use crate::app::HeadSelection;
use crate::hw::HwActivity;

/// Builds the track ruler line with visual highlight for active tracks (0 to 83)
pub fn build_ruler_line(current_track: u8) -> Line<'static> {
    let mut spans = Vec::new();
    spans.push(Span::styled(" ", Style::default()));

    for t in 0..=83 {
        let ch = if t % 10 == 0 {
            ((t / 10) % 10).to_string()
        } else if t % 5 == 0 {
            "+".to_string()
        } else {
            ".".to_string()
        };

        let style = if t <= current_track {
            Style::default()
                .bg(Color::White)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(180, 80, 80))
        };

        spans.push(Span::styled(ch, style));
    }

    Line::from(spans)
}

/// Formats hardware access flags block:
/// - `f`: Format unimplemented (dimmed / dash `-`)
/// - `w`: Write (`-` if WP active / write protected; `w` if write enabled)
/// - `R`: Recalibrate (always highlighted)
/// - `z`: Zero Track (always highlighted)
/// - `d`: Manual density toggle reserved (dimmed / dash `-`)
pub fn format_flags_display(write_protect: bool) -> String {
    let f = "-";
    let w = if write_protect { "-" } else { "w" };
    let r = "R";
    let z = "z";
    let d = "-";
    format!("[{}{}{}{}{}]", f, w, r, z, d)
}

/// Builds styled spans for the hardware access flags block
pub fn build_flags_spans(write_protect: bool) -> Vec<Span<'static>> {
    vec![
        Span::styled("Flags: [", Style::default().fg(Color::White)),
        Span::styled("-", Style::default().fg(Color::DarkGray)),
        if write_protect {
            Span::styled("-", Style::default().fg(Color::DarkGray))
        } else {
            Span::styled(
                "w",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        },
        Span::styled(
            "R",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "z",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("-", Style::default().fg(Color::DarkGray)),
        Span::styled("]", Style::default().fg(Color::White)),
    ]
}

/// Builds the Write Protect status badge span
pub fn build_wp_span(write_protect: bool) -> Span<'static> {
    if write_protect {
        Span::styled(
            "WP: PROTECTED",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "WP: WRITE-ENABLED",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )
    }
}

/// Formats the RPM display string according to drive status
pub fn format_rpm_display(motor_on: bool, rpm: u32) -> String {
    if motor_on {
        if rpm > 0 {
            format!("{} RPM", rpm)
        } else {
            "... RPM".to_string()
        }
    } else {
        "--- RPM".to_string()
    }
}

/// Builds the Live RPM continuous measurement metric string:
/// `RPM: 300.1 (Avg: 300.0 | Min: 299.8 | Max: 300.2 | Jitter: ±0.07%)`
pub fn format_rpm_metric_line(meas: &crate::hw::RpmMeasurement) -> String {
    format!(
        "RPM: {:.1} (Avg: {:.1} | Min: {:.1} | Max: {:.1} | Jitter: ±{:.2}%)",
        meas.instant_rpm, meas.avg_rpm, meas.min_rpm, meas.max_rpm, meas.jitter_pct
    )
}

/// Builds styled spans for the Live RPM continuous measurement metric line
pub fn build_rpm_metric_spans(meas: &crate::hw::RpmMeasurement, target_rpm: f64) -> Vec<Span<'static>> {
    let target = if target_rpm > 0.0 { target_rpm } else { 300.0 };
    let dev_pct = if target > 0.0 {
        ((meas.instant_rpm - target) / target) * 100.0
    } else {
        0.0
    };

    let rpm_color = if dev_pct.abs() <= 0.5 {
        Color::LightGreen
    } else if dev_pct.abs() <= 1.5 {
        Color::Yellow
    } else {
        Color::Red
    };

    let jitter_color = if meas.jitter_pct <= 0.20 {
        Color::LightGreen
    } else if meas.jitter_pct <= 0.50 {
        Color::Yellow
    } else {
        Color::Red
    };

    vec![
        Span::styled("RPM: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{:.1} ", meas.instant_rpm),
            Style::default().fg(rpm_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "(Avg: {:.1} | Min: {:.1} | Max: {:.1} | Jitter: ",
                meas.avg_rpm, meas.min_rpm, meas.max_rpm
            ),
            Style::default().fg(Color::LightCyan),
        ),
        Span::styled(
            format!("±{:.2}%", meas.jitter_pct),
            Style::default().fg(jitter_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(")", Style::default().fg(Color::LightCyan)),
    ]
}

/// Builds a 21-character visual centering gauge: `[----|----▼----|----]`
/// - Green (`Color::LightGreen`) if deviation <= ±0.5%
/// - Yellow (`Color::Yellow`) if deviation <= ±1.5%
/// - Red (`Color::Red`) if out of tolerance (> ±1.5%)
pub fn build_rpm_centering_gauge(current_rpm: f64, target_rpm: f64) -> (String, Color) {
    let target = if target_rpm > 0.0 { target_rpm } else { 300.0 };
    let dev_pct = ((current_rpm - target) / target) * 100.0;

    let color = if dev_pct.abs() <= 0.5 {
        Color::LightGreen
    } else if dev_pct.abs() <= 1.5 {
        Color::Yellow
    } else {
        Color::Red
    };

    // 19 slots inside brackets [0..=18]:
    // Slot 0 = -2.0%, Slot 4 = -1.0% (|), Slot 9 = 0.0% Nominal (|), Slot 14 = +1.0% (|), Slot 18 = +2.0%
    let slot = (9.0 + (dev_pct / 2.0) * 9.0).round().clamp(0.0, 18.0) as usize;

    let mut chars: Vec<char> = "----|----|----|----".chars().collect();
    if slot < chars.len() {
        chars[slot] = '▼';
    }
    let inner: String = chars.into_iter().collect();
    (format!("[{}]", inner), color)
}

/// Builds styled spans for the visual centering gauge line
pub fn build_rpm_gauge_line(current_rpm: f64, target_rpm: f64) -> Line<'static> {
    let (gauge_str, color) = build_rpm_centering_gauge(current_rpm, target_rpm);
    Line::from(vec![
        Span::styled("Centering Gauge: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(
            gauge_str,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

/// Builds a text alignment gauge based on percentage
pub fn build_alignment_gauge(pct: f32) -> String {
    let total_bars = 20;
    let filled_bars = ((pct / 100.0) * total_bars as f32).round() as usize;
    let filled_bars = filled_bars.min(total_bars);
    let empty_bars = total_bars - filled_bars;
    format!("{}{}", "█".repeat(filled_bars), "-".repeat(empty_bars))
}

/// Helper to extract list of sector IDs from error text like "Sec 3, 15" or "Sec 8"
pub fn extract_sector_ids_from_error(line: &str, marker: &str) -> Vec<usize> {
    if let Some(pos) = line.find(marker) {
        let after = &line[pos + marker.len()..];
        let end = after.find(')').unwrap_or(after.len());
        let sec_str = &after[..end];
        return sec_str
            .split(',')
            .filter_map(|s| s.trim().parse::<usize>().ok())
            .collect();
    }
    Vec::new()
}

/// Builds rich styled spans for Standard / Normal mode lines, with individual coloring for each segment:
/// - 🟩 Green (`Color::LightGreen`): Sector read with valid CRC (`█`).
/// - 🟥 Red (`Color::Red`): CRC error sector (`█` in red) or corrupt/error (`?`).
/// - 🟨 Yellow (`Color::Yellow`): NO-DAM or DEL-DAM or warning.
/// - ⬜ Dark Gray (`Color::DarkGray`): Missing sector (`░`).
pub fn build_standard_line_spans(line: &str, _expected_count: u8) -> Vec<Span<'static>> {
    let mut spans = Vec::new();

    let open_bracket = line.find('[');
    let close_bracket = line.find(']');

    if let (Some(open_idx), Some(close_idx)) = (open_bracket, close_bracket) {
        if close_idx > open_idx {
            let prefix = &line[..open_idx];
            let ribbon_inner = line[open_idx + 1..close_idx].trim();
            let suffix = &line[close_idx + 1..];

            // 1. Prefix: "T:00 H:0  500k  "
            spans.push(Span::styled(
                prefix.to_string(),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ));

            // 2. Opening bracket
            spans.push(Span::styled("[ ", Style::default().fg(Color::White)));

            // 3. Segmented blocks
            if ribbon_inner == "?" {
                spans.push(Span::styled(
                    "?",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ));
            } else {
                let crc_dat_secs = extract_sector_ids_from_error(line, "CRC-DAT: Sec ");
                let crc_id_secs = extract_sector_ids_from_error(line, "CRC-ID: Sec ");
                let no_dam_secs = extract_sector_ids_from_error(line, "NO-DAM: Sec ");
                let del_dam_secs = extract_sector_ids_from_error(line, "DEL-DAM: Sec ");
                let missing_secs = extract_sector_ids_from_error(line, "MISSING: Sec ");

                let is_unformatted = line.contains("NO DATA") || (line.contains("MISSING") && !line.contains("MISSING: Sec"));

                let mut block_idx = 1usize;
                for ch in ribbon_inner.chars() {
                    if ch == '█' {
                        let style = if is_unformatted {
                            Style::default().fg(Color::DarkGray)
                        } else if crc_dat_secs.contains(&block_idx) || crc_id_secs.contains(&block_idx) {
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                        } else if no_dam_secs.contains(&block_idx) || del_dam_secs.contains(&block_idx) {
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else if missing_secs.contains(&block_idx) {
                            Style::default().fg(Color::DarkGray)
                        } else {
                            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
                        };

                        spans.push(Span::styled(ch.to_string(), style));
                        block_idx += 1;
                    } else if ch == '░' {
                        let style = Style::default().fg(Color::DarkGray);
                        spans.push(Span::styled(ch.to_string(), style));
                        block_idx += 1;
                    } else if !ch.is_whitespace() {
                        spans.push(Span::styled(ch.to_string(), Style::default().fg(Color::White)));
                    }
                }
            }

            // 4. Closing bracket
            spans.push(Span::styled(" ]", Style::default().fg(Color::White)));

            // 5. Suffix (Status counter e.g. "(18/18 OK)" or "(17/18 CRC-DAT: Sec 15)")
            let mut remaining = suffix;
            let leading_spaces: String = remaining.chars().take_while(|c| c.is_whitespace()).collect();
            if !leading_spaces.is_empty() {
                spans.push(Span::styled(leading_spaces.clone(), Style::default()));
                remaining = &remaining[leading_spaces.len()..];
            }

            for token in remaining.split_whitespace() {
                let style = if token.starts_with('(') && token.ends_with(')') {
                    if token.contains("OK") {
                        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
                    } else if token.contains("CRC-") || token.contains("NO DATA") || token.contains("MISSING") || token.contains("BAD") {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else if token.contains("NO-DAM") || token.contains("DEL-DAM") || token.contains("OFF-TRK") {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    }
                } else if token.starts_with('(') {
                    if token.contains("CRC-") || token.contains("NO") || token.contains("MISSING") || token.contains("BAD") {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else if token.contains("NO-DAM") || token.contains("DEL-DAM") || token.contains("OFF-TRK") {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    }
                } else if token.ends_with(')') {
                    if token.contains("OK") {
                        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
                    } else if token.contains('/') && !token.contains("CRC") && !token.contains("NO") && !token.contains("MISSING") && !token.contains("BAD") {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    }
                } else {
                    Style::default().fg(Color::White)
                };

                spans.push(Span::styled(format!(" {}", token), style));
            }

            return spans;
        }
    }

    let line_style = get_standard_line_style(line, _expected_count);
    vec![Span::styled(line.to_string(), line_style)]
}

/// Formats dynamic visual coloring for Standard mode lines:
/// - 🟢 Bright Green (LightGreen Bold): Perfect pass (100% expected sectors read with CRC OK: 9/9, 15/15, 18/18)
/// - 🟡 Yellow / Orange (Yellow Bold): Close to count (e.g. >= 50% or partial errors/missing)
/// - 🔴 Red (Red Bold): Far from count (< 50% expected sectors or major errors / missing)
pub fn get_standard_line_style(line: &str, _expected_count: u8) -> Style {
    if line.contains("CRC-DAT")
        || line.contains("CRC-ID")
        || line.contains("CRC ERR")
        || line.contains("NO DATA")
        || line.contains("NO SECTORS")
        || line.contains("ERR(")
        || line.contains("BAD")
        || (line.contains("[ ? ]") && line.contains("MISSING"))
    {
        return Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
    }

    if line.contains("NO-DAM")
        || line.contains("DEL-DAM")
        || line.contains("OFF-TRK")
        || line.contains("MISSING: Sec")
    {
        return Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    }

    if line.contains(" OK)") || line.contains("(OK)") {
        return Style::default()
            .fg(Color::LightGreen)
            .add_modifier(Modifier::BOLD);
    }

    Style::default().fg(Color::White)
}

/// Builds rich styled spans for Verbose mode lines, with individual coloring for each ribbon block:
/// - 🟩 Green (`Color::LightGreen`): Sector read with valid IDAM and Data CRC.
/// - 🟥 Red (`Color::Red`): CRC error (Header IDAM or Data CRC).
/// - 🟨 Yellow (`Color::Yellow`): NO-DAM or DEL-DAM (deleted data mark).
/// - ⬜ Dark Gray (`Color::DarkGray`): Missing sector.
pub fn build_verbose_line_spans(line: &str, _expected_count: u8) -> Vec<Span<'static>> {
    let mut spans = Vec::new();

    let open_bracket = line.find('[');
    let close_bracket = line.find(']');

    if let (Some(open_idx), Some(close_idx)) = (open_bracket, close_bracket) {
        if close_idx > open_idx {
            let prefix = &line[..open_idx];
            let ribbon_inner = line[open_idx + 1..close_idx].trim();
            let suffix = &line[close_idx + 1..];

            // 1. Prefix: "T:79 H:0 Rate:500k MFM "
            spans.push(Span::styled(
                prefix.to_string(),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ));

            // 2. Opening bracket
            spans.push(Span::styled("[ ", Style::default().fg(Color::White)));

            // 3. Ribbon blocks
            if ribbon_inner == "?" {
                spans.push(Span::styled(
                    "?",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ));
            } else {
                let crc_dat_secs = extract_sector_ids_from_error(line, "CRC-DAT: Sec ");
                let crc_id_secs = extract_sector_ids_from_error(line, "CRC-ID: Sec ");
                let no_dam_secs = extract_sector_ids_from_error(line, "NO-DAM: Sec ");
                let del_dam_secs = extract_sector_ids_from_error(line, "DEL-DAM: Sec ");
                let missing_secs = extract_sector_ids_from_error(line, "MISSING: Sec ");

                let is_unformatted = line.contains("NO DATA") || (line.contains("MISSING") && !line.contains("MISSING: Sec"));

                let mut block_idx = 1usize;
                for ch in ribbon_inner.chars() {
                    if ch == '■' {
                        let style = if is_unformatted {
                            Style::default().fg(Color::DarkGray)
                        } else if crc_dat_secs.contains(&block_idx) || crc_id_secs.contains(&block_idx) {
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                        } else if no_dam_secs.contains(&block_idx) || del_dam_secs.contains(&block_idx) {
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else if missing_secs.contains(&block_idx) {
                            Style::default().fg(Color::DarkGray)
                        } else {
                            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
                        };

                        spans.push(Span::styled(ch.to_string(), style));
                        block_idx += 1;
                    } else if !ch.is_whitespace() {
                        spans.push(Span::styled(ch.to_string(), Style::default().fg(Color::White)));
                    }
                }
            }

            // 4. Closing bracket
            spans.push(Span::styled(" ]", Style::default().fg(Color::White)));

            // 5. Suffix (Ratio, IL, timing, RPM, Gap0, Quality)
            let mut remaining = suffix;
            let leading_spaces: String = remaining.chars().take_while(|c| c.is_whitespace()).collect();
            if !leading_spaces.is_empty() {
                spans.push(Span::styled(leading_spaces.clone(), Style::default()));
                remaining = &remaining[leading_spaces.len()..];
            }

            for token in remaining.split_whitespace() {
                let style = if token.starts_with('(') && token.ends_with(')') {
                    if token.contains("OK") {
                        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
                    } else if token.contains("CRC-") || token.contains("NO DATA") || token.contains("MISSING") {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else if token.contains("NO-DAM") || token.contains("DEL-DAM") || token.contains("OFF-TRK") {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    }
                } else if token.starts_with('(') {
                    if token.contains("CRC-") || token.contains("NO") || token.contains("MISSING") {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else if token.contains("NO-DAM") || token.contains("DEL-DAM") || token.contains("OFF-TRK") {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    }
                } else if token.ends_with(')') {
                    if token.contains("OK") {
                        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
                    } else if token.contains('/') && !token.contains("CRC") && !token.contains("NO") && !token.contains("MISSING") {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    }
                } else if token.starts_with("IL:") {
                    if token == "IL:---" {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::LightCyan)
                    }
                } else if token.starts_with("Gap0:") {
                    if token == "Gap0:----" {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::LightMagenta)
                    }
                } else if let Some(stripped) = token.strip_prefix("Q:") {
                    if token == "Q:--%" {
                        Style::default().fg(Color::DarkGray)
                    } else {
                        let num: u8 = stripped.trim_end_matches('%').parse().unwrap_or(0);
                        if num >= 85 {
                            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
                        } else if num >= 60 {
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                        }
                    }
                } else {
                    Style::default().fg(Color::White)
                };

                spans.push(Span::styled(format!(" {}", token), style));
            }

            return spans;
        }
    }

    let line_style = get_verbose_line_style(line, _expected_count);
    vec![Span::styled(line.to_string(), line_style)]
}

/// Formats dynamic visual coloring for Verbose mode lines:
/// - 🟢 Bright Green (LightGreen Bold): Perfect pass (100% expected sectors read with CRC OK and (OK))
/// - 🟡 Yellow / Orange (Yellow Bold): Partial pass with missing sectors, weak signal, or off-track
/// - 🔴 Red (Red Bold): CRC error (CRC-DAT / CRC-ID / CRC ERR), header error (?), or no sectors (NO DATA / MISSING)
pub fn get_verbose_line_style(line: &str, _expected_count: u8) -> Style {
    if line.contains("CRC-DAT")
        || line.contains("CRC-ID")
        || line.contains("CRC ERR")
        || line.contains("NO DATA")
        || line.contains("NO SECTORS")
        || line.contains("ERR(")
        || (line.contains("[ ? ]") && line.contains("MISSING"))
    {
        return Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
    }

    if line.contains("NO-DAM")
        || line.contains("DEL-DAM")
        || line.contains("OFF-TRK")
        || line.contains("MISSING: Sec")
        || line.contains("[T")
    {
        return Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    }

    if line.contains(" OK)") || line.contains("(OK)") {
        return Style::default()
            .fg(Color::LightGreen)
            .add_modifier(Modifier::BOLD);
    }

    Style::default().fg(Color::White)
}

/// Formats the animated activity badge
pub fn format_activity_badge(activity: HwActivity, io_cycle: u64) -> (Span<'static>, Span<'static>) {
    const SPINNER: &[char] = &['|', '/', '-', '\\'];
    match activity {
        HwActivity::MeasuringRpm => {
            let spin = SPINNER[(io_cycle as usize) % SPINNER.len()];
            (
                Span::styled(
                    "► ",
                    Style::default()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[TACHOMETER / MOTOR RPM TEST {}]", spin),
                    Style::default()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                ),
            )
        }
        HwActivity::ReadingAnalyzing => {
            let spin = SPINNER[(io_cycle as usize) % SPINNER.len()];
            (
                Span::styled(
                    "► ",
                    Style::default()
                        .fg(Color::LightGreen)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[READING / ANALYZING {}]", spin),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            )
        }
        HwActivity::Seeking => (
            Span::styled(
                "► ",
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "[SEEKING...]",
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ),
        HwActivity::Stopped => (
            Span::styled(
                "■ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "[STOPPED / MOTOR OFF]",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ),
        HwActivity::WaitingPort => (
            Span::styled(
                "► ",
                Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "[CONNECTING...]",
                Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD),
            ),
        ),
        HwActivity::Idle => (
            Span::styled("► ", Style::default().fg(Color::White)),
            Span::styled("[IDLE / READY]", Style::default().fg(Color::White)),
        ),
    }
}

/// Formats the head selection display string for status summaries
pub fn format_head_display(head_select: HeadSelection, active_head: u8) -> String {
    match head_select {
        HeadSelection::Head0 => String::from("Head 0"),
        HeadSelection::Head1 => String::from("Head 1"),
        HeadSelection::Both => format!("BOTH (0+1) [Active: H:{}]", active_head),
    }
}

/// Formats the short head string for the top header banner
pub fn format_head_header_str(head_select: HeadSelection, active_head: u8) -> String {
    match head_select {
        HeadSelection::Head0 => String::from("H0"),
        HeadSelection::Head1 => String::from("H1"),
        HeadSelection::Both => format!("HB(H{})", active_head),
    }
}

/// Builds the dedicated 2-line fixed persistent display for Both mode:
/// - Line 1: Result for Head 0
/// - Line 2: Result for Head 1
///
/// An active pointer `► ` is placed on the head currently under acquisition (`status.head`), while inactive head uses `"  "`.
pub fn build_both_mode_display_lines(status: &crate::hw::DriveStatus) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(2);
    let expected = if status.sector_count > 0 { status.sector_count } else { 18 };

    let format_head_line = |head_idx: u8| -> Line<'static> {
        let is_active = status.head == head_idx && (status.analyzing || status.in_progress_pass);
        let prefix_span = if is_active {
            Span::styled(
                "► ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("  ", Style::default())
        };

        let pass_opt = if head_idx == 0 {
            &status.last_pass_h0
        } else {
            &status.last_pass_h1
        };

        let content_spans = if let Some(pass) = pass_opt {
            if status.verbose_mode {
                build_verbose_line_spans(&pass.line_verbose, pass.expected_count)
            } else {
                build_standard_line_spans(&pass.line_standard, pass.expected_count)
            }
        } else if is_active && !status.sector_log.is_empty() {
            let last_log = status.sector_log.last().unwrap();
            if status.verbose_mode {
                build_verbose_line_spans(last_log, expected)
            } else {
                build_standard_line_spans(last_log, expected)
            }
        } else {
            let empty_blocks = "░".repeat(expected as usize);
            let raw_ribbon = format!("[ {} ]", empty_blocks);
            let ribbon_col = format!("{:<22}", raw_ribbon);
            let status_col = format!("( 0/{})", expected);
            let line_str = if status.verbose_mode {
                format!(
                    "T:{:02} H:{} Rate:{}k MFM {}  {} IL:--- Gap0:---- Q:--%",
                    status.track, head_idx, status.bitrate, ribbon_col, status_col
                )
            } else {
                format!(
                    "T:{:02} H:{}  {}k  {}   {}",
                    status.track, head_idx, status.bitrate, ribbon_col, status_col
                )
            };
            if status.verbose_mode {
                build_verbose_line_spans(&line_str, expected)
            } else {
                build_standard_line_spans(&line_str, expected)
            }
        };

        let mut spans = vec![prefix_span];
        spans.extend(content_spans);
        Line::from(spans)
    };

    lines.push(format_head_line(0));
    lines.push(format_head_line(1));
    lines
}


