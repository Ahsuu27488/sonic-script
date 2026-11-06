//! The settings window's commands (M2 phase 4, DESIGN.md "Settings window"). Every change is saved at once
//! (`config.toml`, Credential Manager, `words.txt`) and the running dictation picks it up at its next idle
//! moment (`dictate::request_reload`), without a restart. Keys never come back to the page: only where a key
//! comes from and its last 4 characters.

use serde::Serialize;
use serde_json::Value;
use sonicscript::history::{History, Item};
use sonicscript::key::{self, KeySource};
use sonicscript::mywords::{self, Preview};
use sonicscript::settings::{self, MAX_FALLBACKS, Settings};
use sonicscript::{dictate, inject, keystore, setkey};
use tauri::{AppHandle, Manager};

const MODELS: &str = include_str!("../../../spec/models.json");

/// (provider in `spec/providers.json`, its key's variable, label, where to get a key, required).
const PROVIDERS: [(&str, &str, &str, &str, bool); 3] = [
    (
        "gemini",
        "GEMINI_API_KEY",
        "Gemini",
        "https://aistudio.google.com/apikey",
        true,
    ),
    (
        "groq",
        "GROQ_API_KEY",
        "Groq",
        "https://console.groq.com/keys",
        false,
    ),
    (
        "openai",
        "OPENAI_API_KEY",
        "OpenAI (paid)",
        "https://platform.openai.com/api-keys",
        false,
    ),
];

/// The only pages the window opens (in the user's browser).
const LINKS: [&str; 6] = [
    "https://aistudio.google.com/apikey",
    "https://console.groq.com/keys",
    "https://platform.openai.com/api-keys",
    "https://ai.google.dev/gemini-api/terms",
    "https://github.com/Ahsuu27488/sonic-script",
    "https://github.com/Ahsuu27488/sonic-script/blob/main/LICENSE",
];

#[derive(Serialize)]
pub struct KeyState {
    provider: &'static str,
    label: &'static str,
    env: &'static str,
    link: &'static str,
    required: bool,
    /// The key dictation uses, without the key.
    source: Option<KeySource>,
}

#[derive(Serialize)]
pub struct ModelInfo {
    id: String,
    label: String,
    tier: String,
    keys: Vec<String>,
    accuracy: Option<f64>,
    p50_ms: Option<u64>,
    limits: String,
    recommended: bool,
}

#[derive(Serialize)]
pub struct KeyChoice {
    name: &'static str,
    label: &'static str,
    warning: Option<&'static str>,
}

#[derive(Serialize)]
pub struct State {
    settings: Settings,
    key: KeyChoice,
    keys: Vec<KeyState>,
    models: Vec<ModelInfo>,
    autostart: bool,
    version: &'static str,
    /// Problems with the settings file.
    notices: Vec<String>,
}

fn choice(k: setkey::DictationKey) -> KeyChoice {
    KeyChoice {
        name: k.name,
        label: k.label,
        warning: k.warning,
    }
}

fn key_states() -> Vec<KeyState> {
    PROVIDERS
        .iter()
        .map(|&(provider, env, label, link, required)| KeyState {
            provider,
            label,
            env,
            link,
            required,
            source: key::source(env),
        })
        .collect()
}

