# Decision Log

> Each entry: what was decided, **why**, and where it lives in the code. Numbering is permanent: never
> renumber or delete an entry. When a decision changes, add a new entry and mark the old heading
> *(superseded by D-NNN)*. A decision here beats anything that contradicts it elsewhere.

**D-001 Project memory and workflow set up with shipshape.** (2026-10-27)
Docs in `docs/` (STATUS, DECISIONS, ARCHITECTURE, CHANGELOG) are the project's memory and are loaded at every
session start; code commits must update STATUS and CHANGELOG (`.githooks/commit-msg`); work happens on branches
and lands through pull requests; the default branch is protected by `.githooks/pre-push`.
*Why:* AI coding sessions start with no memory; without written state and reasons, settled decisions get
re-made and working code gets broken. *Where:* `AGENTS.md`, `.shipshape/`, `.githooks/`, `docs/`.
Set up as a new project: solo, private for now (public at launch), nothing live.

**D-002 Product shape: cloud-first, bring-your-own-key, Windows then Android.** (2026-10-27) *(order of the focus pill superseded by D-016)*
Speech runs on free cloud tiers (Groq, Gemini) with the user's own API key; no SonicScript server. Order:
Windows → Android → public launch → focus pill, Ubuntu, streaming, iOS (iOS left to contributors: no Mac).
*Why:* the owner's Pixel 6 is too slow for good on-device models; BYOK keeps the project free to run and easy
to trust as open source. *Where:* `blueprint.md` §2, §5, §8, §12.

**D-003 One design language: the Voice Ring, dark-first, one Ember accent.** (2026-10-27)
Every surface uses `design/tokens.css` and the rules in `DESIGN.md`: Geist / Geist Mono, OKLCH palette with a
single warm amber accent, glass only on the floating HUD, reduced motion honoured.
*Why:* consistent, premium feel across HUD, settings, keyboard, report and website; amber avoids the generic
purple AI look and alarm red, and is not a political colour in Pakistan or India. *Where:* `DESIGN.md`,
`design/tokens.css`.

**D-004 Voice recordings stay out of git.** (2026-10-27)
`spec/fixtures/audio/` is git-ignored; only the sentence list is committed.
*Why:* recordings of the owner's voice are personal data and the repo will become public; publishing them is
the owner's call later. *Where:* `.gitignore`.

**D-005 Benchmark in Python; the Windows app in Rust.** (2026-10-27)
M0 (`eval/`) is a Python package; the always-on desktop app (M1+) will be Rust with a Tauri shell.
*Why:* the benchmark only records audio and calls APIs, so Python is fastest to build; the desktop app runs all
day and must be small, low on memory and less likely to be flagged by antivirus. *Where:* `eval/`,
`blueprint.md` §5.2.

**D-006 Default hotkey: hold Right Ctrl.** (2026-10-27) *(superseded by D-015)*
Push-to-talk on Right Ctrl, configurable (mouse side button offered).
*Why:* one key, does nothing on its own in almost every app, and avoids the terminal shortcuts that Claude Code
and Codex users rely on. *Where:* `blueprint.md` §3.1, §12 (implemented in M1).

**D-007 Engines must write nothing when nobody speaks: no-speech marker now, voice detection in the apps.**
(2026-10-27)
The Gemini prompt asks for `[no speech]` on silence/noise and `clean_output` turns it into empty text; the test
set includes two silence clips and the report's "Quiet" column scores them. The apps will gate audio with a
local voice detector before calling any engine.
*Why:* in the first probe Gemini invented a full sentence from 1 s of near-silence, and all Whisper engines
still invent words ("shukriya", "music"); in a dictation app that text would be typed into the user's chat.
*Where:* `spec/prompts/transcribe-roman-v1.md` rule 8, `eval/sonic_eval/engines.py` `clean_output`,
`eval/sonic_eval/scoring.py` `score`.

**D-008 Engine candidates follow what free keys can actually use.** (2026-10-27)
`gemini-2.5-flash-lite` dropped (closed to new users); Groq `llama-3.1-8b-instant` replaced by
`openai/gpt-oss-120b` (not offered on standard keys); `whisper-large-v3` (full) added next to the turbo model.
*Why:* found by `probe` with the owner's keys; the benchmark must test engines users can really get.
*Where:* `spec/providers.json`.

