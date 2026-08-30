use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};
use crate::app::{App, HeadSelection, PendingConfirmation, RangeField, RangeModalKind};
use crate::hw::{FsInitMode, HwActivity, PresetProfile, TrackRange};

/// Builds the track ruler line with visual highlight for active tracks (0 to 83)
pub fn build_ruler_line(current_track: u8) -> Line<'static> {
    const DIGIT_CHARS: [&str; 9] = ["0", "1", "2", "3", "4", "5", "6", "7", "8"];
    let mut spans = Vec::with_capacity(85);
    spans.push(Span::styled(" ", Style::default()));

    for t in 0..=83 {
        let ch = if t % 10 == 0 {
            let digit = ((t / 10) % 10) as usize;
            if digit < DIGIT_CHARS.len() {
                DIGIT_CHARS[digit]
            } else {
                "0"
            }
        } else if t % 5 == 0 {
            "+"
        } else {
            "."
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

/// Formats the audio radar beep status string
pub fn format_beep_status(beep_enabled: bool) -> &'static str {
    if beep_enabled {
        "ON (Radar)"
    } else {
        "OFF"
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

/// Builds styled spans for labeled RPM metric lines (e.g. in Dual or specific modes)
pub fn build_labeled_rpm_metric_spans(
    label: &str,
    meas: &crate::hw::RpmMeasurement,
    target_rpm: f64,
) -> Vec<Span<'static>> {
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
        Span::styled(format!("{}: ", label), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{:.1} RPM ", meas.instant_rpm),
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

/// Builds styled spans for the active RPM Measurement Mode indicator and shortcut guide
pub fn build_rpm_mode_spans(mode: crate::hw::RpmMode) -> Vec<Span<'static>> {
    vec![
        Span::styled("► Measurement Mode       : ", Style::default().fg(Color::White)),
        Span::styled(
            format!("[{}] ", mode.as_str()),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "(Press [I] to cycle Auto / HW Index / SW Sync / Dual)",
            Style::default().fg(Color::DarkGray),
        ),
    ]
}

/// Builds styled spans for Dual Mode speed differential between HW mechanical index and SW magnetic sync
pub fn build_rpm_dual_differential_spans(
    hw_meas: &crate::hw::RpmMeasurement,
    sw_meas: &crate::hw::RpmMeasurement,
) -> Vec<Span<'static>> {
    let diff = hw_meas.instant_rpm - sw_meas.instant_rpm;
    let sign = if diff >= 0.0 { "+" } else { "" };
    let diff_pct = if sw_meas.instant_rpm > 0.0 {
        (diff / sw_meas.instant_rpm) * 100.0
    } else {
        0.0
    };
    let diff_color = if diff.abs() <= 0.5 {
        Color::LightGreen
    } else if diff.abs() <= 1.5 {
        Color::Yellow
    } else {
        Color::Red
    };
    vec![
        Span::styled("► Speed Differential (Δ)  : ", Style::default().fg(Color::White)),
        Span::styled(
            format!("Δ = {}{:.1} RPM ({}{:.2}% mechanical vs magnetic)", sign, diff, sign, diff_pct),
            Style::default().fg(diff_color).add_modifier(Modifier::BOLD),
        ),
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

    let mut s = String::with_capacity(24);
    s.push('[');
    const TEMPLATE: &[u8; 19] = b"----|----|----|----";
    for (i, &b) in TEMPLATE.iter().enumerate() {
        if i == slot {
            s.push('▼');
        } else {
            s.push(b as char);
        }
    }
    s.push(']');
    (s, color)
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
    let mut s = String::with_capacity(64);
    for _ in 0..filled_bars {
        s.push('█');
    }
    for _ in 0..empty_bars {
        s.push('-');
    }
    s
}

/// Helper to extract list of sector IDs from error text like "Sec 3, 15", "Sec 8", "Sec C1, C5", "Sec 41"
pub fn extract_sector_ids_from_error(line: &str, marker: &str) -> Vec<usize> {
    if let Some(pos) = line.find(marker) {
        let after = &line[pos + marker.len()..];
        let end = after.find(')').unwrap_or(after.len());
        let sec_str = &after[..end];
        return sec_str
            .split(',')
            .filter_map(|s| {
                let trimmed = s.trim();
                // If it's a CPC hex sector ID (0x41..=0x4A or 0xC1..=0xCA), parse as hex
                if let Ok(v) = usize::from_str_radix(trimmed, 16) {
                    if (0x41..=0x4A).contains(&v) || (0xC1..=0xCA).contains(&v) {
                        return Some(v);
                    }
                }
                // Otherwise standard decimal ID (0..=36)
                if let Ok(v) = trimmed.parse::<usize>() {
                    return Some(v);
                }
                // Fallback to hex if decimal failed (e.g. C1 without 0x)
                usize::from_str_radix(trimmed, 16).ok()
            })
            .collect();
    }
    Vec::new()
}

/// Formats the header title of the disk format profile for the top header bar
pub fn format_disk_format_header(
    format: crate::hw::DiskFormat,
    bitrate: u16,
    sector_count: u8,
) -> String {
    match format {
        crate::hw::DiskFormat::AmigaDos => {
            if bitrate == 500 {
                "AmigaDOS HD 22x512".to_string()
            } else {
                "AmigaDOS DD 11x512".to_string()
            }
        }
        crate::hw::DiskFormat::AtariSt => {
            if sector_count >= 11 {
                "Atari ST 11x512".to_string()
            } else if sector_count == 10 {
                "Atari ST 10x512".to_string()
            } else {
                "Atari ST 9x512".to_string()
            }
        }
        crate::hw::DiskFormat::AmstradCpcData => {
            if sector_count >= 10 {
                "CPC Data 10x512".to_string()
            } else {
                "CPC Data 9x512".to_string()
            }
        }
        crate::hw::DiskFormat::AmstradCpcSystem => "CPC System 9x512".to_string(),
        crate::hw::DiskFormat::IbmPc => {
            if bitrate == 500 {
                if sector_count == 15 {
                    "PC HD 15x512 (1.2M)".to_string()
                } else {
                    "PC HD 18x512 (1.44M)".to_string()
                }
            } else if bitrate == 300 {
                "PC DD 9x512 (360K)".to_string()
            } else {
                "PC DD 9x512 (720K)".to_string()
            }
        }
        crate::hw::DiskFormat::AutoDetect => {
            if bitrate == 500 {
                if sector_count == 15 {
                    "PC HD 15x512 (1.2M)".to_string()
                } else if sector_count == 22 {
                    "AmigaDOS HD 22x512".to_string()
                } else {
                    "PC HD 18x512 (1.44M)".to_string()
                }
            } else if bitrate == 300 {
                "PC DD 9x512 (360K)".to_string()
            } else if sector_count == 11 {
                "AmigaDOS DD 11x512".to_string()
            } else if sector_count == 10 {
                "Atari ST 10x512".to_string()
            } else {
                "PC DD 9x512 (720K)".to_string()
            }
        }
    }
}

/// Builds rich styled spans for Standard / Normal mode lines, with individual coloring for each segment:
/// - 🟩 Green (`Color::LightGreen`): Sector read with valid CRC (`■ `).
/// - 🟥 Red (`Color::LightRed`): CRC error sector (`■ ` in red) or corrupt/error (`? `).
/// - 🟨 Yellow (`Color::Yellow`): NO-DAM or DEL-DAM or warning.
/// - ⬜ Dark Gray (`Color::DarkGray`): Missing sector (`░ `).
pub fn build_standard_line_spans(line: &str, expected_count: u8) -> Vec<Span<'static>> {
    build_standard_line_spans_with_color(line, expected_count, Color::LightGreen)
}

/// Builds rich styled spans for Standard / Normal mode lines with a configurable valid block color (for decay effects):
pub fn build_standard_line_spans_with_color(
    line: &str,
    _expected_count: u8,
    valid_color: Color,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();

    let open_bracket = line.find('[');
    let close_bracket = line.find(']');

    if let (Some(open_idx), Some(close_idx)) = (open_bracket, close_bracket) {
        if close_idx > open_idx {
            let prefix = &line[..open_idx];
            let ribbon_inner = line[open_idx + 1..close_idx].trim();
            let suffix = &line[close_idx + 1..];

            // 1. Prefix: "T:00 H:0 Rate:500k MFM " (or legacy "T:00 H:0  500k  ")
            spans.push(Span::styled(
                prefix.to_string(),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ));

            // 2. Opening bracket
            spans.push(Span::styled("[ ", Style::default().fg(Color::White)));

            // 3. Segmented blocks
            if ribbon_inner == "?" {
                spans.push(Span::styled(
                    "? ",
                    Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                ));
            } else {
                let crc_dat_secs = extract_sector_ids_from_error(line, "CRC-DAT: Sec ");
                let crc_id_secs = extract_sector_ids_from_error(line, "CRC-ID: Sec ");
                let no_dam_secs = extract_sector_ids_from_error(line, "NO-DAM: Sec ");
                let del_dam_secs = extract_sector_ids_from_error(line, "DEL-DAM: Sec ");
                let missing_secs = extract_sector_ids_from_error(line, "MISSING: Sec ");

                let is_cpc_data = line.contains("Sec C") || (crc_dat_secs.iter().any(|&s| (0xC1..=0xCA).contains(&s)));
                let is_cpc_sys = line.contains("Sec 4") || (crc_dat_secs.iter().any(|&s| (0x41..=0x4A).contains(&s)));
                let is_amiga_zero = line.contains("Sec 0") || crc_dat_secs.contains(&0) || crc_id_secs.contains(&0) || missing_secs.contains(&0);

                let is_unformatted = line.contains("NO DATA") || line.contains("NO DISK") || (line.contains("MISSING") && !line.contains("MISSING: Sec"));
                let is_misaligned = line.contains("MISALIGNED");

                let mut block_idx = 1usize;
                for ch in ribbon_inner.chars() {
                    if ch == '█' || ch == '■' {
                        let sec_id = if is_cpc_data {
                            0xC0 + block_idx
                        } else if is_cpc_sys {
                            0x40 + block_idx
                        } else if is_amiga_zero {
                            block_idx.saturating_sub(1)
                        } else {
                            block_idx
                        };

                        let is_crc_dat = crc_dat_secs.contains(&sec_id) || crc_dat_secs.contains(&block_idx);
                        let is_crc_id = crc_id_secs.contains(&sec_id) || crc_id_secs.contains(&block_idx);
                        let is_no_dam = no_dam_secs.contains(&sec_id) || no_dam_secs.contains(&block_idx);
                        let is_del_dam = del_dam_secs.contains(&sec_id) || del_dam_secs.contains(&block_idx);
                        let is_missing = missing_secs.contains(&sec_id) || missing_secs.contains(&block_idx);

                        let style = if is_unformatted {
                            Style::default().fg(Color::DarkGray)
                        } else if is_misaligned {
                            Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD)
                        } else if is_crc_dat || is_crc_id {
                            Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
                        } else if is_no_dam || is_del_dam {
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else if is_missing {
                            Style::default().fg(Color::DarkGray)
                        } else {
                            Style::default().fg(valid_color).add_modifier(Modifier::BOLD)
                        };

                        spans.push(Span::styled("■ ", style));
                        block_idx += 1;
                    } else if ch == '░' {
                        let style = Style::default().fg(Color::DarkGray);
                        spans.push(Span::styled("░ ", style));
                        block_idx += 1;
                    } else if ch == '?' {
                        spans.push(Span::styled("? ", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)));
                        block_idx += 1;
                    } else if !ch.is_whitespace() {
                        spans.push(Span::styled(format!("{} ", ch), Style::default().fg(Color::White)));
                    }
                }
            }

            // 4. Closing bracket
            spans.push(Span::styled("]", Style::default().fg(Color::White)));

            // 5. Suffix (Status counter e.g. "(18/18 OK)" or "(17/18 CRC-DAT: Sec 15)")
            let mut remaining = suffix;
            let leading_spaces: String = remaining.chars().take_while(|c| c.is_whitespace()).collect();
            if !leading_spaces.is_empty() {
                spans.push(Span::styled(leading_spaces.clone(), Style::default()));
                remaining = &remaining[leading_spaces.len()..];
            }

            for token in remaining.split_whitespace() {
                let style = if token.starts_with('(') && token.ends_with(')') {
                    if token.contains("MISALIGNED") {
                        Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD)
                    } else if token.contains("OK") {
                        Style::default().fg(valid_color).add_modifier(Modifier::BOLD)
                    } else if token.contains("CRC-") || token.contains("NO DATA") || token.contains("NO DISK") || token.contains("MISSING") || token.contains("BAD") {
                        Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
                    } else if token.contains("NO-DAM") || token.contains("DEL-DAM") || token.contains("OFF-TRK") {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    }
                } else if token.starts_with('(') {
                    if token.contains("MISALIGNED") {
                        Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD)
                    } else if token.contains("CRC-") || token.contains("NO") || token.contains("MISSING") || token.contains("BAD") {
                        Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
                    } else if token.contains("NO-DAM") || token.contains("DEL-DAM") || token.contains("OFF-TRK") {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    }
                } else if token.ends_with(')') {
                    if token.contains("MISALIGNED") {
                        Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD)
                    } else if token.contains("OK") {
                        Style::default().fg(valid_color).add_modifier(Modifier::BOLD)
                    } else if token.contains('/') && !token.contains("CRC") && !token.contains("NO") && !token.contains("MISSING") && !token.contains("BAD") {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
                    }
                } else if token.contains("MISALIGNED") {
                    Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD)
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
/// - 🔴 Red (LightRed Bold): Far from count (< 50% expected sectors or major errors / missing / no disk)
pub fn get_standard_line_style(line: &str, _expected_count: u8) -> Style {
    if line.contains("CRC-DAT")
        || line.contains("CRC-ID")
        || line.contains("CRC ERR")
        || line.contains("NO DATA")
        || line.contains("NO DISK")
        || line.contains("NO SECTORS")
        || line.contains("ERR(")
        || line.contains("BAD")
        || (line.contains("[ ? ]") && (line.contains("MISSING") || line.contains("NO DATA") || line.contains("NO DISK")))
    {
        return Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD);
    }

    if line.contains("MISALIGNED") {
        return Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD);
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
/// - 🟩 Green (`Color::LightGreen`): Sector read with valid IDAM and Data CRC (`■ `).
/// - 🟥 Red (`Color::LightRed`): CRC error (Header IDAM or Data CRC, `■ ` in red).
/// - 🟨 Yellow (`Color::Yellow`): NO-DAM or DEL-DAM (deleted data mark).
/// - ⬜ Dark Gray (`Color::DarkGray`): Missing sector (`░ `).
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
                    "? ",
                    Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                ));
            } else {
                let crc_dat_secs = extract_sector_ids_from_error(line, "CRC-DAT: Sec ");
                let crc_id_secs = extract_sector_ids_from_error(line, "CRC-ID: Sec ");
                let no_dam_secs = extract_sector_ids_from_error(line, "NO-DAM: Sec ");
                let del_dam_secs = extract_sector_ids_from_error(line, "DEL-DAM: Sec ");
                let missing_secs = extract_sector_ids_from_error(line, "MISSING: Sec ");

                let is_cpc_data = line.contains("Sec C") || (crc_dat_secs.iter().any(|&s| (0xC1..=0xCA).contains(&s)));
                let is_cpc_sys = line.contains("Sec 4") || (crc_dat_secs.iter().any(|&s| (0x41..=0x4A).contains(&s)));
                let is_amiga_zero = line.contains("Sec 0") || crc_dat_secs.contains(&0) || crc_id_secs.contains(&0) || missing_secs.contains(&0);

                let is_unformatted = line.contains("NO DATA") || line.contains("NO DISK") || (line.contains("MISSING") && !line.contains("MISSING: Sec"));
                let is_misaligned = line.contains("MISALIGNED");

                let mut block_idx = 1usize;
                for ch in ribbon_inner.chars() {
                    if ch == '■' || ch == '█' {
                        let sec_id = if is_cpc_data {
                            0xC0 + block_idx
                        } else if is_cpc_sys {
                            0x40 + block_idx
                        } else if is_amiga_zero {
                            block_idx.saturating_sub(1)
                        } else {
                            block_idx
                        };

                        let is_crc_dat = crc_dat_secs.contains(&sec_id) || crc_dat_secs.contains(&block_idx);
                        let is_crc_id = crc_id_secs.contains(&sec_id) || crc_id_secs.contains(&block_idx);
                        let is_no_dam = no_dam_secs.contains(&sec_id) || no_dam_secs.contains(&block_idx);
                        let is_del_dam = del_dam_secs.contains(&sec_id) || del_dam_secs.contains(&block_idx);
                        let is_missing = missing_secs.contains(&sec_id) || missing_secs.contains(&block_idx);

                        let style = if is_unformatted {
                            Style::default().fg(Color::DarkGray)
                        } else if is_misaligned {
                            Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD)
                        } else if is_crc_dat || is_crc_id {
                            Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
                        } else if is_no_dam || is_del_dam {
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                        } else if is_missing {
                            Style::default().fg(Color::DarkGray)
                        } else {
                            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
                        };

                        spans.push(Span::styled("■ ", style));
                        block_idx += 1;
                    } else if ch == '░' {
                        let style = Style::default().fg(Color::DarkGray);
                        spans.push(Span::styled("░ ", style));
                        block_idx += 1;
                    } else if ch == '?' {
                        spans.push(Span::styled("? ", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)));
                        block_idx += 1;
                    } else if !ch.is_whitespace() {
                        spans.push(Span::styled(format!("{} ", ch), Style::default().fg(Color::White)));
                    }
                }
            }

            // 4. Closing bracket
            spans.push(Span::styled("]", Style::default().fg(Color::White)));

            // 5. Suffix (Ratio, IL, timing, RPM, Gap0, Quality)
            let mut remaining = suffix;
            let leading_spaces: String = remaining.chars().take_while(|c| c.is_whitespace()).collect();
            if !leading_spaces.is_empty() {
                spans.push(Span::styled(leading_spaces.clone(), Style::default()));
                remaining = &remaining[leading_spaces.len()..];
            }

            for token in remaining.split_whitespace() {
                let style = if token.starts_with('(') && token.ends_with(')') {
                    if token.contains("MISALIGNED") {
                        Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD)
                    } else if token.contains("OK") {
                        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
                    } else if token.contains("CRC-") || token.contains("NO DATA") || token.contains("NO DISK") || token.contains("MISSING") {
                        Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
                    } else if token.contains("NO-DAM") || token.contains("DEL-DAM") || token.contains("OFF-TRK") {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    }
                } else if token.starts_with('(') {
                    if token.contains("MISALIGNED") {
                        Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD)
                    } else if token.contains("CRC-") || token.contains("NO") || token.contains("MISSING") {
                        Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
                    } else if token.contains("NO-DAM") || token.contains("DEL-DAM") || token.contains("OFF-TRK") {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    }
                } else if token.ends_with(')') {
                    if token.contains("MISALIGNED") {
                        Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD)
                    } else if token.contains("OK") {
                        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
                    } else if token.contains('/') && !token.contains("CRC") && !token.contains("NO") && !token.contains("MISSING") {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
                    }
                } else if token.contains("MISALIGNED") {
                    Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD)
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
                            Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
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
/// - 🔴 Red (LightRed Bold): CRC error (CRC-DAT / CRC-ID / CRC ERR), header error (?), or no sectors (NO DATA / NO DISK / MISSING)
pub fn get_verbose_line_style(line: &str, _expected_count: u8) -> Style {
    if line.contains("CRC-DAT")
        || line.contains("CRC-ID")
        || line.contains("CRC ERR")
        || line.contains("NO DATA")
        || line.contains("NO DISK")
        || line.contains("NO SECTORS")
        || line.contains("ERR(")
        || (line.contains("[ ? ]") && (line.contains("MISSING") || line.contains("NO DATA") || line.contains("NO DISK")))
    {
        return Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD);
    }

    if line.contains("MISALIGNED") {
        return Style::default().fg(Color::Rgb(255, 140, 0)).add_modifier(Modifier::BOLD);
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
        HwActivity::Formatting => {
            let spin = SPINNER[(io_cycle as usize) % SPINNER.len()];
            (
                Span::styled(
                    "► ",
                    Style::default()
                        .fg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[LOW-LEVEL FORMATTING {}]", spin),
                    Style::default()
                        .fg(Color::LightYellow)
                        .add_modifier(Modifier::BOLD),
                ),
            )
        }
        HwActivity::Erasing => {
            let spin = SPINNER[(io_cycle as usize) % SPINNER.len()];
            (
                Span::styled(
                    "► ",
                    Style::default()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[DC ERASING FLUX {}]", spin),
                    Style::default()
                        .fg(Color::LightRed)
                        .add_modifier(Modifier::BOLD),
                ),
            )
        }
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

        let last_pass = if head_idx == 0 {
            &status.last_pass_h0
        } else {
            &status.last_pass_h1
        };

        if let Some(pass) = last_pass {
            if status.verbose_mode {
                let mut spans = vec![prefix_span];
                spans.extend(build_verbose_line_spans(&pass.line_verbose, expected));
                Line::from(spans)
            } else {
                let mut spans = vec![prefix_span];
                spans.extend(build_standard_line_spans(&pass.line_standard, expected));
                Line::from(spans)
            }
        } else {
            let empty_text = format!(
                "T:{:02} H:{} : (Waiting read stream...)",
                status.track, head_idx
            );
            Line::from(vec![
                prefix_span,
                Span::styled(empty_text, Style::default().fg(Color::DarkGray)),
            ])
        }
    };

    lines.push(format_head_line(0));
    lines.push(format_head_line(1));
    lines
}

