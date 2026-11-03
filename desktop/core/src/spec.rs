//! The shared spec, read at build time (M1 plan step 3): engines and providers from `spec/providers.json`,
//! Whisper hints from `spec/prompts/`. Model ids and the default engine live there, never in code.

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
];

#[derive(Debug, Clone, Deserialize)]
pub struct Provider {
    pub base_url: String,
    pub key_env: String,
    /// Requests per minute we allow ourselves on the free tier (batch runs only).
    pub rpm: u32,
}

/// A Whisper-only engine (`kind: "whisper-only"`), the only kind the M1 app runs.
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
}

#[derive(Deserialize)]
struct File {
    default_engine: String,
    providers: std::collections::HashMap<String, Provider>,
    engines: Vec<RawEngine>,
}

#[derive(Deserialize)]
struct RawEngine {
    id: String,
    kind: String,
    provider: String,
    whisper_model: Option<String>,
    language: Option<String>,
    hint: Option<String>,
    #[serde(default)]
    price: Price,
}

#[derive(Deserialize, Default)]
struct Price {
    #[serde(default)]
    audio_per_hour: f64,
}

/// The engine `spec/providers.json` names as `default_engine` (D-011).
pub fn default_engine() -> WhisperEngine {
    let file = parse();
    engine(&file.default_engine).expect("default_engine must be a whisper-only engine")
}

/// A whisper-only engine by id; `None` when it does not exist or is another kind.
pub fn engine(id: &str) -> Option<WhisperEngine> {
    let file = parse();
    let raw = file
        .engines
        .into_iter()
        .find(|e| e.id == id && e.kind == "whisper-only")?;
    Some(WhisperEngine {
        provider: file.providers.get(&raw.provider)?.clone(),
        model: raw.whisper_model?,
        language: raw.language,
        hint: match raw.hint {
            Some(name) => Some(prompt(&name)?),
            None => None,
        },
        audio_per_hour: raw.price.audio_per_hour,
        id: raw.id,
    })
}

fn prompt(name: &str) -> Option<&'static str> {
    PROMPTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| text.trim())
}

fn parse() -> File {
    serde_json::from_str(PROVIDERS).expect("spec/providers.json is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_groq_whisper_with_the_hint() {
        let e = default_engine();
        assert_eq!(e.id, "groq-whisper-hint");
        assert_eq!(e.provider.key_env, "GROQ_API_KEY");
        assert!(e.provider.base_url.starts_with("https://"));
        assert_eq!(e.language, None, "D-011: never forced to ur");
        assert!(e.hint.unwrap().ends_with("main abhi meeting mein hoon."));
        assert!(e.audio_per_hour > 0.0);
    }

    #[test]
    fn other_kinds_are_not_whisper_engines() {
        assert!(engine("groq-whisper+gptoss20b").is_none());
        assert!(engine("gemini-3.5-flash-lite").is_none());
        assert!(engine("no-such-engine").is_none());
    }

    #[test]
    fn every_hint_named_in_the_spec_is_embedded() {
        for e in parse().engines.iter().filter(|e| e.kind == "whisper-only") {
            assert!(
                engine(&e.id).is_some(),
                "{} names a hint not in PROMPTS",
                e.id
            );
        }
    }
}
