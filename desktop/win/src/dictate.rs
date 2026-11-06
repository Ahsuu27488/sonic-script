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

use crate::audio::{Ready, Recorder, Recording};
use crate::cue::{self, Cue};
use crate::events::{Console, Event, Paste, Sink};
use crate::history::{Clip, History};
use crate::inject::{self, Pasted};
use crate::key::{Choice, Engines};
use crate::{hotkey, key, setkey};

/// Set by the settings window (M2 phase 4): `run` returns at the next idle moment so its caller starts it again
/// with the new settings.
static RELOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Asks the running dictation to stop when no recording runs, so it can be started with new settings.
pub fn request_reload() {
    RELOAD.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// The last recording nothing answered, kept for "Try again" (M2 phase 6).
struct Pending {
    samples: Vec<f32>,
    stream: sonicscript_core::encode::Stream,
    mic_open_ms: u128,
    seq: u64,
}

static PENDING: std::sync::Mutex<Option<Pending>> = std::sync::Mutex::new(None);
/// 0: no retry asked; [`RETRY_PASTE`] or [`RETRY_COPY`]: send the kept recording at the next idle moment.
static RETRY: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
const RETRY_PASTE: u8 = 1;
const RETRY_COPY: u8 = 2;

/// "Try again" (the tray, the HUD): the kept recording is sent again at the next idle moment. `copy_only`:
/// the text goes on the clipboard (the tray menu has the focus, a paste would miss the user's app). False
/// when no recording is kept. A recording started before it runs cancels it.
pub fn retry(copy_only: bool) -> bool {
    let kept = PENDING.lock().is_ok_and(|p| p.is_some());
    if kept {
        let how = if copy_only { RETRY_COPY } else { RETRY_PASTE };
        RETRY.store(how, std::sync::atomic::Ordering::SeqCst);
    }
    kept
}

/// What a finished job means for the kept recording: a newer failure replaces it, a newer text that came
/// through forgets it, anything older changes nothing. `Some(true)` = keep this job's, `Some(false)` = forget.
fn keep_decision(
    kept: Option<u64>,
    job: u64,
    retryable_failure: bool,
    typed: bool,
) -> Option<bool> {
    let newer = kept.is_none_or(|k| job > k);
    match (newer, retryable_failure, typed) {
        (true, true, _) => Some(true),
        (true, false, true) if kept.is_some() => Some(false),
        _ => None,
    }
}

/// Whether sending the same recording again could work: no answer came, but not because of a key or because
/// every model's free requests for the day are used up (those need the user, or tomorrow).
fn retryable(e: &PipelineError) -> bool {
    let PipelineError::Api(e) = e else {
        return false;
    };
    let daily = |e: &ApiError| matches!(e, ApiError::RateLimited { daily: true, .. });
    match e {
        ApiError::AllFailed(all) => !e.is_key_error() && !all.iter().all(|(_, e)| daily(e)),
        other => !other.is_key_error() && !daily(other),
    }
}

/// True once when a reload was asked for and nothing is recording (a recording finishes first).
fn reload_now(recording: bool) -> bool {
    !recording && take_reload()
}

/// True once per [`request_reload`] (for a caller whose dictation stopped and waits for new settings).
pub fn take_reload() -> bool {
    RELOAD.swap(false, std::sync::atomic::Ordering::SeqCst)
}

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
    /// Put the text on the clipboard instead of pasting it (Try again from the tray menu: the menu took the
    /// focus from the user's app).
    copy_only: bool,
    /// The order the recordings were made in (a retry keeps its first one): only a newer result decides
    /// what Try again keeps.
    seq: u64,
}

