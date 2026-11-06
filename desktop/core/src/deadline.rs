//! The learned deadline (D-017, M2 phase 6): how long the chain waits for an engine before it also asks the
//! next one, learned from that engine's own recent answers instead of a fixed wait. Each good answer leaves
//! its time per second of audio; once an engine has [`MIN_ANSWERS`], the wait for a clip is twice what its
//! median predicts for that clip, never under [`MIN_WAIT`]. Until then the spec's `wait_ms` is used.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::time::Duration;

/// Answers kept per engine (the newest).
pub const KEEP: usize = 50;
/// Answers needed before the learned wait replaces the spec's.
pub const MIN_ANSWERS: usize = 10;
/// The learned wait is never shorter (D-017: "minimum 3 s").
pub const MIN_WAIT: Duration = Duration::from_secs(3);
/// Clips shorter than this count as this long: a 0.4 s clip still costs a whole request.
const MIN_CLIP_S: f64 = 1.0;

/// Per engine id: milliseconds per second of audio of its last answers, oldest first.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Timings {
    per_s: HashMap<String, VecDeque<f64>>,
}

impl Timings {
    /// Records one good answer: `took` for a clip of `audio_ms`.
    pub fn record(&mut self, id: &str, took: Duration, audio_ms: u32) {
        let secs = (f64::from(audio_ms) / 1000.0).max(MIN_CLIP_S);
        let v = self.per_s.entry(id.to_string()).or_default();
        v.push_back(took.as_secs_f64() * 1000.0 / secs);
        while v.len() > KEEP {
            v.pop_front();
        }
    }

    /// How long to wait for `id` on a clip of `audio_ms` before asking the next engine too.
    pub fn wait(&self, id: &str, audio_ms: u32, spec_wait: Duration) -> Duration {
        let Some(v) = self.per_s.get(id).filter(|v| v.len() >= MIN_ANSWERS) else {
            return spec_wait;
        };
        let mut sorted: Vec<f64> = v.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() / 2];
        let secs = (f64::from(audio_ms) / 1000.0).max(MIN_CLIP_S);
        Duration::from_secs_f64(2.0 * median * secs / 1000.0).max(MIN_WAIT)
    }

    /// The timings in `path` (JSON: id → list of ms per second); none when it is missing or broken.
    pub fn load(path: &Path) -> Self {
        let per_s = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<HashMap<String, Vec<f64>>>(&s).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|(id, v)| {
                let skip = v.len().saturating_sub(KEEP);
                (
                    id,
                    v.into_iter()
                        .skip(skip)
                        .filter(|x| x.is_finite() && *x > 0.0)
                        .collect(),
                )
            })
            .collect();
        Self { per_s }
    }

    /// Writes the timings through a temporary file; a failed write only means they are learned again.
    pub fn save(&self, path: &Path) {
        let plain: HashMap<&String, Vec<f64>> = self
            .per_s
            .iter()
            .map(|(id, v)| (id, v.iter().map(|x| (x * 10.0).round() / 10.0).collect()))
            .collect();
        if let Ok(text) = serde_json::to_string(&plain) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: Duration = Duration::from_secs(4);

    #[test]
    fn the_spec_wait_until_ten_answers() {
        let mut t = Timings::default();
        for _ in 0..MIN_ANSWERS - 1 {
            t.record("flash", Duration::from_millis(500), 5000);
        }
        assert_eq!(t.wait("flash", 5000, SPEC), SPEC);
        assert_eq!(t.wait("other", 5000, SPEC), SPEC);
        t.record("flash", Duration::from_millis(500), 5000);
        assert_ne!(t.wait("flash", 5000, SPEC), SPEC);
    }

    #[test]
    fn twice_the_median_for_the_clip_never_under_three_seconds() {
        let mut t = Timings::default();
        // 400 ms per second of audio, with one slow outlier the median ignores.
        for _ in 0..10 {
            t.record("transcribe", Duration::from_millis(2000), 5000);
        }
        t.record("transcribe", Duration::from_secs(30), 5000);
        // A 5 s clip: 2 x 0.4 s x 5 = 4 s.
        assert_eq!(t.wait("transcribe", 5000, SPEC), Duration::from_secs(4));
        // A 20 s clip: 16 s.
        assert_eq!(t.wait("transcribe", 20_000, SPEC), Duration::from_secs(16));
        // A 2 s clip: 1.6 s, raised to the 3 s floor.
        assert_eq!(t.wait("transcribe", 2000, SPEC), MIN_WAIT);
    }

    #[test]
    fn keeps_the_last_fifty_and_survives_a_restart() {
        let mut t = Timings::default();
        for i in 0..60 {
            t.record("lite", Duration::from_millis(100 * (i + 1)), 1000);
        }
        assert_eq!(t.per_s["lite"].len(), KEEP);
        assert_eq!(t.per_s["lite"][0], 1100.0, "the oldest ten are gone");
        let path =
            std::env::temp_dir().join(format!("sonicscript-timings-{}.json", std::process::id()));
        t.save(&path);
        let back = Timings::load(&path);
        assert_eq!(back.wait("lite", 1000, SPEC), t.wait("lite", 1000, SPEC));
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Timings::load(&path), Timings::default());
        let _ = std::fs::remove_file(&path);
    }
}
