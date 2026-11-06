//! What the HUD Voice Ring shows (M2 task 3.1, DESIGN.md "HUD pill"), worked out from dictation events.
//! Pure, so it is tested without a window; `hudwin.rs` puts the result on the real window.
//! A view never holds dictated text: the pill floats over every app and anyone can read it.

use std::collections::VecDeque;

use serde::Serialize;
use sonicscript::events::{Event, Paste};

/// The pill's state (DESIGN.md: one at a time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Listening,
    Writing,
    Inserted,
    Copied,
    Problem,
}

/// The colour of the ring or dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Accent,
    Ok,
    Warn,
    Danger,
}

/// One state of the pill, as the web view gets it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct View {
    pub kind: Kind,
    pub tone: Tone,
    pub label: String,
    /// The pill leaves by itself after this long; `None` = until the next event.
    pub hide_after_ms: Option<u64>,
    /// A button on the pill ("Try again"); the window takes clicks only while there is one.
    pub action: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    None,
    Show(View),
    Hide,
}

/// How long each passing state stays (DESIGN.md: the tick 600 ms, "Copied" 2.5 s).
pub const INSERTED_MS: u64 = 600;
pub const COPIED_MS: u64 = 2500;
pub const NO_SPEECH_MS: u64 = 1800;
pub const LIMIT_MS: u64 = 4000;
pub const NOT_PASTED_MS: u64 = 5000;
pub const FAILED_MS: u64 = 6000;
/// The failure with its "Try again" button stays longer: the user has to reach it.
pub const RETRY_MS: u64 = 12_000;
pub const TRY_AGAIN: &str = "Try again";
/// The longest label the pill shows; longer ones are cut with "…".
pub const LABEL_MAX: usize = 72;

/// The HUD's memory between events.
#[derive(Debug, Default)]
pub struct Hud {
    recording: bool,
    /// The last failure's line, for the "Try again" view that follows it.
    last_failure: Option<String>,
    /// Recordings sent and not answered yet (a slow answer can still run when the next clip is sent).
    jobs: u32,
    /// The pill shows a passing view (one with `hide_after_ms`): a limit note waits behind it.
    passing: bool,
    /// Views waiting for the passing one to end (limit notes after "Copied" or "Not pasted").
    queue: VecDeque<View>,
}

fn view(kind: Kind, tone: Tone, label: &str, hide_after_ms: Option<u64>) -> View {
    View {
        kind,
        tone,
        label: short(label),
        hide_after_ms,
        action: None,
    }
}

fn writing() -> View {
    view(Kind::Writing, Tone::Accent, "Writing…", None)
}

/// The first sentence of a console line, without its "! " mark, cut to `LABEL_MAX`.
fn short(line: &str) -> String {
    let line = line.trim().trim_start_matches("! ").trim();
    let first = match line.find(". ") {
        Some(i) => &line[..=i],
        None => line,
    };
    if first.chars().count() > LABEL_MAX {
        first.chars().take(LABEL_MAX - 1).collect::<String>() + "…"
    } else {
        first.to_string()
    }
}

impl Hud {
    pub fn apply(&mut self, e: &Event) -> Change {
        match e {
            Event::Listening { .. } => {
                self.recording = true;
                self.queue.clear();
                self.show(view(Kind::Listening, Tone::Accent, "Listening", None))
            }
            Event::Processing => {
                self.recording = false;
                self.jobs += 1;
                self.show(writing())
            }
            Event::Cancelled => {
                self.recording = false;
                match self.next() {
                    Some(v) => self.show(v),
                    None => self.hide(),
                }
            }
            Event::Text { .. }
            | Event::NoSpeech { .. }
            | Event::HintOnly
            | Event::Failed { .. } => {
                self.jobs = self.jobs.saturating_sub(1);
                if let Event::Failed { lines } = e {
                    self.last_failure = lines.first().cloned();
                }
                // A job's end while the next recording runs (a slow answer): the recording keeps the pill.
                if self.recording {
                    return Change::None;
                }
                self.show(Self::result(e))
            }
            // Limit notes ("! <model> has used up today's free requests. ...") come right after a text: they
            // wait for its "Done" / "Copied" to end.
            Event::Notice(n) if n.starts_with("! ") => {
                let v = view(Kind::Problem, Tone::Warn, n, Some(LIMIT_MS));
                if self.recording {
                    Change::None
                } else if self.passing {
                    self.queue.push_back(v);
                    Change::None
                } else {
                    self.show(v)
                }
            }
            // Start-up lines, timing notes and setting problems belong on the tray, not over the user's app.
            // The failed recording is kept: the failure's pill gets a "Try again" button, for longer.
            Event::CanRetry if !self.recording => {
                let label = self
                    .last_failure
                    .clone()
                    .unwrap_or_else(|| "Nothing typed".into());
                let mut v = view(Kind::Problem, Tone::Danger, &label, Some(RETRY_MS));
                v.action = Some(TRY_AGAIN);
                self.show(v)
            }
            Event::CanRetry => Change::None,
            Event::Ready { .. } | Event::Notice(_) | Event::Warning(_) | Event::CapReached => {
                Change::None
            }
        }
    }

