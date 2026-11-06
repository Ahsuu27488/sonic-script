//! The dictation history (D-032): every dictation keeps its whole recording as Ogg/Opus 16 kbps (about 2 KB per
//! second, 7 MB per hour of speech) and one JSON line saying what came back, so the owner can later hear what
//! was said and compare it with the text. Everything stays on this PC, in `%LOCALAPPDATA%\SonicScript\history`:
//!
//! ```text
//! history.jsonl                      one line per dictation (newest last)
//! audio\2026-11-04\15-30-12-345.ogg  the whole recording, from key-down to key-up
//! ```
//!
//! The lines also hold the timings, so this is the latency log of plan step 9 as well. `--no-history` turns
//! it off. Writing happens after the text is pasted, so it adds nothing to the wait.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sonicscript_core::pipeline::Outcome;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

pub struct History {
    dir: PathBuf,
}

/// One writer of `history.jsonl` at a time in this process: two dictations can finish at the same moment, and
/// the settings window rewrites the file to delete a line (M2 phase 4).
static LINES: Mutex<()> = Mutex::new(());

/// One dictation as the settings window lists it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Item {
    /// The line's Unix time in seconds (with ms): its id for [`History::delete`].
    pub at: f64,
    /// Local time, `2026-11-06T15:30:12`.
    pub time: String,
    pub text: String,
    /// The app it went into (exe name), when known.
    pub app: Option<String>,
    /// The recording, relative to the history folder.
    pub audio: Option<String>,
}

/// What the app knows about a dictation besides the pipeline's [`Outcome`].
pub struct Clip<'a> {
    /// The whole recording (Ogg/Opus), or why it could not be built.
    pub audio: Result<Vec<u8>, String>,
    pub mic_open_ms: u128,
    pub key_up_to_text_ms: u128,
    /// "dictation" or "record" (`--record`, which never pastes).
    pub mode: &'static str,
    /// The engine first in line (the chain's main model, or `--engine`).
    pub main_engine: &'a str,
    pub formatting: bool,
    /// The app in front when the text was pasted (exe name and window class).
    pub app: Option<(String, String)>,
    /// "pasted", "copied: <why>", "failed: <error>" or "off".
    pub paste: String,
    pub paste_ms: Option<u128>,
}

impl History {
    /// `%LOCALAPPDATA%\SonicScript\history`, created when missing.
    pub fn open() -> Result<Self, String> {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or("LOCALAPPDATA is not set")?;
        Self::at(base.join("SonicScript").join("history"))
    }

    pub fn at(dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes the recording and appends its line. Returns the recording's path.
    pub fn save(&self, out: &Outcome, clip: Clip) -> Result<PathBuf, String> {
        let now = local_now();
        let audio_rel = format!("audio/{}/{}.ogg", now.date, now.file);
        let (audio, audio_error) = match &clip.audio {
            Ok(bytes) => {
                let path = self.dir.join(&audio_rel);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
                (Some(audio_rel), None)
            }
            Err(e) => (None, Some(e.clone())),
        };
        let line = line(out, &clip, &now.iso, audio.as_deref(), audio_error);
        let _one = LINES.lock().unwrap_or_else(|e| e.into_inner());
        let index = self.dir.join("history.jsonl");
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&index)
            .map_err(|e| format!("{}: {e}", index.display()))?;
        writeln!(f, "{line}").map_err(|e| format!("{}: {e}", index.display()))?;
        Ok(self.dir.join(audio.unwrap_or_default()))
    }
}

