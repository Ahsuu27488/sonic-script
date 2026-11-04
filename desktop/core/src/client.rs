//! One engine from the spec, ready to use: its speech call (Groq / OpenAI transcription or Gemini) and, when
//! the spec gives one, the `refine` step that turns the transcript into Roman Urdu / Hinglish on another
//! provider (an OpenAI-style chat call, as `eval` `Refiner`). [`Chain`] puts engines in order of preference,
//! each asked when the one before is slow, fails or is resting after a rate limit (D-029).

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::blocking::Client as Http;
use serde_json::{Value, json};

use crate::api::{self, ApiError, Passed, Skip, Transcript};
use crate::encode::Encoded;
use crate::gemini::Gemini;
use crate::groq::Groq;
use crate::pipeline::Transcribe;
use crate::spec::{Engine, Refine};
use crate::words::{AUDIO_PROMPT_MAX, FORMAT_PROMPT_MAX, WordList};

enum Speech {
    Groq(Groq),
    Gemini(Gemini),
}

pub struct Client {
    engine: Engine,
    speech: Speech,
    refiner: Option<Refiner>,
    /// The refiner is the formatting step after an engine whose own text is already Roman: when it fails,
    /// that text is used unformatted (D-030).
    optional: bool,
    formats: bool,
    /// The user's words (D-034): in the Gemini prompt and the format / refine step.
    words: Option<Arc<WordList>>,
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
            optional: false,
            formats: false,
            words: None,
        })
    }

    /// The user's word list ("My words", D-034): the first [`AUDIO_PROMPT_MAX`] words go with the audio to a
    /// Gemini model that takes a prompt; the format / refine step gets the whole list when it is short, else
    /// the words that come close to the transcript. Never in the Whisper prompt (D-014).
    pub fn with_words(mut self, words: Arc<WordList>) -> Self {
        if let Speech::Gemini(g) = &mut self.speech {
            g.set_words(words.prompt_line(None, AUDIO_PROMPT_MAX));
        }
        self.words = Some(words);
        self
    }

    /// Runs the smart-formatting step (D-030, `spec::format`) after the speech call, in place of the engine's
    /// own `refine` step (the format prompt also romanizes). After an engine without a refine step it is
    /// optional: a failed format request leaves the engine's text as it is.
    pub fn with_format(
        mut self,
        format: Refine,
        key_for: impl Fn(&str) -> String,
    ) -> Result<Self, ApiError> {
        let key = key_for(&format.provider.key_env);
        self.refiner = Some(Refiner::new(format, key)?);
        self.optional = self.engine.refine().is_none();
        self.formats = true;
        Ok(self)
    }

    /// Whether [`Self::with_format`] was applied.
    pub fn formats(&self) -> bool {
        self.formats
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }
}

impl Transcribe for Client {
    fn id(&self) -> &str {
        self.engine.id()
    }

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
                let refined = match r.run(&heard.text, self.words.as_deref()) {
                    Ok(refined) => refined,
                    Err(e) if self.optional => {
                        return Ok(Transcript {
                            unformatted: Some(e.to_string()),
                            ..heard
                        });
                    }
                    Err(e) => return Err(e),
                };
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

/// How long an engine is skipped after its free requests for the day ran out. Google resets them at
/// midnight Pacific (about 1 PM in Pakistan); asking again every hour costs one quick 429 at most.
pub const DAILY_REST: Duration = Duration::from_secs(60 * 60);
/// The shortest pause after any other 429 (a per-minute limit).
const MIN_REST: Duration = Duration::from_secs(60);

/// Engines in order of preference (D-029). The clip goes to the first; when it fails, or has not answered
/// within its wait, it also goes to the next one, and so on; the first good answer wins. A last-resort engine
/// (D-033: Groq Whisper) is asked only once every engine asked before it has failed, never for slowness. An
/// engine that hit a rate limit rests (is skipped at once) for a while. A request that loses keeps running on
/// its own thread until it ends (at most the 10 s client timeout) and is ignored.
pub struct Chain<T> {
    links: Vec<Link<T>>,
    /// Per link: skipped until then.
    resting: Arc<Mutex<Vec<Option<Instant>>>>,
}

struct Link<T> {
    engine: Arc<T>,
    /// How long to wait for this engine before also asking the next one.
    wait: Duration,
    last_resort: bool,
}

/// (place among the engines asked, answer)
type Reply = (usize, Result<Transcript, ApiError>);

impl<T: Transcribe + Send + Sync + 'static> Chain<T> {
    /// `links`: (engine, wait) in order; the last engine's wait is not used. Panics when empty.
    pub fn new(links: Vec<(T, Duration)>) -> Self {
        Self::with_last_resort(links.into_iter().map(|(e, w)| (e, w, false)).collect())
    }

