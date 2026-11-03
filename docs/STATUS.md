# Project Status

> Current truth only: what works now, what doesn't, what's next. History goes in CHANGELOG.md, reasons in
> DECISIONS.md. Updated in the same commit as every code change (the commit-msg hook checks). Keep it short:
> it is loaded into every agent session.

**Last updated:** 2026-11-03 · **Code at:** `feat/m1-voice-gate` (PR #4; `main` has M0, the M1 plan and
step 1, PR #3 merged 2026-11-03)
**In one line:** M0 is done (Groq default, Gemini opt-in, MIT). M1 (Windows dictation app in Rust): plan
steps 1 (text cleanup) and 2 (voice gate) are built and tested, each in its own PR; next is step 3, encode +
Groq.

**Start here (new session):** read `docs/plans/2026-11-02-m1-windows-push-to-talk.md`, then decisions
D-011 to D-020. One plan step per branch and PR (`feat/m1-<step>`); the Rust workspace is `desktop/`.

## What works (verified)

- Toolchain (2026-11-03): Rust 1.99.1 (stable, x86_64-pc-windows-msvc), VS 2022 Build Tools with the C++
  workload; a test program builds, links and runs.
- `desktop/core` normalizer (M1 step 1, D-019): script guard (Urdu / Devanagari → Roman, `needs_llm` flag),
  phantom filter, word list, spacing + trailing space. `cargo test`: 22 shared cases in
  `spec/normalizer/cases.json` + 3 unit tests pass; fmt and clippy `-D warnings` clean (2026-11-03).
- Normalizer on all 131 M0 `groq-whisper-hint` opus16 outputs (re-scored with `eval` scoring, mean over every
  row incl. the 2 silence clips): Latin-only 93.1% → 100%, silence 0% → 100%, accuracy 85.1% → 90.0%
  (90.2% with a 10-word list), English kept 87.3% → 87.3% (88.5% with the list).
- `desktop/core` voice gate (M1 step 2, D-020): on the 44 M0 recordings it sends all 42 speech clips (least
  speech 1,420 ms) and drops both silence clips (20 ms and 80 ms of "speech"; the limit is 300 ms). The test
  also passes every speech clip without padding, with the room noise mixed in (also +10 dB) and 20 dB quieter,
  and drops the silence clips at +10 dB. `cargo test`: 9 tests pass (2026-11-03); the recordings test is skipped
  where they are absent (CI).
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
  - `gemini-3.5-flash-lite`: 97-97.5%, English 95-96%, silence 100%. Re-test 2026-10-29 16:15
    (`m0-gemini-am`): p50 1.8 s but p95 23 s (14 of 44 took 10-29 s), 1 timeout.
    `gemini-3.1-flash-lite` mostly HTTP 503 "high demand", also with a fresh key: Google-side.
- Opus 16 kbps (8 KB/clip) is as accurate as WAV (128 KB) for both providers.
- Every Groq engine invents text on silence (always the hint's last sentence or "Tch, tch"); Gemini returns
  nothing (D-007).
- Code knowledge graph (D-010): `graphify query "..."` works; built 2026-11-02 (code only).

## In progress

- M1 (plan approved 2026-11-02): tap Right Ctrl to start, tap or Enter to stop and paste, hold to talk, Esc
  cancels (D-015); key changeable (D-018); model catalog `spec/models.json` from `eval` (D-017). Pop-up mic on
  text fields, settings page and fallback model are M2 (D-016, D-017). Step 1 merged (PR #3), step 2 on
`feat/m1-voice-gate` (PR #4).

## Live state

- Nothing is live.

## Known gaps

1. Hint + LLM misses the latency target (p95 2.7 s > 1.2 s); only Whisper-hint alone meets it.
2. Names fail on Groq; the word list fixes sound-alikes ("Jaminai" → Gemini, "Codec" → Codex) but not words
   run together ("Algrock" = "aur Groq") or 2-consonant words ("rhyme" for RAM).
3. Whisper-hint sometimes translates instead of transcribing (`mx-04` → "Take a backup and run the database."):
   no cleanup can fix that; watch for it in M1.
4. Dictating exactly the hint's last sentence ("Theek hai, main abhi meeting mein hoon.") is dropped as a
   phantom (D-019). The voice gate's `speech_ms` could let the filter skip clips with clear speech (not done).
5. The voice gate is tuned on one male voice (the owner) and one room; loud talking or TV in the background
   counts as speech (energy + zero-crossing, not a speech model). Silero VAD only if this fails in daily use.
6. Urdu-script "میں" is always romanized "mein" (it can also be "main" = I). Mostly-Urdu output only gets a
   rough letter-by-letter draft and is flagged `needs_llm`; the LLM re-ask is not wired yet (step 3).
7. Prices for `gpt-oss-120b` and Gemini 3.x audio are from 2026-10-26; recheck before launch.
8. Final name not decided (license is MIT, D-012). `main` is guarded by the pre-push hook only.

## Next

1. Agent: M1 step 3, encode (Ogg/Opus 16 kbps, FLAC fallback) + Groq client (`desktop/core/src/encode.rs`,
   `groq.rs`, `spec.rs`), then `--bench` golden check → hotkey (D-015, D-018) → inject → model catalog →
   latency log. Delegate boilerplate to Gemini CLI via Foreman if the owner has signed Gemini CLI in.
2. Owner: review and merge PR #4 (voice gate); keep or change the name "SonicScript".

## How to run and check

- Setup: `cd eval && pip install -e ".[dev]"` (needs ffmpeg on PATH). Keys: `GROQ_API_KEY`, `GEMINI_API_KEY`
  in the environment, the Windows user environment, or `eval/.env`.
- Run: `python -m sonic_eval record | probe | bench --preset compression|engines | report | rescore`
- Try the cleanup: `cd desktop && cargo run -q --example clean -- [words.txt]` (one text per line on stdin).
- Try the voice gate: `cd desktop && cargo run -q --example gate -- <16 kHz mono .wav files>`.
- Code map: `graphify query "<question>"`; after code changes `graphify update .` (see CLAUDE.md).
- Tests: `cd eval && python -m ruff check . && python -m pytest -q` (25 tests) and
  `cd desktop && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.
