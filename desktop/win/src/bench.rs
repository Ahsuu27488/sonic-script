//! `--bench <dir>`: the real pipeline on every WAV file in a folder (M1 plan step 4, the golden check without a
//! mic). Writes one row per clip in the shape of `eval`'s `results.jsonl`, so
//! `python -m sonic_eval rescore --run <run>` scores it with the M0 scorer and builds the report. Requests are
//! spaced by the first engine's `rpm`. With a chain (D-029) the rows are named `app-<first>~<next>~...` and
//! say which engine answered and which were passed over.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sonicscript_core::api::ApiError;
use sonicscript_core::normalize::Normalizer;
use sonicscript_core::pipeline::{self, Outcome, PipelineError, Transcribe};
use sonicscript_core::spec::{self, Engine, Refine};

use crate::key::Engines;
use crate::wav;

pub fn run(dir: &Path, out: &mut impl Write, client: &Engines) -> Result<Summary, String> {
    let files = wav_files(dir)?;
    if files.is_empty() {
        return Err(format!("no .wav files in {}", dir.display()));
    }
    let engines: Vec<&Engine> = client.links().map(|(c, _)| c.engine()).collect();
    let primary = engines[0];
    let ids: Vec<&str> = engines.iter().map(|e| e.id()).collect();
    // With smart formatting (D-030) the format step replaces each engine's own refine step, and its price.
    let format = client
        .links()
        .next()
        .is_some_and(|(c, _)| c.formats())
        .then(spec::format)
        .flatten();
    let name = format!(
        "app-{}{}",
        ids.join("~"),
        if format.is_some() { "+fmt" } else { "" }
    );
    let gap = Duration::from_secs(60) / primary.rpm().max(1);
    let normalizer = Normalizer::new();
    client.warm();
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
        let outcome = pipeline::run(&samples, client, &normalizer);
        let answered = engines
            .iter()
            .copied()
            .find(|e| e.id() == outcome.engine)
            .unwrap_or(primary);
        writeln!(
            out,
            "{}",
            row(&id, &outcome, &name, answered, format.as_ref())
        )
        .map_err(|e| e.to_string())?;
        eprintln!("[{}/{}] {id:<8} {}", i + 1, files.len(), status(&outcome));
        let fatal = matches!(
            &outcome.error,
            Some(PipelineError::Api(
                ApiError::BadKey(_) | ApiError::MissingKey(_)
            ))
        );
        done.push(outcome);
        if fatal {
            eprintln!("Stopping: this error will repeat. Fix it and run the same command again.");
            break;
        }
    }
    Ok(Summary::of(&done, primary.id()))
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
            "{:5.0} ms  (speech {:4.0} ms{}){}",
            o.total_ms,
            o.stages.request,
            if o.stages.refine > 0.0 {
                format!(", refine {:4.0} ms", o.stages.refine)
            } else {
                String::new()
            },
            if o.phantom { "  phantom" } else { "" }
        ),
    }
    .to_string()
        + &if o.passed.is_empty() {
            String::new()
        } else {
            format!("  {} → {}", passed(o).join("; "), o.engine)
        }
}

fn passed(o: &Outcome) -> Vec<String> {
    o.passed.iter().map(ToString::to_string).collect()
}

/// A row as `eval` writes it (`bench.Row`); `rescore` adds the category and the score fields. `name` is the
/// row's engine name; `engine` is the one that answered, which sets the cost, with `format` when the
/// formatting step ran in place of its refine step.
pub fn row(id: &str, o: &Outcome, name: &str, engine: &Engine, format: Option<&Refine>) -> Value {
    let s = &o.stages;
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default();
    json!({
        "engine": name,
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
            "refine": round1(s.refine),
            "transcribe": round1(s.transcribe),
            "clean": round1(s.clean),
        },
        "bytes": o.bytes,
        "duration_s": f64::from(o.audio_ms) / 1000.0,
        "tokens_in": 0,
        "tokens_out": 0,
        "cost_usd": cost_usd(engine, format, o),
        "retries": o.retries,
        "error": o.error.as_ref().map(ToString::to_string),
        "speech_ms": o.speech_ms,
        "sent": o.sent,
        "phantom": o.phantom,
        "needs_llm": o.needs_llm,
        "answered_by": if o.sent && o.error.is_none() { Some(&o.engine) } else { None },
        "passed": passed(o),
        "unformatted": o.unformatted,
        "at": at,
    })
}

