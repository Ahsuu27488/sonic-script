# Auto-update for the tray app

**Tier:** T2 (a release path, a signing key, the repo going public). Branch `feat/auto-update`.

**Progress:**
- 2026-11-13: plan written. The owner's two choices (2026-11-11) are given, so building follows the plan's commit.
- 2026-11-13: built and checked end to end with signed test builds and a local server (STATUS "Auto-update"). Change
  from the plan: an install that waited for dictation runs after 3 s of idle, not on the next event (the text's
  event comes before the history line is written). Left for the owner: key backup, the public flip, the secrets,
  the first tagged releases.

## Goal

An installed SonicScript (a friend's PC) gets new versions like a normal Windows app: it finds them, downloads
them quietly, says "Update ready: Restart to update", installs on one click, or at the next start if nobody
clicked. Settings, keys, history and "Start with Windows" are kept. A recording is never interrupted.

## The owner's decisions (2026-11-11), recorded as D-053

1. Updates come from the repo's own GitHub Releases, so `Ahsuu27488/sonic-script` becomes **public now**.
   Supersedes D-001's "private for now, public at launch" and the M2 plan's "auto-update out of scope".
   The owner flips it to public himself after reviewing the history scan (or tells the agent to).
2. "Ask, then update": check at start and every 6 hours; download in the background; tray menu + a small HUD
   notice say "Update ready"; one click installs and restarts; not clicked: installed at the next app start.

## How

- `tauri-plugin-updater` 2.14 (Rust side only: the web pages call our own commands, so no capability file).
  `tauri.conf.json`: `bundle.createUpdaterArtifacts: true`, `plugins.updater` with the public key, the endpoint
  `https://github.com/Ahsuu27488/sonic-script/releases/latest/download/latest.json`,
  `requireSignedVersion: true` (the signature must name the version, so an old signed installer cannot be
  passed off as a new one), `windows.installMode: passive` (a progress bar, no questions).
- The plugin runs the NSIS installer with `/P /UPDATE /R`: in update mode Tauri's template skips the old
  uninstaller (no `PREUNINSTALL` / `POSTUNINSTALL` hooks), keeps the Start menu shortcut and the HKCU `Run`
  value, and `/R` starts the new app. `installer.nsh` already leaves data and the Run value alone in that path
  (no Apps entry missing = not a first install; no note = Run value untouched). Confirmed in the end-to-end check.
- New `desktop/app/src/update.rs`:
  - pure part (tested): `Phase` (`Idle`, `Checking`, `Downloading(v)`, `Ready(v)`, `UpToDate`, `Failed(why)`)
    → tray menu label and the Settings line; `busy(tray state)` (Recording / Working never get interrupted).
  - the cache `%LOCALAPPDATA%\SonicScript\update\` (`release.json` = the server's JSON, `setup.exe` = the
    verified download). At start: `restore_update(json)` + `install(bytes)` (the plugin verifies the bytes
    against the signature and the version again); not newer any more, or a bad file: the cache is deleted.
  - a thread: check 15 s after start, then every 6 hours; a manual check on request. An update found is
    downloaded (verified by the plugin), saved to the cache, then shown as ready.
  - `install(app)`: a thread waits until dictation is quiet (no recording, no clip in flight, no event for
    3 s), then `Update::install` (the plugin starts the installer and exits the app).
- Tray: one menu item "Check for updates" whose label follows the phase ("Checking for updates…",
  "Downloading update 0.2.1…", "Restart to update (0.2.1)"); the tooltip gets "Update ready" while ready.
- HUD: once per ready version, when idle, a pill "Update 0.2.1 ready" with a "Restart to update" button
  (the same click path as "Try again", D-051), 10 s. Results of a manual check ("Up to date", "Could not
  check") also show briefly.
- Settings → About: the version, the update line and "Check for updates" / "Restart to update".
- Release workflow: the signing key and password come from GitHub secrets
  (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`); `cargo tauri build` then writes
  `SonicScript_<v>_x64-setup.exe.sig`; a step writes `latest.json` (version, notes, date, `windows-x86_64`
  signature + the release download URL) and uploads it with the installer to the **draft** release. The owner
  publishes; only a published release is "latest", so friends never see a draft.
- Signing key: `cargo tauri signer generate` into `%USERPROFILE%\.sonicscript-keys\` (outside the repo,
  random password in a file next to it), never printed, never committed. The public key goes in
  `tauri.conf.json`. GitHub secrets only after the owner says yes.

## Out of scope

Code signing of the installer (M4), release notes UI, a beta channel, delta updates, Android.

## The check

1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` green; new unit tests for the
   phase labels, `busy`, and the cache (save / load / clear).
2. End to end on this PC, with the owner's data kept: build 0.2.0 with a test config (endpoint
   `http://127.0.0.1:<port>/latest.json`, `dangerousInsecureTransportProtocol`), install it; build 0.2.1 with
   the same key, write its `latest.json`, serve it with a local HTTP server; the installed app finds it,
   downloads it, the tray shows "Restart to update (0.2.1)"; a click installs and the app comes back as 0.2.1;
   `config.toml`, the keys (sources only, never values), the history line count and the `Run` value are the
   same before and after.
3. A tampered installer (one byte changed) and an unsigned one (signature from another key) are refused: no
   install, the cache is not kept, the Settings line says why.
4. Not interrupting: an install requested while the tray says Recording / Working waits for the end (unit test
   of `Hud::busy` with two clips in flight, and the quiet wait).

## Risks and rollback

- **Lost key** = no more updates for installed copies (they only trust this public key). The owner backs up
  `%USERPROFILE%\.sonicscript-keys\` (key + password) somewhere safe. A new key needs a manual reinstall.
- **Leaked key** lets someone sign a fake update, but they also need to publish it on this repo's releases.
- **A bad release** reaches everyone: the owner publishes only after trying the draft; rollback = publish a
  fixed higher version (downgrades are off).
- **Repo public:** anything in git history becomes public. The scan (secrets + personal data) comes first and
  the owner decides.
- Rollback of the code: revert the PR; installed 0.2.0 copies without the updater are unaffected.
