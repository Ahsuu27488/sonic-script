# Project Status

> Current truth only: what works now, what doesn't, what's next. History goes in CHANGELOG.md, reasons in
> DECISIONS.md. Updated in the same commit as every code change (the commit-msg hook checks). Keep it short:
> it is loaded into every agent session.

**Last updated:** 2026-11-02 · **Code at:** `main` (PR #1 merged) · plan on `plan/m1-windows`
**In one line:** M0 is done: default engine is Groq (D-011), Gemini is an opt-in accuracy mode, license MIT
(D-012), merged. Next: M1, the Windows push-to-talk app (plan written, waiting for go-ahead).

## What works (verified)

- `eval/` tool: 25 offline tests pass (2026-11-02). Runs per provider in parallel (`concurrency` in
  `spec/providers.json`); thread-safe rate limiter.
- M0 results, opus16, owner's voice (`eval/results/m0/report.html`, local only), p50 / p95, accuracy, English kept:
  (re-scored 2026-11-02 after 3 expected-text fixes, D-013)
  - `groq-whisper-hint` (no LLM): 0.25 s / 1.2 s, 89.2%, 87%; 7% of outputs leak Urdu letters.
  - `groq-whisper-hint+gptoss20b`: 1.15 s / 2.7 s, 90.7%, 87%, 100% Latin, 5 errors in 132 (Groq rate limits).
  - `groq-whisper-hint-words` (word list in the Whisper prompt): 0.2 s / 0.4 s, 84.8%, English 91%, 88% Latin:
    names fixed but pure Urdu gets translated to English, so no (D-014).
  - `groq-whisper-auto+gptoss20b`: 1.8 s / 3.5 s, 87.0%, 85%.
  - Whisper forced to `language=ur` (+ LLM): 81-86%, English 69-73%: forcing Urdu wrecks English words.
  - `gemini-3.5-flash-lite`: 97-97.5%, English 95-96%, silence 100%. Evening 2026-10-28: p50 12-16 s; re-test
    2026-10-29 16:15 (`m0-gemini-am`): p50 1.8 s but p95 23 s (14 of 44 took 10-29 s), 1 timeout.
    `gemini-3.1-flash-lite` mostly HTTP 503 "high demand", also with a fresh key: Google-side.
- Opus 16 kbps (8 KB/clip) is as accurate as WAV (128 KB) for both providers.
- Every Groq engine invents text on silence (even echoes the hint); Gemini returns nothing (D-007).
- Code knowledge graph (D-010): `graphify query "..."` works; built 2026-11-02 (251 nodes, code only).

## In progress

- M1 plan: `docs/plans/2026-11-02-m1-windows-push-to-talk.md` (branch `plan/m1-windows`). Rust and the C++
  build tools are not installed yet (plan step 0).

## Live state

- Nothing is live.

## Known gaps

1. Hint + LLM misses the latency target (p95 2.7 s > 1.2 s); only Whisper-hint alone meets it.
2. Whisper-hint output leaks Urdu letters (7%) and spells loosely; needs a local script-guard/cleanup.
3. Names and tech words fail on Groq ("Jaminai Algrock" = Gemini aur Groq, Codex → "Codec", RAM → "rhyme",
   styling → "selling"); the word list goes in the local cleanup, not the Whisper prompt (D-014).
4. Whisper-hint sometimes translates instead of transcribing (`mx-04` → "Take a backup and run the database."):
   no cleanup can fix that; watch for it in M1. Output can also contain Devanagari, not only Urdu letters.
5. Prices for `gpt-oss-120b` and Gemini 3.x audio are from 2026-10-26; recheck before launch.
6. Final name not decided (license is MIT, D-012). `main` is guarded by the pre-push hook only.

## Next

1. Owner: keep or change the name "SonicScript" (check GitHub and the Play Store); optionally listen to `ur-04`,
   `mx-04`, `mx-06` in the report to confirm the D-013 fixes.
2. Owner: read the M1 plan, say go, and install Rust + C++ build tools (plan step 0, one UAC click).
3. Agent: build M1 in plan order, normalizer first (`spec/normalizer/`, `desktop/core`).

## How to run and check

- Setup: `cd eval && pip install -e ".[dev]"` (needs ffmpeg on PATH). Keys: `GROQ_API_KEY`, `GEMINI_API_KEY`
  in the environment, the Windows user environment, or `eval/.env`.
- Run: `python -m sonic_eval record | probe | bench --preset compression|engines | report | rescore`
- Code map: `graphify query "<question>"`; after code changes `graphify update .` (see CLAUDE.md).
- Tests: `cd eval && python -m ruff check . && python -m pytest -q` (25 tests, all passing on 2026-11-02)
