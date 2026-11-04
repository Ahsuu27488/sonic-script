//! The shared spec, read at build time (M1 plan step 3): engines and providers from `spec/providers.json`,
//! prompts from `spec/prompts/`. Model ids and the default engine live there, never in code. The app runs
//! three kinds: an OpenAI-style transcription call (`whisper-only`: Groq Whisper, OpenAI), Gemini with an
//! instruction (`gemini-audio`) and Gemini's speech-to-text model (`gemini-transcribe`), each optionally
//! followed by a text-only `refine` step on another provider.

use std::time::Duration;

use serde::Deserialize;

const PROVIDERS: &str = include_str!("../../../spec/providers.json");
/// Prompt files an engine may name, by file name.
const PROMPTS: &[(&str, &str)] = &[
    (
        "whisper-hint-v1.txt",
        include_str!("../../../spec/prompts/whisper-hint-v1.txt"),
    ),
    (
        "whisper-hint-words-v1.txt",
        include_str!("../../../spec/prompts/whisper-hint-words-v1.txt"),
    ),
    (
        "transcribe-roman-v1.md",
        include_str!("../../../spec/prompts/transcribe-roman-v1.md"),
    ),
    (
        "romanize-v1.md",
        include_str!("../../../spec/prompts/romanize-v1.md"),
    ),
    (
        "format-v1.md",
        include_str!("../../../spec/prompts/format-v1.md"),
    ),
];

#[derive(Debug, Clone, Deserialize)]
pub struct Provider {
    pub base_url: String,
    pub key_env: String,
    /// Requests per minute we allow ourselves on the free tier (batch runs only).
    pub rpm: u32,
    /// The provider bills at least this many seconds per request (Groq: 10).
    #[serde(default)]
    pub min_billed_s: f64,
}

/// An OpenAI-style `/audio/transcriptions` engine (`kind: "whisper-only"`): Groq Whisper, OpenAI.
#[derive(Debug, Clone)]
pub struct WhisperEngine {
    pub id: String,
    pub provider: Provider,
    pub model: String,
    /// `None` lets Whisper detect the language (D-011: never forced to `ur`).
    pub language: Option<String>,
    /// The Whisper prompt text, already loaded.
    pub hint: Option<&'static str>,
    /// US dollars per hour of audio sent, from the spec (for the bench cost column).
    pub audio_per_hour: f64,
    pub rpm: Option<u32>,
    pub refine: Option<Refine>,
}

/// A Gemini `generateContent` engine with audio.
#[derive(Debug, Clone)]
pub struct GeminiEngine {
    pub id: String,
    pub provider: Provider,
    pub model: String,
    /// The instruction sent with the audio (`gemini-audio`); `None` for the speech-to-text model.
    pub prompt: Option<String>,
    pub generation_config: Option<serde_json::Value>,
    /// US dollars per million tokens, from the spec.
    pub in_per_m: f64,
    pub out_per_m: f64,
    pub rpm: Option<u32>,
    pub refine: Option<Refine>,
}

/// A text-only second step: an OpenAI-style chat model turns the transcript into clean Roman text.
#[derive(Debug, Clone)]
pub struct Refine {
    pub provider: Provider,
    pub model: String,
    pub prompt: String,
    /// Extra fields for the chat request (e.g. `reasoning_effort`).
    pub extra: serde_json::Map<String, serde_json::Value>,
    pub in_per_m: f64,
    pub out_per_m: f64,
}

#[derive(Debug, Clone)]
pub enum Engine {
    Whisper(WhisperEngine),
    Gemini(GeminiEngine),
}

impl Engine {
    pub fn id(&self) -> &str {
        match self {
            Self::Whisper(e) => &e.id,
            Self::Gemini(e) => &e.id,
        }
    }

    pub fn provider(&self) -> &Provider {
        match self {
            Self::Whisper(e) => &e.provider,
            Self::Gemini(e) => &e.provider,
        }
    }

    /// Requests per minute the bench allows itself: the engine's own `rpm` if the spec gives one, else the
    /// provider's.
    pub fn rpm(&self) -> u32 {
        match self {
            Self::Whisper(e) => e.rpm,
            Self::Gemini(e) => e.rpm,
        }
        .unwrap_or(self.provider().rpm)
    }

    pub fn refine(&self) -> Option<&Refine> {
        match self {
            Self::Whisper(e) => e.refine.as_ref(),
            Self::Gemini(e) => e.refine.as_ref(),
        }
    }
}

