"""Terminal recorder for the test set: shows each sentence, records 16 kHz mono, trims silence, saves WAV."""

from __future__ import annotations

import json
import sys
import threading
import time
from pathlib import Path

import numpy as np

from . import audio
from .config import AUDIO_DIR, Sentence, load_sentences

ACCENT = "\033[38;2;245;180;90m"
DIM = "\033[2m"
BOLD = "\033[1m"
OK = "\033[38;2;110;210;160m"
WARN = "\033[38;2;235;205;100m"
RESET = "\033[0m"
BARS = " ▁▂▃▄▅▆▇█"


def _sd():
    try:
        import sounddevice as sd
    except OSError as exc:  # PortAudio missing
        raise SystemExit(f"Audio input not available: {exc}") from exc
    return sd


def list_devices() -> None:
    sd = _sd()
    default_in = sd.default.device[0]
    for i, d in enumerate(sd.query_devices()):
        if d["max_input_channels"] > 0:
            mark = f"{ACCENT}*{RESET}" if i == default_in else " "
            print(f"{mark} {i:>3}  {d['name']}  ({int(d['default_samplerate'])} Hz)")


class Recorder:
    """Records until stop(); keeps a live level for the meter."""

    def __init__(self, device: int | None = None) -> None:
        self.sd = _sd()
        self.device = device
        self.level = 0.0
        self._chunks: list[np.ndarray] = []
        self._stream = None
        self.rate = audio.RATE

    def start(self) -> None:
        self._chunks = []

        def cb(indata, frames, t, status):  # noqa: ARG001
            mono = indata[:, 0].copy()
            self._chunks.append(mono)
            self.level = float(np.sqrt(np.mean(mono**2)))

        try:
            self.rate = audio.RATE
            self._stream = self.sd.InputStream(samplerate=self.rate, channels=1, dtype="float32",
                                               device=self.device, callback=cb)
        except Exception:  # device does not do 16 kHz: record at its own rate, resample on save
            info = self.sd.query_devices(self.device, "input")
            self.rate = int(info["default_samplerate"])
            self._stream = self.sd.InputStream(samplerate=self.rate, channels=1, dtype="float32",
                                               device=self.device, callback=cb)
        self._stream.start()

    def stop(self) -> np.ndarray:
        if self._stream:
            self._stream.stop()
            self._stream.close()
            self._stream = None
        return np.concatenate(self._chunks) if self._chunks else np.zeros(0, dtype=np.float32)

    def play(self, samples: np.ndarray) -> None:
        self.sd.play(samples, audio.RATE)
        self.sd.wait()


def _meter(rec: Recorder, stop: threading.Event) -> None:
    start = time.monotonic()
    while not stop.is_set():
        db = 20 * np.log10(rec.level + 1e-9)
        n = int(np.clip((db + 60) / 60, 0, 1) * 24)
        bar = "".join(BARS[min(8, max(0, (n - i) * 2))] if i < n else " " for i in range(24))
        sys.stdout.write(f"\r  {ACCENT}● rec{RESET} {time.monotonic() - start:5.1f}s  {ACCENT}{bar}{RESET}  "
                         f"{DIM}Enter to stop{RESET} ")
        sys.stdout.flush()
        time.sleep(0.05)
    sys.stdout.write("\r" + " " * 70 + "\r")


def _take(rec: Recorder, out: Path, silence: bool = False) -> np.ndarray | None:
    """One take: Enter to start, Enter to stop. Returns trimmed 16 kHz samples, or None if empty.
    Silence items are kept untrimmed: the point is to send the engine audio with no speech in it."""
    input(f"  {DIM}Enter to start recording…{RESET}")
    rec.start()
    stop = threading.Event()
    t = threading.Thread(target=_meter, args=(rec, stop), daemon=True)
    t.start()
    input()
    samples = rec.stop()
    stop.set()
    t.join()
    if rec.rate != audio.RATE:
        tmp = out.with_suffix(".raw.wav")
        audio.write_wav(tmp, samples, rec.rate)
        audio.resample_to_16k(tmp, out)
        tmp.unlink()
        samples, _ = audio.read_wav(out)
    if silence:
        print(f"  {len(samples) / audio.RATE:.1f}s of no-speech audio")
        return samples
    trimmed = audio.trim_silence(samples)
    if len(trimmed) < audio.RATE * 0.3:
        print(f"  {WARN}No speech heard. Check the mic (`record --list-devices`) and try again.{RESET}")
        return None
    peak = audio.peak_db(trimmed)
    note = f"{WARN}quiet, move closer{RESET}" if peak < -20 else f"{OK}level ok{RESET}"
    if peak > -0.5:
        note = f"{WARN}clipping, move back a little{RESET}"
    print(f"  {len(trimmed) / audio.RATE:.1f}s, peak {peak:.0f} dB, {note}")
    return trimmed