impl History {
    /// The last `n` dictations that typed something, newest first.
    pub fn recent(&self, n: usize) -> Vec<Item> {
        let index = fs::read_to_string(self.dir.join("history.jsonl")).unwrap_or_default();
        index
            .lines()
            .rev()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| {
                let text = v["text"].as_str().unwrap_or_default().trim().to_string();
                (!text.is_empty()).then(|| Item {
                    at: v["at"].as_f64().unwrap_or_default(),
                    time: v["time"].as_str().unwrap_or_default().to_string(),
                    text,
                    app: v["app"].as_str().map(str::to_string),
                    audio: v["audio"].as_str().map(str::to_string),
                })
            })
            .take(n)
            .collect()
    }

    /// Deletes one dictation: its line and its recording. False when no line has that `at`.
    pub fn delete(&self, at: f64) -> Result<bool, String> {
        let _one = LINES.lock().unwrap_or_else(|e| e.into_inner());
        let index = self.dir.join("history.jsonl");
        let old = fs::read_to_string(&index).map_err(|e| format!("{}: {e}", index.display()))?;
        let mut audio = None;
        let mut kept = String::with_capacity(old.len());
        for line in old.lines() {
            let v = serde_json::from_str::<Value>(line).unwrap_or(Value::Null);
            if audio.is_none() && v["at"].as_f64() == Some(at) {
                audio = Some(v["audio"].as_str().map(str::to_string));
                continue;
            }
            kept.push_str(line);
            kept.push('\n');
        }
        let Some(audio) = audio else {
            return Ok(false);
        };
        // The recording first: when it is open in a player, the line stays and the user is told, so nothing
        // is left behind that the list no longer shows. Only a recording inside the history folder: a
        // hand-edited line must not delete anything else.
        if let Some(rel) = audio.filter(|r| r.starts_with("audio/") && !r.contains("..")) {
            match fs::remove_file(self.dir.join(rel)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(format!(
                        "The recording could not be deleted (is it playing?): {e}"
                    ));
                }
            }
        }
        let tmp = index.with_extension("jsonl.tmp");
        fs::write(&tmp, kept).map_err(|e| format!("{}: {e}", tmp.display()))?;
        fs::rename(&tmp, &index).map_err(|e| format!("{}: {e}", index.display()))?;
        Ok(true)
    }
}

/// One history line. Personal data (the words said): it stays in the history folder, never in a log or git.
fn line(
    out: &Outcome,
    clip: &Clip,
    time: &str,
    audio: Option<&str>,
    audio_error: Option<String>,
) -> Value {
    let s = &out.stages;
    let r = |x: f64| (x * 10.0).round() / 10.0;
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default();
    json!({
        "time": time,
        "at": (at * 1000.0).round() / 1000.0,
        "mode": clip.mode,
        "audio": audio,
        "audio_error": audio_error,
        "recorded_s": f64::from(out.audio_ms) / 1000.0,
        "speech_ms": out.speech_ms,
        "sent": out.sent,
        "sent_s": f64::from(out.sent_ms) / 1000.0,
        "bytes_sent": out.bytes,
        "main_engine": clip.main_engine,
        "answered_by": if out.sent && out.error.is_none() { Some(&out.engine) } else { None },
        "passed": out.passed.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "formatting": clip.formatting,
        "unformatted": out.unformatted,
        "raw": if out.sent { Some(&out.raw) } else { None },
        "text": out.text.trim_end(),
        "needs_llm": out.needs_llm,
        "phantom": out.phantom,
        "error": out.error.as_ref().map(ToString::to_string),
        "app": clip.app.as_ref().map(|(exe, _)| exe),
        "window_class": clip.app.as_ref().map(|(_, class)| class),
        "paste": clip.paste,
        "ms": {
            "mic_open": clip.mic_open_ms,
            "key_up_to_text": clip.key_up_to_text_ms,
            "paste": clip.paste_ms,
            "gate": r(s.gate),
            "encode": r(s.encode),
            "transcribe": r(s.transcribe),
            "speech": r(s.request),
            "refine": r(s.refine),
            "clean": r(s.clean),
        },
        "retries": out.retries,
        "tokens": out.tokens,
        "version": env!("CARGO_PKG_VERSION"),
    })
}

struct Now {
    /// `2026-11-04T15:30:12.345` (local time).
    iso: String,
    /// `2026-11-04`
    date: String,
    /// `15-30-12-345`
    file: String,
}

