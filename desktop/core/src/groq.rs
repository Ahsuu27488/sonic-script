//! Groq Whisper client (M1 plan step 3): one HTTP client per app, warmed when the dictation key goes down
//! so the TLS handshake is done by the time the clip is ready; 10 s timeout; one retry on 429 / 5xx /
//! a dropped connection. The request matches `eval` `GroqWhisper` (model, temperature 0, json, hint as the
//! prompt, no language unless the engine sets one).

use std::time::{Duration, Instant};

use reqwest::StatusCode;
use reqwest::blocking::{Client, Response, multipart};

use crate::encode::Encoded;
use crate::spec::WhisperEngine;

pub const TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// A 429 asking us to wait longer than this is reported instead of retried: the user is waiting.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(2);
const SERVER_ERROR_WAIT: Duration = Duration::from_millis(300);

/// What Whisper heard (before the text cleanup).
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub text: String,
    /// Time of the request that succeeded, without earlier attempts or waits.
    pub request_ms: u32,
    pub retries: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GroqError {
    /// The environment variable that should hold the key is empty.
    MissingKey(String),
    /// 401 / 403: the key is wrong or revoked.
    BadKey,
    RateLimited {
        retry_after_s: Option<u64>,
    },
    /// Any other HTTP error, with Groq's own message.
    Http(u16, String),
    Timeout,
    Network(String),
    /// 200 but not the JSON we expect.
    BadResponse(String),
}

impl std::fmt::Display for GroqError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingKey(var) => write!(f, "no Groq key: set {var}"),
            Self::BadKey => write!(f, "Groq refused the key (check GROQ_API_KEY)"),
            Self::RateLimited {
                retry_after_s: Some(s),
            } => write!(f, "Groq rate limit reached: try again in {s} s"),
            Self::RateLimited {
                retry_after_s: None,
            } => write!(f, "Groq rate limit reached"),
            Self::Http(status, msg) => write!(f, "Groq error {status}: {msg}"),
            Self::Timeout => write!(f, "Groq did not answer within {} s", TIMEOUT.as_secs()),
            Self::Network(msg) => write!(f, "network error: {msg}"),
            Self::BadResponse(msg) => write!(f, "unexpected answer from Groq: {msg}"),
        }
    }
}

impl std::error::Error for GroqError {}

pub struct Groq {
    http: Client,
    engine: WhisperEngine,
    key: String,
}

impl Groq {
    /// Never logs or prints the key.
    pub fn new(engine: WhisperEngine, key: String) -> Result<Self, GroqError> {
        if key.trim().is_empty() {
            return Err(GroqError::MissingKey(engine.provider.key_env.clone()));
        }
        let http = Client::builder()
            .timeout(TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| GroqError::Network(e.to_string()))?;
        Ok(Self {
            http,
            engine,
            key: key.trim().to_string(),
        })
    }

    /// Reads the key from the engine's `key_env` variable in this process's environment.
    pub fn from_env(engine: WhisperEngine) -> Result<Self, GroqError> {
        let key = std::env::var(&engine.provider.key_env).unwrap_or_default();
        Self::new(engine, key)
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

    pub fn transcribe(&self, audio: &Encoded) -> Result<Transcript, GroqError> {
        match self.attempt(audio) {
            Ok((text, ms)) => Ok(Transcript {
                text,
                request_ms: ms,
                retries: 0,
            }),
            Err(Failure {
                wait: Some(wait), ..
            }) => {
                std::thread::sleep(wait);
                let (text, ms) = self.attempt(audio).map_err(|f| f.error)?;
                Ok(Transcript {
                    text,
                    request_ms: ms,
                    retries: 1,
                })
            }
            Err(Failure { error, wait: None }) => Err(error),
        }
    }

    fn attempt(&self, audio: &Encoded) -> Result<(String, u32), Failure> {
        let e = &self.engine;
        let file = multipart::Part::bytes(audio.bytes.clone())
            .file_name(format!("audio.{}", audio.ext))
            .mime_str(audio.mime)
            .map_err(|err| Failure::fatal(GroqError::Network(err.to_string())))?;
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
        let started = Instant::now();
        let resp = self
            .http
            .post(format!("{}/audio/transcriptions", e.provider.base_url))
            .bearer_auth(&self.key)
            .multipart(form)
            .send()
            .map_err(Failure::from_send)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(Failure::from_status(resp));
        }
        let body: serde_json::Value = resp.json().map_err(|err| {
            if err.is_timeout() {
                Failure::fatal(GroqError::Timeout)
            } else {
                Failure::fatal(GroqError::BadResponse(err.to_string()))
            }
        })?;
        let ms = started.elapsed().as_millis() as u32;
        match body.get("text").and_then(|t| t.as_str()) {
            Some(text) => Ok((text.trim().to_string(), ms)),
            None => Err(Failure::fatal(GroqError::BadResponse(
                "no \"text\" field".into(),
            ))),
        }
    }
}