**D-009 (provisional, superseded by D-011) Default engine: Groq Whisper turbo with a Roman Urdu hint; audio Opus 16 kbps.** (2026-10-28)
Default `groq-whisper-hint` (fastest: p50 0.25 s) with a local cleanup for stray Urdu letters, or
`groq-whisper-hint+gptoss20b` (~1.2 s, 100% Latin) if cleanup is not enough. Gemini 3.5 Flash-Lite is the
quality option, not the default, until its latency is re-tested. Whisper must not be forced to `language=ur`.
*Why:* on the owner's 44 recordings Gemini was most accurate (96-97%) but took 12-16 s and Gemini 3.1 mostly
failed with 503 "high demand", also with a fresh key; Groq never failed and was 5-50x faster; forcing Urdu cut
English words from ~87% to ~70%. *Where:* `spec/providers.json`, results in `eval/results/m0/` (local).
Final once Gemini is re-tested at a quiet hour (STATUS Next 1).

**D-010 Code knowledge graph with Graphify, code only, built locally.** (2026-11-02)
Graphify (PyPI `graphifyy`, MIT) builds `graphify-out/` from the code with tree-sitter; agents query it before
grepping (`graphify query`), and a Claude Code PreToolUse hook (`.claude/settings.json`) reminds them. Only
`--code-only` builds with `GRAPHIFY_BACKEND=none`; `graphify-out/` is git-ignored; `.graphifyignore` keeps out
the voice recordings and results. No git hooks (they would clash with `.githooks/`); rebuild with
`graphify update .` after code changes.
*Why:* the owner asked for it to save agent tokens as the code grows (M1 adds a Rust app). The non-code mode
sends docs to an LLM chosen by whichever key is set, which would push the docs to the Gemini key without asking;
code-only keeps everything on the PC. *Where:* `CLAUDE.md` (graphify section), `.claude/settings.json`,
`.graphifyignore`, `.gitignore`.

**D-011 Default engine is Groq; Gemini 3.5 Flash-Lite is an opt-in accuracy mode.** (2026-11-02)
Supersedes D-009. Default: Groq Whisper turbo with the Roman Urdu hint, audio Opus 16 kbps, never forced to
`language=ur`. M1 builds a local script-guard (stray Urdu letters → removed or transliterated); if it is not
enough, the default becomes `groq-whisper-hint+gptoss20b`. Gemini 3.5 Flash-Lite is a selectable "accuracy"
engine with a client timeout and a Groq fallback; `gemini-3.1-flash-lite` is not offered.
*Why:* the re-test (run `m0-gemini-am`, 2026-10-29 16:15 PK, 44 clips) gave Gemini 96.7% accuracy and 95% English
kept, p50 1.8 s, but 14 of 44 requests took 10-29 s (p95 23 s) and 1 timed out: the tail is Google's queue, not
the network. The target is p95 ≤ 1.2 s (blueprint §2); Groq hint meets it (p95 1.2 s, no failures).
*Where:* `spec/providers.json`, results in `eval/results/m0/` and `eval/results/m0-gemini-am/` (local).

**D-012 License: MIT.** (2026-11-02)
The project is MIT-licensed (`LICENSE`, `eval/pyproject.toml`); this replaces the Apache-2.0 proposal in
`blueprint.md` §5/§12. *Why:* the owner's choice: the shortest, best-known permissive license, easy for
contributors; the patent grant of Apache-2.0 matters little for a free BYOK app. *Where:* `LICENSE`,
`README.md`, `blueprint.md`, `eval/pyproject.toml`.

**D-013 Fix expected text only when every engine of both providers agrees.** (2026-11-02)
Three test sentences were corrected (`ur-04` "kar raha tha", `mx-04` "aur", `mx-06` no "environment") because
Gemini and Groq outputs all agreed with each other and not with the old text; saved runs were re-scored with
`rescore` (no API calls). Engine-specific mishearings (`ag-02` "unused", `ag-05` "Codex") stay as they are.
*Why:* a wrong expected text lowers every engine unfairly, but "fixing" it from one engine's output would bias
the test toward that engine. *Where:* `spec/fixtures/sentences.json`, `eval/sonic_eval/bench.py` `rescore`.

