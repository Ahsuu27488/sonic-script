//! The user's settings in `%APPDATA%\SonicScript\config.toml` (D-045, superseding D-037's one-line file): the
//! dictation key, the main model and up to two fallbacks (D-029), smart formatting (D-041), sounds, history
//! (D-032, D-044: kept 30 days) and the pop-up mic (D-016). Every field has a default, so M1's file
//! (`hotkey = "F9"` and nothing else) still loads. Model ids are spec ids (`spec/providers.json`), never copies.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// At most this many fallbacks after the main model (blueprint §4.2).
pub const MAX_FALLBACKS: usize = 2;

/// The fields of `Settings`, as written in the file.
const KNOWN: [&str; 9] = [
    "hotkey",
    "main_model",
    "fallbacks",
    "format",
    "sound",
    "history",
    "history_days",
    "popup_mic",
    "popup_off_apps",
];

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
        match Self::read(path) {
            Ok(loaded) => loaded,
            Err(why) => (
                Settings::default(),
                vec![format!(
                    "{} could not be read ({why}): using the default settings.",
                    path.display()
                )],
            ),
        }
    }

    /// Like `load`, but a file that is there and cannot be read is an error (so nothing overwrites it).
    pub fn read(path: &Path) -> Result<(Settings, Vec<String>), String> {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Ok((Settings::default(), Vec::new()));
        };
        let table: toml::Table = toml::from_str(&text).map_err(|e| e.message().to_string())?;
        let mut notes: Vec<String> = table
            .keys()
            .filter(|k| !KNOWN.contains(&k.as_str()))
            .map(|k| {
                format!(
                    "{}: `{k}` is not a setting this version knows (ignored, kept in the file).",
                    path.display()
                )
            })
            .collect();
        let mut s: Settings = table
            .try_into()
            .map_err(|e: toml::de::Error| e.message().to_string())?;
        if s.fallbacks.len() > MAX_FALLBACKS {
            notes.push(format!(
                "{}: only {MAX_FALLBACKS} fallbacks are used ({} kept).",
                path.display(),
                s.fallbacks[..MAX_FALLBACKS].join(", ")
            ));
            s.fallbacks.truncate(MAX_FALLBACKS);
        }
        Ok((s, notes))
    }

    /// Writes the settings (a header comment, then the values) through a temporary file, so a crash never
    /// leaves half a file. Settings this version does not know (a newer version's, a typo) are kept. A file
    /// that is there and cannot be read is never overwritten: the user's other values would be lost.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut table = toml::Table::try_from(self).map_err(io::Error::other)?;
        if let Ok(text) = std::fs::read_to_string(path) {
            let old: toml::Table = toml::from_str(&text).map_err(|e| {
                io::Error::other(format!(
                    "it could not be read ({}); fix or delete it first, nothing was changed",
                    e.message()
                ))
            })?;
            for (k, v) in old {
                if !KNOWN.contains(&k.as_str()) {
                    table.insert(k, v);
                }
            }
        }
        let body = toml::to_string(&table).map_err(io::Error::other)?;
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
    fn saving_over_a_broken_file_refuses_and_keeps_it() {
        let p = scratch("keep-broken");
        let text = "hotkey = F9\nmain_model = \"gemini-3.5-flash\"\n";
        std::fs::write(&p, text).unwrap();
        let err = Settings::default().save(&p).unwrap_err().to_string();
        assert!(err.contains("nothing was changed"), "{err}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), text);
        assert!(Settings::read(&p).is_err());
    }

    #[test]
    fn unknown_settings_are_reported_and_kept() {
        let p = scratch("unknown");
        std::fs::write(&p, "main-model = \"x\"\nformat = true\n").unwrap();
        let (s, notes) = Settings::load(&p);
        assert!(s.format);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("`main-model`"), "{notes:?}");
        Settings { sound: false, ..s }.save(&p).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(
            text.contains("main-model = \"x\"") && text.contains("sound = false"),
            "{text}"
        );
    }

    #[test]
    fn fallbacks_capped_at_two() {
        let (s, notes) = load_text("cap", "fallbacks = [\"a\", \"b\", \"c\"]\n");
        assert_eq!(s.fallbacks, ["a", "b"]);
        assert_eq!(notes.len(), 1, "{notes:?}");
    }
}
