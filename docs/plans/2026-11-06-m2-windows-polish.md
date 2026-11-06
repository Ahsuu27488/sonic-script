# M2: Windows polish Implementation Plan

> **For agentic workers:** execute inline with superpowers:executing-plans (the owner's standing choice). Steps
> use checkbox (`- [ ]`) syntax. One branch and one PR per phase (`feat/m2-<phase>`); each phase leaves the app
> working and is checked before the next starts.

**Progress:**
- 2026-11-06: plan written; the owner took the recommended answer to all 5 decisions (D-044) and asked
  for M2 to be built. Phase 1 started on `feat/m2-settings`.
- 2026-11-06: Phase 1 built (tasks 1.1-1.4). Change from the plan: the mic level is `audio::level()`, read by
  the HUD at its own frame rate, not a `Level` event (an event every 20 ms would flood the dictation channel);
  `dictate::run` takes `keep_history`, not the whole `Settings` (the key is still read by `setkey::load`).
- 2026-11-06: Phase 2 started on `feat/m2-tray` (from `feat/m2-settings`: PRs #35 / #36 still open); tasks
  2.1-2.4 written below.
- 2026-11-06: Phase 2 built (tasks 2.1-2.3; 2.4 checked except a real-key dictation). Changes from the plan
  (D-047): the app's exe is `sonicscript-tray.exe` (`SonicScript.exe` and `sonicscript.exe` are one file on
  Windows); start with Windows is off until ticked (the installer turns it on, Phase 7).
- 2026-11-06: Phase 3 built on `feat/m2-hud` (tasks 3.1-3.2; 3.3 checked except a real dictation). Change from
  the plan (D-048): the pill is solid `--surface-2`, no acrylic.

**Goal:** turn the M1 console app into a Windows app a non-technical friend installs from a GitHub release and
dictates with within 2 minutes (blueprint §8 M2 "done when").

**Tier:** T2 overall (new stored secrets in Credential Manager, an installer, start-with-Windows, a new UI
process). Each phase states its own tier; phases 1, 3, 6 are T1.

**Architecture:** `desktop/win` becomes a library + the existing `sonicscript.exe` CLI (bench, `--file`,
`--record`, dev use; unchanged). A new crate `desktop/app` (`SonicScript.exe`, Tauri 2, no console window)
links that library: the tray, the HUD Voice Ring window and the settings window are web views over plain
HTML/CSS/JS in `desktop/app/ui/` using `design/tokens.css` (no npm build step). The dictation loop stops
printing directly: it emits `Event`s to a `Sink`; the CLI's sink prints the M1 lines, the app's sink drives
the tray and the HUD. Settings move from one `hotkey` line to a full `config.toml`.

**Tech Stack:** Rust 1.99, Tauri 2 (tray, windows, NSIS bundler), WebView2 (ships with Windows 10/11),
`toml` + `serde`, Windows Credential Manager (`CredWriteW` / `CredReadW`), UI Automation (`windows` crate) for
the pop-up mic.

**Spec:** `blueprint.md` §3.1, §4.2, §6.1, §7, §8 (M2 row); `DESIGN.md` (HUD pill, settings window, motion,
accessibility); D-016 (pop-up mic), D-017 (learned deadline), D-018 / D-037 (key), D-029 / D-040 (chain),
D-032 (history), D-034 (My words), D-041 (formatting off by default).

## Decisions needed before Phase 1 (owner)

1. **UI stack: Tauri 2** as D-005 planned (recommended), or native Win32 windows. Tauri gives the HUD's glass,
   the settings page and the installer for little code; cost: ~2.5-3 GB more in `desktop/target` and ~60 MB
   of RAM for WebView2 while a window is open. **Drive C: has 6.4 GB free (2026-11-06)**: about 10 GB free is
   needed before Phase 2.
2. **Output modes** (English only / Urdu script / Devanagari, blueprint §4.4): each needs its own prompt and a
   benchmark. Recommended: **not in M2** (Roman only), after launch.
3. **History for new users** (D-032 left it open): recommended **on, with an off switch and "delete after 30
   days"** (setting, default 30).
4. **The friend's release is unsigned** (SmartScreen: "More info → Run anyway"); signing (SignPath) stays M4.
   Recommended: yes.
5. **Keys a friend needs:** the default chain's main model (Transcribe) needs a Groq key too (its Roman step).
   Recommended: onboarding asks for the Gemini key (required) and the Groq key (optional, "makes it better");
   without Groq the chain starts at Gemini Flash.

## Global Constraints

- Model ids, waits and prompts stay in `spec/` (AGENTS.md); the settings store engine **ids**, never copies.
- Keys are never printed, logged, put in a URL or shown in full in the UI (show the last 4 characters).
- No key is swallowed outside recording except the user's own dictation key as D-037 says.
- HUD: `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST`, never takes focus (blueprint §6.1); motion and
  states exactly as DESIGN.md "HUD pill"; `prefers-reduced-motion` honoured.
- The pop-up mic never appears on password fields; terminals off by default; never starts the mic by itself
  (D-016).
- The CLI keeps every M1 command and output; `--bench` stays comparable (no words, no settings).
- Work in this checkout, no worktree (disk).

## Review Focus

1. A `config.toml` written by M1 (`hotkey = "F9"` only, or hand-edited with comments) must load with that key
   and defaults for the rest → test `old_config_loads` (Task 1.1).
2. A main / fallback id that is no longer in `spec/providers.json` (a renamed model) must fall back to the
   spec's chain with a notice, not refuse to start → test `unknown_model_falls_back_to_spec_chain` (Task 1.2).
3. A key in both the environment and Credential Manager: the environment wins (developers' setup keeps
   working) → test `environment_beats_credential_manager` (Task 1.3).
4. Two copies of the app running (installer + dev build) would install two keyboard hooks and paste twice →
   single-instance check (Phase 2 check list).
5. The HUD or the pop-up mic stealing focus would make the paste go to our own window → Phase 3 / 5 checks
   paste into Notepad and Chrome with the HUD showing.

---

## Phase 1: settings, keys in Credential Manager, events (T1, `feat/m2-settings`)

No UI yet; the CLI gains what the UI will use, so every piece is tested before a window exists.

### Task 1.1: `config.toml` as real settings

**Files:**
- Create: `desktop/win/src/settings.rs`
- Modify: `desktop/win/src/setkey.rs` (`load` / `save` go through `Settings`; `read_hotkey` / `with_hotkey` removed)
- Modify: `desktop/win/Cargo.toml` (`serde = { version = "1", features = ["derive"] }`, `toml = "0.9"`)

**Interfaces:**
- Produces: `pub struct Settings { pub hotkey: String, pub main_model: Option<String>, pub fallbacks: Vec<String>, pub format: bool, pub sound: bool, pub history: bool, pub history_days: u32, pub popup_mic: bool, pub popup_off_apps: Vec<String> }` with `Default` (`"RightCtrl"`, `None`, `[]`, `false`, `true`, `true`, `30`, `true`, `[]`); `Settings::load(path: &Path) -> (Settings, Vec<String>)` (notices); `Settings::save(&self, path: &Path) -> io::Result<()>`; `settings::path() -> Result<PathBuf, String>` (= today's `setkey::config_path`).

- [ ] **Step 1: Write the failing tests** in `settings.rs`:
  - `old_config_loads`: `"# mine\nhotkey = \"F9\"\n"` → `hotkey == "F9"`, every other field default, no notice.
  - `missing_file_gives_defaults`: a path that does not exist → `Settings::default()`, no notice.
  - `broken_file_gives_defaults_and_says_so`: `"hotkey = "` → defaults, one notice containing `config.toml`.
  - `round_trip`: a non-default `Settings` saved to a temp dir and loaded back is equal.
  - `fallbacks_capped_at_two`: three fallbacks in the file → the first two, one notice.
- [ ] **Step 2:** `cargo test -p sonicscript settings` → FAIL (module missing).
- [ ] **Step 3:** implement with `#[serde(default)]` on the struct; `save` writes a header comment
  (`# SonicScript settings (written by the app; you may edit it)`) then `toml::to_string`; atomic write
  (temp file + rename). `setkey::load` reads `Settings.hotkey`; `setkey::save` loads, sets `hotkey`, saves.
- [ ] **Step 4:** `cargo test -p sonicscript` → all pass, including setkey's existing tests (adjusted only where
  they called the removed helpers).
- [ ] **Step 5:** commit (STATUS, CHANGELOG, D-045 "settings in TOML, superseding D-037's one-line file").

### Task 1.2: main model + 2 fallbacks from the settings

**Files:** Modify `desktop/win/src/key.rs`, `desktop/win/src/main.rs`, `desktop/win/src/dictate.rs`.

**Interfaces:**
- Consumes: `Settings` (1.1), `spec::chain() -> Vec<Link>`, `spec::engine(id)`.
- Produces: `key::choose_from(s: &Settings, engine_override: Option<&str>) -> (Choice, Vec<String>)`;
  `--engine` still wins over the settings; `--format` / `--no-format` / `--no-sound` / `--no-history` override
  the settings for that run.

- [ ] **Step 1: Failing tests** in `key.rs`:
  - `default_settings_give_the_spec_chain`: ids and waits equal `spec::chain()`.
  - `main_and_fallbacks_in_order`: main `gemini-3.5-flash-lite`, fallbacks `[gemini-3.5-flash]` → ids
    `[flash-lite, flash]`, waits: the main gets the spec chain's first wait (4 s), the next one the second (3 s).
  - `unknown_model_falls_back_to_spec_chain`: main `gemini-9` → the spec chain, one notice naming `gemini-9`.
  - `duplicates_are_dropped`: main also listed as a fallback → listed once.
- [ ] **Step 2:** run → FAIL. **Step 3:** implement; `main.rs` loads `Settings` once and passes it on;
  `dictate::run` prints the notices. **Step 4:** run all tests → PASS; real run: settings with
  `main_model = "gemini-3.5-flash-lite"` → the start line says `gemini-3.5-flash-lite, then ...`.
- [ ] **Step 5:** commit.

### Task 1.3: keys in Windows Credential Manager

**Files:** Create `desktop/win/src/keystore.rs`; modify `desktop/win/src/key.rs` (`read`), `main.rs`
(`--key <PROVIDER>` command), `Cargo.toml` (`Win32_Security_Credentials`).

**Interfaces:**
- Produces: `keystore::get(env_name: &str) -> Option<String>`, `keystore::set(env_name: &str, value: &str) -> Result<(), String>`,
  `keystore::delete(env_name: &str) -> Result<(), String>`; target name `SonicScript/<ENV_NAME>`, type
  `CRED_TYPE_GENERIC`, persist `CRED_PERSIST_LOCAL_MACHINE`. `key::read` order: process env → Credential
  Manager → `HKCU\Environment`. `sonicscript --key gemini|groq|openai` reads the key from the console without
  echo and stores it; `--key gemini --delete` removes it.

- [ ] **Step 1: Failing tests:** `environment_beats_credential_manager` and `credential_manager_beats_user_env`
  on a pure `read_from(sources: &[&dyn Fn(&str) -> Option<String>], var)`; manual test
  `real_credential_round_trip` (`#[ignore]`, target `SonicScript/TEST_KEY`: set, get, delete, get = None).
- [ ] **Step 2:** run → FAIL. **Step 3:** implement. **Step 4:** unit tests PASS and
  `cargo test -p sonicscript keystore -- --ignored` PASS; the key never reaches stdout / stderr (grep the test
  output for the test value).
- [ ] **Step 5:** commit (D-046: "keys in Credential Manager; environment first").

### Task 1.4: the dictation loop emits events

**Files:** Create `desktop/win/src/events.rs`; modify `dictate.rs` (all `println!` / `eprintln!` in `run`,
`transcribe` become events), `audio.rs` (RMS per 20 ms frame to a level callback).

**Interfaces:**
- Produces: `pub enum Event { Ready { describe: String, key: String }, Notice(String), Listening { device: String, mic_ms: u128 }, Level(f32), Processing, Inserted { text: String, ms: u128 }, Copied { text: String, why: String }, NoSpeech { seconds: f64 }, Failed { lines: Vec<String> }, Cancelled }`;
  `pub trait Sink: Send + Sync { fn send(&self, e: Event); }`; `pub struct Console;` (prints today's lines);
  `dictate::run(choice, settings, sink: Arc<dyn Sink>)`; `fn outcome_events(out: &Outcome, pasted: Option<&Pasted>, first: &str) -> Vec<Event>` (pure). `Level` is 0.0-1.0 (dBFS -60..0 mapped linearly), at most every 20 ms.

- [ ] **Step 1: Failing tests:** `outcome_events` for: text pasted → `[Inserted]` + the timing notice; copied
  → `Copied` with the reason; no speech → `NoSpeech`; all failed → `Failed` with the lines of today's
  `failure_notes`; daily limit → a `Notice` with today's `limit_notes` text. `level_of(&[f32])`: silence → 0.0,
  full-scale sine → ~1.0.
- [ ] **Step 2:** run → FAIL. **Step 3:** implement; `Console` prints exactly M1's text (the existing
  `dictate` tests keep passing). **Step 4:** all tests PASS; real dictation in a console: same lines as before.
- [ ] **Step 5:** commit; docs: ARCHITECTURE (settings, keystore, events), STATUS, CHANGELOG. Open the PR.

**Phase 1 check:** `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` clean; a real
dictation with default settings prints what M1 printed; with `main_model` set, the chain starts there; a key
stored with `--key gemini` and removed from the environment still works.

---

## Phase 2: the app shell and tray (T2, `feat/m2-tray`)

Detailed tasks below (written 2026-11-06 when the phase started; C: had 29 GB free). Decision 1 = Tauri 2 (D-044).

- `desktop/win` split into `lib.rs` (modules) + `main.rs` (CLI). New workspace member `desktop/app` (Tauri 2,
  `windows_subsystem = "windows"`, product name SonicScript, identifier `app.sonicscript`), static `ui/`.
- Runs `dictate::run` on a thread with a `TauriSink`. Tray icon: idle / recording / paused / problem (Voice
  Ring shapes from DESIGN.md); menu: Pause dictation, Settings…, Open history folder, Quit. Tooltip: the
  state and the last problem.
- Single instance (a named mutex; a second start opens Settings of the first). Start with Windows (HKCU `Run`
  value, a setting, on by default once installed).
- **Check:** start it, dictate into Notepad and Chrome, pause (key does nothing), resume, quit (the hook is
  gone: Right Ctrl + C still copies); start twice → one hook; sign out and in → it starts; CPU idle < 1%,
  RAM noted.
- **Rollback:** the CLI is unchanged; delete the `Run` value; uninstall.

### Task 2.1: `desktop/win` becomes a library + the CLI

**Files:** Create `desktop/win/src/lib.rs` (every module `pub`); modify `main.rs` (uses `sonicscript::...`).
- [ ] **Step 1:** move the `mod` lines to `lib.rs`; `main.rs` keeps only the CLI. No behaviour change.
- [ ] **Step 2:** `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` green;
  `sonicscript --models` and `--file` print as before.

### Task 2.2: pause and one dictation per user, in the library

**Files:** modify `hotkey.rs` (`set_paused(bool)`, `Hook.paused`), create `instance.rs`, modify `dictate.rs`.
**Interfaces:** `hotkey::set_paused(p: bool) -> bool` (false when no hook yet): while paused the hook passes
every key and button on untouched and runs no rules; pausing during a recording cancels it (a `Cancel` action
down the channel) and resets the rules so Enter / Esc are no longer taken. `instance::claim() -> Result<Guard,
String>`: a named mutex `Local\SonicScript.Dictation`; `dictate::run` takes it first, so the CLI and the app
(or two apps) never install two hooks (Review Focus 4).
- [ ] **Step 1: Failing tests:** `paused_hook_passes_everything` (dictation key down / up while paused: no
  action, not swallowed, even for a `PassOn::Never` key); `pause_cancels_a_recording`;
  `second_claim_is_refused` (two claims in one process: the second fails, after dropping the first it works).
- [ ] **Step 2:** run → FAIL. **Step 3:** implement. **Step 4:** PASS.

### Task 2.3: the app crate `desktop/app` (Tauri 2, tray)

**Files:** Create `desktop/app/{Cargo.toml, build.rs, tauri.conf.json, icons/,
ui/settings.html, src/main.rs, src/tray.rs, src/autostart.rs}`; add `app` to the workspace.
**Interfaces:**
- `tray::State` (`Starting`, `Idle`, `Recording`, `Working`, `Paused`, `Problem`); `tray::apply(&mut Status,
  &Event) -> bool` (pure: the new state, tooltip and last problem from one dictation event; true = redraw);
  `tray::icon(State) -> Vec<u8>` (RGBA, `ICON_SIZE` square drawn in code: the Voice Ring as a ring in `--accent`, a
  filled dot while recording, grey while paused, a `--danger` dot on a problem).
- `TraySink` (`events::Sink`): writes M1's console lines to stderr (seen in a debug build; the release build
  has no console), updates the tray icon and tooltip. Never puts dictated text in the tooltip.
