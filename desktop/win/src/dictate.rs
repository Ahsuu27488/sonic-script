//! Dictation mode, the default when `sonicscript.exe` runs without arguments (M1 plan step 5). The dictation
//! key opens the mic and warms both engines' connections at once; stopping runs the pipeline on a worker thread,
//! prints the text and pastes it at the cursor of the window in front (step 7, `inject`), then keeps the
//! recording and the text in the local history (`history`, D-032).

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use sonicscript_core::api::{ApiError, Skip};
use sonicscript_core::keys::Action;
use sonicscript_core::normalize::Normalizer;
use sonicscript_core::pipeline::{self, Outcome, PipelineError, Transcribe};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_RCONTROL;

use crate::audio::{Ready, Recorder, Recording};
use crate::cue::{self, Cue};
use crate::history::{Clip, History};
use crate::inject::{self, Pasted};
use crate::key::{Choice, Engines};
use crate::{hotkey, key};

/// Safety cap on one recording (M1 plan step 5): a forgotten toggle does not keep the mic open.
const MAX_RECORDING: Duration = Duration::from_secs(5 * 60);

/// One finished recording on its way to text.
struct Job {
    rec: Recording,
    stopped: Instant,
    mic_open_ms: u128,
    /// False for `--record`: a mic check only prints (its window is in front, and text pasted into a console
    /// could run as commands).
    paste: bool,
}

pub fn run(choice: Choice, keep_history: bool) -> Result<(), String> {
    let normalizer = Arc::new(key::normalizer(&choice));
    let words = choice.words.as_ref().map(|w| crate::mywords::summary(w));
    let client = Arc::new(key::engines(choice)?);
    let mut id = key::describe(&client);
    if !client.first().formats() {
        id += "; no formatting";
    }
    let history = open_history(keep_history).map(Arc::new);
    if let Some(words) = words {
        println!("{words}");
    }
    // Sets up the mic stream now, so the key-down only starts it (D-036).
    let ready = Ready::new();
    let (tx, rx) = mpsc::channel();
    // The key is a setting from step 6 on (D-018); Right Ctrl is the default (D-015).
    hotkey::spawn(VK_RCONTROL, tx)?;
    println!(
        "SonicScript is listening ({id}).\n  \
         Tap Right Ctrl, speak, tap it again or press Enter: the text is typed where your cursor is\n  \
         (and shown here).\n  \
         Or hold Right Ctrl while you speak. Esc cancels. Close this window or press Ctrl+C to quit."
    );

    let mut recording: Option<(Recorder, Instant, u128)> = None;
    loop {
        let action = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(action) => action,
            Err(RecvTimeoutError::Timeout) => match &recording {
                Some((_, since, _)) if since.elapsed() >= MAX_RECORDING => {
                    println!("  5-minute limit reached: stopping.");
                    hotkey::force_stop();
                    Action::Stop
                }
                _ => continue,
            },
            Err(RecvTimeoutError::Disconnected) => return Err("the key hook stopped".into()),
        };
        match action {
            Action::Start => {
                // The chime starts first; the mic starts ~15 ms later, so with speakers (not headphones) its
                // start may be in the recording, quietly.
                cue::play(Cue::Start);
                let client = client.clone();
                std::thread::spawn(move || client.warm());
                let t = Instant::now();
                match ready.start() {
                    Ok(r) => {
                        let mic_open_ms = t.elapsed().as_millis();
                        let how = if r.prepared { "ready" } else { "opened" };
                        println!(
                            "● Recording ({}, {} Hz, mic {how} in {mic_open_ms} ms)",
                            r.device, r.rate,
                        );
                        recording = Some((r, Instant::now(), mic_open_ms));
                    }
                    Err(e) => {
                        ready.refill();
                        hotkey::force_stop();
                        cue::play(Cue::Cancel);
                        eprintln!("  Could not open the microphone: {e}");
                    }
                }
            }
            Action::Cancel => {
                if let Some((r, ..)) = recording.take() {
                    drop(r.stop());
                    ready.refill();
                    cue::play(Cue::Cancel);
                    println!("  Cancelled.");
                }
            }
            Action::Stop => {
                let Some((r, _, mic_open_ms)) = recording.take() else {
                    continue;
                };
                let job = Job {
                    stopped: Instant::now(),
                    rec: r.stop(),
                    mic_open_ms,
                    paste: true,
                };
                ready.refill();
                // After the mic is closed, so the chime is not recorded.
                cue::play(Cue::Stop);
                let (client, normalizer, history) =
                    (client.clone(), normalizer.clone(), history.clone());
                std::thread::spawn(move || {
                    transcribe(job, &client, &normalizer, history.as_deref());
                });
            }
        }
    }
}

