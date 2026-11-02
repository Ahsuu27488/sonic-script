# M1: Windows push-to-talk MVP (Rust)

**Goal.** Hold Right Ctrl, speak, release: clean Roman Urdu / Hinglish appears at the cursor in any normal app.
Done when the owner uses it instead of typing for 3 days, with latency logged locally (blueprint §8).

**Tier.** T1 (new local app, nothing live, no money, no user data leaves the PC except audio to the user's own
Groq key). Handled with T2 care in two places: the global keyboard hook and the clipboard (both touch every app).

## Scope

In: hotkey → record → voice gate → Opus → Groq → normalizer → paste; console window + log file; key from the
environment (`GROQ_API_KEY`, same as `eval/`).

Out (M2): tray, settings window, HUD / Voice Ring, Credential Manager, history UI, Gemini engine, provider
fallback, installer, autostart. Out (later): focus pill, streaming, Silero VAD (only if the energy gate fails).

## Files

```
desktop/                     Cargo workspace (new)
  Cargo.toml
  core/                      platform-free, fully unit-tested
    src/normalize.rs         script guard, word list (D-014), phantom filter, spacing
    src/vad.rs               energy gate + trim (port of eval audio.trim_silence)
    src/encode.rs            PCM → Ogg/Opus 16 kbps
    src/groq.rs              Whisper call, warm connection, timeout, retry once
    src/spec.rs              loads spec/providers.json + prompts at build time (include_str!)
    src/pipeline.rs          clip → text, with per-stage timings
  win/                       the app (binary `sonicscript.exe`)
    src/hotkey.rs            WH_KEYBOARD_LL on VK_RCONTROL
    src/audio.rs             WASAPI capture via cpal, 16 kHz mono
    src/inject.rs            clipboard paste + restore; Ctrl+Shift+V in terminals
    src/main.rs              wiring, console status, latency log
spec/normalizer/             rules + shared test cases (JSON), reused by Android later
  rules.json  cases.json
docs/                        STATUS, ARCHITECTURE (+ desktop/ map), CHANGELOG, DECISIONS
.github/workflows/ci.yml     + Rust job on windows-latest: fmt, clippy, test
```

## Approach

0. **Toolchain (owner, once, needs one UAC click):**
   `winget install Rustlang.Rustup` and
   `winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`.
   The C++ build tools are needed for the MSVC linker and to compile libopus.
1. **Normalizer first (core, no Windows code).** Rules as data in `spec/normalizer/rules.json`; `cases.json`
   built from the real M0 failures:
   - Script guard: Urdu (U+0600-06FF) and Devanagari (U+0900-097F) letters → transliterate with a small
     letter table; if more than 30% of letters are non-Latin, mark the output "needs LLM" (M1: re-ask once with
     `groq-whisper-hint+gptoss20b`'s LLM step).
   - Word list (D-014): fuzzy-match the user's words (edit distance on lowercase, word-boundary, length ≥ 3):
     "Jaminai" → "Gemini", "Codec" → "Codex". `words.txt` next to the exe in M1.
   - Phantom filter: drop output that is only a phrase from the Whisper hint ("main abhi meeting mein hoon")
     or a known Whisper phantom ("Thank you for watching", "Tch, tch").
   - Spacing and a trailing space so the next dictation does not glue on.
2. **Voice gate (D-007).** Port `trim_silence`; a clip with < 300 ms of speech is never sent. Tested on the
   two M0 silence clips (must give no request) and all 42 speech clips (must pass).
3. **Encode + Groq.** Ogg/Opus 16 kbps (`audiopus` static build + `ogg`); fallback FLAC if the libopus build
   fights us. reqwest + rustls, one client, connection opened when the key goes **down** (warm by release),
   10 s timeout, one retry on 5xx/429. Model id and hint from `spec/`, never hard-coded.
4. **Golden check (no mic needed).** `sonicscript.exe --file <wav>` and `--bench <dir>` run the real pipeline
   on `spec/fixtures/audio/`; accuracy must match `groq-whisper-hint` in M0 (±2 points) or better, and
   Latin-only output must go from 93% to ≥ 99% (blueprint §2.2).
5. **Hotkey + audio.** Low-level hook: Right Ctrl down → start capture + warm connection; up → stop. Cancel if
   any other key is pressed while held (Right Ctrl + C must stay a normal shortcut) or the hold is < 250 ms.
   The hook never swallows keys and its callback only posts a message (a slow hook gets unhooked by Windows).
6. **Inject.** Save clipboard → set text (plus `ExcludeClipboardContentFromMonitorProcessing` so Win+V history
   stays clean) → `SendInput` Ctrl+V (Ctrl+Shift+V when the foreground class is a terminal:
   `CASCADIA_HOSTING_WINDOW_CLASS`, `ConsoleWindowClass`) → restore clipboard after 300 ms. Elevated window
   (UIPI) → console message "copied, press Ctrl+V", text left on the clipboard.
7. **Latency log.** `%LOCALAPPDATA%\SonicScript\log.jsonl`: timestamp, clip length, stage timings, bytes,
   engine, error. **No audio and no text** (personal data); text only with `--log-text`.
8. Delegate boilerplate (CI job, cpal capture, clipboard helpers) to Gemini CLI through Foreman; the hook,
   normalizer rules and review stay with Claude.

## The check

- `cargo test` (normalizer cases, voice gate on the M0 clips, Ogg header), `cargo clippy -D warnings`, CI green.
- `--bench spec/fixtures/audio` result recorded in STATUS: accuracy, Latin %, p50 / p95 key-up → text.
- Manual matrix (blueprint §9): Notepad, Chrome, VS Code, Slack/Discord web, Windows Terminal with Claude Code,
  an elevated window. Right Ctrl + C / + V still work. Win+V history shows no SonicScript entries.
- Owner: 3 days of daily use; log shows p50 ≤ 700 ms key-up → paste.

## Risks

| Risk | Mitigation |
|---|---|
| Antivirus flags an unsigned exe with a keyboard hook | Local build only in M1; signing (SignPath) before M4. |
| Hook breaks Right Ctrl shortcuts | Never swallow keys; cancel on any other key; manual matrix row. |
| Clipboard restore races the paste | 300 ms delay, restore only if the clipboard still holds our text. |
| libopus does not build on Windows | FLAC fallback (pure Rust), ~4x bigger than Opus but ~2x smaller than WAV. |
| Groq free-tier limits (2,000 requests/day) | Far above personal use; 429 shows a clear message. |

## Rollback

Nothing is installed or live: stop the exe (Ctrl+C in its console) and the hook is gone. Revert = delete
`desktop/` and `spec/normalizer/` on the branch. The eval tool is untouched.

## Assumptions

- The owner's mic works through WASAPI shared mode at 48 kHz and we resample to 16 kHz (as `eval` records).
- `groq-whisper-hint` stays the default (D-011); the LLM step is used only when the script guard gives up.
- Right Ctrl is free on the owner's keyboard layout (D-006).
