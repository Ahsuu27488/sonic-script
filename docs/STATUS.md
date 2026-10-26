# Project Status

> Current truth only: what works now, what doesn't, what's next. History goes in CHANGELOG.md, reasons in
> DECISIONS.md. Updated in the same commit as every code change (the commit-msg hook checks). Keep it short:
> it is loaded into every agent session.

**Last updated:** 2026-10-27 · **Code at:** branch `feat/m0-benchmark`
**In one line:** Milestone 0 benchmark tool is built and tested; waiting for the owner's voice recordings.

## What works (verified)

- `eval/` benchmark tool: `record`, `probe`, `bench`, `report` - verified by 23 offline tests
  (`cd eval && python -m pytest -q`, 2026-10-27) and real runs below.
- All 6 engines in `spec/providers.json` answer with the owner's keys - `python -m sonic_eval probe`, 2026-10-27.
- Real smoke run on 3 Windows text-to-speech English clips (`eval/results/smoke-tts/`, local only):
  `gemini-3.5-flash-lite` p50 ~1.3 s / p95 ~1.5 s; `gemini-3.1-flash-lite` p50 ~2.5 s with spikes to 20 s.
  Opus 16 kbps = 7.5 KB vs 134 KB WAV. English only: says nothing yet about Urdu quality.
- Silence: with the `[no speech]` prompt rule both Gemini engines return empty text; every Whisper engine
  still invents a word ("shukriya", "music", "Tarek") - see D-007.

## In progress

- PR https://github.com/Ahsuu27488/sonic-script/pull/1 (`feat/m0-benchmark`): M0 harness, open for review.
  Next is the owner recording the 44 clips (the benchmark results can land on the same branch).

## Live state

- Nothing is live.

## Known gaps

1. No recordings yet, so no Urdu/mixed-speech results and no engine decision (M0 is not done).
2. Prices for `gpt-oss-120b` and Gemini 3.x audio are from the 2026-10-26 pricing pages; recheck before launch.
3. Accuracy folding (`eval/sonic_eval/scoring.py` `VARIANTS`) covers common Roman Urdu spellings only; review
   outputs by ear in the report, not just the score.
4. License (Apache-2.0 proposed) and the final name are not decided. Repo is private
   (github.com/Ahsuu27488/sonic-script); `main` is guarded by the pre-push hook only (GitHub Free can't protect
   branches of private repos).

## Next

1. Owner records the test set: `cd eval && python -m sonic_eval record` (~25 min; 42 sentences + 2 silence clips),
   optionally `record --free 5`.
2. Run `python -m sonic_eval bench --preset compression`, then `bench --preset engines` (uses the format the
   compression run picks; pass `--formats` if it isn't opus16). Read `eval/results/m0/report.html`.
3. Write the result as a decision (default engine, fallback, audio format), then start M1 (Windows app, Rust).

## How to run and check

- Setup: `cd eval && pip install -e ".[dev]"` (needs ffmpeg on PATH). Keys: `GROQ_API_KEY`, `GEMINI_API_KEY`
  in the environment, the Windows user environment, or `eval/.env`.
- Run: `python -m sonic_eval record | probe | bench --preset compression|engines | report`
- Tests: `cd eval && python -m ruff check . && python -m pytest -q` (23 tests, all passing on 2026-10-27)