fn local_now() -> Now {
    // SAFETY: GetLocalTime only writes the struct it is given.
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    let date = format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay);
    let clock = [t.wHour, t.wMinute, t.wSecond];
    Now {
        iso: format!(
            "{date}T{:02}:{:02}:{:02}.{:03}",
            clock[0], clock[1], clock[2], t.wMilliseconds
        ),
        file: format!(
            "{:02}-{:02}-{:02}-{:03}",
            clock[0], clock[1], clock[2], t.wMilliseconds
        ),
        date,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_lists_newest_first_and_delete_removes_line_and_recording() {
        let dir = std::env::temp_dir().join(format!("sonicscript-recent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("audio/2026-11-06")).unwrap();
        fs::write(dir.join("audio/2026-11-06/a.ogg"), b"OggS").unwrap();
        fs::write(dir.join("outside.txt"), b"keep").unwrap();
        let lines = [
            r#"{"at":1.0,"time":"t1","text":"pehla","app":"notepad.exe","audio":"audio/2026-11-06/a.ogg"}"#,
            r#"{"at":2.0,"time":"t2","text":"","audio":null}"#,
            r#"{"at":3.0,"time":"t3","text":"teesra","app":null,"audio":"../outside.txt"}"#,
        ];
        fs::write(dir.join("history.jsonl"), lines.join("\n") + "\n").unwrap();
        let h = History::at(dir.clone()).unwrap();
        let items = h.recent(50);
        assert_eq!(
            items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(),
            ["teesra", "pehla"]
        );
        assert_eq!(items[1].app.as_deref(), Some("notepad.exe"));
        assert_eq!(h.recent(1).len(), 1);
        assert!(h.delete(1.0).unwrap());
        assert!(
            !dir.join("audio/2026-11-06/a.ogg").exists(),
            "its recording is gone"
        );
        assert!(!h.delete(1.0).unwrap(), "already gone");
        assert!(h.delete(3.0).unwrap());
        assert!(
            dir.join("outside.txt").exists(),
            "never a file outside audio/"
        );
        let left = fs::read_to_string(dir.join("history.jsonl")).unwrap();
        assert_eq!(left.lines().count(), 1);
        assert!(left.contains(r#""at":2.0"#));
        let _ = fs::remove_dir_all(&dir);
    }

    fn clip(audio: Result<Vec<u8>, String>) -> Clip<'static> {
        Clip {
            audio,
            mic_open_ms: 210,
            key_up_to_text_ms: 2400,
            mode: "dictation",
            main_engine: "gemini-3.5-transcribe+gptoss20b",
            formatting: true,
            app: Some(("notepad.exe".into(), "Notepad".into())),
            paste: "pasted".into(),
            paste_ms: Some(320),
        }
    }

    #[test]
    fn a_dictation_leaves_its_recording_and_one_line() {
        let dir = std::env::temp_dir().join(format!("sonicscript-history-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let h = History::at(dir.clone()).unwrap();
        let out = Outcome {
            text: "Kal 6 baje milte hain. ".into(),
            raw: "کل 6:00 ملتے ہیں".into(),
            sent: true,
            audio_ms: 3200,
            engine: "gemini-3.5-transcribe+gptoss20b".into(),
            ..Outcome::default()
        };
        let path = h.save(&out, clip(Ok(b"OggS fake".to_vec()))).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"OggS fake");
        h.save(&Outcome::default(), clip(Err("mic failed".into())))
            .unwrap();

        let index = fs::read_to_string(dir.join("history.jsonl")).unwrap();
        let lines: Vec<Value> = index
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        let first = &lines[0];
        assert_eq!(first["text"], "Kal 6 baje milte hain.");
        assert_eq!(first["raw"], "کل 6:00 ملتے ہیں");
        assert_eq!(first["recorded_s"], 3.2);
        assert_eq!(first["app"], "notepad.exe");
        assert_eq!(first["ms"]["key_up_to_text"], 2400);
        let rel = first["audio"].as_str().unwrap();
        assert!(rel.starts_with("audio/") && rel.ends_with(".ogg"), "{rel}");
        assert!(dir.join(rel).is_file());
        assert_eq!(lines[1]["audio"], Value::Null);
        assert_eq!(lines[1]["audio_error"], "mic failed");
        assert_eq!(lines[1]["raw"], Value::Null, "nothing sent, nothing heard");
        fs::remove_dir_all(&dir).unwrap();
    }
}
