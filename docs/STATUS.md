# Project Status

> Current truth only: what works now, what doesn't, what's next. History goes in CHANGELOG.md, reasons in
> DECISIONS.md. Updated in the same commit as every code change (the commit-msg hook checks). Keep it short:
> it is loaded into every agent session.

**Last updated:** 2026-11-02 · **Code at:** branch `feat/m0-benchmark` (PR #1)
**In one line:** M0 is done: default engine is Groq (D-011), Gemini is an opt-in accuracy mode. Next is the
owner's report review, then merge PR #1 and start M1 (Windows app, Rust).

## What works (verified)

- `eval/` tool: 24 offline tests pass (2026-10-28). Runs per provider in parallel (`concurrency` in
  `spec/providers.json`); thread-safe rate limiter.
- M0 results, opus16, owner's voice (`eval/results/m0/report.html`, local only), p50 / p95, accuracy, English kept:
  - `groq-whisper-hint` (no LLM): 0.25 s / 1.2 s, 88.6%, 87%; 7% of outputs leak Urdu letters.
  - `groq-whisper-hint+gptoss20b`: 1.15 s / 2.7 s, 90.0%, 86%, 100% Latin, 5 errors in 132 (Groq rate limits).
  - `groq-whisper-auto+gptoss20b`: 1.8 s / 3.5 s, 86.5%, 84%.
  - Whisper forced to `language=ur` (+ LLM): 81-86%, English 69-73%: forcing Urdu wrecks English words.
  - `gemini-3.5-flash-lite`: 96-97%, English 94-95%, silence 100%. Evening 2026-10-28: p50 12-16 s; re-test
    2026-10-29 16:15 (`m0-gemini-am`): p50 1.8 s but p95 23 s (14 of 44 took 10-29 s), 1 timeout.
    `gemini-3.1-flash-lite` mostly HTTP 503 "high demand", also with a fresh key: Google-side.
- Opus 16 kbps (8 KB/clip) is as accurate as WAV (128 KB) for both providers.
- Every Groq engine invents text on silence (even echoes the hint); Gemini returns nothing (D-007).
- Code knowledge graph (D-010): `graphify query "..."` works; built 2026-11-02 (251 nodes, code only).

## In progress

- PR https://github.com/Ahsuu27488/sonic-script/pull/1: M0 harness + results. Not merged.

## Live state

- Nothing is live.

## Known gaps

1. Hint + LLM misses the latency target (p95 2.7 s > 1.2 s); only Whisper-hint alone meets it.
2. Whisper-hint output leaks Urdu letters (7%) and spells loosely; needs a local script-guard/cleanup.
3. Names fail without a word list ("Jaminai Algrock" = Gemini aur Groq); test `--words` style word list next.
4. Prices for `gpt-oss-120b` and Gemini 3.x audio are from 2026-10-26; recheck before launch.
5. License and final name not decided. `main` is guarded by the pre-push hook only.

## Next

1. Owner: open `eval/results/m0/report.html`, click "With issues", note sentences whose "expected" text is
   wrong and outputs too bad to send. Decide license (Apache-2.0 / MIT) and the name.
2. Agent: fix any wrong expected text, add a word-list test, merge PR #1, then start M1 (Windows app, Rust),
   first piece the local script-guard (D-011).

## How to run and check

- Setup: `cd eval && pip install -e ".[dev]"` (needs ffmpeg on PATH). Keys: `GROQ_API_KEY`, `GEMINI_API_KEY`
  in the environment, the Windows user environment, or `eval/.env`.
- Run: `python -m sonic_eval record | probe | bench --preset compression|engines | report`
- Code map: `graphify query "<question>"`; after code changes `graphify update .` (see CLAUDE.md).
- Tests: `cd eval && python -m ruff check . && python -m pytest -q` (24 tests, all passing on 2026-10-28)
