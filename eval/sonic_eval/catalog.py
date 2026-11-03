"""Model catalog for the model page (D-017, M1 plan step 8): `spec/models.json`.

Each model the app can run gets what `spec/providers.json` says about it (`catalog`: label, free / paid tier,
free-tier limits with their source, notes) and what the benchmark measured: accuracy, English words kept,
Roman-only output, silence, latency, errors and cost per hour of speech. Results come from the `eval` runs
named, and the app's own run (`sonicscript --bench`, rows named `app-<id>`) is shown next to them as a check
that the app gets the same.
"""

from __future__ import annotations

import datetime as dt
import statistics
from pathlib import Path

from .bench import percentile, read_rows


def build(providers: dict, results_dir: Path, runs: list[str], app_runs: list[str] | None = None,
          total: int | None = None) -> dict:
    """`total` = clips in the test set, so a model measured on fewer (a daily limit ran out) says so."""
    eval_rows = _rows(results_dir, runs)
    app_rows = _rows(results_dir, app_runs or [])
    default = providers.get("default_engine")
    fallback = providers.get("fallback_engine")
    models = []
    for spec in providers["engines"]:
        info = spec.get("catalog")
        if not info:
            continue
        keys = [providers["providers"][spec["provider"]]["key_env"]]
        if spec.get("refine"):
            keys.append(providers["providers"][spec["refine"]["provider"]]["key_env"])
        entry = {
            "id": spec["id"],
            "label": info["label"],
            "provider": spec["provider"],
            "tier": info["tier"],
            "recommended": spec["id"] == default,
            "fallback": spec["id"] == fallback,
            "keys": list(dict.fromkeys(keys)),
            "limits": info.get("limits", {}),
            "notes": info.get("notes", ""),
            "results": _summary(eval_rows.get(spec["id"], []), total),
        }
        app = _summary(app_rows.get(f"app-{spec['id']}", []), total)
        if app:
            entry["app_check"] = app
        models.append(entry)
    # Free first, then by measured accuracy.
    models.sort(key=lambda m: (m["tier"] != "free", -(m["results"] or {}).get("accuracy", 0)))
    return {
        "version": 1,
        "generated": dt.date.today().isoformat(),
        "runs": runs,
        "app_runs": app_runs or [],
        "about": "Written by `python -m sonic_eval catalog`; do not edit by hand. Accuracy, English kept and "
                 "Roman-only are over speech clips; silence counts clips with no speech that gave no text "
                 "(before the app's voice gate); latency is clip in -> text out; usd_per_hour is per hour "
                 "of speech at paid prices (0 where the price is not in the spec).",
        "models": models,
    }


def _rows(results_dir: Path, runs: list[str]) -> dict[str, list[dict]]:
    """Rows by engine; one row per sentence: the first successful one in run order, else the first error."""
    best: dict[tuple[str, str], dict] = {}
    for run in runs:
        for r in read_rows(results_dir / run / "results.jsonl"):
            if r.get("fmt") != "opus16":
                continue
            key = (r["engine"], r["sentence"])
            if key not in best or (best[key]["error"] and not r["error"]):
                best[key] = dict(r, run=run)
    out: dict[str, list[dict]] = {}
    for (engine, _), r in best.items():
        out.setdefault(engine, []).append(r)
    return out


def _summary(rows: list[dict], total: int | None = None) -> dict | None:
    if not rows:
        return None
    done = [r for r in rows if not r["error"]]
    speech = [r for r in done if r["category"] != "silence"]
    silent = [r for r in done if r["category"] == "silence"]
    english_total = sum(r["english_total"] for r in speech)
    seconds = sum(r.get("duration_s") or 0 for r in done)
    lat = [r["latency_ms"] for r in speech]
    return {
        "clips": len(rows),
        "of": total or len(rows),
        "errors": len(rows) - len(done),
        "accuracy": round(statistics.fmean(r["accuracy"] for r in speech), 4) if speech else 0.0,
        "english_kept": round(sum(r["english_found"] for r in speech) / english_total, 4) if english_total else None,
        "roman_only": round(sum(bool(r["script_ok"]) for r in speech) / len(speech), 4) if speech else 0.0,
        "silence_ok": f"{sum(r['accuracy'] == 1.0 for r in silent)}/{len(silent)}" if silent else None,
        "p50_ms": round(percentile(lat, 50)),
        "p95_ms": round(percentile(lat, 95)),
        "usd_per_hour": round(sum(r.get("cost_usd") or 0 for r in done) / seconds * 3600, 3) if seconds else 0.0,
        "runs": sorted({r["run"] for r in rows}),
        "measured": dt.date.fromtimestamp(max(r.get("at") or 0 for r in rows)).isoformat(),
    }
