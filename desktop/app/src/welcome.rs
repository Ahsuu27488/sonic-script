//! First run (M2 phase 7, D-044): when dictation has no Gemini key from any source (D-046), the app opens a
//! short window: "Bolo, hum likh denge.", the Gemini key (required), the Groq key (optional), then a try box.
//! The page saves keys with the settings window's commands (`prefs::key_test`, `prefs::key_save`); keys never
//! come back to it.

use sonicscript::key::{self, KeySource};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "welcome";

/// Whether the first-run window is needed: no Gemini key in the environment, Credential Manager or the user
/// environment.
pub fn needed(gemini: Option<&KeySource>) -> bool {
    gemini.is_none()
}

/// Opens the window when no Gemini key is found.
pub fn open_if_needed(app: &AppHandle) {
    if needed(key::source("GEMINI_API_KEY").as_ref()) {
        open(app);
    }
}

pub fn open(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("welcome.html".into()))
        .title("Welcome to SonicScript")
        .inner_size(540.0, 680.0)
        .min_inner_size(400.0, 480.0)
        .resizable(true)
        .center()
        .build();
    if let Err(e) = built {
        crate::problem(app, format!("could not open the welcome window: {e}"));
    }
}

/// "Done": closes the window (SonicScript stays in the tray).
#[tauri::command]
pub fn welcome_done(app: AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonicscript::key::KeyFrom;

    #[test]
    fn welcome_when_no_gemini_key() {
        assert!(needed(None));
    }

    #[test]
    fn no_welcome_with_a_key() {
        for from in [KeyFrom::Environment, KeyFrom::App, KeyFrom::UserEnvironment] {
            let k = KeySource {
                from,
                last4: "1a2b".into(),
            };
            assert!(!needed(Some(&k)));
        }
    }
}
