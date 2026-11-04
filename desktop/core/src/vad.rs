//! Voice gate (M1 plan step 2, D-007, D-020): decides whether a clip has speech before any engine sees it,
//! and trims the silence around it. A clip with less than [`MIN_SPEECH_MS`] of speech is never sent, so
//! Whisper gets no chance to invent text.
//!
//! A 20 ms frame counts as speech when it is [`MARGIN_DB`] above the clip's own noise floor (the 10th
//! percentile frame, never below [`MIN_DB`]) and its zero-crossing rate is below [`MAX_ZCR`] (voiced
//! sound; room hiss and fans cross zero far more often). The fixed -42 dB threshold of `eval`
//! `trim_silence` is not enough here: the M0 room-noise clip `si-02` has 3.6 s above -42 dB.

use std::ops::Range;

/// Sample rate of every clip the core handles.
pub const RATE: usize = 16_000;
/// 20 ms frames.
pub const FRAME: usize = RATE / 50;
pub const MARGIN_DB: f64 = 9.0;
pub const MIN_DB: f64 = -50.0;
pub const MAX_ZCR: f64 = 0.15;
pub const MIN_SPEECH_MS: u32 = 300;
/// Kept before the first and after the last speech frame, so soft onsets ("sh", "f") are not cut. 500 ms,
/// not 200 ms: with less silence around the speech Whisper clipped "Let's" and translated two mixed
/// sentences into English (D-022).
pub const PAD: usize = RATE / 2;
const FLOOR_PERCENTILE: usize = 10;

/// What the gate found in a clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    /// Milliseconds of frames that count as speech.
    pub speech_ms: u32,
    /// The samples to send: speech plus [`PAD`] on each side. Empty when no frame is speech.
    pub keep: Range<usize>,
}

impl Detection {
    pub fn has_speech(&self) -> bool {
        self.speech_ms >= MIN_SPEECH_MS
    }
}

/// Mono samples in [-1, 1] at [`RATE`].
pub fn detect(samples: &[f32]) -> Detection {
    let frames: Vec<(f64, f64)> = samples
        .as_chunks::<FRAME>()
        .0
        .iter()
        .map(|f| frame_stats(f))
        .collect();
    if frames.is_empty() {
        return Detection {
            speech_ms: 0,
            keep: 0..0,
        };
    }
    let mut levels: Vec<f64> = frames.iter().map(|(db, _)| *db).collect();
    levels.sort_by(f64::total_cmp);
    let floor = levels[(levels.len() - 1) * FLOOR_PERCENTILE / 100];
    let threshold = (floor + MARGIN_DB).max(MIN_DB);
    let speech: Vec<usize> = frames
        .iter()
        .enumerate()
        .filter(|(_, (db, zcr))| *db > threshold && *zcr < MAX_ZCR)
        .map(|(i, _)| i)
        .collect();
    let (Some(&first), Some(&last)) = (speech.first(), speech.last()) else {
        return Detection {
            speech_ms: 0,
            keep: 0..0,
        };
    };
    let speech_ms = u32::try_from(speech.len() * 20).unwrap_or(u32::MAX);
    let keep = (first * FRAME).saturating_sub(PAD)..((last + 1) * FRAME + PAD).min(samples.len());
    Detection { speech_ms, keep }
}

