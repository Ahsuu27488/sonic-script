# SonicScript

Dictation for South Asians: speak Urdu, Hindi or English (mixed), get Roman Urdu / Hinglish text at the cursor.
Windows first, then Android. Open source (planned).

The working rules for every agent are in AGENTS.md; the project's memory is in docs/. All four are imported
below, so they are in every session's context. Trust them over your own memory, and keep them current.

@AGENTS.md
@docs/STATUS.md
@docs/DECISIONS.md
@docs/ARCHITECTURE.md

Read on demand: `blueprint.md` (product plan), `DESIGN.md` (design system), `docs/CHANGELOG.md` (history),
`docs/plans/` (plans for risky changes).

## Project-specific rules

- Python commands for the benchmark run from `eval/` (`cd eval && python -m sonic_eval ...`).
- The owner is on Windows: give PowerShell commands, and use forward slashes in Git Bash.

## graphify

A code knowledge graph (Graphify, D-010) lives in `graphify-out/` (git-ignored, built locally, code only: no LLM,
nothing leaves the PC). The `graphify` command is on the user PATH
(pip user Scripts folder); `python -m graphify` also works.

- For a codebase question, run `graphify query "<question>"` before your first grep or source-file read,
  and put this rule in every subagent prompt that explores code. Read raw files only for lines the graph located.
  `graphify path "<A>" "<B>"` for relationships, `graphify explain "<name>"` for one symbol.
- `graphify-out/GRAPH_REPORT.md` only for broad architecture review. The docs above stay the source of truth for
  decisions and status; the graph only maps code.
- After changing code, run `graphify update .` (AST only, free). If `graphify-out/` is missing, build it:
  `GRAPHIFY_BACKEND=none graphify extract . --code-only` then `graphify cluster-only . --no-label`.
  Never run a non-code (LLM) extraction: it would send the docs to whichever API key is set (`GEMINI_API_KEY`).