/// `--record <seconds>`: the same mic → pipeline path without the key, to check the microphone (and to test
/// it where no key can be pressed, e.g. a locked PC).
pub fn record_for(seconds: f64, choice: Choice, keep_history: bool) -> Result<(), String> {
    let normalizer = key::normalizer(&choice);
    let client = key::engines(choice)?;
    let history = open_history(keep_history);
    let warm = std::thread::spawn(move || {
        client.warm();
        client
    });
    let t = Instant::now();
    let r = Recorder::start()?;
    let mic_open_ms = t.elapsed().as_millis();
    println!("● Recording {seconds} s ({}, {} Hz)", r.device, r.rate);
    std::thread::sleep(Duration::from_secs_f64(seconds));
    let job = Job {
        stopped: Instant::now(),
        rec: r.stop(),
        mic_open_ms,
        paste: false,
    };
    let client = warm.join().map_err(|_| "warm-up thread panicked")?;
    transcribe(job, &client, &normalizer, history.as_ref());
    Ok(())
}

/// The history folder, or `None` with a notice when it is turned off or cannot be made (dictation goes on).
fn open_history(keep: bool) -> Option<History> {
    if !keep {
        println!("History is off (--no-history): recordings and text are not kept.");
        return None;
    }
    match History::open() {
        Ok(h) => {
            println!(
                "Each recording and its text are kept in {} (--no-history to turn off).",
                h.dir().display()
            );
            Some(h)
        }
        Err(e) => {
            eprintln!("  ! History is off: could not create its folder: {e}");
            None
        }
    }
}

fn transcribe(job: Job, client: &Engines, normalizer: &Normalizer, history: Option<&History>) {
    let Job {
        rec,
        stopped,
        mic_open_ms,
        paste,
    } = job;
    let mut stream = match rec.stream {
        Ok(s) => s,
        Err(e) => {
            eprintln!("  {e}");
            return;
        }
    };
    let out = pipeline::run_streamed(&rec.samples, &mut stream, client, normalizer);
    let key_up_to_text = stopped.elapsed().as_millis();
    let mut app = None;
    let mut paste_result = if paste { "not pasted" } else { "off" }.to_string();
    let mut paste_ms = None;
    let seconds = f64::from(out.audio_ms) / 1000.0;
    if let Some(e) = &out.error {
        for line in failure_notes(e) {
            eprintln!("  {line}");
        }
    } else if !out.sent {
        println!("  No speech heard ({seconds:.1} s recorded): nothing sent.");
    } else if out.text.is_empty() {
        println!("  Nothing to type (the engine only heard its own hint).");
    } else {
        println!("→ {}", out.text.trim_end().replace('\n', "\n  "));
        let pasted = if paste {
            app = inject::app_in_front();
            let t = Instant::now();
            let result = inject::paste(&out.text);
            paste_ms = Some(t.elapsed().as_millis());
            match result {
                Ok(Pasted::Pasted) => {
                    paste_result = "pasted".into();
                    format!(" · pasted in {} ms", t.elapsed().as_millis())
                }
                Ok(Pasted::Copied(why)) => {
                    paste_result = format!("copied: {why}");
                    println!(
                        "  ! Copied, not pasted ({why}): press Ctrl+V where you want the text."
                    );
                    String::new()
                }
                Err(e) => {
                    paste_result = format!("failed: {e}");
                    eprintln!("  ! Not pasted: {e}");
                    String::new()
                }
            }
        } else {
            String::new()
        };
        let note = if out.needs_llm {
            " · mostly Urdu script, rough romanization"
        } else {
            ""
        };
        let refine = if out.stages.refine > 0.0 {
            format!(" + refine {:.0} ms", out.stages.refine)
        } else {
            String::new()
        };
        println!(
            "  {seconds:.1} s recorded, {:.1} s sent · key-up → text {key_up_to_text} ms (speech {:.0} ms{refine}){pasted}{note}",
            f64::from(out.sent_ms) / 1000.0,
            out.stages.request
        );
        if let Some(note) = fallback_note(&out, client.first().id()) {
            println!("  {note}");
        }
        if let Some(why) = &out.unformatted {
            println!("  Not formatted (exact words): the formatting step failed: {why}");
        }
    }
    for note in limit_notes(&out) {
        println!("  {note}");
    }
    // After the paste, so keeping the history adds nothing to the wait.
    if let Some(history) = history {
        let clip = Clip {
            audio: stream
                .finish(0..stream.len())
                .map(|e| e.bytes)
                .map_err(|e| e.to_string()),
            mic_open_ms,
            key_up_to_text_ms: key_up_to_text,
            mode: if paste { "dictation" } else { "record" },
            main_engine: client.first().id(),
            formatting: client.first().formats(),
            app,
            paste: paste_result,
            paste_ms,
        };
        if let Err(e) = history.save(&out, clip) {
            eprintln!("  ! Not kept in the history: {e}");
        }
    }
}