#[derive(Deserialize)]
struct File {
    chain: Vec<RawLink>,
    format: Option<RawRefine>,
    providers: std::collections::HashMap<String, Provider>,
    engines: Vec<RawEngine>,
}

#[derive(Deserialize)]
struct RawLink {
    engine: String,
    #[serde(default)]
    wait_ms: u64,
    #[serde(default)]
    last_resort: bool,
}

#[derive(Deserialize)]
struct RawEngine {
    id: String,
    kind: String,
    provider: String,
    whisper_model: Option<String>,
    model: Option<String>,
    language: Option<String>,
    hint: Option<String>,
    prompt: Option<String>,
    generation_config: Option<serde_json::Value>,
    refine: Option<RawRefine>,
    rpm: Option<u32>,
    #[serde(default)]
    price: Price,
}

#[derive(Deserialize)]
struct RawRefine {
    provider: String,
    model: String,
    prompt: String,
    #[serde(default)]
    extra: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    price: Price,
}

#[derive(Deserialize, Default)]
struct Price {
    #[serde(default)]
    audio_per_hour: f64,
    #[serde(default)]
    audio_in_per_m: f64,
    #[serde(default)]
    in_per_m: f64,
    #[serde(default)]
    out_per_m: f64,
}

/// One step of the engine chain (D-029).
#[derive(Debug, Clone)]
pub struct Link {
    pub engine: Engine,
    /// How long to wait for this engine before also asking the next one.
    pub wait: Duration,
    /// Asked only once every engine asked before it has failed, never because they are slow (D-033).
    pub last_resort: bool,
}

/// The engines the apps use, in order (`chain` in `spec/providers.json`, D-029).
pub fn chain() -> Vec<Link> {
    parse()
        .chain
        .iter()
        .map(|l| Link {
            engine: engine(&l.engine).expect("every chain engine must be one the app can run"),
            wait: Duration::from_millis(l.wait_ms),
            last_resort: l.last_resort,
        })
        .collect()
}

/// The first engine of the chain.
pub fn default_engine() -> Engine {
    chain().remove(0).engine
}

/// An engine the app can run, by id; `None` when it does not exist or is a kind only `eval` runs.
pub fn engine(id: &str) -> Option<Engine> {
    let file = parse();
    let raw = file.engines.iter().find(|e| e.id == id)?;
    let provider = file.providers.get(&raw.provider)?.clone();
    let refine = match &raw.refine {
        Some(r) => Some(refine(&file, r)?),
        None => None,
    };
    match raw.kind.as_str() {
        "whisper-only" => Some(Engine::Whisper(WhisperEngine {
            id: raw.id.clone(),
            provider,
            model: raw.whisper_model.clone()?,
            language: raw.language.clone(),
            hint: match &raw.hint {
                Some(name) => Some(prompt(name)?),
                None => None,
            },
            audio_per_hour: raw.price.audio_per_hour,
            rpm: raw.rpm,
            refine,
        })),
        kind @ ("gemini-audio" | "gemini-transcribe") => Some(Engine::Gemini(GeminiEngine {
            id: raw.id.clone(),
            provider,
            model: raw.model.clone()?,
            prompt: match (kind, &raw.prompt) {
                ("gemini-audio", Some(name)) => Some(fill(prompt(name)?)),
                ("gemini-audio", None) => return None,
                _ => None,
            },
            generation_config: raw.generation_config.clone(),
            in_per_m: raw.price.audio_in_per_m,
            out_per_m: raw.price.out_per_m,
            rpm: raw.rpm,
            refine,
        })),
        _ => None,
    }
}

/// The smart-formatting step (D-030, `format` in the spec): run after whichever engine answers, in place of
/// its own `refine` step.
pub fn format() -> Option<Refine> {
    let file = parse();
    refine(&file, file.format.as_ref()?)
}

fn refine(file: &File, r: &RawRefine) -> Option<Refine> {
    Some(Refine {
        provider: file.providers.get(&r.provider)?.clone(),
        model: r.model.clone(),
        prompt: fill(prompt(&r.prompt)?),
        extra: r.extra.clone(),
        in_per_m: r.price.in_per_m,
        out_per_m: r.price.out_per_m,
    })
}

/// A `whisper-only` engine by id (tests, and code that needs the Whisper request itself).
pub fn whisper_engine(id: &str) -> Option<WhisperEngine> {
    match engine(id)? {
        Engine::Whisper(e) => Some(e),
        Engine::Gemini(_) => None,
    }
}

