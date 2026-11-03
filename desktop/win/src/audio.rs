//! Microphone capture (M1 plan step 5): the default input device through WASAPI shared mode (cpal), mixed to
//! mono, resampled to 16 kHz and Opus-encoded on a worker thread while the user talks, so stopping leaves
//! only the last frame to encode. The audio callback only copies and sends; nothing slow runs there.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use sonicscript_core::encode::{self, EncodeError};
use sonicscript_core::resample::Resampler;

/// What was recorded, at 16 kHz, with the encoder that has already seen all of it.
pub struct Recording {
    pub samples: Vec<f32>,
    pub stream: Result<encode::Stream, EncodeError>,
}

/// An open microphone. Drop it or call [`Recorder::stop`] to close it.
pub struct Recorder {
    stream: cpal::Stream,
    stop: Arc<AtomicBool>,
    worker: JoinHandle<Recording>,
    pub device: String,
    pub rate: u32,
}

impl Recorder {
    pub fn start() -> Result<Self, String> {
        let device = cpal::default_host()
            .default_input_device()
            .ok_or("no microphone found (Windows sound settings → Input)")?;
        let name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "microphone".into());
        let config = device
            .default_input_config()
            .map_err(|e| format!("{name}: {e}"))?;
        let rate = config.sample_rate();
        let channels = usize::from(config.channels());
        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        let stream = match config.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, config.config(), channels, tx),
            SampleFormat::I16 => build::<i16>(&device, config.config(), channels, tx),
            SampleFormat::I32 => build::<i32>(&device, config.config(), channels, tx),
            SampleFormat::U16 => build::<u16>(&device, config.config(), channels, tx),
            other => return Err(format!("{name}: unsupported sample format {other}")),
        }
        .map_err(|e| format!("{name}: {e}"))?;
        stream.play().map_err(|e| format!("{name}: {e}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let stop = stop.clone();
            std::thread::spawn(move || worker(rx, rate, &stop))
        };
        Ok(Self {
            stream,
            stop,
            worker,
            device: name,
            rate,
        })
    }

    pub fn stop(self) -> Recording {
        drop(self.stream);
        self.stop.store(true, Ordering::Relaxed);
        self.worker.join().unwrap_or_else(|_| Recording {
            samples: Vec::new(),
            stream: Err(EncodeError::from(std::io::Error::other(
                "recording thread panicked",
            ))),
        })
    }
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    tx: mpsc::Sender<Vec<f32>>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device.build_input_stream(
        config,
        move |data: &[T], _: &_| {
            let mono = data
                .chunks(channels.max(1))
                .map(|frame| {
                    frame.iter().map(|&s| f32::from_sample(s)).sum::<f32>() / frame.len() as f32
                })
                .collect();
            let _ = tx.send(mono);
        },
        |e| eprintln!("microphone: {e}"),
        None,
    )
}

/// Resamples and encodes chunks as they arrive; after `stop`, takes what is left in the channel and ends.
fn worker(rx: Receiver<Vec<f32>>, rate: u32, stop: &AtomicBool) -> Recording {
    let mut resampler = Resampler::new(rate);
    let mut samples = Vec::with_capacity(16_000 * 30);
    let mut stream = encode::Stream::new();
    let mut feed = |chunk: Vec<f32>, samples: &mut Vec<f32>| {
        let out = resampler.process(&chunk);
        feed_encoder(&mut stream, &out);
        samples.extend_from_slice(&out);
    };
    loop {
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(chunk) => feed(chunk, &mut samples),
            Err(RecvTimeoutError::Timeout) if !stop.load(Ordering::Relaxed) => {}
            Err(_) => break,
        }
    }
    while let Ok(chunk) = rx.try_recv() {
        feed(chunk, &mut samples);
    }
    let tail = resampler.flush();
    feed_encoder(&mut stream, &tail);
    samples.extend_from_slice(&tail);
    Recording { samples, stream }
}

/// Keeps the first encoder error; the pipeline reports it when the clip is sent.
fn feed_encoder(stream: &mut Result<encode::Stream, EncodeError>, samples: &[f32]) {
    if let Ok(s) = stream
        && let Err(e) = s.push(samples)
    {
        *stream = Err(e);
    }
}
