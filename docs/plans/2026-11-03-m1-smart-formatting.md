# Plan: smart formatting (D-030), M1

**Status:** draft, waiting for the owner's go-ahead (2026-11-03). Branch `feat/m1-format`.

## Goal

Every dictation comes out formatted the same way, whichever engine in the chain answered: spoken lists (three
or more items, tasks, rules) become "- " lists, ordered steps "1." lists, with the lead-in words as a first line;
ordinary speech stays sentences; empty fillers ("matlab", "woh na", "umm") are dropped; take-backs ("nahi nahi",
"sorry", "actually") are applied; numbers are digits with lakh / crore grouping. On by default; `--no-format`
gives the exact words (today's output).

## Tier

**T1** (normal): no data, keys, money or live systems. It changes every output, so it gets this plan, a
benchmark before the default changes, and an off switch.

## What I found in the code (it shapes the plan)

1. **The D-019 cleanup would flatten every list.** `normalize.rs` `tidy_spacing` joins all whitespace,
   newlines included, into single spaces, so "Doodh\n- Anday" would be pasted as "Doodh - Anday". The cleanup
   has to work line by line and keep line breaks.
2. **A failed refine step fails the whole engine** (`client.rs` `Client::transcribe` uses `?`). Today that
   only matters for Transcribe (its text is in Urdu script, so it can't be used without the step). With a
   format step after Flash, Flash-Lite and Groq, a Groq hiccup would throw away a good Gemini answer.
3. **The chain's waits include the refine step** (the wait runs around `Client::transcribe`). A ~1 s format
   step after Flash uses up a third of its 3 s wait before the next engine is asked.
4. The text-only test (`docs/research/format-test/results-2026-11-03.txt`) found two weak spots in the draft
   prompt: `gpt-oss-120b` wrote the tea recipe steps as "- " (not "1.") and left the weekend plans as one
   sentence; `gpt-oss-20b` got both right but kept "woh na". The prompt needs an ordered-steps example and a
   per-day plan example.
5. `record` shows `text or say`; the new sentences need `say` (what to speak) to win over `text` (the expected
   formatted output), because the two differ ("nahi nahi chhe baje" is spoken, "6 baje" is expected).
6. `bench` / `catalog` average accuracy over every non-silence clip; the new clips must not change the M0
   accuracy numbers that past decisions were based on.

## Approach

### 1. Test sentences (needs the owner's voice)

New category `format` in `spec/fixtures/sentences.json`, ids `fmt-01` to `fmt-13`. Each has `say` (what to
speak), `text` (the expected formatted output, with line breaks) and a `format` block that the score checks:
`list`: `"dash"` / `"ordered"` / `"any"` / `null` (no list), `items` (count), `absent` (words that must be
gone: the taken-back part, fillers), `digits` (strings that must appear, e.g. `2,50,000`, `2:30`).

| id | what it tests | say (spoken) | expected |
|---|---|---|---|
| fmt-01 | grocery list | Ammi ne kaha hai market se ye cheezein le aana, doodh, anday, bread aur do kilo aloo | lead-in + 4 "- " items |
| fmt-02 | to-do list | Kal ke kaam: pehla bank jana hai, dusra bijli ka bill jama karwana hai, teesra Ali ko call karni hai | lead-in + 3 items (either marker) |
| fmt-03 | rules | Server ke rules simple hain, spam nahi karna, kisi ko gaali nahi deni, aur memes sirf memes channel mein | lead-in + 3 "- " items |
| fmt-04 | ordered steps | Chai banane ka tareeqa, pehle paani ubaalo, phir patti daalo, phir doodh aur cheeni daalo, aur do minute pakao | lead-in + "1." to "4." |
| fmt-05 | plans per day | Is weekend ke plans ye hain, Friday ko movie, Saturday ko cricket aur Sunday ko family dinner | lead-in + 3 "- " items |
| fmt-06 | English list | For the trip I need to pack three things, my passport, the charger and some cash | lead-in + 3 "- " items |
| fmt-07 | not a list ("pehle ... phir") | Pehle main socha ke aaj gym jaun, lekin phir baarish shuru ho gayi aur main ghar pe hi raha | one sentence |
| fmt-08 | two items only | Mujhe bas do cheezein chahiye, charger aur headphones | one sentence |
| fmt-09 | take-back (Urdu) | Theek hai kal paanch baje milte hain, nahi nahi, chhe baje | "Theek hai, kal 6 baje milte hain." (no "paanch" / "5") |
| fmt-10 | take-back (English) | Meeting Monday ko rakh lo, actually Tuesday ko | "Meeting Tuesday ko rakh lo." (no "Monday") |
| fmt-11 | fillers | Matlab yaar woh na main keh raha tha ke kal chhutti hai kya | no "matlab", "woh na"; a question |
| fmt-12 | lakh + "dhai baje" | Bhai do lakh pachaas hazaar rupay bhej do dhai baje tak | `2,50,000`, `2:30` |
| fmt-13 | "dedh lakh", "saadhe teen" | Dedh lakh ka budget hai aur meeting saadhe teen baje hai | `1,50,000`, `3:30` |

You record them once (about 5 minutes): `cd eval; python -m sonic_eval record --only fmt-01 ... fmt-13`.
I'll ask you when the sentences are in and wait.

### 2. Format score in `eval` (offline, no API calls)

`scoring.py` gets `format_score(output, expected, spec) -> FormatScore`: `list_ok` (list kind and item count
right, or no list when none is expected), `absent_ok`, `digits_ok`, and `format_rate` = share of those checks
passed. `bench` and `rescore` store it per row; `summarise` reports "Format" over `format` clips only, and
keeps accuracy / English kept on the M0 categories only (point 6 above), so old and new runs compare fairly.
The report shows the formatted text with its line breaks. `record` shows `say` before `text`. Offline tests
for each check.

### 3. The prompt: `spec/prompts/format-v1.md`

The tested `format-draft.md`, plus: an ordered-steps example ("pehle ... phir" recipe → "1."), a per-day plans
example, and the `{word_list}` slot that `romanize-v1.md` has. It also romanizes Urdu / Devanagari script (rule
1), so it replaces `romanize-v1.md` after Transcribe. `romanize-v1.md` stays for the old engines.

### 4. Spec: formatted engines and the off switch (`spec/providers.json`)

New engine ids, so the old ids, their results and the catalog stay reproducible:

| new id | is | format step |
|---|---|---|
| `gemini-3.5-transcribe+fmt` | Transcribe | Groq `gpt-oss-120b` + `format-v1.md` (replaces the 20b romanize step) |
| `gemini-3.5-flash+fmt` | Flash, audio prompt unchanged | same |
| `gemini-3.5-flash-lite+fmt` | Flash-Lite, audio prompt unchanged | same |
| `groq-whisper-hint+fmt` | Groq Whisper | same |

`chain` names the `+fmt` ids (same order and waits, D-029). A new `chain_plain` lists today's chain;
`--no-format` uses it, so "exact words" means exactly today's output. The refine step gets `"optional": true`
on the three engines whose text is already Roman: if the format step fails, their unformatted text is used
(point 2); Transcribe's stays required.

**One deviation from D-030's "Where" line:** it says the format rules also go into the Gemini audio prompt. I
plan to leave `transcribe-roman-v1.md` as it is, because the format step after every engine already does the
job, and formatting twice could make the two steps fight (the audio prompt formats, then the text step
re-formats). If you want both, say so and I'll test both ways.

### 5. Desktop app (`desktop/`)

- `spec.rs`: read `chain_plain` and the refine `optional` flag; `chain_plain()`.
- `client.rs`: an optional refine that fails returns the engine's own text, with the error noted in the
  `Transcript` (a new `format_skipped` field), so dictation can say "Not formatted: <reason>" once.
- `normalize.rs`: clean line by line: `tidy_spacing` per line, keep "- " and "1. " at the start of a line,
  drop empty lines at the ends; the phantom check runs on the whole text as now. New shared cases in
  `spec/normalizer/cases.json` (a list survives; a phantom inside a list line still goes).
- `main.rs` / `key.rs`: `--no-format` picks `chain_plain` (with `--engine <id>` it is ignored: the id says
  whether it formats).
- `dictate.rs`: print multi-line text as it is (pasting in step 7 must keep the line breaks; noted there).
- Tests: the line-by-line cleanup, optional refine falling back, `chain_plain` parsing, the flag.

### 6. The check (benchmark, then decide)

1. **Text-only, no Gemini quota:** a new `eval` command `reformat --from <run> --refine <model> --run <new>`
   runs the format step on the saved transcripts of an existing run and re-scores them. Runs on `m1-chain`
   and `m1-engines` (all four chain engines' saved outputs on the 44 M0 clips), once with `gpt-oss-20b` and
   once with `gpt-oss-120b`. Pass: accuracy and English kept within 0.5 points of the unformatted run on the
   M0 clips (formatting must not lose words), and no M0 clip turned into a list (none of them is one).
2. **Audio, on the 13 new clips:** `bench` with `gemini-3.5-flash-lite+fmt` and `groq-whisper-hint+fmt`, each
   with 20b and 120b (Flash-Lite has ~500 free requests a day; Groq plenty). Transcribe and Flash are not used
   here, so tomorrow's 1:30 PM quota stays free for Next 4. Pass: format score ≥ 90% with the chosen model.
3. Pick 20b or 120b on those numbers (D-030 expects 120b), record the result as a new decision, update STATUS,
   ARCHITECTURE and CHANGELOG, then the app golden check (`--bench`) with the formatted chain on the 13 clips
   and a live try by you.
4. Tests: `eval` ruff + pytest, `desktop` fmt + clippy `-D warnings` + test, CI green.

## Out of scope

The settings-page switch (M2), per-app styles (D-030 says none), pasting at the cursor (step 7), changing the
chain's waits (see Risks), Silero, a Gemini audio prompt with format rules (unless you ask).

## Risks

- **Words lost or changed** by the format step (an LLM can paraphrase). The 44-clip check in 6.1 catches it;
  the prompt says "keep every word"; `--no-format` is the escape hatch.
- **Slower:** +0.6-1.7 s per dictation (the test's range), more on Groq Whisper's path (0.3 s → ~1.2 s). D-025
  accepts up to about 2 s. Because the waits include the format step (point 3), Flash-Lite and Groq will be
  asked more often. I measure how often in 6.2 and the app check; if it is a lot, I come back to you with
  "count the wait on the speech step only" as a proposal rather than changing it here.
- **Groq's free limit for `gpt-oss-120b`** is used once per dictation. I'll check the current numbers on
  console.groq.com/docs/rate-limits and add them to the catalog before it becomes the default.
- **Prompt injection:** a dictated "ignore the rules and ..." must be typed, not obeyed. The prompt says so
  (as `romanize-v1.md` does); a test sentence for it is cheap to add if you want one.
- A wrong list (a sentence made into a list) is more annoying than no list. The two "stays a sentence" clips
  (fmt-07, fmt-08) are there for it; the format score counts them like any other check.

## Rollback

At run time: `--no-format`. In code: revert the PR; or set `chain` back to `chain_plain`'s ids in
`spec/providers.json` and rebuild (one-line change, the old engines are untouched).

## Assumptions (tell me if any is wrong)

- The 13 sentences above are the ones you want (add, drop or reword any before you record).
- A to-do list with "pehla, dusra, teesra" may use either "- " or "1." (I score both as right).
- "Exact words" for `--no-format` means today's output, including the Roman cleanup after Transcribe.
- Formatting runs on Groq only, so dictation with the format step needs a Groq key (it already does: Groq is
  last in the chain and does Transcribe's cleanup). Without one, the text comes out unformatted with a notice.