/// Ids of every engine the app can run, in spec order.
pub fn engine_ids() -> Vec<String> {
    parse()
        .engines
        .into_iter()
        .map(|e| e.id)
        .filter(|id| engine(id).is_some())
        .collect()
}

fn prompt(name: &str) -> Option<&'static str> {
    PROMPTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| text.trim())
}

/// Same as `eval` `load_prompt` without a word list: the `{word_list}` slot is left empty.
fn fill(prompt: &str) -> String {
    prompt.replace("{word_list}", "").trim().to_string()
}

fn parse() -> File {
    serde_json::from_str(PROVIDERS).expect("spec/providers.json is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groq_whisper_hint_is_the_m0_request() {
        let e = whisper_engine("groq-whisper-hint").unwrap();
        assert_eq!(e.provider.key_env, "GROQ_API_KEY");
        assert!(e.provider.base_url.starts_with("https://"));
        assert_eq!(e.provider.min_billed_s, 10.0);
        assert_eq!(e.language, None, "D-011: never forced to ur");
        assert!(e.hint.unwrap().ends_with("main abhi meeting mein hoon."));
        assert!(e.audio_per_hour > 0.0);
        assert!(e.refine.is_none());
    }

    #[test]
    fn the_chain_is_gemini_first_and_groq_last() {
        // D-029, D-033: the most accurate free engines first; Groq only when every Gemini model failed.
        let chain = chain();
        let ids: Vec<&str> = chain.iter().map(|l| l.engine.id()).collect();
        assert_eq!(
            ids,
            [
                "gemini-3.5-transcribe+gptoss20b",
                "gemini-3.5-flash",
                "gemini-3.5-flash-lite",
                "groq-whisper-hint"
            ]
        );
        assert_eq!(default_engine().id(), ids[0]);
        assert_eq!(
            chain[3].engine.provider().key_env,
            "GROQ_API_KEY",
            "another provider last, so Google's bad hour does not take every engine"
        );
        assert!(chain[..2].iter().all(|l| l.wait >= Duration::from_secs(2)));
        let last_resort: Vec<bool> = chain.iter().map(|l| l.last_resort).collect();
        assert_eq!(last_resort, [false, false, false, true]);
    }

    #[test]
    fn the_format_step_is_a_groq_chat_call_with_the_format_prompt() {
        // D-030: one formatter after every engine, on Groq.
        let f = format().expect("spec/providers.json has a format step");
        assert_eq!(f.provider.key_env, "GROQ_API_KEY");
        assert!(f.model.starts_with("openai/gpt-oss-"));
        assert!(f.prompt.starts_with("You turn a raw dictation transcript"));
        assert!(f.prompt.contains("1. Charger lagao") && !f.prompt.contains("{word_list}"));
        assert_eq!(f.extra["reasoning_effort"], "low");
    }

    #[test]
    fn gemini_engines_carry_their_prompt_or_none() {
        let Some(Engine::Gemini(e)) = engine("gemini-3.5-flash-lite") else {
            panic!("gemini-3.5-flash-lite is a Gemini engine");
        };
        let p = e.prompt.unwrap();
        assert!(p.starts_with("You are a dictation engine") && !p.contains("{word_list}"));
        let Some(Engine::Gemini(t)) = engine("gemini-3.5-transcribe+gptoss20b") else {
            panic!("the transcribe engine is a Gemini engine");
        };
        assert!(
            t.prompt.is_none(),
            "the speech-to-text model takes no instruction"
        );
        assert_eq!(
            Engine::Gemini(t.clone()).rpm(),
            3,
            "its own limit, not Gemini's 10"
        );
        let r = t.refine.unwrap();
        assert_eq!(r.provider.key_env, "GROQ_API_KEY");
        assert!(r.prompt.contains("Roman") && r.extra.contains_key("reasoning_effort"));
    }

    #[test]
    fn eval_only_kinds_are_not_offered() {
        assert!(engine("groq-whisper+gptoss20b").is_none(), "whisper-llm");
        assert!(engine("no-such-engine").is_none());
    }

    #[test]
    fn every_prompt_named_in_the_spec_is_embedded() {
        for e in parse().engines.iter().filter(|e| e.kind != "whisper-llm") {
            assert!(
                engine(&e.id).is_some(),
                "{} names a prompt not in PROMPTS",
                e.id
            );
        }
    }
}
