use crossbeam_channel::Receiver;
use crate::app::{DiagnosticPass, HeadSelection};

/// Audio events emitted by the diagnostic alignment radar
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioEvent {
    /// Perfect alignment with dynamic pitch (440 Hz to 1760 Hz, 40 ms)
    PerfectAlignment { pitch_hz: u32 },
    /// Dissonant warning sound for cross-track mismatch or off-target track (220 Hz, 120 ms)
    TrackMismatch,
    /// Attenuated click for missing sectors or CRC integrity errors (150 Hz, 20 ms)
    OffTrackOrCrcError,
}

#[cfg(windows)]
mod platform_sound {
    extern "system" {
        fn Beep(dwFreq: u32, dwDuration: u32) -> i32;
    }

    pub fn play_beep(freq: u32, duration_ms: u32) {
        unsafe {
            Beep(freq, duration_ms);
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
        AudioEvent::PerfectAlignment { pitch_hz } => play_beep(pitch_hz, 40),
        AudioEvent::TrackMismatch => play_beep(220, 120),
        AudioEvent::OffTrackOrCrcError => play_beep(150, 20),
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

/// Calculates the dynamic pitch for the variometer radar according to signal quality percentage
/// Formula: pitch = 440 + ((clamp(Q, 30, 100) - 30) / 70) * (1760 - 440)
pub fn calculate_radar_pitch(quality_pct: u8) -> u32 {
    let clamped_q = quality_pct.clamp(30, 100) as f32;
    let normalized = (clamped_q - 30.0) / 70.0;
    (440.0 + normalized * (1760.0 - 440.0)) as u32
}

/// Evaluates alignment condition and returns appropriate AudioEvent if beep is enabled
pub fn evaluate_alignment_audio_event(
    head_select: HeadSelection,
    target_track: u8,
    active_head: u8,
    pass_h0: Option<&DiagnosticPass>,
    pass_h1: Option<&DiagnosticPass>,
    fallback_expected: u8,
) -> Option<AudioEvent> {
    let expected = if fallback_expected > 0 {
        fallback_expected
    } else {
        18
    };

    match head_select {
        HeadSelection::Both => {
            if let (Some(h0), Some(h1)) = (pass_h0, pass_h1) {
                // Cross-track mismatch detection:
                // If head 0 and head 1 are reading different physical tracks, or either is off target
                if h0.track_id != h1.track_id || h0.track_id != target_track || h1.track_id != target_track {
                    return Some(AudioEvent::TrackMismatch);
                }

                let h0_match = h0.track_id == target_track
                    && (h0.valid_sectors == 18 || h0.valid_sectors >= expected)
                    && h0.crc_errors == 0;
                let h1_match = h1.track_id == target_track
                    && (h1.valid_sectors == 18 || h1.valid_sectors >= expected)
                    && h1.crc_errors == 0;

                if h0_match && h1_match {
                    let q_ref = h0.quality_pct.min(h1.quality_pct);
                    let pitch_hz = calculate_radar_pitch(q_ref);
                    Some(AudioEvent::PerfectAlignment { pitch_hz })
                } else {
                    Some(AudioEvent::OffTrackOrCrcError)
                }
            } else {
                // Only one head pass has completed so far
                let active_pass = if active_head == 0 { pass_h0 } else { pass_h1 };
                if let Some(p) = active_pass {
                    if p.track_id != target_track {
                        return Some(AudioEvent::TrackMismatch);
                    }
                    if (p.valid_sectors == 18 || p.valid_sectors >= expected) && p.crc_errors == 0 {
                        let pitch_hz = calculate_radar_pitch(p.quality_pct);
                        Some(AudioEvent::PerfectAlignment { pitch_hz })
                    } else {
                        Some(AudioEvent::OffTrackOrCrcError)
                    }
                } else {
                    None
                }
            }
        }
        HeadSelection::Head0 => {
            if let Some(p) = pass_h0 {
                if p.track_id != target_track {
                    return Some(AudioEvent::TrackMismatch);
                }
                if (p.valid_sectors == 18 || p.valid_sectors >= expected) && p.crc_errors == 0 {
                    let pitch_hz = calculate_radar_pitch(p.quality_pct);
                    Some(AudioEvent::PerfectAlignment { pitch_hz })
                } else {
                    Some(AudioEvent::OffTrackOrCrcError)
                }
            } else {
                None
            }
        }
        HeadSelection::Head1 => {
            if let Some(p) = pass_h1 {
                if p.track_id != target_track {
                    return Some(AudioEvent::TrackMismatch);
                }
                if (p.valid_sectors == 18 || p.valid_sectors >= expected) && p.crc_errors == 0 {
                    let pitch_hz = calculate_radar_pitch(p.quality_pct);
                    Some(AudioEvent::PerfectAlignment { pitch_hz })
                } else {
                    Some(AudioEvent::OffTrackOrCrcError)
                }
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radar_pitch_calculation_bounds() {
        // Lower bound (30% -> 440 Hz)
        assert_eq!(calculate_radar_pitch(30), 440);
        // Under lower bound clamped to 30% -> 440 Hz
        assert_eq!(calculate_radar_pitch(0), 440);
        assert_eq!(calculate_radar_pitch(20), 440);

        // Upper bound (100% -> 1760 Hz)
        assert_eq!(calculate_radar_pitch(100), 1760);
        // Over upper bound clamped to 100% -> 1760 Hz
        assert_eq!(calculate_radar_pitch(120), 1760);

        // Midpoint: (65 - 30) / 70 = 0.5 -> 440 + 0.5 * 1320 = 1100 Hz
        assert_eq!(calculate_radar_pitch(65), 1100);
    }

    #[test]
    fn test_evaluate_alignment_both_mode_ok() {
        let pass_h0 = DiagnosticPass::with_details(
            40, 0, 500,
            "T:40 H:0".into(), "T:40 H:0".into(),
            18, 18, 0, 95, true,
        );
        let pass_h1 = DiagnosticPass::with_details(
            40, 1, 500,
            "T:40 H:1".into(), "T:40 H:1".into(),
            18, 18, 0, 85, true,
        );

        let event = evaluate_alignment_audio_event(
            HeadSelection::Both,
            40,
            1,
            Some(&pass_h0),
            Some(&pass_h1),
            18,
        );

        // Reference quality is min(95, 85) = 85%
        let expected_pitch = calculate_radar_pitch(85);
        assert_eq!(event, Some(AudioEvent::PerfectAlignment { pitch_hz: expected_pitch }));
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
}
