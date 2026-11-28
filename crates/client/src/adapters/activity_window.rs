use std::time::Duration;

use timetracker_model::ObservationKind;

#[derive(Debug, Default)]
pub(super) struct ActivityWindow {
    keypress_count: u64,
    mouse_click_count: u64,
    mouse_move_count: u64,
}

impl ActivityWindow {
    pub(super) fn record(&mut self, observation: &ObservationKind) {
        match observation {
            ObservationKind::KeyboardButtonPressed { .. } => {
                self.keypress_count = self.keypress_count.saturating_add(1);
            }
            ObservationKind::MouseClicked { .. } => {
                self.mouse_click_count = self.mouse_click_count.saturating_add(1);
            }
            ObservationKind::MouseMoved { .. } => {
                self.mouse_move_count = self.mouse_move_count.saturating_add(1);
            }
            _ => {}
        }
    }

    pub(super) fn summary_and_reset(&mut self, duration: Duration) -> ObservationKind {
        let summary = ObservationKind::MouseKeyboardActivitySummary {
            keypress_count: self.keypress_count,
            mouse_click_count: self.mouse_click_count,
            mouse_move_count: self.mouse_move_count,
            window_duration_ms: u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
        };

        self.keypress_count = 0;
        self.mouse_click_count = 0;
        self.mouse_move_count = 0;

        summary
    }

    pub(super) fn is_empty(&self) -> bool {
        self.keypress_count == 0 && self.mouse_click_count == 0 && self.mouse_move_count == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_keyboard_and_mouse_events() {
        let mut window = ActivityWindow::default();

        window.record(&ObservationKind::KeyboardButtonPressed {
            key: "A".to_owned(),
        });
        window.record(&ObservationKind::KeyboardButtonPressed {
            key: "B".to_owned(),
        });

        window.record(&ObservationKind::MouseClicked {
            button: "Left".to_owned(),
            x: Some(100),
            y: Some(200),
        });

        window.record(&ObservationKind::MouseMoved { x: 150, y: 250 });
        window.record(&ObservationKind::MouseMoved { x: 160, y: 260 });
        window.record(&ObservationKind::MouseMoved { x: 170, y: 270 });

        let summary = window.summary_and_reset(Duration::from_secs(10));

        assert_eq!(
            summary,
            ObservationKind::MouseKeyboardActivitySummary {
                keypress_count: 2,
                mouse_click_count: 1,
                mouse_move_count: 3,
                window_duration_ms: 10_000,
            }
        );
    }

    #[test]
    fn resets_counters_after_summary() {
        let mut window = ActivityWindow::default();

        window.record(&ObservationKind::KeyboardButtonPressed {
            key: "A".to_owned(),
        });

        let _first = window.summary_and_reset(Duration::from_secs(10));

        let second = window.summary_and_reset(Duration::from_secs(10));

        assert_eq!(
            second,
            ObservationKind::MouseKeyboardActivitySummary {
                keypress_count: 0,
                mouse_click_count: 0,
                mouse_move_count: 0,
                window_duration_ms: 10_000,
            }
        );
    }

    #[test]
    fn ignores_unrelated_observations() {
        let mut window = ActivityWindow::default();

        window.record(&ObservationKind::IdleChanged { idle: true });

        let summary = window.summary_and_reset(Duration::from_secs(5));

        assert_eq!(
            summary,
            ObservationKind::MouseKeyboardActivitySummary {
                keypress_count: 0,
                mouse_click_count: 0,
                mouse_move_count: 0,
                window_duration_ms: 5_000,
            }
        );
    }

    #[test]
    fn preserves_window_duration() {
        let mut window = ActivityWindow::default();

        let summary = window.summary_and_reset(Duration::from_millis(7_500));

        assert_eq!(
            summary,
            ObservationKind::MouseKeyboardActivitySummary {
                keypress_count: 0,
                mouse_click_count: 0,
                mouse_move_count: 0,
                window_duration_ms: 7_500,
            }
        );
    }

    #[test]
    fn window_is_empty_initially_and_after_reset() {
        let mut window = ActivityWindow::default();

        assert!(window.is_empty());

        window.record(&ObservationKind::KeyboardButtonPressed {
            key: "A".to_owned(),
        });

        assert!(!window.is_empty());

        window.summary_and_reset(Duration::from_secs(10));

        assert!(window.is_empty());
    }
}
