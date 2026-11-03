//! SonicScript for Windows (M1 plan: `docs/plans/2026-11-02-m1-windows-push-to-talk.md`). So far the golden
//! check without a mic (step 4): `--file` and `--bench` run the real pipeline on WAV files. The hotkey,
//! recording and paste come in steps 5-7.

mod bench;
mod key;
mod wav;

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sonicscript_core::normalize::Normalizer;
use sonicscript_core::pipeline;
use sonicscript_core::spec;

const USAGE: &str = "\
usage:
  sonicscript --file <wav>...                 transcribe 16 kHz mono WAV files, print the text
  sonicscript --bench <dir> [--out <file>]    run every WAV in <dir>, write eval rows (JSON lines)
The Groq key comes from GROQ_API_KEY (this console or your Windows user environment).";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("--file") if args.len() > 1 => file(&args[1..]),
        Some("--bench") => match &args[1..] {
            [dir] => bench(Path::new(dir), None),
            [dir, flag, out] if flag == "--out" => bench(Path::new(dir), Some(PathBuf::from(out))),
            _ => Err(USAGE.into()),
        },
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

/// Prints the text to paste for each file (one line each) and its timings to stderr.
fn file(paths: &[String]) -> Result<(), String> {
    let groq = key::groq(spec::default_engine()).map_err(|e| e.to_string())?;
    let normalizer = Normalizer::new();
    groq.warm();
    for path in paths {
        let samples = wav::read(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
        let out = pipeline::run(&samples, &groq, &normalizer);
        if let Some(e) = &out.error {
            return Err(format!("{path}: {e}"));
        }
        println!("{}", out.text.trim_end());
        let note = match (out.sent, out.phantom, out.needs_llm) {
            (false, ..) => " (no speech, not sent)",
            (_, true, _) => " (Whisper invented text: dropped)",
            (_, _, true) => " (mostly Urdu script: rough romanization)",
            _ => "",
        };
        eprintln!(
            "{path}: {:.0} ms clip -> text, Whisper {:.0} ms, {} bytes{note}",
            out.total_ms, out.stages.request, out.bytes
        );
    }
    Ok(())
}

fn bench(dir: &Path, out: Option<PathBuf>) -> Result<(), String> {
    let groq = key::groq(spec::default_engine()).map_err(|e| e.to_string())?;
    let summary = match &out {
        Some(path) => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            let f = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut w = BufWriter::new(f);
            let summary = bench::run(dir, &mut w, &groq)?;
            w.flush().map_err(|e| e.to_string())?;
            summary
        }
        None => bench::run(dir, &mut io::stdout().lock(), &groq)?,
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
