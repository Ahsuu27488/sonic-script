"""End to end without network: synthetic recordings -> encode -> fake engine -> results -> HTML report."""

import json

import numpy as np
import pytest

from sonic_eval import audio, bench, report
from sonic_eval.config import Sentence, load_prompt, load_providers, load_sentences
from sonic_eval.engines import build_engines
from sonic_eval.ratelimit import RateLimiter

needs_ffmpeg = pytest.mark.skipif(audio.ffmpeg_path() is None, reason="ffmpeg not installed")


def tone(seconds=1.5, lead=0.5):
    t = np.arange(int(audio.RATE * seconds)) / audio.RATE
    sig = 0.3 * np.sin(2 * np.pi * 220 * t).astype(np.float32)
    pad = np.zeros(int(audio.RATE * lead), dtype=np.float32)
    return np.concatenate([pad, sig, pad])


def test_trim_silence_cuts_both_ends():
    trimmed = audio.trim_silence(tone(1.0, lead=0.8))
    assert 1.0 <= len(trimmed) / audio.RATE <= 1.4


def test_trim_silence_on_silence_is_empty():
    assert len(audio.trim_silence(np.zeros(audio.RATE, dtype=np.float32))) == 0


def test_wav_roundtrip(tmp_path):
    p = tmp_path / "a.wav"
    audio.write_wav(p, tone())
    data, rate = audio.read_wav(p)
    assert rate == audio.RATE and abs(len(data) / rate - 2.5) < 0.01


@needs_ffmpeg
def test_opus_is_much_smaller_than_wav(tmp_path):
    p = tmp_path / "a.wav"
    audio.write_wav(p, tone(4.0))
    wav = audio.encode(p, "wav")
    opus = audio.encode(p, "opus16", tmp_path / "cache")
    assert opus.mime == "audio/ogg" and opus.data[:4] == b"OggS"
    assert len(opus.data) * 5 < len(wav.data)


def test_spec_files_load():
    sentences = load_sentences()
    assert len(sentences) >= 40
    assert len({s.id for s in sentences}) == len(sentences)
    providers = load_providers()
    for e in providers["engines"]:
        for key in ("prompt", "hint"):
            if e.get(key):
                assert load_prompt(e[key])
    assert "{word_list}" not in load_prompt("transcribe-roman-v1.md", ["Taskvane"])


@needs_ffmpeg
def test_fake_run_produces_results_and_report(tmp_path):
    audio_dir = tmp_path / "audio"
    sentences = [Sentence("mx-01", "mixed-tech", "PR merge kardo aur deployment check karlo.",
                          ["PR", "merge", "deployment"]),
                 Sentence("ur-06", "urdu", "Shukriya bhai, aap ne bohat madad ki.", [])]
    for s in sentences:
        audio.write_wav(audio_dir / f"{s.id}.wav", tone())
    providers = {"providers": {}, "engines": [
        {"id": "fake-good", "kind": "fake", "delay_ms": 3},
        {"id": "fake-urdu", "kind": "fake", "delay_ms": 1, "text": "پی آر مرج کردو"},
    ]}
    engines, _ = build_engines(providers, None, RateLimiter())
    run_dir = tmp_path / "results" / "t"
    results = bench.run(engines, sentences, ["wav", "opus16"], 2, run_dir, audio_dir, log=lambda *a: None)

    rows = bench.read_rows(results)
    assert len(rows) == 2 * 2 * 2 * 2
    summary = bench.summarise(rows)
    good = next(s for s in summary if s["engine"] == "fake-good" and s["fmt"] == "opus16")
    assert good["script_ok"] == 1.0 and good["accuracy"] == 1.0 and good["errors"] == 0
    bad = next(s for s in summary if s["engine"] == "fake-urdu")
    assert bad["script_ok"] == 0.0

    rec = bench.recommend(summary)
    assert rec["best"]["engine"] == "fake-good"
    assert bench.pick_format(summary, "fake-good") == "opus16"  # same accuracy, smaller

    # Resumable: running again sends nothing new.
    bench.run(engines, sentences, ["wav", "opus16"], 2, run_dir, audio_dir, log=lambda *a: None)
    assert len(results.read_text(encoding="utf-8").splitlines()) == 16

    page = report.build(run_dir, audio_dir, sentences)
    content = page.read_text(encoding="utf-8")
    assert "fake-good</span> for Roman Urdu dictation" in content
    assert "<mark>" in content  # Urdu script in fake-urdu output is highlighted
    assert "../../audio/mx-01.wav" in content
    json.loads(results.read_text(encoding="utf-8").splitlines()[0])
