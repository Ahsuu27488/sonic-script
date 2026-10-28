"""Run engines x audio formats x sentences, save every result as a JSON line, and summarise.

Runs are resumable: results.jsonl is appended to, and a (engine, format, sentence, repeat) that already
succeeded is skipped. Re-run the same command after a crash or a rate-limit stop.
"""

from __future__ import annotations

import json
import statistics
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass
from pathlib import Path

from . import audio
from .config import AUDIO_DIR, RESULTS, Sentence
from .engines import Engine, FakeEngine
from .scoring import score

PRESETS = {
    # Which Opus bitrate is enough? One engine per provider, every format, one pass.
    "compression": {"engines": ["gemini-3.5-flash-lite", "groq-whisper+gptoss20b"],
                    "formats": ["wav", "opus32", "opus24", "opus16", "opus12"], "repeats": 1},
    # Which engine? All engines at one format, three passes for stable latency numbers.
    "engines": {"engines": None, "formats": ["opus16"], "repeats": 3},
}


@dataclass
class Row:
    engine: str
    fmt: str
    sentence: str
    category: str
    repeat: int
    text: str
    raw_asr: str | None
    latency_ms: float
    stages_ms: dict
    bytes: int
    duration_s: float
    tokens_in: int
    tokens_out: int
    cost_usd: float
    retries: int
    error: str | None
    script_ok: bool
    english_found: int
    english_total: int
    accuracy: float
    wer: float
    at: float


def run(engines: list[Engine], sentences: list[Sentence], formats: list[str], repeats: int,
        out_dir: Path, audio_dir: Path = AUDIO_DIR, limit: int | None = None,
        log=print) -> Path:
    out_dir.mkdir(parents=True, exist_ok=True)
    results = out_dir / "results.jsonl"
    done = {(r["engine"], r["fmt"], r["sentence"], r["repeat"]) for r in read_rows(results) if not r["error"]}
    have = [s for s in sentences if (audio_dir / f"{s.id}.wav").exists()]
    missing = len(sentences) - len(have)
    if not have:
        raise SystemExit(f"No recordings in {audio_dir}. Run: python -m sonic_eval record")
    if missing:
        log(f"note: {missing} sentence(s) not recorded yet; benchmarking the {len(have)} that are.")
    if limit:
        have = have[:limit]
    FakeEngine.expected = {s.id: s.text for s in have}
    cache = out_dir.parent / "cache"
    jobs = [(e, f, s, r) for r in range(repeats) for f in formats for s in have for e in engines
            if (e.id, f, s.id, r) not in done]
    for f in formats:  # encode up front, so worker threads never run ffmpeg on the same file at once
        for s in have:
            audio.encode(audio_dir / f"{s.id}.wav", f, cache)
    log(f"{len(jobs)} request(s) to run ({len(done)} already done) -> {results}")

    # Each provider gets its own workers, so a slow provider never holds up a fast one. Up to `concurrency`
    # requests per provider are in flight; the rate limiter still spaces their start times.
    by_provider: dict[str, list] = {}
    for job in jobs:
        by_provider.setdefault(job[0].spec.get("provider", "fake"), []).append(job)
    lock = threading.Lock()
    counter = {"n": 0}
    stopped: set[str] = set()

    def work(provider: str, job) -> None:
        engine, fmt, s, rep = job
        if provider in stopped:
            return
        enc = audio.encode(audio_dir / f"{s.id}.wav", fmt, cache)
        res = engine.transcribe(enc, s.id)
        sc = score(res.text, s.text, s.english) if not res.error else None
        row = Row(
            engine=engine.id, fmt=fmt, sentence=s.id, category=s.category, repeat=rep,
            text=res.text, raw_asr=res.raw_asr, latency_ms=res.latency_ms, stages_ms=res.stages_ms,
            bytes=len(enc.data), duration_s=round(enc.duration_s, 2), tokens_in=res.tokens_in,
            tokens_out=res.tokens_out, cost_usd=engine.cost_usd(res, enc.duration_s),
            retries=res.retries, error=res.error,
            script_ok=sc.script_ok if sc else False,
            english_found=sc.english_found if sc else 0, english_total=len(s.english),
            accuracy=sc.accuracy if sc else 0.0, wer=sc.wer if sc else 1.0, at=time.time(),
        )
        status = f"ERROR {res.error}" if res.error else f"{res.latency_ms:6.0f} ms  acc {sc.accuracy:.2f}"
        with lock:
            fh.write(json.dumps(asdict(row), ensure_ascii=False) + "\n")
            fh.flush()
            counter["n"] += 1
            log(f"[{counter['n']}/{len(jobs)}] {engine.id:<24} {fmt:<7} {s.id:<7} {status}")
            if res.error and _fatal(res.error) and provider not in stopped:
                stopped.add(provider)
                log(f"Stopping {provider}: this error will repeat. Fix it and re-run the same command.")

    with results.open("a", encoding="utf-8") as fh:
        pools = [ThreadPoolExecutor(max_workers=max(1, int(provider_jobs[0][0].provider.get("concurrency", 1))),
                                    thread_name_prefix=name)
                 for name, provider_jobs in by_provider.items()]
        futures = [pool.submit(work, name, job)
                   for pool, (name, provider_jobs) in zip(pools, by_provider.items(), strict=True)
                   for job in provider_jobs]
        for fut in futures:
            fut.result()
        for pool in pools:
            pool.shutdown()
    return results


