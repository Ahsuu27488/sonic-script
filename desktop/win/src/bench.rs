//! `--bench <dir>`: the real pipeline on every WAV file in a folder (M1 plan step 4, the golden check without a
//! mic). Writes one row per clip in the shape of `eval`'s `results.jsonl`, so
//! `python -m sonic_eval rescore --run <run>` scores it with the M0 scorer and builds the report. Requests are
//! spaced by the provider's `rpm`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sonicscript_core::groq::{Groq, GroqError};
use sonicscript_core::normalize::Normalizer;
use sonicscript_core::pipeline::{self, Outcome, PipelineError};
use sonicscript_core::spec::WhisperEngine;

use crate::wav;

/// Groq bills Whisper with a 10-second minimum per request (same as `eval` `GroqWhisper.cost_usd`).
const MIN_BILLED_S: f64 = 10.0;

pub fn run(dir: &Path, out: &mut impl Write, groq: &Groq) -> Result<Summary, String> {
    let files = wav_files(dir)?;
    if files.is_empty() {
        return Err(format!("no .wav files in {}", dir.display()));
    }
    let engine = groq.engine();
    let gap = Duration::from_secs(60) / engine.provider.rpm.max(1);
    let normalizer = Normalizer::new();
    groq.warm();
    let mut done = Vec::new();
    let mut last: Option<Instant> = None;
    for (i, path) in files.iter().enumerate() {
        let id = path.file_stem().unwrap_or_default().to_string_lossy();
        let samples = match wav::read(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[{}/{}] {id}: skipped, {e}", i + 1, files.len());
                continue;
            }
        };
        if let Some(t) = last {
            std::thread::sleep(gap.saturating_sub(t.elapsed()));
        }
        last = Some(Instant::now());
        let outcome = pipeline::run(&samples, groq, &normalizer);
        writeln!(out, "{}", row(&id, &outcome, engine)).map_err(|e| e.to_string())?;
        eprintln!("[{}/{}] {id:<8} {}", i + 1, files.len(), status(&outcome));
        let fatal = matches!(
            &outcome.error,
            Some(PipelineError::Groq(
                GroqError::BadKey | GroqError::MissingKey(_)
            ))
        );
        done.push(outcome);
        if fatal {
            eprintln!("Stopping: this error will repeat. Fix it and run the same command again.");
            break;
        }
    }
    Ok(Summary::of(&done))
}

fn wav_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))
        .collect();
    files.sort();
    Ok(files)
}

/// One console line per clip, without the text (the text goes only into the rows).
fn status(o: &Outcome) -> String {
    match (&o.error, o.sent) {
        (Some(e), _) => format!("ERROR {e}"),
        (None, false) => format!("not sent ({} ms of speech)", o.speech_ms),
        (None, true) => format!(
            "{:5.0} ms  (Whisper {:4.0} ms){}",
            o.total_ms,
            o.stages.request,
            if o.phantom { "  phantom" } else { "" }
        ),
    }
}

/// A row as `eval` writes it (`bench.Row`); `rescore` adds the category and the score fields.
pub fn row(id: &str, o: &Outcome, engine: &WhisperEngine) -> Value {
    let s = &o.stages;
    let billed_s = f64::from(o.sent_ms) / 1000.0;
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default();
    json!({
        "engine": format!("app-{}", engine.id),
        "fmt": "opus16",
        "sentence": id,
        "category": null,
        "repeat": 0,
        "text": o.text.trim_end(),
        "raw_asr": if o.sent { Some(&o.raw) } else { None },
        "latency_ms": round1(o.total_ms),
        "stages_ms": {
            "gate": round1(s.gate),
            "encode": round1(s.encode),
            "whisper": round1(s.request),
            "transcribe": round1(s.transcribe),
            "clean": round1(s.clean),
        },
        "bytes": o.bytes,
        "duration_s": f64::from(o.audio_ms) / 1000.0,
        "tokens_in": 0,
        "tokens_out": 0,
        "cost_usd": if o.sent {
            billed_s.max(MIN_BILLED_S) / 3600.0 * engine.audio_per_hour
        } else {
            0.0
        },
        "retries": o.retries,
        "error": o.error.as_ref().map(ToString::to_string),
        "speech_ms": o.speech_ms,
        "sent": o.sent,
        "phantom": o.phantom,
        "needs_llm": o.needs_llm,
        "at": at,
    })
}

fn round1(ms: f64) -> f64 {
    (ms * 10.0).round() / 10.0
}

