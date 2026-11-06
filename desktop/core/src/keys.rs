//! The dictation key's rules (D-015, M1 plan step 5), without any Windows code so every case is tested here.
//! The Windows hook feeds every key event in and does what comes back.
//!
//! - Key down → start recording at once (so a hold loses nothing).
//! - Up again within [`HOLD_MS`] → toggle mode: recording goes on until the key is tapped again or Enter is
//!   pressed (stop and transcribe), or Esc (cancel).
//! - Held for [`HOLD_MS`] or longer → push-to-talk: release stops and transcribes.
//! - Any other key while the dictation key is down from idle → cancel: it was a shortcut (Right Ctrl + C).
//! - Enter and Esc are swallowed only while recording; nothing else is ever swallowed, and the dictation key
//!   itself always reaches the app.

/// D-015: a tap is under 300 ms and a hold over 400 ms. A press in between counts as a (slow) tap: staying in
/// toggle mode loses nothing, while stopping after less than 0.4 s would send a clip with no words in it.
pub const HOLD_MS: u32 = 400;

/// A key event, sorted by what it means here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// The dictation key (Right Ctrl by default, D-018).
    Dictation,
    Enter,
    Esc,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Open the mic and start recording.
    Start,
    /// Stop recording and transcribe.
    Stop,
    /// Stop recording and throw the clip away.
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reply {
    pub action: Option<Action>,
    /// Keep the event from the app (only Enter / Esc while recording, and their key-up).
    pub swallow: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    /// Dictation key down from idle, recording; `since` is the key-down time.
    Pressed {
        since: u32,
    },
    /// Recording in toggle mode, key up.
    Toggled,
    /// Recording in toggle mode, key down again; `other` = another key was pressed meanwhile (a shortcut).
    ToggledPressed {
        other: bool,
    },
    /// Not recording; wait for the dictation key to come up before anything else.
    WaitUp,
}

