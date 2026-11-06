//! The pop-up mic's window (M2 phase 5, D-016): a small Voice Ring next to the text field that has the focus.
//! A click starts or stops a recording as a tap of the dictation key would; it never starts the mic by
//! itself. Like the HUD it never takes the focus (`WS_EX_NOACTIVATE`), so the text still goes to the field.

use std::sync::Mutex;
use std::sync::mpsc;

use sonicscript::events::Event;
use sonicscript::settings;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::popup::{self, Focus};

pub const LABEL: &str = "popup";
/// The window in logical pixels: the 36 px ring, then the 20 px "Not in this app" button.
const WIDTH: f64 = 64.0;
const HEIGHT: f64 = 40.0;
/// The gap between the field and the pill, logical pixels.
const GAP: f64 = 6.0;

/// The program the pill is showing for ("Not in this app" turns it off there).
#[derive(Default)]
pub struct PopupState {
    exe: Mutex<String>,
}

/// Builds the hidden pill window and starts watching the focus. Without UI Automation the pill never shows
/// (returned as a problem for the tray).
pub fn start(app: &AppHandle) -> Result<(), String> {
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("popup.html".into()))
        .title("SonicScript mic")
        .inner_size(WIDTH, HEIGHT)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    crate::hudwin::native::no_activate(&w);
    app.manage(PopupState::default());
    let (tx, rx) = mpsc::channel();
    let handle = app.clone();
    crate::focuswatch::start(tx, move |e| {
        crate::popup::debug_log(&e);
        crate::problem(&handle, format!("The pop-up mic is off: {e}"));
    });
    let app = app.clone();
    std::thread::spawn(move || follow(app, rx));
    Ok(())
}

/// Shows or hides the pill for each focus change (only the newest of a burst).
fn follow(app: AppHandle, rx: mpsc::Receiver<Option<Focus>>) {
    let own = std::process::id();
    let own_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    // A config.toml that cannot be read keeps the last good settings (not the defaults: the pill on
    // everywhere again).
    let mut last = settings::load().0;
    while let Ok(mut focus) = rx.recv() {
        while let Ok(newer) = rx.try_recv() {
            focus = newer;
        }
        // Our own windows (the pill, the HUD, settings): leave the pill as it is.
        if focus.as_ref().is_some_and(|f| f.pid == own) {
            continue;
        }
        if let Ok(path) = settings::path()
            && let Ok((s, _)) = settings::Settings::read(&path)
        {
            last = s;
        }
        let s = &last;
        crate::popup::debug_log(&format!(
            "popup: focus {:?} -> {}",
            focus,
            focus.as_ref().is_some_and(|f| popup::shows(
                f,
                s.popup_mic,
                &s.popup_off_apps,
                own,
                &own_exe
            ))
        ));
        match focus.filter(|f| popup::shows(f, s.popup_mic, &s.popup_off_apps, own, &own_exe)) {
            Some(f) => {
                if let Some(state) = app.try_state::<PopupState>()
                    && let Ok(mut exe) = state.exe.lock()
                {
                    *exe = f.exe.clone();
                }
                let rect = f.rect;
                on_main(&app, move |w| native::show_near(w, rect));
            }
            None => on_main(&app, native::hide),
        }
    }
}

/// The pill follows the recording: a filled ring while one runs.
pub fn apply(app: &AppHandle, e: &Event) {
    let recording = match e {
        Event::Listening { .. } => true,
        Event::Processing | Event::Cancelled | Event::Failed { .. } => false,
        _ => return,
    };
    on_main(app, move |w| {
        let _ = w.eval(format!(
            "window.popupRecording && window.popupRecording({recording})"
        ));
    });
}

fn on_main(app: &AppHandle, f: impl FnOnce(&WebviewWindow) + Send + 'static) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = handle.get_webview_window(LABEL) {
            f(&w);
        }
    });
}

/// A click on the ring: starts or stops a recording (false when dictation is not running or paused).
#[tauri::command]
pub fn popup_click() -> bool {
    sonicscript::hotkey::click()
}

/// "Turn on again" in the settings window: takes a program off `popup_off_apps`; the list left.
#[tauri::command]
pub fn popup_app_on(exe: String) -> Result<Vec<String>, String> {
    let path = settings::path()?;
    let (mut s, _) = settings::Settings::read(&path)?;
    s.popup_off_apps.retain(|a| !a.eq_ignore_ascii_case(&exe));
    s.save(&path).map_err(|e| e.to_string())?;
    Ok(s.popup_off_apps)
}

/// "Not in this app": adds the program to `popup_off_apps` and hides the pill.
#[tauri::command]
pub fn popup_not_here(app: AppHandle) -> Result<String, String> {
    let exe = app
        .try_state::<PopupState>()
        .and_then(|s| s.exe.lock().ok().map(|e| e.clone()))
        .filter(|e| !e.is_empty())
        .ok_or("no app to turn it off for")?;
    let path = settings::path()?;
    let (mut s, _) = settings::Settings::read(&path)?;
    if !s
        .popup_off_apps
        .iter()
        .any(|a| a.eq_ignore_ascii_case(&exe))
    {
        s.popup_off_apps.push(exe.clone());
    }
    s.save(&path).map_err(|e| e.to_string())?;
    on_main(&app, native::hide);
    Ok(exe)
}

#[cfg(windows)]
mod native {
    use tauri::WebviewWindow;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
    };
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetWindowPos, ShowWindow,
    };

    use super::{GAP, HEIGHT, WIDTH};
    use crate::popup::{Rect, place};

    fn hwnd(w: &WebviewWindow) -> Option<HWND> {
        w.hwnd().ok().map(|h| h.0 as HWND)
    }

    /// Shows the pill next to `field` (physical pixels), on the field's monitor, without activating it.
    pub fn show_near(w: &WebviewWindow, field: Rect) {
        let Some(h) = hwnd(w) else { return };
        let r = RECT {
            left: field.left,
            top: field.top,
            right: field.right,
            bottom: field.bottom,
        };
        // SAFETY: plain Win32 calls on our own window and a monitor handle.
        unsafe {
            let monitor = MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return;
            }
            let (mut dx, mut dy) = (96, 96);
            if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) != 0 {
                dx = 96;
            }
            let scale = f64::from(dx.max(96)) / 96.0;
            let (cw, ch) = (
                (WIDTH * scale).round() as i32,
                (HEIGHT * scale).round() as i32,
            );
            let wa = info.rcWork;
            let work = Rect {
                left: wa.left,
                top: wa.top,
                right: wa.right,
                bottom: wa.bottom,
            };
            // The ring is the window's left square; its right part holds the small "Not in this app" button.
            let ring = ch;
            let (x, y) = place(field, work, ring, (GAP * scale).round() as i32);
            let x = (x + ring - cw).max(work.left).min(work.right - cw);
            for _ in 0..2 {
                SetWindowPos(h, HWND_TOPMOST, x, y, cw, ch, SWP_NOACTIVATE);
            }
            ShowWindow(h, SW_SHOWNOACTIVATE);
        }
    }

    pub fn hide(w: &WebviewWindow) {
        if let Some(h) = hwnd(w) {
            // SAFETY: our own window.
            unsafe { ShowWindow(h, SW_HIDE) };
        }
    }
}

#[cfg(not(windows))]
mod native {
    use crate::popup::Rect;
    use tauri::WebviewWindow;
    pub fn show_near(w: &WebviewWindow, _: Rect) {
        let _ = w.show();
    }
    pub fn hide(w: &WebviewWindow) {
        let _ = w.hide();
    }
}
