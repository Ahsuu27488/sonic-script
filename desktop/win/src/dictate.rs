//! Dictation mode, the default when `sonicscript.exe` runs without arguments (M1 plan step 5). The dictation
//! key opens the mic and warms both engines' connections at once; stopping runs the pipeline on a worker thread and
//! prints the text. Pasting it at the cursor is step 7.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use sonicscript_core::api::{ApiError, Skip};
use sonicscript_core::keys::Action;
use sonicscript_core::normalize::Normalizer;
use sonicscript_core::pipeline::{self, Outcome, Transcribe};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_RCONTROL;

use crate::audio::Recorder;
use crate::key::{Choice, Engines};
use crate::{hotkey, key};

/// Safety cap on one recording (M1 plan step 5): a forgotten toggle does not keep the mic open.
const MAX_RECORDING: Duration = Duration::from_secs(5 * 60);

pub fn run(choice: Choice) -> Result<(), String> {
    let client = Arc::new(key::engines(choice)?);
    let mut id = key::describe(&client);
    if !client.first().formats() {
        id += "; no formatting";
    }
    let normalizer = Arc::new(Normalizer::new());
    let (tx, rx) = mpsc::channel();
    // The key is a setting from step 6 on (D-018); Right Ctrl is the default (D-015).
    hotkey::spawn(VK_RCONTROL, tx)?;
    println!(
        "SonicScript is listening ({id}).\n  \
         Tap Right Ctrl, speak, tap it again or press Enter: the text appears here.\n  \
         Or hold Right Ctrl while you speak. Esc cancels. Close this window or press Ctrl+C to quit."
    );

    let mut recording: Option<(Recorder, Instant)> = None;
    loop {
        let action = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(action) => action,
            Err(RecvTimeoutError::Timeout) => match &recording {
                Some((_, since)) if since.elapsed() >= MAX_RECORDING => {
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
                let client = client.clone();
                std::thread::spawn(move || client.warm());
                let t = Instant::now();
                match Recorder::start() {
                    Ok(r) => {
                        println!(
                            "● Recording ({}, {} Hz, mic open in {} ms)",
                            r.device,
                            r.rate,
                            t.elapsed().as_millis()
                        );
                        recording = Some((r, Instant::now()));
                    }
                    Err(e) => {
                        hotkey::force_stop();
                        eprintln!("  Could not open the microphone: {e}");
                    }
                }
            }
            Action::Cancel => {
                if let Some((r, _)) = recording.take() {
                    drop(r.stop());
                    println!("  Cancelled.");
                }
            }
            Action::Stop => {
                let Some((r, _)) = recording.take() else {
                    continue;
                };
                let stopped = Instant::now();
                let rec = r.stop();
                let (client, normalizer) = (client.clone(), normalizer.clone());
                std::thread::spawn(move || transcribe(rec, stopped, &client, &normalizer));
            }
        }
    }
}

/// `--record <seconds>`: the same mic → pipeline path without the key, to check the microphone (and to test
/// it where no key can be pressed, e.g. a locked PC).
pub fn record_for(seconds: f64, choice: Choice) -> Result<(), String> {
    let client = key::engines(choice)?;
    let normalizer = Normalizer::new();
    let warm = std::thread::spawn(move || {
        client.warm();
        client
    });
    let r = Recorder::start()?;
    println!("● Recording {seconds} s ({}, {} Hz)", r.device, r.rate);
    std::thread::sleep(Duration::from_secs_f64(seconds));
    let stopped = Instant::now();
    let rec = r.stop();
    let client = warm.join().map_err(|_| "warm-up thread panicked")?;
    transcribe(rec, stopped, &client, &normalizer);
    Ok(())
}

fn transcribe(
    rec: crate::audio::Recording,
    stopped: Instant,
    client: &Engines,
    normalizer: &Normalizer,
) {
    let stream = match rec.stream {
        Ok(s) => s,
        Err(e) => {
            eprintln!("  {e}");
            return;
        }
    };
    let out = pipeline::run_streamed(&rec.samples, stream, client, normalizer);
    let key_up_to_text = stopped.elapsed().as_millis();
    let seconds = f64::from(out.audio_ms) / 1000.0;
    if let Some(e) = &out.error {
        // In a chain, an error comes back only when every engine asked failed.
        let both = if client.links().count() > 1 {
            " (every engine in the chain failed)"
        } else {
            ""
        };
        eprintln!("  Error: {e}{both}");
    } else if !out.sent {
        println!("  No speech heard ({seconds:.1} s recorded): nothing sent.");
    } else if out.text.is_empty() {
        println!("  Nothing to type (the engine only heard its own hint).");
    } else {
        println!("→ {}", out.text.trim_end().replace('\n', "\n  "));
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
            "  {seconds:.1} s recorded, {:.1} s sent · key-up → text {key_up_to_text} ms (speech {:.0} ms{refine}){note}",
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
