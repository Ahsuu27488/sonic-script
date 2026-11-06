//! The learned deadline (D-017 as changed by D-051, M2 phase 6): how long the chain waits for an engine before
//! it also asks the next one, learned from that engine's own recent answers. Each good answer leaves its time;
//! once an engine has [`MIN_ANSWERS`], the wait is twice its median time, never shorter than the spec's
//! `wait_ms` (so a fallback is never asked earlier than before: no extra free requests spent) and never longer
//! than [`MAX_WAIT`] (under the client's 10 s timeout, so a slow request still brings in the fallback).
//! The clip's length is not used: the owner's history shows Transcribe taking 2.1 s on a 1.5 s clip and 3.7 s
//! on a 35 s one; a time per second of audio asked Flash too early on short clips and never on long ones.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::time::Duration;

/// Answers kept per engine (the newest).
pub const KEEP: usize = 50;
/// Answers needed before the learned wait replaces the spec's.
pub const MIN_ANSWERS: usize = 10;
/// The learned wait never goes past this: the client gives up on a request at 10 s (`api::TIMEOUT`).
pub const MAX_WAIT: Duration = Duration::from_secs(8);

/// Per engine id: the milliseconds its last answers took, oldest first.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Timings {
    took_ms: HashMap<String, VecDeque<f64>>,
}

impl Timings {
    /// Records one good answer that took `took`.
    pub fn record(&mut self, id: &str, took: Duration) {
        let v = self.took_ms.entry(id.to_string()).or_default();
        v.push_back(took.as_secs_f64() * 1000.0);
        while v.len() > KEEP {
            v.pop_front();
        }
    }

    /// How long to wait for `id` before asking the next engine too.
    pub fn wait(&self, id: &str, spec_wait: Duration) -> Duration {
        let Some(v) = self.took_ms.get(id).filter(|v| v.len() >= MIN_ANSWERS) else {
            return spec_wait;
        };
        let mut sorted: Vec<f64> = v.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        let median = sorted[sorted.len() / 2];
        Duration::from_secs_f64(2.0 * median / 1000.0)
            .min(MAX_WAIT)
            .max(spec_wait)
    }

    /// The timings in `path` (JSON: id → list of ms); none when it is missing or broken.
    pub fn load(path: &Path) -> Self {
        let took_ms = std::fs::read_to_string(path)
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
        Self { took_ms }
    }

    /// Writes this run's engines into `path`, keeping the others' (another copy of SonicScript, a CLI run),
    /// through a temporary file of this process; a failed write only means they are learned again.
    pub fn save(&self, path: &Path) {
        let mut all = Self::load(path).took_ms;
        all.extend(self.took_ms.clone());
        let plain: HashMap<&String, Vec<f64>> = all
            .iter()
            .map(|(id, v)| (id, v.iter().map(|x| x.round()).collect()))
            .collect();
        if let Ok(text) = serde_json::to_string(&plain) {
            let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
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

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_spec_wait_until_ten_answers() {
        let mut t = Timings::default();
        for _ in 0..MIN_ANSWERS - 1 {
            t.record("flash", ms(3000));
        }
        assert_eq!(t.wait("flash", SPEC), SPEC);
        assert_eq!(t.wait("other", SPEC), SPEC);
        t.record("flash", ms(3000));
        assert_eq!(t.wait("flash", SPEC), ms(6000));
    }

    #[test]
    fn twice_the_median_never_under_the_spec_nor_over_the_cap() {
        // The owner's Transcribe: 2.1-3.7 s whatever the clip's length, one slow outlier.
        let mut t = Timings::default();
        for took in [2100, 2500, 2400, 2700, 3600, 3700, 2300, 2600, 2500, 2200] {
            t.record("transcribe", ms(took));
        }
        t.record("transcribe", ms(30_000));
        // Median 2.5 s: Flash is asked after 5 s, not 4 s, on every clip, short or long.
        assert_eq!(t.wait("transcribe", SPEC), ms(5000));
        // A fast engine never waits less than the spec says (no fallback asked earlier than before).
        let mut fast = Timings::default();
        for _ in 0..10 {
            fast.record("lite", ms(900));
        }
        assert_eq!(fast.wait("lite", SPEC), SPEC);
        // A slow one stops at the cap, under the 10 s timeout.
        let mut slow = Timings::default();
        for _ in 0..10 {
            slow.record("flash", ms(7000));
        }
        assert_eq!(slow.wait("flash", SPEC), MAX_WAIT);
    }

    #[test]
    fn keeps_the_last_fifty_and_merges_with_the_file() {
        let mut t = Timings::default();
        for i in 0..60 {
            t.record("lite", ms(100 * (i + 1)));
        }
        assert_eq!(t.took_ms["lite"].len(), KEEP);
        assert_eq!(t.took_ms["lite"][0], 1100.0, "the oldest ten are gone");
        let path =
            std::env::temp_dir().join(format!("sonicscript-timings-{}.json", std::process::id()));
        // Another run's engine is in the file already: it stays.
        std::fs::write(&path, r#"{"groq":[300,310]}"#).unwrap();
        t.save(&path);
        let back = Timings::load(&path);
        assert_eq!(back.wait("lite", SPEC), t.wait("lite", SPEC));
        assert_eq!(back.took_ms["groq"].len(), 2);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Timings::load(&path), Timings::default());
        let _ = std::fs::remove_file(&path);
    }
}