/// What the console shows after a run; accuracy comes from `eval` (one scorer for every platform).
#[derive(Debug, PartialEq)]
pub struct Summary {
    pub clips: usize,
    pub sent: usize,
    pub errors: usize,
    pub retries: u32,
    /// Clip in → text out, over the clips that were sent without error.
    pub total_p50: f64,
    pub total_p95: f64,
    pub request_p50: f64,
    pub request_p95: f64,
    /// Mean size of the clips that were sent without error.
    pub kb: f64,
}

impl Summary {
    pub fn of(outcomes: &[Outcome]) -> Self {
        let ok: Vec<&Outcome> = outcomes
            .iter()
            .filter(|o| o.sent && o.error.is_none())
            .collect();
        let total: Vec<f64> = ok.iter().map(|o| o.total_ms).collect();
        let request: Vec<f64> = ok.iter().map(|o| o.stages.request).collect();
        let bytes: usize = ok.iter().map(|o| o.bytes).sum();
        Self {
            clips: outcomes.len(),
            sent: outcomes.iter().filter(|o| o.sent).count(),
            errors: outcomes.iter().filter(|o| o.error.is_some()).count(),
            retries: outcomes.iter().map(|o| u32::from(o.retries)).sum(),
            total_p50: percentile(&total, 50.0),
            total_p95: percentile(&total, 95.0),
            request_p50: percentile(&request, 50.0),
            request_p95: percentile(&request, 95.0),
            kb: bytes as f64 / ok.len().max(1) as f64 / 1024.0,
        }
    }
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{} clips, {} sent, {} errors, {} retries, {:.1} KB per sent clip",
            self.clips, self.sent, self.errors, self.retries, self.kb
        )?;
        write!(
            f,
            "clip -> text p50 {:.0} ms / p95 {:.0} ms; Whisper request p50 {:.0} ms / p95 {:.0} ms",
            self.total_p50, self.total_p95, self.request_p50, self.request_p95
        )
    }
}

/// Linear interpolation between ranks, as `eval` `bench.percentile`.
pub fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut s = values.to_vec();
    s.sort_by(f64::total_cmp);
    let k = (s.len() - 1) as f64 * p / 100.0;
    let (lo, hi) = (k as usize, (k as usize + 1).min(s.len() - 1));
    s[lo] + (s[hi] - s[lo]) * (k - lo as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonicscript_core::spec;

    #[test]
    fn percentile_matches_eval() {
        // eval: percentile([100, 200, 300, 400], 50) == 250, (.., 95) == 385
        let v = [400.0, 100.0, 300.0, 200.0];
        assert_eq!(percentile(&v, 50.0), 250.0);
        assert!((percentile(&v, 95.0) - 385.0).abs() < 1e-9);
        assert_eq!(percentile(&[], 50.0), 0.0);
    }

    fn outcome(sent: bool, total_ms: f64, error: Option<PipelineError>) -> Outcome {
        let mut o = Outcome {
            sent,
            total_ms,
            error,
            text: if sent {
                "Yaar PR merge kar do ".into()
            } else {
                String::new()
            },
            raw: "Yaar PR merge kar do".into(),
            audio_ms: 4000,
            sent_ms: 3500,
            bytes: if sent { 8192 } else { 0 },
            ..Outcome::default()
        };
        o.stages.request = total_ms / 2.0;
        o
    }

    #[test]
    fn rows_have_the_eval_shape_and_bill_ten_seconds_minimum() {
        let e = spec::default_engine();
        let r = row("mx-01", &outcome(true, 340.0, None), &e);
        assert_eq!(r["engine"], "app-groq-whisper-hint");
        assert_eq!(r["sentence"], "mx-01");
        assert_eq!(
            r["text"], "Yaar PR merge kar do",
            "no trailing space, like eval rows"
        );
        assert_eq!(r["error"], Value::Null);
        let cost = r["cost_usd"].as_f64().unwrap();
        assert!((cost - 10.0 / 3600.0 * e.audio_per_hour).abs() < 1e-12);

        let quiet = row("si-01", &outcome(false, 2.0, None), &e);
        assert_eq!(
            quiet["raw_asr"],
            Value::Null,
            "not sent: Whisper never saw it"
        );
        assert_eq!(quiet["cost_usd"], 0.0);
    }

    #[test]
    fn summary_counts_only_clean_sent_clips_for_latency() {
        let s = Summary::of(&[
            outcome(true, 300.0, None),
            outcome(true, 500.0, None),
            outcome(false, 1.0, None),
            outcome(true, 9000.0, Some(PipelineError::Groq(GroqError::Timeout))),
        ]);
        assert_eq!((s.clips, s.sent, s.errors), (4, 3, 1));
        assert_eq!(s.total_p50, 400.0);
        assert_eq!(s.request_p50, 200.0);
        assert_eq!(s.kb, 8.0);
    }
}