- Menu: status line (disabled), Pause / Resume dictation, Settings… (placeholder window `ui/settings.html`
  using `design/tokens.css`, copied in by `build.rs`; the real page is Phase 4), Open history folder, Start
  with Windows (check item), Quit.
- `autostart::{enabled, set}`: HKCU `...\CurrentVersion\Run` value `SonicScript` = `"<exe>"`. Left off by
  default in this phase: a dev build must not register `target\...` at sign-in; Phase 7's installer turns it on.
- Single instance: `tauri-plugin-single-instance`; a second start opens the first one's Settings window.
- Dictation: `dictate::run` on a thread with the settings and keys exactly as the CLI's dictation uses them;
  when it ends with an error (no key, a second copy) the tray shows Problem with the reason.
- [ ] **Step 1: Failing tests:** `tray::apply` for Listening → Recording, Processing → Working, Text pasted →
  Idle (tooltip without the text), Text copied → Idle + "press Ctrl+V" note, Failed → Problem with its first
  line, Warning → Problem; `icon` sizes and the paused icon has no accent pixel; `autostart::command_line`.
- [ ] **Step 2:** run → FAIL. **Step 3:** implement. **Step 4:** PASS; `cargo build -p sonicscript-app`.

### Task 2.4: the real check, docs, PR
- [ ] Run the app: tray icon shows; menu items work; a dictation by the key pastes through `TraySink`;
  pause → key does nothing and Right Ctrl + C still copies; quit → hook gone; start twice → one copy, the
  first opens Settings; the CLI while the app runs → refused ("already running"). CPU idle and RAM noted.