#[derive(Debug)]
pub struct Machine {
    state: State,
    /// A swallowed key whose key-up must be swallowed too.
    swallowed: Option<Key>,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            swallowed: None,
        }
    }

    pub fn recording(&self) -> bool {
        matches!(
            self.state,
            State::Pressed { .. } | State::Toggled | State::ToggledPressed { .. }
        )
    }

    /// A key went down; `time_ms` is the event's time stamp (any clock that counts milliseconds).
    /// Auto-repeat sends more key-downs while a key is held; they are handled like the first.
    pub fn down(&mut self, key: Key, time_ms: u32) -> Reply {
        use State::*;
        let (state, action) = match (self.state, key) {
            (Idle, Key::Dictation) => (Pressed { since: time_ms }, Some(Action::Start)),
            (Pressed { .. } | ToggledPressed { .. }, Key::Dictation) => (self.state, None),
            (Toggled, Key::Dictation) => (ToggledPressed { other: false }, None),
            (Pressed { .. } | Toggled | ToggledPressed { .. }, Key::Enter) => {
                (self.after_stop(), Some(Action::Stop))
            }
            (Pressed { .. } | Toggled | ToggledPressed { .. }, Key::Esc) => {
                (self.after_stop(), Some(Action::Cancel))
            }
            (Pressed { .. }, Key::Other) => (WaitUp, Some(Action::Cancel)),
            (ToggledPressed { .. }, Key::Other) => (ToggledPressed { other: true }, None),
            (state, _) => (state, None),
        };
        let swallow = self.recording() && matches!(key, Key::Enter | Key::Esc);
        if swallow {
            self.swallowed = Some(key);
        }
        self.state = state;
        Reply { action, swallow }
    }

    pub fn up(&mut self, key: Key, time_ms: u32) -> Reply {
        use State::*;
        let swallow = key != Key::Dictation && self.swallowed == Some(key);
        if swallow {
            self.swallowed = None;
        }
        let (state, action) = match (self.state, key) {
            (Pressed { since }, Key::Dictation) if time_ms.wrapping_sub(since) < HOLD_MS => {
                (Toggled, None)
            }
            (Pressed { .. }, Key::Dictation) => (Idle, Some(Action::Stop)),
            (ToggledPressed { other: false }, Key::Dictation) => (Idle, Some(Action::Stop)),
            (ToggledPressed { other: true }, Key::Dictation) => (Toggled, None),
            (WaitUp, Key::Dictation) => (Idle, None),
            (state, _) => (state, None),
        };
        self.state = state;
        Reply { action, swallow }
    }

    /// The app ended the recording itself (the 5-minute cap, or the mic would not open): back to not
    /// recording, so Enter and Esc reach the app again. `Some(Stop)` if a recording was running.
    pub fn force_stop(&mut self) -> Option<Action> {
        if !self.recording() {
            return None;
        }
        self.state = self.after_stop();
        Some(Action::Stop)
    }

    /// A click on the pop-up mic (M2 phase 5, D-016): starts a recording in toggle mode (the key, Enter or
    /// another click stops it), or stops the toggled one. Ignored while the dictation key is held.
    pub fn click(&mut self) -> Option<Action> {
        let (state, action) = match self.state {
            State::Idle => (State::Toggled, Some(Action::Start)),
            State::Toggled => (State::Idle, Some(Action::Stop)),
            other => (other, None),
        };
        self.state = state;
        action
    }

    /// Where to go when recording ends while the dictation key may still be down.
    fn after_stop(&self) -> State {
        match self.state {
            State::Pressed { .. } | State::ToggledPressed { .. } => State::WaitUp,
            _ => State::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action::*;
    use Key::*;

    fn act(r: Reply) -> Option<Action> {
        r.action
    }

    #[test]
    fn a_click_starts_in_toggle_mode_and_the_key_or_a_click_stops() {
        let mut m = Machine::new();
        assert_eq!(m.click(), Some(Start));
        assert!(m.recording());
        // The key's tap stops it, as in toggle mode.
        assert_eq!(act(m.down(Dictation, 1000)), None);
        assert_eq!(act(m.up(Dictation, 1050)), Some(Stop));
        assert_eq!(m.click(), Some(Start));
        assert_eq!(m.click(), Some(Stop));
        assert!(!m.recording());
        // Enter stops a clicked recording too.
        m.click();
        assert_eq!(act(m.down(Enter, 0)), Some(Stop));
        // While the key is held, a click does nothing.
        let mut held = Machine::new();
        held.down(Dictation, 0);
        assert_eq!(held.click(), None);
    }

    #[test]
    fn tap_starts_and_a_second_tap_stops() {
        let mut m = Machine::new();
        assert_eq!(act(m.down(Dictation, 1000)), Some(Start));
        assert_eq!(act(m.up(Dictation, 1120)), None);
        assert!(m.recording(), "toggle mode keeps recording");
        assert_eq!(act(m.down(Dictation, 5000)), None);
        assert_eq!(act(m.up(Dictation, 5100)), Some(Stop));
        assert!(!m.recording());
    }

    #[test]
    fn hold_is_push_to_talk() {
        let mut m = Machine::new();
        assert_eq!(act(m.down(Dictation, 0)), Some(Start));
        for t in (30..3000).step_by(30) {
            assert_eq!(act(m.down(Dictation, t)), None, "auto-repeat is ignored");
        }
        assert_eq!(act(m.up(Dictation, 3000)), Some(Stop));
        assert!(!m.recording());
    }

    #[test]
    fn the_gap_between_tap_and_hold_counts_as_a_tap() {
        let mut m = Machine::new();
        m.down(Dictation, 0);
        assert_eq!(act(m.up(Dictation, 350)), None);
        assert!(m.recording());
        let mut m = Machine::new();
        m.down(Dictation, 0);
        assert_eq!(act(m.up(Dictation, HOLD_MS)), Some(Stop));
    }

    #[test]
    fn enter_stops_and_esc_cancels_and_both_are_swallowed_only_while_recording() {
        let mut m = Machine::new();
        assert_eq!(
            m.down(Enter, 0),
            Reply::default(),
            "Enter works normally when idle"
        );
        assert_eq!(m.up(Enter, 10), Reply::default());
        m.down(Dictation, 100);
        m.up(Dictation, 150);
        assert_eq!(
            m.down(Enter, 900),
            Reply {
                action: Some(Stop),
                swallow: true
            }
        );
        assert_eq!(
            m.up(Enter, 950),
            Reply {
                action: None,
                swallow: true
            },
            "and its key-up"
        );
        assert_eq!(m.down(Enter, 2000), Reply::default(), "back to normal");

        m.down(Dictation, 3000);
        m.up(Dictation, 3050);
        assert_eq!(
            m.down(Esc, 4000),
            Reply {
                action: Some(Cancel),
                swallow: true
            }
        );
        assert!(m.up(Esc, 4050).swallow);
        assert!(!m.recording());
    }

    #[test]
    fn a_shortcut_with_the_key_cancels_and_is_not_swallowed() {
        let mut m = Machine::new();
        assert_eq!(act(m.down(Dictation, 0)), Some(Start));
        assert_eq!(
            m.down(Other, 80),
            Reply {
                action: Some(Cancel),
                swallow: false
            }
        );
        assert_eq!(m.up(Other, 120), Reply::default());
        assert_eq!(
            act(m.up(Dictation, 200)),
            None,
            "no toggle after a shortcut"
        );
        assert!(!m.recording());
        assert_eq!(act(m.down(Dictation, 1000)), Some(Start), "next tap works");
    }

    #[test]
    fn typing_and_shortcuts_during_toggle_mode_keep_recording() {
        let mut m = Machine::new();
        m.down(Dictation, 0);
        m.up(Dictation, 100);
        assert_eq!(
            m.down(Other, 500),
            Reply::default(),
            "typing passes through"
        );
        assert_eq!(act(m.down(Dictation, 900)), None);
        assert_eq!(
            act(m.down(Other, 950)),
            None,
            "Right Ctrl + C while recording"
        );
        assert_eq!(act(m.up(Dictation, 1000)), None);
        assert!(m.recording(), "a shortcut is not a stop tap");
        assert_eq!(act(m.down(Dictation, 2000)), None);
        assert_eq!(act(m.up(Dictation, 2050)), Some(Stop));
    }

    #[test]
    fn enter_while_holding_stops_and_the_release_does_nothing() {
        let mut m = Machine::new();
        m.down(Dictation, 0);
        assert_eq!(act(m.down(Enter, 2000)), Some(Stop));
        assert_eq!(act(m.up(Dictation, 2500)), None);
        assert_eq!(act(m.down(Dictation, 3000)), Some(Start));
    }

    #[test]
    fn force_stop_ends_recording_once() {
        let mut m = Machine::new();
        assert_eq!(m.force_stop(), None);
        m.down(Dictation, 0);
        m.up(Dictation, 100);
        assert_eq!(m.force_stop(), Some(Stop));
        assert_eq!(m.force_stop(), None);
        assert_eq!(
            m.down(Enter, 500),
            Reply::default(),
            "Enter reaches the app again"
        );
    }

    #[test]
    fn tick_counter_wrapping_around_is_fine() {
        let mut m = Machine::new();
        m.down(Dictation, u32::MAX - 50);
        assert_eq!(
            act(m.up(Dictation, 50)),
            None,
            "101 ms across the wrap is a tap"
        );
    }
}
