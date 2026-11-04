//! Short sounds that say the dictation key worked (owner, live try 2026-11-04: "show that my click registered"),
//! until M2's on-screen Voice Ring: a rising chime when recording starts, a falling one when it stops, one low
//! note on cancel or a mic failure. Soft sine notes with smooth edges, built once in memory as WAV and played
//! without waiting (`PlaySoundW`), so the mic still opens at once. `--no-sound` turns them off.

use std::f32::consts::PI;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

#[derive(Clone, Copy)]
pub enum Cue {
    Start,
    Stop,
    Cancel,
}

const RATE: u32 = 44_100;
/// Peak level (full scale = 1): clearly heard, never loud.
const VOLUME: f32 = 0.22;
/// Each note's length; the start chime is short, so it is over before the mic usually opens (125-500 ms).
const NOTE_MS: u32 = 55;

static ON: AtomicBool = AtomicBool::new(true);

pub fn set_enabled(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Plays the cue and returns at once (a newer cue cuts an older one short).
pub fn play(cue: Cue) {
    if ON.load(Ordering::Relaxed) {
        start(cue);
    }
}

/// False when Windows did not accept the sound (no audio device, for one).
fn start(cue: Cue) -> bool {
    static SOUNDS: OnceLock<[Vec<u8>; 3]> = OnceLock::new();
    let sounds = SOUNDS.get_or_init(|| {
        [
            wav(&[(659.3, NOTE_MS), (880.0, NOTE_MS)]), // E5 → A5
            wav(&[(880.0, NOTE_MS), (659.3, NOTE_MS)]), // A5 → E5
            wav(&[(392.0, NOTE_MS * 2)]),               // G4
        ]
    });
    let bytes = &sounds[cue as usize];
    // SAFETY: with SND_MEMORY the first argument points to a whole WAV file in memory; it lives in a static,
    // so it outlasts the asynchronous playback.
    unsafe {
        PlaySoundW(
            bytes.as_ptr().cast(),
            std::ptr::null_mut(),
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        ) != 0
    }
}

/// A 16-bit mono WAV file of the notes (frequency in Hz, length in ms) one after another, each with a
/// raised-cosine fade in and out (no clicks).
fn wav(notes: &[(f32, u32)]) -> Vec<u8> {
    let mut samples: Vec<i16> = Vec::new();
    for &(freq, ms) in notes {
        let n = (RATE * ms / 1000) as usize;
        let fade = n / 4;
        for i in 0..n {
            let edge = i.min(n - 1 - i);
            let env = if edge < fade {
                0.5 - 0.5 * (PI * edge as f32 / fade as f32).cos()
            } else {
                1.0
            };
            let x = (2.0 * PI * freq * i as f32 / RATE as f32).sin() * env * VOLUME;
            samples.push((x * f32::from(i16::MAX)) as i16);
        }
    }
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cue_is_a_short_soft_wav_that_starts_and_ends_quietly() {
        let w = wav(&[(659.3, NOTE_MS), (880.0, NOTE_MS)]);
        let mut r = hound::WavReader::new(std::io::Cursor::new(&w)).unwrap();
        let spec = r.spec();
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (1, RATE, 16)
        );
        let s: Vec<i16> = r.samples::<i16>().map(Result::unwrap).collect();
        assert_eq!(s.len(), (RATE * NOTE_MS / 1000) as usize * 2);
        let peak = s.iter().map(|x| x.unsigned_abs()).max().unwrap();
        let limit = (VOLUME * f32::from(i16::MAX)) as u16 + 1;
        assert!(peak <= limit && peak > limit / 2, "peak {peak}");
        assert!(
            s[0].unsigned_abs() < 50 && s[s.len() - 1].unsigned_abs() < 50,
            "no clicks"
        );
    }

    /// Plays the three cues through the speakers (manual: `cargo test -p sonicscript cue -- --ignored`).
    #[test]
    #[ignore = "plays sound"]
    fn the_cues_play() {
        for cue in [Cue::Start, Cue::Stop, Cue::Cancel] {
            assert!(start(cue), "Windows did not play the cue");
            std::thread::sleep(std::time::Duration::from_millis(600));
        }
    }
}