static NEXT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_seq() -> u64 {
    NEXT_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

/// Dictates until the key hook stops (an error) or a reload is asked for (`Ok`, see [`request_reload`]).
pub fn run(choice: Choice, keep_history: bool, sink: Arc<dyn Sink>) -> Result<(), String> {
    // One dictation per user: a second copy would install a second hook and paste twice (M2 task 2.2).
    let _instance = crate::instance::claim()?;
    let normalizer = Arc::new(key::normalizer(&choice));
    let words = choice.words.as_ref().map(|w| crate::mywords::summary(w));
    let client = Arc::new(key::engines(choice)?);
    let mut id = key::describe(&client);
    if !client.first().formats() {
        id += "; no formatting";
    }
    let history = open_history(keep_history, &*sink).map(Arc::new);
    if let Some(words) = words {
        sink.send(Event::Notice(words));
    }
    // Sets up the mic stream now, so the key-down only starts it (D-036).
    let ready = Ready::new();
    let (tx, rx) = mpsc::channel();
    // The user's key (D-018, `sonicscript --set-key`); Right Ctrl by default (D-015).
    let (dictation_key, notice) = setkey::load();
    if let Some(notice) = notice {
        sink.send(Event::Warning(notice));
    }
    hotkey::spawn(dictation_key, tx)?;
    sink.send(Event::Ready {
        engines: id,
        key: dictation_key.label.to_string(),
    });

    let mut recording: Option<(Recorder, Instant, u128)> = None;
    loop {
        let action = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(action) => action,
            Err(RecvTimeoutError::Timeout) => match &recording {
                Some((_, since, _)) if since.elapsed() >= MAX_RECORDING => {
                    sink.send(Event::CapReached);
                    hotkey::force_stop();
                    Action::Stop
                }
                _ if reload_now(recording.is_some()) => return Ok(()),
                None if RETRY.load(std::sync::atomic::Ordering::SeqCst) != 0 => {
                    let how = RETRY.swap(0, std::sync::atomic::Ordering::SeqCst);
                    let Some(p) = PENDING.lock().ok().and_then(|mut p| p.take()) else {
                        continue;
                    };
                    let job = Job {
                        stopped: Instant::now(),
                        rec: Recording {
                            samples: p.samples,
                            stream: Ok(p.stream),
                        },
                        mic_open_ms: p.mic_open_ms,
                        paste: true,
                        copy_only: how == RETRY_COPY,
                        seq: p.seq,
                    };
                    sink.send(Event::Processing);
                    let (client, normalizer, history, sink) = (
                        client.clone(),
                        normalizer.clone(),
                        history.clone(),
                        sink.clone(),
                    );
                    std::thread::spawn(move || {
                        transcribe(job, &client, &normalizer, history.as_deref(), &*sink);
                    });
                    continue;
                }
                _ => continue,
            },
            Err(RecvTimeoutError::Disconnected) => return Err("the key hook stopped".into()),
        };
        match action {
            Action::Start => {
                // A Try again asked before this recording is dropped: it would paste in the middle of it.
                RETRY.store(0, std::sync::atomic::Ordering::SeqCst);
                // The chime starts first; the mic starts ~15 ms later, so with speakers (not headphones) its
                // start may be in the recording, quietly.
                cue::play(Cue::Start);
                let client = client.clone();
                std::thread::spawn(move || client.warm());
                let t = Instant::now();
                match ready.start() {
                    Ok(r) => {
                        let mic_open_ms = t.elapsed().as_millis();
                        sink.send(Event::Listening {
                            device: r.device.clone(),
                            rate: r.rate,
                            mic_ms: mic_open_ms,
                            prepared: r.prepared,
                        });
                        recording = Some((r, Instant::now(), mic_open_ms));
                    }
                    Err(e) => {
                        ready.refill();
                        hotkey::force_stop();
                        cue::play(Cue::Cancel);
                        sink.send(Event::Failed {
                            lines: vec![format!("Could not open the microphone: {e}")],
                        });
                    }
                }
            }
            Action::Cancel => {
                if let Some((r, ..)) = recording.take() {
                    drop(r.stop());
                    ready.refill();
                    cue::play(Cue::Cancel);
                    sink.send(Event::Cancelled);
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
                    copy_only: false,
                    seq: next_seq(),
                };
                ready.refill();
                // After the mic is closed, so the chime is not recorded.
                cue::play(Cue::Stop);
                sink.send(Event::Processing);
                let (client, normalizer, history, sink) = (
                    client.clone(),
                    normalizer.clone(),
                    history.clone(),
                    sink.clone(),
                );
                std::thread::spawn(move || {
                    transcribe(job, &client, &normalizer, history.as_deref(), &*sink);
                });
            }
        }
    }
}

/// `--record <seconds>`: the same mic → pipeline path without the key, to check the microphone (and to test
/// it where no key can be pressed, e.g. a locked PC).
pub fn record_for(seconds: f64, choice: Choice, keep_history: bool) -> Result<(), String> {
    let sink = Console;
    let normalizer = key::normalizer(&choice);
    let client = key::engines(choice)?;
    let history = open_history(keep_history, &sink);
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
        copy_only: false,
        seq: next_seq(),
    };
    let client = warm.join().map_err(|_| "warm-up thread panicked")?;
    transcribe(job, &client, &normalizer, history.as_ref(), &sink);
    Ok(())
}

