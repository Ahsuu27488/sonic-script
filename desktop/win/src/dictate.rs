//! Dictation mode, the default when `sonicscript.exe` runs without arguments (M1 plan step 5). The dictation
//! key opens the mic and warms the engine's connections at once; stopping runs the pipeline on a worker thread and
//! prints the text. Pasting it at the cursor is step 7.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use sonicscript_core::client::Client;
use sonicscript_core::keys::Action;
use sonicscript_core::normalize::Normalizer;
use sonicscript_core::pipeline;
use sonicscript_core::spec::Engine;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_RCONTROL;

use crate::audio::Recorder;
use crate::{hotkey, key};

/// Safety cap on one recording (M1 plan step 5): a forgotten toggle does not keep the mic open.
const MAX_RECORDING: Duration = Duration::from_secs(5 * 60);

pub fn run(engine: Engine) -> Result<(), String> {
    let id = engine.id().to_string();
    let client = Arc::new(key::client(engine).map_err(|e| format!("{id}: {e}"))?);
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
pub fn record_for(seconds: f64, engine: Engine) -> Result<(), String> {
    let id = engine.id().to_string();
    let client = key::client(engine).map_err(|e| format!("{id}: {e}"))?;
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
    client: &Client,
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
        eprintln!("  Error: {e}");
    } else if !out.sent {
        println!("  No speech heard ({seconds:.1} s recorded): nothing sent.");
    } else if out.text.is_empty() {
        println!("  Nothing to type (the engine only heard its own hint).");
    } else {
        println!("→ {}", out.text.trim_end());
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
    }
}
