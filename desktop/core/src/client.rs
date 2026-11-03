//! One engine from the spec, ready to use: its speech call (Groq / OpenAI transcription or Gemini) and, when
//! the spec gives one, the `refine` step that turns the transcript into Roman Urdu / Hinglish on another
//! provider (an OpenAI-style chat call, as `eval` `Refiner`). [`Racer`] puts a fallback engine behind it for
//! slow or failed requests (D-028).

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use reqwest::blocking::Client as Http;
use serde_json::{Value, json};

use crate::api::{self, ApiError, Fallback, Transcript};
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
}

impl Transcribe for Client {
    /// Opens the TLS connections the next request needs. Blocking; run it on a worker thread.
    fn warm(&self) {
        match &self.speech {
            Speech::Groq(g) => g.warm(),
            Speech::Gemini(g) => g.warm(),
        }
        if let Some(r) = &self.refiner {
            r.warm();
        }
    }

    fn transcribe(&self, audio: &Encoded) -> Result<Transcript, ApiError> {
        let heard = match &self.speech {
            Speech::Groq(g) => g.transcribe(audio)?,
            Speech::Gemini(g) => g.transcribe(audio)?,
        };
        let heard = Transcript {
            engine: self.engine.id().to_string(),
            ..heard
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

/// How long the primary engine gets before the fallback is asked too (D-028: a fixed 3 s; D-017's learned
/// deadline comes later).
pub const FALLBACK_AFTER: Duration = Duration::from_secs(3);

/// The primary engine with an optional fallback (D-028). The clip goes to the primary; if it fails, or has not
/// answered by the deadline, it also goes to the fallback, and the first good answer wins. A request that
/// loses keeps running on its own thread until it ends (at most the 10 s client timeout) and is ignored.
pub struct Racer<T> {
    primary: Arc<T>,
    fallback: Option<Arc<T>>,
    deadline: Duration,
}

#[derive(Clone, Copy)]
enum Side {
    Primary,
    Fallback,
}

impl<T: Transcribe + Send + Sync + 'static> Racer<T> {
    pub fn new(primary: T, fallback: Option<T>, deadline: Duration) -> Self {
        Self {
            primary: Arc::new(primary),
            fallback: fallback.map(Arc::new),
            deadline,
        }
    }

    pub fn primary(&self) -> &T {
        &self.primary
    }

    pub fn fallback(&self) -> Option<&T> {
        self.fallback.as_deref()
    }

    fn ask(
        &self,
        who: Side,
        audio: &Arc<Encoded>,
        tx: mpsc::Sender<(Side, Result<Transcript, ApiError>)>,
    ) {
        let engine = match who {
            Side::Primary => self.primary.clone(),
            Side::Fallback => self.fallback.clone().expect("only asked when there is one"),
        };
        let audio = audio.clone();
        std::thread::spawn(move || {
            // The receiver is gone when the other engine already won: nothing to do then.
            let _ = tx.send((who, engine.transcribe(&audio)));
        });
    }
}

impl<T: Transcribe + Send + Sync + 'static> Transcribe for Racer<T> {
    /// Warms both engines at once (the fallback on its own thread).
    fn warm(&self) {
        if let Some(f) = &self.fallback {
            let f = f.clone();
            std::thread::spawn(move || f.warm());
        }
        self.primary.warm();
    }

    /// Both failing returns the primary's error (it says more about the engine the user chose).
    fn transcribe(&self, audio: &Encoded) -> Result<Transcript, ApiError> {
        if self.fallback.is_none() {
            return self.primary.transcribe(audio);
        }
        let audio = Arc::new(audio.clone());
        let (tx, rx) = mpsc::channel();
        self.ask(Side::Primary, &audio, tx.clone());
        let why = match rx.recv_timeout(self.deadline) {
            Ok((_, Ok(t))) => return Ok(t),
            Ok((_, Err(e))) => Fallback::Failed(e),
            Err(_) => Fallback::Slow,
        };
        let mut primary_error = match &why {
            Fallback::Failed(e) => Some(e.clone()),
            Fallback::Slow => None,
        };
        // `tx` moves into the fallback's thread: once both threads are done, `recv` ends.
        self.ask(Side::Fallback, &audio, tx);
        let mut fallback_error = None;
        while let Ok((who, result)) = rx.recv() {
            match (who, result) {
                (_, Ok(t)) => {
                    return Ok(Transcript {
                        fallback: Some(why),
                        ..t
                    });
                }
                (Side::Primary, Err(e)) => primary_error = Some(e),
                (Side::Fallback, Err(e)) => fallback_error = Some(e),
            }
            if primary_error.is_some() && fallback_error.is_some() {
                break;
            }
        }
        Err(primary_error
            .or(fallback_error)
            .unwrap_or_else(|| ApiError::Network("the engine thread stopped".into())))
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
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Instant;

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

    struct Fake {
        id: &'static str,
        after: Duration,
        reply: Result<&'static str, ApiError>,
        calls: Arc<AtomicU32>,
    }

    impl Fake {
        fn new(id: &'static str, after_ms: u64, reply: Result<&'static str, ApiError>) -> Self {
            Self {
                id,
                after: Duration::from_millis(after_ms),
                reply,
                calls: Arc::default(),
            }
        }
    }

    impl Transcribe for Fake {
        fn transcribe(&self, _: &Encoded) -> Result<Transcript, ApiError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.after);
            self.reply.clone().map(|text| Transcript {
                text: text.into(),
                engine: self.id.into(),
                ..Transcript::default()
            })
        }
    }