/// The history folder, or `None` with a notice when it is turned off or cannot be made (dictation goes on).
fn open_history(keep: bool, sink: &dyn Sink) -> Option<History> {
    if !keep {
        sink.send(Event::Notice(
            "History is off: recordings and text are not kept.".into(),
        ));
        return None;
    }
    match History::open() {
        Ok(h) => {
            // Recordings older than the kept days go (D-044, Settings → History); 0 keeps them forever.
            let days = crate::settings::load().0.history_days;
            let h = h.with_keep_days(days);
            h.prune_now_and_then();
            let kept = match days {
                0 => String::new(),
                1 => " for 1 day".into(),
                n => format!(" for {n} days"),
            };
            sink.send(Event::Notice(format!(
                "Each recording and its text are kept{kept} in {} (--no-history to turn off).",
                h.dir().display()
            )));
            Some(h)
        }
        Err(e) => {
            sink.send(Event::Warning(format!(
                "History is off: could not create its folder: {e}"
            )));
            None
        }
    }
}

fn transcribe(
    job: Job,
    client: &Engines,
    normalizer: &Normalizer,
    history: Option<&History>,
    sink: &dyn Sink,
) {
    let Job {
        rec,
        stopped,
        mic_open_ms,
        paste,
        copy_only,
        seq,
    } = job;
    let mut stream = match rec.stream {
        Ok(s) => s,
        Err(e) => {
            sink.send(Event::Failed {
                lines: vec![e.to_string()],
            });
            return;
        }
    };
    let out = pipeline::run_streamed(&rec.samples, &mut stream, client, normalizer);
    let key_up_to_text = stopped.elapsed().as_millis();
    let typeable = out.error.is_none() && out.sent && !out.text.is_empty();
    let mut app = None;
    // Timed for every attempt (pasted, only copied, failed): the history is also the latency log (D-032).
    let mut paste_ms = None;
    let pasted = if paste && typeable && copy_only {
        match inject::copy(out.text.trim_end()) {
            Ok(()) => Paste::Copied("Try again from the tray menu".into()),
            Err(e) => Paste::Failed(e),
        }
    } else if paste && typeable {
        app = inject::app_in_front();
        let t = Instant::now();
        let result = inject::paste(&out.text);
        let ms = t.elapsed().as_millis();
        paste_ms = Some(ms);
        match result {
            Ok(Pasted::Pasted) => Paste::Pasted(ms),
            Ok(Pasted::Copied(why)) => Paste::Copied(why.to_string()),
            Err(e) => Paste::Failed(e.to_string()),
        }
    } else {
        Paste::Off
    };
    for e in outcome_events(&out, pasted.clone(), key_up_to_text, client.first().id()) {
        sink.send(e);
    }
    // After the paste, so keeping the history adds nothing to the wait.
    if let Some(history) = history {
        let paste_result = match &pasted {
            Paste::Off if paste => "not pasted".to_string(),
            Paste::Off => "off".to_string(),
            Paste::Pasted(_) => "pasted".to_string(),
            Paste::Copied(why) => format!("copied: {why}"),
            Paste::Failed(e) => format!("failed: {e}"),
        };
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
            sink.send(Event::Warning(format!("Not kept in the history: {e}")));
        }
        history.prune_now_and_then();
    }
    // "Try again" keeps the last recording nothing answered (dictation only; a text that came through
    // forgets it).
    if paste && let Ok(mut pending) = PENDING.lock() {
        let failed = out.error.as_ref().is_some_and(retryable);
        match keep_decision(pending.as_ref().map(|p| p.seq), seq, failed, typeable) {
            Some(true) => {
                *pending = Some(Pending {
                    samples: rec.samples,
                    stream,
                    mic_open_ms,
                    seq,
                });
                drop(pending);
                sink.send(Event::CanRetry);
            }
            Some(false) => *pending = None,
            None => {}
        }
    }
}