**D-014 The user's word list is applied after Whisper, never inside the Whisper prompt.** (2026-11-02)
In the apps, the word list ("Gemini, Groq, Codex, RAM, ...") fixes spellings in the local cleanup step (fuzzy
match, e.g. "Jaminai" → "Gemini") or in the LLM step (`romanize-v1.md` already has `{word_list}`). The Whisper
prompt stays the short Roman Urdu style hint. *Why:* engine `groq-whisper-hint-words` (word list + hint) fixed
names (English kept 87% → 91%) but made Whisper translate pure Urdu into English ("I'll reach the house for a
little while") and write Devanagari / Urdu script: accuracy 89.2% → 84.8%, Latin 93% → 88%. *Where:*
`spec/prompts/whisper-hint-words-v1.txt`, `spec/providers.json` (kept to reproduce), run `m0` (local).

**D-015 One key, two ways: tap Right Ctrl to toggle (default), hold it to talk; Enter stops and pastes.**
(2026-11-02) Supersedes D-006. Tap Right Ctrl (< 300 ms) starts recording; tap it again or press Enter to stop,
transcribe and paste; Esc cancels. Holding it (> 400 ms) is push-to-talk. While recording, Enter and Esc are
taken by SonicScript (not passed to the app); at all other times no key is ever swallowed. Enter never sends
the message: an "Enter also sends" setting exists, off by default. The key is configurable.
*Why:* the owner prefers tap-to-start over holding; Enter to finish is natural in chat boxes; sending
automatically would post a wrong transcription before the user can see it. *Where:* M1 plan step 5,
`blueprint.md` §3.1.

**D-016 The pop-up mic on text fields comes in M2, before launch (was M5, after launch).** (2026-11-02)
When a text field gets focus (UI Automation), a small mic pill appears near it, not recording; the D-015 key or
a click on the pill starts it. Never on password fields, per-app off switch (terminals off by default), hides
when focus leaves. Built with the HUD in M2; M1 ships the key modes only.
*Why:* it is the owner's preferred way to dictate, so it should be in v1.0; it needs the overlay window and
text-field detection, which are M2 work anyway. Auto-starting the mic on focus stays out (blueprint §3.1:
background speech would be typed into chats). *Where:* `blueprint.md` §3.1, §8.

**D-017 Model catalog, preferred + fallback model, and a self-learning deadline.** (2026-11-02)
`spec/models.json` is generated from the benchmark (accuracy, English kept, % Roman, p50 / p95, failure rate,
cost, free-tier limits, test date, a Recommended flag = the D-011 default) and shown in the settings page next
to the key fields. The user picks a preferred and a fallback model. The app learns each model's time per
second of audio from its last 50 successful requests; a request whose time passes about 2x the expected value
(minimum 3 s) starts the fallback in parallel, the first answer wins, and a short notice says why ("Used Groq:
Gemini was slow, 9 s"). Clips over 2 minutes are split at pauses. *Why:* Gemini is most accurate but has a
long tail (p50 1.8 s, p95 23 s); racing the fallback at the deadline keeps the quality when Gemini is fast and
caps the wait when it is not, without a fixed timeout that is wrong for long clips. *Where:* `spec/models.json`
(M1, from `eval`), settings + fallback in M2, `blueprint.md` §4.2, §7.

**D-018 The dictation key is the user's choice.** (2026-11-02)
Right Ctrl is only the default. M1: `config.toml` + `sonicscript.exe --set-key`; M2: a "press a key" picker in
settings. Allowed: one-sided modifiers, F-keys, Pause, Scroll Lock, Caps Lock, Insert, mouse side buttons.
Refused: keys that type or edit text, and Enter / Esc (they stop and cancel a recording). Tap, hold and cancel
rules (D-015) are the same for every key. *Why:* the owner asked for it; some users already use Right Ctrl
(e.g. as their only Ctrl, or for a VM / remote desktop host key). *Where:* M1 plan step 6, `blueprint.md` §3.1.

**D-019 Text cleanup rules: only the hint's tail is a phantom; fuzzy word-list matches need 3 consonants.**
(2026-11-03) Output becomes empty only when it matches the Whisper hint's last sentence(s) (the last one, the
last two, ... the whole hint) or a phantom phrase within 1 word in 4, or is filler only ("Tch, tch"). The
word list fixes exact matches (any case → the list's spelling) and sound-alikes with the same sound
skeleton ("Jaminai" = "Gemini", "Codec" = "Codex") only when the skeleton has at least 3 consonants. Output
with more than 30% Urdu / Devanagari letters is romanized letter by letter and flagged `needs_llm`.
*Why:* on silence Whisper only ever echoed the hint's tail; matching every hint sentence dropped the real
sentence `mx-01`, which is the hint's first sentence. Short words ("code", "cold", "room") would otherwise be
"corrected" into list words. Cost: dictating exactly the hint's last sentence is dropped (STATUS known gap).
*Where:* `spec/normalizer/rules.json`, `desktop/core/src/normalize.rs`, `spec/normalizer/cases.json`.

**D-020 Voice gate: relative energy + zero-crossing, at least 300 ms of speech.** (2026-11-03) *(200 ms padding superseded by D-022)*
A 20 ms frame is speech when it is 9 dB above the clip's own noise floor (10th-percentile frame, never below
-50 dBFS) and its zero-crossing rate is below 0.15; a clip with less than 300 ms of speech is not sent. The
clip is trimmed to the speech frames plus 200 ms on each side. No speech model (Silero) for now.
*Why:* the fixed -42 dB threshold of `eval` `trim_silence` finds 3.6 s of "speech" in the M0 room-noise clip
`si-02`, so it would still send silence to Whisper (D-007). Energy alone could not separate the cases (margin
12 dB passed `si-02`, 15-18 dB dropped real speech with no pause or in noise); adding the zero-crossing limit
gives silence 20-80 ms and the weakest real speech variant ~720 ms (Python prototype), a wide gap on both sides of 300 ms.
*Where:* `desktop/core/src/vad.rs` (constants at the top), test `m0_recordings`.

<!-- Template for the next entry:

**D-00N <Decision in one line>.** (YYYY-MM-DD)
<What exactly was decided.>
*Why:* <the problem, the alternatives considered, why this one.> *Where:* <files / functions / settings>.
-->

**D-021 Opus is encoded in the app with libopus at complexity 10; HTTP is reqwest + rustls; the default
engine is named in the spec.** (2026-11-03)
`encode.rs` writes Ogg/Opus 16 kbps VoIP itself (crates `opus` → `opusic-sys`, a static libopus 1.6 built with
CMake, and `ogg`); no ffmpeg in the app. Complexity stays 10 (libopus's and M0's default). `groq.rs` uses
blocking reqwest with rustls (aws-lc). `spec/providers.json` `default_engine` names the engine the apps use
(`groq-whisper-hint`, D-011). *Why:* the same format as M0's `opus16` keeps M0's accuracy (88.0% on the 44
clips vs 84.4% for M0's run); complexity 5 encoded 2x faster but lost 1 point of accuracy and 4 of English
kept; a built-in encoder means no ffmpeg to ship. Cost: building needs CMake on PATH (VS Build Tools ships one;
CI's windows-latest has it), and encoding takes ~1/32 of the clip's length, so step 5 encodes while recording.
*Where:* `desktop/core/src/encode.rs` (`COMPLEXITY`), `groq.rs`, `spec.rs`, `spec/providers.json`.

**D-022 The voice gate keeps 500 ms of audio around the speech (was 200 ms).** (2026-11-03)
Supersedes the padding in D-020; the gate itself is unchanged. The clip sent to Whisper is the speech frames plus
500 ms on each side (`vad::PAD`). *Why:* the step 4 golden check (`sonicscript --bench`, 44 M0 clips, scored by
`eval rescore`) gave 87.2% accuracy with 200 ms against M0's 89.1% on the same 41 clips: with less silence around
the speech Whisper translated `mx-02` and `nm-02` into English and cut "Let's" to "La" (`en-03`); the same
clips sent whole came back right. 500 ms gave 89.0% (M0 level), no trim at all 88.7%. Cost: ~0.6 s more audio
per clip, no measurable latency (p50 298 ms vs 286 ms), and Groq bills 10 s minimum per request anyway.
*Where:* `desktop/core/src/vad.rs` (`PAD`), runs `eval/results/m1-bench`, `m1-pad500`, `m1-notrim` (local).

**D-023 Tap or hold is decided at 400 ms; a 300-400 ms press counts as a tap.** (2026-11-03)
D-015 gives a tap as < 300 ms and a hold as > 400 ms; the gap between them is now a (slow) tap: the key comes
up, recording goes on in toggle mode. *Why:* staying in toggle mode loses nothing (the next tap or Enter
stops it), while a push-to-talk stop after less than 0.4 s would send a clip with no words in it, which looks
like the app ignored the key. *Where:* `desktop/core/src/keys.rs` (`HOLD_MS`, test
`the_gap_between_tap_and_hold_counts_as_a_tap`).

**D-024 Recording: cpal (WASAPI shared), own resampler, Opus encoded while recording.** (2026-11-03)
The default input device is opened through cpal at its own rate, mixed to mono and resampled to 16 kHz by a
windowed-sinc resampler in `core` (cut-off 7.2 kHz); a worker thread encodes each 20 ms frame to Opus as it
arrives. When recording stops, the voice gate runs on the whole clip and the Ogg file is built from the packets
that cover what it keeps (speech ± 500 ms, D-022), so only the last frame is encoded after the key-up.
*Why:* encoding takes ~1/32 of the clip's length (a 30 s clip ~1 s), which would come after the key-up
otherwise (known gap from step 3). Cutting the packet stream instead of re-encoding the trimmed clip scored
89.3% on the 44 M0 clips (re-encoding: 89.1%), so starting the decoder mid-stream costs nothing. Shared mode
runs at the mix rate (usually 48 kHz), so resampling is needed; a small resampler in `core` is tested on
48 / 44.1 / 32 kHz and keeps Android's path the same. *Where:* `desktop/core/src/encode.rs` (`Stream`),
`resample.rs`, `pipeline.rs` (`run_streamed`), `desktop/win/src/audio.rs`, run `eval/results/m1-stream` (local).

**D-025 Quality first: up to about 2 s after the key-up is fine.** (2026-11-03)
The owner's live test of step 5 got the text 0.2 s after the key-up, but Groq Whisper mangled natural Urdu /
Hindi ("terni" for "tareeke se"). The owner's rule now: the text should be right; 1-2 s of "loading" after
stopping is normal (Claude, ChatGPT and the Gemini app all take that long) and nobody minds it. This replaces
the p95 <= 1.2 s target behind D-011; the default engine is chosen by accuracy among engines whose typical wait
is about 2 s or less, with a fallback for slow requests (D-017). *Why:* the owner's words; the big apps run a
careful pass over the whole clip (an LLM-based recognizer, often followed by a cleanup model), which is what
makes their text read "refined". *Where:* `spec/providers.json` `default_engine`, `blueprint.md` §2.

**D-026 More engines: OpenAI transcription, Gemini Flash and the Gemini app's transcribe model, each with an
optional Roman `refine` step; Gemini 3.8 Flash dropped.** (2026-11-03)
`spec/providers.json` gains the `openai` provider (paid, no free tier), `gemini-3.5-flash`,
`gemini-3.5-transcribe` (kind `gemini-transcribe`: audio only, the text comes back in `audioTranscription`) and
a `refine` field: a text-only chat step on another provider (Groq `gpt-oss-20b` with `romanize-v1.md`) after any
engine. The transcribe model and both OpenAI models hear Urdu / Hindi well but write it in Urdu or Devanagari
script even with the Roman hint, so they always get the refine step. Both `eval` (`GeminiTranscribe`,
`Refiner`) and the app (`desktop/core` `api.rs`, `gemini.rs`, `client.rs`; `sonicscript --engine <id>`) run
every kind. `gemini-3.8-flash` was tried and dropped the same day: on the free tier every request timed out or
got 503 "high demand". *Why:* the owner asked for the models behind the big dictation apps to be compared on
his recordings; one shared spec keeps `eval` and the app measuring the same thing. *Where:* `spec/providers.json`,
`eval/sonic_eval/engines.py`, `desktop/core/src/{api,gemini,client,spec}.rs`, run `eval/results/m1-engines`.

**D-027 The model page lists every model as free or paid, with its free-tier limits and our results.**
(2026-11-03) `spec/models.json` (written by `python -m sonic_eval catalog`, read by `sonicscript --models` now
and the settings page in M2) holds per model: label, tier, the keys it needs, free-tier limits with their source,
notes (including that Google may use free-tier Gemini requests to improve its products), and the benchmark
results (accuracy, English kept, Roman only, silence, typical / slowest-5% wait, errors, paid price per hour of
speech), plus the app's own run as a check. Free-tier limits come from the provider's docs (Groq) or Google's own
quota errors for the owner's key (Gemini Flash 20 requests/day, Transcribe 25/day, 2026-11-03), never guessed.
*Why:* the owner asked for a model-selection page with tiers, limits and results; the daily limits decide
which free models can be a default at all. *Where:* `spec/providers.json` `catalog`, `eval/sonic_eval/catalog.py`,
`spec/models.json`, `desktop/win/src/models.rs`.

**D-028 Default engine: Gemini 3.5 Flash-Lite, with Groq Whisper as the fallback for slow or failed requests.**
(2026-11-03) *(superseded by D-029)* Supersedes the default in D-011 (Groq). The app sends the clip to `gemini-3.5-flash-lite`; if no
answer has come after a deadline (start with a fixed 3 s; D-017's learned deadline later), it also sends it to
`groq-whisper-hint`, and the first good answer is pasted, with a short notice when Groq's was used. A Gemini
error (429, 5xx, timeout) goes to Groq at once. Without a Gemini key the app runs Groq alone. Gemini 3.5 Flash,
Gemini 3.5 Transcribe and the OpenAI models stay choices in the catalog (`--engine`). *Why:* the owner's choice
after the benchmark (D-025, D-027): Flash-Lite is 97.0% accurate (Groq 89.2%) with a free limit that fits daily
use (~500 requests/day), while Flash and Transcribe allow only 20 / 25 a day; but some Flash-Lite requests take
10-20 s (2 of 44 hit the 10 s timeout in the app), and the fallback caps that wait. *Where:*
`spec/providers.json` (`default_engine`, `fallback_engine`), `desktop/core/src/client.rs` (`Racer`,
`FALLBACK_AFTER`), `desktop/win/src/key.rs` (pair or `--engine` alone), `dictate.rs` (notice); built
2026-11-03, run `eval/results/m1-fallback` (local).

**D-029 Engine chain: Gemini 3.5 Transcribe (+ Roman cleanup on Groq) → Gemini 3.5 Flash → Gemini 3.5 Flash-Lite
→ Groq Whisper; a model out of free requests is skipped and the user is told.** (2026-11-03) *(the Groq
`gpt-oss-20b` Roman cleanup after Transcribe is replaced by the D-031 format step while formatting is on)*
Supersedes D-028. The clip goes to `gemini-3.5-transcribe+gptoss20b`; with no answer after 4 s, or on an error,
also to `gemini-3.5-flash`; 3 s later `gemini-3.5-flash-lite`; 3 s later `groq-whisper-hint`; the first good
answer wins, errors move on at once. A 429 that names a daily quota (`...PerDay...`) rests that model for an hour
(asked again hourly until Google's reset at midnight Pacific, about 1 PM in Pakistan), any other 429 for at least
60 s; resting models are skipped without a request, and the console tells the user once which model ran out, when
its requests come back and how to pick another main model (`--engine <id>`). `--engine <id>` runs one engine
alone. In M2 the settings page lets the user choose the main model and two fallbacks from the catalog (extends
D-017). OpenAI stays in the catalog as a paid choice, outside the chain. *Why:* the owner's choice after the
benchmark report: the Gemini models are the most accurate free engines (Transcribe + cleanup 97.9%, Flash 97.7%,
Flash-Lite 97.0%, Groq 89%) and each has its own daily free limit (owner's key: Transcribe 25, Flash 20,
Flash-Lite ~500), so the chain gives about 45 top-quality clips a day before Flash-Lite. Groq stays last (owner,
2026-11-03) because all Gemini models slow down together on Google's busy hours, and Groq `gpt-oss-20b` stays
the Roman cleanup after Transcribe (which writes Urdu / Devanagari script). The waits for Flash and Flash-Lite
were 2 s in run `m1-chain` (Groq then answered 13 of the 18 clips after Transcribe ran out, 88%); the owner chose
3 s each (2026-11-03) to keep more Gemini answers. Cost: when Google is slow on every model, Groq starts after
10 s (3 s under D-028), or 6 s once Transcribe is out of free requests, and a model asked during a race uses its own daily quota even when
it loses. *Where:* `spec/providers.json` `chain`, `desktop/core/src/spec.rs` `chain()`, `client.rs` `Chain`
(`DAILY_REST`), `api.rs` (`RateLimited.daily`, `Passed`), `desktop/win/src/key.rs`, `dictate.rs`
(`limit_notes`), `eval/sonic_eval/catalog.py` (`fallback` = place in the chain), run `eval/results/m1-chain`
(local).

**D-030 Smart formatting: one formatter for every app, on by default, on Groq `gpt-oss-120b`, after every
engine.** (2026-11-03) *(its "Where" line superseded by D-031: built as one `format` step, the Gemini audio
prompt unchanged)* The text is formatted the same way whichever app it goes into (no per-app styles, unlike
Wispr Flow): spoken lists (three or more items, tasks, rules or steps) become "- " lists, ordered steps "1." lists,
with the lead-in words as a first line; ordinary speech stays sentences. The same step removes empty fillers
("matlab", "woh na", "umm"), applies take-backs ("nahi nahi", "sorry", "actually") and writes numbers as digits
with lakh / crore grouping. On by default, with a setting to turn it off (exact words). The cleanup / format step
moves to Groq `gpt-oss-120b` and runs after every engine in the chain, Groq Whisper included (+~1 s on that path).
*Why:* the owner's choices (2026-11-03) after the Wispr Flow research (`docs/research/2026-11-03-wispr-flow.md`):
Wispr formats lists well but has no Roman Urdu and no auto-detected Hinglish. In the text-only test
(`docs/research/format-test/`) one prompt with two examples got lists, take-backs, numbers and Urdu-script
romanizing right in ~1 s; `gpt-oss-120b` got the most details right. Same formatting after every engine keeps the
text style from changing when the chain falls back. *Where:* to build (STATUS Next): `spec/prompts/` (format rules
in the refine prompt and the Gemini audio prompt), `spec/providers.json` (refine model, a refine step on the
Flash / Flash-Lite / Groq engines), spoken list sentences in `spec/fixtures/sentences.json` and a list score in
`eval`. Not built yet; the model choice is confirmed by that benchmark first.

**D-031 Smart formatting as built: one `format` step on Groq `gpt-oss-120b` after every engine; the Gemini
audio prompt stays as it is.** (2026-11-04)
Builds D-030 (its "Where" line changes). `spec/providers.json` `format` (Groq `openai/gpt-oss-120b`,
`reasoning_effort` low, `spec/prompts/format-v1.md`) runs after whichever engine answers, chain or `--engine`,
in place of that engine's own `refine` step (the prompt also romanizes, so Transcribe's 20b romanize step is
replaced). After an engine without a refine step (Flash, Flash-Lite, Groq Whisper) it is optional: a failed
format request keeps the engine's text and the console says "Not formatted"; after Transcribe it is required
(its text is in Urdu / Devanagari script). `--no-format` skips it and gives exactly the old output.
`transcribe-roman-v1.md` (the Gemini audio prompt) gets no format rules: formatting twice could fight, and
one step after every engine already makes the text look the same whatever answered. *Why:* the owner's
go-ahead on the plan (`docs/plans/2026-11-03-m1-smart-formatting.md`) with these defaults, and the check:
- word loss, 120b on saved transcripts of the 44 M0 clips (runs `fmt-120b`, `fmt-120b-final`): accuracy and
  English kept unchanged on the 40 clips that are not spoken lists (Transcribe 97.2 → 97.1%, Flash 97.4 →
  97.4%, Flash-Lite 96.9 → 96.9%, again with the final prompt); the two spoken lists (`ag-03`, `lg-02`) became lists;
- layout, the owner's 13 `fmt` recordings (run `fmt-lite-v3`): 96% of format checks after Flash-Lite and
  after Groq Whisper (misses: the filler "wo na" kept once; Whisper's garbled `fmt-13`);
- 120b over 20b: same layout on Groq Whisper's text (88.5% each, earlier prompt) but 20b paraphrased `fmt-11`
  ("Kya kal chutti hai?"), losing words; format step p50 ~0.7 s, p95 ~1.3 s.
Cost: one Groq request of ~1,300 tokens per dictation; the free tier allows 8K tokens/min (about 6
dictations a minute; STATUS known gap 3); past that the text comes out unformatted. *Where:*
`spec/providers.json` `format`, `spec/prompts/format-v1.md`, `desktop/core/src/spec.rs` `format()`,
`client.rs` `Client::with_format`, `normalize.rs` (per line), `desktop/win/src/key.rs` (`Choice.format`),
`main.rs` (`--no-format`), `eval/sonic_eval/scoring.py` `format_score`, `reformat.py`, sentences `fmt-01`..`13`.

**D-032 Dictation history: every recording and its text are kept on the PC, on by default.** (2026-11-04)
Dictation and `--record` save the whole recording (key-down to key-up, the same Ogg/Opus 16 kbps packets that
were encoded while recording: ~2 KB a second, ~7 MB per hour of speech) to
`%LOCALAPPDATA%\SonicScript\history\audio\<date>\<time>.ogg` and append one line to `history.jsonl`: the raw
engine text, the final text, which engines answered or were passed over, the app in front, the paste result and
every timing. This line is also the latency log of M1 plan step 9, which had planned "no audio and no text"
(text only with `--log-text`): that part of the plan is replaced. `--no-history` keeps nothing. Written after the
paste. Nothing is deleted automatically yet; nothing leaves the PC; the folder is outside the repo (D-004
still holds for `spec/fixtures/audio/`). *Why:* the owner asked for it before the live paste test (2026-11-04),
to check later what was said, what was recorded and what came back; compressed so it cannot fill the nearly full
drive C:. *Where:* `desktop/win/src/history.rs`, `dictate.rs` (`transcribe`, `open_history`), `main.rs`
(`--no-history`), `encode.rs` (`Stream::finish` reusable), `inject.rs` (`app_in_front`). Before launch (M2): a
settings switch, a size or age limit, and whether it starts on or off for other users.

**D-033 Groq Whisper is a last resort: asked only when every Gemini model has failed, never because they are
slow.** (2026-11-04) Changes D-029's timing for Groq (the Gemini order and waits stay: Transcribe, Flash after
4 s, Flash-Lite 3 s later). `spec/providers.json` marks `groq-whisper-hint` `"last_resort": true`; the chain
asks it only once every engine asked before it has failed or is resting (also when every Gemini model is out of
free requests, or the Gemini key is missing). *Why:* the owner's live try (2026-11-04, dictation history): with
Transcribe out of free requests and Flash / Flash-Lite slower than 3 s, Groq answered 3 of 16 clips after about
7 s, 2 of them garbled ("Nain, aare terny athrakaam ..."); the owner prefers waiting for Gemini (97%) to fast
wrong text from Groq (89%), in line with D-025. Gemini Flash-Lite's ~500 free requests a day per user's key make
it the free engine; Groq stays for Google outages. Cost: on a slow Google hour the wait is the fastest Gemini
model's, up to ~18 s in the worst case (Flash-Lite asked at 7 s, timing out at 10 s, then Groq). *Where:*
`spec/providers.json` `chain`, `desktop/core/src/spec.rs` (`Link.last_resort`), `client.rs`
(`Chain::with_last_resort`, `is_last_resort`), `desktop/win/src/key.rs` (`Choice.links`, `describe`).
