# Project Status

> Current truth only: what works now, what doesn't, what's next. History goes in CHANGELOG.md, reasons in
> DECISIONS.md. Updated in the same commit as every code change (the commit-msg hook checks). Keep it short:
> it is loaded into every agent session.

**Last updated:** 2026-11-06 (M2 phase 1 in PR #36; phase 2, the tray app, on `feat/m2-tray`, D-047) · **Code at:** `main` = PRs
#3-#34 (#32 D-041 formatting off by default, #33 D-042 cut-answer guard, #34 M1 done; merged 2026-11-05).
**M1 is done (D-043, the owner's call on 2026-11-05):** every step is built and used live; the owner's own checks
(Next) continue alongside M2. Next big step: M2, plan `docs/plans/2026-11-06-m2-windows-polish.md` (Next 1).
**In one line:** M1 (Windows dictation app in Rust): key, mic and pipeline work live; the default is an
**engine chain** (D-029, D-035): Gemini 3.5 Transcribe + Roman cleanup → Gemini 3.5 Flash → Flash-Lite, Gemini
only (when all three fail nothing is typed and the console says why),
skipping a model whose free requests are used up (run `m1-chain`: 94.9%, Transcribe's own answers 98.5%, 0 errors,
p95 4.3 s). **Paste at the cursor (step 7)** works live in Windows Terminal, the Claude app and Chrome.
**Dictation history (D-032)** keeps every recording (Opus, ~2 KB/s) and its text on this PC. **Smart formatting (D-030, D-031)** is
merged (PR #12; off by default since D-041, `--format` turns it on; Layout 96%, no word loss; live list, take-back
and lakh amount right); since D-038 (PR #28) the format step gets the transcript in tags and its answer is
checked, after a live paste of the whole format prompt.

**Start here (new session):**
1. `git switch main && git pull`; run `sh .shipshape/context.sh`; read this file, then `DECISIONS.md` D-011 to
   D-038 (D-025 to D-038 are the newest and set the direction), `ARCHITECTURE.md` and the plan
   `docs/plans/2026-11-02-m1-windows-push-to-talk.md` (Progress at the top). Formatting details:
   `docs/plans/2026-11-03-m1-smart-formatting.md`.
2. Do "Next" below in order, one branch and PR per item (`feat/m1-<topic>`), each with its check.
3. **Work in this checkout, not a git worktree:** drive C: is nearly full (13 GB free on 2026-11-04, 3.4 GB on
   2026-11-03, when a worktree needing ~5.4 GB failed; 11 GB on the evening of 2026-11-05). `desktop/target` alone is 3.2 GB. Ask the owner before
   deleting anything.
4. Keys: `GROQ_API_KEY` and `GEMINI_API_KEY` are in the owner's Windows user environment (the app also reads Windows Credential Manager, `sonicscript --key`, D-046); `OPENAI_API_KEY` only
   in `eval/.env` (paid: benchmark only, spend sparingly). Never print, log or commit a key. CMake is on the
   user PATH.
5. Gemini free limits per model, per day (owner's key): Transcribe 25, Flash 20, Flash-Lite ~500; they reset at
   midnight Pacific (about 1 PM in Pakistan). Every app or bench run that uses the chain spends them: plan
   benchmarks so two runs don't need the same day's quota.
6. Long runs (bench, tests) belong in your own shell with a log file: the desktop app's hidden Terminal pane
   throttles console output. Keys and mic need the owner's unlocked PC.

## What works (verified)

- **Reliability (M2 phase 6, D-051, branch `feat/m2-reliability`):** each engine's wait is learned from its
  last 50 answer times (`answer-times.json`: twice the median, never sooner than the spec's wait, at most 8 s;
  `--bench` keeps the fixed waits); clips over 2 minutes go in parts cut at pauses and come back joined; after a
  failure a retry could fix, the HUD's "Try again" button pastes the kept recording's text, the tray's item
  copies it; history older than the kept days (Settings → History, default 30) is deleted. Checked 2026-11-06:
  unit tests; a 164 s clip of 31 test sentences through `sonicscript --file` and the real chain came back whole
  (first to last sentence, two parts, Flash); `--hud-demo` with the Try again state: focus moved 0 times. Not
  checked: Try again with the network really off (the owner), the HUD button's look at full size. A
  `timings.json` from the first (dropped) version may be in `%LOCALAPPDATA%\SonicScript` (3 numbers): unused.

- **Pop-up mic (M2 phase 5, D-016, D-050, branch `feat/m2-popup-mic`):** when a text field gets the focus, a
  small Voice Ring shows just below its right end; a click starts a recording (as a tap of the key) and another
  click, the key or Enter stops it. Never on password fields, in terminals, or in apps turned off with its x
  (Settings → General switches it off everywhere and lists the apps turned off, with Turn on). Checked
  2026-11-06, before and after the review fixes, on a test window of its own
  (`--popup-preview`, scratch settings): shown under the text box, hidden on the password box, a click left the
  focus in the text box, hidden when the window closed. Not checked: Chrome, VS Code, the Claude app, WhatsApp,
  a dictation started by a click (the owner).

- **Settings window (M2 phase 4, D-049, branch `feat/m2-settings-ui`):** tray menu → Settings…: General
  (dictation key: Change, then press it; formatting, sounds, start with Windows, pop-up mic switches), Models &
  keys (Gemini / Groq / OpenAI keys: save, test, remove, "Get a key"; main model + 2 fallbacks from the
  catalog), Words (copy the assistant prompt, add words from the clipboard with a preview, edit the list),
  History (keep on / off, the last 50 with copy / insert / play / delete; "delete after N days" is saved but
  nothing deletes yet: Phase 6), About (privacy).
  Every change saves at once and dictation reloads it between recordings (no restart). Checked 2026-11-06: unit
  tests; the real app (`--settings-preview` with a scratch `APPDATA` / `LOCALAPPDATA`) driven over DevTools,
  14 of 14: each control wrote `config.toml` / `words.txt` / the history, Space refused, F9 taken, Test key
  "works" with the real Gemini key and "refused" with a wrong one; screenshots dark / light / 400 px.
  Review fixes: a failed restart (e.g. the Gemini key removed) stops the hook so it takes no keys; closing the
  window while picking a key ends the pause; one lock order in the hook. Not checked: a dictation in the running
  tray app after a change (the owner), Play opening the player, keyboard-only by a person.

- **HUD Voice Ring (M2 phase 3, D-048, branch `feat/m2-hud`):** the tray app shows a pill at the bottom of the
  monitor you work on: Listening (ring follows the mic, timer), Writing… (one shimmer), a Done tick (600 ms),
  "Copied: press Ctrl+V" (2.5 s), problems (dot + one sentence: no speech, not pasted, nothing typed, a model
  out of free requests). Never the dictated text; never takes focus. Checked 2026-11-06: unit tests; every state
  rendered in a browser (screenshots); `sonicscript-tray --hud-demo` ran every state on the real window with the
  foreground window unchanged (0 moves). Not checked: a real dictation through the HUD, two monitors, the
  ring with a real mic level (the owner).

- **Tray app (M2 phase 2, D-047, branch `feat/m2-tray`):** `cd desktop && cargo run -p sonicscript-app`
  (debug: with a console showing M1's lines) or `target\release\sonicscript-tray.exe` (no console). Runs
  dictation with the user's settings; tray icon = the Voice Ring per state; tooltip / first menu line say the
  state or the last problem, never the text. Menu: Pause dictation (keys pass untouched; a running recording
  is cancelled), Settings… (placeholder page until Phase 4), Open history folder, Start with Windows (HKCU
  `Run`, off until ticked), Quit. One dictation per user: the CLI's dictation while it runs is refused; a
  second tray start opens the first one's Settings. Checked 2026-11-06 on the owner's PC: icon, menu, pause /
  resume, settings window (closing it keeps the app), history folder, quit, second start (opens the first one's Settings), CLI refused; a tray started while the CLI dictates waits for it to quit.
  Not checked: a dictation by the real key through the tray (programs' keys are ignored by the hook).

- **Engine chain (D-029, PR #10):** `sonicscript` without `--engine` sends the clip to
  `gemini-3.5-transcribe+gptoss20b`; after 4 s or an error also to `gemini-3.5-flash`; 3 s later
  `gemini-3.5-flash-lite`; first good answer wins. No Groq Whisper in the chain (D-035, owner 2026-11-04:
  its answers were misspelt live, 7 of 8): when every Gemini model fails, nothing is typed and the console
  prints "Nothing typed: no model answered", each model's reason, and what to do (free requests back at about
  1 PM, or Google slow: try again in a minute); `--engine groq-whisper-hint` still runs it alone. Checked:
  unit tests and a run with a wrong Gemini key (all three refused, each listed). A 429 naming Google's daily
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
    With our Whisper hint as its prompt, `gpt-4o-transcribe` often writes the hint itself instead of the
    speech: 8 of the owner's 15 live clips of 2026-11-05 came back as "Yaar, PR merge kar do aur deployment
    check kar lo..." (the hint's first sentence; the 2026-11-04 "only the first sentence" finding was this
    echo). `gpt-4o-mini-transcribe` never did: on those 15 clips it lost no sentence, misheard a few words
    ("speed at GTR", "pro" for "bro"), and kept the 2 clips Transcribe cut (Known gap 15). Not in the chain;
    catalog only. Both write text on silence. About $0.10 spent in all.
  - `groq-whisper-hint` (out of the chain since D-035; `--engine` only): 89.2%; in the app 89.3%, 0.3 s / 0.5 s.
  - `gemini-3.8-flash` and `gemini-3.1-flash-lite`: dropped (timeouts / 503 "high demand" on the free tier).
  - `sonicscript --models` prints the catalog (`spec/models.json`): free / paid, limits, results, the default
    (★) and fallbacks 1-3.
- **Dictation (M1 step 5, D-015, D-023, D-024):** tap / hold Right Ctrl, Enter stops, Esc cancels; cpal mic,
  own 16 kHz resampler, Opus encoded while recording. Live test by the owner (tap, hold, Realtek mic) and key
  test (Enter / Esc taken only while recording; Right Ctrl + C still copies) passed 2026-11-03.
- **Paste at the cursor (M1 step 7, PR #16; live 2026-11-04: pasted in Windows Terminal,
  the Claude app and Chrome, ~300 ms each, 12 pastes in the history; the owner has not reported Win+V, the
  image and the administrator window yet):** `inject::paste` saves the
  clipboard (every memory format: text, HTML, images as DIB, files), sets the text marked so Win+V history and
  cloud clipboard skip it, sends Ctrl+V (Ctrl+Shift+V in Windows Terminal / console windows) and puts the
  clipboard back after 300 ms unless something new was copied. Window running as administrator: text only
  copied, the console says to press Ctrl+V. Waits up to 1 s for Shift / Alt / Ctrl / Win to be let go. After
  every paste SonicScript clears its own console's input (a paste into its own window would otherwise run as
  commands in PowerShell after it quits); old console windows get line breaks as spaces. `--record` never
  pastes. The key hook ignores injected keys. Checked: unit tests (keys per window class, line breaks, saved
  formats) and a manual round-trip on the real clipboard (`cargo test -p sonicscript inject -- --ignored`).
- **Cut-answer guard (D-042, PR #33):** when the main model's answer has under 4 words a
  second of speech (3+ s of speech; `Encoded.speech_ms` from the voice gate), the next model is asked too and
  the fuller answer is pasted. Checked: unit tests; both live cut clips of 2026-11-05 (17:08, 17:11) through
  the real pipeline with the saved cut answer as the main model and the real Flash-Lite as the fallback: both
  came back whole; all 36 other history clips with 3+ s of speech run at 5.1+ words a second (no extra
  request).
- **Formatting off by default (D-041, PR #32):** the owner's choice after his `--no-format`
  try: dictation, `--record`, `--file` and `--bench` give the exact words (Transcribe still gets its Roman
  step); `--format` turns smart formatting on; `--no-format` is still accepted. Checked: a real clip without
  the flag (no format step, 1.4 s) and with it (format step 0.9 s).
- **Chain speed when the main model is out (D-040, PR #31):** a model out of free
  requests rests until Google's reset (midnight in Los Angeles + 5 min), remembered across restarts in
  `%LOCALAPPDATA%\SonicScript\resting.json`; while the main model rests or has failed, Flash and Flash-Lite
  are asked together. Gemini's running single time stamps ("0:02 ... 0:08 ...") are removed too. Checked:
  unit tests and the real chain on a 3.8 s clip: 1.8 s with Transcribe resting (6.0 s when it was asked).
  Evening of 2026-11-05 before it: 2.3-11 s after Transcribe ran out at 18:13 (2.1-3.7 s before).
- **Script answers (D-039, PR #30):** an answer mostly in Urdu / Devanagari from
  Flash / Flash-Lite / Groq that no format step made Roman goes through the spec's `romanize` step (Groq 20b)
  or counts as failed (the chain moves on); Gemini's subtitle time-line ("0:00 - 0:05 ...") is removed. Fixes
  the live 19:48 paste of "baree traansakript 0:00 - 0:05mein". Checked: unit tests and the saved 19:48 answer
  through the real Groq step (884 ms, all Roman, sentences start lower-case).
- **Fourth live try (2026-11-05, 18:19-19:50, 10 dictations, last 6 with `--no-format`):** Transcribe out of
  free requests all evening; Flash-Lite answered 6, Flash 4, key-up → text 2.6-8.3 s. Compared with OpenAI
  `gpt-4o-mini-transcribe` (~$0.012, `eval/results/live-openai-2026-11-05/compare-mini-since-18-19.jsonl`):
  9 of 10 as good as or better than OpenAI ("panga" vs "ping", "persona" vs "purpose"); both missed "Hermes"
  at 18:29 ("her message in ka"); 19:48 was the D-039 bug. The owner said while dictating (19:46): the
  formatter should be off by default with a setting to turn it on (needs a decision superseding D-031).
- **Text step guard (D-038, PR #28):** the format / refine step gets the
  transcript in `<transcript>` tags and its answer is checked (empty, its own prompt, far longer / shorter)
  before it is pasted: asked once more, then "Not formatted" (after Transcribe: the next engine). Fixes a live
  paste of the whole format prompt (2026-11-05 17:38: a spoken "dobara bhej" was obeyed). Checked: unit tests,
  that text 8 of 8 right wrapped (1 of 8 bare), the app's real path twice, D-031's checks re-run (no word
  loss; layout 92% / 88%, the drop all on the recipe clip `fmt-04`, which varied before).
- **Dictation history (D-032, PR #17, also plan step 9's latency log):** dictation and
  `--record` keep the whole recording (key-down to key-up, Ogg/Opus 16 kbps: ~2 KB a second, ~7 MB per hour
  of speech) in `%LOCALAPPDATA%\SonicScript\history\audio\<date>\<time>.ogg` and one line in
  `history.jsonl`: time, recording, recorded / speech / sent length, engines (main, answered, passed), raw
  engine text, final text, formatted or not, app in front (exe, window class), paste result, timings (mic open,
  key-up → text, paste, every stage), tokens, error. Written after the paste (no extra wait); `--no-history`
  keeps nothing. Checked: unit test (files and lines) and a real 2 s `--record` (3.5 KB file, 14 kbps,
  plays back at 2.0 s). Live 2026-11-04: 16 dictations kept (392 KB); it showed where "Nain" came from (Groq
  Whisper, Transcribe out of free requests, Flash / Flash-Lite slow) and that a 62 s clip held no speech.
- **Second live try (2026-11-04, 22:17-23:11, 8 dictations in the history, `main`):** chimes heard, paste
  into the Claude app worked every time. Transcribe and Flash were out of free requests (the day's benchmarks
  and tests had used them); Flash-Lite answered 1 of 6 (3.0 s) and timed out (10 s) on the other 5, so Groq
  Whisper answered 5 clips after 10-12.6 s with misspelt text ("Bhuaii, sach bethau na ... grog" for "Bhai, sach
  bataun na ... Groq"; OpenAI's model got that one right). The owner: Groq's answers are no use, better none
  (D-035: Groq is now out of the chain). Flash's one answer (14 s clip) was clean, "1 baje" / "4 baje" right.
- **My words (D-034, PR #20, phase A of `docs/plans/2026-11-04-word-list.md`):** the user's
  list is `%APPDATA%\SonicScript\words.txt` (created with a format guide at the top: `Word` or
  `Word = how it gets misheard`); commands: `--words` (Notepad), `--words-prompt` (copies the prompt for the
  user's own AI assistant, `spec/prompts/words-assistant-v1.md`, with a collect mode), `--words-add <file>`,
  `--words-paste` (clipboard), `--words-suggest` (names from the history); each shows what it found ("8 found, 8
  new", skipped sentences) and asks first. The forgiving parser (`core/src/words.rs`) takes our format, bullets,
  numbers, commas, bold, code fences, "Word: note", "Word → misheard". Used by dictation, `--record`, `--file`
  (not `--bench`): the local cleanup (mishearings become exact replacements), the Gemini audio prompt (first 300
  words), the format step (whole list up to 40, else only words close to the transcript); never Whisper's
  prompt (D-014). Checked: unit tests (parser, merge, near words, the words in both requests, cleanup), real
  runs of every command on a scratch folder, clip #12 with and without a list on Flash (right both times: the
  live "McCloud" was a one-off). Not checked yet: a before / after where an engine gets a name wrong.
- **Mic ready on key-down (D-036, PR #25):** dictation keeps one mic stream set up but not
  started (no mic icon while idle) and only starts it on key-down; the console says "mic ready in N ms".
  Checked: a manual test (`cargo test -p sonicscript ready_mic -- --ignored`): after 25 s idle it started in
  2 ms (was 113-512 ms) with a full second of audio; `examples/mic_open.rs` measured the cause (stream build
  ~130 ms, device wake after ~20 s idle ~330 ms) and 0 ms with a prepared stream after 2 minutes idle. Live
  2026-11-05: `ms.mic_open` 14-16 ms on all 18 of the owner's dictations.
- **Third live try (2026-11-05, 17:07-17:38, 18 dictations, `main` before PR #28):** Transcribe answered every
  one, key-up → text 2.1-3.6 s, pasted in Windows Terminal, the Claude app and Chrome. Problems found: 2 long
  mixed clips cut to a few English keywords by Transcribe (Known gap 15) and one paste of the whole format
  prompt (fixed, D-038). A second opinion on the 15 clips up to 17:20 from OpenAI (~$0.04): `gpt-4o-mini-transcribe` + format lost no
  sentence but misheard a few words; `gpt-4o-transcribe` wrote our Whisper hint instead of the speech on 8 (see
  Engines). The re-usable script and the side-by-side texts: `eval/results/live-openai-2026-11-05/`
  (`openai_compare.py <date-prefix> <out.jsonl>`, run from `eval/`; local, git-ignored).
- **Changeable dictation key (M1 step 6, D-018, D-037, PR #26):** `sonicscript --set-key`
  (press the key or a mouse side button), `--set-key <name>`, `--set-key list`; saved as `hotkey = "..."` in
  `%APPDATA%\SonicScript\config.toml`. Ctrl / Shift pass on to the app, Alt / Win pass on with a masking key
  (no menu on a lone tap), F-keys, Insert, Caps Lock, Pause, Scroll Lock and side buttons are kept from the
  app. Checked: unit tests and real runs on a scratch config (refusals, warnings, saving, dictation starting
  with F9 and Mouse4, a bad value falling back to Right Ctrl). Not yet checked: real presses (the owner).
- **Start / stop chimes (PR #19, until M2's on-screen ring):** dictation plays a soft rising
  chime when recording starts (before the mic opens, ~110 ms), a falling one after the mic closes, one low note
  on cancel or a mic failure (`cue.rs`: sine notes built in memory, `PlaySoundW` without waiting); `--no-sound`
  turns them off. Checked: unit test (format, level, no clicks) and a manual test that played all three on the
  owner's PC. Not yet checked: whether the start chime reaches the recording when the mic opens fast (~125 ms)
  and speakers (not headphones) are used: look at the first 200 ms of the next history recordings.
- **Pipeline pieces (M1 steps 1-4):** text cleanup (D-019: script guard, phantom filter, word list), voice gate
  (D-020, D-022: sends all 42 speech clips, drops both silence clips, keeps 500 ms around speech), Ogg/Opus
  16 kbps (D-021: as accurate as WAV at 8 KB per clip), golden check `--bench` scored by `eval rescore`.
- **Smart formatting (D-030, PR #12, plan `docs/plans/2026-11-03-m1-smart-formatting.md`):**
  `spec/providers.json` `format` (Groq `gpt-oss-120b` + `spec/prompts/format-v1.md`) runs after whichever
  engine answers, in place of its own refine step; optional after Flash / Flash-Lite / Groq (a failure keeps
  the engine's text and says "Not formatted"), required after Transcribe. Off by default since D-041 (exact words); `--format` turns it on. The
  cleanup keeps line breaks (lists). Checked 2026-11-04 (D-031), final prompt:
  - App (`sonicscript --bench` on the owner's 13 `fmt` clips, run `m1-format-app`): Layout 96%, 0 errors,
    clip → text p50 2.6 s / p95 3.8 s (format step p50 ~0.7 s); Transcribe answered all 13.
  - Layout after Flash-Lite and after Groq Whisper (run `fmt-lite-v3`): 96% each. Misses: the filler "wo na"
    kept (fmt-11), Whisper's garbled fmt-13.
  - Word loss (saved transcripts of the 44 M0 clips, runs `fmt-120b`, `fmt-120b-final`): accuracy and English
    kept unchanged on the 40 clips that are not spoken lists (Transcribe 97.2 → 97.1%, Flash 97.4 → 97.4%,
    Flash-Lite 96.9 → 96.9%); `ag-03` and `lg-02` (spoken lists) became lists.
  - 20b lost words on fmt-11 ("Kya kal chutti hai?"); 120b kept them.
  - Clock times (PR #15): Gemini Transcribe writes "6 baje" as "6:00"; the prompt now
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
- **Tests (2026-11-06, `feat/m2-reliability`):** `eval`: 42 offline tests, ruff clean; `desktop`: 103 core + 56 `win` + 32 tray-app tests (+1 manual clipboard test, +1 manual sound test, +1 manual mic test: `cargo test -p sonicscript -- --ignored`)
  tests, fmt and clippy `-D warnings` clean (with the owner's recordings present: the voice gate test reads
  the 44 M0 clips only, not `fmt-*`).
- Code knowledge graph (D-010): works, all 39 code files (checked 2026-11-04); CLAUDE.md says which lookup
  fits (PR #14).

## In progress

- M1 done (D-043, 2026-11-05): all plan steps built and used live, plus the engine chain (D-029, D-035,
  D-040), formatting (D-031, off by default since D-041), the history (D-032), My words phase A (D-034), the
  ready mic (D-036), the changeable key (D-037), the text-step guard (D-038), script answers (D-039) and the
  cut-answer guard (D-042). Not done as the plan wrote it: the 3 days of daily use and the owner's checks
  (Next 2-4, 7), now alongside M2. Pop-up mic on text fields, settings page (main model + 2 fallbacks,
  formatting switch) are M2 (D-016, D-017).
- Punctuation: the owner finds the formatted text over-punctuated and wants about as much as the Gemini app's
  and Claude's dictation. Measured 2026-11-04 (runs `fmt-120b`, `m1-format-app`): after Transcribe the format
  step keeps Gemini's sentence ends (10.5 → 9.6 per 100 words on the M0 clips) and drops about half its commas
  (3.4 → 1.9; owner's clips 8.8 → 1.6), so no prompt change yet: waiting for the owner's live examples (Next 7).

## Live state

- Nothing is live.

## Known gaps

1. Every engine above 95% takes 1.5-3 s typically, which D-025 accepts. With 2 s waits, Groq answered 13 of
   the 18 clips after Transcribe ran out (88%) in `m1-chain`; the owner chose 3 s each for Flash and
   Flash-Lite. Since D-035 the chain is Gemini only: on a slow Google day the wait is whatever the fastest
   Gemini model takes, and when all three fail (worst case ~17 s: Flash-Lite asked at 7 s, timing out at 10 s)
   nothing is typed; the recording stays in the history. Not benchmarked yet; D-017's learned deadline later.
2. Every Gemini model has a slow tail (some requests 10-30 s) and they slow down together; nothing caps the
   wait but the 10 s timeout (D-035). A request that loses keeps running in the background (at most 10 s) and still uses its model's daily
   quota. Bench cost counts only the answer used. The "free requests used up" notice was first seen live
   2026-11-04; the owner has not said yet whether it reads well (Next 7).
3. Smart formatting is limited by Groq's free tokens per minute: each format call is ~1,300 tokens and
   `gpt-oss-120b` allows 8K tokens/min (Groq's `x-ratelimit-limit-tokens` header): about 6 formatted
   dictations a minute; past that the text comes out unformatted (after Transcribe, Flash answers instead).
   Groq's table also lists 200K tokens/day, but ~350K were used on 2026-11-04 with no daily 429, so the daily
   limit is not known. Options if it bites: a shorter prompt, 20b as a second formatter (its own quota), or
   Groq's paid tier. Benchmarks spend the same quota.
4. Names fail on Groq Whisper; the word list fixes sound-alikes ("Jaminai" → Gemini, "Codec" → Codex) but not
   words run together ("Algrock" = "aur Groq") or 2-consonant words ("rhyme" for RAM). Since D-034 the user's
   own list is loaded (`words.txt`), and "Word = mishearing" fixes run-together ones; an empty list (the
   default) changes nothing. Whether the list makes Gemini spell names right more often is not measured yet.
5. Groq Whisper sometimes translates instead of transcribing (`mx-04`, `nm-06`); worse with too little silence
   around the speech (D-022). Only matters with `--engine groq-whisper-hint` (out of the chain, D-035).
6. Dictating exactly the Whisper hint's last sentence ("Theek hai, main abhi meeting mein hoon.") is dropped as
   a phantom (D-019).
7. The voice gate is tuned on one male voice (the owner) and one room; loud talking or TV counts as speech.
   Silero VAD only if this fails in daily use.
8. Mic-open delay: fixed by D-036 (a prepared stream starts in ~2 ms); was 113-512 ms of lost speech. Still
   slow (~130-500 ms) right after start-up (the first stream is still being prepared) and when the ready
   stream broke (mic changed, PC resumed from sleep): then the mic opens the old way. The start chime now
   plays while the mic is already open: with speakers it may be faintly in the recording (check the first
   200 ms of a few history recordings).
9. Urdu-script "میں" is always romanized "mein" (it can be "main" = I) by the local cleanup. With formatting on
   (D-031) the format step romanizes Urdu script first, so this matters only without `--format` (the default, D-041) or when the
   format step fails (then mostly-Urdu output from Groq gets only a rough letter-by-letter draft, `needs_llm`).
10. Free-tier Gemini requests may be used by Google to improve its products (Gemini API terms); the catalog
    says so. Prices for `gpt-oss-120b` and Gemini 3.x audio are from 2026-10-26; recheck before launch.
11. Building `desktop/` needs CMake on PATH (for libopus); on another PC: the VS Build Tools CMake folder or
    `winget install Kitware.CMake`. Drive C: is nearly full (see Start here 3).
12. Final name decided after M2 (Next 7); "SonicScript" until then (license is MIT, D-012). `main` is guarded by the pre-push hook only.
13. Smart formatting (D-031): the format step (~0.7 s typical, ~1.3 s slow) counts inside the chain's waits,
    so a later engine is asked a little more often; not measured on a full chain run yet (Next 6). The
    prompt still sometimes keeps the filler "wo na" (fmt-11) and sometimes merges the last two steps of the
    tea recipe (fmt-04: old and new prompt alike), and Whisper's garbled text cannot be formatted. "Hmm" is
    kept on purpose (owner). Prompt
    changes need a re-run of the checks (D-031's runs). Engine ids and labels still say "+gptoss20b" /
    "Roman cleanup" (the path without `--format`, the default since D-041); the catalog notes say the 120b format step replaces it.
14. Changeable key (D-037): real key presses not tried yet (the hook ignores keys sent by programs, so no
    agent can test them; Next 2). `--set-key` with no name catches every key until one is chosen (Esc keeps
    the old key).
15. Gemini Transcribe sometimes cuts a long mixed Hindi / English clip to a few English keywords (2 of the
    owner's 15 live clips on 2026-11-05). Since D-042 such an answer (under 4 words a second of speech) gets a
    second opinion from the next model and the fuller one is pasted; not yet seen live with the real
    Transcribe (it was out of free requests when the guard was built).

## Next

M1 is done (D-043); the order below is from 2026-11-05 night. One branch and PR per item.

1. **M2** (plan `docs/plans/2026-11-06-m2-windows-polish.md`, written 2026-11-06): 7 phases, one PR each
   (settings + keys + events, tray, HUD Voice Ring, settings window, pop-up mic, reliability, release). Go-ahead
   and the 5 decisions given 2026-11-06 (D-044). Phase 1 done on `feat/m2-settings` (PR #36, base PR #35): `config.toml` settings (D-045), the main model + fallbacks from them, keys in Credential Manager (`--key`, D-046), dictation events (`events.rs`); review fixes in. Not checked live yet: a paste through the new event path (the owner's next dictation). Phase 2 (tray) built on `feat/m2-tray` (PR based on `feat/m2-settings`): the tray app `sonicscript-tray.exe` (D-047). Not checked yet: a dictation by the real key through the tray, Start with Windows after a real sign-in (the owner). Phase 3 (HUD Voice Ring, D-048) built on `feat/m2-hud`; not checked yet: a real dictation with the HUD (the owner). Phase 4 (settings window, D-049) built on `feat/m2-settings-ui` (PR based on `feat/m2-hud`); not checked yet: a dictation after a settings change in the running tray app (the owner). Phase 5 (pop-up mic, D-050) built on `feat/m2-popup-mic` (PR based on `feat/m2-settings-ui`); not checked yet: the pill in the owner's apps (Chrome, VS Code, Claude, WhatsApp) and a dictation started by a click. Phase 6 (reliability, D-051) built on `feat/m2-reliability` (PR based on `feat/m2-popup-mic`); not checked yet: Try again with the network off and on, history clean-up after N days in daily use (the owner). Next: Phase 7 (release and first run), waiting for the owner. C: had 26 GB free after the Tauri builds (2026-11-06).
2. **Owner: the key (PR #26):** `--set-key`, press F9: tap and hold both work, F9 no longer reaches the app;
   a mouse side button (no Back in Chrome) and Right Alt (no menu bar on a lone tap); Space is refused;
   `--set-key RightCtrl` goes back.
3. **Owner: fill My words** (`--words-prompt` → Claude → copy its answer → `--words-paste`, restart): Claude
   (heard "cloud", "clod", "claud"), Taskvane, Relay, Groq, Gemini, "Bro", the people he talks to (live 17:35
   "Kenzie Joice / Kenjhar Joys": ask him who it is).
4. **Owner + agent: daily use review** (was M1's "done when", waived by D-043). At the end of each day the agent reads
   `history.jsonl`, compares with a second opinion (the script above; ~$0.01 per minute of audio) and lists
   and fixes the mistakes.
5. **Agent, after Google's daily reset (~1 PM Pakistan):** a My words before / after on names the engines got
   wrong; the full chain benchmark `m1-chain3` (the app benchmark command under "How to run", on the 44 M0
   clips without `fmt-*`); update known gaps 1, 2 and 13. Transcribe 25 and Flash 20 a day go fast: never
   run two benchmarks that need the same day's quota, and leave the owner some.
6. **Optional: one-step formatting** (owner's question 2026-11-05: can the transcript come back formatted?).
   Whisper cannot (its prompt is only context; a longer one made it translate, D-014; `gpt-4o-transcribe`
   even returned the hint). Gemini Transcribe takes no prompt at all and writes Urdu / Devanagari script, so
   it always needs the second step. Gemini Flash / Flash-Lite do take a prompt (`transcribe-roman-v1.md`):
   the format rules could go there, one request instead of two (saves the format step's ~0.6-1.0 s). A
   benchmark brief exists as a proposed session ("Benchmark one-step formatting in the Gemini audio prompt":
   Flash-Lite only, new prompt `transcribe-format-v1.md`, an engine outside the chain, compared with runs
   `fmt-wrap-m0` / `fmt-wrap-layout2`). Cost: the text would look different depending on which engine
   answered, the reason D-031 has one format step after every engine.
7. **Owner, later:** the paste checks (Win+V shows no dictated text; a copied image is back after a
   dictation; an administrator window gets "press Ctrl+V"); punctuation examples (2-3 over-punctuated outputs,
   best with the same words said into Claude's dictation; then the agent tunes the prompt and re-runs D-031's
   checks); the chimes are fine (describe changes and the agent tunes `cue.rs`); the name stays "SonicScript"
   until after M2 (English, human-sounding, nothing AI-ish; "Utter" is taken; to check then: Mynah, Blurt,
   Ramble, Natter, Spill, Hum); whether the "free requests used up" notice reads well.

## How to run and check

- Setup: `cd eval && pip install -e ".[dev]"` (needs ffmpeg on PATH). Keys: `GROQ_API_KEY`, `GEMINI_API_KEY`
  in the environment, the Windows user environment, or `eval/.env`.
- Benchmark: `python -m sonic_eval record | probe | bench --preset compression|engines | report | rescore`.
- Dictate: `cd desktop && cargo run -q --release -p sonicscript` (tap / hold Right Ctrl; the text is printed).
  Mic check: `... -p sonicscript -- --record 5`.
- Engines: without `--engine` the chain runs (D-029); `--engine <id>` runs one engine alone (ids:
  `... -p sonicscript -- --models`). The app reads keys from the environment / Windows user environment only,
  not `eval/.env`. The format step runs after any engine only with `--format` (D-041); by default the exact words.
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
- Tests: `cd eval && python -m ruff check . && python -m pytest -q` (42 tests) and
  `cd desktop && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.
- Code map: `graphify query "<question>"`; after code changes `graphify update .` (see CLAUDE.md).
