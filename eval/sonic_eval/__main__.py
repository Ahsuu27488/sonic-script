"""SonicScript M0 command line.

  python -m sonic_eval record [--only ID ...] [--redo] [--device N] [--list-devices]
  python -m sonic_eval record --free 5
  python -m sonic_eval probe [--engines ID ...]
  python -m sonic_eval bench --preset compression|engines [--run NAME] [--engines ...] [--formats ...]
  python -m sonic_eval report [--run NAME]
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import numpy as np

from . import audio, bench, record, report
from .config import AUDIO_DIR, RESULTS, load_env, load_providers, load_sentences
from .engines import build_engines, warm_up
from .ratelimit import RateLimiter


def main(argv: list[str] | None = None) -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(prog="sonic_eval", description="SonicScript M0 engine benchmark")
    sub = ap.add_subparsers(dest="cmd", required=True)

    r = sub.add_parser("record", help="record the test sentences")
    r.add_argument("--only", nargs="+", help="sentence ids to (re)record")
    r.add_argument("--redo", action="store_true", help="record again even if a file exists")
    r.add_argument("--device", type=int, help="input device index (see --list-devices)")
    r.add_argument("--list-devices", action="store_true")
    r.add_argument("--free", type=int, metavar="N", help="record N unscripted sentences instead")

    p = sub.add_parser("probe", help="one request per engine, to check keys and model names")
    p.add_argument("--engines", nargs="+")

    b = sub.add_parser("bench", help="run the benchmark")
    b.add_argument("--preset", choices=sorted(bench.PRESETS), default="engines")
    b.add_argument("--engines", nargs="+", help="override the preset's engines")
    b.add_argument("--formats", nargs="+", help="override the preset's formats (wav, opus32 ... opus12)")
    b.add_argument("--repeats", type=int)
    b.add_argument("--limit", type=int, help="only the first N sentences (quick try)")
    b.add_argument("--run", help="run folder name under eval/results (default: m0)", default="m0")
    b.add_argument("--providers", type=Path, help="alternate providers.json (tests)")
    b.add_argument("--audio-dir", type=Path, default=AUDIO_DIR)

    rp = sub.add_parser("report", help="build the HTML report for a run")
    rp.add_argument("--run", default="m0")
    rp.add_argument("--audio-dir", type=Path, default=AUDIO_DIR)

    args = ap.parse_args(argv)
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(line_buffering=True)  # progress shows up live in logs
    load_env()

    if args.cmd == "record":
        if args.list_devices:
            record.list_devices()
        elif args.free:
            record.record_free(args.free, args.device)
        else:
            record.record_set(args.only, args.redo, args.device)
        return 0

    if args.cmd == "probe":
        return _probe(args.engines)

    if args.cmd == "bench":
        preset = bench.PRESETS[args.preset]
        providers = load_providers(args.providers) if args.providers else load_providers()
        ids = args.engines or preset["engines"]
        engines, clients = build_engines(providers, ids, RateLimiter())
        warm_up(providers, clients)
        run_dir = RESULTS / args.run
        results = bench.run(engines, load_sentences(), args.formats or preset["formats"],
                            args.repeats or preset["repeats"], run_dir, args.audio_dir, args.limit)
        print()
        bench.print_summary(bench.summarise(bench.read_rows(results)))
        out = report.build(run_dir, args.audio_dir)
        print(f"\nReport: {out}")
        return 0

    if args.cmd == "report":
        out = report.build(RESULTS / args.run, args.audio_dir)
        bench.print_summary(bench.summarise(bench.read_rows(RESULTS / args.run / "results.jsonl")))
        print(f"\nReport: {out}")
        return 0
    return 1


def _probe(ids: list[str] | None) -> int:
    providers = load_providers()
    engines, clients = build_engines(providers, ids, RateLimiter())
    warm_up(providers, clients)
    clip = next(iter(sorted(AUDIO_DIR.glob("*.wav"))), None)
    if clip is None:
        clip = RESULTS / "cache" / "probe-silence.wav"
        rng = np.random.default_rng(0)
        audio.write_wav(clip, (rng.standard_normal(audio.RATE) * 0.001).astype(np.float32))
        print("No recordings yet; probing with 1 s of near-silence (outputs will be empty, that's fine).")
    enc = audio.encode(clip, "opus16", RESULTS / "cache")
    failed = 0
    for e in engines:
        res = e.transcribe(enc, clip.stem)
        if res.error:
            failed += 1
            print(f"  FAIL  {e.id:<24} {res.error}")
        else:
            print(f"  ok    {e.id:<24} {res.latency_ms:6.0f} ms  {res.text[:70]!r}")
    if failed:
        print("\nFix the failures (key in eval/.env, or the model id in spec/providers.json), then probe again.")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
