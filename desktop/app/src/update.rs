//! Auto-update (D-053): the app checks GitHub Releases at start and every 6 hours, downloads a new version
//! quietly, then says "Update ready: Restart to update" (tray menu, a HUD pill, Settings → About). One click
//! installs it and restarts; when nobody clicks, it is installed at the next start. A recording or an answer on
//! its way is never cut: an install asked for meanwhile waits for it to end.
//!
//! `tauri-plugin-updater` does the network, the signature check (the public key in `tauri.conf.json`, and the
//! signature must name the version) and the installer run (`/P /UPDATE /R`: Tauri's NSIS template then keeps
//! the user's files, shortcut and "Start with Windows"). The verified download is kept in
//! `%LOCALAPPDATA%\SonicScript\update\` for the next start, and checked again before it is installed.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::Tray;

/// The first check waits for the app to settle (dictation ready, the network up after sign-in).
const FIRST_CHECK: Duration = Duration::from_secs(15);
/// Then every 6 hours.
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// The cache's file names: the release's JSON is written last, so its presence means a complete download.
const CACHE_SETUP: &str = "setup.exe";
const CACHE_RELEASE: &str = "release.json";
/// How long dictation must stay idle before an install that waited for it runs.
const SETTLE: Duration = Duration::from_secs(3);

/// Where the updater is; the tray menu, the HUD and Settings → About show it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "phase", content = "version", rename_all = "kebab-case")]
pub enum Phase {
    Idle,
    Checking,
    Downloading(String),
    /// Downloaded and verified: one click installs it.
    Ready(String),
    /// The user asked to install while dictation was busy: it installs when that ends.
    Waiting(String),
    UpToDate,
    Failed(String),
}

impl Phase {
    /// The tray menu item's text.
    pub fn menu_label(&self) -> String {
        match self {
            Phase::Checking => "Checking for updates…".into(),
            Phase::Downloading(v) => format!("Downloading update {v}…"),
            Phase::Ready(v) => format!("Restart to update ({v})"),
            Phase::Waiting(v) => format!("Updating to {v} after this dictation…"),
            Phase::Idle | Phase::UpToDate | Phase::Failed(_) => "Check for updates".into(),
        }
    }

    /// Whether the menu item can be clicked (not while a check or a download runs).
    pub fn menu_enabled(&self) -> bool {
        !matches!(
            self,
            Phase::Checking | Phase::Downloading(_) | Phase::Waiting(_)
        )
    }

    /// The line in Settings → About.
    pub fn line(&self, current: &str) -> String {
        match self {
            Phase::Idle => {
                format!("Version {current}. Updates are checked at start and every 6 hours.")
            }
            Phase::Checking => "Checking for updates…".into(),
            Phase::Downloading(v) => format!("Downloading version {v}…"),
            Phase::Ready(v) => format!(
                "Version {v} is ready. Restart to update (or it installs at the next start)."
            ),
            Phase::Waiting(v) => format!("Updating to {v} when this dictation ends…"),
            Phase::UpToDate => format!("Version {current} is the newest."),
            Phase::Failed(why) => format!("Could not update: {why}"),
        }
    }

    pub fn ready_version(&self) -> Option<&str> {
        match self {
            Phase::Ready(v) | Phase::Waiting(v) => Some(v),
            _ => None,
        }
    }
}

