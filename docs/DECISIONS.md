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

**D-002 Product shape: cloud-first, bring-your-own-key, Windows then Android.** (2026-10-27)
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

**D-006 Default hotkey: hold Right Ctrl.** (2026-10-27)
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

<!-- Template for the next entry:

**D-00N <Decision in one line>.** (YYYY-MM-DD)
<What exactly was decided.>
*Why:* <the problem, the alternatives considered, why this one.> *Where:* <files / functions / settings>.
-->
