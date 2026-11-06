//! SonicScript for Windows (M1 plan: `docs/plans/2026-11-02-m1-windows-push-to-talk.md`). Without arguments
//! it listens for the dictation key and types what was said at the cursor (steps 5 and 7). `--file` and
//! `--bench` run the same pipeline on WAV files (the golden check, step 4). Dictation keeps each recording and
//! its text in the local history (`history`, D-032) unless `--no-history`.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sonicscript_core::pipeline::{self, Transcribe};

use sonicscript::key::Choice;
use sonicscript::{bench, key, keystore, models, setkey, settings, wav};
#[cfg(windows)]
use sonicscript::{cue, dictate, events, mywords};

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
Keys: sonicscript --key gemini|groq|openai saves one in Windows Credential Manager (--delete removes it);
GROQ_API_KEY / GEMINI_API_KEY in the environment are used first.";

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
    // The user's settings (D-045); the flags below override them for this run.
    let (settings, setting_notes) = settings::load();
    // D-041: off by default (settings `format`); --format turns smart formatting on, --no-format off.
    let format_on = take_flag(&mut args, "--format");
    let format_off = take_flag(&mut args, "--no-format");
    if format_on && format_off {
        eprintln!("--format and --no-format: pick one");
        return usage();
    }
    // --bench measures the spec's engines with the flags only, never the user's settings (comparable runs).
    let bench_run = args.iter().any(|a| a == "--bench");
    let format = format_on || (!bench_run && settings.format && !format_off);
    let keep_history = !take_flag(&mut args, "--no-history") && settings.history;
    if take_flag(&mut args, "--no-sound") || !settings.sound {
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
    if let Some(result) = keystore::command(&args) {
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
    let chosen = if bench_run {
        key::choose(engine_id.as_deref(), format).map(|c| (c, Vec::new()))
    } else {
        key::choose_from(&settings, engine_id.as_deref(), format)
    };
    let mut choice = match chosen {
        Ok((c, notes)) => {
            for note in setting_notes.iter().chain(&notes) {
                eprintln!("  ! {note}");
            }
            c
        }
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    // The user's words for dictation and --file; --bench stays without them (comparable runs).
    #[cfg(windows)]
    if !bench_run {
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
        None => dictate::run(choice, keep_history, std::sync::Arc::new(events::Console)),
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
    let client = key::engines_fixed(choice)?;
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