/// The verified download kept for the next start.
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `%LOCALAPPDATA%\SonicScript\update`.
    pub fn default_dir() -> Option<PathBuf> {
        Some(
            PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
                .join("SonicScript")
                .join("update"),
        )
    }

    pub fn save(&self, release: &serde_json::Value, setup: &[u8]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        // An older, half-written cache must never pair with this setup.
        let _ = std::fs::remove_file(self.dir.join(CACHE_RELEASE));
        std::fs::write(self.dir.join(CACHE_SETUP), setup)?;
        std::fs::write(
            self.dir.join(CACHE_RELEASE),
            serde_json::to_vec(release).map_err(std::io::Error::other)?,
        )
    }

    /// The release JSON and the installer, when both are there.
    pub fn load(&self) -> Option<(serde_json::Value, Vec<u8>)> {
        let release = std::fs::read(self.dir.join(CACHE_RELEASE)).ok()?;
        let release = serde_json::from_slice(&release).ok()?;
        let setup = std::fs::read(self.dir.join(CACHE_SETUP)).ok()?;
        Some((release, setup))
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(self.dir.join(CACHE_RELEASE));
        let _ = std::fs::remove_file(self.dir.join(CACHE_SETUP));
        let _ = std::fs::remove_dir(&self.dir);
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// The updater's state in the running app.
pub struct Updates {
    phase: Mutex<Phase>,
    /// The verified update and its installer, once downloaded.
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    /// An install was asked for while dictation was busy.
    waiting: AtomicBool,
    /// A thread is waiting for dictation to stay idle (`install_if_waiting`).
    settling: AtomicBool,
    /// The version whose "Update ready" pill was shown (once per version, unless asked).
    noticed: Mutex<Option<String>>,
    /// Asks the checking thread for a check now (true = the user asked: show the result).
    ask: Mutex<Sender<bool>>,
    cache: Option<Cache>,
}

/// What Settings → About gets.
#[derive(Serialize)]
pub struct View {
    version: String,
    phase: Phase,
    line: String,
    can_install: bool,
}

pub fn view(app: &AppHandle) -> View {
    let phase = current(app);
    let version = app_version(app);
    View {
        line: phase.line(&version),
        version,
        can_install: matches!(phase, Phase::Ready(_)),
        phase,
    }
}

/// The version the updater compares (`tauri.conf.json`'s, the installer's).
pub fn app_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// The phase now (`Idle` in the previews, which run no updater).
pub fn current(app: &AppHandle) -> Phase {
    app.try_state::<Updates>()
        .and_then(|u| u.phase.lock().ok().map(|p| p.clone()))
        .unwrap_or(Phase::Idle)
}

/// Starts the updater: installs an update downloaded in an earlier run (this exits the app and the installer
/// starts the new one), then checks on a thread of its own.
pub fn start(app: &AppHandle) {
    let (tx, rx) = mpsc::channel::<bool>();
    app.manage(Updates {
        phase: Mutex::new(Phase::Idle),
        ready: Mutex::new(None),
        waiting: AtomicBool::new(false),
        settling: AtomicBool::new(false),
        noticed: Mutex::new(None),
        ask: Mutex::new(tx),
        cache: Cache::default_dir().map(Cache::new),
    });
    install_cached(app);
    let app = app.clone();
    std::thread::spawn(move || {
        let mut manual = match rx.recv_timeout(FIRST_CHECK) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        loop {
            run_check(&app, manual);
            manual = match rx.recv_timeout(EVERY) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => false,
                Err(RecvTimeoutError::Disconnected) => return,
            };
        }
    });
}

/// "Check for updates" (tray menu, Settings).
pub fn check_now(app: &AppHandle) {
    if let Some(u) = app.try_state::<Updates>()
        && let Ok(tx) = u.ask.lock()
    {
        let _ = tx.send(true);
    }
}

/// An update downloaded in an earlier run that nobody installed: install it now, at start, before dictation
/// runs. Not newer any more (it was installed) or not valid: the cache is deleted.
fn install_cached(app: &AppHandle) {
    let Some(u) = app.try_state::<Updates>() else {
        return;
    };
    let Some(cache) = &u.cache else {
        return;
    };
    let Some((release, setup)) = cache.load() else {
        return;
    };
    let restored = app
        .updater()
        .map_err(|e| e.to_string())
        .and_then(|up| up.restore_update(release).map_err(|e| e.to_string()));
    match restored {
        Ok(Some(update)) => {
            eprintln!("update: installing {} downloaded earlier", update.version);
            // Verifies the bytes first; on success the app exits here.
            if let Err(e) = update.install(&setup) {
                eprintln!("update: the saved update was refused: {e}");
                cache.clear();
            }
        }
        Ok(None) => cache.clear(),
        Err(e) => {
            eprintln!("update: the saved update is not usable: {e}");
            cache.clear();
        }
    }
}

fn set_phase(app: &AppHandle, phase: Phase) {
    if let Some(u) = app.try_state::<Updates>()
        && let Ok(mut p) = u.phase.lock()
    {
        *p = phase;
    }
    crate::redraw(app);
}

