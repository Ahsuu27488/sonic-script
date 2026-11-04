"""Run a text-only step (a `refine` model + prompt) on the saved transcripts of an earlier run, and score it.

Used to choose the smart-formatting model (D-030) without spending audio requests: each saved row's transcript
goes through the format step, and the new row is scored as if an engine `<engine>+fmt-<tag>` had produced it.
The input is what the formatted engine would see: the speech step's own text (`raw_asr`, before an old
romanize step) when the row has one, else the row's text. Resumable like `bench.run`.
"""

from __future__ import annotations

import json
import time
from dataclasses import asdict
from pathlib import Path

import httpx

from .bench import Row, read_rows
from .config import Sentence
from .engines import TIMEOUT, EngineError, EngineResult, Refiner, clean_output, size_tag
from .ratelimit import RateLimiter
from .scoring import format_score, score


def refine_spec(model: str, prompt: str, rpm: float) -> dict:
    return {"provider": "groq", "model": model, "prompt": prompt, "rpm": rpm,
            "extra": {"reasoning_effort": "low"}}


def run(src: Path, out_dir: Path, providers: dict, spec: dict, sentences: list[Sentence],
        engines: list[str] | None = None, repeat: int = 0, log=print) -> Path:
    by_id = {s.id: s for s in sentences}
    rows = [r for r in read_rows(src) if not r["error"] and r["repeat"] == repeat and r["sentence"] in by_id
            and (not engines or r["engine"] in engines)]
    out_dir.mkdir(parents=True, exist_ok=True)
    results = out_dir / "results.jsonl"
    suffix = f"+fmt-{size_tag(spec['model'])}"
    done = {(r["engine"], r["fmt"], r["sentence"]) for r in read_rows(results) if not r["error"]}
    jobs = [r for r in rows if (r["engine"] + suffix, r["fmt"], r["sentence"]) not in done]
    log(f"{len(jobs)} row(s) to format ({len(done)} already done) -> {results}")
    client = httpx.Client(timeout=TIMEOUT)
    refiner = Refiner(spec, providers["providers"][spec["provider"]], client, RateLimiter())
    with results.open("a", encoding="utf-8") as fh:
        for n, r in enumerate(jobs, 1):
            s = by_id[r["sentence"]]
            heard = r.get("raw_asr") or r["text"]
            res = EngineResult(raw_asr=heard)
            try:
                res.text = clean_output(refiner.run(heard, res, [])) if heard.strip() else ""
            except (EngineError, httpx.HTTPError) as exc:
                res.error = str(exc)
            sc = score(res.text, s.text, s.english) if not res.error else None
            row = Row(
                engine=r["engine"] + suffix, fmt=r["fmt"], sentence=s.id, category=s.category, repeat=repeat,
                text=res.text, raw_asr=heard, latency_ms=res.stages_ms.get("refine", 0.0),
                stages_ms=res.stages_ms, bytes=r.get("bytes", 0), duration_s=r.get("duration_s", 0.0),
                tokens_in=res.refine_tokens_in, tokens_out=res.refine_tokens_out, cost_usd=0.0,
                retries=res.retries, error=res.error,
                script_ok=sc.script_ok if sc else False, english_found=sc.english_found if sc else 0,
                english_total=len(s.english), accuracy=sc.accuracy if sc else 0.0, wer=sc.wer if sc else 1.0,
                at=time.time(),
                format_rate=format_score(res.text, s.format_check).rate if sc and s.format_check else None,
            )
            fh.write(json.dumps(asdict(row), ensure_ascii=False) + "\n")
            fh.flush()
            status = f"ERROR {res.error}" if res.error else f"{row.latency_ms:6.0f} ms  acc {row.accuracy:.2f}"
            log(f"[{n}/{len(jobs)}] {row.engine:<40} {s.id:<7} {status}")
    return results
