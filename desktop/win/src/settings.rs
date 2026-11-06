//! The user's settings in `%APPDATA%\SonicScript\config.toml` (D-045, superseding D-037's one-line file): the
//! dictation key, the main model and up to two fallbacks (D-029), smart formatting (D-041), sounds, history
//! (D-032, D-044: kept 30 days) and the pop-up mic (D-016). Every field has a default, so M1's file
//! (`hotkey = "F9"` and nothing else) still loads. Model ids are spec ids (`spec/providers.json`), never copies.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// At most this many fallbacks after the main model (blueprint §4.2).
pub const MAX_FALLBACKS: usize = 2;

const HEADER: &str = "# SonicScript settings (written by the app; you may edit it).\n\
                      # Change the dictation key with: sonicscript --set-key\n";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// A `setkey` name ("RightCtrl", "F9", "Mouse4", ...).
    pub hotkey: String,
    /// The engine asked first; `None` = the spec's chain.
    pub main_model: Option<String>,
    /// Engines asked after the main one, in order (at most `MAX_FALLBACKS`).
    pub fallbacks: Vec<String>,
    /// Smart formatting (D-030); off by default (D-041).
    pub format: bool,
    /// Start / stop chimes.
    pub sound: bool,
    /// Keep recordings and text on this PC (D-032).
    pub history: bool,
    /// Delete history older than this many days; 0 = keep forever (D-044).
    pub history_days: u32,
    /// The mic pill on focused text fields (D-016).
    pub popup_mic: bool,
    /// Programs (exe names) where the pop-up mic never shows.
    pub popup_off_apps: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            hotkey: "RightCtrl".into(),
            main_model: None,
            fallbacks: Vec::new(),
            format: false,
            sound: true,
            history: true,
            history_days: 30,
            popup_mic: true,
            popup_off_apps: Vec::new(),
        }
    }
}

impl Settings {
    /// The settings in `path`, and notices for the user (a broken file, too many fallbacks). A missing file
    /// gives the defaults without a notice; a broken one gives the defaults and says so.
    pub fn load(path: &Path) -> (Settings, Vec<String>) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return (Settings::default(), Vec::new());
        };
        let mut notes = Vec::new();
        let mut s: Settings = match toml::from_str(&text) {
            Ok(s) => s,
            Err(e) => {
                let why = e.message().to_string();
                notes.push(format!(
                    "{} could not be read ({why}): using the default settings.",
                    path.display()
                ));
                return (Settings::default(), notes);
            }
        };
        if s.fallbacks.len() > MAX_FALLBACKS {
            notes.push(format!(
                "{}: only {MAX_FALLBACKS} fallbacks are used ({} kept).",
                path.display(),
                s.fallbacks[..MAX_FALLBACKS].join(", ")
            ));
            s.fallbacks.truncate(MAX_FALLBACKS);
        }
        (s, notes)
    }

    /// Writes the settings (a header comment, then the values) through a temporary file, so a crash never
    /// leaves half a file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = toml::to_string(self).map_err(io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, format!("{HEADER}{body}"))?;
        std::fs::rename(&tmp, path)
    }
}

/// `%APPDATA%\SonicScript\config.toml`.
pub fn path() -> Result<PathBuf, String> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or("APPDATA is not set")?;
    Ok(base.join("SonicScript").join("config.toml"))
}

/// The settings from `path()`, defaults when there is no `APPDATA`.
pub fn load() -> (Settings, Vec<String>) {
    match path() {
        Ok(p) => Settings::load(&p),
        Err(_) => (Settings::default(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sonicscript-settings-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.toml")
    }

    fn load_text(name: &str, text: &str) -> (Settings, Vec<String>) {
        let p = scratch(name);
        std::fs::write(&p, text).unwrap();
        Settings::load(&p)
    }

    #[test]
    fn old_config_loads() {
        let (s, notes) = load_text("old", "# mine\nhotkey = \"F9\"\n");
        assert_eq!(
            s,
            Settings {
                hotkey: "F9".into(),
                ..Settings::default()
            }
        );
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn missing_file_gives_defaults() {
        let p = scratch("missing");
        let (s, notes) = Settings::load(&p);
        assert_eq!(s, Settings::default());
        assert!(notes.is_empty());
    }

    #[test]
    fn broken_file_gives_defaults_and_says_so() {
        let (s, notes) = load_text("broken", "hotkey = ");
        assert_eq!(s, Settings::default());
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("config.toml"), "{notes:?}");
    }

    #[test]
    fn round_trip() {
        let p = scratch("round");
        let s = Settings {
            hotkey: "Mouse4".into(),
            main_model: Some("gemini-3.5-flash-lite".into()),
            fallbacks: vec!["gemini-3.5-flash".into()],
            format: true,
            sound: false,
            history: false,
            history_days: 0,
            popup_mic: false,
            popup_off_apps: vec!["WindowsTerminal.exe".into()],
        };
        s.save(&p).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("# SonicScript settings"), "{text}");
        assert_eq!(Settings::load(&p), (s, Vec::new()));
    }

    #[test]
    fn fallbacks_capped_at_two() {
        let (s, notes) = load_text("cap", "fallbacks = [\"a\", \"b\", \"c\"]\n");
        assert_eq!(s.fallbacks, ["a", "b"]);
        assert_eq!(notes.len(), 1, "{notes:?}");
    }
}
