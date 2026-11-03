//! Reads the 16 kHz mono WAV files `eval record` writes (16-bit or float) as samples in [-1, 1].

use std::path::Path;

use sonicscript_core::vad::RATE;

pub fn read(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.sample_rate as usize != RATE || spec.channels != 1 {
        return Err(format!(
            "needs 16 kHz mono, got {} Hz, {} channel(s)",
            spec.sample_rate, spec.channels
        ));
    }
    let samples: Result<Vec<f32>, _> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|s| s.map(|s| f32::from(s) / 32768.0))
            .collect(),
        (hound::SampleFormat::Float, 32) => reader.samples::<f32>().collect(),
        (format, bits) => {
            return Err(format!(
                "needs 16-bit or float WAV, got {bits}-bit {format:?}"
            ));
        }
    };
    samples.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, rate: u32, samples: &[i16]) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for &s in samples {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn reads_16_khz_mono_and_refuses_other_rates() {
        let dir = std::env::temp_dir().join(format!("sonicscript-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.wav");
        write(&good, 16_000, &[0, 16384, -32768]);
        assert_eq!(read(&good).unwrap(), vec![0.0, 0.5, -1.0]);
        let bad = dir.join("bad.wav");
        write(&bad, 48_000, &[0]);
        assert!(read(&bad).unwrap_err().contains("48000 Hz"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
