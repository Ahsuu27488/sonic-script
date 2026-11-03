//! Groq Whisper client (M1 plan step 3), also used for OpenAI's `/audio/transcriptions` (same API): one HTTP
//! client per app, warmed when the dictation key goes down so the TLS handshake is done by the time the clip
//! is ready. The request matches `eval` `GroqWhisper` (model, temperature 0, json, hint as the prompt, no
//! language unless the engine sets one). Timeout and retry rules: [`crate::api`].

use reqwest::blocking::{Client, multipart};

use crate::api::{self, ApiError, Transcript};
use crate::encode::Encoded;
use crate::spec::WhisperEngine;

pub struct Groq {
    http: Client,
    engine: WhisperEngine,
    key: String,
}

impl Groq {
    /// Never logs or prints the key.
    pub fn new(engine: WhisperEngine, key: String) -> Result<Self, ApiError> {
        let key = api::check_key(&key, &engine.provider.key_env)?;
        Ok(Self {
            http: api::client()?,
            engine,
            key,
        })
    }

    pub fn engine(&self) -> &WhisperEngine {
        &self.engine
    }

    /// Opens the TLS connection so the next request skips the handshake. Errors are ignored: the real
    /// request reports them. Blocking; call it from a worker thread when the dictation key goes down.
    pub fn warm(&self) {
        let _ = self
            .http
            .get(format!("{}/models", self.engine.provider.base_url))
            .bearer_auth(&self.key)
            .send();
    }

    pub fn transcribe(&self, audio: &Encoded) -> Result<Transcript, ApiError> {
        let e = &self.engine;
        let build = || {
            let file = multipart::Part::bytes(audio.bytes.clone())
                .file_name(format!("audio.{}", audio.ext))
                .mime_str(audio.mime)
                .map_err(|err| ApiError::Network(err.to_string()))?;
            let mut form = multipart::Form::new()
                .text("model", e.model.clone())
                .text("temperature", "0")
                .text("response_format", "json")
                .part("file", file);
            if let Some(lang) = &e.language {
                form = form.text("language", lang.clone());
            }
            if let Some(hint) = e.hint {
                form = form.text("prompt", hint);
            }
            Ok(self
                .http
                .post(format!("{}/audio/transcriptions", e.provider.base_url))
                .bearer_auth(&self.key)
                .multipart(form))
        };
        let (body, request_ms, retries) = api::send(build, &e.provider.key_env)?;
        match body.get("text").and_then(|t| t.as_str()) {
            Some(text) => Ok(Transcript {
                text: text.trim().to_string(),
                request_ms,
                retries,
                ..Transcript::default()
            }),
            None => Err(ApiError::BadResponse("no \"text\" field".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_server::server;
    use crate::spec;
    use std::time::{Duration, Instant};

    fn client(url: &str) -> Groq {
        let mut engine = spec::whisper_engine("groq-whisper-hint").unwrap();
        engine.provider.base_url = url.to_string();
        Groq::new(engine, "test-key".into()).unwrap()
    }

    fn clip() -> Encoded {
        crate::encode::opus(&vec![0.1; 16_000]).unwrap()
    }

    #[test]
    fn sends_the_m0_request_and_returns_trimmed_text() {
        let (url, requests) = server(vec![(200, "", r#"{"text":" Theek hai. "}"#)]);
        let t = client(&url).transcribe(&clip()).unwrap();
        assert_eq!(t.text, "Theek hai.");
        assert_eq!(t.retries, 0);
        let req = requests.recv().unwrap();
        assert!(req.starts_with("POST /audio/transcriptions"));
        assert!(req.contains("Bearer test-key"));
        assert!(req.contains("whisper-large-v3-turbo"));
        assert!(req.contains("name=\"prompt\"") && req.contains("main abhi meeting mein hoon."));
        assert!(req.contains("filename=\"audio.ogg\"") && req.contains("audio/ogg"));
        assert!(
            !req.contains("name=\"language\""),
            "D-011: never forced to ur"
        );
    }

    #[test]
    fn retries_once_after_a_server_error() {
        let (url, _) = server(vec![
            (503, "", r#"{"error":{"message":"over capacity"}}"#),
            (200, "", r#"{"text":"ok"}"#),
        ]);
        let t = client(&url).transcribe(&clip()).unwrap();
        assert_eq!((t.text.as_str(), t.retries), ("ok", 1));
    }

    #[test]
    fn gives_up_after_the_second_failure_with_groqs_message() {
        let (url, _) = server(vec![
            (500, "", r#"{"error":{"message":"boom"}}"#),
            (502, "", r#"{"error":{"message":"bad gateway"}}"#),
        ]);
        let err = client(&url).transcribe(&clip()).unwrap_err();
        assert_eq!(err, ApiError::Http(502, "bad gateway".into()));
    }

    #[test]
    fn long_rate_limit_waits_are_reported_not_slept() {
        let (url, _) = server(vec![(429, "Retry-After: 30\r\n", "{}")]);
        let started = Instant::now();
        let err = client(&url).transcribe(&clip()).unwrap_err();
        assert_eq!(
            err,
            ApiError::RateLimited {
                retry_after_s: Some(30)
            }
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn short_rate_limit_is_retried() {
        let (url, _) = server(vec![
            (429, "Retry-After: 1\r\n", "{}"),
            (200, "", r#"{"text":"ok"}"#),
        ]);
        assert_eq!(client(&url).transcribe(&clip()).unwrap().retries, 1);
    }

    #[test]
    fn bad_key_is_not_retried() {
        let (url, _) = server(vec![(
            401,
            "",
            r#"{"error":{"message":"Invalid API Key"}}"#,
        )]);
        assert_eq!(
            client(&url).transcribe(&clip()).unwrap_err(),
            ApiError::BadKey("GROQ_API_KEY".into())
        );
    }

    #[test]
    fn empty_key_is_refused_before_any_request() {
        let err = Groq::new(
            spec::whisper_engine("groq-whisper-hint").unwrap(),
            "  ".into(),
        )
        .err()
        .unwrap();
        assert_eq!(err, ApiError::MissingKey("GROQ_API_KEY".into()));
    }
}