def _fatal(error: str) -> bool:
    return error.startswith("missing ") or "HTTP 401" in error or "HTTP 403" in error or "HTTP 404" in error


def read_rows(path: Path) -> list[dict]:
    if not path.exists():
        return []
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            rows.append(json.loads(line))
    # Keep the latest result per key (a successful retry replaces an earlier error).
    latest: dict[tuple, dict] = {}
    for r in rows:
        latest[(r["engine"], r["fmt"], r["sentence"], r["repeat"])] = r
    return list(latest.values())


def percentile(values: list[float], p: float) -> float:
    if not values:
        return 0.0
    s = sorted(values)
    k = (len(s) - 1) * p / 100
    lo, hi = int(k), min(int(k) + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (k - lo)


def summarise(rows: list[dict]) -> list[dict]:
    """One line per (engine, format)."""
    groups: dict[tuple[str, str], list[dict]] = {}
    for r in rows:
        groups.setdefault((r["engine"], r["fmt"]), []).append(r)
    out = []
    for (engine, fmt), rs in sorted(groups.items()):
        done = [r for r in rs if not r["error"]]
        silent = [r for r in done if r["category"] == "silence"]
        ok = [r for r in done if r["category"] != "silence"]  # quality and latency: speech clips only
        lat = [r["latency_ms"] for r in ok]
        eng_total = sum(r["english_total"] for r in ok)
        out.append({
            "engine": engine, "fmt": fmt, "n": len(rs), "errors": len(rs) - len(done),
            "silence_ok": _rate(sum(r["accuracy"] == 1.0 for r in silent), len(silent)) if silent else None,
            "script_ok": _rate(sum(r["script_ok"] for r in ok), len(ok)),
            "english": _rate(sum(r["english_found"] for r in ok), eng_total) if eng_total else 1.0,
            "accuracy": round(statistics.fmean([r["accuracy"] for r in ok]), 4) if ok else 0.0,
            "wer": round(statistics.fmean([r["wer"] for r in ok]), 4) if ok else 1.0,
            "p50": round(percentile(lat, 50)), "p95": round(percentile(lat, 95)),
            "kb": round(statistics.fmean([r["bytes"] for r in rs]) / 1024, 1) if rs else 0,
            "cost_per_1k": round(statistics.fmean([r["cost_usd"] for r in done]) * 1000, 3) if done else 0.0,
            "retries": sum(r["retries"] for r in rs),
        })
    return out


def _rate(a: float, b: float) -> float:
    return round(a / b, 4) if b else 0.0


FORMAT_ORDER = ["wav", "opus32", "opus24", "opus16", "opus12"]


def pick_format(summary: list[dict], engine: str, max_acc_drop: float = 0.01,
                max_eng_drop: float = 0.02) -> str | None:
    """Smallest format whose accuracy and English rate stay within the allowed drop from the best format."""
    rows = {s["fmt"]: s for s in summary if s["engine"] == engine and s["errors"] < s["n"]}
    if not rows:
        return None
    best_acc = max(r["accuracy"] for r in rows.values())
    best_eng = max(r["english"] for r in rows.values())
    good = [f for f, r in rows.items()
            if r["accuracy"] >= best_acc - max_acc_drop and r["english"] >= best_eng - max_eng_drop]
    return min(good, key=lambda f: rows[f]["kb"]) if good else None


def recommend(summary: list[dict], p95_target_ms: float = 1200) -> dict | None:
    """Best engine: must keep Latin script >= 99% and have no errors; then within target, highest quality."""
    candidates = [s for s in summary if s["errors"] == 0 and s["script_ok"] >= 0.99]
    if not candidates:
        return None
    fast = [s for s in candidates if s["p95"] <= p95_target_ms] or candidates
    quality = lambda s: (round(s["accuracy"] * 0.6 + s["english"] * 0.4, 3), -s["p95"])  # noqa: E731
    ranked = sorted(fast, key=quality, reverse=True)
    best = ranked[0]
    runner_up = next((s for s in ranked[1:] if _provider(s["engine"]) != _provider(best["engine"])), None)
    return {"best": best, "fallback": runner_up, "within_target": best["p95"] <= p95_target_ms}


def _provider(engine_id: str) -> str:
    return engine_id.split("-")[0]


def print_summary(summary: list[dict], file=sys.stdout) -> None:
    head = f"{'engine':<24} {'fmt':<7} {'n':>4} {'err':>4} {'latin':>6} {'eng':>6} {'acc':>6} " \
           f"{'quiet':>6} {'p50':>6} {'p95':>6} {'KB':>6} {'$/1k':>7}"
    print(head, file=file)
    print("-" * len(head), file=file)
    for s in summary:
        print(f"{s['engine']:<24} {s['fmt']:<7} {s['n']:>4} {s['errors']:>4} {s['script_ok']:>6.0%} "
              f"{s['english']:>6.0%} {s['accuracy']:>6.1%} {_pct(s['silence_ok']):>6} "
              f"{s['p50']:>6} {s['p95']:>6} {s['kb']:>6} {s['cost_per_1k']:>7.3f}", file=file)


def _pct(v: float | None) -> str:
    return "-" if v is None else f"{v:.0%}"


def new_run_dir(name: str | None = None) -> Path:
    return RESULTS / (name or time.strftime("%Y%m%d-%H%M%S"))