- [ ] Docs: ARCHITECTURE (lib + app crate), STATUS, CHANGELOG, plan Progress; PR with base `feat/m2-settings`.

## Phase 3: the HUD Voice Ring (T1, `feat/m2-hud`)

- A transparent, topmost, no-activate, click-through 44 px pill window, bottom-centre of the monitor with the
  window in front (12% from the bottom). States from DESIGN.md: Listening (ring follows `Level`, attack ~40 ms,
  release ~180 ms, Geist Mono timer), Processing (one shimmer, "Writing…"), Inserted (600 ms tick), Copied
  ("Copied: press Ctrl+V", 2.5 s), Error / limit (dot + one sentence + the action). Acrylic via Tauri
  `window-vibrancy`, solid `--surface-2` when transparency is off. States also announced (live region).
- The chimes stay (setting). Fonts (Geist, Geist Mono) bundled in `ui/fonts/`.
- **Check:** every state shown from a real dictation (and the error ones from a wrong key / `resting.json`);
  paste lands in Notepad, Chrome, Windows Terminal with the HUD up (focus never moves: checked with
  `GetForegroundWindow` before and after); reduced motion; two monitors.

Detailed tasks (written 2026-11-06 when the phase started; C: 35 GB free).

### Task 3.1: what the HUD shows, pure (`desktop/app/src/hud.rs`)

