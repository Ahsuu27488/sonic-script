# Changelog

> Newest first. Each entry: date, commit (or "uncommitted"), what changed, and for fixes the root cause.
> This is the history; STATUS.md holds only the current truth.

## 2026-11-04 · feat/m1-paste (M1 step 7)

- Paste at the cursor: new `desktop/win/src/inject.rs`; dictation pastes the text into the window in front and
  restores the user's clipboard (plan step 7). Beyond the plan: our own console's input is cleared after every
  paste and old console windows get spaces for line breaks (pasted text in a console would run as PowerShell
  commands); `--record` never pastes; the key hook ignores injected keys (our Ctrl+V cannot disturb the
  tap / hold rules); waits up to 1 s for held modifiers.
- Tests: 3 unit tests (keys per window class, line breaks, saved formats) + 1 manual real-clipboard test.

## 2026-11-04 · feat/m1-format-baje

- PR #14 (graphify rule) merged.
- `format-v1.md`: clock times in Urdu / Hindi are "N baje", also when the transcript has "N:00" (Gemini
  Transcribe writes "baje" that way: live "Kal 6:00 milte hain", fmt-09). "Hmm" stays: the owner wants it.
- Checks: runs `fmt-baje-app2` (fmt-09 right; Layout 88%, the same as the old prompt's re-run `fmt-old-app`:
  fmt-04's recipe steps merge in both), `fmt-baje-m0` (no word loss, first wording), `fmt-baje-m0-final`
  (stopped at 8 clips by Groq's rate limit, unchanged).
- Punctuation measured (no change): the format step keeps Gemini's sentence ends and drops about half its
  commas; waiting for the owner's live examples.
- STATUS Next: M1's open steps (paste, key, latency log) before the mic delay and the chain benchmark.

## 2026-11-04 · docs/graphify-rule

- PR #13 (model catalog) merged after CI passed.
- CLAUDE.md graphify rule: `explain <file>::<symbol>` / `path` first for code, `query` with a budget for open
  questions; spec JSON is not in the graph and Markdown only as headings that `graphify update` does not
  refresh, so those are read directly. Checked: all 39 code files in the graph (987 nodes), current.

## 2026-11-04 · feat/m1-catalog (model catalog refreshed)

- PR #12 (smart formatting) merged after CI passed.
- `spec/models.json` re-generated from the finished `m1-engines` run: Transcribe + cleanup 97.3% on all 44
  clips (was 97.9% on 22), Flash 97.5% (44 rows, 3 daily-limit errors), Flash-Lite 97.0%, Groq 89.2%, OpenAI
  94.7% / 93.8%.
- `spec/providers.json` catalog notes for Transcribe and both OpenAI models: the second step is the 120b format
  step while formatting is on, the 20b Roman cleanup with `--no-format`.
- `catalog.py`: the model page lists the chain in its order (default, then fallbacks) before other models;
  sorting by accuracy alone had put Flash (97.5%) above the default. Test added (39 `eval` tests).

## 2026-11-04 · feat/m1-format (live list / take-back try)

- Owner's second live try, 5 dictations: list, take-back (5 → 6) and lakh amount ("3,50,000") came out right;
  "6 baje" written "6:00" (prompt follow-up noted). Transcribe hit a per-minute 429, then its daily limit;
  both limit notices seen live for the first time; Groq Whisper answered the last clip after 7.6 s, garbled.
- PR #12 CI all green (`checks`, `docs-gate`, `rust`). STATUS: Next 1 is now only the owner's merge; C: has
  13 GB free.

## 2026-11-04 · feat/m1-format (hand-off)

- PR #12 opened. Owner's first live try with formatting: 2 natural dictations right (2.9 s / 3.8 s key-up →
  text); "hmm" kept, names as heard (no user word list in the app yet); no list dictated live yet.
- Docs brought up to date for a new session: STATUS (Start here, In progress, Next re-ordered: PR #12, catalog,
  mic delay, chain run with formatting, paste), known gaps 4 / 9 / 13, the M1 plan's Progress, README, blueprint
  §4.4 / §4.5, the Wispr research note, ARCHITECTURE (`reformat.py`); D-029 / D-030 headings note what D-031
  changed.

## 2026-11-04 · feat/m1-format (checks done, D-031)

- `format-v1.md` final tweaks from the audio checks: a lead-in ending in a full stop still makes a list (an
  "Office ke rules" example; the owner's rules clip fmt-03 stayed a sentence on both engines before), lakh
  grouping "never 250,000" and "1.5 lakh" → 1,50,000, "wo na" as a filler spelling.
- Scoring: "6:00" counts as "6"; fmt-11 also checks "wo na".
- Fix: `vad::tests::m0_recordings` read every WAV in `spec/fixtures/audio` and expected 44; with the 13
  `fmt-*` recordings it found 57 and failed (only on the owner's PC: CI has no recordings). It now reads the
  M0 clips only.
- Results: app `m1-format-app` Layout 96%, 0 errors, p50 2.6 s; `fmt-lite-v3` 96% after Flash-Lite and after
  Groq Whisper; no word loss on the 40 non-list M0 clips (`fmt-120b`, `fmt-120b-final`). Decision D-031.
- Next 4's bench ran (Transcribe all 44 M0 clips, Flash 41; it also asked 2 new `fmt` clips before Google's
  per-day 429); catalog left for its own PR. Groq: ~350K tokens on 120b in a day without a daily 429 (known
  gap 3 corrected).

## 2026-11-03 · feat/m1-format (smart formatting, D-030; not merged)

- Plan `docs/plans/2026-11-03-m1-smart-formatting.md`, approved by the owner with the defaults. One change
  while building: a top-level `format` step in `spec/providers.json` instead of four new `+fmt` engine ids and
  a second chain (same behaviour; engine ids and the catalog untouched).
- `spec/prompts/format-v1.md` (from `format-draft.md`): lists ("- " / "1."), take-backs, fillers, digits with
  lakh grouping, romanizing. Tightened after a text-only check: the list replaces the sentence (120b had
  written both on `ag-03`), joining words are never the lead-in, a sentence after a list stays (both models
  had dropped words on `lg-02`), an "actually" take-back example.
- Test set: 13 `format` sentences `fmt-01`..`fmt-13` with `say` (what to speak) and `format` checks;
  `record` shows `say`. Not recorded yet.
- `eval`: `format_score` (list kind / count, absent words, digits) per row (`format_rate`), "layout" column
  in the summary and report; accuracy, English and latency now come from the M0 clips only, the catalog too.
  `reformat` command (format step on saved transcripts); `bench --only` and `--format-model`; `clean_output`
  keeps line breaks; an optional format step that fails keeps the engine's text (`unformatted`).
- App: `spec::format()`, `Client::with_format` (replaces the engine's refine; optional after engines without
  one), `--no-format`, cleanup per line, multi-line console output, "Not formatted" notice, bench rows
  `app-...+fmt`. A missing Groq key turns formatting off with a notice.
- Found: Groq's free `gpt-oss-120b` / 20b allow 8K tokens/min and 200K/day; a format call is ~1,300 tokens,
  so ~160 formatted dictations a day (STATUS known gap 3). A first 20b / 120b run on saved transcripts was
  stopped half-way (the prompt changed under it) and deleted; run `fmt-say-120b` (the 13 sentences as text):
  12 of 13 right.
- Owner recorded the 13 `fmt` clips. Run `m1-format`: Groq Whisper + format, Layout 88.5% with both 120b and
  20b (20b lost words on fmt-11); Flash-Lite timed out on all 13 (Google). `spec/providers.json` `format`
  gained `rpm: 6` (benchmarks only: 8K tokens/min).

## 2026-11-03 · docs/handoff-formatting

- PR #10 (engine chain, D-029) merged after green CI.
- Hand-off: STATUS rewritten (current facts only, older results condensed, Next 1 = smart formatting with a full
  brief); README, blueprint §4.2 / §4.4 and the M1 plan's progress brought up to D-029 / D-030. A session for
  the formatting work could not start: drive C: has 3.4 GB free and a git worktree needs ~5.4 GB (STATUS
  Start here 3).

## 2026-11-03 · feat/m1-fallback (engine chain)

- Owner chose a chain of free Gemini models with Groq last (D-029, supersedes D-028): `spec/providers.json`
  `chain` (Transcribe + Groq Roman cleanup 4 s → Flash 3 s → Flash-Lite 3 s → Groq Whisper) replaces
  `default_engine` / `fallback_engine`; `spec::chain()`.
- `client::Racer` became `client::Chain`: any number of engines, each with its wait; errors move on at once; a
  429 rests the engine (1 h for a daily quota, else ≥ 60 s) so later clips skip it without a request.
  `ApiError::RateLimited` gained `daily` (Google's `...PerDay...` quota; never retried); `Transcript` /
  `Outcome` list the engines `passed` over (`Slow` / `Failed` / `Resting`); `Transcribe` gained `id`.
- App: an engine with a missing key is left out of the chain with a notice; dictation prints which model
  answered and, once, that a model's free requests are used up (when they come back, how to change the main
  model). Bench rows `app-<a>~<b>~...` with `passed`; the summary counts answers per engine.
- Catalog: `fallback` is the place in the chain; `--models` shows "fallback 1 / 2 / 3".
- Run `m1-chain`: 94.9%, 0 errors, p50 3.3 s / p95 4.3 s; Transcribe 98.5% on its 24 clips.
- Research: Wispr Flow's formatting (`docs/research/2026-11-03-wispr-flow.md`) and a smart-list formatting test
  (`docs/research/format-test/`): works in ~1 s on Groq; not wired in. Owner's choices: D-030 (formatting on by
  default, one formatter for every app, `gpt-oss-120b` after every engine) and 3 s waits for Flash / Flash-Lite
  (run `m1-chain` used 2 s).

## 2026-11-03 · feat/m1-fallback

- Default engine is now `gemini-3.5-flash-lite` with `groq-whisper-hint` as the fallback (D-028):
  `spec/providers.json` `default_engine` / new `fallback_engine`, `spec::fallback_engine()`.
- `desktop/core` `client::Racer`: primary on a thread, the fallback too after 3 s (`FALLBACK_AFTER`) or on a
  primary error, first good answer wins, both failing returns the primary's error. `Transcript` and `Outcome`
  say which engine answered and why the fallback was asked (`api::Fallback`); `Transcribe` gained `warm`.
- App: without `--engine` the pair runs (a missing key runs the other engine alone, with a notice); `--engine
  <id>` runs one engine alone. Dictation prints "Used groq-whisper-hint: ... was slow" when Groq's text is used.
  `--bench` rows are named `app-<primary>~<fallback>` with `answered_by` and `fallback`; the summary counts
  fallbacks.
- Catalog: `fallback` flag per model; `--models` marks it. `spec/models.json` regenerated (same runs).
- Run `m1-fallback`: 95.5% on speech clips, 0 errors, p50 1.5 s / p95 3.4 s; Groq answered 10 of 42.

## 2026-11-03 · docs/handoff-default-engine

- PRs #6 (step 4), #7 (step 5) and #8 (engines + catalog) merged into `main` after green CI.
- Owner chose the default engine: Gemini 3.5 Flash-Lite with Groq Whisper as the fallback (D-028); not built yet.
- Key test passed with `sonicscript.exe` in a private test window: Enter and Esc are taken only while recording,
  Right Ctrl + C still copies. It also measured the mic-open delay (406 ms of a 1.5 s recording lost).
- The VS Build Tools CMake folder added to the owner's user PATH.
- STATUS rewritten as a hand-off: "Start here" and an ordered "Next" for the next session.

## 2026-11-03 · feat/m1-engines

- Owner's live test of step 5: tap / hold, mic and key-up → text in 0.2 s work, but Groq Whisper mangled natural
  Urdu / Hindi. New rule (D-025): quality first, 1-2 s after the key-up is fine.
- Researched how the big apps dictate (Claude: Anthropic's own transcription service; ChatGPT: GPT-4o-based
  transcription; Gemini app: Gemini 3.5 Transcribe; Wispr Flow: recognizer + LLM cleanup).
- New engines (D-026): OpenAI `gpt-4o-transcribe` / `gpt-4o-mini-transcribe` (provider `openai`, paid),
  `gemini-3.5-flash`, `gemini-3.5-transcribe` (new kind `gemini-transcribe`) and a `refine` step (Roman cleanup
  on Groq `gpt-oss-20b`) usable after any engine; per-engine `rpm`; provider `min_billed_s`. `eval`:
  `GeminiTranscribe`, `Refiner`; `OPENAI_API_KEY` read like the other keys. `gemini-3.8-flash` tried and
  dropped (every request timed out or got 503 on the free tier; its rows are kept aside in the run folder).
- App: shared HTTP layer `desktop/core/src/api.rs` (`ApiError` replaces `GroqError`), `gemini.rs`, `client.rs`
  (speech call + refine), `spec.rs` with an `Engine` enum; `sonicscript --engine <id>` for every mode.
- Model catalog (D-027): `catalog` info in `spec/providers.json` (label, free / paid, free-tier limits + source,
  notes), `python -m sonic_eval catalog` → `spec/models.json`, `sonicscript --models` prints it free / paid.
- Benchmark `m1-engines` (44 M0 clips) and the same engines through the app (`m1-cli-*`): Gemini 97-98%,
  OpenAI 93-95%, Groq 89%; numbers in STATUS. Flash and Transcribe ran out of their free daily limit (20 / 25,
  from Google's own 429s) after 22-25 clips. OpenAI cost about $0.06 in all.
- Found on the way: the hidden Terminal pane in the desktop app throttles a program's console output (the
  bench advanced only while its tab was read), so long runs go to a log file instead.

## 2026-11-03 · feat/m1-hotkey

- M1 step 5, dictation key + recording. `sonicscript.exe` without arguments now listens for Right Ctrl: tap to
  start, tap or Enter to stop, hold to talk, Esc cancels, a shortcut with the key cancels (D-015); the text is
  printed in the console (pasting is step 7). New in `core`: `keys.rs` (the key rules as a tested state machine;
  a 300-400 ms press is a tap, D-023), `resample.rs` (any mic rate → 16 kHz) and `encode::Stream` (Opus while
  recording; the Ogg file is cut from the packets at the end, D-024); `pipeline::run_streamed`. New in `win`:
  `hotkey.rs` (low-level keyboard hook thread), `audio.rs` (cpal capture + worker), `dictate.rs`, and
  `--record <seconds>` as a mic check. The console is switched to UTF-8.
- `--bench` with the streaming encoder: 89.3% (vs 89.1% re-encoding the trimmed clip), Latin 100%.
- Not tried live: the PC was locked and had no microphone connected while this was built (STATUS known gap 8).
  Found on the way: Claude's shells run without a foreground window, so simulated keys only work when the PC
  is unlocked.

## 2026-11-03 · feat/m1-bench

- M1 step 4, golden check: new app crate `desktop/win` (binary `sonicscript.exe`) with `--file <wav>...`
  (prints the text) and `--bench <dir> [--out <file>]` (one row per clip in `eval`'s `results.jsonl` shape,
  paced by `rpm`, p50 / p95 summary). The Groq key is read from the process environment, then the Windows user
  environment. `spec.rs` now reads the engine's `audio_per_hour` price for the cost column.
- `eval` `rescore` fills in the category of rows that have none (the app's rows), so the app is scored by the
  same scorer and report as M0. `desktop/core/examples/transcribe.rs` removed (replaced by the app).
- Fix: the first run scored 87.2% vs M0's 89.1% on the same 41 clips. Root cause: the voice gate trimmed to
  speech + 200 ms, and with that little silence Whisper translated `mx-02` / `nm-02` into English and cut
  "Let's" to "La"; the whole clips came back right. Padding is now 500 ms (D-022): 89.1% accuracy, Latin 100%,
  English 86%, silence 100%, 0 errors, clip → text p50 298 ms / p95 387 ms. No trim at all: 88.8%.
- Housekeeping: C: was full (0 MB), which broke `git pull` and builds; npm and npx caches cleared (5.4 GB free).

## 2026-11-03 · feat/m1-encode-groq

- M1 step 3, encode + Groq: `desktop/core/src/encode.rs` (Ogg/Opus 16 kbps, VoIP, written with libopus and
  the `ogg` crate, no ffmpeg), `spec.rs` (engine + hint from `spec/providers.json`, new `default_engine` field),
  `groq.rs` (blocking reqwest + rustls, `warm()`, 10 s timeout, one retry on 429 / 5xx / dropped connection),
  `pipeline.rs` (gate → trim → Opus → Whisper → cleanup, timings per stage) and `examples/transcribe.rs`.
- Real run on the 44 M0 recordings (one pass, Groq): 0 errors, 0 retries, both silence clips not sent,
  accuracy 88.0% (M0 `groq-whisper-hint` opus16 with the same scorer: 84.4%), Latin-only 100%, English kept
  86.6% (same as M0), ~7.7 KB per clip, Whisper request p50 178 ms / p95 228 ms, clip → text p50 341 ms /
  p95 455 ms.
- Opus complexity tried at 5 (encode 2x faster) but accuracy fell to 86.9% and English kept to 82.9%: kept at
  10 (D-021). Encoding takes ~125 ms per 4 s of audio, so step 5 should encode while recording.
- The FLAC fallback was not needed: libopus builds, but needs CMake on PATH (D-021).

## 2026-11-03 · feat/m1-voice-gate

- M1 step 2, voice gate: `desktop/core/src/vad.rs` (`detect` → speech ms + the samples to keep, 200 ms padding)
  and `examples/gate.rs`. On the 44 M0 recordings: 42 speech clips sent, both silence clips dropped.
- Not a straight port of `eval` `trim_silence`: its fixed -42 dB threshold finds 3.6 s of "speech" in the
  room-noise clip `si-02`. The gate uses each clip's own noise floor + 9 dB and a zero-crossing limit (D-020);
  the thresholds were picked on 7 variants of every clip (no padding, room noise mixed in, +10 dB, quieter).
- `graphify` reinstalled (`pip install --user graphifyy`, 0.9.101, Python 3.14); graph rebuilt, code only.
- Docs check before a new session: README status (M0 done, tap-to-toggle per D-015, repo folders), a Progress
  section at the top of the M1 plan, STATUS "Start here" says PR #4 must be merged first. PR #3 merged.

## 2026-11-03 · feat/m1-normalizer

- Toolchain confirmed: Rust 1.99.1 + VS 2022 Build Tools (C++), a test program links and runs (plan step 0).
- M1 step 1, text cleanup: new Rust workspace `desktop/` with crate `sonicscript-core`
  (`core/src/normalize.rs`); rules as data in `spec/normalizer/rules.json`, 22 shared test cases in
  `spec/normalizer/cases.json` (most from real M0 failures). `examples/clean.rs` runs it on stdin.
- On the 131 M0 Groq-hint outputs: Latin-only 93% → 100%, silence 0% → 100%, accuracy 85.1% → 90.0%.
- Fix during the work: the first phantom filter matched any whole hint sentence and dropped the real test
  sentence `mx-01` ("PR merge kar do aur deployment check kar lo."), which is the hint's first sentence. Root
  cause: Whisper only echoes the hint's tail, so only runs ending with the last sentence count now (D-019).
- CI: new `rust` job on windows-latest (fmt, clippy `-D warnings`, test); `.shipshape/config` check commands
  include the Rust checks.

## 2026-11-02 · plan/m1-windows

- STATUS rewritten as a hand-off for a fresh session ("Start here"); Rust + C++ Build Tools installing.
- PR #1 (M0) merged into `main`. M1 plan written: `docs/plans/2026-11-02-m1-windows-push-to-talk.md`.
- Owner decisions: tap-to-toggle default with Enter to finish (D-015), pop-up mic moved into M2 (D-016), model
  catalog + preferred/fallback model with a learned deadline (D-017), changeable dictation key (D-018). Blueprint §3.1, §4.2, §8 updated.

## 2026-11-02 · feat/m0-benchmark (word list)

- Word-list test: new engine `groq-whisper-hint-words` (`spec/prompts/whisper-hint-words-v1.txt`). Names
  improve but Whisper starts translating Urdu to English; word list moves to the cleanup step (D-014).

## 2026-11-02 · feat/m0-benchmark (license, fixtures)

- License MIT (D-012): `LICENSE`, README, blueprint, `eval/pyproject.toml`.
- Report review done by the agent: 3 expected texts fixed where every engine of both providers agreed
  (D-013); new `rescore` command re-scores saved runs without API calls (keeps `results.jsonl.bak`). All
  engines gained ~0.5-1 point; ranking unchanged.

## 2026-11-02 · feat/m0-benchmark

- Graphify code knowledge graph (D-010): code-only, local, `graphify-out/` git-ignored, Claude Code hook in
  `.claude/settings.json`, rules in `CLAUDE.md`. `graphify` installed with pip and its folder added to PATH.
- Gemini re-test (2026-10-29 16:15 PK) recorded; engine decision final (D-011 supersedes D-009): Groq default,
  Gemini 3.5 Flash-Lite opt-in.

## 2026-10-28 · feat/m0-benchmark

- Ran M0 on the owner's 44 recordings (compression + engines + 3 extra Groq variants + Gemini re-run with a
  fresh key). Provisional engine decision D-009.
- Benchmark runs each provider in its own worker pool, so slow Gemini no longer blocks Groq; rate limiter is
  thread-safe; progress logs flush live.
- New engines: `groq-whisper-auto+gptoss20b`, `groq-whisper-hint+gptoss20b`.
- Keys: `eval/.env` now wins over global variables (lets the benchmark use its own key). `.env.*` git-ignored.

## 2026-10-27 · feat/m0-benchmark

- Milestone 0 benchmark tool (`eval/`, D-005): terminal recorder with level meter, engine probe, resumable
  benchmark (engines × audio formats × sentences, rate-limited for free tiers), spelling-tolerant scoring and an
  HTML report built on the design tokens.
- Shared spec: 42 test sentences + 2 silence clips (`spec/fixtures/sentences.json`), prompts (`spec/prompts/`),
  engines and prices (`spec/providers.json`).
- Engines: Gemini 3.5 / 3.1 Flash-Lite (one call), Groq Whisper turbo / v3 + gpt-oss-20b / 120b, Whisper with a
  Roman Urdu hint.
- Fix during development: Gemini invented text on silence; added the `[no speech]` rule (D-007). Dropped
  engines that free keys can't use (D-008).
- Keys are also read from the Windows user environment, so a key saved with `SetEnvironmentVariable` works
  without restarting.
- CI workflow (`.github/workflows/ci.yml`): ruff + pytest for `eval/`, plus the shipshape docs gate.

## 2026-10-27 · 0f2eb71

- Project memory and workflow set up with shipshape (D-001): docs/, AGENTS.md, .shipshape/, .githooks/.
- Blueprint rewritten as v1 (cloud-first, BYOK, Windows → Android → launch; D-002). Original kept in
  `archive/blueprint-v0.md`.
- Design system added: `DESIGN.md` + `design/tokens.css` (D-003).
