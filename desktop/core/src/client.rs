//! One engine from the spec, ready to use: its speech call (Groq / OpenAI transcription or Gemini) and, when
//! the spec gives one, the `refine` step that turns the transcript into Roman Urdu / Hinglish on another
//! provider (an OpenAI-style chat call, as `eval` `Refiner`). [`Chain`] puts engines in order of preference,
//! each asked when the one before is slow, fails or is resting after a rate limit (D-029).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client as Http;
use serde_json::{Value, json};

use crate::api::{self, ApiError, Passed, Skip, Transcript};
use crate::deadline::Timings;
use crate::encode::Encoded;
use crate::gemini::Gemini;
use crate::groq::Groq;
use crate::normalize::needs_romanizing;
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
    /// D-039: makes an answer in Urdu / Devanagari script Roman when no refine or format step did.
    romanizer: Option<Refiner>,
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
            romanizer: None,
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

    /// The Roman step (D-039, `spec::romanize`) for an answer that comes back mostly in Urdu / Devanagari
    /// script while no step of the engine's own made it Roman: no refine and no format step
    /// (the default since D-041), or the optional format step failed. Without it such an answer is an error, so a
    /// chain asks its next engine: the local letter table's rough draft is not pasted.
    pub fn with_romanize(
        mut self,
        romanize: Refine,
        key_for: impl Fn(&str) -> String,
    ) -> Result<Self, ApiError> {
        if self.engine.refine().is_none() {
            let key = key_for(&romanize.provider.key_env);
            self.romanizer = Some(Refiner::new(romanize, key)?);
        }
        Ok(self)
    }

    /// The engine's own answer in Urdu / Devanagari script, made Roman by the D-039 step, or an error.
    fn romanized(
        &self,
        heard: Transcript,
        unformatted: Option<String>,
    ) -> Result<Transcript, ApiError> {
        let Some(r) = &self.romanizer else {
            return Err(ApiError::BadResponse(format!(
                "the answer came in Urdu / Devanagari script and no Roman step ran{}",
                unformatted.map(|u| format!(" ({u})")).unwrap_or_default()
            )));
        };
        let refined = r.run(&heard.text, self.words.as_deref())?;
        Ok(Transcript {
            asr: Some(heard.text),
            text: refined.text,
            refine_ms: refined.request_ms,
            retries: heard.retries + refined.retries,
            refine_tokens_in: refined.tokens_in,
            refine_tokens_out: refined.tokens_out,
            unformatted,
            ..heard
        })
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
                    Err(e) if self.optional && needs_romanizing(&heard.text) => {
                        return self.romanized(heard, Some(e.to_string()));
                    }
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
            None if needs_romanizing(&heard.text) => self.romanized(heard, None),
            _ => Ok(heard),
        }
    }
}

/// How long an engine is skipped after its free requests for the day ran out: until Google resets them at
/// midnight Pacific (about 1 PM in Pakistan), plus a few minutes (D-040). Asking again sooner is not a quick
/// 429: live 2026-11-05 Google took 4-6 s to refuse, and the next engine waited that long.
pub fn daily_rest(now: SystemTime) -> Duration {
    let t = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    Duration::from_secs(next_pacific_midnight(t) - t + 5 * 60)
}

/// The next midnight in Los Angeles after `t` (Unix seconds): 08:00 UTC, or 07:00 UTC while US daylight
/// saving time is on (second Sunday of March to first Sunday of November).
fn next_pacific_midnight(t: u64) -> u64 {
    let day = t / 86_400;
    (day..day + 3)
        .flat_map(|d| [7, 8].map(|h| d * 86_400 + h * 3_600))
        .find(|&c| c > t && (c / 3_600) % 24 == if pacific_dst(c) { 7 } else { 8 })
        .expect("a midnight within 3 days")
}

/// Whether US daylight saving time is on at `t` (Unix seconds) in Los Angeles.
fn pacific_dst(t: u64) -> bool {
    let year = civil(t as i64 / 86_400).0;
    // Day numbers (days since 1970-01-01, a Thursday) of the n-th Sunday on or after a date.
    let sunday = |m: u32, nth: i64| {
        let first = days(year, m, 1);
        first + (7 - (first + 4).rem_euclid(7)) % 7 + 7 * (nth - 1)
    };
    let start = sunday(3, 2) * 86_400 + 10 * 3_600; // 2 AM PST
    let end = sunday(11, 1) * 86_400 + 9 * 3_600; // 2 AM PDT
    (start..end).contains(&(t as i64))
}

