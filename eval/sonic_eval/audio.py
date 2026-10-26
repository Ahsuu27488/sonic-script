"""WAV read/write, silence trimming and Opus encoding (through ffmpeg)."""

from __future__ import annotations

import shutil
import subprocess
import wave
from dataclasses import dataclass
from pathlib import Path

import numpy as np

RATE = 16_000


@dataclass(frozen=True)
class EncodedAudio:
    data: bytes
    mime: str
    ext: str
    fmt: str
    duration_s: float


def write_wav(path: Path, samples: np.ndarray, rate: int = RATE) -> None:
    """Write mono int16 PCM."""
    path.parent.mkdir(parents=True, exist_ok=True)
    if samples.dtype.kind == "f":
        pcm = (np.clip(samples, -1.0, 1.0) * 32767).astype(np.int16)
    else:
        pcm = samples.astype(np.int16)
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm.tobytes())


def read_wav(path: Path) -> tuple[np.ndarray, int]:
    """Mono float32 in [-1, 1] and the sample rate."""
    with wave.open(str(path), "rb") as w:
        rate, channels, width = w.getframerate(), w.getnchannels(), w.getsampwidth()
        raw = w.readframes(w.getnframes())
    if width != 2:
        raise ValueError(f"{path}: expected 16-bit PCM, got {width * 8}-bit")
    data = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768
    if channels > 1:
        data = data.reshape(-1, channels).mean(axis=1)
    return data, rate


def wav_duration(path: Path) -> float:
    with wave.open(str(path), "rb") as w:
        return w.getnframes() / w.getframerate()


def trim_silence(samples: np.ndarray, rate: int = RATE, threshold_db: float = -42.0,
                 pad_s: float = 0.15) -> np.ndarray:
    """Cut leading/trailing silence (20 ms frames below threshold), keeping a little padding."""
    frame = int(rate * 0.02)
    if len(samples) < frame:
        return samples
    n = len(samples) // frame
    frames = samples[: n * frame].reshape(n, frame)
    rms = np.sqrt(np.mean(frames.astype(np.float64) ** 2, axis=1)) + 1e-12
    loud = np.nonzero(20 * np.log10(rms) > threshold_db)[0]
    if len(loud) == 0:
        return samples[:0]
    pad = int(pad_s * rate)
    start = max(0, loud[0] * frame - pad)
    end = min(len(samples), (loud[-1] + 1) * frame + pad)
    return samples[start:end]


def peak_db(samples: np.ndarray) -> float:
    if len(samples) == 0:
        return -120.0
    return float(20 * np.log10(np.max(np.abs(samples)) + 1e-12))


def ffmpeg_path() -> str | None:
    return shutil.which("ffmpeg")


def resample_to_16k(src: Path, dst: Path) -> None:
    _ffmpeg(["-i", str(src), "-ac", "1", "-ar", str(RATE), "-sample_fmt", "s16", str(dst)])


def encode(wav_path: Path, fmt: str, cache_dir: Path | None = None) -> EncodedAudio:
    """fmt is 'wav' or 'opusNN' (NN kbps, Ogg container, VoIP tuning). Encoded files are cached."""
    duration = wav_duration(wav_path)
    if fmt == "wav":
        return EncodedAudio(wav_path.read_bytes(), "audio/wav", "wav", fmt, duration)
    if not fmt.startswith("opus") or not fmt[4:].isdigit():
        raise ValueError(f"unknown audio format {fmt!r}")
    out = (cache_dir or wav_path.parent / ".cache") / fmt / (wav_path.stem + ".ogg")
    if not out.exists() or out.stat().st_mtime < wav_path.stat().st_mtime:
        out.parent.mkdir(parents=True, exist_ok=True)
        _ffmpeg(["-i", str(wav_path), "-ac", "1", "-c:a", "libopus", "-b:a", f"{fmt[4:]}k",
                 "-application", "voip", "-f", "ogg", str(out)])
    return EncodedAudio(out.read_bytes(), "audio/ogg", "ogg", fmt, duration)


def _ffmpeg(args: list[str]) -> None:
    exe = ffmpeg_path()
    if not exe:
        raise RuntimeError("ffmpeg not found on PATH (needed for Opus). Install it: winget install Gyan.FFmpeg")
    proc = subprocess.run([exe, "-hide_banner", "-loglevel", "error", "-y", *args],
                          capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f"ffmpeg failed: {proc.stderr.strip()}")
