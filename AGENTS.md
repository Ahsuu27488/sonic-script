<!-- shipshape:start (managed by shipshape; edit outside the markers) -->
## How to work in this repository (shipshape)

**Before writing any code, in every session:**
1. Run `sh .shipshape/context.sh` (Windows without sh: `pwsh -File .shipshape/context.ps1`) and act on its warnings.
2. Read `docs/STATUS.md` (where we are, what's next), `docs/DECISIONS.md` (what was decided and why; never undo a
   decision without a new entry) and `docs/ARCHITECTURE.md` (how the code connects).
3. Read the code you will change and its callers. Never build on a guess; if the docs and the code disagree, the
   code is the truth and the docs get fixed.
4. If a request contradicts a decision in DECISIONS.md, stop before editing: quote the decision and its reason, and
   ask the user to confirm. A confirmed change gets a new decision that supersedes the old one.

**Every change:** say the tier (T0 trivial / T1 normal / T2 risky: data, auth, money, deletes, live systems),
work on a branch (never commit to the default branch), name the check that proves it works, test the real code
path, read the whole diff, then update `docs/STATUS.md` + `docs/CHANGELOG.md` (and DECISIONS / ARCHITECTURE when
relevant) in the same commit. The `commit-msg` hook rejects code commits without them; `[no-docs]` is only for
changes with no behaviour change. Land through a pull request with What / Why / How verified / Risk.

**T2 and anything live:** written plan in `docs/plans/`, the human's go-ahead, dry run first, written rollback.
**Unattended:** never merge, never touch anything live or any secret; stop and report when stuck.

If the shipshape skills are installed, use them (`shipshape`, `shipshape-memory`, `shipshape-unattended`).
Project-specific rules below and in the docs take precedence over these defaults.
<!-- shipshape:end -->

# SonicScript

Cross-platform dictation that turns Urdu / Hindi / English (mixed) speech into clean Roman Urdu / Hinglish text at
the cursor. Plan: `blueprint.md`. Design system: `DESIGN.md` + `design/tokens.css`.

## Project rules

- **Shared spec first.** Prompts (`spec/prompts/`), model ids and prices (`spec/providers.json`) and test sentences
  (`spec/fixtures/sentences.json`) are shared by every platform. Change them there, never hard-code copies.
- **Model ids live in `spec/providers.json`**, never in code. Before switching the default engine, re-run the
  benchmark (`eval/`) and record the result in `docs/DECISIONS.md`.
- **Keys:** read from the environment (`GROQ_API_KEY`, `GEMINI_API_KEY`), the Windows user environment, or
  `eval/.env` (git-ignored). Never print, log, commit or screenshot a key.
- **Voice recordings are personal data:** `spec/fixtures/audio/` is git-ignored until the owner decides otherwise
  (D-004).
- **UI work follows `DESIGN.md`** (tokens, the Voice Ring, motion and accessibility rules). Use the design-craft
  skill for any new surface.
- Windows is the first desktop target, Android second (`blueprint.md` §8). Don't start Linux or iOS work early.