/// What the clip cost, by the spec's prices (as `eval` `cost_usd`): per second of audio for transcription
/// APIs (Groq bills at least 10 s), per token for Gemini, plus the refine step's tokens. Only the answer
/// that was used is counted; a losing request's cost is unknown (its tokens never come back).
fn cost_usd(engine: &Engine, format: Option<&Refine>, o: &Outcome) -> f64 {
    if !o.sent {
        return 0.0;
    }
    let [tin, tout, rin, rout] = o.tokens.map(f64::from);
    let speech = match engine {
        Engine::Whisper(e) => {
            let billed_s = (f64::from(o.sent_ms) / 1000.0).max(e.provider.min_billed_s);
            billed_s / 3600.0 * e.audio_per_hour
        }
        Engine::Gemini(e) => (tin * e.in_per_m + tout * e.out_per_m) / 1e6,
    };
    let refine = format
        .or(engine.refine())
        .map_or(0.0, |r| (rin * r.in_per_m + rout * r.out_per_m) / 1e6);
    speech + refine
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
    /// Clips where the chain passed over an engine, and how many were answered by another than the first.
    pub fallback_asked: usize,
    pub fallback_used: usize,
    /// Clips answered per engine (sent, no error), most first.
    pub answers: Vec<(String, usize)>,
    /// Clip in → text out, over the clips that were sent without error.
    pub total_p50: f64,
    pub total_p95: f64,
    pub request_p50: f64,
    pub request_p95: f64,
    /// Mean size of the clips that were sent without error.
    pub kb: f64,
}

