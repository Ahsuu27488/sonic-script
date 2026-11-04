# Project Status

> Current truth only: what works now, what doesn't, what's next. History goes in CHANGELOG.md, reasons in
> DECISIONS.md. Updated in the same commit as every code change (the commit-msg hook checks). Keep it short:
> it is loaded into every agent session.

**Last updated:** 2026-11-04 · **Code at:** `feat/m1-paste` (paste at the cursor, M1 step 7; PR open, on top of
`feat/m1-format-baje`, PR #15); `main` = PRs #3-#14.
**In one line:** M1 (Windows dictation app in Rust): key, mic and pipeline work live; the default is an
**engine chain** (D-029): Gemini 3.5 Transcribe + Roman cleanup → Gemini 3.5 Flash → Flash-Lite → Groq Whisper,
skipping a model whose free requests are used up (run `m1-chain`: 94.9%, Transcribe's own answers 98.5%, 0 errors,
p95 4.3 s). **Paste at the cursor (step 7)** is built on `feat/m1-paste`, tests pass, not tried live yet. **Smart formatting (D-030, D-031)** is
merged (PR #12; on by default, `--no-format` for exact words; Layout 96%, no word loss; live list, take-back
and lakh amount right).

**Start here (new session):**
1. `git switch main && git pull`; run `sh .shipshape/context.sh`; read this file, then `DECISIONS.md` D-011 to
   D-031 (D-025 to D-031 are the newest and set the direction), `ARCHITECTURE.md` and the plan
   `docs/plans/2026-11-02-m1-windows-push-to-talk.md` (Progress at the top). Formatting details:
   `docs/plans/2026-11-03-m1-smart-formatting.md`.
2. Do "Next" below in order, one branch and PR per item (`feat/m1-<topic>`), each with its check.
3. **Work in this checkout, not a git worktree:** drive C: is nearly full (13 GB free on 2026-11-04, 3.4 GB on
   2026-11-03, when a worktree needing ~5.4 GB failed). `desktop/target` alone is 3.2 GB. Ask the owner before
   deleting anything.
4. Keys: `GROQ_API_KEY` and `GEMINI_API_KEY` are in the owner's Windows user environment; `OPENAI_API_KEY` only
   in `eval/.env` (paid: benchmark only, spend sparingly). Never print, log or commit a key. CMake is on the
   user PATH.
5. Gemini free limits per model, per day (owner's key): Transcribe 25, Flash 20, Flash-Lite ~500; they reset at
   midnight Pacific (about 1 PM in Pakistan). Every app or bench run that uses the chain spends them: plan
   benchmarks so two runs don't need the same day's quota.
6. Long runs (bench, tests) belong in your own shell with a log file: the desktop app's hidden Terminal pane
   throttles console output. Keys and mic need the owner's unlocked PC.

## What works (verified)

- **Engine chain (D-029, PR #10):** `sonicscript` without `--engine` sends the clip to
  `gemini-3.5-transcribe+gptoss20b`; after 4 s or an error also to `gemini-3.5-flash`; 3 s later
  `gemini-3.5-flash-lite`; 3 s later `groq-whisper-hint`; first good answer wins. A 429 naming Google's daily
  quota (`...PerDay...`) rests that model for an hour (other 429s ≥ 60 s): later clips skip it with no request,
  and dictation prints once that its free requests are used up, when they come back and how to change the main
  model (`--engine <id>`). An engine with a missing key is left out with a notice. Run `m1-chain` (44 M0 clips,
  scored by `eval rescore`, with 2 s waits for Flash / Flash-Lite; 3 s chosen afterwards, not benchmarked yet):
  94.9% on speech clips, English kept 93%, Latin-only 100%, silence 100%, 0 errors, p50 3.3 s / p95 4.3 s.
  Transcribe answered 24 clips (98.5%, typical 2.9 s) until Google's real daily-limit 429 on clip 26, then was
  skipped; Google was very slow that hour, so Groq answered 13 (88.1%), Flash 4, Flash-Lite 1.
- **Engines and catalog (D-026, D-027):** 44 M0 clips, opus16, scored by `eval` on speech clips:
  - `gemini-3.5-transcribe+gptoss20b` (the Gemini app's model + Roman cleanup on Groq): 97.3%, English kept
    95%, 2.8 s / 3.9 s, all 44 clips (run finished 2026-11-04; 97.9% on the first 22).
  - `gemini-3.5-flash`: 97.5%, English kept 94%, 2.0 s / 7.4 s, 41 of 44 clips (daily limit).
  - `spec/models.json` (catalog re-run 2026-11-04, PR #13) holds these numbers; the model
    page lists the chain in its order (default, fallback 1-3), then the rest by accuracy.
  - `gemini-3.5-flash-lite`: 97.0%, 1.6 s / 9.1 s; in the app 96.6%, 1.8 s / 6.3 s, 2 of 44 hit the 10 s timeout.
  - OpenAI (paid) `gpt-4o-mini-transcribe` / `gpt-4o-transcribe` + Roman cleanup: 94.7% / 93.8%, ~1.5 s / 2.6 s.
    Not in the chain; catalog only. Both write text on silence. About $0.06 spent in all.
  - `groq-whisper-hint` (last in the chain): 89.2%; in the app 89.3%, 0.3 s / 0.5 s.
  - `gemini-3.8-flash` and `gemini-3.1-flash-lite`: dropped (timeouts / 503 "high demand" on the free tier).
  - `sonicscript --models` prints the catalog (`spec/models.json`): free / paid, limits, results, the default
    (★) and fallbacks 1-3.
- **Dictation (M1 step 5, D-015, D-023, D-024):** tap / hold Right Ctrl, Enter stops, Esc cancels; cpal mic,
  own 16 kHz resampler, Opus encoded while recording. Live test by the owner (tap, hold, Realtek mic) and key
  test (Enter / Esc taken only while recording; Right Ctrl + C still copies) passed 2026-11-03.
- **Paste at the cursor (M1 step 7, branch `feat/m1-paste`, not tried live yet):** `inject::paste` saves the
  clipboard (every memory format: text, HTML, images as DIB, files), sets the text marked so Win+V history and
  cloud clipboard skip it, sends Ctrl+V (Ctrl+Shift+V in Windows Terminal / console windows) and puts the
  clipboard back after 300 ms unless something new was copied. Window running as administrator: text only
  copied, the console says to press Ctrl+V. Waits up to 1 s for Shift / Alt / Ctrl / Win to be let go. After
  every paste SonicScript clears its own console's input (a paste into its own window would otherwise run as
  commands in PowerShell after it quits); old console windows get line breaks as spaces. `--record` never
  pastes. The key hook ignores injected keys. Checked: unit tests (keys per window class, line breaks, saved
  formats) and a manual round-trip on the real clipboard (`cargo test -p sonicscript inject -- --ignored`).
- **Pipeline pieces (M1 steps 1-4):** text cleanup (D-019: script guard, phantom filter, word list), voice gate
  (D-020, D-022: sends all 42 speech clips, drops both silence clips, keeps 500 ms around speech), Ogg/Opus
  16 kbps (D-021: as accurate as WAV at 8 KB per clip), golden check `--bench` scored by `eval rescore`.
- **Smart formatting (D-030, branch `feat/m1-format`, plan `docs/plans/2026-11-03-m1-smart-formatting.md`):**
  `spec/providers.json` `format` (Groq `gpt-oss-120b` + `spec/prompts/format-v1.md`) runs after whichever
  engine answers, in place of its own refine step; optional after Flash / Flash-Lite / Groq (a failure keeps
  the engine's text and says "Not formatted"), required after Transcribe. `--no-format` = exact words. The
  cleanup keeps line breaks (lists). Checked 2026-11-04 (D-031), final prompt:
  - App (`sonicscript --bench` on the owner's 13 `fmt` clips, run `m1-format-app`): Layout 96%, 0 errors,
    clip → text p50 2.6 s / p95 3.8 s (format step p50 ~0.7 s); Transcribe answered all 13.
  - Layout after Flash-Lite and after Groq Whisper (run `fmt-lite-v3`): 96% each. Misses: the filler "wo na"
    kept (fmt-11), Whisper's garbled fmt-13.
  - Word loss (saved transcripts of the 44 M0 clips, runs `fmt-120b`, `fmt-120b-final`): accuracy and English
    kept unchanged on the 40 clips that are not spoken lists (Transcribe 97.2 → 97.1%, Flash 97.4 → 97.4%,
    Flash-Lite 96.9 → 96.9%); `ag-03` and `lg-02` (spoken lists) became lists.
  - 20b lost words on fmt-11 ("Kya kal chutti hai?"); 120b kept them.
  - Clock times (branch `feat/m1-format-baje`): Gemini Transcribe writes "6 baje" as "6:00"; the prompt now
    turns a whole hour in Urdu / Hindi into "N baje" (fmt-09 → "Theek hai, kal 6 baje milte hain."; the old
    prompt kept "6:00"). Layout 88% with the old and the new prompt alike (fmt-04's last 2 steps merged in both:
    model variation; 96% in the first run), word loss none: 96.1 → 96.4% on the M0 clips with the rule's first
    wording (run `fmt-baje-m0`), 8 clips unchanged with the final wording before Groq's rate limit stopped it.
  - Owner's live try (2026-11-04, branch `feat/m1-format`, tap and hold both used): 2 natural dictations (6 s and
    21 s), Transcribe answered, key-up → text 2.9 s / 3.8 s (format step 0.7 s / 1.9 s); ordinary talk stayed
    sentences, English kept. Seen: "Hmm" kept (the owner wants it kept, 2026-11-04); a run of to-dos said as
    sentences ("Maine Sonic script ka bhi kuch dekhna hai. ... Relay ... Taskman ki guides ...") stayed
    sentences; "Taskman" for Taskvane and "or" for "aur" (speech step; the app has no user word list yet).
  - Owner's second live try (2026-11-04, 5 dictations): a spoken list became the lead-in line + 3 "- " items;
    a take-back (5 → 6 o'clock) kept only the correction ("Kal 6:00 milte hain yaar, chal.": Gemini
    Transcribe writes "baje" as ":00"; fixed in the prompt afterwards, see Smart formatting); a lakh amount came out "3,50,000 rupaye"; a dollar amount "2,400
    dollar". Transcribe answered 3 (key-up → text 2.1-2.8 s, format step 0.7-1.0 s), then hit a per-minute
    429 (Flash answered, 3.6 s), then its daily limit (earlier bench runs had used most of it): Flash and
    Flash-Lite were slow, Groq Whisper answered after 7.6 s with garbled text (known gaps 1, 5). Both limit
    notices (per-minute and daily) showed live. A slow answer prints under the next "● Recording" line
    when the owner has already started the next clip.
- **M0 benchmark (`eval/`):** Whisper forced to `language=ur` wrecks English words (81-86%, English ~70%); the
  word list in the Whisper prompt makes it translate (D-014); every Groq engine invents text on silence
  (D-007, handled by the voice gate). Reports per run: `eval/results/<run>/report.html` (local only).
- **Tests (2026-11-04, `main`):** `eval`: 39 offline tests, ruff clean; `desktop`: 65 core + 11 app (+1 manual clipboard test)
  tests, fmt and clippy `-D warnings` clean (with the owner's recordings present: the voice gate test reads
  the 44 M0 clips only, not `fmt-*`).
- Code knowledge graph (D-010): works, all 39 code files (checked 2026-11-04); CLAUDE.md says which lookup
  fits (PR #14).

## In progress

- M1 (plan approved 2026-11-02): steps 0-5 and 8 (catalog) done; engine chain done early (D-029); smart
  formatting merged (D-031, PR #12); step 7 (paste) built, waiting for the owner's live try. Left: step 6 (changeable key,
  D-018), step 9 (latency log). Pop-up mic on text fields, settings page (main model + 2 fallbacks,
  formatting switch) are M2 (D-016, D-017).
- Punctuation: the owner finds the formatted text over-punctuated and wants about as much as the Gemini app's
  and Claude's dictation. Measured 2026-11-04 (runs `fmt-120b`, `m1-format-app`): after Transcribe the format
  step keeps Gemini's sentence ends (10.5 → 9.6 per 100 words on the M0 clips) and drops about half its commas
  (3.4 → 1.9; owner's clips 8.8 → 1.6), so no prompt change yet: waiting for the owner's live examples (Next 2).

## Live state

- Nothing is live.

## Known gaps

1. Every engine above 95% takes 1.5-3 s typically, which D-025 accepts. With 2 s waits, Groq answered 13 of
   the 18 clips after Transcribe ran out (88%) in `m1-chain`; the owner chose 3 s each for Flash and
   Flash-Lite, so on a slow Google day Groq now comes at ~6 s (~10 s while Transcribe still has free requests).
   Not benchmarked with 3 s yet; D-017's learned deadline comes later.
2. Every Gemini model has a slow tail (some requests 10-30 s) and they slow down together; Groq last caps the
   wait. A request that loses keeps running in the background (at most 10 s) and still uses its model's daily
   quota. Bench cost counts only the answer used. The "free requests used up" notice was first seen live
   2026-11-04; the owner has not said yet whether it reads well (Next 6).
3. Smart formatting is limited by Groq's free tokens per minute: each format call is ~1,300 tokens and
   `gpt-oss-120b` allows 8K tokens/min (Groq's `x-ratelimit-limit-tokens` header): about 6 formatted
   dictations a minute; past that the text comes out unformatted (after Transcribe, Flash answers instead).
   Groq's table also lists 200K tokens/day, but ~350K were used on 2026-11-04 with no daily 429, so the daily
   limit is not known. Options if it bites: a shorter prompt, 20b as a second formatter (its own quota), or
   Groq's paid tier. Benchmarks spend the same quota.
4. Names fail on Groq Whisper; the word list fixes sound-alikes ("Jaminai" → Gemini, "Codec" → Codex) but not
   words run together ("Algrock" = "aur Groq") or 2-consonant words ("rhyme" for RAM). The app does not load a
   user word list yet (`Normalizer::new()` without one; the format prompt's `{word_list}` slot is empty): names
   like Taskvane come out as heard ("Taskman", live try 2026-11-04). A word-list setting comes with the config
   file (step 6) or M2.
5. Groq Whisper sometimes translates instead of transcribing (`mx-04`, `nm-06`); worse with too little silence
   around the speech (D-022). Only matters when Groq answers (last in the chain).
6. Dictating exactly the Whisper hint's last sentence ("Theek hai, main abhi meeting mein hoon.") is dropped as
   a phantom (D-019).
7. The voice gate is tuned on one male voice (the owner) and one room; loud talking or TV counts as speech.
   Silero VAD only if this fails in daily use.
8. Opening the mic takes 160-690 ms after the key goes down, and that audio is lost (key test: 406 ms; likely
   clipped the owner's first word "Bhai yaar"). Not fixed (Next 4).
9. Urdu-script "میں" is always romanized "mein" (it can be "main" = I) by the local cleanup. With formatting on
   (D-031) the format step romanizes Urdu script first, so this matters only with `--no-format` or when the
   format step fails (then mostly-Urdu output from Groq gets only a rough letter-by-letter draft, `needs_llm`).
10. Free-tier Gemini requests may be used by Google to improve its products (Gemini API terms); the catalog
    says so. Prices for `gpt-oss-120b` and Gemini 3.x audio are from 2026-10-26; recheck before launch.
11. Building `desktop/` needs CMake on PATH (for libopus); on another PC: the VS Build Tools CMake folder or
    `winget install Kitware.CMake`. Drive C: is nearly full (see Start here 3).
12. Final name not decided (license is MIT, D-012). `main` is guarded by the pre-push hook only.
13. Smart formatting (D-031): the format step (~0.7 s typical, ~1.3 s slow) counts inside the chain's waits,
    so a later engine is asked a little more often; not measured on a full chain run yet (Next 5). The
    prompt still sometimes keeps the filler "wo na" (fmt-11) and sometimes merges the last two steps of the
    tea recipe (fmt-04: old and new prompt alike), and Whisper's garbled text cannot be formatted. "Hmm" is
    kept on purpose (owner). Prompt
    changes need a re-run of the checks (D-031's runs). Engine ids and labels still say "+gptoss20b" /
    "Roman cleanup" (the `--no-format` path); the catalog notes say the 120b format step replaces it.

## Next

1. **Owner: merge the format-prompt PR** (`feat/m1-format-baje`: "6:00" → "6 baje" in Urdu / Hindi).
2. **Owner: punctuation examples:** paste 2-3 outputs that felt over-punctuated (best: the same words said
   into Claude's dictation too); then the agent changes the prompt's punctuation rule and re-runs D-031's checks.
3. **Owner, then agent: try paste live** (`feat/m1-paste`; close any running SonicScript window first, then
   `cd desktop; cargo run -q --release -p sonicscript`): dictate into Notepad, WhatsApp, a browser, Claude in
   Windows Terminal (a list keeps its lines), with an image copied before (it must be on the clipboard again
   after), and into an app run as administrator (text copied, console says Ctrl+V); Win+V must show no
   dictated text. Agent: then merge, then step 6 (changeable key, D-018), then step 9 (latency log). M1's open
   steps come before anything else (owner, 2026-11-04: milestones unchanged, M1 first).
4. **Agent: mic-open delay (known gap 8):** measure which part of `audio::Recorder::start` is slow (device
   lookup, `build_input_stream`, `play`); move what can be prepared to startup. If only a mic kept open removes
   it, ask the owner first (Windows shows the mic-in-use icon the whole time). Check: the key test's
   "recorded" seconds match the time between the keys.
5. **Agent, after M1: benchmark the full chain with the 3 s waits and formatting** (run `m1-chain3`, the app benchmark
   command under "How to run", on the 44 M0 clips: copy them without `fmt-*` to a folder, or the 13 `fmt`
   clips spend Transcribe / Flash quota too), after 1 PM Pakistan time; update known gaps 1 and 13.
6. **Owner:** keep or change the name "SonicScript"; say if the "free requests used up" notice reads well when
   it shows up.

## How to run and check

- Setup: `cd eval && pip install -e ".[dev]"` (needs ffmpeg on PATH). Keys: `GROQ_API_KEY`, `GEMINI_API_KEY`
  in the environment, the Windows user environment, or `eval/.env`.
- Benchmark: `python -m sonic_eval record | probe | bench --preset compression|engines | report | rescore`.
- Dictate: `cd desktop && cargo run -q --release -p sonicscript` (tap / hold Right Ctrl; the text is printed).
  Mic check: `... -p sonicscript -- --record 5`.
- Engines: without `--engine` the chain runs (D-029); `--engine <id>` runs one engine alone (ids:
  `... -p sonicscript -- --models`). The app reads keys from the environment / Windows user environment only,
  not `eval/.env`. The format step runs after any engine; `--no-format` turns it off (exact words).
- Files: `cd desktop && cargo run -q --release -p sonicscript -- --file <wav>...` (prints the text, so test
  recordings only).
- App benchmark (golden check): `cd desktop && cargo run -q --release -p sonicscript -- --bench
  ../spec/fixtures/audio --out ../eval/results/<run>/results.jsonl`, then
  `cd eval && python -m sonic_eval rescore --run <run>`. The chain run spends Gemini's daily free requests.
- Try the cleanup / voice gate: `cd desktop && cargo run -q --example clean -- [words.txt]` (stdin lines),
  `cargo run -q --example gate -- <16 kHz mono .wav files>`.
- Formatting, text only: `cd eval && python -m sonic_eval reformat --from <run> --run <new> --model
  openai/gpt-oss-120b --rpm 6 [--engines ...]` (formats a run's saved transcripts and scores them; Groq
  tokens only). Audio: `bench ... --only format --format-model openai/gpt-oss-120b` (engine ids get
  `+fmt-120b`; the "layout" column is the format score). The older research script is
  `docs/research/format-test/format_test.py`.
- Catalog: `cd eval && python -m sonic_eval catalog --runs m1-engines m0 --app-runs m1-cli-groq-whisper-hint
  m1-cli-gemini-3.5-flash-lite m1-cli-openai-4o-mini-transcribe_gptoss20b m1-cli-openai-4o-transcribe_gptoss20b`
  writes `spec/models.json` (rebuild the app afterwards: it embeds the file).
- Tests: `cd eval && python -m ruff check . && python -m pytest -q` (38 tests) and
  `cd desktop && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.
- Code map: `graphify query "<question>"`; after code changes `graphify update .` (see CLAUDE.md).