/// Builds the sliding multi-line scroll history for single-head mode (Head 0 or Head 1):
/// - Historical lines: aligned neutral prefix ("      ") and standard green valid sector blocks (`Color::Rgb(0, 180, 0)`)
/// - Last line (`is_last`): rotating spinner prefix ("▸ [/] ") and dynamic TrueColor decay interpolation over ~220ms
pub fn build_single_head_stream_lines(app: &App, available_height: usize) -> Vec<Line<'static>> {
    let status = &app.status;
    let mut lines = Vec::new();
    let max_vertical_items = available_height.saturating_sub(11).clamp(1, 13);
    let start_idx = status
        .sector_log
        .len()
        .saturating_sub(max_vertical_items);
    let recent_logs = &status.sector_log[start_idx..];

    if recent_logs.is_empty() {
        lines.push(Line::from(Span::styled(
            format!(
                "T:{:02} H:{} : (Waiting read stream...)",
                status.track, status.head
            ),
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        const SPINNERS: &[&str] = &["|", "/", "-", "\\"];
        let spin_char = SPINNERS[app.stream_spinner_idx % SPINNERS.len()];

        for (i, log_line) in recent_logs.iter().enumerate() {
            let is_last = i == recent_logs.len().saturating_sub(1);
            let prefix_span = if is_last {
                Span::styled(
                    format!("▸ [{}] ", spin_char),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("      ", Style::default())
            };

            if status.verbose_mode {
                let mut spans = vec![prefix_span];
                spans.extend(build_verbose_line_spans(log_line, status.sector_count));
                lines.push(Line::from(spans));
            } else {
                let valid_color = if is_last {
                    let elapsed = app.last_capture_instant.elapsed().as_millis() as f32;
                    let factor = (1.0 - (elapsed / 220.0)).clamp(0.0, 1.0);
                    let r = (30.0 + factor * 180.0) as u8;
                    let g = (180.0 + factor * 75.0) as u8;
                    let b = (30.0 + factor * 180.0) as u8;
                    Color::Rgb(r, g, b)
                } else {
                    Color::Rgb(0, 180, 0)
                };

                let mut spans = vec![prefix_span];
                spans.extend(build_standard_line_spans_with_color(
                    log_line,
                    status.sector_count,
                    valid_color,
                ));
                lines.push(Line::from(spans));
            }
        }
    }
    lines
}

/// Returns the clean top header branding title:
/// ` 💾 AlignTesterDiag v{VERSION} `
pub fn get_header_title() -> String {
    format!(" 💾 AlignTesterDiag v{} ", env!("CARGO_PKG_VERSION"))
}

/// Formats the port badge string for the top header banner:
/// ` [ Port: {PORT_NAME} ] `
pub fn format_port_badge(port_name: &str) -> String {
    let p = if port_name.trim().is_empty() {
        "Auto"
    } else {
        port_name.trim()
    };
    format!(" [ Port: {} ] ", p)
}

/// Builds the clean 1-line footer status bar displaying shortcuts and hardware status:
/// ` [?] / [F1] Help | [Q] Quit | [Esc] Stop | Hardware: Greaseweazle `
pub fn build_footer_line() -> Line<'static> {
    Line::from(vec![
        Span::styled(" [?] / [F1] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled("Help", Style::default().fg(Color::White)),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled("[Q] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled("Quit", Style::default().fg(Color::White)),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled("[Esc] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled("Stop", Style::default().fg(Color::White)),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled("Hardware: ", Style::default().fg(Color::White)),
        Span::styled("Greaseweazle", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
        Span::styled(" ", Style::default()),
    ])
}

fn shortcut_row(key: &str, desc: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {:<16}", key), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(": ", Style::default().fg(Color::DarkGray)),
        Span::styled(desc.to_string(), Style::default().fg(Color::White)),
    ])
}

/// Builds the formatted help and keyboard shortcut reference lines for the interactive modal
pub fn build_help_modal_lines() -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("  Version: ", Style::default().fg(Color::White)),
            Span::styled(format!("v{}", env!("CARGO_PKG_VERSION")), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled(" | Author: ", Style::default().fg(Color::White)),
            Span::styled("Mr JeAn-FReD", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" | License: ", Style::default().fg(Color::White)),
            Span::styled("GPL-3.0", Style::default().fg(Color::LightMagenta)),
        ]),
        Line::from(Span::styled("  ────────────────────────────────────────────────────────────────────────", Style::default().fg(Color::DarkGray))),
        shortcut_row("? / F1", "Toggle this Help Modal"),
        shortcut_row("A", "Analyze (Continuous real-time alignment & flux acquisition)"),
        shortcut_row("B", "Toggle Audio Radar (Pitch-variometer feedback)"),
        shortcut_row("D", "Read Data (Sector integrity check & CRC verification)"),
        shortcut_row("E", "Erase (Low-level DC flux wipe / erase modal)"),
        shortcut_row("Esc", "Stop / Motor off (Enter safe idle state)"),
        shortcut_row("Backspace", "PANIC RESET (Instant motor cut & hardware re-init)"),
        shortcut_row("F", "Format (Low-level track format)"),
        shortcut_row("H", "Toggle Head (Head 0 -> Head 1 -> Both 0+1)"),
        shortcut_row("I", "Track Image / Cycle RPM Mode (Auto -> HW Index -> SW Sync -> Dual in RPM test)"),
        shortcut_row("L", "Live RPM (High-precision continuous tachometer test)"),
        shortcut_row("M", "Toggle Motor (Spindle motor ON / OFF)"),
        shortcut_row("P", "Preset Hardware & Format (3.5\" HD/DD, 5.25\" HD/DD/DD@HD, Amiga, Atari, CPC)"),
        shortcut_row("R", "Recalibrate Seek (Track 0 seek & verify)"),
        shortcut_row("S", "Toggle Step Rate (Single 1:1 / Double 2:1 for 48/96 TPI)"),
        shortcut_row("T", "Toggle Bus Type (IBM PC <-> Shugart)"),
        shortcut_row("U", "Toggle Drive Unit (PC: A/B, Shugart: DS0..DS3)"),
        shortcut_row("V", "Toggle Verbose (Standard vs Verbose detail)"),
        shortcut_row("W", "Write Data"),
        shortcut_row("Z", "Zero Track (Direct seek to Track 0)"),
        shortcut_row("+ / - / Arrows", "Step track +1 / -1 (0 to 83)"),
        shortcut_row("0 - 9", "Direct jump to tracks (0, 10, 20... 80)"),
        shortcut_row("Q / X / Ctrl+C", "Clean Exit (Instant motor & LED shutdown)"),
        Line::from(Span::styled("  ────────────────────────────────────────────────────────────────────────", Style::default().fg(Color::DarkGray))),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Esc]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(", ", Style::default().fg(Color::DarkGray)),
            Span::styled("[?]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(", or ", Style::default().fg(Color::DarkGray)),
            Span::styled("[F1]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" to return", Style::default().fg(Color::DarkGray)),
        ]),
    ]
}

