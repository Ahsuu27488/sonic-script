# Changelog

> Newest first. Each entry: date, commit (or "uncommitted"), what changed, and for fixes the root cause.
> This is the history; STATUS.md holds only the current truth.

## 2026-11-06 (feat/m2-reliability)

- History older than the kept days is deleted (at start, then at most hourly). The chain learns each engine's
  wait from its last 50 answers (D-017), kept in `timings.json`.
  Clips over 2 minutes go in parts cut at the longest pause near every 90 s, sent in order and joined.
  Try again (tray item, HUD button) sends the last recording nothing answered again (D-051).

## 2026-11-06 (feat/m2-popup-mic)

- Pop-up mic (M2 phase 5) started: a click starts or stops a recording like a key tap (`Machine::click`,
  `hotkey::click`); the rules for which focused field gets the pill and where it goes (`popup.rs`); the UI
  Automation focus watcher and the pill window, wired into the tray app (`--popup-preview` in debug builds). The watcher no longer makes Tauri's setup
  wait (it hung the registration). Checked on a test window: shown, hidden on a password box, focus kept on a click. Review
  fixes: properties read from the event's cache, the app from the focused element's own process, never over
  our settings page, the settings page no longer undoes the pill's x (and lists those apps with Turn on).

## 2026-11-06 (feat/m2-settings-ui)

- The settings window (M2 phase 4, D-049): General, Models & keys, Words, History, About; every change saves
  at once and dictation reloads it without a restart. Test key, words from the clipboard with a preview, the
  last 50 dictations with copy / insert / play / delete. Review fixes: a failed restart stops the key hook,
  closing the window while picking a key ends the pause, one lock order in the hook, Play on Windows paths.
- Settings without a restart: the key hook switches keys while it runs, dictation reloads its settings at the
  next idle moment. Library pieces for the settings window: Test key (`GET /models`, no daily request), where a
  key comes from (last 4 only), history list / delete, word-list preview / add.

## 2026-11-06 (feat/m2-hud)

- HUD Voice Ring (M2 phase 3, D-048): the tray app shows a pill at the bottom of the monitor you work on:
  Listening (ring + timer), Writing…, a Done tick, "Copied: press Ctrl+V", problems. Never takes focus
  (`--hud-demo`: 0 moves), never shows the dictated text. Review fixes: show / hide ordered on the main thread,
  limit notes wait behind Copied, a late answer keeps the next clip's Writing, a dictation stop resets it.

## 2026-11-06 · feat/m2-tray (M2 phase 2, D-047)

- `desktop/win` is a library (`lib.rs`) + the CLI; the CLI is unchanged (`--models`, tests as before).
- The tray app `desktop/app` (Tauri 2, plain HTML, no npm): `sonicscript-tray.exe` runs dictation with the
  user's settings (as the CLI does without flags) and shows its state on a tray icon drawn in code (the Voice
  Ring: Ember ring when ready, filled while recording, a dot while writing, grey when paused, a red dot on a
  problem); the tooltip and the menu's first line say the state ("Ready: press Right Ctrl to dictate", the last
  problem), never the dictated text. Menu: Pause dictation, Settings… (a placeholder page with the settings file
  and commands, styled with `design/tokens.css`; the real page is Phase 4), Open history folder, Start with
  Windows, Quit. A debug build keeps a console with M1's lines; the release build has none.
- Pause (`hotkey::set_paused`): the hook passes every key on and runs no rules; a recording running then is
  cancelled. One dictation per user (`instance::claim`, a named mutex): the CLI's dictation while the tray app
  runs stops with "SonicScript is already running"; a second tray start opens the first one's Settings.
