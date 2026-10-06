//! Responsive breakpoints. Pure functions of the window width so they can be unit-tested; the
//! views only read the numbers.

/// Smallest window the app allows (logical points).
pub const MIN_WINDOW: [f32; 2] = [640.0, 480.0];

/// Below this width the sidebar shrinks to an icon rail.
pub const COMPACT_BELOW: f32 = 1000.0;

const SIDEBAR_FULL: f32 = 232.0;
const SIDEBAR_RAIL: f32 = 72.0;

/// Page chrome for the current window width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chrome {
    /// Sidebar is an icon rail without labels.
    pub compact: bool,
    pub sidebar_width: f32,
    /// Horizontal padding of the page area and the top bar.
    pub page_margin: i8,
}

pub fn chrome(window_width: f32) -> Chrome {
    let compact = window_width < COMPACT_BELOW;
    Chrome {
        compact,
        sidebar_width: if compact { SIDEBAR_RAIL } else { SIDEBAR_FULL },
        page_margin: if window_width < 760.0 { 14 } else { 24 },
    }
}

/// How the bottom player bar divides its width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerBar {
    /// Width of the now-playing chip on the left.
    pub left: f32,
    /// Width of the volume / queue cluster on the right.
    pub right: f32,
    /// Title and artist next to the cover (otherwise the cover only).
    pub show_text: bool,
    /// Volume slider next to the mute button.
    pub show_volume: bool,
    /// Shuffle and repeat buttons around the transport.
    pub show_shuffle_repeat: bool,
}

/// `inner` is the bar's width without its outer padding.
pub fn player_bar(inner: f32) -> PlayerBar {
    if inner >= 900.0 {
        let side = (inner * 0.28).clamp(200.0, 420.0);
        PlayerBar {
            left: side,
            right: side,
            show_text: true,
            show_volume: true,
            show_shuffle_repeat: true,
        }
    } else if inner >= 700.0 {
        PlayerBar {
            left: 220.0,
            right: 136.0,
            show_text: true,
            show_volume: false,
            show_shuffle_repeat: true,
        }
    } else {
        // Cover + heart on the left; queue and now-playing buttons on the right.
        PlayerBar {
            left: 96.0,
            right: 92.0,
            show_text: false,
            show_volume: false,
            show_shuffle_repeat: inner >= 560.0,
        }
    }
}

/// Width for modal dialogs: 420 when it fits, otherwise the window minus a margin.
pub fn dialog_width(window_width: f32) -> f32 {
    420.0_f32.min((window_width - 64.0).max(240.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_becomes_a_rail_below_the_breakpoint() {
        assert!(!chrome(1280.0).compact);
        assert_eq!(chrome(1280.0).sidebar_width, 232.0);
        assert!(chrome(999.0).compact);
        assert_eq!(chrome(MIN_WINDOW[0]).sidebar_width, 72.0);
        assert_eq!(chrome(700.0).page_margin, 14);
        assert_eq!(chrome(1000.0).page_margin, 24);
    }

    #[test]
    fn transport_always_has_room_between_the_player_bar_sides() {
        // Transport row: 4 buttons of 38 + play button 44 + gaps (see player_bar.rs); with shuffle and
        // repeat hidden only 2 buttons + play are needed.
        for w in (600..=2200).step_by(20) {
            let inner = w as f32 - 40.0;
            let p = player_bar(inner);
            let centre = inner - p.left - p.right - 24.0;
            let need = if p.show_shuffle_repeat { 236.0 } else { 160.0 };
            assert!(centre >= need, "window {w}: centre {centre} < {need}");
        }
    }

    #[test]
    fn dialogs_fit_the_smallest_window() {
        assert_eq!(dialog_width(1280.0), 420.0);
        assert!(dialog_width(MIN_WINDOW[0]) <= MIN_WINDOW[0] - 64.0);
    }
}