    /// `links`: (engine, wait, last resort) in order (D-033). Panics when empty.
    pub fn with_last_resort(links: Vec<(T, Duration, bool)>) -> Self {
        assert!(!links.is_empty(), "a chain needs an engine");
        let resting = Arc::new(Mutex::new(vec![None; links.len()]));
        let links = links
            .into_iter()
            .map(|(engine, wait, last_resort)| Link {
                engine: Arc::new(engine),
                wait,
                last_resort,
            })
            .collect();
        Self { links, resting }
    }

    pub fn first(&self) -> &T {
        &self.links[0].engine
    }

    /// The engines in order, each with its wait.
    pub fn links(&self) -> impl Iterator<Item = (&T, Duration)> {
        self.links.iter().map(|l| (&*l.engine, l.wait))
    }

    /// Whether link `i` is asked only after every engine before it has failed (D-033).
    pub fn is_last_resort(&self, i: usize) -> bool {
        self.links.get(i).is_some_and(|l| l.last_resort)
    }

    /// Starts link `link` on its own thread; the answer comes back tagged with `place`.
    fn ask(&self, link: usize, place: usize, audio: &Arc<Encoded>, tx: mpsc::Sender<Reply>) {
        let engine = self.links[link].engine.clone();
        let (audio, resting) = (audio.clone(), self.resting.clone());
        std::thread::spawn(move || {
            let result = engine.transcribe(&audio);
            let until = result.as_ref().err().and_then(rest_for);
            if let Ok(mut r) = resting.lock() {
                r[link] = until.map(|d| Instant::now() + d);
            }
            // The receiver is gone when another engine already won: nothing to do then.
            let _ = tx.send((place, result));
        });
    }
}

fn rest_for(e: &ApiError) -> Option<Duration> {
    match e {
        ApiError::RateLimited { daily: true, .. } => Some(DAILY_REST),
        ApiError::RateLimited { retry_after_s, .. } => {
            Some(Duration::from_secs(retry_after_s.unwrap_or(0)).max(MIN_REST))
        }
        _ => None,
    }
}

/// Our sender goes to the last engine asked, so `recv` ends once every thread is done.
fn sender(tx: &mut Option<mpsc::Sender<Reply>>, last: bool) -> mpsc::Sender<Reply> {
    if last { tx.take() } else { tx.clone() }.expect("each engine is asked once")
}