/// Level in dBFS and zero-crossing rate (share of neighbouring samples with a different sign).
fn frame_stats(frame: &[f32]) -> (f64, f64) {
    let power = frame
        .iter()
        .map(|&s| f64::from(s) * f64::from(s))
        .sum::<f64>()
        / frame.len() as f64;
    let db = 20.0 * (power.sqrt() + 1e-12).log10();
    let crossings = frame
        .windows(2)
        .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
        .count();
    (db, crossings as f64 / (frame.len() - 1) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;
    use std::path::Path;

    /// Deterministic white noise at roughly `rms`.
    fn noise(len: usize, rms: f32, seed: u32) -> Vec<f32> {
        let mut x = seed;
        (0..len)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * rms * 1.73
            })
            .collect()
    }

    /// Voice-like: 140 Hz with a few harmonics, loudness pulsing 4 times a second like syllables.
    fn voice(seconds: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let tone: f32 = (1..=4)
                    .map(|h| (2.0 * PI * 140.0 * h as f32 * t).sin() / h as f32)
                    .sum();
                let syllables = 0.5 + 0.5 * (2.0 * PI * 4.0 * t).sin();
                0.15 * tone * syllables
            })
            .collect()
    }

    fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b.iter().cycle()).map(|(x, y)| x + y).collect()
    }

    #[test]
    fn empty_and_short_clips_have_no_speech() {
        assert!(!detect(&[]).has_speech());
        assert!(!detect(&[0.5; 100]).has_speech());
        assert!(!detect(&vec![0.0; RATE]).has_speech());
    }

    #[test]
    fn room_noise_is_not_speech() {
        assert!(!detect(&noise(3 * RATE, 0.01, 1)).has_speech());
        assert!(!detect(&noise(3 * RATE, 0.1, 2)).has_speech()); // loud hiss
    }

    #[test]
    fn voice_passes_and_silence_around_it_is_trimmed() {
        let quiet = noise(RATE, 0.002, 3);
        let clip = [quiet.as_slice(), &voice(1.0), &quiet].concat();
        let d = detect(&clip);
        assert!(d.has_speech(), "{d:?}");
        assert!(
            d.keep.start >= RATE - PAD - FRAME && d.keep.start <= RATE,
            "{d:?}"
        );
        assert!(
            d.keep.end >= 2 * RATE && d.keep.end <= 2 * RATE + PAD + FRAME,
            "{d:?}"
        );
    }

    #[test]
    fn voice_without_any_pause_or_in_noise_passes() {
        assert!(detect(&voice(1.0)).has_speech());
        assert!(detect(&mix(&voice(2.0), &noise(RATE, 0.02, 4))).has_speech());
    }

    /// The owner's M0 recordings (git-ignored, D-004): both silence clips must give no request and all
    /// 42 speech clips must pass, also without their padding and with the noisy room mixed in.
    #[test]
    fn m0_recordings() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/fixtures/audio");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            eprintln!("skipped: no recordings in {}", dir.display());
            return;
        };
        let mut clips: Vec<(String, Vec<f32>)> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "wav"))
            // The M0 set only: the formatting clips (D-030, `fmt-*`) came later.
            .filter(|p| !p.file_stem().unwrap().to_string_lossy().starts_with("fmt-"))
            .map(|p| {
                (
                    p.file_stem().unwrap().to_string_lossy().into_owned(),
                    read_wav(&p),
                )
            })
            .collect();
        clips.sort_by(|a, b| a.0.cmp(&b.0));
        let room = &clips
            .iter()
            .find(|(id, _)| id == "si-02")
            .expect("si-02.wav")
            .1;
        let louder_room: Vec<f32> = room.iter().map(|s| s * 3.0).collect();
        let mut failures = Vec::new();
        for (id, clip) in &clips {
            if id.starts_with("si-") {
                for (variant, x) in [
                    ("as recorded", clip.clone()),
                    ("+10 dB", clip.iter().map(|s| s * 3.0).collect()),
                ] {
                    let d = detect(&x);
                    if d.has_speech() {
                        failures.push(format!(
                            "{id} {variant}: silence passed ({} ms)",
                            d.speech_ms
                        ));
                    }
                }
                continue;
            }
            let cut = PAD.min(clip.len() / 4);
            let variants = [
                ("as recorded", clip.clone()),
                ("no padding", clip[cut..clip.len() - cut].to_vec()),
                ("room noise mixed in", mix(clip, room)),
                ("room noise +10 dB", mix(clip, &louder_room)),
                ("20 dB quieter", clip.iter().map(|s| s * 0.1).collect()),
            ];
            for (variant, x) in variants {
                let d = detect(&x);
                if !d.has_speech() {
                    failures.push(format!(
                        "{id} {variant}: speech dropped ({} ms)",
                        d.speech_ms
                    ));
                }
            }
        }
        assert_eq!(
            clips.len(),
            44,
            "expected the 44 M0 clips in {}",
            dir.display()
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    fn read_wav(path: &Path) -> Vec<f32> {
        let mut r = hound::WavReader::open(path).unwrap();
        assert_eq!(r.spec().sample_rate as usize, RATE, "{}", path.display());
        r.samples::<i16>()
            .map(|s| f32::from(s.unwrap()) / 32768.0)
            .collect()
    }
}