/// What the user is told about one finished recording, in order (M1's console lines, now events).
fn outcome_events(out: &Outcome, paste: Paste, key_up_ms: u128, first: &str) -> Vec<Event> {
    let mut events = Vec::new();
    let seconds = f64::from(out.audio_ms) / 1000.0;
    if let Some(e) = &out.error {
        events.push(Event::Failed {
            lines: failure_notes(e),
        });
    } else if !out.sent {
        events.push(Event::NoSpeech { seconds });
    } else if out.text.is_empty() {
        events.push(Event::HintOnly);
    } else {
        let pasted = match &paste {
            Paste::Pasted(ms) => format!(" · pasted in {ms} ms"),
            _ => String::new(),
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
        let details = format!(
            "{seconds:.1} s recorded, {:.1} s sent · key-up → text {key_up_ms} ms (speech {:.0} ms{refine}){pasted}{note}",
            f64::from(out.sent_ms) / 1000.0,
            out.stages.request
        );
        events.push(Event::Text {
            text: out.text.clone(),
            paste,
            details,
        });
        if let Some(note) = fallback_note(out, first) {
            events.push(Event::Notice(note));
        }
        if let Some(why) = &out.unformatted {
            events.push(Event::Notice(format!(
                "Not formatted (exact words): the formatting step failed: {why}"
            )));
        }
    }
    events.extend(limit_notes(out).into_iter().map(Event::Notice));
    events
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

    #[test]
    fn only_failures_a_second_try_could_fix_are_kept() {
        let api = |e| PipelineError::Api(e);
        assert!(retryable(&api(ApiError::Network("dns".into()))));
        assert!(retryable(&api(ApiError::Timeout)));
        assert!(retryable(&all(vec![
            ApiError::Timeout,
            daily(),
            ApiError::Timeout
        ])));
        assert!(
            !retryable(&all(vec![daily(), daily(), daily()])),
            "tomorrow, not now"
        );
        assert!(!retryable(&api(ApiError::BadKey("GEMINI_API_KEY".into()))));
        assert!(!retryable(&api(ApiError::MissingKey(
            "GEMINI_API_KEY".into()
        ))));
        assert!(!retry(false), "nothing kept: nothing to send again");
    }

    #[test]
    fn only_a_newer_result_changes_what_try_again_keeps() {
        // Nothing kept: a failure is kept, a text changes nothing.
        assert_eq!(keep_decision(None, 5, true, false), Some(true));
        assert_eq!(keep_decision(None, 5, false, true), None);
        // Clip 7 failed and is kept; clip 6 (older, slow) then comes through: 7 stays kept.
        assert_eq!(keep_decision(Some(7), 6, false, true), None);
        // Clip 8 (newer) comes through: forget 7.
        assert_eq!(keep_decision(Some(7), 8, false, true), Some(false));
        // An older failure never replaces a newer kept one; a newer one does.
        assert_eq!(keep_decision(Some(7), 6, true, false), None);
        assert_eq!(keep_decision(Some(7), 9, true, false), Some(true));
    }

    #[test]
    fn a_reload_waits_for_the_recording() {
        request_reload();
        assert!(!reload_now(true), "never during a recording");
        assert!(reload_now(false));
        assert!(!reload_now(false), "once per request");
    }

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

    fn spoken(text: &str) -> Outcome {
        Outcome {
            text: text.into(),
            sent: true,
            audio_ms: 2000,
            sent_ms: 1500,
            engine: "transcribe".into(),
            ..Outcome::default()
        }
    }

    #[test]
    fn a_pasted_text_is_one_text_event_with_its_timing() {
        let ev = outcome_events(&spoken("Haan bhai"), Paste::Pasted(310), 2100, "transcribe");
        let [
            Event::Text {
                text,
                paste,
                details,
            },
        ] = &ev[..]
        else {
            panic!("{ev:?}");
        };
        assert_eq!(text, "Haan bhai");
        assert_eq!(paste, &Paste::Pasted(310));
        assert!(
            details.starts_with("2.0 s recorded, 1.5 s sent · key-up → text 2100 ms")
                && details.ends_with(" · pasted in 310 ms"),
            "{details}"
        );
    }

    #[test]
    fn copied_no_speech_and_failures_become_their_events() {
        let ev = outcome_events(&spoken("x"), Paste::Copied("admin".into()), 1, "transcribe");
        assert!(matches!(&ev[0], Event::Text { paste: Paste::Copied(w), .. } if w == "admin"));
        let quiet = Outcome {
            audio_ms: 3000,
            ..Outcome::default()
        };
        assert_eq!(
            outcome_events(&quiet, Paste::Off, 1, "t"),
            [Event::NoSpeech { seconds: 3.0 }]
        );
        let failed = Outcome {
            error: Some(all(vec![daily(), daily(), daily()])),
            ..Outcome::default()
        };
        let ev = outcome_events(&failed, Paste::Off, 1, "t");
        let [Event::Failed { lines }] = &ev[..] else {
            panic!("{ev:?}");
        };
        assert_eq!(lines, &failure_notes(&all(vec![daily(), daily(), daily()])));
    }

    #[test]
    fn a_used_up_model_and_a_fallback_are_notices_after_the_text() {
        let mut out = spoken("ok");
        out.engine = "flash".into();
        out.passed = vec![sonicscript_core::api::Passed {
            engine: "transcribe".into(),
            why: Skip::Failed(daily()),
        }];
        let ev = outcome_events(&out, Paste::Pasted(1), 1, "transcribe");
        assert!(matches!(ev[0], Event::Text { .. }));
        assert!(
            matches!(&ev[1], Event::Notice(n) if n.starts_with("Used flash:")),
            "{ev:?}"
        );
        assert!(
            matches!(&ev[2], Event::Notice(n) if n.contains("used up today's free requests")),
            "{ev:?}"
        );
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
