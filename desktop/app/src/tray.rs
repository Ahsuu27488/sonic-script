//! What the tray shows (M2 task 2.3): the state, its tooltip and its icon, worked out from dictation events.
//! Pure, so it is tested without a tray; `main.rs` puts the result on the real tray icon.
//! The tooltip never holds dictated text: anyone glancing at the screen could read it.

use sonicscript::events::{Event, Paste};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Starting,
    Idle,
    Recording,
    /// The recording is on its way to text.
    Working,
    Paused,
    Problem,
}

/// Everything the tray needs to draw itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    state: State,
    /// The user paused dictation from the menu.
    pub paused: bool,
    /// The last thing that went wrong; cleared by the next text that was pasted or copied.
    problem: Option<String>,
    /// A one-off note for the tooltip (e.g. "copied, press Ctrl+V"); cleared by the next recording.
    note: Option<String>,
    /// The dictation key's name, from `Ready`.
    key: String,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            state: State::Starting,
            paused: false,
            problem: None,
            note: None,
            key: String::new(),
        }
    }
}

/// Windows cuts a tray tooltip at 127 characters.
const TOOLTIP_MAX: usize = 127;

impl Status {
    /// What the icon shows: paused wins, except while a recording still runs.
    pub fn shown(&self) -> State {
        if self.paused && self.state != State::Recording {
            State::Paused
        } else {
            self.state
        }
    }

    /// The state to go back to after a recording.
    fn rest(&self) -> State {
        if self.problem.is_some() {
            State::Problem
        } else {
            State::Idle
        }
    }

    /// Forgets the last problem (it was solved outside dictation, e.g. the other copy quit).
    pub fn clear_problem(&mut self) {
        self.problem = None;
        if self.state == State::Problem {
            self.state = State::Idle;
        }
    }

    /// The state after a finished job: a recording that started meanwhile keeps showing.
    fn after_job(&self) -> State {
        if self.state == State::Recording {
            State::Recording
        } else {
            self.rest()
        }
    }

    /// The first line: the menu's status item and the tooltip's start.
    pub fn headline(&self) -> String {
        match self.shown() {
            State::Starting => "Starting…".into(),
            State::Idle if self.key.is_empty() => "Ready".into(),
            State::Idle => format!("Ready: press {} to dictate", self.key),
            State::Recording => "Listening…".into(),
            State::Working => "Writing…".into(),
            State::Paused => "Paused".into(),
            State::Problem => self.problem.clone().unwrap_or_else(|| "Problem".into()),
        }
    }

    pub fn tooltip(&self) -> String {
        let mut t = format!("SonicScript: {}", self.headline());
        if let Some(note) = &self.note {
            t.push('\n');
            t.push_str(note);
        }
        if t.chars().count() > TOOLTIP_MAX {
            t = t.chars().take(TOOLTIP_MAX - 1).collect::<String>() + "…";
        }
        t
    }
}

/// Applies one dictation event; true when the tray must be redrawn.
pub fn apply(s: &mut Status, e: &Event) -> bool {
    let before = s.clone();
    match e {
        Event::Ready { key, .. } => {
            s.key = key.clone();
            s.state = s.rest();
        }
        // Timing lines and fallback notes belong in a log, not on the tray.
        Event::Notice(_) | Event::CapReached => {}
        Event::Warning(w) => {
            s.problem = Some(w.clone());
            if s.state != State::Recording && s.state != State::Working {
                s.state = State::Problem;
            }
        }
        Event::Listening { .. } => {
            s.note = None;
            s.state = State::Recording;
        }
        Event::Processing => s.state = State::Working,
        // Only the dictation loop sends it, and only for the running recording.
        Event::Cancelled => s.state = s.rest(),
        // A job's end, from its worker thread: the next recording may already run (a slow answer).
        Event::NoSpeech { .. } | Event::HintOnly => s.state = s.after_job(),
        Event::Text { paste, .. } => {
            match paste {
                Paste::Failed(why) => s.problem = Some(format!("Not pasted: {why}")),
                Paste::Copied(why) => {
                    s.problem = None;
                    s.note = Some(format!("Copied, not pasted ({why}): press Ctrl+V"));
                }
                Paste::Pasted(_) | Paste::Off => s.problem = None,
            }
            s.state = s.after_job();
        }
        Event::Failed { lines } => {
            let first = lines.first().map(String::as_str).unwrap_or("Nothing typed");
            s.problem = Some(first.trim_start_matches("! ").to_string());
            s.state = s.after_job();
        }
    }
    *s != before
}