/// Days since 1970-01-01 of a date (H. Hinnant's `days_from_civil`).
fn days(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    era * 146_097 + (yoe * 365 + yoe / 4 - yoe / 100 + doy) - 719_468
}

/// (year, month, day) of a day number (H. Hinnant's `civil_from_days`).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}
/// The shortest pause after any other 429 (a per-minute limit).
const MIN_REST: Duration = Duration::from_secs(60);

/// Engines in order of preference (D-029). The clip goes to the first; when it fails, or has not answered
/// within its wait, it also goes to the next one, and so on; the first good answer wins. A last-resort engine
/// (D-033: Groq Whisper) is asked only once every engine asked before it has failed, never for slowness. While
/// the first (main) engine is out, resting or failed, the others are asked together (D-040). An engine that
/// hit a rate limit rests (is skipped at once) for a while: until Google's reset for a daily quota, kept in a
/// file between runs with [`Chain::with_rest_file`]. A request that loses keeps running on
/// its own thread until it ends (at most the 10 s client timeout) and is ignored.
pub struct Chain<T> {
    links: Vec<Link<T>>,
    /// Per link: skipped until then.
    resting: Arc<Mutex<Vec<Option<SystemTime>>>>,
    /// Where the rests are kept between runs (D-040): a model out of free requests stays skipped after a
    /// restart. `None`: in memory only.
    rest_file: Option<Arc<PathBuf>>,
    /// Each engine's recent speed: its wait is learned from it (D-017, M2 phase 6).
    timings: Arc<Mutex<Timings>>,
    /// Where the timings are kept between runs; `None`: in memory only.
    timing_file: Option<Arc<PathBuf>>,
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
        Self {
            links,
            resting,
            rest_file: None,
            timings: Arc::new(Mutex::new(Timings::default())),
            timing_file: None,
        }
    }

    /// Learns each engine's wait from its answers and keeps them in `path` between runs (D-017).
    pub fn with_timing_file(mut self, path: PathBuf) -> Self {
        self.timings = Arc::new(Mutex::new(Timings::load(&path)));
        self.timing_file = Some(Arc::new(path));
        self
    }

    /// How long to wait for link `link` on a clip of `audio_ms` before also asking the next engine: learned
    /// once the engine has answered often enough, else the spec's wait.
    pub fn wait_for(&self, link: usize, audio_ms: u32) -> Duration {
        let l = &self.links[link];
        match self.timings.lock() {
            Ok(t) => t.wait(l.engine.id(), audio_ms, l.wait),
            Err(_) => l.wait,
        }
    }

    /// Keeps the rests in `path` (JSON: engine id → Unix seconds) and loads the ones still running (D-040).
    /// A missing or broken file is no rest at all.
    pub fn with_rest_file(mut self, path: PathBuf) -> Self {
        let saved: HashMap<String, u64> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let now = SystemTime::now();
        if let Ok(mut r) = self.resting.lock() {
            for (i, link) in self.links.iter().enumerate() {
                if let Some(&until) = saved.get(link.engine.id()) {
                    let until = UNIX_EPOCH + Duration::from_secs(until);
                    r[i] = (until > now).then_some(until);
                }
            }
        }
        self.rest_file = Some(Arc::new(path));
        self
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
        let file = self.rest_file.clone();
        let (timings, timing_file) = (self.timings.clone(), self.timing_file.clone());
        let ids: Vec<String> = self
            .links
            .iter()
            .map(|l| l.engine.id().to_string())
            .collect();
        std::thread::spawn(move || {
            let started = Instant::now();
            let result = engine.transcribe(&audio);
            // A good answer teaches the engine's speed, also when another engine won (D-017).
            if result.is_ok()
                && let Ok(mut t) = timings.lock()
            {
                t.record(engine.id(), started.elapsed(), audio.audio_ms);
                if let Some(file) = &timing_file {
                    t.save(file);
                }
            }
            let until = result.as_ref().err().and_then(rest_for);
            if let Ok(mut r) = resting.lock() {
                let changed = r[link].is_some() || until.is_some();
                r[link] = until.map(|d| SystemTime::now() + d);
                if let (true, Some(file)) = (changed, &file) {
                    save_rests(file, &ids, &r);
                }
            }
            // The receiver is gone when another engine already won: nothing to do then.
            let _ = tx.send((place, result));
        });
    }
}

