//! Times each part of opening the microphone, the way `audio::Recorder::start` does it (STATUS known gap 8).
//! `cargo run -q --release -p sonicscript --example mic_open -- [trials] [gap_ms] [--sound] [--reuse]`
//! `--sound` plays a short chime just before, like dictation does; `--reuse` looks the device up once;
//! `--cache` also keeps its format; `--prebuild` sets the stream up ahead, as `audio::Ready` does (D-036).
//! `HOLD_MS` (env) sets how long each trial records (default 300 ms).

use std::sync::mpsc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

#[cfg(windows)]
fn chime() {
    use std::sync::OnceLock;
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    let wav = WAV.get_or_init(|| {
        let rate = 44_100u32;
        let n = rate as usize / 9;
        let mut data = Vec::with_capacity(n * 2);
        for i in 0..n {
            let t = i as f32 / rate as f32;
            let env = (1.0 - i as f32 / n as f32).powi(2);
            let s = (t * 880.0 * std::f32::consts::TAU).sin() * 0.1 * env;
            data.extend_from_slice(&((s * 32767.0) as i16).to_le_bytes());
        }
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&rate.to_le_bytes());
        w.extend_from_slice(&(rate * 2).to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(data.len() as u32).to_le_bytes());
        w.extend_from_slice(&data);
        w
    });
    // SAFETY: SND_MEMORY with a whole WAV file that lives in a static.
    unsafe {
        PlaySoundW(
            wav.as_ptr().cast(),
            std::ptr::null_mut(),
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let nums: Vec<u64> = args.iter().filter_map(|a| a.parse().ok()).collect();
    let trials = nums.first().copied().unwrap_or(10);
    let gap = Duration::from_millis(nums.get(1).copied().unwrap_or(1500));
    let sound = args.iter().any(|a| a == "--sound");
    let reuse = args.iter().any(|a| a == "--reuse");
    let cache = args.iter().any(|a| a == "--cache");
    let prebuild = args.iter().any(|a| a == "--prebuild");
    let host = cpal::default_host();
    let kept = (reuse || cache || prebuild).then(|| host.default_input_device().expect("no mic"));
    let kept_config = (cache || prebuild).then(|| {
        kept.as_ref()
            .unwrap()
            .default_input_config()
            .expect("config")
    });
    let (tx, mut rx) = mpsc::channel::<(Instant, usize)>();
    let _ = &tx;
    let build = |device: &cpal::Device,
                 config: &cpal::SupportedStreamConfig,
                 tx: mpsc::Sender<(Instant, usize)>| {
        let mut first = true;
        device
            .build_input_stream(
                config.config(),
                move |data: &[f32], _: &_| {
                    if first {
                        first = false;
                        let _ = tx.send((Instant::now(), data.len()));
                    }
                },
                |e| eprintln!("mic: {e}"),
                None,
            )
            .expect("build")
    };
    let mut ready = prebuild.then(|| {
        build(
            kept.as_ref().unwrap(),
            kept_config.as_ref().unwrap(),
            tx.clone(),
        )
    });
    if prebuild {
        std::thread::sleep(gap);
    }
    println!(
        "trial  chime  device  config  build   play  total | first data after play (ms), first chunk frames"
    );
    for i in 0..trials {
        let t0 = Instant::now();
        if sound {
            #[cfg(windows)]
            chime();
        }
        let chime_ms = ms(t0);
        let t = Instant::now();
        let device = match &kept {
            Some(d) => d.clone(),
            None => host.default_input_device().expect("no mic"),
        };
        let device_ms = ms(t);
        let t = Instant::now();
        let config = match &kept_config {
            Some(c) => *c,
            None => device.default_input_config().expect("config"),
        };
        let config_ms = ms(t);
        let t = Instant::now();
        let stream = match ready.take() {
            Some(s) => s,
            None => {
                let (t2, r2) = mpsc::channel();
                rx = r2;
                build(&device, &config, t2)
            }
        };
        let build_ms = ms(t);
        let t = Instant::now();
        stream.play().expect("play");
        let play_ms = ms(t);
        let played = Instant::now();
        let total = ms(t0);
        let (first_ms, frames) = match rx.recv_timeout(Duration::from_secs(2)) {
            Ok((at, n)) => (
                at.duration_since(played).as_secs_f64() * 1000.0,
                n / usize::from(config.channels()),
            ),
            Err(_) => (f64::NAN, 0),
        };
        println!(
            "{i:>5} {chime_ms:>6.1} {device_ms:>7.1} {config_ms:>7.1} {build_ms:>6.1} {play_ms:>6.1} {total:>6.1} | {first_ms:>6.1} {frames:>6}"
        );
        std::thread::sleep(Duration::from_millis(
            std::env::var("HOLD_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
        ));
        drop(stream);
        if prebuild {
            let (t2, r2) = mpsc::channel();
            rx = r2;
            ready = Some(build(&device, &config, t2));
        }
        std::thread::sleep(gap);
    }
}
