//! Tray icon visuals (spec §3.8): the tray-level state, its tooltip line,
//! and simple programmatically drawn RGBA icons. Polished icon assets are
//! Phase 5; these stay legible at taskbar size by using one bold shape per
//! state, and the tooltip always carries the words (guardrails §3: color
//! never conveys a state alone).

use crate::pipeline::PipelineStatus;
use crate::theme;
use egui::Color32;

/// Icon bitmap edge in pixels; the OS scales down as needed.
pub const SIZE: u32 = 32;

/// Windows caps tray tooltips at 127 characters and cuts longer strings
/// mid-word, so we truncate ourselves with an ellipsis.
const TOOLTIP_MAX: usize = 127;

/// What the icon can express: `PipelineStatus` with the failure causes
/// collapsed into needs-key / failed / stopped groups. The tooltip carries
/// the detail text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrayState {
    /// Accent ring: listening for the chord.
    Idle,
    /// Red disc: capturing audio.
    Recording,
    /// Accent disc: request in flight.
    Processing,
    /// Amber disc + "!": key missing or rejected (Settings fixes it).
    NeedsKey,
    /// Red disc + "!": the last dictation failed (network, provider,
    /// injection); the pipeline is still listening.
    Error,
    /// Gray disc + "!": pipeline not running for a non-key reason.
    Stopped,
}

pub fn state(status: &PipelineStatus) -> TrayState {
    match status {
        PipelineStatus::Idle => TrayState::Idle,
        PipelineStatus::Recording => TrayState::Recording,
        PipelineStatus::Processing => TrayState::Processing,
        PipelineStatus::Errored {
            key_related: true, ..
        }
        | PipelineStatus::Stopped {
            key_related: true, ..
        } => TrayState::NeedsKey,
        PipelineStatus::Errored { .. } => TrayState::Error,
        PipelineStatus::Stopped { .. } => TrayState::Stopped,
    }
}

/// One honest line per state. `chord` is the configured PTT chord.
pub fn tooltip(status: &PipelineStatus, chord: &str) -> String {
    let line = match status {
        PipelineStatus::Idle => format!("Hark: listening for {chord}"),
        PipelineStatus::Recording => "Hark: recording".to_string(),
        PipelineStatus::Processing => "Hark: processing".to_string(),
        PipelineStatus::Errored { detail, .. } => {
            format!("Hark: last dictation failed. {detail}")
        }
        PipelineStatus::Stopped {
            key_related: true, ..
        } => "Hark: no STT key. Open Settings from the tray menu.".to_string(),
        PipelineStatus::Stopped { detail, .. } => format!("Hark: stopped. {detail}"),
    };
    truncate(line)
}

fn truncate(line: String) -> String {
    if line.chars().count() <= TOOLTIP_MAX {
        return line;
    }
    let mut cut: String = line.chars().take(TOOLTIP_MAX - 1).collect();
    cut.push('\u{2026}');
    cut
}

/// Disc radius and the hole radius of the idle ring, in pixels.
const OUTER: f32 = 13.0;
const INNER: f32 = 8.0;

/// Draw the icon for a state as straight (non-premultiplied) RGBA,
/// `SIZE * SIZE * 4` bytes.
pub fn rgba(state: TrayState) -> Vec<u8> {
    let (color, hollow, bang) = match state {
        TrayState::Idle => (theme::TRAY_ACCENT, true, false),
        TrayState::Recording => (theme::DANGER, false, false),
        TrayState::Processing => (theme::TRAY_ACCENT, false, false),
        TrayState::NeedsKey => (theme::WARNING, false, true),
        TrayState::Error => (theme::DANGER, false, true),
        TrayState::Stopped => (theme::TRAY_STOPPED, false, true),
    };
    let size = SIZE as usize;
    let mut buf = vec![0u8; size * size * 4];
    let center = SIZE as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            // 1.0 deep inside the disc, 0.0 outside, a ~1 px soft edge.
            let mut alpha = (OUTER - dist + 0.5).clamp(0.0, 1.0);
            if hollow {
                alpha = alpha.min((dist - INNER + 0.5).clamp(0.0, 1.0));
            }
            if alpha <= 0.0 {
                continue;
            }
            let px = if bang && in_bang(x, y) {
                Color32::WHITE
            } else {
                color
            };
            let i = (y * size + x) * 4;
            buf[i] = px.r();
            buf[i + 1] = px.g();
            buf[i + 2] = px.b();
            buf[i + 3] = (alpha * 255.0) as u8;
        }
    }
    buf
}