/// What to tell the user when no text came back: nothing was typed, why each model failed, and what to do
/// (D-035: no weak fallback, a clear message instead). The recording stays in the history.
fn failure_notes(e: &PipelineError) -> Vec<String> {
    let PipelineError::Api(ApiError::AllFailed(all)) = e else {
        return vec![format!("! Nothing typed: {e}")];
    };
    let mut lines = vec!["! Nothing typed: no model answered.".to_string()];
    lines.extend(all.iter().map(|(id, e)| format!("    {id}: {e}")));
    let daily = |e: &ApiError| matches!(e, ApiError::RateLimited { daily: true, .. });
    let slow = |e: &ApiError| {
        matches!(
            e,
            ApiError::Timeout
                | ApiError::Http(500..=599, _)
                | ApiError::RateLimited { daily: false, .. }
        )
    };
    if all.iter().all(|(_, e)| daily(e)) {
        lines.push(
            "  Today's free requests are used up on every model. They come back at midnight Pacific time \
             (about 1 PM in Pakistan). Until then: sonicscript --engine groq-whisper-hint (fast, less accurate)."
                .into(),
        );
    } else if all.iter().any(|(_, e)| slow(e)) {
        lines.push("  Google is slow or busy right now: try again in a minute.".into());
    }
    lines.push("  Your recording is kept in the history.".into());
    lines
}

/// Which engine's text this is when it is not the first one's, and why (D-029).
fn fallback_note(out: &Outcome, first: &str) -> Option<String> {
    if out.engine == first || out.passed.is_empty() {
        return None;
    }
    let why: Vec<String> = out.passed.iter().map(ToString::to_string).collect();
    Some(format!("Used {}: {}.", out.engine, why.join("; ")))
}

/// What to tell the user when an engine hits its free limit (once, when the 429 comes; afterwards the engine
/// is skipped without asking until it has rested).
fn limit_notes(out: &Outcome) -> Vec<String> {
    out.passed
        .iter()
        .filter_map(|p| match &p.why {
            Skip::Failed(ApiError::RateLimited { daily: true, .. }) => Some(format!(
                "! {} has used up today's free requests. They come back at midnight Pacific time \
                 (about 1 PM in Pakistan); until then the next model in the chain answers first. To make \
                 another model your main one: sonicscript --engine <id> (list: sonicscript --models).",
                p.engine
            )),
            Skip::Failed(ApiError::RateLimited { .. }) => Some(format!(
                "! {} hit its per-minute limit: the next model answers for about a minute.",
                p.engine
            )),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(errors: Vec<ApiError>) -> PipelineError {
        let ids = ["transcribe", "flash", "flash-lite"];
        PipelineError::Api(ApiError::AllFailed(
            ids.iter().map(|s| s.to_string()).zip(errors).collect(),
        ))
    }

    fn daily() -> ApiError {
        ApiError::RateLimited {
            retry_after_s: None,
            daily: true,
        }
    }

    #[test]
    fn when_no_model_answers_the_user_is_told_why_and_what_to_do() {
        let used_up = failure_notes(&all(vec![daily(), daily(), daily()])).join("\n");
        assert!(
            used_up.contains("Nothing typed") && used_up.contains("about 1 PM"),
            "{used_up}"
        );
        assert!(used_up.contains("flash-lite: today's free requests are used up"));

        let slow = failure_notes(&all(vec![daily(), daily(), ApiError::Timeout])).join("\n");
        assert!(
            slow.contains("Google is slow") && !slow.contains("1 PM"),
            "{slow}"
        );
        assert!(slow.ends_with("Your recording is kept in the history."));

        let one = failure_notes(&PipelineError::Api(ApiError::Timeout)).join("\n");
        assert_eq!(one, "! Nothing typed: no answer within 10 s");
    }
}
