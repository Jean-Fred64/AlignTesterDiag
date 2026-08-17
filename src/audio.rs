use crossbeam_channel::Receiver;
use crate::app::{DiagnosticPass, HeadSelection};

/// Audio events emitted by the diagnostic alignment radar variometer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioEvent {
    /// Continuous alignment tone with dynamic pitch according to signal quality:
    /// - Nominal (95% - 100%): High clean tone (1500 Hz - 2200 Hz, scaled linearly by alignment %)
    /// - Marginal (70% - 94%): Medium tone (600 Hz - 1400 Hz)
    /// - Severe Misalignment (< 70%): Low continuous tone (250 Hz - 500 Hz, never silenced)
    AlignmentTone { pitch_hz: u32 },
    /// Off-Track / Track Mismatch (e.g. reading Track 39 while seeking Track 40):
    /// Pulsed low-pitch buzz (180 Hz pulsed at 8 Hz interval)
    TrackMismatch,
    /// Zero Decoded Sectors: Low-frequency warning hum (150 Hz continuous)
    ZeroDecodedSectors,
    /// Backward-compatibility alias for AlignmentTone
    PerfectAlignment { pitch_hz: u32 },
    /// Backward-compatibility alias for OffTrackOrCrcError
    OffTrackOrCrcError,
}

#[cfg(windows)]
mod platform_sound {
    extern "system" {
        fn Beep(dwFreq: u32, dwDuration: u32) -> i32;
    }

    pub fn play_beep(freq: u32, duration_ms: u32) {
        let clamped_freq = freq.clamp(37, 32767);
        unsafe {
            Beep(clamped_freq, duration_ms);
        }
    }
}

#[cfg(not(windows))]
mod platform_sound {
    use std::io::{stdout, Write};

    pub fn play_beep(_freq: u32, duration_ms: u32) {
        if duration_ms <= 20 {
            print!("\x07");
        } else {
            print!("\x07\x07");
        }
        let _ = stdout().flush();
    }
}

/// Plays a raw frequency beep for the specified duration (non-blocking when called inside worker thread)
pub fn play_beep(freq: u32, duration_ms: u32) {
    platform_sound::play_beep(freq, duration_ms);
}

/// Dispatches an AudioEvent with its exact sonic signature
pub fn play_audio_event(event: AudioEvent) {
    match event {
        AudioEvent::AlignmentTone { pitch_hz } | AudioEvent::PerfectAlignment { pitch_hz } => {
            play_beep(pitch_hz, 40);
        }
        AudioEvent::TrackMismatch => {
            // Pulsed low-pitch buzz (180 Hz pulsed at 8 Hz interval)
            play_beep(180, 50);
            std::thread::sleep(std::time::Duration::from_millis(15));
            play_beep(180, 50);
        }
        AudioEvent::ZeroDecodedSectors => {
            // Low-frequency warning hum (150 Hz continuous)
            play_beep(150, 40);
        }
        AudioEvent::OffTrackOrCrcError => {
            play_beep(150, 20);
        }
    }
}

/// Background worker thread function to process audio events without blocking hardware capture or TUI
pub fn sound_worker(rx: Receiver<AudioEvent>) {
    while let Ok(mut event) = rx.recv() {
        // Drain backlog to avoid audio lag under high revolution rates
        while let Ok(newer) = rx.try_recv() {
            event = newer;
        }
        play_audio_event(event);
    }
}

/// Calculates the dynamic pitch for the variometer radar according to signal quality percentage:
/// - Nominal Factory Alignment (95% – 100%): High clean tone (1500 Hz – 2200 Hz, scaled linearly by alignment %)
/// - Marginal Tracking (70% – 94%): Medium tone (600 Hz – 1400 Hz, scaled linearly)
/// - Severe Misalignment (< 70%): Low continuous tone (250 Hz – 500 Hz, never silenced)
pub fn calculate_radar_pitch(quality_pct: u8) -> u32 {
    let q = quality_pct.min(100);
    if q >= 95 {
        let norm = (q as f32 - 95.0) / 5.0;
        (1500.0 + norm * (2200.0 - 1500.0)).round() as u32
    } else if q >= 70 {
        let norm = (q as f32 - 70.0) / (94.0 - 70.0);
        (600.0 + norm * (1400.0 - 600.0)).round() as u32
    } else {
        let norm = q as f32 / 69.0;
        (250.0 + norm * (500.0 - 250.0)).round() as u32
    }
}

