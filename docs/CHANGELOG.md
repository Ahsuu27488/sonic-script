# Changelog

> Newest first. Each entry: date, commit (or "uncommitted"), what changed, and for fixes the root cause.
> This is the history; STATUS.md holds only the current truth.

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