impl Summary {
    /// `primary` is the id of the engine asked first.
    pub fn of(outcomes: &[Outcome], primary: &str) -> Self {
        let ok: Vec<&Outcome> = outcomes
            .iter()
            .filter(|o| o.sent && o.error.is_none())
            .collect();
        let total: Vec<f64> = ok.iter().map(|o| o.total_ms).collect();
        let request: Vec<f64> = ok.iter().map(|o| o.stages.request).collect();
        let bytes: usize = ok.iter().map(|o| o.bytes).sum();
        let mut answers: Vec<(String, usize)> = Vec::new();
        for o in &ok {
            match answers.iter_mut().find(|(e, _)| *e == o.engine) {
                Some((_, n)) => *n += 1,
                None => answers.push((o.engine.clone(), 1)),
            }
        }
        answers.sort_by_key(|a| std::cmp::Reverse(a.1));
        Self {
            clips: outcomes.len(),
            sent: outcomes.iter().filter(|o| o.sent).count(),
            errors: outcomes.iter().filter(|o| o.error.is_some()).count(),
            retries: outcomes.iter().map(|o| u32::from(o.retries)).sum(),
            fallback_asked: outcomes.iter().filter(|o| !o.passed.is_empty()).count(),
            fallback_used: ok
                .iter()
                .filter(|o| !o.passed.is_empty() && o.engine != primary)
                .count(),
            answers,
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
        if self.fallback_asked > 0 {
            writeln!(
                f,
                "an engine was passed over for {} clips; {} were answered by a later one",
                self.fallback_asked, self.fallback_used
            )?;
        }
        if self.answers.len() > 1 {
            let each: Vec<String> = self
                .answers
                .iter()
                .map(|(e, n)| format!("{e} {n}"))
                .collect();
            writeln!(f, "answers: {}", each.join(", "))?;
        }
        write!(
            f,
            "clip -> text p50 {:.0} ms / p95 {:.0} ms; speech request p50 {:.0} ms / p95 {:.0} ms",
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
    use sonicscript_core::api::{Passed, Skip};
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
        let e = spec::engine("groq-whisper-hint").unwrap();
        let r = row(
            "mx-01",
            &outcome(true, 340.0, None),
            "app-groq-whisper-hint",
            &e,
            None,
        );
        assert_eq!(r["engine"], "app-groq-whisper-hint");
        assert_eq!(r["sentence"], "mx-01");
        assert_eq!(
            r["text"], "Yaar PR merge kar do",
            "no trailing space, like eval rows"
        );
        assert_eq!(r["error"], Value::Null);
        let cost = r["cost_usd"].as_f64().unwrap();
        let Engine::Whisper(w) = &e else { panic!() };
        assert!((cost - 10.0 / 3600.0 * w.audio_per_hour).abs() < 1e-12);

        let quiet = row(
            "si-01",
            &outcome(false, 2.0, None),
            "app-groq-whisper-hint",
            &e,
            None,
        );
        assert_eq!(
            quiet["raw_asr"],
            Value::Null,
            "not sent: Whisper never saw it"
        );
        assert_eq!(quiet["cost_usd"], 0.0);
        assert_eq!(quiet["answered_by"], Value::Null);
    }

    #[test]
    fn rows_say_which_engine_answered_and_which_were_passed_over() {
        let groq = spec::engine("groq-whisper-hint").unwrap();
        let mut o = outcome(true, 3400.0, None);
        o.engine = "groq-whisper-hint".into();
        o.passed = vec![
            Passed {
                engine: "gemini-3.5-flash".into(),
                why: Skip::Resting,
            },
            Passed {
                engine: "gemini-3.5-flash-lite".into(),
                why: Skip::Slow,
            },
        ];
        let r = row("mx-01", &o, "app-a~b", &groq, None);
        assert_eq!(r["engine"], "app-a~b");
        assert_eq!(r["answered_by"], "groq-whisper-hint");
        assert_eq!(
            r["passed"],
            json!([
                "gemini-3.5-flash skipped (rate limit)",
                "gemini-3.5-flash-lite was slow"
            ])
        );
        assert!(r["cost_usd"].as_f64().unwrap() > 0.0, "priced as Groq");
    }

    #[test]
    fn summary_counts_only_clean_sent_clips_for_latency() {
        let gemini_slow = || {
            vec![Passed {
                engine: "gemini".into(),
                why: Skip::Slow,
            }]
        };
        let mut slow = outcome(true, 3300.0, None);
        (slow.engine, slow.passed) = ("groq".into(), gemini_slow());
        let mut late = outcome(true, 3500.0, None);
        (late.engine, late.passed) = ("gemini".into(), gemini_slow());
        let s = Summary::of(
            &[
                outcome(true, 300.0, None),
                outcome(true, 500.0, None),
                outcome(false, 1.0, None),
                outcome(true, 9000.0, Some(PipelineError::Api(ApiError::Timeout))),
                slow,
                late,
            ],
            "gemini",
        );
        assert_eq!((s.clips, s.sent, s.errors), (6, 5, 1));
        assert_eq!((s.fallback_asked, s.fallback_used), (2, 1));
        assert_eq!(s.answers[1], ("groq".to_string(), 1));
        assert_eq!(s.total_p50, 1900.0);
        assert_eq!(s.request_p50, 950.0);
        assert_eq!(s.kb, 8.0);
    }
}

#[cfg(test)]
mod cost_tests {
    use super::*;
    use sonicscript_core::spec;

    #[test]
    fn gemini_and_refine_are_priced_per_token() {
        let e = spec::engine("gemini-3.5-transcribe+gptoss20b").unwrap();
        let o = Outcome {
            sent: true,
            sent_ms: 4000,
            tokens: [100, 20, 300, 10],
            ..Outcome::default()
        };
        let Engine::Gemini(g) = &e else { panic!() };
        let r = g.refine.as_ref().unwrap();
        let want =
            (100.0 * g.in_per_m + 20.0 * g.out_per_m + 300.0 * r.in_per_m + 10.0 * r.out_per_m)
                / 1e6;
        assert!((cost_usd(&e, None, &o) - want).abs() < 1e-15);
        assert!(want > 0.0);
    }
}
