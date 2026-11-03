//! Gemini client: one `generateContent` call with the Ogg/Opus clip inline. With an instruction
//! (`gemini-audio`: the Roman Urdu prompt, as `eval` `GeminiAudio`) the answer is the Roman text; without one
//! (`gemini-transcribe`, Google's speech-to-text model) it is a transcript in the speaker's own script, for
//! a `refine` step to romanize. Timeout and retry rules: [`crate::api`].

use base64::Engine as _;
use reqwest::blocking::Client;
use serde_json::{Value, json};

use crate::api::{self, ApiError, Transcript};
use crate::encode::Encoded;
use crate::spec::GeminiEngine;

/// What the prompt asks for when the audio has no speech (D-007); it becomes empty text.
const NO_SPEECH: &str = "[no speech]";

pub struct Gemini {
    http: Client,
    engine: GeminiEngine,
    key: String,
}

impl Gemini {
    /// Never logs or prints the key.
    pub fn new(engine: GeminiEngine, key: String) -> Result<Self, ApiError> {
        let key = api::check_key(&key, &engine.provider.key_env)?;
        Ok(Self {
            http: api::client()?,
            engine,
            key,
        })
    }

    pub fn engine(&self) -> &GeminiEngine {
        &self.engine
    }

    /// Opens the TLS connection; errors are ignored (the real request reports them).
    pub fn warm(&self) {
        let _ = self
            .http
            .get(format!(
                "{}/models/{}",
                self.engine.provider.base_url, self.engine.model
            ))
            .header("x-goog-api-key", &self.key)
            .send();
    }

    pub fn transcribe(&self, audio: &Encoded) -> Result<Transcript, ApiError> {
        let e = &self.engine;
        let mut parts = Vec::new();
        if let Some(prompt) = &e.prompt {
            parts.push(json!({ "text": prompt }));
        }
        parts.push(json!({ "inline_data": {
            "mime_type": audio.mime,
            "data": base64::engine::general_purpose::STANDARD.encode(&audio.bytes),
        }}));
        let mut body = json!({ "contents": [{ "role": "user", "parts": parts }] });
        if let Some(config) = &e.generation_config {
            body["generationConfig"] = config.clone();
        }
        let url = format!("{}/models/{}:generateContent", e.provider.base_url, e.model);
        let build = || {
            Ok(self
                .http
                .post(&url)
                .header("x-goog-api-key", &self.key)
                .json(&body))
        };
        let (reply, request_ms, retries) = api::send(build, &e.provider.key_env)?;
        let usage = &reply["usageMetadata"];
        Ok(Transcript {
            text: text_of(&reply)?,
            request_ms,
            retries,
            tokens_in: usage["promptTokenCount"].as_u64().unwrap_or(0) as u32,
            tokens_out: usage["candidatesTokenCount"].as_u64().unwrap_or(0) as u32,
            ..Transcript::default()
        })
    }
}

/// The answer's text: `text` parts that are not thoughts, or the speech-to-text model's
/// `audioTranscription`, cleaned as `eval` `clean_output` does.
fn text_of(reply: &Value) -> Result<String, ApiError> {
    let Some(candidate) = reply["candidates"].get(0) else {
        let why = reply["promptFeedback"].to_string();
        return Err(ApiError::BadResponse(format!("no candidates ({why})")));
    };
    let text: Vec<&str> = candidate["content"]["parts"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|p| p["thought"] != Value::Bool(true))
        .filter_map(|p| {
            p["text"]
                .as_str()
                .or_else(|| p["audioTranscription"]["text"].as_str())
        })
        .collect();
    Ok(clean(&text.join(" ")))
}

/// Trims, drops wrapping quotes or a code fence, collapses spaces and turns the no-speech marker into "".
fn clean(text: &str) -> String {
    let mut t = text.trim();
    if t.trim_end_matches(['.', ' '])
        .eq_ignore_ascii_case(NO_SPEECH)
    {
        return String::new();
    }
    if t.len() >= 6 && t.starts_with("```") && t.ends_with("```") {
        t = t.trim_matches('`').trim();
    }
    for q in ['"', '\''] {
        if t.len() >= 2 && t.starts_with(q) && t.ends_with(q) {
            t = t[1..t.len() - 1].trim();
        }
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_server::server;
    use crate::spec::{self, Engine};

    fn client(id: &str, url: &str) -> Gemini {
        let Some(Engine::Gemini(mut engine)) = spec::engine(id) else {
            panic!("{id} is a Gemini engine");
        };
        engine.provider.base_url = url.to_string();
        Gemini::new(engine, "g-key".into()).unwrap()
    }

    fn clip() -> Encoded {
        crate::encode::opus(&vec![0.1; 16_000]).unwrap()
    }

    #[test]
    fn sends_prompt_and_audio_and_skips_thoughts() {
        let (url, requests) = server(vec![(
            200,
            "",
            r#"{"candidates":[{"content":{"parts":[{"text":"hmm","thought":true},{"text":" PR merge kar do. "}]}}],
                "usageMetadata":{"promptTokenCount":120,"candidatesTokenCount":9}}"#,
        )]);
        let t = client("gemini-3.5-flash-lite", &url)
            .transcribe(&clip())
            .unwrap();
        assert_eq!(t.text, "PR merge kar do.");
        assert_eq!((t.tokens_in, t.tokens_out), (120, 9));
        let req = requests.recv().unwrap();
        assert!(req.starts_with("POST /models/gemini-3.5-flash-lite:generateContent"));
        assert!(req.contains("x-goog-api-key: g-key"));
        let body: Value = serde_json::from_str(&req[req.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        let parts = body["contents"][0]["parts"].as_array().unwrap();
        assert!(parts[0]["text"].as_str().unwrap().contains("Roman"));
        assert_eq!(parts[1]["inline_data"]["mime_type"], "audio/ogg");
        assert!(body["generationConfig"]["thinkingConfig"].is_object());
    }

    #[test]
    fn transcribe_model_gets_audio_only_and_its_transcription_is_read() {
        let (url, requests) = server(vec![(
            200,
            "",
            r#"{"candidates":[{"content":{"parts":[{"audioTranscription":{"text":"इस फंक्शन को रिफैक्टर करो"}}]}}]}"#,
        )]);
        let t = client("gemini-3.5-transcribe+gptoss20b", &url)
            .transcribe(&clip())
            .unwrap();
        assert_eq!(t.text, "इस फंक्शन को रिफैक्टर करो");
        let req = requests.recv().unwrap();
        let body: Value = serde_json::from_str(&req[req.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body["contents"][0]["parts"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn no_speech_marker_and_wrapping_become_clean_text() {
        assert_eq!(clean("[no speech]"), "");
        assert_eq!(clean(" [No speech]. "), "");
        assert_eq!(clean("\"Theek  hai.\""), "Theek hai.");
        assert_eq!(clean("```\nAcha\n```"), "Acha");
    }

    #[test]
    fn overload_is_retried_once() {
        let (url, _) = server(vec![
            (503, "", r#"{"error":{"message":"high demand"}}"#),
            (
                200,
                "",
                r#"{"candidates":[{"content":{"parts":[{"text":"ok"}]}}]}"#,
            ),
        ]);
        let t = client("gemini-3.5-flash-lite", &url)
            .transcribe(&clip())
            .unwrap();
        assert_eq!((t.text.as_str(), t.retries), ("ok", 1));
    }
}