/// Writes the rests still running; a failed write only means they are forgotten on the next start.
fn save_rests(file: &Path, ids: &[String], rests: &[Option<SystemTime>]) {
    let now = SystemTime::now();
    let secs = |t: SystemTime| t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs());
    // Other runs (`--engine`, another chain) keep their engines' rests in the same file.
    let mut map: HashMap<String, u64> = std::fs::read_to_string(file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let now_s = secs(now).unwrap_or(0);
    map.retain(|id, until| *until > now_s && !ids.contains(id));
    for (id, until) in ids.iter().zip(rests) {
        if let Some(until) = until.filter(|u| *u > now).and_then(secs) {
            map.insert(id.clone(), until);
        }
    }
    if let Ok(text) = serde_json::to_string(&map) {
        let _ = std::fs::write(file, text);
    }
}

fn rest_for(e: &ApiError) -> Option<Duration> {
    match e {
        ApiError::RateLimited { daily: true, .. } => Some(daily_rest(SystemTime::now())),
        ApiError::RateLimited { retry_after_s, .. } => {
            Some(Duration::from_secs(retry_after_s.unwrap_or(0)).max(MIN_REST))
        }
        _ => None,
    }
}

/// Whether the main engine's answer looks cut (D-042): 3+ s of speech and under 4 words a second of it. The
/// owner's normal clips run 5-9 words a second; Transcribe's two cut answers of 2026-11-05 had 2.1 and 2.4.
fn looks_cut(text: &str, speech_ms: Option<u32>) -> bool {
    match speech_ms {
        Some(ms) if ms >= 3_000 => (words(text) as u64) * 1_000 < 4 * u64::from(ms),
        _ => false,
    }
}

fn words(text: &str) -> usize {
    text.split_whitespace().count()
}