**Interfaces:** `hud::Kind` (`Listening`, `Writing`, `Inserted`, `Copied`, `Problem`), `hud::Tone` (`Accent`,
`Ok`, `Warn`, `Danger`), `hud::View { kind, tone, label: String, hide_after_ms: Option<u64> }` (serialised to
the web view), `hud::Hud::apply(&mut self, &Event) -> Change` with `Change::{None, Show(View), Hide}`.
- Listening → Listening (accent, no timeout); Processing → Writing "Writing…"; Text pasted → Inserted
  (ok, 600 ms); copied → Copied "Copied: press Ctrl+V" (2.5 s); paste failed → Problem (danger, 5 s);
  NoSpeech / HintOnly → Problem (warn, "No speech heard" / "Nothing to type", 1.8 s); Failed → Problem
  (danger, the first line, 6 s); Cancelled → Hide; a Notice starting "! " (limit notes) → Problem (warn, its
  first sentence, 4 s) unless recording; Ready / Warning / other Notices / CapReached → None (the tray shows them).
- A job's end while the next recording runs (a slow answer) never replaces Listening (as the tray, D-047).
- Never puts dictated text in a view (the label is ours).
- [ ] Failing tests: `a_dictation_shows_listening_writing_inserted`, `copied_and_failed_paste`,
  `failures_show_their_first_line_not_the_text`, `cancel_hides`, `slow_answer_keeps_listening`,
  `limit_notice_warns`, `startup_notices_do_not_show`. → FAIL → implement → PASS.