/// A failed attempt; `wait` is set when it is worth one retry after that pause.
struct Failure {
    error: GroqError,
    wait: Option<Duration>,
}

impl Failure {
    fn fatal(error: GroqError) -> Self {
        Self { error, wait: None }
    }

    fn from_send(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            // Waiting another 10 s would be worse than reporting it.
            return Self::fatal(GroqError::Timeout);
        }
        // Most often a pooled connection the server already closed: retry at once on a fresh one.
        Self {
            error: GroqError::Network(err.to_string()),
            wait: Some(Duration::ZERO),
        }
    }

    fn from_status(resp: Response) -> Self {
        let status = resp.status();
        let retry_after_s = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|s| s.ceil() as u64);
        let message = error_message(resp);
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Self::fatal(GroqError::BadKey),
            StatusCode::TOO_MANY_REQUESTS => {
                let wait = Duration::from_secs(retry_after_s.unwrap_or(0)).max(SERVER_ERROR_WAIT);
                Self {
                    error: GroqError::RateLimited { retry_after_s },
                    wait: (wait <= MAX_RETRY_WAIT).then_some(wait),
                }
            }
            s if s.is_server_error() => Self {
                error: GroqError::Http(s.as_u16(), message),
                wait: Some(SERVER_ERROR_WAIT),
            },
            s => Self::fatal(GroqError::Http(s.as_u16(), message)),
        }
    }
}

/// Groq's `{"error": {"message": ...}}`, else the start of the body.
fn error_message(resp: Response) -> String {
    let body = resp.text().unwrap_or_default();
    let msg = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or(body);
    msg.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A local server that answers each request with the next canned (status, extra headers, body) and
    /// sends every request it got (head + body) back over the channel.
    fn server(replies: Vec<(u16, &'static str, &'static str)>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for (status, headers, body) in replies {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut head = String::new();
                let mut len = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    head.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut req_body = vec![0; len];
                reader.read_exact(&mut req_body).unwrap();
                // Tests that do not look at the requests drop the receiver.
                let _ = tx.send(head + &String::from_utf8_lossy(&req_body));
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                     Connection: close\r\n{headers}\r\n{body}",
                    body.len()
                );
                reader.get_mut().write_all(reply.as_bytes()).unwrap();
            }
        });
        (url, rx)
    }

    fn client(url: &str) -> Groq {
        let mut engine = spec::default_engine();
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
        assert_eq!(err, GroqError::Http(502, "bad gateway".into()));
    }

    #[test]
    fn long_rate_limit_waits_are_reported_not_slept() {
        let (url, _) = server(vec![(429, "Retry-After: 30\r\n", "{}")]);
        let started = Instant::now();
        let err = client(&url).transcribe(&clip()).unwrap_err();
        assert_eq!(
            err,
            GroqError::RateLimited {
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
            GroqError::BadKey
        );
    }

    #[test]
    fn empty_key_is_refused_before_any_request() {
        let err = Groq::new(spec::default_engine(), "  ".into())
            .err()
            .unwrap();
        assert_eq!(err, GroqError::MissingKey("GROQ_API_KEY".into()));
    }
}
