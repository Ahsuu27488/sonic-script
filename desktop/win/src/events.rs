//! What dictation tells the user, as events (M2 plan task 1.4). The dictation loop sends them to a [`Sink`]:
//! the console sink prints M1's lines; the app (M2 phase 2+) drives the tray and the HUD Voice Ring with them.
//! The live mic level is not an event: the HUD reads `audio::level()` at its own frame rate.

/// How the text reached the app in front.
#[derive(Debug, Clone, PartialEq)]
pub enum Paste {
    /// `--record`: printed only.
    Off,
    /// Pasted at the cursor in this many ms.
    Pasted(u128),
    /// Only on the clipboard (e.g. an administrator window): the user presses Ctrl+V.
    Copied(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Dictation is listening for the key: which engines run and which key starts it.
    Ready {
        engines: String,
        key: String,
    },
    /// Something worth a line (fallback used, not formatted, history folder, ...).
    Notice(String),
    /// Something went wrong but dictation goes on (a setting, the history, the mic).
    Warning(String),
    /// The mic is recording.
    Listening {
        device: String,
        rate: u32,
        mic_ms: u128,
        prepared: bool,
    },
    Cancelled,
    /// The 5-minute cap stopped the recording.
    CapReached,
    /// The recording is on its way to text.
    Processing,
    /// The text, how it was pasted, and the timing line.
    Text {
        text: String,
        paste: Paste,
        details: String,
    },
    /// The voice gate heard no speech: nothing was sent.
    NoSpeech {
        seconds: f64,
    },
    /// The engine only gave back its own hint: nothing to type.
    HintOnly,
    /// Nothing typed; what happened and what to do, one line each.
    Failed {
        lines: Vec<String>,
    },
    /// The failed recording is kept and can be sent again (`dictate::retry`): no answer came, but a key or the
    /// day's free requests were not the reason (M2 phase 6).
    CanRetry,
}

/// Where events go. Called from the dictation thread and the transcription threads.
pub trait Sink: Send + Sync {
    fn send(&self, e: Event);
}

/// Prints the events as M1 did.
pub struct Console;

impl Sink for Console {
    fn send(&self, e: Event) {
        for (err, line) in console_lines(&e) {
            if err {
                eprintln!("{line}");
            } else {
                println!("{line}");
            }
        }
    }
}

/// The console's lines for an event; `true` = stderr.
pub fn console_lines(e: &Event) -> Vec<(bool, String)> {
    let out = |s: String| vec![(false, s)];
    match e {
        Event::Ready { engines, key: k } => out(format!(
            "SonicScript is listening ({engines}).\n  \
             Tap {k}, speak, tap it again or press Enter: the text is typed where your cursor is\n  \
             (and shown here).\n  \
             Or hold {k} while you speak. Esc cancels. Close this window or press Ctrl+C to quit.\n  \
             Another key: sonicscript --set-key"
        )),
        Event::Notice(s) => out(format!("  {s}")),
        Event::Warning(s) => vec![(true, format!("  ! {s}"))],
        Event::Listening {
            device,
            rate,
            mic_ms,
            prepared,
        } => {
            let how = if *prepared { "ready" } else { "opened" };
            out(format!(
                "● Recording ({device}, {rate} Hz, mic {how} in {mic_ms} ms)"
            ))
        }
        Event::Cancelled => out("  Cancelled.".into()),
        Event::CapReached => out("  5-minute limit reached: stopping.".into()),
        Event::Processing => Vec::new(),
        Event::Text {
            text,
            paste,
            details,
        } => {
            let mut lines = vec![(
                false,
                format!("→ {}", text.trim_end().replace('\n', "\n  ")),
            )];
            match paste {
                Paste::Copied(why) => lines.push((
                    false,
                    format!(
                        "  ! Copied, not pasted ({why}): press Ctrl+V where you want the text."
                    ),
                )),
                Paste::Failed(e) => lines.push((true, format!("  ! Not pasted: {e}"))),
                Paste::Off | Paste::Pasted(_) => {}
            }
            lines.push((false, format!("  {details}")));
            lines
        }
        Event::NoSpeech { seconds } => out(format!(
            "  No speech heard ({seconds:.1} s recorded): nothing sent."
        )),
        Event::HintOnly => out("  Nothing to type (the engine only heard its own hint).".into()),
        Event::Failed { lines } => lines.iter().map(|l| (true, format!("  {l}"))).collect(),
        Event::CanRetry => Vec::new(),
    }
}

/// The level of a chunk of samples for the Voice Ring: 0.0 at -60 dBFS RMS or below, 1.0 at 0 dBFS.
pub fn level_of(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
    let db = 20.0 * rms.max(1e-9).log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_follows_loudness() {
        assert_eq!(level_of(&[]), 0.0);
        assert_eq!(level_of(&[0.0; 320]), 0.0);
        let sine: Vec<f32> = (0..320).map(|i| (i as f32 * 0.2).sin()).collect();
        let loud = level_of(&sine);
        assert!((0.9..=1.0).contains(&loud), "{loud}");
        let quiet: Vec<f32> = sine.iter().map(|s| s * 0.01).collect();
        let q = level_of(&quiet);
        assert!((0.25..0.4).contains(&q), "{q}");
    }

    #[test]
    fn console_keeps_m1_lines() {
        let e = Event::Text {
            text: "- one\n- two\n".into(),
            paste: Paste::Copied("administrator window".into()),
            details: "1.0 s recorded".into(),
        };
        let lines: Vec<String> = console_lines(&e).into_iter().map(|(_, l)| l).collect();
        assert_eq!(
            lines,
            [
                "→ - one\n  - two",
                "  ! Copied, not pasted (administrator window): press Ctrl+V where you want the text.",
                "  1.0 s recorded",
            ]
        );
        let failed = console_lines(&Event::Failed {
            lines: vec!["! Nothing typed: x".into()],
        });
        assert_eq!(failed, [(true, "  ! Nothing typed: x".to_string())]);
    }
}