/// Evaluates alignment condition and returns appropriate AudioEvent if beep is enabled.
/// Ensures continuous, non-silent audio feedback across all alignment and error states:
/// - Nominal Factory Alignment (95% - 100%): 1500 Hz - 2200 Hz
/// - Marginal Tracking (70% - 94%): 600 Hz - 1400 Hz
/// - Severe Misalignment (< 70%): 250 Hz - 500 Hz continuous tone (never muted)
/// - Off-Track / Track Mismatch: Pulsed low-pitch buzz (180 Hz @ 8 Hz interval)
/// - Zero Decoded Sectors: Low-frequency warning hum (150 Hz continuous)
/// - In "Both" mode, emits real-time pitch for the active head pass on each revolution.
pub fn evaluate_alignment_audio_event(
    head_select: HeadSelection,
    target_track: u8,
    active_head: u8,
    pass_h0: Option<&DiagnosticPass>,
    pass_h1: Option<&DiagnosticPass>,
    _fallback_expected: u8,
) -> Option<AudioEvent> {
    let active_pass = match head_select {
        HeadSelection::Head0 => pass_h0,
        HeadSelection::Head1 => pass_h1,
        HeadSelection::Both => {
            if active_head == 0 {
                pass_h0
            } else {
                pass_h1
            }
        }
    };

    let p = match active_pass {
        Some(p) => p,
        None => return Some(AudioEvent::ZeroDecodedSectors),
    };

    // 1. Off-Track / Track Mismatch (e.g. reading Track 39 while seeking Track 40)
    if p.track_id != target_track || p.track != target_track {
        return Some(AudioEvent::TrackMismatch);
    }

    // 2. Zero Decoded Sectors: Low-frequency warning hum (150 Hz continuous)
    if p.valid_sectors == 0 && p.ok_count == 0 && p.crc_errors == 0 {
        return Some(AudioEvent::ZeroDecodedSectors);
    }

    // 3. Continuous Alignment Tone (Nominal 95-100%, Marginal 70-94%, Severe <70%)
    let pitch_hz = calculate_radar_pitch(p.quality_pct);
    Some(AudioEvent::AlignmentTone { pitch_hz })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radar_pitch_calculation_bounds() {
        // Severe Misalignment (< 70% -> 250 Hz to 500 Hz)
        assert_eq!(calculate_radar_pitch(0), 250);
        assert_eq!(calculate_radar_pitch(35), 377);
        assert_eq!(calculate_radar_pitch(69), 500);

        // Marginal Tracking (70% - 94% -> 600 Hz to 1400 Hz)
        assert_eq!(calculate_radar_pitch(70), 600);
        assert_eq!(calculate_radar_pitch(82), 1000);
        assert_eq!(calculate_radar_pitch(94), 1400);

        // Nominal Factory Alignment (95% - 100% -> 1500 Hz to 2200 Hz)
        assert_eq!(calculate_radar_pitch(95), 1500);
        assert_eq!(calculate_radar_pitch(99), 2060);
        assert_eq!(calculate_radar_pitch(100), 2200);
        // Over 100% clamped to 2200 Hz
        assert_eq!(calculate_radar_pitch(120), 2200);
    }

    #[test]
    fn test_evaluate_alignment_both_mode_alternation() {
        let pass_h0 = DiagnosticPass::with_details(
            40, 0, 500,
            "T:40 H:0".into(), "T:40 H:0".into(),
            18, 18, 0, 99, true,
        );
        let pass_h1 = DiagnosticPass::with_details(
            40, 1, 500,
            "T:40 H:1".into(), "T:40 H:1".into(),
            15, 18, 3, 82, false,
        );

        // When active head is H0 (pass_h0 has 99% nominal quality)
        let event_h0 = evaluate_alignment_audio_event(
            HeadSelection::Both,
            40,
            0,
            Some(&pass_h0),
            Some(&pass_h1),
            18,
        );
        assert_eq!(event_h0, Some(AudioEvent::AlignmentTone { pitch_hz: calculate_radar_pitch(99) }));
        assert_eq!(event_h0, Some(AudioEvent::AlignmentTone { pitch_hz: 2060 }));

        // When active head is H1 (pass_h1 has 82% marginal quality)
        let event_h1 = evaluate_alignment_audio_event(
            HeadSelection::Both,
            40,
            1,
            Some(&pass_h0),
            Some(&pass_h1),
            18,
        );
        assert_eq!(event_h1, Some(AudioEvent::AlignmentTone { pitch_hz: calculate_radar_pitch(82) }));
        assert_eq!(event_h1, Some(AudioEvent::AlignmentTone { pitch_hz: 1000 }));
    }

    #[test]
    fn test_evaluate_alignment_track_mismatch() {
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

        // H1 read Track 76 instead of target 75 -> TrackMismatch
        let event = evaluate_alignment_audio_event(
            HeadSelection::Both,
            75,
            1,
            Some(&pass_h0),
            Some(&pass_h1),
            18,
        );
        assert_eq!(event, Some(AudioEvent::TrackMismatch));
    }

    #[test]
    fn test_zero_decoded_sectors_warning_hum() {
        let pass_zero = DiagnosticPass::with_details(
            40, 0, 500,
            "T:40 H:0".into(), "T:40 H:0".into(),
            0, 0, 0, 0, false,
        );

        let event = evaluate_alignment_audio_event(
            HeadSelection::Head0,
            40,
            0,
            Some(&pass_zero),
            None,
            18,
        );
        assert_eq!(event, Some(AudioEvent::ZeroDecodedSectors));

        // When pass is None (initial / unpopulated state)
        let event_none = evaluate_alignment_audio_event(
            HeadSelection::Head0,
            40,
            0,
            None,
            None,
            18,
        );
        assert_eq!(event_none, Some(AudioEvent::ZeroDecodedSectors));
    }

    #[test]
    fn test_severe_misalignment_continuous_tone() {
        let pass_severe = DiagnosticPass::with_details(
            40, 0, 500,
            "T:40 H:0".into(), "T:40 H:0".into(),
            6, 18, 2, 40, false,
        );

        let event = evaluate_alignment_audio_event(
            HeadSelection::Head0,
            40,
            0,
            Some(&pass_severe),
            None,
            18,
        );

        let expected_pitch = calculate_radar_pitch(40);
        assert_eq!(event, Some(AudioEvent::AlignmentTone { pitch_hz: expected_pitch }));
        assert!(expected_pitch >= 250 && expected_pitch <= 500);
    }
}
