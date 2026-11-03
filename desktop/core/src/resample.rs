//! Microphone rate → 16 kHz (M1 plan step 5). Windows shared-mode capture runs at the device's mix rate
//! (usually 48 kHz), the pipeline at [`RATE`]. A windowed-sinc low-pass is evaluated at every output point,
//! so any input rate works (48, 44.1, 32 kHz ...) and nothing above 7.2 kHz folds back into the speech band.
//! Streaming: feed chunks as they arrive; the last few input samples wait for the next chunk or [`flush`].
//!
//! [`flush`]: Resampler::flush

use crate::vad::RATE;

/// Zero crossings of the sinc on each side, in output samples. With the Blackman window (stop band about
/// -70 dB) 32 gives a ~3 kHz transition band around the cut-off, at ~200 multiply-adds per output sample
/// from 48 kHz (3 M per second of audio).
const ZEROS: f64 = 32.0;
/// Cut-off as a share of the output Nyquist (8 kHz): 0.9 → 7.2 kHz, the transition band ends at 8 kHz.
const CUTOFF: f64 = 0.9;

pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Kernel half-width in input samples.
    half: f64,
    /// Input not fully used yet; `buf[0]` is input sample number `offset`.
    buf: Vec<f32>,
    offset: usize,
    /// Position of the next output sample, in input samples since the start.
    next: f64,
}

impl Resampler {
    pub fn new(in_rate: u32) -> Self {
        let step = f64::from(in_rate) / RATE as f64;
        Self {
            step,
            half: ZEROS * step.max(1.0),
            buf: Vec::new(),
            offset: 0,
            next: 0.0,
        }
    }

    /// Returns the 16 kHz samples that the input so far fully determines.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        self.buf.extend_from_slice(input);
        self.run(false)
    }

    /// The rest, with silence assumed after the last input sample.
    pub fn flush(&mut self) -> Vec<f32> {
        self.run(true)
    }

    fn run(&mut self, flush: bool) -> Vec<f32> {
        let end = (self.offset + self.buf.len()) as f64;
        let mut out = Vec::new();
        if self.step == 1.0 {
            out.extend_from_slice(&self.buf[(self.next as usize - self.offset)..]);
            self.next = end;
        } else {
            let limit = if flush { end } else { end - self.half };
            while self.next < limit {
                out.push(self.sample_at(self.next));
                self.next += self.step;
            }
        }
        // Keep what the next outputs still need.
        let keep_from = ((self.next - self.half).floor().max(0.0) as usize).max(self.offset);
        self.buf
            .drain(..(keep_from - self.offset).min(self.buf.len()));
        self.offset = keep_from;
        out
    }

    fn sample_at(&self, t: f64) -> f32 {
        // Low-pass at CUTOFF × the output Nyquist, in cycles per input sample.
        let fc = CUTOFF * 0.5 / self.step.max(1.0);
        let lo = (t - self.half).ceil().max(self.offset as f64) as usize;
        let hi =
            ((t + self.half).floor() as usize).min(self.offset + self.buf.len().saturating_sub(1));
        let (mut acc, mut norm) = (0.0f64, 0.0f64);
        for i in lo..=hi {
            let x = i as f64 - t;
            let w = 2.0 * fc * sinc(2.0 * fc * x) * blackman(x / self.half);
            acc += w * f64::from(self.buf[i - self.offset]);
            norm += w;
        }
        // Dividing by the kernel's sum keeps the gain at 1 near the edges, where part of the kernel is missing.
        if norm.abs() < 1e-9 {
            0.0
        } else {
            (acc / norm) as f32
        }
    }
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

/// Blackman window on [-1, 1].
fn blackman(x: f64) -> f64 {
    if x.abs() >= 1.0 {
        return 0.0;
    }
    let a = std::f64::consts::PI * (x + 1.0);
    0.42 - 0.5 * a.cos() + 0.08 * (2.0 * a).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, hz: f64, seconds: f64) -> Vec<f32> {
        (0..(seconds * f64::from(rate)) as usize)
            .map(|i| (0.5 * (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin()) as f32)
            .collect()
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    fn resample_in_chunks(rate: u32, input: &[f32], chunk: usize) -> Vec<f32> {
        let mut r = Resampler::new(rate);
        let mut out = Vec::new();
        for c in input.chunks(chunk) {
            out.extend(r.process(c));
        }
        out.extend(r.flush());
        out
    }

    #[test]
    fn keeps_speech_band_tones_and_the_length() {
        for rate in [48_000, 44_100, 32_000] {
            let input = tone(rate, 1000.0, 1.0);
            let out = resample_in_chunks(rate, &input, 480);
            assert!(
                out.len().abs_diff(RATE) <= 1,
                "{rate}: {} samples",
                out.len()
            );
            let mid = &out[1000..out.len() - 1000];
            assert!(
                (rms(mid) / rms(&input) - 1.0).abs() < 0.01,
                "{rate}: gain {}",
                rms(mid) / rms(&input)
            );
            // Compare with an ideal 1 kHz sine at 16 kHz.
            let ideal = tone(RATE as u32, 1000.0, 1.0);
            let err: Vec<f32> = mid.iter().zip(&ideal[1000..]).map(|(a, b)| a - b).collect();
            assert!(rms(&err) < 0.005, "{rate}: error rms {}", rms(&err));
        }
    }

    #[test]
    fn removes_tones_above_8_khz_instead_of_folding_them_down() {
        // 12 kHz at 48 kHz would alias to 4 kHz without the low-pass.
        let out = resample_in_chunks(48_000, &tone(48_000, 12_000.0, 1.0), 1024);
        assert!(
            rms(&out[1000..out.len() - 1000]) < 0.001,
            "rms {}",
            rms(&out)
        );
    }

    #[test]
    fn chunk_size_does_not_change_the_result() {
        let input = tone(48_000, 440.0, 0.5);
        let a = resample_in_chunks(48_000, &input, 7);
        let b = resample_in_chunks(48_000, &input, 4800);
        assert_eq!(a.len(), b.len());
        assert!(a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6));
    }

    #[test]
    fn sixteen_khz_passes_through() {
        let input = tone(16_000, 300.0, 0.1);
        assert_eq!(resample_in_chunks(16_000, &input, 100), input);
    }
}