### Task 3.2: the HUD window (`desktop/app/src/hudwin.rs`, `ui/hud.{html,css,js}`, fonts)

- One window `hud`, built hidden at start (shown instantly later): transparent, no decorations, always on
  top, skip taskbar, not focusable, click-through (`set_ignore_cursor_events`); after build its style gets
  `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST`. Shown with `ShowWindow(SW_SHOWNOACTIVATE)` and
  placed with `SetWindowPos(.., SWP_NOACTIVATE)`: bottom-centre of the work area of the monitor holding the
  foreground window, its bottom 12% up; size in that monitor's DPI.
- The window is wider than the widest pill (420 x 72 logical, transparent): the pill's width animates in
  CSS with no window resize. Glass: DESIGN.md's solid `--surface-2` form (see D-048).
- `ui/hud.js`: listens for `hud` / `hud-hide` events (`withGlobalTauri`), polls the `level` command each
  frame while listening (smoothing: attack 40 ms, release 180 ms), runs the timer (Geist Mono), plays one
  shimmer while writing, the tick on inserted; the label in an `aria-live="polite"` region;
  `prefers-reduced-motion`: static ring whose opacity follows the level, no shimmer.
- Geist + Geist Mono (OFL) bundled as woff2 in `ui/fonts/` with their licence.
- `HudSink` = the tray sink + the HUD: hide timers carry a generation number, so an old timer never hides a
  newer view.