/// The catalog's models the app can run (`spec/models.json`, D-027), in its order (the chain first).
fn models() -> Vec<ModelInfo> {
    let catalog: Value = serde_json::from_str(MODELS).unwrap_or(Value::Null);
    let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
    catalog["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| sonicscript_core::spec::engine(m["id"].as_str().unwrap_or_default()).is_some())
        .map(|m| ModelInfo {
            id: s(&m["id"]),
            label: s(&m["label"]),
            tier: s(&m["tier"]),
            keys: m["keys"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|k| k.as_str().map(str::to_string))
                .collect(),
            accuracy: m["results"]["accuracy"].as_f64(),
            p50_ms: m["results"]["p50_ms"].as_u64(),
            limits: s(&m["limits"]["text"]),
            recommended: m["recommended"] == true,
        })
        .collect()
}

/// The `setkey` name for a key or mouse button the user pressed in the page (`KeyboardEvent.code`, or
/// `Mouse3` / `Mouse4` for `MouseEvent.button` 3 / 4); `None` for a key that cannot be the dictation key's.
pub fn key_name(code: &str) -> Option<String> {
    let name = match code {
        "ControlRight" => "RightCtrl",
        "ControlLeft" => "LeftCtrl",
        "AltRight" => "RightAlt",
        "AltLeft" => "LeftAlt",
        "ShiftRight" => "RightShift",
        "ShiftLeft" => "LeftShift",
        "MetaRight" | "OSRight" => "RightWin",
        "Pause" => "Pause",
        "ScrollLock" => "ScrollLock",
        "CapsLock" => "CapsLock",
        "Insert" => "Insert",
        // MouseEvent.button: 3 = back (XButton1), 4 = forward (XButton2).
        "Mouse3" => "Mouse4",
        "Mouse4" => "Mouse5",
        f if f.len() <= 3
            && f.starts_with('F')
            && f[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) =>
        {
            f
        }
        // Any other key: `by_name` explains why it cannot be used.
        other => other,
    };
    (!name.is_empty()).then(|| name.to_string())
}

/// Checks settings from the page before they are saved: the key is allowed, the models exist, at most two
/// fallbacks and none twice, history kept up to 365 days (0 = forever).
pub fn check(mut s: Settings) -> Result<Settings, String> {
    setkey::by_name(&s.hotkey)?;
    let known = |id: &str| sonicscript_core::spec::engine(id).is_some();
    if let Some(main) = &s.main_model
        && !known(main)
    {
        return Err(format!("{main} is not a model this version can use."));
    }
    let mut seen: Vec<String> = s.main_model.iter().cloned().collect();
    let mut fallbacks = Vec::new();
    for f in s.fallbacks.drain(..) {
        if !known(&f) {
            return Err(format!("{f} is not a model this version can use."));
        }
        if !seen.contains(&f) {
            seen.push(f.clone());
            fallbacks.push(f);
        }
    }
    if fallbacks.len() > MAX_FALLBACKS {
        return Err(format!("At most {MAX_FALLBACKS} fallback models."));
    }
    s.fallbacks = fallbacks;
    if s.history_days > 365 {
        return Err("Keep the history up to 365 days (0 = keep it forever).".into());
    }
    Ok(s)
}

/// The page's copy of `popup_off_apps` may be older than the file: the pill's x writes the file directly
/// (`popupwin::popup_not_here`). The file's list wins; a missing or broken file keeps the page's.
fn keep_off_apps(mut s: Settings, path: &std::path::Path) -> Settings {
    if let Ok((disk, _)) = Settings::read(path) {
        s.popup_off_apps = disk.popup_off_apps;
    }
    s
}

#[tauri::command]
pub fn prefs_state() -> State {
    let (settings, notices) = settings::load();
    let (k, _) = setkey::from_settings(&settings);
    State {
        settings,
        key: choice(k),
        keys: key_states(),
        models: models(),
        autostart: crate::autostart::enabled(),
        version: env!("CARGO_PKG_VERSION"),
        notices,
    }
}

/// Saves the settings and tells dictation to reload them. Returns what was saved (cleaned up by `check`).
#[tauri::command]
pub fn prefs_save(settings: Settings) -> Result<Settings, String> {
    let path = settings::path()?;
    let s = keep_off_apps(check(settings)?, &path);
    s.save(&path)
        .map_err(|e| format!("Not saved: {} {e}", path.display()))?;
    dictate::request_reload();
    Ok(s)
}

/// The key the user pressed while picking the dictation key, or why it cannot be used.
#[tauri::command]
pub fn prefs_key_name(code: String) -> Result<KeyChoice, String> {
    let name = key_name(&code).ok_or("That key cannot start dictation.")?;
    setkey::by_name(&name).map(choice)
}

/// Pauses dictation while the user picks a key (pressing the current key must not start a recording);
/// `false` puts back what the tray's Pause says.
#[tauri::command]
pub fn prefs_pause(app: AppHandle, picking: bool) {
    let tray_paused = app
        .try_state::<crate::Tray>()
        .and_then(|t| t.status.lock().ok().map(|s| s.paused))
        .unwrap_or(false);
    sonicscript::hotkey::set_paused(picking || tray_paused);
}

fn provider_env(provider: &str) -> Result<&'static str, String> {
    PROVIDERS
        .iter()
        .find(|p| p.0 == provider)
        .map(|p| p.1)
        .ok_or_else(|| format!("unknown provider {provider}"))
}

