//! One clip → text to paste, with a timing per stage (M1 plan step 3): voice gate → trim → Opus → Whisper
//! → text cleanup. A clip without speech never reaches Whisper (D-007, D-020). The timings are what the
//! latency log (plan step 9) and the learned deadline (D-017) will read.

use std::time::Instant;

use crate::encode::{self, EncodeError, Encoded};
use crate::groq::{Groq, GroqError, Transcript};
use crate::normalize::Normalizer;
use crate::vad::{self, RATE};

/// Anything that turns an encoded clip into raw Whisper text. [`Groq`] in the app, a fake in tests.
pub trait Transcribe {
    fn transcribe(&self, audio: &Encoded) -> Result<Transcript, GroqError>;
}

impl Transcribe for Groq {
    fn transcribe(&self, audio: &Encoded) -> Result<Transcript, GroqError> {
        Groq::transcribe(self, audio)
    }
}

#[derive(Debug)]
pub enum PipelineError {
    Encode(EncodeError),
    Groq(GroqError),
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(e) => e.fmt(f),
            Self::Groq(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for PipelineError {}

/// Milliseconds per stage. `transcribe` includes a retry and its wait; `request` is the attempt that worked.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stages {
    pub gate: f64,
    pub encode: f64,
    pub transcribe: f64,
    pub request: f64,
    pub clean: f64,
}

#[derive(Debug, Default)]
pub struct Outcome {
    /// Text to paste: empty when there was no speech, Whisper invented a phantom, or the request failed.
    pub text: String,
    /// What Whisper returned before cleanup. Personal data: never logged unless the user asks.
    pub raw: String,
    pub needs_llm: bool,
    pub phantom: bool,
    /// False when the voice gate dropped the clip (no request was made).
    pub sent: bool,
    pub audio_ms: u32,
    pub speech_ms: u32,
    /// Length of the trimmed clip that was encoded.
    pub sent_ms: u32,
    pub bytes: usize,
    pub retries: u8,
    pub stages: Stages,
    /// Clip in → text out.
    pub total_ms: f64,
    pub error: Option<PipelineError>,
}

/// Mono samples in [-1, 1] at [`RATE`].
pub fn run(samples: &[f32], engine: &impl Transcribe, normalizer: &Normalizer) -> Outcome {
    let started = Instant::now();
    let mut out = Outcome {
        audio_ms: ms_of(samples.len()),
        ..Outcome::default()
    };

    let t = Instant::now();
    let detection = vad::detect(samples);
    out.stages.gate = elapsed(t);
    out.speech_ms = detection.speech_ms;
    if !detection.has_speech() {
        out.total_ms = elapsed(started);
        return out;
    }
    let clip = &samples[detection.keep];
    out.sent_ms = ms_of(clip.len());

    let t = Instant::now();
    let encoded = encode::opus(clip);
    out.stages.encode = elapsed(t);
    let encoded = match encoded {
        Ok(e) => e,
        Err(e) => {
            out.error = Some(PipelineError::Encode(e));
            out.total_ms = elapsed(started);
            return out;
        }
    };
    out.bytes = encoded.bytes.len();
    out.sent = true;

    let t = Instant::now();
    let transcript = engine.transcribe(&encoded);
    out.stages.transcribe = elapsed(t);
    match transcript {
        Ok(tr) => {
            out.stages.request = f64::from(tr.request_ms);
            out.retries = tr.retries;
            out.raw = tr.text;
        }
        Err(e) => {
            out.error = Some(PipelineError::Groq(e));
            out.total_ms = elapsed(started);
            return out;
        }
    }

    let t = Instant::now();
    let cleaned = normalizer.clean(&out.raw);
    out.stages.clean = elapsed(t);
    out.text = cleaned.text;
    out.needs_llm = cleaned.needs_llm;
    out.phantom = cleaned.phantom;
    out.total_ms = elapsed(started);
    out
}

fn ms_of(samples: usize) -> u32 {
    u32::try_from(samples * 1000 / RATE).unwrap_or(u32::MAX)
}

fn elapsed(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Fake {
        reply: Result<&'static str, GroqError>,
        calls: Cell<u32>,
    }

    impl Fake {
        fn new(reply: Result<&'static str, GroqError>) -> Self {
            Self {
                reply,
                calls: Cell::new(0),
            }
        }
    }

    impl Transcribe for Fake {
        fn transcribe(&self, audio: &Encoded) -> Result<Transcript, GroqError> {
            self.calls.set(self.calls.get() + 1);
            assert_eq!(&audio.bytes[..4], b"OggS");
            self.reply.clone().map(|text| Transcript {
                text: text.into(),
                request_ms: 7,
                retries: 0,
            })
        }
    }

    /// 1 s of quiet room noise, 1 s of a voiced 150 Hz tone, 1 s of quiet.
    fn speechy() -> Vec<f32> {
        let mut x = 1u32;
        let mut noise = || {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((x >> 9) as f32 / (1u32 << 23) as f32 - 0.5) * 0.002
        };
        (0..3 * RATE)
            .map(|i| {
                let voiced = (RATE..2 * RATE).contains(&i);
                let tone = (std::f32::consts::TAU * 150.0 * i as f32 / RATE as f32).sin() * 0.2;
                noise() + if voiced { tone } else { 0.0 }
            })
            .collect()
    }

    #[test]
    fn speech_is_trimmed_sent_and_cleaned() {
        let fake = Fake::new(Ok("Yaar  PR merge kar do"));
        let out = run(&speechy(), &fake, &Normalizer::new());
        assert!(out.error.is_none(), "{:?}", out.error);
        assert!(out.sent);
        assert_eq!(fake.calls.get(), 1);
        assert_eq!(out.text, "Yaar PR merge kar do ");
        assert_eq!(out.audio_ms, 3000);
        assert!(
            (1300..=1500).contains(&out.sent_ms),
            "trimmed to speech + pad: {}",
            out.sent_ms
        );
        assert!(out.bytes > 0 && out.stages.request == 7.0);
    }

    #[test]
    fn silence_makes_no_request() {
        let fake = Fake::new(Ok("Theek hai, main abhi meeting mein hoon."));
        let out = run(&vec![0.0; 2 * RATE], &fake, &Normalizer::new());
        assert!(!out.sent);
        assert_eq!(fake.calls.get(), 0);
        assert_eq!(out.text, "");
    }

    #[test]
    fn phantom_output_pastes_nothing() {
        let fake = Fake::new(Ok("Theek hai, main abhi meeting mein hoon."));
        let out = run(&speechy(), &fake, &Normalizer::new());
        assert!(out.phantom);
        assert_eq!(out.text, "");
    }

    #[test]
    fn groq_errors_are_returned_with_timings() {
        let fake = Fake::new(Err(GroqError::BadKey));
        let out = run(&speechy(), &fake, &Normalizer::new());
        assert!(matches!(
            out.error,
            Some(PipelineError::Groq(GroqError::BadKey))
        ));
        assert_eq!(out.text, "");
        assert!(out.total_ms > 0.0);
    }
}
