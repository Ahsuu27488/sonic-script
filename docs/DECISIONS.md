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

<!-- Template for the next entry:

**D-00N <Decision in one line>.** (YYYY-MM-DD)
<What exactly was decided.>
*Why:* <the problem, the alternatives considered, why this one.> *Where:* <files / functions / settings>.
-->