fn run_check(app: &AppHandle, manual: bool) {
    let Some(u) = app.try_state::<Updates>() else {
        return;
    };
    // One is ready: the automatic checks rest until it is installed; a manual one shows it again.
    if let Some(v) = current(app).ready_version().map(str::to_string) {
        if manual {
            notice_ready(app, &v, true);
        }
        return;
    }
    set_phase(app, Phase::Checking);
    let found = app
        .updater()
        .map_err(|e| e.to_string())
        .and_then(|up| tauri::async_runtime::block_on(up.check()).map_err(|e| e.to_string()));
    let update = match found {
        Ok(Some(update)) => update,
        Ok(None) => {
            set_phase(app, Phase::UpToDate);
            if manual {
                crate::hudwin::notice(
                    app,
                    crate::hud::note(&format!("SonicScript {} is up to date", app_version(app))),
                );
            }
            return;
        }
        Err(e) => {
            eprintln!("update: check failed: {e}");
            set_phase(app, Phase::Failed(short(&e)));
            if manual {
                crate::hudwin::notice(app, crate::hud::note("Could not check for updates"));
            }
            return;
        }
    };
    let version = update.version.clone();
    set_phase(app, Phase::Downloading(version.clone()));
    // `download` verifies the signature (and the version in it) before it returns the bytes.
    match tauri::async_runtime::block_on(update.download(|_, _| {}, || {})) {
        Ok(setup) => {
            if let Some(cache) = &u.cache
                && let Err(e) = cache.save(&update.raw_json, &setup)
            {
                // Still installable from memory; only the next-start install is lost.
                eprintln!(
                    "update: could not keep the download in {:?}: {e}",
                    cache.dir()
                );
            }
            if let Ok(mut r) = u.ready.lock() {
                *r = Some((update, setup));
            }
            set_phase(app, Phase::Ready(version.clone()));
            notice_ready(app, &version, manual);
        }
        Err(e) => {
            eprintln!("update: download of {version} refused: {e}");
            set_phase(app, Phase::Failed(short(&e.to_string())));
            if manual {
                crate::hudwin::notice(app, crate::hud::note("The update could not be downloaded"));
            }
        }
    }
}

/// The "Update ready" pill, once per version (again when the user asked).
fn notice_ready(app: &AppHandle, version: &str, again: bool) {
    let Some(u) = app.try_state::<Updates>() else {
        return;
    };
    if let Ok(mut n) = u.noticed.lock() {
        if !again && n.as_deref() == Some(version) {
            return;
        }
        *n = Some(version.to_string());
    }
    crate::hudwin::notice(app, crate::hud::update_ready(version));
}

/// "Restart to update": installs now, or when the running dictation ends.
pub fn install(app: &AppHandle) -> Result<(), String> {
    let Some(u) = app.try_state::<Updates>() else {
        return Err("Updates are off in this window.".into());
    };
    let Some(version) = current(app).ready_version().map(str::to_string) else {
        return Err("No update is ready.".into());
    };
    if dictation_busy(app) {
        u.waiting.store(true, Ordering::SeqCst);
        set_phase(app, Phase::Waiting(version));
        return Ok(());
    }
    install_ready(app)
}

/// After each dictation event: an install that waited runs once dictation has been idle for `SETTLE`. Not at
/// once: dictation sends the text's event before it writes the history line (`dictate::transcribe`), and the
/// install ends the app.
pub fn install_if_waiting(app: &AppHandle) {
    let Some(u) = app.try_state::<Updates>() else {
        return;
    };
    if !u.waiting.load(Ordering::SeqCst)
        || dictation_busy(app)
        || u.settling.swap(true, Ordering::SeqCst)
    {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SETTLE);
        let Some(u) = app.try_state::<Updates>() else {
            return;
        };
        u.settling.store(false, Ordering::SeqCst);
        // A new recording started meanwhile: the next event after it tries again.
        if dictation_busy(&app) || !u.waiting.swap(false, Ordering::SeqCst) {
            return;
        }
        if let Err(e) = install_ready(&app) {
            eprintln!("update: {e}");
        }
    });
}

/// A recording runs or its text is on its way.
fn dictation_busy(app: &AppHandle) -> bool {
    app.try_state::<Tray>()
        .and_then(|t| t.status.lock().ok().map(|s| s.busy()))
        .unwrap_or(false)
}

