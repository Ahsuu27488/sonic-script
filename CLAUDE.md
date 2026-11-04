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

- **Code** (`.rs`, `.py`, scripts, CI): look it up in the graph before your first grep or source-file read, then
  read only the lines it points to. Put this rule in every subagent prompt that explores code.
  - A symbol you can name: `graphify explain "<file>::<symbol>"` (a bare name lists the matches when it is
    ambiguous): its file, line, callers and callees. Cheapest and most precise; use it first.
  - How two things connect: `graphify path "<A>" "<B>"`.
  - An open question: `graphify query "<question>" --budget 800`. It walks outwards from words in the question,
    so it also returns unrelated neighbours; take the file:line it points to, not the whole list.
- **Not code: grep or read directly.** `spec/*.json` (providers, sentences, normalizer rules) and
  `spec/models.json` are not in the graph; `eval/results/` and the recordings are left out on purpose
  (`.graphifyignore`). Markdown (docs, `spec/prompts/*.md`) is in the graph as headings only, and
  `graphify update .` does not refresh it, so the hook's "may be STALE" notice is expected for `.md` files:
  read them directly (STATUS / DECISIONS / ARCHITECTURE are in context already).
- `graphify-out/GRAPH_REPORT.md` only for broad architecture review. The docs above stay the source of truth for
  decisions and status; the graph only maps code.
- After changing code, run `graphify update .` (AST only, free). If `graphify-out/` is missing, build it:
  `GRAPHIFY_BACKEND=none graphify extract . --code-only` then `graphify cluster-only . --no-label`.
  Never run a non-code (LLM) extraction: it would send the docs to whichever API key is set (`GEMINI_API_KEY`).
  The doc refresh that `graphify update` suggests ("/graphify --update") is such an LLM extraction (D-010): skip
  it.
