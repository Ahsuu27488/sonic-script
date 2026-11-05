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
    /// The user's words for the prompt's `{word_list}` slot (`words::WordList::prompt_line`), or "".
    words: String,
}

impl Gemini {
    /// Never logs or prints the key.
    pub fn new(engine: GeminiEngine, key: String) -> Result<Self, ApiError> {
        let key = api::check_key(&key, &engine.provider.key_env)?;
        Ok(Self {
            http: api::client()?,
            engine,
            key,
            words: String::new(),
        })
    }

    /// The user's words for the prompt (a `words::WordList::prompt_line`); the speech-to-text model has no
    /// prompt and ignores them.
    pub fn set_words(&mut self, line: String) {
        self.words = line;
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
            let prompt = crate::spec::with_words(prompt, e.words_at, &self.words);
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
    strip_stamps(&strip_timeline(t))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Removes subtitle-style time ranges ("0:00 - 0:05", "0:05 - 0:10", ...) that Gemini sometimes puts into
/// a long clip's transcript (live 2026-11-05 19:48, D-039). Only a running timeline counts: two or more
/// ranges, the first from 0:00, each starting where the one before ended; a time the speaker said
/// ("3:00 - 4:00") stays.
fn strip_timeline(text: &str) -> String {
    let mut ranges = Vec::new(); // (start byte, end byte, from s, to s)
    let mut i = 0;
    while i < text.len() {
        let word_start = !text[..i].ends_with(|c: char| c.is_ascii_digit() || c == ':');
        if let Some((from, to, len)) = time_range(&text[i..]).filter(|_| word_start) {
            ranges.push((i, i + len, from, to));
            i += len;
            continue;
        }
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    let timeline =
        ranges.len() >= 2 && ranges[0].2 == 0 && ranges.windows(2).all(|w| w[1].2 == w[0].3);
    if !timeline {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, end, ..) in ranges {
        out.push_str(&text[last..start]);
        out.push(' ');
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

/// Removes running single time stamps ("kya 0:02 chahata tha? ... aur 0:08 open code ..."), the other way
/// Gemini marks time in a long answer (live 2026-11-05 19:56, D-040): three or more, the first at most 0:10,
/// each later than the one before and at most 30 s after it. Spoken times ("6:00, 7:00 aur 8:00") stay.
fn strip_stamps(text: &str) -> String {
    let mut stamps = Vec::new(); // (start byte, end byte, s)
    let mut i = 0;
    while i < text.len() {
        let word_start = !text[..i].ends_with(|c: char| c.is_ascii_digit() || c == ':');
        if let Some((.., len)) = time_range(&text[i..]).filter(|_| word_start) {
            i += len; // a range is the other pass's business
            continue;
        }
        if let Some((at, len)) = clock(&text[i..]).filter(|_| word_start) {
            stamps.push((i, i + len, at));
            i += len;
            continue;
        }
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    let running = stamps.len() >= 3
        && stamps[0].2 <= 10
        && stamps
            .windows(2)
            .all(|w| w[1].2 > w[0].2 && w[1].2 - w[0].2 <= 30);
    if !running {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, end, _) in stamps {
        out.push_str(&text[last..start]);
        out.push(' ');
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

/// "M:SS - M:SS" at the start of `s` (any spaces around a hyphen or an en dash): (from s, to s, bytes).
fn time_range(s: &str) -> Option<(u32, u32, usize)> {
    let (from, a) = clock(s)?;
    let rest = s[a..].trim_start();
    let dash = ['-', '\u{2013}']
        .into_iter()
        .find(|&d| rest.starts_with(d))?;
    let rest = rest[dash.len_utf8()..].trim_start();
    let (to, b) = clock(rest)?;
    Some((from, to, s.len() - rest.len() + b))
}

/// "M:SS" or "MM:SS" at the start of `s`, not followed by another digit: (seconds, bytes).
fn clock(s: &str) -> Option<(u32, usize)> {
    let b = s.as_bytes();
    let m = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if !(1..=2).contains(&m) || b.get(m) != Some(&b':') {
        return None;
    }
    let sec = &b[m + 1..];
    if sec.len() < 2
        || !sec[..2].iter().all(u8::is_ascii_digit)
        || sec.get(2).is_some_and(u8::is_ascii_digit)
    {
        return None;
    }
    let minutes: u32 = s[..m].parse().ok()?;
    let seconds: u32 = s[m + 1..m + 3].parse().ok()?;
    Some((minutes * 60 + seconds, m + 3))
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
    fn a_running_timeline_is_removed_but_a_spoken_time_stays() {
        // Live 2026-11-05 19:48 (Flash, 57 s clip), shortened.
        let raw = "बड़ी ट्रांसक्रिप्ट 0:00 - 0:05में इसने थोड़ी गलतियां की हैं। और मैं 0:05 - 0:10अभी अपना \
                   मोबाइल चला रहा हूं 0:10 \u{2013} 0:15 तो";
        assert_eq!(
            clean(raw),
            "बड़ी ट्रांसक्रिप्ट में इसने थोड़ी गलतियां की हैं। और मैं अभी अपना मोबाइल चला रहा हूं तो"
        );
        for kept in [
            "Meeting 3:00 - 4:00 tak hai.",
            "Pehla 0:00 - 0:05 tha.",
            "0:00 - 0:05 aur phir 0:20 - 0:25 dekho.",
            "Score 10:00-12:301 tha.",
        ] {
            assert_eq!(clean(kept), kept);
        }
    }

    #[test]
    fn running_single_stamps_are_removed_but_spoken_times_stay() {
        // Live 2026-11-05 19:56 (Flash, 41 s clip), shortened.
        let raw = "You know what? Main hamesha se kya 0:02 chahata tha? Ke main Hermes ka aur 0:08 open code ka                    tujhe is tarah se control 0:13 doon jaisey ke anti-gravity ka 0:15 tu browser mein leta hai.";
        assert_eq!(
            clean(raw),
            "You know what? Main hamesha se kya chahata tha? Ke main Hermes ka aur open code ka tujhe is tarah se control doon jaisey ke anti-gravity ka tu browser mein leta hai."
        );
        for kept in [
            "Kal 6:00, 7:00 aur 8:00 baje teen meetings hain.",
            "Score 0:02 tha, phir 0:05.",
            "Lap 0:05, 0:09, 0:08 tha.",
            "Pehle 0:05, phir 0:50, phir 1:00.",
        ] {
            assert_eq!(clean(kept), kept);
        }
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
