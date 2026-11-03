//! One engine from the spec, ready to use: its speech call (Groq / OpenAI transcription or Gemini) and, when
//! the spec gives one, the `refine` step that turns the transcript into Roman Urdu / Hinglish on another
//! provider (an OpenAI-style chat call, as `eval` `Refiner`).

use reqwest::blocking::Client as Http;
use serde_json::{Value, json};

use crate::api::{self, ApiError, Transcript};
use crate::encode::Encoded;
use crate::gemini::Gemini;
use crate::groq::Groq;
use crate::pipeline::Transcribe;
use crate::spec::{Engine, Refine};

enum Speech {
    Groq(Groq),
    Gemini(Gemini),
}

pub struct Client {
    engine: Engine,
    speech: Speech,
    refiner: Option<Refiner>,
}

impl Client {
    /// `key_for` returns the key held in the named variable (the app reads the environment).
    pub fn new(engine: Engine, key_for: impl Fn(&str) -> String) -> Result<Self, ApiError> {
        let refiner = match engine.refine() {
            Some(r) => Some(Refiner::new(r.clone(), key_for(&r.provider.key_env))?),
            None => None,
        };
        let speech = match &engine {
            Engine::Whisper(e) => Speech::Groq(Groq::new(e.clone(), key_for(&e.provider.key_env))?),
            Engine::Gemini(e) => {
                Speech::Gemini(Gemini::new(e.clone(), key_for(&e.provider.key_env))?)
            }
        };
        Ok(Self {
            engine,
            speech,
            refiner,
        })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Opens the TLS connections the next request needs. Blocking; run it on a worker thread.
    pub fn warm(&self) {
        match &self.speech {
            Speech::Groq(g) => g.warm(),
            Speech::Gemini(g) => g.warm(),
        }
        if let Some(r) = &self.refiner {
            r.warm();
        }
    }
}

impl Transcribe for Client {
    fn transcribe(&self, audio: &Encoded) -> Result<Transcript, ApiError> {
        let heard = match &self.speech {
            Speech::Groq(g) => g.transcribe(audio)?,
            Speech::Gemini(g) => g.transcribe(audio)?,
        };
        match &self.refiner {
            Some(r) if !heard.text.trim().is_empty() => {
                let refined = r.run(&heard.text)?;
                Ok(Transcript {
                    asr: Some(heard.text),
                    text: refined.text,
                    refine_ms: refined.request_ms,
                    retries: heard.retries + refined.retries,
                    refine_tokens_in: refined.tokens_in,
                    refine_tokens_out: refined.tokens_out,
                    ..heard
                })
            }
            _ => Ok(heard),
        }
    }
}

struct Refiner {
    http: Http,
    spec: Refine,
    key: String,
}

impl Refiner {
    fn new(spec: Refine, key: String) -> Result<Self, ApiError> {
        let key = api::check_key(&key, &spec.provider.key_env)?;
        Ok(Self {
            http: api::client()?,
            spec,
            key,
        })
    }

    fn warm(&self) {
        let _ = self
            .http
            .get(format!("{}/models", self.spec.provider.base_url))
            .bearer_auth(&self.key)
            .send();
    }

    fn run(&self, text: &str) -> Result<Transcript, ApiError> {
        let s = &self.spec;
        let mut body = json!({
            "model": s.model,
            "temperature": 0,
            "messages": [
                { "role": "system", "content": s.prompt },
                { "role": "user", "content": text },
            ],
        });
        for (k, v) in &s.extra {
            body[k] = v.clone();
        }
        let url = format!("{}/chat/completions", s.provider.base_url);
        let build = || Ok(self.http.post(&url).bearer_auth(&self.key).json(&body));
        let (reply, request_ms, retries) = api::send(build, &s.provider.key_env)?;
        let Some(out) = reply["choices"][0]["message"]["content"].as_str() else {
            return Err(ApiError::BadResponse("no message content".into()));
        };
        let usage = &reply["usage"];
        Ok(Transcript {
            text: out.trim().to_string(),
            request_ms,
            retries,
            tokens_in: count(&usage["prompt_tokens"]),
            tokens_out: count(&usage["completion_tokens"]),
            ..Transcript::default()
        })
    }
}

fn count(v: &Value) -> u32 {
    v.as_u64().unwrap_or(0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_server::server;
    use crate::spec;

    #[test]
    fn transcript_is_refined_on_the_other_provider() {
        let (gemini_url, _) = server(vec![(
            200,
            "",
            r#"{"candidates":[{"content":{"parts":[{"audioTranscription":{"text":"पीआर मर्ज कर दो"}}]}}],
                "usageMetadata":{"promptTokenCount":87,"candidatesTokenCount":14}}"#,
        )]);
        let (groq_url, requests) = server(vec![(
            200,
            "",
            r#"{"choices":[{"message":{"content":" PR merge kar do. "}}],"usage":{"prompt_tokens":300,"completion_tokens":10}}"#,
        )]);
        let Some(Engine::Gemini(mut e)) = spec::engine("gemini-3.5-transcribe+gptoss20b") else {
            panic!("transcribe engine");
        };
        e.provider.base_url = gemini_url;
        e.refine.as_mut().unwrap().provider.base_url = groq_url;
        let client = Client::new(Engine::Gemini(e), |var| format!("{var}-value")).unwrap();
        let audio = crate::encode::opus(&vec![0.1; 16_000]).unwrap();
        let t = client.transcribe(&audio).unwrap();
        assert_eq!(t.text, "PR merge kar do.");
        assert_eq!(t.asr.as_deref(), Some("पीआर मर्ज कर दो"));
        assert_eq!((t.tokens_in, t.refine_tokens_in), (87, 300));
        let req = requests.recv().unwrap();
        assert!(req.contains("Bearer GROQ_API_KEY-value"));
        assert!(req.contains("\"reasoning_effort\":\"low\"") && req.contains("पीआर मर्ज कर दो"));
    }

    #[test]
    fn a_missing_refine_key_is_reported_up_front() {
        let engine = spec::engine("gemini-3.5-transcribe+gptoss20b").unwrap();
        let err = Client::new(engine, |var| {
            if var == "GROQ_API_KEY" {
                String::new()
            } else {
                "k".into()
            }
        })
        .err()
        .unwrap();
        assert_eq!(err, ApiError::MissingKey("GROQ_API_KEY".into()));
    }
}