- `--hud-demo` (debug aid): runs every state with made-up events and prints `GetForegroundWindow` before and
  after each show (the focus check without a real key press).

### Task 3.3: the real check, docs, PR
- [ ] `--hud-demo` over Notepad: the foreground window is the same before and after every state; screenshots of
  each state (dark; light; reduced motion). A real dictation through the tray (owner: real key).
- [ ] fmt, clippy `-D warnings`, tests; docs (STATUS, CHANGELOG, ARCHITECTURE, D-048); PR; Auto-fix on.

## Phase 4: the settings window (T2: stores keys; `feat/m2-settings-ui`)

DESIGN.md "Settings window", 760 x 540, sidebar → tabs under 560 px, light and dark:
- **General:** dictation key (key-cap + "Change", press a key; D-037's refusals shown), smart formatting switch
  (off, with a two-line explanation, D-041), sounds, start with Windows, pop-up mic on / off.
- **Models & keys:** key fields (show / hide, last 4 only, "Test key" = one tiny request, link "Get a free
  key"), main model + 2 fallbacks chosen from `spec/models.json` with its numbers and free limits (D-027).
- **Words:** the list (edit, add), "Copy prompt for my AI assistant", "Paste words" (the `mywords` commands).
- **History:** last 50 (text, time, app; copy / re-insert / play / delete), off switch, keep N days.
- **About:** version, privacy summary (blueprint §5.3, free-tier Gemini data use), links.
- Saving writes `config.toml` / Credential Manager and the running dictation loop picks it up without a
  restart (a reload message to the dictation thread).
- **Check:** each control changes the real behaviour (a dictation after each change); a wrong key shows the
  plain error; keyboard-only use; contrast 4.5:1.

## Phase 5: the pop-up mic on text fields (T1, `feat/m2-popup-mic`, D-016)

- UI Automation focus-changed events on a thread; editable = Edit control type, or Value pattern not read-only,
  or Text pattern; never `IsPassword`. A small mic pill (the ring, not recording) near the field's bounding
  rectangle (below-right, kept on screen); a click or the dictation key starts recording; hides when focus
  leaves. Off for terminals by default (window classes from `inject`) and per app (`popup_off_apps`, a
  "Not in this app" item on the pill).
- **Check:** shows in Notepad, Chrome text box, VS Code editor, Claude app input, WhatsApp desktop; not on a
  password field (Chrome sign-in page), not in Windows Terminal; never steals focus; CPU while switching
  windows noted.

## Phase 6: reliability (T1, `feat/m2-reliability`)

- **Learned deadline (D-017):** per engine, time per second of audio from its last 50 good answers (kept next
  to `resting.json`); the next engine starts at max(3 s, 2x expected) instead of the fixed wait; the spec's
  `wait_ms` until 10 answers exist. Unit tests on the estimator; then a chain bench run (Next 5's `m1-chain3`
  doubles as this check).
- **Retries (blueprint §7):** when nothing answered because the network is down, the HUD says so and a "Try
  again" (tray item and HUD click) sends the kept recording again; the history line is updated.
- **Clips over 2 minutes:** split at the longest pause near each 90 s mark, sent in order, joined.
- **Check:** unit tests; network off → message, network on → Try again types the text; a 3-minute clip comes
  back whole.

## Phase 7: release and first run (T2, `feat/m2-release`)

- Tauri NSIS installer (per-user, no admin), app icon (the Voice Ring), uninstaller removes the `Run` value
  (asks before deleting history and keys).
- First run: a short window: "Bolo, hum likh denge." → paste a Gemini key (link to Google AI Studio), optional
  Groq key, "Test", then "Tap Right Ctrl and speak" with a live try box.
- CI: on a `v*` tag, build the installer on `windows-latest` and attach it to a draft GitHub Release (the owner
  publishes it).
- **Done when (blueprint §8):** a non-technical friend installs it from the release and dictates within 2
  minutes (timed, the owner watching). Before that: a clean Windows user account on the owner's PC as the
  dry run.
- **Rollback:** delete the release; the installer's uninstaller.

## Out of scope (M2)

Output modes other than Roman (Decision 2), streaming preview (M7), code signing and winget (M4), Android (M3),
Linux, auto-update, a "hands-free" mode, telemetry, starter word packs (D-034 says M2 settings box; packs later).

## Risks

- Disk: Tauri build space on a nearly full C: (Decision 1). Mitigation: free space first; `cargo clean -p` of
  old profiles; build release only for the app.
- WebView2 RAM and start-up: the HUD window is created once at start and kept hidden, so showing it is instant.
- Antivirus / SmartScreen on an unsigned keyboard-hook app (blueprint §11): the friend test may need "Run
  anyway"; signing in M4.
- UI Automation is slow or noisy in some apps (Electron trees): the pop-up mic has a per-app off switch and a
  global one.
- Two processes hooking keys (CLI dev build + installed app): single-instance mutex shared by both.

## Rollback

Each phase is its own PR, revertable alone. The CLI (`sonicscript.exe`) keeps working throughout, so the owner
can always dictate the M1 way. Stored data: `config.toml` (old files still load), Credential Manager entries
`SonicScript/*` (`--key <p> --delete`), the HKCU `Run` value.

## Assumptions

- The friend has Windows 10 or 11 with WebView2 (preinstalled on both since 2021) and can make a free Gemini
  key in a browser.
- The default chain and its waits stay as D-029 / D-040 set them; the settings only reorder within the catalog.
