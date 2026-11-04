//! What every engine client shares (Groq, Gemini): one blocking HTTP client with a 10 s timeout, one retry on
//! 429 / 5xx / a dropped connection, and errors that say what to fix. Keys are never logged or printed.

use std::time::{Duration, Instant};

use reqwest::StatusCode;
use reqwest::blocking::{Client, RequestBuilder, Response};

pub const TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// A 429 asking us to wait longer than this is reported instead of retried: the user is waiting.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(2);
const SERVER_ERROR_WAIT: Duration = Duration::from_millis(300);

/// What the engine heard (before the text cleanup).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Transcript {
    /// The final text (after the refine step when the engine has one).
    pub text: String,
    /// What the speech model returned when a refine step changed it.
    pub asr: Option<String>,
    /// Time of the speech request that succeeded, without earlier attempts or waits.
    pub request_ms: u32,
    /// Time of the refine request (0 without one).
    pub refine_ms: u32,
    /// Retries across both steps.
    pub retries: u8,
    /// Tokens billed by the speech model (Gemini); 0 where the price is per second of audio (Whisper).
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub refine_tokens_in: u32,
    pub refine_tokens_out: u32,
    /// Id of the engine that answered.
    pub engine: String,
    /// Engines of the [`crate::client::Chain`] that were passed over on the way, and why (D-029). The answer
    /// can still be from one of them (a slow engine that answered first after all).
    pub passed: Vec<Passed>,
    /// Why the smart-formatting step (D-030) failed when the engine's own text was used instead.
    pub unformatted: Option<String>,
}

/// An engine the chain went past.
#[derive(Debug, Clone, PartialEq)]
pub struct Passed {
    pub engine: String,
    pub why: Skip,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Skip {
    /// No answer within its wait, so the next engine was asked too.
    Slow,
    /// It failed (429, 5xx, timeout, ...).
    Failed(ApiError),
    /// Not asked: it hit a rate limit a little earlier.
    Resting,
}

impl std::fmt::Display for Passed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.why {
            Skip::Slow => write!(f, "{} was slow", self.engine),
            Skip::Failed(e) => write!(f, "{} failed ({e})", self.engine),
            Skip::Resting => write!(f, "{} skipped (rate limit)", self.engine),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// The environment variable that should hold the key is empty.
    MissingKey(String),
    /// 401 / 403: the key is wrong or revoked; holds the variable to check.
    BadKey(String),
    RateLimited {
        retry_after_s: Option<u64>,
        /// The provider's daily quota is used up (Google names a `...PerDay...` quota); per-minute otherwise.
        daily: bool,
    },
    /// Any other HTTP error, with the provider's own message.
    Http(u16, String),
    Timeout,
    Network(String),
    /// 200 but not the JSON we expect.
    BadResponse(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingKey(var) => write!(f, "no API key: set {var}"),
            Self::BadKey(var) => write!(f, "the key was refused (check {var})"),
            Self::RateLimited { daily: true, .. } => write!(f, "today's free requests are used up"),
            Self::RateLimited {
                retry_after_s: Some(s),
                ..
            } => write!(f, "rate limit reached: try again in {s} s"),
            Self::RateLimited {
                retry_after_s: None,
                ..
            } => write!(f, "rate limit reached"),
            Self::Http(status, msg) => write!(f, "HTTP {status}: {msg}"),
            Self::Timeout => write!(f, "no answer within {} s", TIMEOUT.as_secs()),
            Self::Network(msg) => write!(f, "network error: {msg}"),
            Self::BadResponse(msg) => write!(f, "unexpected answer: {msg}"),
        }
    }
}

impl std::error::Error for ApiError {}

pub fn client() -> Result<Client, ApiError> {
    Client::builder()
        .timeout(TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| ApiError::Network(e.to_string()))
}

/// Refuses an empty key before any request is made.
pub fn check_key(key: &str, key_env: &str) -> Result<String, ApiError> {
    match key.trim() {
        "" => Err(ApiError::MissingKey(key_env.to_string())),
        k => Ok(k.to_string()),
    }
}

/// Sends what `build` makes and returns the JSON body, the time of the attempt that worked (ms) and the
/// number of retries (0 or 1). `build` runs once per attempt, since a request body can only be sent once.
pub fn send(
    build: impl Fn() -> Result<RequestBuilder, ApiError>,
    key_env: &str,
) -> Result<(serde_json::Value, u32, u8), ApiError> {
    match attempt(&build, key_env) {
        Ok((body, ms)) => Ok((body, ms, 0)),
        Err(Failure {
            wait: Some(wait), ..
        }) => {
            std::thread::sleep(wait);
            let (body, ms) = attempt(&build, key_env).map_err(|f| f.error)?;
            Ok((body, ms, 1))
        }
        Err(Failure { error, wait: None }) => Err(error),
    }
}

fn attempt(
    build: &impl Fn() -> Result<RequestBuilder, ApiError>,
    key_env: &str,
) -> Result<(serde_json::Value, u32), Failure> {
    let request = build().map_err(Failure::fatal)?;
    let started = Instant::now();
    let resp = request.send().map_err(Failure::from_send)?;
    if !resp.status().is_success() {
        return Err(Failure::from_status(resp, key_env));
    }
    let body = resp.json().map_err(|err| {
        Failure::fatal(if err.is_timeout() {
            ApiError::Timeout
        } else {
            ApiError::BadResponse(err.to_string())
        })
    })?;
    Ok((body, started.elapsed().as_millis() as u32))
}

/// A failed attempt; `wait` is set when it is worth one retry after that pause.
struct Failure {
    error: ApiError,
    wait: Option<Duration>,
}

impl Failure {
    fn fatal(error: ApiError) -> Self {
        Self { error, wait: None }
    }

    fn from_send(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            // Waiting another 10 s would be worse than reporting it.
            return Self::fatal(ApiError::Timeout);
        }
        // Most often a pooled connection the server already closed: retry at once on a fresh one.
        Self {
            error: ApiError::Network(err.to_string()),
            wait: Some(Duration::ZERO),
        }
    }

    fn from_status(resp: Response, key_env: &str) -> Self {
        let status = resp.status();
        let retry_after_s = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|s| s.ceil() as u64);
        let body = resp.text().unwrap_or_default();
        let message = error_message(&body);
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                Self::fatal(ApiError::BadKey(key_env.to_string()))
            }
            StatusCode::TOO_MANY_REQUESTS => {
                // Google names the quota that ran out; a daily one is not worth a retry.
                let daily = body.contains("PerDay");
                let wait = Duration::from_secs(retry_after_s.unwrap_or(0)).max(SERVER_ERROR_WAIT);
                Self {
                    error: ApiError::RateLimited {
                        retry_after_s,
                        daily,
                    },
                    wait: (!daily && wait <= MAX_RETRY_WAIT).then_some(wait),
                }
            }
            s if s.is_server_error() => Self {
                error: ApiError::Http(s.as_u16(), message),
                wait: Some(SERVER_ERROR_WAIT),
            },
            s => Self::fatal(ApiError::Http(s.as_u16(), message)),
        }
    }
}

/// The provider's `{"error": {"message": ...}}` (Groq, OpenAI and Gemini all use it), else the body's start.
fn error_message(body: &str) -> String {
    let msg = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.to_string());
    msg.chars().take(200).collect()
}

#[cfg(test)]
pub(crate) mod test_server {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A local server that answers each request with the next canned (status, extra headers, body) and
    /// sends every request it got (head + body) back over the channel.
    pub fn server(
        replies: Vec<(u16, &'static str, &'static str)>,
    ) -> (String, mpsc::Receiver<String>) {
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
}