fn install_ready(app: &AppHandle) -> Result<(), String> {
    let Some(u) = app.try_state::<Updates>() else {
        return Err("Updates are off in this window.".into());
    };
    let result = match u.ready.lock() {
        // Verifies the bytes again, starts the installer and exits the app (it never returns on success).
        Ok(r) => match r.as_ref() {
            Some((update, setup)) => update.install(setup).map_err(|e| e.to_string()),
            None => Err("No update is ready.".into()),
        },
        Err(_) => Err("No update is ready.".into()),
    };
    if let Err(e) = &result {
        if let Ok(mut r) = u.ready.lock() {
            *r = None;
        }
        if let Some(cache) = &u.cache {
            cache.clear();
        }
        set_phase(app, Phase::Failed(short(e)));
    }
    result
}

/// Settings → About: the version and the update line.
#[tauri::command]
pub fn update_state(app: AppHandle) -> View {
    view(&app)
}

/// Settings → About: "Check for updates".
#[tauri::command]
pub fn update_check(app: AppHandle) {
    check_now(&app);
}

/// Settings → About and the HUD pill: "Restart to update".
#[tauri::command]
pub fn update_install(app: AppHandle) -> Result<(), String> {
    install(&app)
}

/// The first line of an error, cut for the tray and the About page.
fn short(e: &str) -> String {
    let line = e.lines().next().unwrap_or("").trim();
    if line.chars().count() > 90 {
        line.chars().take(89).collect::<String>() + "…"
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_item_follows_the_phase() {
        assert_eq!(Phase::Idle.menu_label(), "Check for updates");
        assert_eq!(
            Phase::Ready("0.2.1".into()).menu_label(),
            "Restart to update (0.2.1)"
        );
        assert!(Phase::Ready("0.2.1".into()).menu_enabled());
        assert!(!Phase::Checking.menu_enabled());
        assert!(!Phase::Downloading("0.2.1".into()).menu_enabled());
        assert!(!Phase::Waiting("0.2.1".into()).menu_enabled());
        assert!(Phase::Failed("x".into()).menu_enabled());
    }

    #[test]
    fn only_ready_and_waiting_have_a_version_to_install() {
        assert_eq!(Phase::Ready("1.0.0".into()).ready_version(), Some("1.0.0"));
        assert_eq!(
            Phase::Waiting("1.0.0".into()).ready_version(),
            Some("1.0.0")
        );
        assert_eq!(Phase::Downloading("1.0.0".into()).ready_version(), None);
        assert_eq!(Phase::UpToDate.ready_version(), None);
    }

    #[test]
    fn the_about_line_names_the_versions() {
        assert!(Phase::UpToDate.line("0.2.0").contains("0.2.0"));
        assert!(Phase::Ready("0.2.1".into()).line("0.2.0").contains("0.2.1"));
        assert!(
            Phase::Failed("bad signature".into())
                .line("0.2.0")
                .contains("bad signature")
        );
    }

    #[test]
    fn the_phase_reaches_the_page_as_tagged_json() {
        let j = serde_json::to_string(&Phase::Ready("0.2.1".into())).unwrap();
        assert_eq!(j, r#"{"phase":"ready","version":"0.2.1"}"#);
        let j = serde_json::to_string(&Phase::UpToDate).unwrap();
        assert_eq!(j, r#"{"phase":"up-to-date"}"#);
    }

    #[test]
    fn the_cache_keeps_a_complete_download_and_clears() {
        let dir = std::env::temp_dir().join(format!("sonic-update-test-{}", std::process::id()));
        let cache = Cache::new(dir.join("update"));
        assert!(cache.load().is_none());
        let release = serde_json::json!({"version": "0.2.1"});
        cache.save(&release, b"MZ installer").unwrap();
        let (r, s) = cache.load().unwrap();
        assert_eq!(r, release);
        assert_eq!(s, b"MZ installer");
        // A setup without its release JSON (a write cut short) is not a download.
        std::fs::remove_file(cache.dir().join(CACHE_RELEASE)).unwrap();
        assert!(cache.load().is_none());
        cache.save(&release, b"MZ").unwrap();
        cache.clear();
        assert!(cache.load().is_none());
        assert!(!cache.dir().exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn errors_are_cut_to_one_short_line() {
        assert_eq!(short("first\nsecond"), "first");
        assert_eq!(short(&"x".repeat(200)).chars().count(), 90);
    }
}