/// Saves a key in Credential Manager (D-046) and returns the keys' new state.
#[tauri::command]
pub fn key_save(provider: String, value: String) -> Result<Vec<KeyState>, String> {
    let env = provider_env(&provider)?;
    let value = value.trim();
    if value.is_empty() {
        return Err("Paste the key first.".into());
    }
    keystore::set(env, value)?;
    dictate::request_reload();
    Ok(key_states())
}

#[tauri::command]
pub fn key_delete(provider: String) -> Result<Vec<KeyState>, String> {
    keystore::delete(provider_env(&provider)?)?;
    dictate::request_reload();
    Ok(key_states())
}

/// One request that needs the key (the provider's model list: no daily request used). The key in use, or the
/// one typed in the field when given.
#[tauri::command]
pub async fn key_test(provider: String, value: Option<String>) -> Result<String, String> {
    let env = provider_env(&provider)?;
    let p = sonicscript_core::spec::provider(&provider).ok_or("unknown provider")?;
    tauri::async_runtime::spawn_blocking(move || {
        let key = value
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| key::get(env));
        sonicscript_core::api::test_key(&p, &key)
            .map(|()| "The key works.".to_string())
            .map_err(|e| match e {
                sonicscript_core::api::ApiError::MissingKey(_) => "No key to test.".to_string(),
                sonicscript_core::api::ApiError::BadKey(_) => {
                    "This key was refused: copy it again from the provider's page.".to_string()
                }
                other => format!("Could not check the key: {other}"),
            })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn words_text() -> Result<String, String> {
    mywords::text()
}

#[tauri::command]
pub fn words_save(text: String) -> Result<usize, String> {
    mywords::save_text(&text)?;
    dictate::request_reload();
    Ok(mywords::load().len())
}

#[tauri::command]
pub fn words_prompt_copy() -> Result<(), String> {
    inject::copy(mywords::assistant_prompt())
}

/// What the clipboard (an assistant's answer) would add to the list.
#[tauri::command]
pub fn words_preview_clipboard() -> Result<(String, Preview), String> {
    let text = inject::clipboard_text()?;
    let preview = mywords::preview(&text, &mywords::load());
    Ok((text, preview))
}

#[tauri::command]
pub fn words_add(text: String) -> Result<String, String> {
    mywords::add(&text)?;
    dictate::request_reload();
    mywords::text()
}

/// The last 50 dictations, newest first.
#[tauri::command]
pub fn history_list() -> Result<Vec<Item>, String> {
    Ok(History::open()?.recent(50))
}

#[tauri::command]
pub fn history_delete(at: f64) -> Result<bool, String> {
    History::open()?.delete(at)
}

#[tauri::command]
pub fn history_copy(text: String) -> Result<(), String> {
    inject::copy(&text)
}

/// Plays a recording in the user's default player.
#[tauri::command]
pub fn history_play(audio: String) -> Result<(), String> {
    if !audio.starts_with("audio/") || audio.contains("..") {
        return Err("not a recording of the history".into());
    }
    // Explorer reads a "/" as a switch: the path gets Windows separators.
    let path = History::open()?.dir().join(audio.replace('/', "\\"));
    if !path.exists() {
        return Err("The recording is no longer there.".into());
    }
    std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

/// Hides the settings window, lets the app the user worked in come back to the front, and pastes there.
#[tauri::command]
pub async fn history_reinsert(app: AppHandle, text: String) -> Result<String, String> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.minimize();
    }
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        match inject::paste(&text)? {
            inject::Pasted::Pasted => Ok("Inserted.".to_string()),
            inject::Pasted::Copied(why) => Ok(format!("Copied ({why}): press Ctrl+V.")),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn autostart_set(app: AppHandle, on: bool) -> Result<bool, String> {
    crate::autostart::set(on)?;
    let now = crate::autostart::enabled();
    // The tray's tick says the same.
    if let Some(t) = app.try_state::<crate::Tray>() {
        let _ = t.autostart.set_checked(now);
    }
    Ok(now)
}

#[tauri::command]
pub fn open_link(url: String) -> Result<(), String> {
    if !LINKS.contains(&url.as_str()) {
        return Err("not a link this window opens".into());
    }
    std::process::Command::new("explorer")
        .arg(url)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_keys_map_to_setkey_names() {
        assert_eq!(key_name("ControlRight").as_deref(), Some("RightCtrl"));
        assert_eq!(key_name("AltRight").as_deref(), Some("RightAlt"));
        assert_eq!(key_name("F9").as_deref(), Some("F9"));
        assert_eq!(key_name("F24").as_deref(), Some("F24"));
        assert_eq!(key_name("Mouse3").as_deref(), Some("Mouse4"), "back button");
        for code in ["ControlRight", "F9", "Mouse3", "CapsLock", "MetaRight"] {
            let name = key_name(code).unwrap();
            assert!(setkey::by_name(&name).is_ok(), "{code} -> {name}");
        }
        // A typing key passes through, and `by_name` refuses it with its reason.
        let space = key_name("Space").unwrap();
        assert!(setkey::by_name(&space).is_err());
        assert!(prefs_key_name("KeyA".into()).is_err());
    }

    #[test]
    fn check_cleans_and_refuses() {
        let ok = check(Settings {
            hotkey: "F9".into(),
            main_model: Some("gemini-3.5-flash-lite".into()),
            fallbacks: vec![
                "gemini-3.5-flash-lite".into(),
                "gemini-3.5-flash".into(),
                "gemini-3.5-flash".into(),
            ],
            ..Settings::default()
        })
        .unwrap();
        assert_eq!(
            ok.fallbacks,
            ["gemini-3.5-flash"],
            "the main one and repeats dropped"
        );
        let bad = |s: Settings| check(s).unwrap_err();
        assert!(
            !bad(Settings {
                hotkey: "Space".into(),
                ..Settings::default()
            })
            .is_empty()
        );
        assert!(
            bad(Settings {
                main_model: Some("gemini-9".into()),
                ..Settings::default()
            })
            .contains("gemini-9")
        );
        assert!(
            bad(Settings {
                history_days: 400,
                ..Settings::default()
            })
            .contains("365")
        );
    }

    #[test]
    fn the_catalog_lists_runnable_models_first_the_default() {
        let m = models();
        assert!(m.len() >= 3);
        assert!(m[0].recommended, "the chain's main model first");
        assert!(
            m.iter()
                .all(|x| sonicscript_core::spec::engine(&x.id).is_some())
        );
    }

    #[test]
    fn the_pills_off_list_on_disk_wins_over_the_pages_copy() {
        let dir = std::env::temp_dir().join(format!("sonicscript-offapps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        // The pill's x added chrome.exe while the page had the old, empty list.
        std::fs::write(&path, "popup_off_apps = [\"chrome.exe\"]\n").unwrap();
        let page = Settings {
            sound: false,
            ..Settings::default()
        };
        let saved = keep_off_apps(page, &path);
        assert_eq!(saved.popup_off_apps, ["chrome.exe"]);
        assert!(!saved.sound, "the page's own change stays");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_known_links_open() {
        assert!(open_link("https://evil.example".into()).is_err());
    }
}