    const DEADLINE: Duration = Duration::from_millis(300);

    fn race(primary: Fake, fallback: Fake) -> (Result<Transcript, ApiError>, Duration, u32) {
        let calls = fallback.calls.clone();
        let racer = Racer::new(primary, Some(fallback), DEADLINE);
        let audio = crate::encode::opus(&vec![0.1; 1_600]).unwrap();
        let t = Instant::now();
        let result = racer.transcribe(&audio);
        (result, t.elapsed(), calls.load(Ordering::SeqCst))
    }

    #[test]
    fn a_slow_primary_loses_to_the_fallback_at_the_deadline() {
        let (r, took, asked) = race(
            Fake::new("gemini", 2_000, Ok("Gemini text")),
            Fake::new("groq", 50, Ok("Groq text")),
        );
        let t = r.unwrap();
        assert_eq!((t.text.as_str(), t.engine.as_str()), ("Groq text", "groq"));
        assert_eq!(t.fallback, Some(Fallback::Slow));
        assert_eq!(asked, 1);
        assert!(
            took >= DEADLINE && took < DEADLINE + Duration::from_millis(500),
            "deadline + Groq, not Gemini's 2 s: {took:?}"
        );
    }

    #[test]
    fn a_primary_error_asks_the_fallback_at_once() {
        let failed = ApiError::Http(503, "high demand".into());
        let (r, took, _) = race(
            Fake::new("gemini", 0, Err(failed.clone())),
            Fake::new("groq", 50, Ok("Groq text")),
        );
        let t = r.unwrap();
        assert_eq!(t.engine, "groq");
        assert_eq!(t.fallback, Some(Fallback::Failed(failed)));
        assert!(took < DEADLINE, "no wait for the deadline: {took:?}");
    }

    #[test]
    fn a_fast_primary_never_asks_the_fallback() {
        let (r, _, asked) = race(
            Fake::new("gemini", 20, Ok("Gemini text")),
            Fake::new("groq", 0, Ok("Groq text")),
        );
        let t = r.unwrap();
        assert_eq!((t.engine.as_str(), t.fallback.as_ref()), ("gemini", None));
        assert_eq!(asked, 0);
    }

    #[test]
    fn a_slow_primary_still_wins_when_the_fallback_fails() {
        let (r, _, asked) = race(
            Fake::new("gemini", 600, Ok("Gemini text")),
            Fake::new("groq", 0, Err(ApiError::Timeout)),
        );
        let t = r.unwrap();
        assert_eq!(t.engine, "gemini");
        assert_eq!(t.fallback, Some(Fallback::Slow), "it was asked, and lost");
        assert_eq!(asked, 1);
    }

    #[test]
    fn when_both_fail_the_primary_error_is_returned() {
        let primary_error = ApiError::RateLimited {
            retry_after_s: Some(30),
        };
        let (r, ..) = race(
            Fake::new("gemini", 0, Err(primary_error.clone())),
            Fake::new("groq", 0, Err(ApiError::BadKey("GROQ_API_KEY".into()))),
        );
        assert_eq!(r.unwrap_err(), primary_error);
        let (r, ..) = race(
            Fake::new("gemini", 400, Err(ApiError::Timeout)),
            Fake::new("groq", 0, Err(ApiError::BadKey("GROQ_API_KEY".into()))),
        );
        assert_eq!(r.unwrap_err(), ApiError::Timeout, "also when it fails late");
    }

    #[test]
    fn without_a_fallback_the_primary_answers_however_long_it_takes() {
        let racer = Racer::new(Fake::new("gemini", 400, Ok("Gemini text")), None, DEADLINE);
        let audio = crate::encode::opus(&vec![0.1; 1_600]).unwrap();
        let t = racer.transcribe(&audio).unwrap();
        assert_eq!((t.engine.as_str(), t.fallback), ("gemini", None));
    }

    #[test]
    fn the_client_names_its_engine_in_the_transcript() {
        let (url, _) = server(vec![(200, "", r#"{"text":"PR merge kar do"}"#)]);
        let Some(Engine::Whisper(mut e)) = spec::engine("groq-whisper-hint") else {
            panic!("groq-whisper-hint is a Whisper engine");
        };
        e.provider.base_url = url;
        let client = Client::new(Engine::Whisper(e), |_| "k".into()).unwrap();
        let audio = crate::encode::opus(&vec![0.1; 16_000]).unwrap();
        assert_eq!(
            client.transcribe(&audio).unwrap().engine,
            "groq-whisper-hint"
        );
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