/// The tray icon's size in pixels (Windows scales it to the tray).
pub const ICON_SIZE: u32 = 32;
/// `--accent` "Ember" (dark theme), oklch(0.80 0.14 70) in sRGB.
pub const ACCENT: [u8; 3] = [247, 172, 77];
/// `--danger`, oklch(0.70 0.17 25) in sRGB.
pub const DANGER: [u8; 3] = [246, 109, 103];
/// Paused: a neutral grey, readable on light and dark taskbars.
pub const GREY: [u8; 3] = [150, 150, 150];

/// One painted shape: its colour and whether a point (in pixel units) is inside it.
type Layer<'a> = ([u8; 3], Box<dyn Fn(f32, f32) -> bool + 'a>);

/// The Voice Ring for a state as RGBA pixels (`ICON_SIZE` square): a ring; filled while recording, a dot in
/// the middle while writing, grey while paused, a red dot at the lower right on a problem.
pub fn icon(state: State) -> Vec<u8> {
    let n = ICON_SIZE as f32;
    let c = n / 2.0;
    let ring = if state == State::Paused { GREY } else { ACCENT };
    // (colour, test) layers, painted in order; each test gets a point in pixel units.
    let in_ring = |x: f32, y: f32| {
        let d = ((x - c).powi(2) + (y - c).powi(2)).sqrt();
        (9.0..=13.5).contains(&d)
    };
    let in_disc = |r: f32| move |x: f32, y: f32| ((x - c).powi(2) + (y - c).powi(2)).sqrt() <= r;
    let in_badge = |x: f32, y: f32| ((x - 25.0).powi(2) + (y - 25.0).powi(2)).sqrt() <= 6.0;
    let mut layers: Vec<Layer> = vec![(ring, Box::new(in_ring))];
    match state {
        State::Recording => layers.push((ACCENT, Box::new(in_disc(6.5)))),
        State::Working => layers.push((ACCENT, Box::new(in_disc(3.5)))),
        State::Problem => layers.push((DANGER, Box::new(in_badge))),
        State::Starting | State::Idle | State::Paused => {}
    }
    let mut px = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    const SUB: u32 = 4;
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let i = ((y * ICON_SIZE + x) * 4) as usize;
            for (colour, test) in &layers {
                // Coverage from 4x4 samples: smooth edges.
                let mut hits = 0;
                for sy in 0..SUB {
                    for sx in 0..SUB {
                        let fx = x as f32 + (sx as f32 + 0.5) / SUB as f32;
                        let fy = y as f32 + (sy as f32 + 0.5) / SUB as f32;
                        hits += u32::from(test(fx, fy));
                    }
                }
                if hits == 0 {
                    continue;
                }
                let a = (hits * 255 / (SUB * SUB)) as u8;
                // Painted over what is there (premultiplied would be overkill at 32 px).
                if a >= px[i + 3] || hits == SUB * SUB {
                    px[i..i + 3].copy_from_slice(colour);
                }
                px[i + 3] = px[i + 3].max(a);
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Status {
        let mut s = Status::default();
        apply(
            &mut s,
            &Event::Ready {
                engines: "chain".into(),
                key: "Right Ctrl".into(),
            },
        );
        s
    }

    fn listening() -> Event {
        Event::Listening {
            device: "Mic".into(),
            rate: 48000,
            mic_ms: 2,
            prepared: true,
        }
    }

    fn text(paste: Paste) -> Event {
        Event::Text {
            text: "secret words".into(),
            paste,
            details: "1.0 s".into(),
        }
    }

    #[test]
    fn a_dictation_goes_round_the_states() {
        let mut s = Status::default();
        assert_eq!(s.shown(), State::Starting);
        s = ready();
        assert_eq!(s.shown(), State::Idle);
        assert_eq!(
            s.tooltip(),
            "SonicScript: Ready: press Right Ctrl to dictate"
        );
        assert!(apply(&mut s, &listening()));
        assert_eq!(s.shown(), State::Recording);
        assert!(apply(&mut s, &Event::Processing));
        assert_eq!(s.shown(), State::Working);
        assert!(apply(&mut s, &text(Paste::Pasted(300))));
        assert_eq!(s.shown(), State::Idle);
        assert!(
            !s.tooltip().contains("secret"),
            "dictated text never in the tooltip"
        );
        assert!(!apply(&mut s, &Event::Notice("Used Flash".into())));
    }

    #[test]
    fn copied_text_leaves_a_note_until_the_next_recording() {
        let mut s = ready();
        apply(&mut s, &text(Paste::Copied("administrator window".into())));
        assert_eq!(s.shown(), State::Idle);
        assert!(s.tooltip().contains("press Ctrl+V"), "{}", s.tooltip());
        assert!(!s.tooltip().contains("secret"));
        apply(&mut s, &listening());
        assert!(!s.tooltip().contains("Ctrl+V"));
    }

    #[test]
    fn problems_show_until_text_comes_through() {
        let mut s = ready();
        apply(
            &mut s,
            &Event::Failed {
                lines: vec![
                    "! Nothing typed: no model answered".into(),
                    "Try again".into(),
                ],
            },
        );
        assert_eq!(s.shown(), State::Problem);
        assert_eq!(s.headline(), "Nothing typed: no model answered");
        // A cancelled recording goes back to the problem, a pasted text clears it.
        apply(&mut s, &listening());
        apply(&mut s, &Event::Cancelled);
        assert_eq!(s.shown(), State::Problem);
        apply(&mut s, &listening());
        apply(&mut s, &Event::Processing);
        apply(&mut s, &text(Paste::Pasted(1)));
        assert_eq!(s.shown(), State::Idle);
        // A warning while recording waits until the recording is over.
        apply(&mut s, &listening());
        apply(&mut s, &Event::Warning("history: disk full".into()));
        assert_eq!(s.shown(), State::Recording);
        apply(&mut s, &Event::Processing);
        apply(&mut s, &Event::NoSpeech { seconds: 1.0 });
        assert_eq!(s.shown(), State::Problem);
        apply(&mut s, &text(Paste::Failed("no window".into())));
        assert_eq!(s.headline(), "Not pasted: no window");
    }

    #[test]
    fn a_slow_answer_does_not_end_the_next_recording() {
        // Clip 1 is still being written when clip 2 starts; clip 1's answer must not show "Ready".
        let mut s = ready();
        apply(&mut s, &listening());
        apply(&mut s, &Event::Processing);
        apply(&mut s, &listening());
        apply(&mut s, &text(Paste::Pasted(1)));
        assert_eq!(s.shown(), State::Recording);
        apply(&mut s, &Event::NoSpeech { seconds: 1.0 });
        assert_eq!(s.shown(), State::Recording);
        apply(
            &mut s,
            &Event::Failed {
                lines: vec!["Nothing typed".into()],
            },
        );
        assert_eq!(s.shown(), State::Recording);
        // The problem is kept for when the recording ends.
        apply(&mut s, &Event::Cancelled);
        assert_eq!(s.shown(), State::Problem);
    }

    #[test]
    fn paused_shows_except_during_a_recording() {
        let mut s = ready();
        s.paused = true;
        assert_eq!(s.shown(), State::Paused);
        assert_eq!(s.headline(), "Paused");
        s.state = State::Recording;
        assert_eq!(s.shown(), State::Recording);
    }

    #[test]
    fn long_tooltips_are_cut() {
        let mut s = ready();
        apply(&mut s, &Event::Warning("x".repeat(300)));
        let t = s.tooltip();
        assert_eq!(t.chars().count(), TOOLTIP_MAX);
        assert!(t.ends_with('…'));
    }

    fn count(px: &[u8], colour: [u8; 3]) -> usize {
        px.chunks(4)
            .filter(|p| p[3] > 200 && p[..3] == colour)
            .count()
    }

    #[test]
    fn icons_draw_each_state() {
        for state in [
            State::Starting,
            State::Idle,
            State::Recording,
            State::Working,
            State::Paused,
            State::Problem,
        ] {
            assert_eq!(icon(state).len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        }
        let idle = icon(State::Idle);
        assert!(count(&idle, ACCENT) > 100, "the ring is drawn");
        assert_eq!(count(&idle, DANGER), 0);
        assert!(count(&icon(State::Recording), ACCENT) > count(&idle, ACCENT) + 80);
        assert!(count(&icon(State::Working), ACCENT) > count(&idle, ACCENT));
        let paused = icon(State::Paused);
        assert_eq!(count(&paused, ACCENT), 0, "paused has no accent");
        assert!(count(&paused, GREY) > 100);
        assert!(count(&icon(State::Problem), DANGER) > 40);
        // The middle of the ring is see-through when idle.
        let mid = ((ICON_SIZE / 2 * ICON_SIZE + ICON_SIZE / 2) * 4 + 3) as usize;
        assert_eq!(idle[mid], 0);
    }
}