impl<T: Transcribe + Send + Sync + 'static> Transcribe for Chain<T> {
    fn id(&self) -> &str {
        self.first().id()
    }

    /// Warms every engine at once (all but the first on their own threads).
    fn warm(&self) {
        for link in &self.links[1..] {
            let engine = link.engine.clone();
            std::thread::spawn(move || engine.warm());
        }
        self.first().warm();
    }

    /// When every engine asked fails: that engine's error when only one was asked, else
    /// [`ApiError::AllFailed`] with each one's (D-035).
    fn transcribe(&self, audio: &Encoded) -> Result<Transcript, ApiError> {
        let id = |link: usize| self.links[link].engine.id().to_string();
        let now = Instant::now();
        let resting: Vec<bool> = match self.resting.lock() {
            Ok(r) => r.iter().map(|u| u.is_some_and(|u| u > now)).collect(),
            Err(_) => vec![false; self.links.len()],
        };
        let mut order: Vec<usize> = (0..self.links.len()).filter(|&i| !resting[i]).collect();
        let mut passed = Vec::new();
        if order.is_empty() {
            // Every engine is resting: asking them anyway beats giving nothing.
            order = (0..self.links.len()).collect();
        } else {
            for i in (0..self.links.len()).filter(|&i| resting[i]) {
                passed.push(Passed {
                    engine: id(i),
                    why: Skip::Resting,
                });
            }
        }

        let audio = Arc::new(audio.clone());
        let (tx, rx) = mpsc::channel();
        let mut tx = Some(tx);
        let mut errors: Vec<Option<ApiError>> = vec![None; order.len()];
        self.ask(order[0], 0, &audio, sender(&mut tx, order.len() == 1));
        let (mut asked, mut since) = (1, Instant::now());
        loop {
            // The next engine is a last resort: it waits for failures, not for time.
            let next_waits_for_failures =
                asked < order.len() && self.links[order[asked]].last_resort;
            let reply = if asked < order.len() && !next_waits_for_failures {
                let wait = self.links[order[asked - 1]]
                    .wait
                    .saturating_sub(since.elapsed());
                // We still hold a sender, so this can only time out.
                rx.recv_timeout(wait).ok()
            } else {
                match rx.recv() {
                    Ok(r) => Some(r),
                    Err(_) => break,
                }
            };
            let next = match reply {
                Some((place, Ok(t))) => {
                    return Ok(Transcript {
                        engine: id(order[place]),
                        passed,
                        ..t
                    });
                }
                Some((place, Err(e))) => {
                    // Only the newest engine failing moves the chain on; an older one may still answer.
                    // Before a last resort, every engine asked so far must have failed.
                    let newest = place + 1 == asked && asked < order.len();
                    if newest || next_waits_for_failures {
                        passed.push(Passed {
                            engine: id(order[place]),
                            why: Skip::Failed(e.clone()),
                        });
                    }
                    errors[place] = Some(e);
                    if errors.iter().all(Option::is_some) {
                        break;
                    }
                    if next_waits_for_failures {
                        errors[..asked].iter().all(Option::is_some)
                    } else {
                        newest
                    }
                }
                None => {
                    passed.push(Passed {
                        engine: id(order[asked - 1]),
                        why: Skip::Slow,
                    });
                    true
                }
            };
            if next {
                let last = asked + 1 == order.len();
                self.ask(order[asked], asked, &audio, sender(&mut tx, last));
                asked += 1;
                since = Instant::now();
            }
        }
        let mut failed: Vec<(String, ApiError)> = errors
            .into_iter()
            .enumerate()
            .filter_map(|(place, e)| Some((id(order[place]), e?)))
            .collect();
        match failed.len() {
            0 => Err(ApiError::Network("the engine threads stopped".into())),
            1 => Err(failed.remove(0).1),
            _ => Err(ApiError::AllFailed(failed)),
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

    fn run(&self, text: &str, words: Option<&WordList>) -> Result<Transcript, ApiError> {
        let s = &self.spec;
        let line = words
            .map(|w| w.prompt_line(Some(text), FORMAT_PROMPT_MAX))
            .unwrap_or_default();
        let prompt = crate::spec::with_words(&s.prompt, s.words_at, &line);
        let mut body = json!({
            "model": s.model,
            "temperature": 0,
            "messages": [
                { "role": "system", "content": prompt },
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

    fn flash_lite_at(gemini_url: String) -> Client {
        let Some(Engine::Gemini(mut e)) = spec::engine("gemini-3.5-flash-lite") else {
            panic!("flash-lite engine");
        };
        e.provider.base_url = gemini_url;
        Client::new(Engine::Gemini(e), |var| format!("{var}-value")).unwrap()
    }

    const GEMINI_REPLY: &str =
        r#"{"candidates":[{"content":{"parts":[{"text":"doodh anday bread aur do kilo aloo"}]}}]}"#;

    #[test]
    fn the_format_step_runs_after_an_engine_without_its_own() {
        let (gemini_url, _) = server(vec![(200, "", GEMINI_REPLY)]);
        let (groq_url, requests) = server(vec![(
            200,
            "",
            r#"{"choices":[{"message":{"content":"Lana hai:\n- Doodh\n- Anday"}}]}"#,
        )]);
        let mut format = spec::format().unwrap();
        format.provider.base_url = groq_url;
        let client = flash_lite_at(gemini_url)
            .with_format(format, |var| format!("{var}-value"))
            .unwrap();
        assert!(client.formats());
        let audio = crate::encode::opus(&vec![0.1; 16_000]).unwrap();
        let t = client.transcribe(&audio).unwrap();
        assert_eq!(t.text, "Lana hai:\n- Doodh\n- Anday");
        assert_eq!(t.asr.as_deref(), Some("doodh anday bread aur do kilo aloo"));
        assert_eq!(t.unformatted, None);
        assert!(
            requests
                .recv()
                .unwrap()
                .contains("You turn a raw dictation transcript")
        );
    }

    #[test]
    fn the_users_words_go_with_the_audio_and_to_the_format_step() {
        let (gemini_url, gemini_requests) = server(vec![(200, "", GEMINI_REPLY)]);
        let (groq_url, groq_requests) = server(vec![(
            200,
            "",
            r#"{"choices":[{"message":{"content":"Doodh"}}]}"#,
        )]);
        let mut format = spec::format().unwrap();
        format.provider.base_url = groq_url;
        let words = Arc::new(WordList::from_text("Claude = McCloud\nTaskvane"));
        let client = flash_lite_at(gemini_url)
            .with_format(format, |var| format!("{var}-value"))
            .unwrap()
            .with_words(words);
        let audio = crate::encode::opus(&vec![0.1; 16_000]).unwrap();
        client.transcribe(&audio).unwrap();
        let line = "Spell these words exactly like this when you hear them: Claude (may come out as \
                    McCloud), Taskvane.";
        let gemini = gemini_requests.recv().unwrap();
        assert!(gemini.contains(line), "Gemini prompt: {gemini}");
        let groq = groq_requests.recv().unwrap();
        assert!(groq.contains(line), "format prompt: {groq}");
        // In the slot: just before the closing instruction, not at the end of the prompt.
        assert!(groq.contains(&format!("{line}\\n\\nOutput only the final text")));
    }

    #[test]
    fn a_failed_format_step_keeps_the_engines_own_text() {
        let (gemini_url, _) = server(vec![(200, "", GEMINI_REPLY)]);
        let (groq_url, _) = server(vec![(400, "", r#"{"error":{"message":"bad"}}"#)]);
        let mut format = spec::format().unwrap();
        format.provider.base_url = groq_url;
        let client = flash_lite_at(gemini_url)
            .with_format(format, |var| format!("{var}-value"))
            .unwrap();
        let audio = crate::encode::opus(&vec![0.1; 16_000]).unwrap();
        let t = client.transcribe(&audio).unwrap();
        assert_eq!(t.text, "doodh anday bread aur do kilo aloo");
        assert!(t.unformatted.is_some());
    }

    #[test]
    fn the_format_step_replaces_transcribes_cleanup_and_is_required_there() {
        // Transcribe writes Urdu / Devanagari script: without the step its text cannot be used.
        let transcribe =
            r#"{"candidates":[{"content":{"parts":[{"audioTranscription":{"text":"दूध अंडे"}}]}}]}"#;
        let (gemini_url, _) = server(vec![(200, "", transcribe)]);
        let (groq_url, requests) = server(vec![(400, "", r#"{"error":{"message":"bad"}}"#)]);
        let Some(Engine::Gemini(mut e)) = spec::engine("gemini-3.5-transcribe+gptoss20b") else {
            panic!("transcribe engine");
        };
        e.provider.base_url = gemini_url;
        let mut format = spec::format().unwrap();
        format.provider.base_url = groq_url;
        let client = Client::new(Engine::Gemini(e), |var| format!("{var}-value"))
            .unwrap()
            .with_format(format, |var| format!("{var}-value"))
            .unwrap();
        let audio = crate::encode::opus(&vec![0.1; 16_000]).unwrap();
        assert!(client.transcribe(&audio).is_err());
        let req = requests.recv().unwrap();
        assert!(
            req.contains(&spec::format().unwrap().model)
                && req.contains("You turn a raw dictation")
        );
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
        fn id(&self) -> &str {
            self.id
        }

        fn transcribe(&self, _: &Encoded) -> Result<Transcript, ApiError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.after);
            self.reply.clone().map(|text| Transcript {
                text: text.into(),
                ..Transcript::default()
            })
        }
    }

    const WAIT: Duration = Duration::from_millis(300);

    /// A chain of fakes, each waited on for [`WAIT`], and each fake's call counter.
    fn chain(fakes: Vec<Fake>) -> (Chain<Fake>, Vec<Arc<AtomicU32>>) {
        let calls = fakes.iter().map(|f| f.calls.clone()).collect();
        (
            Chain::new(fakes.into_iter().map(|f| (f, WAIT)).collect()),
            calls,
        )
    }

    fn run(c: &Chain<Fake>) -> (Result<Transcript, ApiError>, Duration) {
        let audio = crate::encode::opus(&vec![0.1; 1_600]).unwrap();
        let t = Instant::now();
        (c.transcribe(&audio), t.elapsed())
    }

    fn calls(c: &[Arc<AtomicU32>]) -> Vec<u32> {
        c.iter().map(|n| n.load(Ordering::SeqCst)).collect()
    }

    fn slow(engine: &str) -> Passed {
        Passed {
            engine: engine.into(),
            why: Skip::Slow,
        }
    }

    fn failed(engine: &str, e: ApiError) -> Passed {
        Passed {
            engine: engine.into(),
            why: Skip::Failed(e),
        }
    }

    fn daily() -> ApiError {
        ApiError::RateLimited {
            retry_after_s: None,
            daily: true,
        }
    }

    #[test]
    fn a_slow_engine_hands_over_to_the_next_after_its_wait() {
        let (c, n) = chain(vec![
            Fake::new("transcribe", 2_000, Ok("Transcribe text")),
            Fake::new("flash", 50, Ok("Flash text")),
        ]);
        let (r, took) = run(&c);
        let t = r.unwrap();
        assert_eq!(
            (t.text.as_str(), t.engine.as_str()),
            ("Flash text", "flash")
        );
        assert_eq!(t.passed, [slow("transcribe")]);
        assert_eq!(calls(&n), [1, 1]);
        assert!(
            took >= WAIT && took < WAIT + Duration::from_millis(500),
            "{took:?}"
        );
    }

    #[test]
    fn each_slow_engine_adds_its_wait_down_the_chain() {
        let (c, _) = chain(vec![
            Fake::new("transcribe", 2_000, Ok("1")),
            Fake::new("flash", 2_000, Ok("2")),
            Fake::new("lite", 2_000, Ok("3")),
            Fake::new("groq", 20, Ok("4")),
        ]);
        let (r, took) = run(&c);
        let t = r.unwrap();
        assert_eq!(t.engine, "groq");
        assert_eq!(t.passed, [slow("transcribe"), slow("flash"), slow("lite")]);
        assert!(
            took >= 3 * WAIT && took < 3 * WAIT + Duration::from_millis(500),
            "{took:?}"
        );
    }

    #[test]
    fn errors_go_down_the_chain_at_once() {
        let busy = ApiError::Http(503, "high demand".into());
        let (c, _) = chain(vec![
            Fake::new("transcribe", 0, Err(busy.clone())),
            Fake::new("flash", 0, Err(daily())),
            Fake::new("lite", 30, Ok("Lite text")),
        ]);
        let (r, took) = run(&c);
        let t = r.unwrap();
        assert_eq!(t.engine, "lite");
        assert_eq!(
            t.passed,
            [failed("transcribe", busy), failed("flash", daily())]
        );
        assert!(took < WAIT, "no waiting on failed engines: {took:?}");
    }

    /// Like [`chain`], with the last fake as a last resort (D-033).
    fn chain_with_last_resort(fakes: Vec<Fake>) -> (Chain<Fake>, Vec<Arc<AtomicU32>>) {
        let calls = fakes.iter().map(|f| f.calls.clone()).collect();
        let last = fakes.len() - 1;
        let links = fakes
            .into_iter()
            .enumerate()
            .map(|(i, f)| (f, WAIT, i == last))
            .collect();
        (Chain::with_last_resort(links), calls)
    }

    #[test]
    fn slow_engines_never_bring_in_the_last_resort() {
        let (c, n) = chain_with_last_resort(vec![
            Fake::new("transcribe", 1_000, Ok("Transcribe text")),
            Fake::new("flash", 1_000, Ok("Flash text")),
            Fake::new("groq", 0, Ok("Groq text")),
        ]);
        let t = run(&c).0.unwrap();
        assert_eq!(t.engine, "transcribe");
        assert_eq!(t.passed, [slow("transcribe")]);
        assert_eq!(calls(&n), [1, 1, 0], "Groq is never asked");
    }

    #[test]
    fn the_last_resort_waits_until_every_engine_before_it_failed() {
        let busy = ApiError::Http(503, "high demand".into());
        let (c, n) = chain_with_last_resort(vec![
            Fake::new("transcribe", 0, Err(daily())),
            // Fails late: the last resort must not start before this.
            Fake::new("flash", 2 * WAIT.as_millis() as u64, Err(busy.clone())),
            Fake::new("groq", 0, Ok("Groq text")),
        ]);
        let (r, took) = run(&c);
        let t = r.unwrap();
        assert_eq!(t.engine, "groq");
        assert_eq!(
            t.passed,
            [failed("transcribe", daily()), failed("flash", busy)]
        );
        assert_eq!(calls(&n), [1, 1, 1]);
        assert!(took >= 2 * WAIT, "Groq only after Flash failed: {took:?}");
    }

    #[test]
    fn an_early_failure_does_not_start_the_last_resort_while_another_engine_runs() {
        let (c, n) = chain_with_last_resort(vec![
            Fake::new(
                "transcribe",
                2 * WAIT.as_millis() as u64,
                Ok("Transcribe text"),
            ),
            Fake::new("flash", 0, Err(daily())),
            Fake::new("groq", 0, Ok("Groq text")),
        ]);
        let t = run(&c).0.unwrap();
        assert_eq!(t.engine, "transcribe");
        assert_eq!(calls(&n), [1, 1, 0]);
    }

    #[test]
    fn a_fast_first_engine_is_the_only_one_asked() {
        let (c, n) = chain(vec![
            Fake::new("transcribe", 20, Ok("Transcribe text")),
            Fake::new("flash", 0, Ok("Flash text")),
            Fake::new("groq", 0, Ok("Groq text")),
        ]);
        let t = run(&c).0.unwrap();
        assert_eq!((t.engine.as_str(), t.passed.len()), ("transcribe", 0));
        assert_eq!(calls(&n), [1, 0, 0]);
    }

    #[test]
    fn a_slow_engine_still_wins_when_the_next_fails() {
        let (c, _) = chain(vec![
            Fake::new("lite", 600, Ok("Lite text")),
            Fake::new("groq", 0, Err(ApiError::Timeout)),
        ]);
        let t = run(&c).0.unwrap();
        assert_eq!(t.engine, "lite");
        assert_eq!(t.passed, [slow("lite")], "the next one was asked, and lost");
    }

    #[test]
    fn when_every_engine_fails_each_error_is_returned() {
        let first = ApiError::Http(503, "high demand".into());
        let bad_key = ApiError::BadKey("GROQ_API_KEY".into());
        let (c, _) = chain(vec![
            Fake::new("lite", 0, Err(first.clone())),
            Fake::new("groq", 0, Err(bad_key.clone())),
        ]);
        let all = ApiError::AllFailed(vec![
            ("lite".into(), first.clone()),
            ("groq".into(), bad_key.clone()),
        ]);
        assert_eq!(run(&c).0.unwrap_err(), all);
        let (c, _) = chain(vec![
            Fake::new("lite", 400, Err(ApiError::Timeout)),
            Fake::new("groq", 0, Err(bad_key.clone())),
        ]);
        assert_eq!(
            run(&c).0.unwrap_err(),
            ApiError::AllFailed(vec![
                ("lite".into(), ApiError::Timeout),
                ("groq".into(), bad_key)
            ]),
            "also when it fails late"
        );
        let (c, _) = chain(vec![Fake::new("lite", 0, Err(first.clone()))]);
        assert_eq!(run(&c).0.unwrap_err(), first, "one engine: its own error");
    }

    #[test]
    fn a_used_up_engine_is_skipped_on_the_next_clips() {
        let (c, n) = chain(vec![
            Fake::new("transcribe", 0, Err(daily())),
            Fake::new("flash", 0, Ok("Flash text")),
        ]);
        let t = run(&c).0.unwrap();
        assert_eq!(t.passed, [failed("transcribe", daily())]);
        let (r, took) = run(&c);
        let t = r.unwrap();
        assert_eq!(t.engine, "flash");
        let resting = Passed {
            engine: "transcribe".into(),
            why: Skip::Resting,
        };
        assert_eq!(t.passed, [resting]);
        assert_eq!(calls(&n), [1, 2], "not asked again while it rests");
        assert!(took < WAIT);
    }

    #[test]
    fn other_errors_do_not_rest_an_engine() {
        let (c, n) = chain(vec![
            Fake::new(
                "transcribe",
                0,
                Err(ApiError::Http(503, "high demand".into())),
            ),
            Fake::new("flash", 0, Ok("Flash text")),
        ]);
        run(&c).0.unwrap();
        run(&c).0.unwrap();
        assert_eq!(calls(&n), [2, 2]);
    }

    #[test]
    fn a_lone_engine_is_asked_even_while_it_rests() {
        let (c, n) = chain(vec![Fake::new("flash", 0, Err(daily()))]);
        assert_eq!(run(&c).0.unwrap_err(), daily());
        assert_eq!(run(&c).0.unwrap_err(), daily());
        assert_eq!(
            calls(&n),
            [2],
            "with nothing else to ask, it is asked anyway"
        );
    }

    #[test]
    fn a_lone_engine_answers_however_long_it_takes() {
        let (c, _) = chain(vec![Fake::new("lite", 400, Ok("Lite text"))]);
        let t = run(&c).0.unwrap();
        assert_eq!((t.engine.as_str(), t.passed.len()), ("lite", 0));
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