    /// What follows a passing view when its time is up: a waiting note, "Writing…" while another clip is
    /// still on its way, or nothing (the pill leaves).
    pub fn next(&mut self) -> Option<View> {
        let v = self
            .queue
            .pop_front()
            .or_else(|| (!self.recording && self.jobs > 0).then(writing));
        self.passing = v.as_ref().is_some_and(|v| v.hide_after_ms.is_some());
        v
    }

    /// Forgets everything (dictation stopped: no recording or answer is coming any more).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn show(&mut self, v: View) -> Change {
        self.passing = v.hide_after_ms.is_some();
        Change::Show(v)
    }

    fn hide(&mut self) -> Change {
        self.passing = false;
        Change::Hide
    }

    /// The view for a finished job.
    fn result(e: &Event) -> View {
        match e {
            Event::Text { paste, .. } => match paste {
                Paste::Pasted(_) | Paste::Off => {
                    view(Kind::Inserted, Tone::Ok, "Done", Some(INSERTED_MS))
                }
                Paste::Copied(_) => view(
                    Kind::Copied,
                    Tone::Warn,
                    "Copied: press Ctrl+V",
                    Some(COPIED_MS),
                ),
                Paste::Failed(why) => view(
                    Kind::Problem,
                    Tone::Danger,
                    &format!("Not pasted: {why}"),
                    Some(NOT_PASTED_MS),
                ),
            },
            Event::NoSpeech { .. } => view(
                Kind::Problem,
                Tone::Warn,
                "No speech heard",
                Some(NO_SPEECH_MS),
            ),
            Event::HintOnly => view(
                Kind::Problem,
                Tone::Warn,
                "Nothing to type",
                Some(NO_SPEECH_MS),
            ),
            Event::Failed { lines } => {
                let first = lines.first().map(String::as_str).unwrap_or("Nothing typed");
                view(Kind::Problem, Tone::Danger, first, Some(FAILED_MS))
            }
            _ => view(
                Kind::Problem,
                Tone::Danger,
                "Nothing typed",
                Some(FAILED_MS),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn shown(c: Change) -> View {
        match c {
            Change::Show(v) => v,
            other => panic!("expected a view, got {other:?}"),
        }
    }

    #[test]
    fn a_dictation_shows_listening_writing_inserted() {
        let mut h = Hud::default();
        let v = shown(h.apply(&listening()));
        assert_eq!(
            (v.kind, v.tone, v.hide_after_ms),
            (Kind::Listening, Tone::Accent, None)
        );
        let v = shown(h.apply(&Event::Processing));
        assert_eq!((v.kind, v.label.as_str()), (Kind::Writing, "Writing…"));
        let v = shown(h.apply(&text(Paste::Pasted(300))));
        assert_eq!((v.kind, v.tone), (Kind::Inserted, Tone::Ok));
        assert_eq!(v.hide_after_ms, Some(INSERTED_MS));
        assert!(!v.label.contains("secret"));
    }

    #[test]
    fn copied_and_failed_paste() {
        let mut h = Hud::default();
        let v = shown(h.apply(&text(Paste::Copied("administrator window".into()))));
        assert_eq!(
            (v.kind, v.label.as_str()),
            (Kind::Copied, "Copied: press Ctrl+V")
        );
        assert_eq!(v.hide_after_ms, Some(COPIED_MS));
        let v = shown(h.apply(&text(Paste::Failed("no window in front".into()))));
        assert_eq!((v.kind, v.tone), (Kind::Problem, Tone::Danger));
        assert_eq!(v.label, "Not pasted: no window in front");
    }

    #[test]
    fn failures_show_their_first_line_not_the_text() {
        let mut h = Hud::default();
        let v = shown(h.apply(&Event::Failed {
            lines: vec![
                "! Nothing typed: no model answered.".into(),
                "    flash: timeout".into(),
            ],
        }));
        assert_eq!(v.label, "Nothing typed: no model answered.");
        assert_eq!((v.tone, v.hide_after_ms), (Tone::Danger, Some(FAILED_MS)));
        let v = shown(h.apply(&Event::NoSpeech { seconds: 2.0 }));
        assert_eq!((v.tone, v.label.as_str()), (Tone::Warn, "No speech heard"));
        let long = shown(h.apply(&Event::Failed {
            lines: vec![format!("! {}", "x".repeat(200))],
        }));
        assert_eq!(long.label.chars().count(), LABEL_MAX);
        assert!(long.label.ends_with('…'));
    }

    #[test]
    fn cancel_hides() {
        let mut h = Hud::default();
        h.apply(&listening());
        assert_eq!(h.apply(&Event::Cancelled), Change::Hide);
    }

    #[test]
    fn slow_answer_keeps_listening() {
        // Clip 1 is still being written when clip 2 starts: its answer must not cover the recording.
        let mut h = Hud::default();
        h.apply(&listening());
        h.apply(&Event::Processing);
        h.apply(&listening());
        assert_eq!(h.apply(&text(Paste::Pasted(1))), Change::None);
        assert_eq!(h.apply(&Event::NoSpeech { seconds: 1.0 }), Change::None);
        assert_eq!(
            h.apply(&Event::Failed {
                lines: vec!["Nothing typed".into()]
            }),
            Change::None
        );
        assert!(matches!(h.apply(&Event::Processing), Change::Show(_)));
    }

    #[test]
    fn limit_notice_warns() {
        let mut h = Hud::default();
        let v = shown(
            h.apply(&Event::Notice(
                "! gemini-3.5-flash has used up today's free requests. They come back at midnight."
                    .into(),
            )),
        );
        assert_eq!(
            v.label,
            "gemini-3.5-flash has used up today's free requests."
        );
        assert_eq!((v.kind, v.tone), (Kind::Problem, Tone::Warn));
    }

    #[test]
    fn startup_notices_do_not_show() {
        let mut h = Hud::default();
        for e in [
            Event::Ready {
                engines: "chain".into(),
                key: "Right Ctrl".into(),
            },
            Event::Notice("Used flash: transcribe was slow".into()),
            Event::Warning("config.toml: unknown setting".into()),
            Event::CapReached,
        ] {
            assert_eq!(h.apply(&e), Change::None, "{e:?}");
        }
    }

    #[test]
    fn views_serialise_for_the_web_view() {
        let v = View {
            kind: Kind::Inserted,
            tone: Tone::Ok,
            label: "Done".into(),
            hide_after_ms: Some(600),
            action: None,
        };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"inserted","tone":"ok","label":"Done","hide_after_ms":600,"action":null}"#
        );
    }

    #[test]
    fn a_kept_failure_offers_try_again() {
        let mut h = Hud::default();
        h.apply(&listening());
        h.apply(&Event::Processing);
        h.apply(&Event::Failed {
            lines: vec!["! Nothing typed: no model answered.".into()],
        });
        let v = shown(h.apply(&Event::CanRetry));
        assert_eq!(v.action, Some(TRY_AGAIN));
        assert_eq!(v.label, "Nothing typed: no model answered.");
        assert_eq!(v.hide_after_ms, Some(RETRY_MS));
        // Never over a recording.
        h.apply(&listening());
        assert_eq!(h.apply(&Event::CanRetry), Change::None);
    }

    #[test]
    fn a_limit_note_waits_for_copied() {
        let mut h = Hud::default();
        h.apply(&listening());
        h.apply(&Event::Processing);
        let v = shown(h.apply(&text(Paste::Copied("admin".into()))));
        assert_eq!(v.kind, Kind::Copied);
        let note =
            Event::Notice("! gemini-3.5-flash has used up today's free requests. More.".into());
        assert_eq!(h.apply(&note), Change::None, "Copied stays on screen");
        let next = h.next().expect("the note after Copied");
        assert_eq!(
            next.label,
            "gemini-3.5-flash has used up today's free requests."
        );
        assert_eq!(h.next(), None, "then the pill leaves");
    }

    #[test]
    fn a_late_answer_keeps_writing_for_the_next_clip() {
        // Clip 1 is slow; clip 2 is recorded and sent; clip 1's answer must not end clip 2's "Writing…".
        let mut h = Hud::default();
        h.apply(&listening());
        h.apply(&Event::Processing);
        h.apply(&listening());
        h.apply(&Event::Processing);
        assert_eq!(shown(h.apply(&text(Paste::Pasted(1)))).kind, Kind::Inserted);
        assert_eq!(h.next().map(|v| v.kind), Some(Kind::Writing));
        assert_eq!(shown(h.apply(&text(Paste::Pasted(1)))).kind, Kind::Inserted);
        assert_eq!(h.next(), None);
    }

    #[test]
    fn cancel_while_an_answer_is_pending_keeps_writing() {
        let mut h = Hud::default();
        h.apply(&listening());
        h.apply(&Event::Processing);
        h.apply(&listening());
        assert_eq!(shown(h.apply(&Event::Cancelled)).kind, Kind::Writing);
    }

    #[test]
    fn reset_lets_a_stop_show_during_a_recording() {
        // Dictation stopped while recording (the key hook died): its failure must show, not a stuck timer.
        let mut h = Hud::default();
        h.apply(&listening());
        h.reset();
        let v = shown(h.apply(&Event::Failed {
            lines: vec!["Dictation stopped: the key hook stopped".into()],
        }));
        assert_eq!(v.tone, Tone::Danger);
    }
}
