# Project Status

> Current truth only: what works now, what doesn't, what's next. History goes in CHANGELOG.md, reasons in
> DECISIONS.md. Updated in the same commit as every code change (the commit-msg hook checks). Keep it short:
> it is loaded into every agent session.

**Last updated:** 2026-11-03 · **Code at:** branch `feat/m1-engines` (PR open), stacked on `feat/m1-hotkey`
(step 5, PR #7) on `feat/m1-bench` (step 4, PR #6); steps 1-3 merged (PR #3, #4, #5). **Merge #6, #7, then this
PR, in that order.**
**In one line:** M1 (Windows dictation app in Rust): the key, mic and pipeline work live (0.2 s key-up → text),
but Groq Whisper is weak on natural Urdu / Hindi, so quality now comes first (D-025): 6 engines benchmarked,
Gemini is the most accurate (~97% vs Groq 89%), and a model catalog (free / paid, limits, results) is in
`spec/models.json` and `sonicscript --models`. The default is still Groq until the owner picks.

**Start here (new session):** check PRs #6, #7 and this one are merged and `main` is pulled; read
`docs/plans/2026-11-02-m1-windows-push-to-talk.md` (Progress at the top), then decisions D-011 to D-027. One plan
step per branch and PR (`feat/m1-<step>`); the Rust workspace is `desktop/`. Checks: see "How to run and check".

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
- `desktop/core` encode + Groq + pipeline (M1 step 3, D-021): Ogg/Opus 16 kbps, Groq client with warm-up and
  one retry, gate → trim → Opus → Whisper → cleanup with per-stage timings. `cargo test`: 26 core tests pass.
- Golden check (M1 step 4): `sonicscript.exe --bench spec/fixtures/audio` on the 44 M0 recordings against Groq,
  scored by `eval rescore` (2026-11-03, padding 500 ms, D-022): accuracy 89.1% (speech clips; M0
  `groq-whisper-hint` opus16: 89.2%, 89.0% vs 89.1% on the 41 clips both have), Latin-only 100% (M0 93%),
  English kept 86% (M0 87%), silence 100% (both not sent), 0 errors, 0 retries, ~7.5 KB per clip, clip → text
  p50 298 ms / p95 387 ms (M0 request alone: 253 / 1154 ms). Report: `eval/results/m1-pad500/report.html`
  (local). `cargo test`: 4 app tests pass; fmt and clippy `-D warnings` clean.
- Dictation key + recording (M1 step 5, D-023, D-024), built and unit-tested, **not yet tried live**:
  `keys.rs` (9 tests: tap, hold, slow tap, Enter / Esc swallowed only while recording, shortcut cancels,
  typing during toggle mode, 5-minute cap, tick wrap), `resample.rs` (4 tests: 48 / 44.1 / 32 kHz, gain and
  error vs an ideal sine, 12 kHz removed, chunk size, 16 kHz pass-through), `encode::Stream` (same bytes as
  one-shot encoding; a cut mid-stream decodes). The hook installs (`sonicscript.exe` prints "listening").
  `--bench` with the streaming encoder (run `m1-stream`): 89.3%, Latin 100%, English 87%, p50 286 ms.
  `cargo test`: 41 core + 4 app tests pass.
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
- Code knowledge graph (D-010): `graphify query "..."` works; graphify 0.9.101 reinstalled and the graph
  rebuilt 2026-11-03 (code only).

- Engines (D-026) and catalog (D-027), 2026-11-03, 44 M0 clips, opus16, scored by `eval` (speech clips):
  - `gemini-3.5-transcribe+gptoss20b` (Gemini app's model + Roman cleanup on Groq): 97.9%, typical 2.5 s,
    slowest 5% 3.9 s, on 22 clips only (free limit 25 requests/day reached).
  - `gemini-3.5-flash`: 97.7%, 2.0 s / 7.5 s, on 25 clips only (free limit 20/day).
  - `gemini-3.5-flash-lite`: 97.0%, 1.6 s / 9.1 s, 44 clips; in the app 96.6%, 1.8 s / 6.3 s, 2 of 44 hit the
    10 s timeout (Google's slow tail).
  - OpenAI (paid) `gpt-4o-mini-transcribe` / `gpt-4o-transcribe` + Roman cleanup: 94.7% / 93.8%, ~1.5 s / 2.6 s;
    in the app 92.8% / 93.8%. Both write text on silence. About $0.06 spent in all.
  - `groq-whisper-hint` (default): 89.2%; in the app 89.3%, 0.3 s / 0.5 s.
  - `gemini-3.8-flash`: dropped, every free-tier request timed out or got 503.
  - Free-tier limits for the owner's key from Google's own 429s: Flash 20/day, Transcribe 25/day (reset at
    midnight Pacific = 1 PM Pakistan). Flash-Lite: not hit at 150+/day (third-party: 500/day).
- `eval`: 29 offline tests pass; `desktop`: 49 core + 6 app tests pass, fmt and clippy clean (2026-11-03).

## In progress

- M1 (plan approved 2026-11-02): tap Right Ctrl to start, tap or Enter to stop and paste, hold to talk, Esc
  cancels (D-015); key changeable (D-018); model catalog `spec/models.json` from `eval` (D-017). Pop-up mic on
  text fields, settings page and fallback model are M2 (D-016, D-017). Steps 0-3 merged (PR #3, #4, #5);
  step 4 PR #6; step 5 PR #7 (owner tried it live: key, mic and 0.2 s latency work); engines + catalog on
  `feat/m1-engines` (PR open). Default engine: owner's pick pending (see Next 1).
- Benchmark `m1-engines`: Flash and Transcribe have 19-22 clips left (resume after the daily reset with the same
  `bench --run m1-engines --engines gemini-3.5-transcribe+gptoss20b gemini-3.5-flash` command, then
  `catalog`).

## Live state

- Nothing is live.

## Known gaps

1. Hint + LLM misses the latency target (p95 2.7 s > 1.2 s); only Whisper-hint alone meets it.
2. Names fail on Groq; the word list fixes sound-alikes ("Jaminai" → Gemini, "Codec" → Codex) but not words
   run together ("Algrock" = "aur Groq") or 2-consonant words ("rhyme" for RAM).
3. Whisper-hint sometimes translates instead of transcribing (`mx-04` → "Take a backup and run the database.",
   also `nm-06`): no cleanup can fix that. It depends on the exact audio: too little silence around the speech
   made it worse (D-022); watch for it with live recordings in step 5.
4. Dictating exactly the hint's last sentence ("Theek hai, main abhi meeting mein hoon.") is dropped as a
   phantom (D-019). The voice gate's `speech_ms` could let the filter skip clips with clear speech (not done).
5. The voice gate is tuned on one male voice (the owner) and one room; loud talking or TV in the background
   counts as speech (energy + zero-crossing, not a speech model). Silero VAD only if this fails in daily use.
6. Urdu-script "میں" is always romanized "mein" (it can also be "main" = I). Mostly-Urdu output only gets a
   rough letter-by-letter draft and is flagged `needs_llm`; the LLM re-ask is not wired yet (the step 3 pipeline returns `needs_llm`; the re-ask comes with the app).
7. Prices for `gpt-oss-120b` and Gemini 3.x audio are from 2026-10-26; recheck before launch.
8. Step 5 live: tap / hold, mic (Realtek array, 48 kHz) and key-up → text 0.2 s confirmed by the owner;
   Enter / Esc swallowing and Right Ctrl + C not reported yet. Opening the mic takes 160-690 ms, which can
   clip the first word ("Bhuaiyaar" = "Bhai yaar"?): not fixed yet.
11. Gemini Flash-Lite, the most accurate engine with a usable free limit, has a slow tail (some requests
    10-20 s; 2 of 44 hit the app's 10 s timeout). As the default it needs the D-017 fallback: start Groq when
    Gemini is slow, first answer wins. Not built yet.
12. Free-tier Gemini requests may be used by Google to improve its products (Gemini API terms); the catalog
    says so. OpenAI and Groq-only engines do not have this note (not checked).
9. Building `desktop/` needs CMake on PATH (for libopus). VS Build Tools ships one at
   `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`;
   add that folder to the user PATH or `winget install Kitware.CMake`.
10. Final name not decided (license is MIT, D-012). `main` is guarded by the pre-push hook only.

## Next

1. Owner: pick the default engine. Proposal: Gemini 3.5 Flash-Lite with the Groq fallback (known gap 11),
   Transcribe / Flash as choices for people with a paid key. Then the agent builds the fallback race and sets
   `default_engine`.
2. Agent: finish the Flash / Transcribe benchmark after the daily reset; fix the mic-open delay (known gap 8);
   then M1 step 6, changeable key → step 7 paste at the cursor → latency log.
3. Owner: add CMake to PATH (known gap 9); merge PR #6, #7, then this PR; keep or change the name.

## How to run and check

- Setup: `cd eval && pip install -e ".[dev]"` (needs ffmpeg on PATH). Keys: `GROQ_API_KEY`, `GEMINI_API_KEY`
  in the environment, the Windows user environment, or `eval/.env`.
- Run: `python -m sonic_eval record | probe | bench --preset compression|engines | report | rescore`
- Try the cleanup: `cd desktop && cargo run -q --example clean -- [words.txt]` (one text per line on stdin).
- Try the voice gate: `cd desktop && cargo run -q --example gate -- <16 kHz mono .wav files>`.
- Dictate: `cd desktop && cargo run -q --release -p sonicscript` (tap / hold Right Ctrl; the text is printed).
  Mic check: `... -p sonicscript -- --record 5`.
- Models: `... -p sonicscript -- --models` (catalog); any command takes `--engine <id>` (ids in the catalog).
  The app reads keys from the environment / Windows user environment only, not `eval/.env`.
- Try the whole pipeline: `cd desktop && cargo run -q --release -p sonicscript -- --file <wav>...`
  (needs `GROQ_API_KEY`; prints the text, so test recordings only).
- Golden check: `cd desktop && cargo run -q --release -p sonicscript -- --bench ../spec/fixtures/audio --out
  ../eval/results/m1-bench/results.jsonl`, then `cd eval && python -m sonic_eval rescore --run m1-bench`.
- Code map: `graphify query "<question>"`; after code changes `graphify update .` (see CLAUDE.md).
- Catalog: `cd eval && python -m sonic_eval catalog --runs m1-engines m0 --app-runs m1-cli-...` writes
  `spec/models.json` (rebuild the app afterwards: it embeds the file).
- Tests: `cd eval && python -m ruff check . && python -m pytest -q` (29 tests) and
  `cd desktop && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.
