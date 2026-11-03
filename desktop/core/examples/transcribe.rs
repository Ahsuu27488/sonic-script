//! Runs the real pipeline (voice gate → Opus → Groq Whisper → cleanup) on 16 kHz mono WAV files and prints
//! one JSON object per file. Needs `GROQ_API_KEY` in the environment. Requests are spaced by the provider's
//! `rpm` from `spec/providers.json`. Prints Whisper's text: run it on test recordings only.
//!
//! cargo run -q --release --example transcribe -- ../spec/fixtures/audio/*.wav > out.jsonl

use std::path::Path;
use std::time::{Duration, Instant};

use sonicscript_core::groq::Groq;
use sonicscript_core::normalize::Normalizer;
use sonicscript_core::pipeline;
use sonicscript_core::spec;
use sonicscript_core::vad::RATE;

fn main() {
    let engine = spec::default_engine();
    let gap = Duration::from_secs(60) / engine.provider.rpm.max(1);
    let groq = match Groq::from_env(engine) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let normalizer = Normalizer::new();
    groq.warm();
    let mut last = Instant::now() - gap;
    for path in std::env::args().skip(1) {
        let samples = match read(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{path}: {e}");
                continue;
            }
        };
        std::thread::sleep(gap.saturating_sub(last.elapsed()));
        last = Instant::now();
        let out = pipeline::run(&samples, &groq, &normalizer);
        let s = &out.stages;
        let json = serde_json::json!({
            "id": Path::new(&path).file_stem().map(|s| s.to_string_lossy()),
            "text": out.text.trim_end(),
            "raw": out.raw,
            "sent": out.sent,
            "phantom": out.phantom,
            "needs_llm": out.needs_llm,
            "audio_ms": out.audio_ms,
            "speech_ms": out.speech_ms,
            "sent_ms": out.sent_ms,
            "bytes": out.bytes,
            "retries": out.retries,
            "stages_ms": { "gate": s.gate, "encode": s.encode, "transcribe": s.transcribe,
                           "request": s.request, "clean": s.clean },
            "total_ms": out.total_ms,
            "error": out.error.map(|e| e.to_string()),
        });
        println!("{json}");
    }
}

fn read(path: &str) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    if reader.spec().sample_rate as usize != RATE || reader.spec().channels != 1 {
        return Err("needs 16 kHz mono".into());
    }
    Ok(reader
        .samples::<i16>()
        .map(|s| f32::from(s.unwrap_or(0)) / 32768.0)
        .collect())
}
