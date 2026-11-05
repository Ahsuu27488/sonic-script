//! SonicScript for Windows (M1 plan: `docs/plans/2026-11-02-m1-windows-push-to-talk.md`). Without arguments
//! it listens for the dictation key and types what was said at the cursor (steps 5 and 7). `--file` and
//! `--bench` run the same pipeline on WAV files (the golden check, step 4). Dictation keeps each recording and
//! its text in the local history (`history`, D-032) unless `--no-history`.

#[cfg(windows)]
mod audio;
mod bench;
#[cfg(windows)]
mod cue;
#[cfg(windows)]
mod dictate;
#[cfg(windows)]
mod history;
#[cfg(windows)]
mod hotkey;
#[cfg(windows)]
mod inject;
mod key;
mod models;
#[cfg(windows)]
mod mywords;
mod setkey;
mod wav;

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sonicscript_core::pipeline::{self, Transcribe};

use crate::key::Choice;

const USAGE: &str = "\
usage: sonicscript [--engine <id>] [--format] [--no-history] [--no-sound] [mode]
  sonicscript                                 dictation: tap or hold the dictation key (Right Ctrl), speak; the text is typed at the cursor
  sonicscript --set-key [<name> | list]       choose the dictation key: press it, or give its name (F9, RightAlt, Mouse4, ...)
  sonicscript --models                        the models you can use: free / paid, limits, test results
  sonicscript --record <seconds>              mic check: record, then print the text (0.5-300 s)
  sonicscript --file <wav>...                 transcribe 16 kHz mono WAV files, print the text
  sonicscript --bench <dir> [--out <file>]    run every WAV in <dir>, write eval rows (JSON lines)
Without --engine the spec's engine chain runs (D-029, D-035): Gemini 3.5 Transcribe, then Gemini 3.5 Flash
and Flash-Lite, each asked when the one before is slow, fails or is out of free requests;
--engine <id> runs that engine from spec/providers.json alone.
The text comes as it was said (exact words, in Roman letters). --format adds smart formatting on Groq after
whichever engine answers: lists, take-backs, fillers dropped, numbers as digits.
Dictation and --record keep each recording (Opus, about 2 KB a second) and its text in
%LOCALAPPDATA%\\SonicScript\\history (history.jsonl + audio\\); --no-history keeps nothing.
A short chime plays when dictation starts, stops or is cancelled; --no-sound turns it off.
Your words (names and terms to always get right): %APPDATA%\\SonicScript\\words.txt
  sonicscript --words                         open your word list in Notepad (the format is explained at the top)
  sonicscript --words-prompt                  copy a prompt for your AI assistant: it writes your word list
  sonicscript --words-add <file>...           add the words in text files (any list works)
  sonicscript --words-paste                   add the words on the clipboard (e.g. your AI assistant's answer)
  sonicscript --words-suggest                 pick names from your dictation history to add
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
    // D-041: off by default; --format turns smart formatting on. --no-format (the old switch) is still taken.
    let format = take_flag(&mut args, "--format");
    if take_flag(&mut args, "--no-format") && format {
        eprintln!("--format and --no-format: pick one");
        return usage();
    }
    let keep_history = match args.iter().position(|a| a == "--no-history") {
        Some(i) => {
            args.remove(i);
            false
        }
        None => true,
    };
    if let Some(i) = args.iter().position(|a| a == "--no-sound") {
        args.remove(i);
        #[cfg(windows)]
        cue::set_enabled(false);
    }
    if let Some(result) = setkey::command(&args) {
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(2)
            }
        };
    }
    #[cfg(windows)]
    if let Some(result) = mywords::command(&args) {
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(2)
            }
        };
    }
    let mut choice = match key::choose(engine_id.as_deref(), format) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    // The user's words for dictation and --file; --bench stays without them (comparable runs).
    #[cfg(windows)]
    if args.first().map(String::as_str) != Some("--bench") {
        choice.words = Some(std::sync::Arc::new(mywords::load()));
    }
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
        None => dictate::run(choice, keep_history),
        #[cfg(windows)]
        Some("--record") => match args.get(1).and_then(|s| s.parse::<f64>().ok()) {
            Some(s) if args.len() == 2 && (0.5..=300.0).contains(&s) => {
                dictate::record_for(s, choice, keep_history)
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

/// Removes `flag` from `args`; whether it was there.
fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    match args.iter().position(|a| a == flag) {
        Some(i) => {
            args.remove(i);
            true
        }
        None => false,
    }
}

/// Prints the text to paste for each file (one line each) and its timings to stderr.
fn file(paths: &[String], choice: Choice) -> Result<(), String> {
    let normalizer = key::normalizer(&choice);
    let client = key::engines(choice)?;
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