def _review(rec: Recorder, samples: np.ndarray) -> str:
    while True:
        choice = input(f"  {BOLD}Enter{RESET} keep · {BOLD}p{RESET} play · {BOLD}r{RESET} redo · "
                       f"{BOLD}q{RESET} quit: ").strip().lower()
        if choice == "p":
            rec.play(samples)
            continue
        if choice in ("", "r", "q"):
            return choice or "keep"


def record_set(only: list[str] | None, redo: bool, device: int | None, audio_dir: Path = AUDIO_DIR) -> None:
    sentences = load_sentences()
    todo = [s for s in sentences if (not only or s.id in only)
            and (redo or only or not (audio_dir / f"{s.id}.wav").exists())]
    done = len(sentences) - len([s for s in sentences if not (audio_dir / f"{s.id}.wav").exists()])
    print(f"\n{BOLD}SonicScript · test recordings{RESET}  {DIM}{done}/{len(sentences)} recorded, "
          f"{len(todo)} to go{RESET}")
    print(f"{DIM}Say each line naturally, like a voice note to a friend or a prompt to Claude. "
          f"Don't read word by word.{RESET}\n")
    if not todo:
        print("Everything is recorded. Use --redo or --only ID to record again.")
        return
    rec = Recorder(device)
    for n, s in enumerate(todo, 1):
        _show(s, n, len(todo))
        out = audio_dir / f"{s.id}.wav"
        while True:
            samples = _take(rec, out, s.is_silence)
            if samples is None:
                continue
            choice = _review(rec, samples)
            if choice == "q":
                print("Stopped. Run `record` again to continue where you left off.")
                return
            if choice == "keep":
                audio.write_wav(out, samples)
                print(f"  {OK}saved{RESET} {out.name}\n")
                break
    print(f"{OK}Done.{RESET} Next: python -m sonic_eval probe")


def record_free(count: int, device: int | None, audio_dir: Path = AUDIO_DIR) -> None:
    """Unscripted speech: record, then type exactly what you said."""
    path = audio_dir / "free.json"
    items = json.loads(path.read_text(encoding="utf-8")) if path.exists() else []
    rec = Recorder(device)
    print(f"\n{BOLD}Free speech{RESET}  {DIM}say anything you'd normally type; then type what you said.{RESET}\n")
    for _ in range(count):
        sid = f"free-{len(items) + 1:02d}"
        out = audio_dir / f"{sid}.wav"
        samples = _take(rec, out)
        if samples is None or _review(rec, samples) != "keep":
            continue
        text = input("  What you said (Roman Urdu / English): ").strip()
        english = input("  English words in it (comma separated, optional): ").strip()
        if not text:
            continue
        audio.write_wav(out, samples)
        items.append({"id": sid, "category": "free", "text": text,
                      "english": [w.strip() for w in english.split(",") if w.strip()]})
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(items, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"  {OK}saved{RESET} {sid}\n")


def _show(s: Sentence, n: int, total: int) -> None:
    print(f"{DIM}{n}/{total} · {s.id} · {s.category}{RESET}")
    print(f"  {BOLD}{s.say or s.text}{RESET}")
    if s.format_check:  # say it as written, take-backs and fillers included: the formatter must clean them up
        print(f"  {DIM}Say it naturally, with the corrections and fillers as written.{RESET}")
