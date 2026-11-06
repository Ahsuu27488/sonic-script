//! What the HUD Voice Ring shows (M2 task 3.1, DESIGN.md "HUD pill"), worked out from dictation events.
//! Pure, so it is tested without a window; `hudwin.rs` puts the result on the real window.
//! A view never holds dictated text: the pill floats over every app and anyone can read it.

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
/// The longest label the pill shows; longer ones are cut with "…".
pub const LABEL_MAX: usize = 72;

/// The HUD's memory between events.
#[derive(Debug, Default)]
pub struct Hud {
    recording: bool,
}

fn view(kind: Kind, tone: Tone, label: &str, hide_after_ms: Option<u64>) -> Change {
    Change::Show(View {
        kind,
        tone,
        label: short(label),
        hide_after_ms,
    })
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
                view(Kind::Listening, Tone::Accent, "Listening", None)
            }
            Event::Processing => {
                self.recording = false;
                view(Kind::Writing, Tone::Accent, "Writing…", None)
            }
            Event::Cancelled => {
                self.recording = false;
                Change::Hide
            }
            // A job's end while the next recording runs (a slow answer): the recording keeps the pill.
            Event::Text { .. }
            | Event::NoSpeech { .. }
            | Event::HintOnly
            | Event::Failed { .. }
            | Event::Notice(_)
                if self.recording =>
            {
                Change::None
            }
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
            // Limit notes ("! <model> has used up today's free requests. ...") come right after a text.
            Event::Notice(n) if n.starts_with("! ") => {
                view(Kind::Problem, Tone::Warn, n, Some(LIMIT_MS))
            }
            // Start-up lines, timing notes and setting problems belong on the tray, not over the user's app.
            Event::Ready { .. } | Event::Notice(_) | Event::Warning(_) | Event::CapReached => {
                Change::None
            }
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
        };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"inserted","tone":"ok","label":"Done","hide_after_ms":600}"#
        );
    }
}