/// Renders an interactive centered overlay help modal dialog
pub fn render_help_modal(f: &mut Frame, area: Rect) {
    let modal_width = (area.width.saturating_sub(2)).clamp(86, 88).min(area.width);
    let modal_height = (area.height.saturating_sub(2)).clamp(28, 28).min(area.height);
    let x = area.x + (area.width.saturating_sub(modal_width)) / 2;
    let y = area.y + (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(x, y, modal_width, modal_height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .title(" 💾 AlignTesterDiag 🛠️ — Help & Shortcuts ")
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(Color::Rgb(15, 20, 35)));

    let paragraph = Paragraph::new(build_help_modal_lines())
        .block(block)
        .alignment(ratatui::layout::Alignment::Left);

    f.render_widget(paragraph, modal_area);
}

#[allow(clippy::too_many_arguments)]
pub fn build_format_modal_lines(
    track: u8,
    head_sel: HeadSelection,
    _max_track: u8,
    preset: PresetProfile,
    target_rpm: f64,
    bitrate: u16,
    bus_name: &str,
    unit: u8,
    target_tracks: u8,
    is_48_tpi: bool,
    format_verify: bool,
    fs_mode: FsInitMode,
    range: TrackRange,
    pending_confirm: Option<&PendingConfirmation>,
) -> Vec<Line<'static>> {
    let accent_bold = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let red_bold = Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD);
    let white = Style::default().fg(Color::White);
    let cyan = Style::default().fg(Color::Cyan);
    let gray = Style::default().fg(Color::DarkGray);

    let standard_tracks = if is_48_tpi { 40 } else { 80 };
    let max_tracks_allowed = if is_48_tpi { 42 } else { 84 };
    let last_track_idx = target_tracks.saturating_sub(1);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  ⚠️  LOW-LEVEL MFM FORMATTING & TRACK SYNTHESIZER", Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(Span::styled("  ────────────────────────────────────────────────────────────────────────", gray)),
        Line::from(vec![
            Span::styled("  Low-level formatting will ", white),
            Span::styled("REWRITE RAW MAGNETIC FLUX", red_bold),
            Span::styled(" on the floppy diskette.", white),
        ]),
        Line::from(Span::styled("  All existing data in the targeted track/disk area will be overwritten.", white)),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Target Drive  : ", gray),
            Span::styled(format!("Unit {} ({})", unit, bus_name), cyan),
            Span::styled("  |  [", gray),
            Span::styled("H", accent_bold),
            Span::styled("] Head: ", white),
            Span::styled(head_sel.label(), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" (Bitrate: {} kbps)", bitrate), cyan),
        ]),
        Line::from(vec![
            Span::styled("  Target Preset : ", gray),
            Span::styled(
                format!("[{}]", preset.label()),
                Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  |  Target: ", gray),
            Span::styled(
                format!("{:.0} RPM", target_rpm),
                Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("+/- or [ ]", accent_bold),
            Span::styled("] Target Track  : ", white),
            Span::styled(format!("Track {:02}", track), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("PgUp/PgDn", accent_bold),
            Span::styled("] Target Tracks : ", white),
            Span::styled(format!("{} tracks", target_tracks), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" (Range: 00..{:02} | Standard: {}, Max: {})", last_track_idx, standard_tracks, max_tracks_allowed), cyan),
        ]),
        Line::from(Span::styled("  ────────────────────────────────────────────────────────────────────────", gray)),
        Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("V", accent_bold),
            Span::styled("] Read-After-Write ", white),
            Span::styled("V", accent_bold),
            Span::styled("erify : ", white),
            if format_verify {
                Span::styled("ON ", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("OFF", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD))
            },
            Span::styled(
                if format_verify {
                    " (Write + CRC verify ~70s for 80 tracks)"
                } else {
                    " (Direct fast write ~35s for 80 tracks)"
                },
                cyan,
            ),
        ]),
        Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("S", accent_bold),
            Span::styled("] ", white),
            Span::styled("S", accent_bold),
            Span::styled("ystem FS Init : ", white),
            match fs_mode {
                FsInitMode::Blank => Span::styled("Blank (Raw 0xE5)", Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)),
                FsInitMode::OsReady => Span::styled(
                    format!("OS-Ready ({})", FsInitMode::os_desc(preset)),
                    Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
                ),
            },
            Span::styled(
                match fs_mode {
                    FsInitMode::Blank => " (Direct unformatted raw sectors)",
                    FsInitMode::OsReady => " (Boot & Root FS initialized)",
                },
                cyan,
            ),
        ]),
    ];

    if let Some(confirm) = pending_confirm {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  ────────────────────────────────────────────────────────────────────────", Style::default().fg(Color::Yellow))));
        lines.push(Line::from(vec![
            Span::styled("  ⚠️  ", Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)),
            Span::styled(confirm.prompt_string(), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("     Press ", gray),
            Span::styled("Y", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled(" to confirm & execute, or ", gray),
            Span::styled("N / Enter / Esc", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
            Span::styled(" to cancel", gray),
        ]));
        lines.push(Line::from(Span::styled("  ────────────────────────────────────────────────────────────────────────", Style::default().fg(Color::Yellow))));
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("S", accent_bold),
            Span::styled("] Toggle ", white),
            Span::styled("S", accent_bold),
            Span::styled("ystem FS Init (Blank / OS-Ready)", white),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("U", accent_bold),
            Span::styled("] Unit (A:/B:)   [", white),
            Span::styled("B", accent_bold),
            Span::styled("] Bus (PC/Shugart)   [", white),
            Span::styled("P", accent_bold),
            Span::styled("] Preset Profile   [", white),
            Span::styled("H", accent_bold),
            Span::styled("] Head", white),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("T", accent_bold),
            Span::styled("] Format Current ", white),
            Span::styled("T", accent_bold),
            Span::styled(format!("rack only (Track {:02}, {})", track, head_sel.conf_label()), white),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("R", accent_bold),
            Span::styled("] Format Track ", white),
            Span::styled("R", accent_bold),
            Span::styled(format!("ange     (Tracks {:02}..{:02}, {})", range.start, range.end, head_sel.conf_label()), white),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("D", accent_bold),
            Span::styled("] Format Entire ", white),
            Span::styled("D", accent_bold),
            Span::styled(format!("isk     (Tracks 00..{:02}, {})", last_track_idx, head_sel.conf_label()), white),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("Esc", red_bold),
            Span::styled("] Cancel & Return", white),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  ────────────────────────────────────────────────────────────────────────", gray)));
    lines.push(Line::from(vec![
        Span::styled("  Hardware Protection: ", gray),
        Span::styled("Write-Protect pin 28 checked before writing.", Style::default().fg(Color::LightCyan)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Verify             : ", gray),
        Span::styled("Read-after-write CRC check enabled.", Style::default().fg(Color::LightCyan)),
    ]));

    lines
}

/// Renders an interactive centered overlay format confirmation modal dialog
#[allow(clippy::too_many_arguments)]
pub fn render_format_modal(
    f: &mut Frame,
    area: Rect,
    track: u8,
    head_sel: HeadSelection,
    max_track: u8,
    preset: PresetProfile,
    target_rpm: f64,
    bitrate: u16,
    bus_name: &str,
    unit: u8,
    target_tracks: u8,
    is_48_tpi: bool,
    format_verify: bool,
    fs_mode: FsInitMode,
    range: TrackRange,
    pending_confirm: Option<&PendingConfirmation>,
) {
    let modal_width = (area.width.saturating_sub(2)).clamp(82, 88).min(area.width);
    let modal_height = (area.height.saturating_sub(2)).clamp(26, 29).min(area.height);
    let x = area.x + (area.width.saturating_sub(modal_width)) / 2;
    let y = area.y + (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(x, y, modal_width, modal_height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD))
        .title(" 💾 Low-Level Track / Disk Format (CMD_WRITE_FLUX) ")
        .title_style(Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(Color::Rgb(20, 15, 30)));

    let paragraph = Paragraph::new(build_format_modal_lines(
        track,
        head_sel,
        max_track,
        preset,
        target_rpm,
        bitrate,
        bus_name,
        unit,
        target_tracks,
        is_48_tpi,
        format_verify,
        fs_mode,
        range,
        pending_confirm,
    ))
    .block(block)
    .alignment(ratatui::layout::Alignment::Left);

    f.render_widget(paragraph, modal_area);
}

/// Builds the formatted confirmation and selection lines for the Low-Level DC Erase modal dialog
#[allow(clippy::too_many_arguments)]
pub fn build_erase_modal_lines(
    track: u8,
    head_sel: HeadSelection,
    preset_label: &str,
    target_rpm: f64,
    bitrate: u16,
    target_tracks: u8,
    is_48_tpi: bool,
    range: TrackRange,
    pending_confirm: Option<&PendingConfirmation>,
) -> Vec<Line<'static>> {
    let accent_bold = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let red_bold = Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD);
    let white = Style::default().fg(Color::White);
    let cyan = Style::default().fg(Color::Cyan);
    let gray = Style::default().fg(Color::DarkGray);

    let standard_tracks = if is_48_tpi { 40 } else { 80 };
    let max_tracks_allowed = if is_48_tpi { 42 } else { 84 };
    let last_track_idx = target_tracks.saturating_sub(1);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  Target Preset : ", gray),
            Span::styled(
                format!("[{}]", preset_label),
                Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  |  Target: ", gray),
            Span::styled(
                format!("{:.0} RPM", target_rpm),
                Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("+/- or [ ]", accent_bold),
            Span::styled("] Target Track  : ", white),
            Span::styled(format!("Track {:02}", track), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled("  |  [", gray),
            Span::styled("H", accent_bold),
            Span::styled("] Head: ", white),
            Span::styled(head_sel.label(), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" (Bitrate: {} kbps)", bitrate), cyan),
        ]),
        Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("PgUp/PgDn", accent_bold),
            Span::styled("] Target Tracks : ", white),
            Span::styled(format!("{} tracks", target_tracks), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" (Range: 00..{:02} | Standard: {}, Max: {})", last_track_idx, standard_tracks, max_tracks_allowed), cyan),
        ]),
        Line::from(Span::styled("  ────────────────────────────────────────────────────────────────────────", gray)),
        Line::from(vec![
            Span::styled("  ⚠️  WARNING: This will permanently wipe all magnetic flux on target!", red_bold),
        ]),
    ];

    if let Some(confirm) = pending_confirm {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("  ────────────────────────────────────────────────────────────────────────", Style::default().fg(Color::Yellow))));
        lines.push(Line::from(vec![
            Span::styled("  ⚠️  ", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
            Span::styled(confirm.prompt_string(), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("     Press ", gray),
            Span::styled("Y", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled(" to confirm & execute, or ", gray),
            Span::styled("N / Enter / Esc", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
            Span::styled(" to cancel", gray),
        ]));
        lines.push(Line::from(Span::styled("  ────────────────────────────────────────────────────────────────────────", Style::default().fg(Color::Yellow))));
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("U", accent_bold),
            Span::styled("] Unit (A:/B:)   [", white),
            Span::styled("B", accent_bold),
            Span::styled("] Bus (PC/Shugart)   [", white),
            Span::styled("P", accent_bold),
            Span::styled("] Preset Profile   [", white),
            Span::styled("H", accent_bold),
            Span::styled("] Head", white),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("T", accent_bold),
            Span::styled("] Erase Current ", white),
            Span::styled("T", accent_bold),
            Span::styled(format!("rack only  (Track {:02}, {})", track, head_sel.conf_label()), white),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("R", accent_bold),
            Span::styled("] Erase Track ", white),
            Span::styled("R", accent_bold),
            Span::styled(format!("ange     (Tracks {:02}..{:02}, {})", range.start, range.end, head_sel.conf_label()), white),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("D", accent_bold),
            Span::styled("] Erase Entire ", white),
            Span::styled("D", accent_bold),
            Span::styled(format!("isk         (Tracks 00..{:02}, {})", last_track_idx, head_sel.conf_label()), white),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  [", gray),
            Span::styled("Esc", red_bold),
            Span::styled("] Cancel & Return", white),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  ────────────────────────────────────────────────────────────────────────", gray)));
    lines.push(Line::from(vec![
        Span::styled("  Hardware Protection: ", gray),
        Span::styled("Write-Protect pin 28 checked before erasing.", Style::default().fg(Color::LightCyan)),
    ]));

    lines
}

/// Renders an interactive centered overlay DC erase confirmation modal dialog
#[allow(clippy::too_many_arguments)]
pub fn render_erase_modal(
    f: &mut Frame,
    area: Rect,
    track: u8,
    head_sel: HeadSelection,
    preset_label: &str,
    target_rpm: f64,
    bitrate: u16,
    target_tracks: u8,
    is_48_tpi: bool,
    range: TrackRange,
    pending_confirm: Option<&PendingConfirmation>,
) {
    let modal_width = (area.width.saturating_sub(2)).clamp(82, 88).min(area.width);
    let modal_height = (area.height.saturating_sub(2)).clamp(18, 22).min(area.height);
    let x = area.x + (area.width.saturating_sub(modal_width)) / 2;
    let y = area.y + (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(x, y, modal_width, modal_height);

    f.render_widget(Clear, modal_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .title(" LOW-LEVEL ERASE (DC ERASE) ")
        .title_style(Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(Color::Rgb(25, 15, 20)));

    let paragraph = Paragraph::new(build_erase_modal_lines(
        track,
        head_sel,
        preset_label,
        target_rpm,
        bitrate,
        target_tracks,
        is_48_tpi,
        range,
        pending_confirm,
    ))
    .block(block)
    .alignment(ratatui::layout::Alignment::Left);

    f.render_widget(paragraph, modal_area);
}

/// Helper to construct the lines for the custom track range editor modal dialog
pub fn build_range_edit_modal_lines<'a>(
    head_sel: HeadSelection,
    start_str: &str,
    end_str: &str,
    active_field: RangeField,
    max_tracks: u8,
    error_msg: Option<&str>,
) -> Vec<Line<'a>> {
    let accent_bold = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let white = Style::default().fg(Color::White);
    let gray = Style::default().fg(Color::DarkGray);
    let cyan = Style::default().fg(Color::Cyan);

    let start_active = active_field == RangeField::Start;
    let end_active = active_field == RangeField::End;

    let start_style = if start_active {
        Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
    };

    let end_style = if end_active {
        Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
    };

    let max_idx = max_tracks.saturating_sub(1);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(" Enter track bounds (00 to ", gray),
            Span::styled(format!("{:02}", max_idx), cyan),
            Span::styled(") for batch operation:", gray),
        ]),
        Line::from(""),
        Line::from(vec![
            if start_active {
                Span::styled(" ► Start Track : [ ", accent_bold)
            } else {
                Span::styled("   Start Track : [ ", white)
            },
            Span::styled(format!("{:>2}", if start_str.is_empty() { "_" } else { start_str }), start_style),
            Span::styled(" ]", if start_active { accent_bold } else { white }),
            Span::styled(format!("  (Minimum: 00, Maximum: {:02})", max_idx), gray),
        ]),
        Line::from(vec![
            if end_active {
                Span::styled(" ► End Track   : [ ", accent_bold)
            } else {
                Span::styled("   End Track   : [ ", white)
            },
            Span::styled(format!("{:>2}", if end_str.is_empty() { "_" } else { end_str }), end_style),
            Span::styled(" ]", if end_active { accent_bold } else { white }),
            Span::styled(format!("  (Must be >= Start Track and <= {:02})", max_idx), gray),
        ]),
        Line::from(""),
    ];

    if let Some(err) = error_msg {
        lines.push(Line::from(Span::styled(
            format!(" ⚠️  {}", err),
            Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
        )));
    } else {
        let s_val = start_str.parse::<u8>().ok();
        let e_val = end_str.parse::<u8>().ok();
        if let (Some(s), Some(e)) = (s_val, e_val) {
            if s <= e && e < max_tracks {
                let count = e - s + 1;
                let (passes_str, head_desc) = match head_sel {
                    HeadSelection::Both => (format!("{} passes", (count as usize) * 2), "Dual-Head"),
                    HeadSelection::Head0 => (format!("{} passes", count), "Head 0 only"),
                    HeadSelection::Head1 => (format!("{} passes", count), "Head 1 only"),
                };
                lines.push(Line::from(vec![
                    Span::styled(" Total: ", gray),
                    Span::styled(format!("{} tracks", count), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" (Tracks {:02}..{:02}, {} = {})", s, e, head_desc, passes_str), cyan),
                ]));
            } else {
                lines.push(Line::from(Span::styled(" Please specify a valid range", gray)));
            }
        } else {
            lines.push(Line::from(Span::styled(" Please specify numeric bounds", gray)));
        }
    }

    lines.push(Line::from(Span::styled(" ────────────────────────────────────────────────────────────────────────────", gray)));
    lines.push(Line::from(vec![
        Span::styled(" [", gray),
        Span::styled("Tab", accent_bold),
        Span::styled("] Switch Field   [", gray),
        Span::styled("+/- or ↑/↓", accent_bold),
        Span::styled("] Inc/Dec   [", gray),
        Span::styled("0-9", accent_bold),
        Span::styled("] Edit   [", gray),
        Span::styled("Bksp", accent_bold),
        Span::styled("] Clear", gray),
    ]));
    lines.push(Line::from(vec![
        Span::styled(" [", gray),
        Span::styled("H", accent_bold),
        Span::styled("] Toggle Head      [", gray),
        Span::styled("Enter", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
        Span::styled("] Validate & Start     [", gray),
        Span::styled("Esc", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
        Span::styled("] Cancel & Back", gray),
    ]));

    lines
}

/// Renders an interactive custom track range editor modal dialog
#[allow(clippy::too_many_arguments)]
pub fn render_range_edit_modal(
    f: &mut Frame,
    area: Rect,
    kind: RangeModalKind,
    head_sel: HeadSelection,
    start_str: &str,
    end_str: &str,
    active_field: RangeField,
    max_tracks: u8,
    error_msg: Option<&str>,
) {
    let modal_width = 82.min(area.width.saturating_sub(2));
    let modal_height = 15.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(modal_width)) / 2;
    let y = area.y + (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(x, y, modal_width, modal_height);

    f.render_widget(Clear, modal_area);

    let (title, title_col, bg_col) = match kind {
        RangeModalKind::Format => (
            " 🔢 Set Custom Track Range (Format) ",
            Color::LightGreen,
            Color::Rgb(20, 15, 30),
        ),
        RangeModalKind::Erase => (
            " 🔢 Set Custom Track Range (DC Erase) ",
            Color::LightRed,
            Color::Rgb(25, 15, 20),
        ),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .title(title)
        .title_style(Style::default().fg(title_col).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(bg_col));

    let paragraph = Paragraph::new(build_range_edit_modal_lines(
        head_sel,
        start_str,
        end_str,
        active_field,
        max_tracks,
        error_msg,
    ))
    .block(block);
    f.render_widget(paragraph, modal_area);
}

/// Helper function to build a text-based segmented progress bar: `[████████████░░░░░░░░] 60.0%`
pub fn build_progress_bar_string(pct: f32, width: usize) -> String {
    let clamped_pct = pct.clamp(0.0, 100.0);
    let filled = ((clamped_pct / 100.0) * width as f32).round() as usize;
    let empty = width.saturating_sub(filled);
    format!(
        "[{}{}] {:>5.1}%",
        "█".repeat(filled),
        "░".repeat(empty),
        clamped_pct
    )
}

/// Builds real-time formatted lines for the right panel when low-level formatting or erasing is active
pub fn build_format_progress_lines(status: &crate::hw::DriveStatus) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let is_erase = status.mode == crate::hw::DisplayMode::Erase;

    if is_erase {
        lines.push(Line::from(Span::styled(
            "=== LOW-LEVEL DC FLUX ERASE ENGINE (CMD_ERASE_FLUX) ===",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "Continuous Neutral Magnetic Wipe (≥ 1.1 Revolutions Hardware Index Synchronized)",
            Style::default().fg(Color::LightRed),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "=== LOW-LEVEL FORMAT ENGINE & MFM SYNTHESIZER (CMD_WRITE_FLUX) ===",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "Hardware-Synchronized Index Writing (72 MHz) with Read-After-Write CRC Verification",
            Style::default().fg(Color::LightCyan),
        )));
    }
    lines.push(Line::from(""));

    let preset = status.preset;
    lines.push(Line::from(vec![
        Span::styled("Target Preset  : ", Style::default().fg(Color::DarkGray)),
        Span::styled(preset.label(), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
        Span::styled(" | Bitrate: ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{} kbps", preset.target_data_rate()), Style::default().fg(Color::Cyan)),
        Span::styled(" | Bus: ", Style::default().fg(Color::DarkGray)),
        Span::styled(status.bus_type.as_str(), Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(""));

    if let Some(ref prog) = status.format_progress {
        let pct = if prog.total_passes > 0 {
            (prog.completed_passes as f32 / prog.total_passes as f32) * 100.0
        } else {
            0.0
        };
        let phys_cyl = prog.current_track * status.step_mode.multiplier();

        lines.push(Line::from(vec![
            Span::styled("Status Phase   : ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                prog.step.as_str(),
                match prog.step {
                    crate::hw::FormatStep::Writing => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    crate::hw::FormatStep::Verifying => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    crate::hw::FormatStep::Erasing => Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                    crate::hw::FormatStep::Retrying => Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                    crate::hw::FormatStep::Completed => Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
                    crate::hw::FormatStep::Error => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    crate::hw::FormatStep::Idle => Style::default().fg(Color::White),
                },
            ),
            Span::styled(" | Target: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "Track {:02} of {} total (Phys: {:02}) | Head {}/{}",
                    prog.current_track,
                    prog.total_tracks,
                    phys_cyl,
                    prog.current_head,
                    prog.total_heads.saturating_sub(1)
                ),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]));

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Progress       : ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                build_progress_bar_string(pct, 36),
                Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" ({}/{} passes)", prog.completed_passes, prog.total_passes),
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        if !is_erase {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Verification   : ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("Sectors: {}/{} | CRC Errors: {} | Quality: {}%", prog.verified_sectors, prog.expected_sectors, prog.crc_errors, prog.quality_pct),
                    if prog.verification_ok {
                        Style::default().fg(Color::LightGreen)
                    } else if prog.crc_errors > 0 {
                        Style::default().fg(Color::LightRed)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
            ]));
        }

        lines.push(Line::from(""));
        if prog.step == crate::hw::FormatStep::Completed {
            let total_secs = prog.elapsed_secs.max(0.0) as u64;
            let dur_h = total_secs / 3600;
            let dur_m = (total_secs % 3600) / 60;
            let dur_s = total_secs % 60;
            let total_duration_str = format!("{:02}:{:02}:{:02}", dur_h, dur_m, dur_s);

            lines.push(Line::from(vec![
                Span::styled("Timing Stats   : ", Style::default().fg(Color::DarkGray)),
                Span::styled("Completed Successfully", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
                Span::styled(" | Total Duration: ", Style::default().fg(Color::DarkGray)),
                Span::styled(total_duration_str, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]));
        } else {
            let now_sys = std::time::SystemTime::now();
            let start_sys = prog.start_time.unwrap_or_else(|| {
                now_sys.checked_sub(std::time::Duration::from_secs_f64(prog.elapsed_secs.max(0.0))).unwrap_or(now_sys)
            });
            let est_end_sys = now_sys + std::time::Duration::from_secs_f64(prog.eta_secs.max(0.0));

            let start_str = chrono::DateTime::<chrono::Local>::from(start_sys).format("%H:%M:%S").to_string();
            let now_str = chrono::DateTime::<chrono::Local>::from(now_sys).format("%H:%M:%S").to_string();
            let est_end_str = chrono::DateTime::<chrono::Local>::from(est_end_sys).format("%H:%M:%S").to_string();

            lines.push(Line::from(vec![
                Span::styled("Timing Stats   : ", Style::default().fg(Color::DarkGray)),
                Span::styled("Start: ", Style::default().fg(Color::DarkGray)),
                Span::styled(start_str, Style::default().fg(Color::White)),
                Span::styled(" | Now: ", Style::default().fg(Color::DarkGray)),
                Span::styled(now_str, Style::default().fg(Color::White)),
                Span::styled(" | Est. End: ", Style::default().fg(Color::DarkGray)),
                Span::styled(est_end_str, Style::default().fg(Color::Yellow)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Message        : ", Style::default().fg(Color::DarkGray)),
            Span::styled(prog.message.clone(), Style::default().fg(Color::LightCyan)),
        ]));
    } else {
        lines.push(Line::from(Span::styled("Initializing engine...", Style::default().fg(Color::DarkGray))));
    }

    let is_finished = if let Some(ref p) = status.format_progress {
        matches!(
            p.step,
            crate::hw::FormatStep::Completed | crate::hw::FormatStep::Error | crate::hw::FormatStep::Idle
        )
    } else {
        false
    };

    lines.push(Line::from(""));
    if is_finished {
        lines.push(Line::from(vec![
            Span::styled("Press ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Enter]", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled(" or ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Esc]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" to return to menu, ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Q]", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
            Span::styled(" for Main View.", Style::default().fg(Color::DarkGray)),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            if is_erase {
                "Press [Esc] at any time to abort erase safely."
            } else {
                "Press [Esc] at any time to abort formatting safely."
            },
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines
}




