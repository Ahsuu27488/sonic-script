//! Runs the voice gate on 16 kHz mono WAV files and prints one line per file: speech ms, kept seconds,
//! and whether the clip would be sent.
//!
//! cargo run -q --example gate -- ../spec/fixtures/audio/*.wav

use sonicscript_core::vad::{self, RATE};

fn main() {
    for path in std::env::args().skip(1) {
        let mut reader = match hound::WavReader::open(&path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{path}: {e}");
                continue;
            }
        };
        if reader.spec().sample_rate as usize != RATE || reader.spec().channels != 1 {
            eprintln!("{path}: needs 16 kHz mono");
            continue;
        }
        let samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| f32::from(s.unwrap_or(0)) / 32768.0)
            .collect();
        let d = vad::detect(&samples);
        println!(
            "{path}\t{:.1} s\tspeech {} ms\tkept {:.1} s\t{}",
            samples.len() as f32 / RATE as f32,
            d.speech_ms,
            d.keep.len() as f32 / RATE as f32,
            if d.has_speech() { "send" } else { "drop" }
        );
    }
}