/// The exclamation mark: a 4 px wide bar and a square dot, centered.
fn in_bang(x: usize, y: usize) -> bool {
    (14..=17).contains(&x) && ((9..=16).contains(&y) || (19..=22).contains(&y))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [TrayState; 6] = [
        TrayState::Idle,
        TrayState::Recording,
        TrayState::Processing,
        TrayState::NeedsKey,
        TrayState::Error,
        TrayState::Stopped,
    ];

    fn pixel(buf: &[u8], x: usize, y: usize) -> [u8; 4] {
        let i = (y * SIZE as usize + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn every_state_draws_a_distinct_correctly_sized_icon() {
        let icons: Vec<_> = ALL.iter().map(|s| rgba(*s)).collect();
        for (i, icon) in icons.iter().enumerate() {
            assert_eq!(icon.len(), (SIZE * SIZE * 4) as usize, "{:?}", ALL[i]);
            for (j, other) in icons.iter().enumerate().skip(i + 1) {
                assert_ne!(icon, other, "{:?} vs {:?}", ALL[i], ALL[j]);
            }
        }
    }

    #[test]
    fn idle_is_a_ring_and_processing_a_disc() {
        // Same accent color; the hole is what tells them apart.
        let center = (SIZE / 2) as usize;
        assert_eq!(pixel(&rgba(TrayState::Idle), center, center)[3], 0);
        assert_eq!(pixel(&rgba(TrayState::Processing), center, center)[3], 255);
    }

    #[test]
    fn failure_states_carry_the_white_exclamation_mark() {
        for state in [TrayState::NeedsKey, TrayState::Error, TrayState::Stopped] {
            let [r, g, b, a] = pixel(&rgba(state), 16, 12);
            assert_eq!((r, g, b, a), (255, 255, 255, 255), "{state:?}");
        }
        // Recording is a plain disc: same spot stays the fill color.
        let [r, g, b, _] = pixel(&rgba(TrayState::Recording), 16, 12);
        assert_ne!((r, g, b), (255, 255, 255));
    }

    #[test]
    fn every_pipeline_status_maps_to_the_expected_tray_state() {
        let detail = "boom".to_string();
        let cases = [
            (PipelineStatus::Idle, TrayState::Idle),
            (PipelineStatus::Recording, TrayState::Recording),
            (PipelineStatus::Processing, TrayState::Processing),
            (
                PipelineStatus::Errored {
                    detail: detail.clone(),
                    key_related: true,
                },
                TrayState::NeedsKey,
            ),
            (
                PipelineStatus::Errored {
                    detail: detail.clone(),
                    key_related: false,
                },
                TrayState::Error,
            ),
            (
                PipelineStatus::Stopped {
                    detail: detail.clone(),
                    key_related: true,
                },
                TrayState::NeedsKey,
            ),
            (
                PipelineStatus::Stopped {
                    detail,
                    key_related: false,
                },
                TrayState::Stopped,
            ),
        ];
        for (status, expected) in cases {
            assert_eq!(state(&status), expected);
        }
    }

    #[test]
    fn tooltips_name_the_state_and_the_chord() {
        let idle = tooltip(&PipelineStatus::Idle, "Ctrl+Alt+Space");
        assert!(idle.contains("Ctrl+Alt+Space"), "{idle}");
        let errored = tooltip(
            &PipelineStatus::Errored {
                detail: "request to deepgram timed out".to_string(),
                key_related: false,
            },
            "F13",
        );
        assert!(errored.contains("deepgram timed out"), "{errored}");
    }

    #[test]
    fn long_details_truncate_to_the_windows_tooltip_cap() {
        let long = tooltip(
            &PipelineStatus::Stopped {
                detail: "x".repeat(300),
                key_related: false,
            },
            "F13",
        );
        assert_eq!(long.chars().count(), TOOLTIP_MAX);
        assert!(long.ends_with('\u{2026}'));
    }
}