- Checked: tests (tray states and icons, pause, the mutex; each shown red once), clippy and fmt clean; the
  real app: icon in the tray, menu, Pause (grey icon, "Paused") and resume, Settings window (closing it keeps
  the app), history folder, Quit (process gone), second start and the CLI refused. Release build idle: 0 s of CPU in 20 s, 21 MB working set (WebView2 starts only with a window). Start
  with Windows: the real `Run` value written, read and removed (`cargo test -p sonicscript-app -- --ignored`). Not checked: a dictation by
  the real key through the tray (the hook ignores keys sent by programs; the owner's check).

- Review fixes: a slow answer arriving during the next recording no longer shows "Ready" (or a problem)
  on the tray while the mic records (test red first); pause is checked again under the key rules' lock, so a
  key-down racing the menu click cannot start a recording that no key can stop; a tray app started while the
  CLI dictates waits for it to quit ("Another SonicScript is dictating ...") and then starts, instead of staying
  broken (checked live: CLI first, tray waited, CLI killed, tray "Ready"; a second tray start opened Settings);
  STATUS / DECISIONS / ARCHITECTURE / the plan back to LF line endings (a Python edit had written CRLF and a
  stray carriage return in a path).

## 2026-11-06 · feat/m2-settings (M2 phase 1, D-045)

- `config.toml` holds every setting now (`settings.rs`: key, main model + 2 fallbacks, formatting, sounds,
  history and its days, pop-up mic, apps without it), read with `toml` + `serde`; every field has a default,
  so M1's `hotkey = "..."` file loads unchanged. `--set-key` saves through it and keeps the other settings.
  Checked: 5 tests (M1's file, missing file, broken file, round trip, at most 2 fallbacks) and `--set-key
  RightAlt` on a scratch M1-style file (key changed, `format = true` kept).
- The settings choose the engines: `main_model` + `fallbacks` (`key::choose_from`), each waited for as long as
  the spec's chain waits at that place (4 s, then 3 s); none = the spec's chain; an id the spec does not know
  gives the spec's chain and a notice; `--engine` still wins. `format`, `sound`, `history` come from the
  settings too, and `--format` / `--no-format` / `--no-sound` / `--no-history` override them for one run;
  `--bench` ignores the settings (comparable runs). Checked: 5 tests (red with the settings ignored), a real
  clip with `main_model = "gemini-3.5-flash-lite"` (Flash-Lite answered) and `main_model = "gemini-9"` (notice).
- Keys in Windows Credential Manager (D-046, `keystore.rs`): `sonicscript --key gemini|groq|openai` asks for
  the key without showing it and saves it as `SonicScript/<ENV_NAME>`; `--delete` removes it. Keys are read
  from the process environment, then Credential Manager, then the Windows user environment. Checked: read-order
  tests (red when reversed), the real Credential Manager round trip (`-- --ignored`), and a wrong Gemini key
  saved with `--key` and the process variable cleared: HTTP 400 "API key not valid" (the saved key beat the
  user environment); after `--delete` the same clip came back right.
- Dictation tells the user through events (`events.rs`: `Event`, `Sink`, `Console`): the loop and the
  transcription threads send `Ready`, `Listening`, `Processing`, `Text` (with how it was pasted), `NoSpeech`,
  `HintOnly`, `Failed`, `Notice`, ...; the console sink prints M1's lines, the M2 app will drive the tray and
  the HUD with the same events. `dictate::outcome_events` (pure) turns a finished recording into them. The mic
  level for the Voice Ring is `audio::level()` (0-1 from the last chunk's RMS, -60 to 0 dBFS), read by the HUD
  at its own frame rate rather than sent as events. The text is now printed after the paste (M1 printed it just
  before). Checked: 5 tests (red with the paste timing removed); a real `--record` (room noise came back as the
  hint only: `HintOnly` printed) and the dictation start-up lines unchanged. A live paste needs real key presses
  (the owner's next dictation).
- Review fixes (shipshape-reviewer): saving settings never overwrites a `config.toml` that cannot be read
  (`--set-key` on a hand-broken file used to reset every other setting; now "fix or delete it first, nothing
  was changed"); settings this version does not know are reported once and kept in the file; a key saved with
  Windows' own tools (Credential Manager's "Add a generic credential", `cmdkey`) is UTF-16 and is now read as
  such (it read as "A\0I\0z\0a..." and would have beaten a working key); an empty saved secret (null blob) no
  longer reaches `from_raw_parts`; the history keeps the paste time for copied and failed pastes too (as M1).
  Checked: 4 tests (each red without its fix), `real_cmdkey_secret_reads` (`-- --ignored`, a real `cmdkey`
  secret), and `--set-key RightAlt` on a broken scratch file (refused, file unchanged).

## 2026-11-06 · docs/m2-plan (D-044)

- The M2 plan (`docs/plans/2026-11-06-m2-windows-polish.md`, 7 phases) and the owner's 5 answers (D-044).

## 2026-11-05 · docs/m1-done (D-043)

- M1 declared done by the owner, without the plan's 3 days of daily use; his open checks (the key, My words,
  paste checks, punctuation) and the agent's chain benchmark after Google's reset stay in STATUS Next, now
  alongside M2. The M2 plan is Next 1.

## 2026-11-05 · feat/m1-cut-guard (D-042)

- Fix (known gap 15): Gemini Transcribe sometimes cut a long mixed clip to a few English keywords (live
  2026-11-05 17:08 and 17:11: 2.4 and 2.1 words a second of speech; every other clip 5.1+). The chain now
  checks the main model's answer against the speech length (`Encoded.speech_ms`, set by the pipeline from the
  voice gate): 3+ s of speech and under 4 words a second → the next model is asked too (the one already
  running, else the next not resting) and the answer with more words is pasted; the main model is listed as
  passed ("its answer looked cut").
- Checked: 3 unit tests (the fuller fallback wins and only one fallback is asked; the cut answer stays when the
  fallback is shorter or fails; normal, short and unknown-length clips ask no one else); both live cut clips
  through the real pipeline (saved cut answer as the main model, real Flash-Lite as the fallback): both came
  back whole.

## 2026-11-05 · feat/m1-format-off (D-041)

- Smart formatting is off by default (the owner's choice after his `--no-format` try, 2026-11-05 19:45-19:50):
  dictation, `--record`, `--file` and `--bench` give the exact words; `--format` turns formatting on;
  `--no-format` is still accepted (both together is an error). Transcribe still gets its Roman step, and
  script answers the D-039 step.
- Checked: unit tests, clippy; a real clip without the flag (no format step, 1.4 s) and with `--format`
  (format step 0.88 s).

## 2026-11-05 · fix/m1-chain-speed (D-040)

- Fix: dictation took 5-11 s all evening once Transcribe's free requests ran out (18:13). Root causes: the
  chain still waited 3 s for a slow Flash (3.8-5.7 s tonight) before asking Flash-Lite, and a used-up model
  was asked again every hour, which cost 4-6 s each time Google was slow to refuse (and after every restart,
  since the rest lived in memory only).
- A daily 429 now rests the model until midnight in Los Angeles + 5 min (`client::daily_rest`, US summer time
  handled), kept in `%LOCALAPPDATA%\SonicScript\resting.json` across restarts and runs (each run merges its
  own engines). While the main model is resting or has failed, Flash and Flash-Lite are asked together.
- Gemini's answer also loses running single time stamps ("kya 0:02 chahata tha? ... 0:08 ...", live 19:56),
  which D-039's range rule missed.
- Checked: 4 new unit tests (racing on failure and on rest, the rest file across a restart and a lone run, the
  Pacific midnight in winter / summer / both switch nights) + the stamp test; the real chain on the 19:57
  recording: 6.0 s with Transcribe asked, 1.8 s with it resting (Flash-Lite answered, Flash's daily 429 saved
  too). The first try showed a lone `--engine` run erasing other engines' rests; fixed before commit.

## 2026-11-05 · fix/m1-script-answer (D-039)

- Fix: a 57 s dictation with `--no-format` (live, 19:48) pasted "纳 jaane galtee kar gayaa kuch. baree
  traansakript 0:00 - 0:05mein ...". Root cause: Gemini Flash answered in Devanagari with a subtitle-style
  timeline ("0:00 - 0:05", every 5 s) and a Chinese character for "Na"; with no format step and no refine
  step on Flash, only the local letter table made it Latin (`needs_llm` was set but nothing acted on it).
  Seen by comparing the owner's last 10 dictations with OpenAI `gpt-4o-mini-transcribe` (~$0.012; the other
  9 were as good as or better than OpenAI's).
- `gemini.rs` `clean` removes a running timeline (2+ ranges, from 0:00, each starting where the last ended);
  a spoken "3:00 - 4:00" stays.
- New spec step `romanize` (Groq `gpt-oss-20b`, `romanize-v1.md`, as Transcribe's refine): run after an engine
  with no refine of its own when its answer is mostly Urdu / Devanagari script (`normalize::needs_romanizing`,
  the rules' 30%) and no format step made it Roman (`--no-format`, or the optional format step failed). Without
  the step (no Groq key) or when it fails, the answer is an error and the chain asks the next engine.
- Checked: 6 new unit tests; the saved 19:48 answer through the real Groq step (884 ms): timeline gone, "纳" →
  "na", all Roman (sentences start lower-case, as `romanize-v1.md` writes); `--file` on the recording with
  Flash (`--no-format`): this time Flash answered in Roman by itself.

## 2026-11-05 · docs/session-2026-11-05b

- STATUS for a new session after PR #28: the third live try (18 dictations, mic 14-16 ms, Transcribe answered
  all, key-up → text 2.1-3.6 s; 2 clips cut, 1 prompt paste), the OpenAI second opinion and where its script
  lives, Next rewritten in the order agreed with the owner (test without the formatter, key, My words, the
  Transcribe guard, 3 days of daily use, chain benchmark, M2 plan, optional one-step formatting), the
  one-step formatting answer (which engines can take format rules in their prompt). M1 plan progress updated.

## 2026-11-05 · fix/m1-format-injection (D-038)

- Fix: a dictation pasted the format step's whole prompt into Chrome (live, 17:38). Root cause: the transcript
  went to `gpt-oss-120b` as a bare user message, and the owner had said "...mujhe abhi tune pehle jo reply
  diya na, yeh mere ko dobara bhej..."; the model took it as an order to itself. Sent bare again: right 1 time
  in 8 (3 answers "Sorry, I don't have a previous reply to resend.", 4 empty).
- Every text step (format / refine, app and `eval`) now sends the transcript wrapped by
  `spec/prompts/transcript-wrap-v1.txt` (in `<transcript>` tags, marked as speech to rewrite): 8 of 8 right on
  that text, and the app's real path (`--file` on the recording, Transcribe + format) twice.
- The answer is checked before it is used (`client::checked`, `engines.checked`): empty for 4+ words said,
  2+ lines of its own prompt, over 2x + 10 the words, or under 30% of 15+ words → asked once more, then a
  format failure (Flash / Flash-Lite keep their own text and the console says "Not formatted"; after
  Transcribe the chain moves on). Nothing like the leak can be pasted any more.
- D-031's checks re-run with the wrapper (Groq only): M0 word loss none (Flash-Lite transcripts, run
  `fmt-wrap-m0`: 95.93 → 95.99% on the same 42 speech clips, only `nm-05` changed: "9:00 baje" → "9 baje");
  layout (run `fmt-wrap-layout2`) 92% after Flash-Lite and 88% after Groq Whisper, all of the drop from
  `fmt-04` (the recipe's steps: last two merged, or "- " for "1."; this clip varied before too); `fmt-11`'s
  filler "wo na" now removed; `fmt-10` gave "Meeting Tuesday ko." (was "Meeting Tuesday ko rakh lo."). A first
  version of the check refused `fmt-01` (the prompt's own example, said aloud) and `fmt-10` (run
  `fmt-wrap-layout`), so one copied prompt line and short take-backs are allowed now.
- Also found (no code change): with our Whisper hint `gpt-4o-transcribe` wrote the hint instead of the speech
  on 8 of the owner's 15 live clips; Gemini Transcribe cut 2 of them to a few English keywords (STATUS known
  gap 15). Comparison in `eval/results/live-openai-2026-11-05/` (local, ~$0.04).

## 2026-11-05 · docs/session-2026-11-05

- STATUS for a new session: PRs #24-#26 merged, all M1 code steps built, Next rewritten (owner's live tries,
  chain benchmark, the M2 plan, the name). The name "Utter" was checked and is taken by an AI dictation app
  for Mac and iPhone (utter.to); its main domains are registered. M1 plan progress updated.

## 2026-11-05 · feat/m1-key (M1 step 6, D-018, D-037)

- The dictation key is a setting: `%APPDATA%\SonicScript\config.toml` (`hotkey = "RightCtrl"`), set with
  `sonicscript --set-key` (press the key or mouse side button; Esc keeps the old one), `--set-key <name>`
  (F9, RightAlt, "right control", XButton1, ...) or `--set-key list`. Allowed: Ctrl / Shift / Alt on either
  side, Right Win, F1-F24, Pause, Scroll Lock, Caps Lock, Insert, mouse side buttons. Refused with a reason:
  keys that type, edit or move the cursor, Enter, Esc, Left Win, Num Lock, media keys, "Ctrl" without a side.
  Warnings for Left Ctrl / Left Alt / Shift (shortcuts and capitals start and cancel a recording), Caps Lock
  and the side buttons (they lose their own job). A bad value in the file: Right Ctrl and a notice.
- How each key reaches the app (D-037): Ctrl / Shift pass on as before; Alt / Win pass on with a masking
  keystroke (unassigned key 0xE8) so a lone tap opens no menu; all others are kept from the app.
- `hotkey.rs`: a mouse hook only when a side button is the key; `capture()` for `--set-key`;
  `inject::send` shared for the mask key. The console names the chosen key. Usage text: Groq is no longer
  listed in the chain (D-035).
- Tests: `setkey` (names, spellings, refusals by name and by key code, pass-on per key, config read / write
  keeping other lines). Real runs on a scratch `APPDATA`: `--set-key list`, `Space` refused, "right control"
  and `Mouse4` saved, dictation starting with `Mouse4` (mouse hook installed) and `F9`, a bad value falling
  back to Right Ctrl. Not checked: real key presses (the hook ignores keys sent by programs, so only the owner
  can test them).

## 2026-11-05 · feat/m1-mic-open (D-036)

- Mic-open delay (known gap 8) measured with the new `desktop/win/examples/mic_open.rs`: building the stream
  takes ~110-160 ms every time, and after ~20 s idle Windows wakes the device, which adds ~330 ms (to
  `default_input_config`, or to the build when the format is cached): the 113-133 / 462-512 ms seen live.
  Caching the device and format moved the wait, it did not remove it. A stream built ahead and only started on
  key-down: 0 ms after 20 s and 2 min idle, first audio after ~14 ms. A built, not started stream shows no mic
  icon on the taskbar (checked by screenshot; a recording stream shows it).
- `audio::Ready`: one stream prepared at start-up and after each recording (on its own thread); the key-down
  starts it and waits for its first audio (nothing lost: Windows buffers it); a stream that is broken (device
  gone, default mic changed, invalidated, e.g. after sleep) or gives no audio within 300 ms is dropped and the
  mic is opened the old way. WASAPI's glitch flag on the first packet (`Xrun`) and reroutes no longer count as
  errors or get printed. The console says "mic ready in N ms" (prepared) or "mic opened in N ms".
- Tests: manual `ready_mic_starts_at_once_after_idle` (25 s idle: started in 2 ms, 1 s = 16,000 samples; the
  next one prepared again). Not checked yet: the owner's live dictations (`ms.mic_open` in the history).

## 2026-11-04 · feat/m1-no-groq-fallback (D-035)

- The owner approved taking Groq Whisper out of the chain (STATUS Next 1 of the end-of-session docs). Chain:
  Transcribe → Flash → Flash-Lite. When all fail: `ApiError::AllFailed` with each reason; dictation prints
  "Nothing typed", the reasons and what to do (`failure_notes`). `ApiError::is_key_error` (bench stops on
  key errors also inside `AllFailed`). `spec/models.json` regenerated (Groq no longer fallback 3).
- Tests: the chain's all-fail error, the spec chain, the console notes. Real: the startup line without Groq; a
  run with a wrong Gemini key listed all three refusals.

## 2026-11-04 · docs/end-of-session

- Second live try (8 dictations): Transcribe and Flash out of free requests, Flash-Lite timed out 5 of 6, so
  Groq answered 5 clips after 10-12.6 s, misspelt; the owner wants Groq's answers gone (STATUS Next 1, a
  decision for the owner). Mic open: 113-133 ms or 462-512 ms, half each; tap and hold start alike.
- Independent check with OpenAI `gpt-4o-transcribe` (~$0.01): it wrote only the first sentence of every clip
  longer than ~9 s, so it is no judge for long natural speech; on the short ones it showed Groq's misspellings.
- STATUS made current for a new session (Next rewritten, known gap 8 with the new numbers).

## 2026-11-04 · fix/words-no-args

- Fix: `sonicscript` with no arguments (dictation) crashed at start ("range start index 1 out of range") since
  PR #20. Root cause: `mywords::command` sliced the arguments before checking there was one; the real runs
  before the merge all had arguments. Now `split_first`; test `other_arguments_and_none_are_not_words_commands`.

## 2026-11-04 · feat/m1-words (D-034)

- My words, phase A of the word-list plan (owner's go-ahead 2026-11-04: "for every user, not just me"): new
  `desktop/core/src/words.rs` (forgiving parser, merge, `prompt_line` with near-word choice for long lists,
  `GUIDE`), `desktop/win/src/mywords.rs` (`words.txt` in `%APPDATA%\SonicScript`, `--words`, `--words-prompt`,
  `--words-add`, `--words-paste`, `--words-suggest`), `spec/prompts/words-assistant-v1.md` (the prompt for the
  user's AI assistant). The list goes to the cleanup (`Normalizer::with_words`, mishearings as replacements),
  the Gemini audio prompt and the format step (`spec::with_words` fills `{word_list}` at request time;
  `Client::with_words`). `inject::copy`, `inject::clipboard_text`.
- Tests: 6 new (parser on our format and on a chatbot answer, merge, near words, words in both requests,
  mishearing replacement, history names, the assistant prompt); real runs of every command.

## 2026-11-04 · feat/m1-sound

- Start / stop chimes (owner, live try: "show that my click registered"): new `desktop/win/src/cue.rs`; a rising
  chime on start (played before the mic opens), a falling one after the mic closes, a low note on cancel or a
  mic failure; `--no-sound`. Tests: the WAV the chimes are built from; a manual test that plays them.
- Word-list plan: the final list of six ways to add words (owner, 2026-11-04); the AI-assistant prompt now says
  plainly that the words are for a dictation / transcription app; the app-in-front hint marked as a separate,
  optional idea.

## 2026-11-04 · feat/m1-chain-gemini-first (D-033)

- Owner's live try on `feat/m1-history` (16 dictations, read from the history): paste worked in Windows
  Terminal, the Claude app and Chrome; "Nain, aare terny athrakaam ..." was Groq Whisper's own text (Transcribe
  out of free requests after the day's benchmarks, Flash / Flash-Lite slower than 3 s); from #7 on the format
  step hit Groq's token limit (text unformatted); "Claude" heard as "McCloud" by Flash.
- Chain: `groq-whisper-hint` is a last resort (`"last_resort": true` in `spec/providers.json`): asked only once
  every Gemini model asked has failed or is resting, never for slowness. `Chain::with_last_resort`,
  `is_last_resort`; `spec::Link.last_resort`; the startup line says "... then groq-whisper-hint if all of them
  fail". Tests: 3 new chain tests, the spec chain test.
- `docs/plans/2026-11-04-word-list.md`: draft plan for the personal word list (owner's ideas: paste box, file,
  import from the user's AI assistant, dictating words to it; plus starter packs, app-in-front context, learning
  as suggestions, no limit on the list with small prompts).

## 2026-11-04 · feat/m1-history (D-032, M1 step 9)

- Dictation history: new `desktop/win/src/history.rs`; dictation and `--record` keep the whole recording
  (Ogg/Opus 16 kbps, ~2 KB/s) and one JSON line (raw and final text, engines, app in front, paste result,
  timings) in `%LOCALAPPDATA%\SonicScript\history`; `--no-history` turns it off. It is also step 9's latency
  log, which the plan had without audio or text: the owner asked for both (D-032).
- `encode::Stream::finish` takes `&mut self` and keeps its packets, so the whole recording can be built after
  the trimmed clip; `pipeline::run_streamed` takes `&mut Stream`. `inject::app_in_front` names the app.
- Tests: `finish_can_be_called_again_for_the_whole_clip`, `a_dictation_leaves_its_recording_and_one_line`;
  a real 2 s `--record` into a scratch folder (3.5 KB, plays back at 2.0 s).

## 2026-11-04 · feat/m1-paste (M1 step 7)

- Paste at the cursor: new `desktop/win/src/inject.rs`; dictation pastes the text into the window in front and
  restores the user's clipboard (plan step 7). Beyond the plan: our own console's input is cleared after every
  paste and old console windows get spaces for line breaks (pasted text in a console would run as PowerShell
  commands); `--record` never pastes; the key hook ignores injected keys (our Ctrl+V cannot disturb the
  tap / hold rules); waits up to 1 s for held modifiers.
- Tests: 3 unit tests (keys per window class, line breaks, saved formats) + 1 manual real-clipboard test.

## 2026-11-04 · feat/m1-format-baje

- PR #14 (graphify rule) merged.
- `format-v1.md`: clock times in Urdu / Hindi are "N baje", also when the transcript has "N:00" (Gemini
  Transcribe writes "baje" that way: live "Kal 6:00 milte hain", fmt-09). "Hmm" stays: the owner wants it.
- Checks: runs `fmt-baje-app2` (fmt-09 right; Layout 88%, the same as the old prompt's re-run `fmt-old-app`:
  fmt-04's recipe steps merge in both), `fmt-baje-m0` (no word loss, first wording), `fmt-baje-m0-final`
  (stopped at 8 clips by Groq's rate limit, unchanged).
- Punctuation measured (no change): the format step keeps Gemini's sentence ends and drops about half its
  commas; waiting for the owner's live examples.
- STATUS Next: M1's open steps (paste, key, latency log) before the mic delay and the chain benchmark.

## 2026-11-04 · docs/graphify-rule

- PR #13 (model catalog) merged after CI passed.
- CLAUDE.md graphify rule: `explain <file>::<symbol>` / `path` first for code, `query` with a budget for open
  questions; spec JSON is not in the graph and Markdown only as headings that `graphify update` does not
  refresh, so those are read directly. Checked: all 39 code files in the graph (987 nodes), current.

## 2026-11-04 · feat/m1-catalog (model catalog refreshed)

- PR #12 (smart formatting) merged after CI passed.
- `spec/models.json` re-generated from the finished `m1-engines` run: Transcribe + cleanup 97.3% on all 44
  clips (was 97.9% on 22), Flash 97.5% (44 rows, 3 daily-limit errors), Flash-Lite 97.0%, Groq 89.2%, OpenAI
  94.7% / 93.8%.
- `spec/providers.json` catalog notes for Transcribe and both OpenAI models: the second step is the 120b format
  step while formatting is on, the 20b Roman cleanup with `--no-format`.
- `catalog.py`: the model page lists the chain in its order (default, then fallbacks) before other models;
  sorting by accuracy alone had put Flash (97.5%) above the default. Test added (39 `eval` tests).

## 2026-11-04 · feat/m1-format (live list / take-back try)

- Owner's second live try, 5 dictations: list, take-back (5 → 6) and lakh amount ("3,50,000") came out right;
  "6 baje" written "6:00" (prompt follow-up noted). Transcribe hit a per-minute 429, then its daily limit;
  both limit notices seen live for the first time; Groq Whisper answered the last clip after 7.6 s, garbled.
- PR #12 CI all green (`checks`, `docs-gate`, `rust`). STATUS: Next 1 is now only the owner's merge; C: has
  13 GB free.

## 2026-11-04 · feat/m1-format (hand-off)

- PR #12 opened. Owner's first live try with formatting: 2 natural dictations right (2.9 s / 3.8 s key-up →
  text); "hmm" kept, names as heard (no user word list in the app yet); no list dictated live yet.
- Docs brought up to date for a new session: STATUS (Start here, In progress, Next re-ordered: PR #12, catalog,
  mic delay, chain run with formatting, paste), known gaps 4 / 9 / 13, the M1 plan's Progress, README, blueprint
  §4.4 / §4.5, the Wispr research note, ARCHITECTURE (`reformat.py`); D-029 / D-030 headings note what D-031
  changed.

## 2026-11-04 · feat/m1-format (checks done, D-031)

- `format-v1.md` final tweaks from the audio checks: a lead-in ending in a full stop still makes a list (an
  "Office ke rules" example; the owner's rules clip fmt-03 stayed a sentence on both engines before), lakh
  grouping "never 250,000" and "1.5 lakh" → 1,50,000, "wo na" as a filler spelling.
- Scoring: "6:00" counts as "6"; fmt-11 also checks "wo na".
- Fix: `vad::tests::m0_recordings` read every WAV in `spec/fixtures/audio` and expected 44; with the 13
  `fmt-*` recordings it found 57 and failed (only on the owner's PC: CI has no recordings). It now reads the
  M0 clips only.
- Results: app `m1-format-app` Layout 96%, 0 errors, p50 2.6 s; `fmt-lite-v3` 96% after Flash-Lite and after
  Groq Whisper; no word loss on the 40 non-list M0 clips (`fmt-120b`, `fmt-120b-final`). Decision D-031.
- Next 4's bench ran (Transcribe all 44 M0 clips, Flash 41; it also asked 2 new `fmt` clips before Google's
  per-day 429); catalog left for its own PR. Groq: ~350K tokens on 120b in a day without a daily 429 (known
  gap 3 corrected).

## 2026-11-03 · feat/m1-format (smart formatting, D-030; not merged)

- Plan `docs/plans/2026-11-03-m1-smart-formatting.md`, approved by the owner with the defaults. One change
  while building: a top-level `format` step in `spec/providers.json` instead of four new `+fmt` engine ids and
  a second chain (same behaviour; engine ids and the catalog untouched).
- `spec/prompts/format-v1.md` (from `format-draft.md`): lists ("- " / "1."), take-backs, fillers, digits with
  lakh grouping, romanizing. Tightened after a text-only check: the list replaces the sentence (120b had
  written both on `ag-03`), joining words are never the lead-in, a sentence after a list stays (both models
  had dropped words on `lg-02`), an "actually" take-back example.
- Test set: 13 `format` sentences `fmt-01`..`fmt-13` with `say` (what to speak) and `format` checks;
  `record` shows `say`. Not recorded yet.
- `eval`: `format_score` (list kind / count, absent words, digits) per row (`format_rate`), "layout" column
  in the summary and report; accuracy, English and latency now come from the M0 clips only, the catalog too.
  `reformat` command (format step on saved transcripts); `bench --only` and `--format-model`; `clean_output`
  keeps line breaks; an optional format step that fails keeps the engine's text (`unformatted`).
- App: `spec::format()`, `Client::with_format` (replaces the engine's refine; optional after engines without
  one), `--no-format`, cleanup per line, multi-line console output, "Not formatted" notice, bench rows
  `app-...+fmt`. A missing Groq key turns formatting off with a notice.
- Found: Groq's free `gpt-oss-120b` / 20b allow 8K tokens/min and 200K/day; a format call is ~1,300 tokens,
  so ~160 formatted dictations a day (STATUS known gap 3). A first 20b / 120b run on saved transcripts was
  stopped half-way (the prompt changed under it) and deleted; run `fmt-say-120b` (the 13 sentences as text):
  12 of 13 right.
- Owner recorded the 13 `fmt` clips. Run `m1-format`: Groq Whisper + format, Layout 88.5% with both 120b and
  20b (20b lost words on fmt-11); Flash-Lite timed out on all 13 (Google). `spec/providers.json` `format`
  gained `rpm: 6` (benchmarks only: 8K tokens/min).

## 2026-11-03 · docs/handoff-formatting

- PR #10 (engine chain, D-029) merged after green CI.
- Hand-off: STATUS rewritten (current facts only, older results condensed, Next 1 = smart formatting with a full
  brief); README, blueprint §4.2 / §4.4 and the M1 plan's progress brought up to D-029 / D-030. A session for
  the formatting work could not start: drive C: has 3.4 GB free and a git worktree needs ~5.4 GB (STATUS
  Start here 3).

## 2026-11-03 · feat/m1-fallback (engine chain)

- Owner chose a chain of free Gemini models with Groq last (D-029, supersedes D-028): `spec/providers.json`
  `chain` (Transcribe + Groq Roman cleanup 4 s → Flash 3 s → Flash-Lite 3 s → Groq Whisper) replaces
  `default_engine` / `fallback_engine`; `spec::chain()`.
- `client::Racer` became `client::Chain`: any number of engines, each with its wait; errors move on at once; a
  429 rests the engine (1 h for a daily quota, else ≥ 60 s) so later clips skip it without a request.
  `ApiError::RateLimited` gained `daily` (Google's `...PerDay...` quota; never retried); `Transcript` /
  `Outcome` list the engines `passed` over (`Slow` / `Failed` / `Resting`); `Transcribe` gained `id`.
- App: an engine with a missing key is left out of the chain with a notice; dictation prints which model
  answered and, once, that a model's free requests are used up (when they come back, how to change the main
  model). Bench rows `app-<a>~<b>~...` with `passed`; the summary counts answers per engine.
- Catalog: `fallback` is the place in the chain; `--models` shows "fallback 1 / 2 / 3".
- Run `m1-chain`: 94.9%, 0 errors, p50 3.3 s / p95 4.3 s; Transcribe 98.5% on its 24 clips.
- Research: Wispr Flow's formatting (`docs/research/2026-11-03-wispr-flow.md`) and a smart-list formatting test
  (`docs/research/format-test/`): works in ~1 s on Groq; not wired in. Owner's choices: D-030 (formatting on by
  default, one formatter for every app, `gpt-oss-120b` after every engine) and 3 s waits for Flash / Flash-Lite
  (run `m1-chain` used 2 s).

## 2026-11-03 · feat/m1-fallback

- Default engine is now `gemini-3.5-flash-lite` with `groq-whisper-hint` as the fallback (D-028):
  `spec/providers.json` `default_engine` / new `fallback_engine`, `spec::fallback_engine()`.
- `desktop/core` `client::Racer`: primary on a thread, the fallback too after 3 s (`FALLBACK_AFTER`) or on a
  primary error, first good answer wins, both failing returns the primary's error. `Transcript` and `Outcome`
  say which engine answered and why the fallback was asked (`api::Fallback`); `Transcribe` gained `warm`.
- App: without `--engine` the pair runs (a missing key runs the other engine alone, with a notice); `--engine
  <id>` runs one engine alone. Dictation prints "Used groq-whisper-hint: ... was slow" when Groq's text is used.
  `--bench` rows are named `app-<primary>~<fallback>` with `answered_by` and `fallback`; the summary counts
  fallbacks.
- Catalog: `fallback` flag per model; `--models` marks it. `spec/models.json` regenerated (same runs).
- Run `m1-fallback`: 95.5% on speech clips, 0 errors, p50 1.5 s / p95 3.4 s; Groq answered 10 of 42.

## 2026-11-03 · docs/handoff-default-engine

- PRs #6 (step 4), #7 (step 5) and #8 (engines + catalog) merged into `main` after green CI.
- Owner chose the default engine: Gemini 3.5 Flash-Lite with Groq Whisper as the fallback (D-028); not built yet.
- Key test passed with `sonicscript.exe` in a private test window: Enter and Esc are taken only while recording,
  Right Ctrl + C still copies. It also measured the mic-open delay (406 ms of a 1.5 s recording lost).
- The VS Build Tools CMake folder added to the owner's user PATH.
- STATUS rewritten as a hand-off: "Start here" and an ordered "Next" for the next session.

## 2026-11-03 · feat/m1-engines

- Owner's live test of step 5: tap / hold, mic and key-up → text in 0.2 s work, but Groq Whisper mangled natural
  Urdu / Hindi. New rule (D-025): quality first, 1-2 s after the key-up is fine.
- Researched how the big apps dictate (Claude: Anthropic's own transcription service; ChatGPT: GPT-4o-based
  transcription; Gemini app: Gemini 3.5 Transcribe; Wispr Flow: recognizer + LLM cleanup).
- New engines (D-026): OpenAI `gpt-4o-transcribe` / `gpt-4o-mini-transcribe` (provider `openai`, paid),
  `gemini-3.5-flash`, `gemini-3.5-transcribe` (new kind `gemini-transcribe`) and a `refine` step (Roman cleanup
  on Groq `gpt-oss-20b`) usable after any engine; per-engine `rpm`; provider `min_billed_s`. `eval`:
  `GeminiTranscribe`, `Refiner`; `OPENAI_API_KEY` read like the other keys. `gemini-3.8-flash` tried and
  dropped (every request timed out or got 503 on the free tier; its rows are kept aside in the run folder).
- App: shared HTTP layer `desktop/core/src/api.rs` (`ApiError` replaces `GroqError`), `gemini.rs`, `client.rs`
  (speech call + refine), `spec.rs` with an `Engine` enum; `sonicscript --engine <id>` for every mode.
- Model catalog (D-027): `catalog` info in `spec/providers.json` (label, free / paid, free-tier limits + source,
  notes), `python -m sonic_eval catalog` → `spec/models.json`, `sonicscript --models` prints it free / paid.
- Benchmark `m1-engines` (44 M0 clips) and the same engines through the app (`m1-cli-*`): Gemini 97-98%,
  OpenAI 93-95%, Groq 89%; numbers in STATUS. Flash and Transcribe ran out of their free daily limit (20 / 25,
  from Google's own 429s) after 22-25 clips. OpenAI cost about $0.06 in all.
- Found on the way: the hidden Terminal pane in the desktop app throttles a program's console output (the
  bench advanced only while its tab was read), so long runs go to a log file instead.

## 2026-11-03 · feat/m1-hotkey

- M1 step 5, dictation key + recording. `sonicscript.exe` without arguments now listens for Right Ctrl: tap to
  start, tap or Enter to stop, hold to talk, Esc cancels, a shortcut with the key cancels (D-015); the text is
  printed in the console (pasting is step 7). New in `core`: `keys.rs` (the key rules as a tested state machine;
  a 300-400 ms press is a tap, D-023), `resample.rs` (any mic rate → 16 kHz) and `encode::Stream` (Opus while
  recording; the Ogg file is cut from the packets at the end, D-024); `pipeline::run_streamed`. New in `win`:
  `hotkey.rs` (low-level keyboard hook thread), `audio.rs` (cpal capture + worker), `dictate.rs`, and
  `--record <seconds>` as a mic check. The console is switched to UTF-8.
- `--bench` with the streaming encoder: 89.3% (vs 89.1% re-encoding the trimmed clip), Latin 100%.
- Not tried live: the PC was locked and had no microphone connected while this was built (STATUS known gap 8).
  Found on the way: Claude's shells run without a foreground window, so simulated keys only work when the PC
  is unlocked.

## 2026-11-03 · feat/m1-bench

- M1 step 4, golden check: new app crate `desktop/win` (binary `sonicscript.exe`) with `--file <wav>...`
  (prints the text) and `--bench <dir> [--out <file>]` (one row per clip in `eval`'s `results.jsonl` shape,
  paced by `rpm`, p50 / p95 summary). The Groq key is read from the process environment, then the Windows user
  environment. `spec.rs` now reads the engine's `audio_per_hour` price for the cost column.
- `eval` `rescore` fills in the category of rows that have none (the app's rows), so the app is scored by the
  same scorer and report as M0. `desktop/core/examples/transcribe.rs` removed (replaced by the app).
- Fix: the first run scored 87.2% vs M0's 89.1% on the same 41 clips. Root cause: the voice gate trimmed to
  speech + 200 ms, and with that little silence Whisper translated `mx-02` / `nm-02` into English and cut
  "Let's" to "La"; the whole clips came back right. Padding is now 500 ms (D-022): 89.1% accuracy, Latin 100%,
  English 86%, silence 100%, 0 errors, clip → text p50 298 ms / p95 387 ms. No trim at all: 88.8%.
- Housekeeping: C: was full (0 MB), which broke `git pull` and builds; npm and npx caches cleared (5.4 GB free).

## 2026-11-03 · feat/m1-encode-groq

- M1 step 3, encode + Groq: `desktop/core/src/encode.rs` (Ogg/Opus 16 kbps, VoIP, written with libopus and
  the `ogg` crate, no ffmpeg), `spec.rs` (engine + hint from `spec/providers.json`, new `default_engine` field),
  `groq.rs` (blocking reqwest + rustls, `warm()`, 10 s timeout, one retry on 429 / 5xx / dropped connection),
  `pipeline.rs` (gate → trim → Opus → Whisper → cleanup, timings per stage) and `examples/transcribe.rs`.
- Real run on the 44 M0 recordings (one pass, Groq): 0 errors, 0 retries, both silence clips not sent,
  accuracy 88.0% (M0 `groq-whisper-hint` opus16 with the same scorer: 84.4%), Latin-only 100%, English kept
  86.6% (same as M0), ~7.7 KB per clip, Whisper request p50 178 ms / p95 228 ms, clip → text p50 341 ms /
  p95 455 ms.
- Opus complexity tried at 5 (encode 2x faster) but accuracy fell to 86.9% and English kept to 82.9%: kept at
  10 (D-021). Encoding takes ~125 ms per 4 s of audio, so step 5 should encode while recording.
- The FLAC fallback was not needed: libopus builds, but needs CMake on PATH (D-021).

## 2026-11-03 · feat/m1-voice-gate

- M1 step 2, voice gate: `desktop/core/src/vad.rs` (`detect` → speech ms + the samples to keep, 200 ms padding)
  and `examples/gate.rs`. On the 44 M0 recordings: 42 speech clips sent, both silence clips dropped.
- Not a straight port of `eval` `trim_silence`: its fixed -42 dB threshold finds 3.6 s of "speech" in the
  room-noise clip `si-02`. The gate uses each clip's own noise floor + 9 dB and a zero-crossing limit (D-020);
  the thresholds were picked on 7 variants of every clip (no padding, room noise mixed in, +10 dB, quieter).
- `graphify` reinstalled (`pip install --user graphifyy`, 0.9.101, Python 3.14); graph rebuilt, code only.
- Docs check before a new session: README status (M0 done, tap-to-toggle per D-015, repo folders), a Progress
  section at the top of the M1 plan, STATUS "Start here" says PR #4 must be merged first. PR #3 merged.

## 2026-11-03 · feat/m1-normalizer

- Toolchain confirmed: Rust 1.99.1 + VS 2022 Build Tools (C++), a test program links and runs (plan step 0).
- M1 step 1, text cleanup: new Rust workspace `desktop/` with crate `sonicscript-core`
  (`core/src/normalize.rs`); rules as data in `spec/normalizer/rules.json`, 22 shared test cases in
  `spec/normalizer/cases.json` (most from real M0 failures). `examples/clean.rs` runs it on stdin.
- On the 131 M0 Groq-hint outputs: Latin-only 93% → 100%, silence 0% → 100%, accuracy 85.1% → 90.0%.
- Fix during the work: the first phantom filter matched any whole hint sentence and dropped the real test
  sentence `mx-01` ("PR merge kar do aur deployment check kar lo."), which is the hint's first sentence. Root
  cause: Whisper only echoes the hint's tail, so only runs ending with the last sentence count now (D-019).
- CI: new `rust` job on windows-latest (fmt, clippy `-D warnings`, test); `.shipshape/config` check commands
  include the Rust checks.

## 2026-11-02 · plan/m1-windows

- STATUS rewritten as a hand-off for a fresh session ("Start here"); Rust + C++ Build Tools installing.
- PR #1 (M0) merged into `main`. M1 plan written: `docs/plans/2026-11-02-m1-windows-push-to-talk.md`.
- Owner decisions: tap-to-toggle default with Enter to finish (D-015), pop-up mic moved into M2 (D-016), model
  catalog + preferred/fallback model with a learned deadline (D-017), changeable dictation key (D-018). Blueprint §3.1, §4.2, §8 updated.

## 2026-11-02 · feat/m0-benchmark (word list)

- Word-list test: new engine `groq-whisper-hint-words` (`spec/prompts/whisper-hint-words-v1.txt`). Names
  improve but Whisper starts translating Urdu to English; word list moves to the cleanup step (D-014).

## 2026-11-02 · feat/m0-benchmark (license, fixtures)

- License MIT (D-012): `LICENSE`, README, blueprint, `eval/pyproject.toml`.
- Report review done by the agent: 3 expected texts fixed where every engine of both providers agreed
  (D-013); new `rescore` command re-scores saved runs without API calls (keeps `results.jsonl.bak`). All
  engines gained ~0.5-1 point; ranking unchanged.

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