fn cut_error(text: &str, speech_ms: Option<u32>) -> ApiError {
    ApiError::BadResponse(format!(
        "its answer looked cut: {} words for {:.1} s of speech",
        words(text),
        f64::from(speech_ms.unwrap_or(0)) / 1_000.0
    ))
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
        let now = SystemTime::now();
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
        // Per place: already in `passed` (slow), so a later failure is not listed twice.
        let mut listed = vec![false; order.len()];
        let mut asked = 0;
        // Per place: its reply came (answer or error).
        let mut replied = vec![false; order.len()];
        // D-042: the main engine's answer while a fallback is asked because it looked cut.
        let mut held: Option<(usize, Transcript)> = None;
        // D-040: while the main engine is out (resting or failed), the fallbacks are asked together, not one
        // wait after another; a last resort still waits for failures.
        let together = |asked: usize| asked < order.len() && !self.links[order[asked]].last_resort;
        loop {
            self.ask(
                order[asked],
                asked,
                &audio,
                sender(&mut tx, asked + 1 == order.len()),
            );
            asked += 1;
            if order[0] == 0 || !together(asked) {
                break;
            }
        }
        let mut since = Instant::now();
        loop {
            // The next engine is a last resort: it waits for failures, not for time.
            let next_waits_for_failures =
                asked < order.len() && self.links[order[asked]].last_resort;
            let reply = if asked < order.len() && !next_waits_for_failures {
                let wait = self
                    .wait_for(order[asked - 1], audio.audio_ms)
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
                    replied[place] = true;
                    if let Some((main_place, main)) = held.take() {
                        // D-042: the fuller of the cut-looking main answer and the fallback's wins.
                        if words(&t.text) <= words(&main.text) {
                            return Ok(Transcript {
                                engine: id(order[main_place]),
                                passed,
                                ..main
                            });
                        }
                        passed.push(Passed {
                            engine: id(order[main_place]),
                            why: Skip::Failed(cut_error(&main.text, audio.speech_ms)),
                        });
                        return Ok(Transcript {
                            engine: id(order[place]),
                            passed,
                            ..t
                        });
                    }
                    if order[place] != 0 || !looks_cut(&t.text, audio.speech_ms) {
                        return Ok(Transcript {
                            engine: id(order[place]),
                            passed,
                            ..t
                        });
                    }
                    // D-042: the main engine's answer looks cut. Hear from one fallback (the one already
                    // running, else the next one; a slow one brings in the next after its wait as usual) and
                    // keep the fuller answer. With none left to hear from, the main answer stands.
                    if !(0..asked).any(|p| !replied[p]) && together(asked) {
                        let last = asked + 1 == order.len();
                        self.ask(order[asked], asked, &audio, sender(&mut tx, last));
                        asked += 1;
                    }
                    if !(0..asked).any(|p| !replied[p]) {
                        return Ok(Transcript {
                            engine: id(order[place]),
                            passed,
                            ..t
                        });
                    }
                    held = Some((place, t));
                    since = Instant::now();
                    false
                }
                Some((place, Err(e))) => {
                    replied[place] = true;
                    // Only the newest engine failing moves the chain on; an older one may still answer.
                    // Before a last resort, every engine asked so far must have failed.
                    let newest = place + 1 == asked && asked < order.len();
                    let main_failed = order[place] == 0 && together(asked);
                    if !listed[place] && (newest || next_waits_for_failures || asked > place + 1) {
                        passed.push(Passed {
                            engine: id(order[place]),
                            why: Skip::Failed(e.clone()),
                        });
                        listed[place] = true;
                    }
                    errors[place] = Some(e);
                    if errors.iter().all(Option::is_some) {
                        break;
                    }
                    if main_failed {
                        // D-040: the main engine failed; every fallback is asked now.
                        while together(asked) {
                            let last = asked + 1 == order.len();
                            self.ask(order[asked], asked, &audio, sender(&mut tx, last));
                            asked += 1;
                        }
                        since = Instant::now();
                        false
                    } else if next_waits_for_failures {
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
                    listed[asked - 1] = true;
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
        if let Some((main_place, main)) = held {
            // Every fallback failed: the main engine's answer is still the best there is.
            return Ok(Transcript {
                engine: id(order[main_place]),
                passed,
                ..main
            });
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

    /// The step, checked (D-038): an answer that [`checked`] refuses is asked for once more, then given up
    /// as an error (the engine's own text is kept when the step is optional).
    fn run(&self, text: &str, words: Option<&WordList>) -> Result<Transcript, ApiError> {
        let first = self.ask(text, words)?;
        let first_why = match checked(text, &first.text, &self.spec.prompt) {
            Ok(out) => return Ok(Transcript { text: out, ..first }),
            Err(why) => why,
        };
        let again = self.ask(text, words)?;
        match checked(text, &again.text, &self.spec.prompt) {
            Ok(out) => Ok(Transcript {
                text: out,
                request_ms: first.request_ms + again.request_ms,
                retries: first.retries + again.retries + 1,
                tokens_in: first.tokens_in + again.tokens_in,
                tokens_out: first.tokens_out + again.tokens_out,
                ..again
            }),
            Err(why) => Err(ApiError::BadResponse(format!(
                "the format answer looked wrong twice ({first_why}, then {why})"
            ))),
        }
    }

    fn ask(&self, text: &str, words: Option<&WordList>) -> Result<Transcript, ApiError> {
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
                { "role": "user", "content": crate::spec::wrap_transcript(text) },
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

/// The text step's answer, or why it must not be pasted (D-038). The step rewrites what was said: it may
/// drop fillers and take-backs and add list marks, but it never writes nothing for real speech, its own
/// instructions, or a text far longer or shorter than the transcript (an answer to a spoken question or
/// order). Tags the model copied from the wrapped input are removed. One copied prompt line is fine (a
/// speaker may say one of its examples); a short take-back may shrink a lot ("Meeting Monday ko rakh lo.
/// Actually, Tuesday ko." → "Meeting Tuesday ko."), so "far shorter" needs 15+ words said.
fn checked(input: &str, answer: &str, prompt: &str) -> Result<String, &'static str> {
    let out = answer
        .trim()
        .trim_start_matches("<transcript>")
        .trim_end_matches("</transcript>")
        .trim()
        .to_string();
    let said = input.split_whitespace().count();
    let wrote = out.split_whitespace().count();
    if wrote == 0 {
        return if said < 4 { Ok(out) } else { Err("empty") };
    }
    let copied = prompt
        .lines()
        .map(str::trim)
        .filter(|l| l.chars().count() >= 30 && out.contains(l))
        .count();
    if copied >= 2 {
        return Err("its own instructions");
    }
    if wrote > 2 * said + 10 {
        return Err("far longer than what was said");
    }
    if said >= 15 && wrote * 10 < said * 3 {
        return Err("far shorter than what was said");
    }
    Ok(out)
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

    const DEVANAGARI_REPLY: &str =
        r#"{"candidates":[{"content":{"parts":[{"text":"मैं अभी अपना मोबाइल चला रहा हूं"}]}}]}"#;
    const ROMAN_REPLY: &str = r#"{"choices":[{"message":{"content":"Main abhi apna mobile chala raha hoon"}}],"usage":{"prompt_tokens":200,"completion_tokens":12}}"#;

    fn romanize_at(url: String) -> Refine {
        let mut r = spec::romanize().unwrap();
        r.provider.base_url = url;
        r
    }

    #[test]
    fn a_script_answer_without_a_format_step_is_romanized() {
        // Live 2026-11-05 19:48 (--no-format): Flash answered in Devanagari, pasted as a letter-by-letter draft.
        let (gemini_url, _) = server(vec![(200, "", DEVANAGARI_REPLY)]);
        let (groq_url, requests) = server(vec![(200, "", ROMAN_REPLY)]);
        let client = flash_lite_at(gemini_url)
            .with_romanize(romanize_at(groq_url), |var| format!("{var}-value"))
            .unwrap();
        let audio = crate::encode::opus(&vec![0.1; 16_000]).unwrap();
        let t = client.transcribe(&audio).unwrap();
        assert_eq!(t.text, "Main abhi apna mobile chala raha hoon");
        assert_eq!(t.asr.as_deref(), Some("मैं अभी अपना मोबाइल चला रहा हूं"));
        assert_eq!(t.refine_tokens_in, 200);
        let req = requests.recv().unwrap();
        assert!(req.contains("openai/gpt-oss-20b") && req.contains("<transcript>"));
    }

    #[test]
    fn a_roman_answer_makes_no_romanize_request() {
        let (gemini_url, _) = server(vec![(200, "", GEMINI_REPLY)]);
        let (groq_url, requests) = server(vec![]);
        let client = flash_lite_at(gemini_url)
            .with_romanize(romanize_at(groq_url), |var| format!("{var}-value"))
            .unwrap();
        let audio = crate::encode::opus(&vec![0.1; 16_000]).unwrap();
        let t = client.transcribe(&audio).unwrap();
        assert_eq!(t.text, "doodh anday bread aur do kilo aloo");
        assert!(t.asr.is_none());
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn a_script_answer_with_no_roman_step_is_an_error_not_a_draft() {
        let (gemini_url, _) = server(vec![(200, "", DEVANAGARI_REPLY)]);
        let audio = crate::encode::opus(&vec![0.1; 16_000]).unwrap();
        let e = flash_lite_at(gemini_url).transcribe(&audio).unwrap_err();
        assert!(e.to_string().contains("Devanagari"), "{e}");
    }

    #[test]
    fn a_failed_format_step_on_a_script_answer_falls_back_to_romanize() {
        let (gemini_url, _) = server(vec![(200, "", DEVANAGARI_REPLY)]);
        let (format_url, _) = server(vec![(400, "", r#"{"error":{"message":"bad"}}"#)]);
        let (groq_url, _) = server(vec![(200, "", ROMAN_REPLY)]);
        let mut format = spec::format().unwrap();
        format.provider.base_url = format_url;
        let key = |var: &str| format!("{var}-value");
        let client = flash_lite_at(gemini_url)
            .with_format(format, key)
            .unwrap()
            .with_romanize(romanize_at(groq_url), key)
            .unwrap();
        let audio = crate::encode::opus(&vec![0.1; 16_000]).unwrap();
        let t = client.transcribe(&audio).unwrap();
        assert_eq!(t.text, "Main abhi apna mobile chala raha hoon");
        assert!(t.unformatted.is_some());
    }

    #[test]
    fn an_engine_with_its_own_refine_gets_no_romanize_step() {
        let Some(e) = spec::engine("gemini-3.5-transcribe+gptoss20b") else {
            panic!("transcribe engine");
        };
        let client = Client::new(e, |var| format!("{var}-value"))
            .unwrap()
            .with_romanize(spec::romanize().unwrap(), |var| format!("{var}-value"))
            .unwrap();
        assert!(client.romanizer.is_none());
    }

    /// The 2026-11-05 live failure: the format model sent back its own prompt, which was pasted.
    const LEAK: &str = r#"{"choices":[{"message":{"content":"You turn a raw dictation transcript into the text the speaker meant to type, in Roman Urdu / Hinglish.\nOutput only the final text, nothing else."}}]}"#;

    #[test]
    fn the_transcript_goes_in_tags_and_a_leaked_prompt_is_asked_again() {
        let (gemini_url, _) = server(vec![(200, "", GEMINI_REPLY)]);
        let (groq_url, requests) = server(vec![
            (200, "", LEAK),
            (
                200,
                "",
                r#"{"choices":[{"message":{"content":"<transcript>\nDoodh, anday, bread aur 2 kilo aloo.\n</transcript>"}}]}"#,
            ),
        ]);
        let mut format = spec::format().unwrap();
        format.provider.base_url = groq_url;
        let client = flash_lite_at(gemini_url)
            .with_format(format, |var| format!("{var}-value"))
            .unwrap();
        let audio = crate::encode::opus(&vec![0.1; 16_000]).unwrap();
        let t = client.transcribe(&audio).unwrap();
        assert_eq!(t.text, "Doodh, anday, bread aur 2 kilo aloo.");
        assert_eq!(t.unformatted, None);
        assert_eq!(t.retries, 1);
        let req = requests.recv().unwrap();
        assert!(
            req.contains("<transcript>\\ndoodh anday bread aur do kilo aloo\\n</transcript>"),
            "{req}"
        );
    }

    #[test]
    fn a_format_answer_wrong_twice_is_never_pasted() {
        let (gemini_url, _) = server(vec![(200, "", GEMINI_REPLY)]);
        let (groq_url, _) = server(vec![(200, "", LEAK), (200, "", LEAK)]);
        let mut format = spec::format().unwrap();
        format.provider.base_url = groq_url;
        let client = flash_lite_at(gemini_url)
            .with_format(format, |var| format!("{var}-value"))
            .unwrap();
        let audio = crate::encode::opus(&vec![0.1; 16_000]).unwrap();
        let t = client.transcribe(&audio).unwrap();
        // Optional after Flash-Lite: its own text is kept and the console says why.
        assert_eq!(t.text, "doodh anday bread aur do kilo aloo");
        let why = t.unformatted.unwrap();
        assert!(why.contains("its own instructions"), "{why}");
    }

    #[test]
    fn checked_refuses_answers_that_are_not_the_speech() {
        let prompt = spec::format().unwrap().prompt;
        let said = "ना ना रुक जा, तुझे नहीं बोलना था। यह वॉइस किसी और को करनी थी। मुझे अभी तूने पहले जो \
                    रिप्लाई दिया ना, यह मेरे को दोबारा भेज, पहले जो दिया था।";
        let leak = format!("{}\n", &prompt[..300]);
        assert_eq!(checked(said, &leak, &prompt), Err("its own instructions"));
        assert_eq!(checked(said, "", &prompt), Err("empty"));
        assert_eq!(
            checked(
                said,
                "Sorry, I don't have a previous reply to resend.",
                &prompt
            ),
            Err("far shorter than what was said")
        );
        let long = "Yaar ".repeat(80);
        assert_eq!(
            checked("Kya haal hai yaar", &long, &prompt),
            Err("far longer than what was said")
        );
        let right = "Na na ruk ja, tujhe nahi bolna tha. Yeh voice kisi aur ko karni thi. Mujhe abhi tune \
                     pehle jo reply diya na, yeh mere ko dobara bhej, pehle jo diya tha.";
        assert_eq!(checked(said, right, &prompt).as_deref(), Ok(right));
        // A take-back, a list and a filler-only clip are fine.
        assert!(
            checked(
                "theek hai kal paanch baje milte hain nahi nahi chhe baje",
                "Theek hai, kal 6 baje milte hain.",
                &prompt
            )
            .is_ok()
        );
        assert!(
            checked(
                "ammi ne kaha hai doodh anday bread aur do kilo aloo",
                "Ammi ne kaha hai:\n- Doodh\n- Anday\n- Bread\n- 2 kilo aloo",
                &prompt
            )
            .is_ok()
        );
        assert_eq!(checked("umm", "", &prompt).as_deref(), Ok(""));
        // The prompt's own example said aloud (sentence fmt-01), and a short take-back (fmt-10).
        assert!(
            checked(
                "Ammi ne kaha hai market se ye cheezein le aana, doodh, ande, bread aur 2 kilo aloo.",
                "Ammi ne kaha hai market se ye cheezein le aana:\n- Doodh\n- Ande\n- Bread\n- 2 kilo aloo",
                &prompt
            )
            .is_ok()
        );
        assert!(
            checked(
                "Meeting Monday ko rakh lo. Actually, it's Tuesday ko.",
                "Meeting Tuesday ko.",
                &prompt
            )
            .is_ok()
        );
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
    fn the_wait_is_learned_from_answers() {
        // D-017: the spec's wait until ten answers, then twice the median for the clip.
        let (c, _) = chain(vec![Fake::new("a", 0, Ok("x")), Fake::new("b", 0, Ok("y"))]);
        assert_eq!(c.wait_for(0, 5000), WAIT);
        if let Ok(mut t) = c.timings.lock() {
            for _ in 0..crate::deadline::MIN_ANSWERS {
                t.record("a", Duration::from_secs(4), 4000);
            }
        }
        // 1 s per second of audio: a 5 s clip waits 10 s.
        assert_eq!(c.wait_for(0, 5000), Duration::from_secs(10));
        assert_eq!(c.wait_for(1, 5000), WAIT, "each engine learns its own");
    }

    #[test]
    fn answers_teach_the_timings() {
        let (c, _) = chain(vec![Fake::new("a", 0, Ok("x"))]);
        for _ in 0..crate::deadline::MIN_ANSWERS {
            run(&c).0.unwrap();
        }
        // Fast fakes: the learned wait is the 3 s floor, not the spec's.
        assert_eq!(c.wait_for(0, 1000), crate::deadline::MIN_WAIT);
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
    fn the_fallbacks_are_asked_together_when_the_main_engine_fails() {
        let busy = ApiError::Http(503, "high demand".into());
        let (c, n) = chain(vec![
            Fake::new("transcribe", 0, Err(busy.clone())),
            Fake::new("flash", 1_000, Ok("Flash text")),
            Fake::new("lite", 50, Ok("Lite text")),
        ]);
        let (r, took) = run(&c);
        let t = r.unwrap();
        assert_eq!(t.engine, "lite");
        assert_eq!(t.passed, [failed("transcribe", busy)]);
        assert_eq!(calls(&n), [1, 1, 1]);
        assert!(took < WAIT, "Lite does not wait for Flash: {took:?}");
    }

    #[test]
    fn the_fallbacks_are_asked_together_while_the_main_engine_rests() {
        let (c, n) = chain(vec![
            Fake::new("transcribe", 0, Err(daily())),
            Fake::new("flash", 1_000, Ok("Flash text")),
            Fake::new("lite", 50, Ok("Lite text")),
        ]);
        run(&c).0.unwrap();
        let (r, took) = run(&c);
        let t = r.unwrap();
        assert_eq!(t.engine, "lite");
        let resting = Passed {
            engine: "transcribe".into(),
            why: Skip::Resting,
        };
        assert_eq!(t.passed, [resting]);
        assert_eq!(calls(&n), [1, 2, 2]);
        assert!(took < WAIT, "{took:?}");
    }

    #[test]
    fn a_used_up_engine_stays_skipped_after_a_restart() {
        let file =
            std::env::temp_dir().join(format!("sonicscript-rest-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let fakes = || {
            vec![
                Fake::new("transcribe", 0, Err(daily())),
                Fake::new("flash", 0, Ok("Flash text")),
            ]
        };
        let (c, _) = chain(fakes());
        run(&c.with_rest_file(file.clone())).0.unwrap();
        let saved = std::fs::read_to_string(&file).unwrap();
        assert!(saved.contains("\"transcribe\""), "{saved}");
        // A run of another engine alone adds its rest and keeps that one.
        let (lone, _) = chain(vec![Fake::new("lite", 0, Err(daily()))]);
        run(&lone.with_rest_file(file.clone())).0.unwrap_err();
        let saved = std::fs::read_to_string(&file).unwrap();
        assert!(
            saved.contains("\"transcribe\"") && saved.contains("\"lite\""),
            "{saved}"
        );
        let (c, n) = chain(fakes());
        let t = run(&c.with_rest_file(file.clone())).0.unwrap();
        assert_eq!(t.engine, "flash");
        assert_eq!(calls(&n), [0, 1], "the new chain knows the rest");
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn a_daily_rest_lasts_until_midnight_in_los_angeles() {
        let at = |y, m, d, h: i64, min: i64| (days(y, m, d) * 86_400 + h * 3_600 + min * 60) as u64;
        // 20:00 in Pakistan, winter time in California: 1 PM in Pakistan the next day.
        assert_eq!(
            next_pacific_midnight(at(2026, 11, 5, 15, 0)),
            at(2026, 11, 6, 8, 0)
        );
        // Summer time: 07:00 UTC.
        assert_eq!(
            next_pacific_midnight(at(2026, 7, 1, 10, 0)),
            at(2026, 7, 2, 7, 0)
        );
        // The night summer time ends (Sunday 1 November 2026): midnight PDT has passed.
        assert_eq!(
            next_pacific_midnight(at(2026, 11, 1, 7, 30)),
            at(2026, 11, 2, 8, 0)
        );
        // The night it starts (Sunday 8 March 2026).
        assert_eq!(
            next_pacific_midnight(at(2026, 3, 8, 7, 30)),
            at(2026, 3, 8, 8, 0)
        );
        assert_eq!(
            next_pacific_midnight(at(2026, 3, 8, 9, 0)),
            at(2026, 3, 9, 7, 0)
        );
        assert_eq!(civil(days(2028, 2, 29)), (2028, 2, 29));
        let rest = daily_rest(UNIX_EPOCH + Duration::from_secs(at(2026, 11, 5, 15, 0)));
        assert_eq!(rest, Duration::from_secs(17 * 3_600 + 5 * 60));
    }

    fn run_with_speech(
        c: &Chain<Fake>,
        speech_ms: u32,
    ) -> (Result<Transcript, ApiError>, Duration) {
        let audio = Encoded {
            speech_ms: Some(speech_ms),
            ..crate::encode::opus(&vec![0.1; 1_600]).unwrap()
        };
        let t = Instant::now();
        (c.transcribe(&audio), t.elapsed())
    }

    const CUT: &str =
        "Cloud focus cursor click cursor focus keyboard type normally input field focus.";
    const FULL: &str = "Yaar ek baat bata, jab main Claude mein cursor click karta hoon to focus chala jata hai aur                         keyboard se type normally nahi hota, input field focus hi nahi leta.";

    #[test]
    fn a_cut_looking_main_answer_loses_to_a_fuller_fallback() {
        // Live 2026-11-05 17:08: 12 words for 10 s of speech.
        let (c, n) = chain(vec![
            Fake::new("transcribe", 0, Ok(CUT)),
            Fake::new("flash", 50, Ok(FULL)),
            Fake::new("lite", 0, Ok("short")),
        ]);
        let t = run_with_speech(&c, 10_000).0.unwrap();
        assert_eq!((t.engine.as_str(), t.text.as_str()), ("flash", FULL));
        assert!(
            matches!(&t.passed[..], [Passed { engine, why: Skip::Failed(ApiError::BadResponse(m)) }]
                if engine == "transcribe" && m.contains("looked cut")),
            "{:?}",
            t.passed
        );
        assert_eq!(calls(&n), [1, 1, 0], "only the next engine is asked");
    }

    #[test]
    fn a_cut_looking_answer_stays_when_the_fallback_is_no_fuller_or_fails() {
        let (c, _) = chain(vec![
            Fake::new("transcribe", 0, Ok(CUT)),
            Fake::new("flash", 0, Ok("Cloud focus.")),
        ]);
        let t = run_with_speech(&c, 10_000).0.unwrap();
        assert_eq!((t.engine.as_str(), t.passed.len()), ("transcribe", 0));
        let (c, _) = chain(vec![
            Fake::new("transcribe", 0, Ok(CUT)),
            Fake::new("flash", 0, Err(ApiError::Timeout)),
        ]);
        let t = run_with_speech(&c, 10_000).0.unwrap();
        assert_eq!(t.engine, "transcribe");
    }

    #[test]
    fn a_normal_answer_asks_no_one_else() {
        let (c, n) = chain(vec![
            Fake::new("transcribe", 0, Ok(FULL)),
            Fake::new("flash", 0, Ok(FULL)),
        ]);
        // 26 words for 4 s of speech; a short clip (2 s) never counts as cut; nor does an unknown length.
        assert_eq!(run_with_speech(&c, 4_000).0.unwrap().engine, "transcribe");
        let (c2, n2) = chain(vec![
            Fake::new("transcribe", 0, Ok("Haan.")),
            Fake::new("flash", 0, Ok(FULL)),
        ]);
        assert_eq!(run_with_speech(&c2, 2_000).0.unwrap().engine, "transcribe");
        assert_eq!(run(&c2).0.unwrap().engine, "transcribe");
        assert_eq!((calls(&n), calls(&n2)), (vec![1, 0], vec![2, 0]));
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
