//! Microphone capture (M1 plan step 5): the default input device through WASAPI shared mode (cpal), mixed to
//! mono, resampled to 16 kHz and Opus-encoded on a worker thread while the user talks, so stopping leaves
//! only the last frame to encode. The audio callback only copies and sends; nothing slow runs there.
//!
//! Opening the mic takes 110-500 ms (Windows wakes the device after ~20 s idle), and that speech would be
//! lost, so dictation keeps one stream set up but not started ([`Ready`], D-036): the key-down only starts it.
//! A stream that is set up but not started does not count as "in use" (no mic icon on the taskbar).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, Sample, SampleFormat, SizedSample};
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
    /// True when it started from a stream [`Ready`] had set up beforehand.
    pub prepared: bool,
}

/// How long a started stream may take to deliver its first audio (normally ~15 ms) before it counts as broken.
const FIRST_AUDIO: Duration = Duration::from_millis(300);

/// A stream that is set up but not started.
struct Prepared {
    stream: cpal::Stream,
    rx: Receiver<Vec<f32>>,
    /// Set by the error callback: the device went away, the default mic changed, or the stream failed.
    broken: Arc<AtomicBool>,
    /// Errors are printed only while recording (a default-mic change while idle is not news).
    live: Arc<AtomicBool>,
    device: String,
    rate: u32,
}

/// Looks up the default mic and sets up a stream on it, without starting it (the slow part of opening the mic).
fn prepare() -> Result<Prepared, String> {
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
    let broken = Arc::new(AtomicBool::new(false));
    let live = Arc::new(AtomicBool::new(false));
    let flags = (broken.clone(), live.clone());
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config.config(), channels, tx, flags),
        SampleFormat::I16 => build::<i16>(&device, config.config(), channels, tx, flags),
        SampleFormat::I32 => build::<i32>(&device, config.config(), channels, tx, flags),
        SampleFormat::U16 => build::<u16>(&device, config.config(), channels, tx, flags),
        other => return Err(format!("{name}: unsupported sample format {other}")),
    }
    .map_err(|e| format!("{name}: {e}"))?;
    Ok(Prepared {
        stream,
        rx,
        broken,
        live,
        device: name,
        rate,
    })
}

/// One microphone stream kept ready for the next recording (D-036). Clones share the same slot.
#[derive(Clone, Default)]
pub struct Ready(Arc<Mutex<Option<Prepared>>>);

impl Ready {
    /// Starts with a stream being prepared in the background.
    pub fn new() -> Self {
        let ready = Self::default();
        ready.refill();
        ready
    }

    /// Prepares the next stream on its own thread (at start-up and after each recording), so the slow part never
    /// runs on a key-down. A failure is left for [`Ready::start`] to report.
    pub fn refill(&self) {
        let slot = self.0.clone();
        std::thread::spawn(move || {
            if let Ok(p) = prepare() {
                let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
                if slot
                    .as_ref()
                    .is_none_or(|old| old.broken.load(Ordering::Relaxed))
                {
                    *slot = Some(p);
                }
            }
        });
    }

    /// Starts the prepared stream; opens the mic the slow way when none is ready yet or the ready one stopped
    /// working (default mic changed, device unplugged, PC resumed from sleep).
    pub fn start(&self) -> Result<Recorder, String> {
        let ready = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(p) = ready.filter(|p| !p.broken.load(Ordering::Relaxed))
            && let Ok(r) = Recorder::begin(p, true)
        {
            return Ok(r);
        }
        Recorder::start()
    }
}

impl Recorder {
    /// Opens the default mic and starts recording (the slow way: [`Ready::start`] is quicker).
    pub fn start() -> Result<Self, String> {
        Self::begin(prepare()?, false)
    }

    /// Starts a prepared stream and waits for its first audio (nothing is lost meanwhile: Windows buffers it), so
    /// a stream that cannot start is found here and not after a silent recording.
    fn begin(p: Prepared, prepared: bool) -> Result<Self, String> {
        let name = &p.device;
        p.live.store(true, Ordering::Relaxed);
        p.stream.play().map_err(|e| format!("{name}: {e}"))?;
        let first = match p.rx.recv_timeout(FIRST_AUDIO) {
            Ok(chunk) if !p.broken.load(Ordering::Relaxed) => chunk,
            _ => {
                p.live.store(false, Ordering::Relaxed);
                return Err(format!("{name}: no audio came from the microphone"));
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let stop = stop.clone();
            let (rx, rate) = (p.rx, p.rate);
            std::thread::spawn(move || worker(rx, first, rate, &stop))
        };
        Ok(Self {
            stream: p.stream,
            stop,
            worker,
            device: p.device,
            rate: p.rate,
            prepared,
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
    (broken, live): (Arc<AtomicBool>, Arc<AtomicBool>),
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
        move |e: cpal::Error| {
            // A glitch (WASAPI flags one on the first audio after a start) or a reroute leaves the stream working.
            if matches!(
                e.kind(),
                ErrorKind::Xrun | ErrorKind::RealtimeDenied | ErrorKind::DeviceChanged
            ) {
                return;
            }
            broken.store(true, Ordering::Relaxed);
            if live.load(Ordering::Relaxed) {
                eprintln!("microphone: {e}");
            }
        },
        None,
    )
}

/// Resamples and encodes chunks as they arrive; after `stop`, takes what is left in the channel and ends.
fn worker(rx: Receiver<Vec<f32>>, first: Vec<f32>, rate: u32, stop: &AtomicBool) -> Recording {
    let mut resampler = Resampler::new(rate);
    let mut samples = Vec::with_capacity(16_000 * 30);
    let mut stream = encode::Stream::new();
    let mut feed = |chunk: Vec<f32>, samples: &mut Vec<f32>| {
        let out = resampler.process(&chunk);
        feed_encoder(&mut stream, &out);
        samples.extend_from_slice(&out);
    };
    feed(first, &mut samples);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Needs a microphone: `cargo test -p sonicscript ready_mic -- --ignored --nocapture`. After 25 s idle
    /// (long enough for Windows to put the mic to sleep) the ready stream starts in well under the 110-500 ms of
    /// opening it, and records real audio.
    #[test]
    #[ignore = "needs a microphone and 30 s"]
    fn ready_mic_starts_at_once_after_idle() {
        let ready = Ready::new();
        std::thread::sleep(Duration::from_secs(25));
        let t = Instant::now();
        let r = ready.start().expect("mic");
        let start_ms = t.elapsed().as_millis();
        assert!(r.prepared, "the prepared stream was not used");
        std::thread::sleep(Duration::from_secs(1));
        let rec = r.stop();
        println!("started in {start_ms} ms, {} samples", rec.samples.len());
        assert!(start_ms < 80, "started in {start_ms} ms");
        assert!(
            (15_000..18_000).contains(&rec.samples.len()),
            "{}",
            rec.samples.len()
        );
        assert!(rec.stream.is_ok());

        // The next one is prepared again after a recording.
        ready.refill();
        std::thread::sleep(Duration::from_secs(2));
        let t = Instant::now();
        let r = ready.start().expect("mic");
        assert!(r.prepared && t.elapsed().as_millis() < 80);
        drop(r.stop());
    }
}
