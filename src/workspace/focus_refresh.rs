//! Window-activation refresh policy for the active repository tab.

use std::time::{Duration, Instant};

use gpui::{Context, Window};

use super::Workspace;

/// Minimum interval between two focus-triggered refreshes. Also suppresses
/// the activation delivered right after window creation, while the initial
/// repository load is still in flight.
const FOCUS_REFRESH_COOLDOWN: Duration = Duration::from_secs(2);

/// Whether a window activation at `now` should refresh the repository.
fn should_refresh_on_focus(
    last_refresh: Option<Instant>,
    now: Instant,
    cooldown: Duration,
) -> bool {
    match last_refresh {
        None => true,
        Some(last) => now.saturating_duration_since(last) >= cooldown,
    }
}

impl Workspace {
    /// Refresh the active repository when the window regains activation.
    /// The observer fires on activation and deactivation alike; only the
    /// activation branch does work.
    pub(super) fn handle_window_activation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !window.is_window_active() {
            return;
        }
        if !self.config.view.auto_refresh_on_focus {
            log::debug!("[workspace] focus refresh disabled in settings");
            return;
        }
        let now = Instant::now();
        if !should_refresh_on_focus(
            self.last_focus_refresh,
            now,
            FOCUS_REFRESH_COOLDOWN,
        ) {
            log::debug!("[workspace] focus refresh skipped: cooldown active");
            return;
        }
        let Some(tab) = self.active_tab_entity() else {
            log::debug!("[workspace] focus refresh skipped: no repository tab");
            return;
        };
        if tab.update(cx, |tab, cx| tab.refresh_on_focus(cx)) {
            self.last_focus_refresh = Some(now);
            log::info!("[workspace] focus refresh requested");
        } else {
            log::debug!("[workspace] focus refresh skipped: tab busy");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{FOCUS_REFRESH_COOLDOWN, should_refresh_on_focus};

    #[test]
    fn first_activation_refreshes_when_no_refresh_recorded() {
        assert!(should_refresh_on_focus(
            None,
            Instant::now(),
            FOCUS_REFRESH_COOLDOWN
        ));
    }

    #[test]
    fn activation_within_cooldown_is_skipped() {
        let now = Instant::now();
        let last = now - Duration::from_millis(1999);
        assert!(!should_refresh_on_focus(
            Some(last),
            now,
            FOCUS_REFRESH_COOLDOWN
        ));
    }

    #[test]
    fn activation_after_cooldown_refreshes() {
        let now = Instant::now();
        let last = now - Duration::from_millis(2001);
        assert!(should_refresh_on_focus(
            Some(last),
            now,
            FOCUS_REFRESH_COOLDOWN
        ));
    }

    #[test]
    fn activation_exactly_at_cooldown_refreshes() {
        let now = Instant::now();
        let last = now - FOCUS_REFRESH_COOLDOWN;
        assert!(should_refresh_on_focus(
            Some(last),
            now,
            FOCUS_REFRESH_COOLDOWN
        ));
    }

    #[test]
    fn very_old_last_refresh_refreshes() {
        let now = Instant::now();
        let last = now - Duration::from_secs(3600);
        assert!(should_refresh_on_focus(
            Some(last),
            now,
            FOCUS_REFRESH_COOLDOWN
        ));
    }
}
