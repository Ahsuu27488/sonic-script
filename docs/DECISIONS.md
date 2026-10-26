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

<!-- Template for the next entry:

**D-00N <Decision in one line>.** (YYYY-MM-DD)
<What exactly was decided.>
*Why:* <the problem, the alternatives considered, why this one.> *Where:* <files / functions / settings>.
-->
