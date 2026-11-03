//! SonicScript for Windows (M1 plan: `docs/plans/2026-11-02-m1-windows-push-to-talk.md`). Without arguments
//! it listens for the dictation key and prints what was said (step 5; pasting is step 7). `--file` and
//! `--bench` run the same pipeline on WAV files (the golden check, step 4).

#[cfg(windows)]
mod audio;
mod bench;
#[cfg(windows)]
mod dictate;
#[cfg(windows)]
mod hotkey;
mod key;
mod models;
mod wav;

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sonicscript_core::normalize::Normalizer;
use sonicscript_core::pipeline::{self, Transcribe};

use crate::key::Choice;

const USAGE: &str = "\
usage: sonicscript [--engine <id>] [mode]
  sonicscript                                 dictation: tap or hold Right Ctrl, speak, get the text
  sonicscript --models                        the models you can use: free / paid, limits, test results
  sonicscript --record <seconds>              mic check: record, then print the text (0.5-300 s)
  sonicscript --file <wav>...                 transcribe 16 kHz mono WAV files, print the text
  sonicscript --bench <dir> [--out <file>]    run every WAV in <dir>, write eval rows (JSON lines)
Without --engine the spec's default_engine runs, with its fallback_engine for slow or failed requests;
--engine <id> runs that engine from spec/providers.json alone.
Keys come from GROQ_API_KEY / GEMINI_API_KEY (this console or your Windows user environment).";

fn main() -> ExitCode {
    // Text and status symbols are UTF-8; the console's default code page would garble them.
    #[cfg(windows)]
    // SAFETY: plain Win32 call; failing only leaves the old code page.
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleOutputCP(65001);
    }
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let engine_id = match args.iter().position(|a| a == "--engine") {
        Some(i) if i + 1 < args.len() => {
            let id = args.remove(i + 1);
            args.remove(i);
            Some(id)
        }
        Some(_) => return usage(),
        None => None,
    };
    let choice = match key::choose(engine_id.as_deref()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let result = match args.first().map(String::as_str) {
        Some("--models") => models::print(),
        Some("--file") if args.len() > 1 => file(&args[1..], choice),
        Some("--bench") => match &args[1..] {
            [dir] => bench(Path::new(dir), None, choice),
            [dir, flag, out] if flag == "--out" => {
                bench(Path::new(dir), Some(PathBuf::from(out)), choice)
            }
            _ => return usage(),
        },
        #[cfg(windows)]
        None => dictate::run(choice),
        #[cfg(windows)]
        Some("--record") => match args.get(1).and_then(|s| s.parse::<f64>().ok()) {
            Some(s) if args.len() == 2 && (0.5..=300.0).contains(&s) => {
                dictate::record_for(s, choice)
            }
            _ => return usage(),
        },
        _ => return usage(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

/// Prints the text to paste for each file (one line each) and its timings to stderr.
fn file(paths: &[String], choice: Choice) -> Result<(), String> {
    let client = key::engines(choice)?;
    let normalizer = Normalizer::new();
    client.warm();
    for path in paths {
        let samples = wav::read(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
        let out = pipeline::run(&samples, &client, &normalizer);
        if let Some(e) = &out.error {
            return Err(format!("{path}: {e}"));
        }
        println!("{}", out.text.trim_end());
        let note = match (out.sent, out.phantom, out.needs_llm) {
            (false, ..) => " (no speech, not sent)",
            (_, true, _) => " (only the hint came back: dropped)",
            (_, _, true) => " (mostly Urdu script: rough romanization)",
            _ => "",
        };
        eprintln!(
            "{path}: {:.0} ms clip -> text, speech {:.0} ms, refine {:.0} ms, {} bytes, {}{note}",
            out.total_ms, out.stages.request, out.stages.refine, out.bytes, out.engine
        );
    }
    Ok(())
}

fn bench(dir: &Path, out: Option<PathBuf>, choice: Choice) -> Result<(), String> {
    let client = key::engines(choice)?;
    let summary = match &out {
        Some(path) => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            let f = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut w = BufWriter::new(f);
            let summary = bench::run(dir, &mut w, &client)?;
            w.flush().map_err(|e| e.to_string())?;
            summary
        }
        None => bench::run(dir, &mut io::stdout().lock(), &client)?,
    };
    eprintln!("\n{summary}");
    if let Some(path) = out {
        eprintln!(
            "Rows: {} (score them with eval: python -m sonic_eval rescore --run <folder name>)",
            path.display()
        );
    }
    Ok(())
}
