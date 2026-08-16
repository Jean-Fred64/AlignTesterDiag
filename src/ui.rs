use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
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
/// - `f`: Format non implémenté (grisé / tiret `-`)
/// - `w`: Write (`-` si WP actif / écriture verrouillée ; `w` si écriture autorisée)
/// - `R`: Recalibrate (toujours allumé en surbrillance)
/// - `z`: Zero Track (toujours allumé en surbrillance)
/// - `d`: Bascule manuelle de densité réservée (grisé / tiret `-`)
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
                    } else {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    }
                } else if token.ends_with(')') {
                    if token.contains("OK") {
                        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
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
                    } else {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    }
                } else if token.ends_with(')') {
                    if token.contains("OK") {
                        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    }
                } else if token.starts_with("IL:") {
                    Style::default().fg(Color::LightCyan)
                } else if token.ends_with("ms") {
                    Style::default().fg(Color::White)
                } else if token.starts_with('[') || token.ends_with(']') || token.contains("RPM") {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else if token.starts_with("Gap0:") {
                    Style::default().fg(Color::LightMagenta)
                } else if token.starts_with("Q:") {
                    let num: u8 = token[2..].trim_end_matches('%').parse().unwrap_or(0);
                    if num >= 95 {
                        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
                    } else if num >= 85 {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
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
