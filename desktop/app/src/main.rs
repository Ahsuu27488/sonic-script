//! SonicScript for Windows, the tray app (M2 plan phase 2, D-044): dictation as the CLI runs it, without a
//! console window, with a tray icon that shows its state and a menu (pause, settings, history folder, start
//! with Windows, quit). A debug build keeps its console: dictation's M1 lines are printed there.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod focuswatch;
mod hud;
mod hudwin;
mod popup;
mod popupwin;
mod prefs;
mod tray;
mod update;
mod welcome;

use std::sync::{Arc, Mutex};

use sonicscript::events::{self, Event, Sink};
use sonicscript::{cue, dictate, history, hotkey, instance, key, mywords, settings};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, Wry};

use crate::tray::{ICON_SIZE, Status};

const TRAY: &str = "main";

/// The tray's menu items that change, and what it shows.
pub(crate) struct Tray {
    pub(crate) status: Mutex<Status>,
    headline: MenuItem<Wry>,
    pause: CheckMenuItem<Wry>,
    /// "Try again": sends the last recording nothing answered (M2 phase 6).
    retry: MenuItem<Wry>,
    /// "Start with Windows": the settings window changes it too (`prefs::autostart_set`).
    pub(crate) autostart: CheckMenuItem<Wry>,
    /// "Check for updates" / "Restart to update (0.2.1)" (D-053).
    update: MenuItem<Wry>,
}

/// Dictation's events, on the tray (and on stderr, seen in a debug build).
struct TraySink(AppHandle);

impl Sink for TraySink {
    fn send(&self, e: Event) {
        for (_, line) in events::console_lines(&e) {
            eprintln!("{line}");
        }
        let changed = match self.0.try_state::<Tray>() {
            Some(t) => t.status.lock().is_ok_and(|mut s| tray::apply(&mut s, &e)),
            None => false,
        };
        if changed {
            redraw(&self.0);
        }
        hudwin::apply(&self.0, &e);
        popupwin::apply(&self.0, &e);
        // "Restart to update" clicked during a dictation: installs once it has ended (D-053).
        update::install_if_waiting(&self.0);
    }
}

fn main() {
    // `--hud-demo`: shows every HUD state with made-up events instead of dictating (task 3.2's focus check).
    let demo = std::env::args().any(|a| a == "--hud-demo");
    // `--settings-preview` (debug builds): only the settings window, no dictation (checks of the page with
    // `APPDATA` / `LOCALAPPDATA` pointed at a scratch folder).
    let preview = cfg!(debug_assertions) && std::env::args().any(|a| a == "--settings-preview");
    // `--popup-preview` (debug builds): only the pop-up mic, no dictation (checks of where it shows).
    let popup_only = cfg!(debug_assertions) && std::env::args().any(|a| a == "--popup-preview");
    // `--welcome-preview` (debug builds): only the first-run window, no dictation.
    let welcome_only = cfg!(debug_assertions) && std::env::args().any(|a| a == "--welcome-preview");
    let preview = preview || popup_only || welcome_only;
    let mut builder = tauri::Builder::default();
    // A second start opens the first one's settings instead of a second dictation (Review Focus 4). The demo
    // and the preview never dictate (no key hook), so they may run next to the real app.
    if !demo && !preview {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
            open_settings(app)
        }));
    }
    builder
        // Rust side only (`update.rs`); the pages call our own commands, so no capability is needed.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            hudwin::level,
            hudwin::retry_last,
            hudwin::hud_hit_area,
            prefs::prefs_state,
            prefs::prefs_save,
            prefs::prefs_key_name,
            prefs::prefs_pause,
            prefs::key_save,
            prefs::key_delete,
            prefs::key_test,
            prefs::words_text,
            prefs::words_save,
            prefs::words_prompt_copy,
            prefs::words_preview_clipboard,
            prefs::words_add,
            prefs::history_list,
            prefs::history_delete,
            prefs::history_copy,
            prefs::history_play,
            prefs::history_reinsert,
            prefs::autostart_set,
            prefs::open_link,
            popupwin::popup_click,
            popupwin::popup_not_here,
            popupwin::popup_app_on,
            welcome::welcome_done,
            update::update_state,
            update::update_check,
            update::update_install,
        ])
        .setup(move |app| {
            build_tray(app.handle())?;
            hudwin::build(app.handle())?;
            // Without UI Automation the pop-up mic never shows; dictation by the key still works. The other
            // previews never show it (they may run next to the real app: two pills).
            if !demo
                && (!preview || popup_only)
                && let Err(e) = popupwin::start(app.handle())
            {
                problem(app.handle(), format!("The pop-up mic is off: {e}"));
            }
            if demo {
                hud_demo(app.handle().clone());
            } else if welcome_only {
                welcome::open(app.handle());
            } else if popup_only {
                // Only the pill: no settings window, no dictation.
            } else if preview {
                open_settings(app.handle());
            } else {
                // First: an update downloaded in an earlier run is installed now (the app exits and the
                // installer starts the new version), before dictation takes the keyboard (D-053).
                update::start(app.handle());
                start_dictation(app.handle().clone());
                // First run: no Gemini key anywhere (D-044).
                welcome::open_if_needed(app.handle());
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("SonicScript could not start")
        .run(|_, event| {
            // Closing the settings window must not end the app: only Quit does (it exits with a code).
            if let RunEvent::ExitRequested {
                api, code: None, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let status = Status::default();
    let headline = MenuItem::with_id(app, "status", status.headline(), false, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause dictation", true, false, None::<&str>)?;
    let retry = MenuItem::with_id(app, "retry", "Try again", false, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let history = MenuItem::with_id(app, "history", "Open history folder", true, None::<&str>)?;
    let start = CheckMenuItem::with_id(
        app,
        "autostart",
        "Start with Windows",
        true,
        autostart::enabled(),
        None::<&str>,
    )?;
    let update = MenuItem::with_id(
        app,
        "update",
        update::Phase::Idle.menu_label(),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit SonicScript", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &headline,
            &PredefinedMenuItem::separator(app)?,
            &retry,
            &pause,
            &settings,
            &history,
            &start,
            &update,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    TrayIconBuilder::with_id(TRAY)
        .icon(icon(&status))
        .tooltip(status.tooltip())
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "pause" => toggle_pause(app),
            "retry" => {
                // From the menu: copied, not pasted (the menu took the focus from the user's app).
                if !dictate::retry(true) {
                    problem(app, "Nothing to send again.".into());
                }
            }
            "settings" => open_settings(app),
            "history" => open_history(app),
            "autostart" => {
                let Some(t) = app.try_state::<Tray>() else {
                    return;
                };
                let on = t.autostart.is_checked().unwrap_or(false);
                if let Err(e) = autostart::set(on) {
                    problem(app, e);
                }
                let _ = t.autostart.set_checked(autostart::enabled());
            }
            "update" => {
                if update::current(app).ready_version().is_some() {
                    if let Err(e) = update::install(app) {
                        problem(app, format!("Could not update: {e}"));
                    }
                } else {
                    update::check_now(app);
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    app.manage(Tray {
        status: Mutex::new(status),
        headline,
        pause,
        retry,
        autostart: start,
        update,
    });
    Ok(())
}

fn icon(s: &Status) -> Image<'static> {
    Image::new_owned(tray::icon(s.shown()), ICON_SIZE, ICON_SIZE)
}

/// Puts the status on the tray icon, its tooltip and the menu's first line, and the updater's phase on its
/// menu item.
pub(crate) fn redraw(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(t) = handle.try_state::<Tray>() else {
            return;
        };
        let Ok(s) = t.status.lock().map(|s| s.clone()) else {
            return;
        };
        let phase = update::current(&handle);
        let extra = phase
            .ready_version()
            .map(|_| "Update ready: restart to update");
        if let Some(tray) = handle.tray_by_id(TRAY) {
            let _ = tray.set_icon(Some(icon(&s)));
            let _ = tray.set_tooltip(Some(s.tooltip_with(extra)));
        }
        let _ = t.update.set_text(phase.menu_label());
        let _ = t.update.set_enabled(phase.menu_enabled());
        let _ = t.headline.set_text(s.headline());
        let _ = t.pause.set_checked(s.paused);
        let _ = t.retry.set_enabled(s.can_retry);
    });
}

/// Shows a problem that is not a dictation event (a menu action failed).
pub(crate) fn problem(app: &AppHandle, why: String) {
    TraySink(app.clone()).send(Event::Warning(why));
}

fn toggle_pause(app: &AppHandle) {
    let Some(t) = app.try_state::<Tray>() else {
        return;
    };
    let paused = t.pause.is_checked().unwrap_or(false);
    // No hook yet (dictation failed to start): nothing to pause.
    let applied = hotkey::set_paused(paused);
    if let Ok(mut s) = t.status.lock() {
        s.paused = paused && applied;
    }
    redraw(app);
}

fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("SonicScript settings")
        .inner_size(760.0, 540.0)
        .min_inner_size(400.0, 420.0)
        .resizable(true)
        .build();
    match built {
        // Closed while a new key was being picked: dictation must not stay paused.
        Ok(w) => {
            let handle = app.clone();
            w.on_window_event(move |e| {
                if matches!(e, tauri::WindowEvent::Destroyed) {
                    prefs::prefs_pause(handle.clone(), false);
                }
            });
        }
        Err(e) => problem(app, format!("could not open the settings window: {e}")),
    }
}

fn open_history(app: &AppHandle) {
    match history::History::open() {
        Ok(h) => {
            if let Err(e) = std::process::Command::new("explorer").arg(h.dir()).spawn() {
                problem(app, format!("could not open the history folder: {e}"));
            }
        }
        Err(e) => problem(app, e),
    }
}

/// Runs dictation on its own thread with the user's settings, as the CLI's dictation does without flags.
fn start_dictation(app: AppHandle) {
    let sink: Arc<dyn Sink> = Arc::new(TraySink(app.clone()));
    std::thread::spawn(move || {
        // Another copy dictating (the CLI in a console): wait for it to quit rather than stay broken.
        let mut told = false;
        while instance::claim().is_err() {
            if !told {
                sink.send(Event::Warning(
                    "Another SonicScript is dictating (a console window?): this one starts when it quits."
                        .into(),
                ));
                told = true;
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
        if told && let Some(t) = app.try_state::<Tray>() {
            if let Ok(mut s) = t.status.lock() {
                s.clear_problem();
            }
            redraw(&app);
        }
        // Each turn runs dictation with the settings as they are now; a change in the settings window ends
        // the turn (`dictate::request_reload`) and the next one picks it up (M2 task 4.1).
        loop {
            let (settings, notes) = settings::load();
            cue::set_enabled(settings.sound);
            let result = key::choose_from(&settings, None, settings.format).and_then(
                |(mut choice, more)| {
                    for note in notes.into_iter().chain(more) {
                        sink.send(Event::Warning(note));
                    }
                    choice.words = Some(Arc::new(mywords::load()));
                    dictate::run(choice, settings.history, sink.clone())
                },
            );
            if let Err(e) = result {
                // The hook of an earlier turn may still be in place: it must not take the user's keys.
                hotkey::stop_listening();
                // No recording or answer is coming any more: the HUD must not wait for one.
                hudwin::reset(&app);
                sink.send(Event::Failed {
                    lines: vec![format!("Dictation stopped: {e}")],
                });
                // A missing or wrong key is fixed in the settings window: wait for its next change.
                while !dictate::take_reload() {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                if let Some(t) = app.try_state::<Tray>() {
                    if let Ok(mut s) = t.status.lock() {
                        s.clear_problem();
                    }
                    redraw(&app);
                }
            }
        }
    });
}

/// Runs every HUD state with made-up events and prints whether the window in front stayed the same (it must:
/// the paste goes there). Put Notepad in front within 3 s of starting it.
fn hud_demo(app: AppHandle) {
    use sonicscript::events::Paste;
    std::thread::spawn(move || {
        let sink = TraySink(app.clone());
        let listening = || Event::Listening {
            device: "demo".into(),
            rate: 48000,
            mic_ms: 2,
            prepared: true,
        };
        let text = |paste| Event::Text {
            text: String::new(),
            paste,
            details: String::new(),
        };
        let steps: Vec<(Event, u64)> = vec![
            (listening(), 2500),
            (Event::Processing, 1500),
            (text(Paste::Pasted(300)), 1500),
            (listening(), 1200),
            (Event::Processing, 800),
            (text(Paste::Copied("administrator window".into())), 3200),
            (listening(), 1000),
            (Event::Processing, 500),
            (Event::NoSpeech { seconds: 1.0 }, 2500),
            (
                Event::Failed {
                    lines: vec!["! Nothing typed: no model answered.".into()],
                },
                3000,
            ),
            (Event::CanRetry, 3000),
            (
                Event::Notice(
                    "! gemini-3.5-transcribe+gptoss20b has used up today's free requests. More."
                        .into(),
                ),
                3000,
            ),
            (listening(), 1500),
            (Event::Cancelled, 1000),
        ];
        std::thread::sleep(std::time::Duration::from_secs(3));
        let mut moved = 0;
        for (e, wait) in steps {
            let before = hudwin::native::foreground();
            let name = format!("{e:?}");
            sink.send(e);
            std::thread::sleep(std::time::Duration::from_millis(wait));
            let after = hudwin::native::foreground();
            let same = before == after;
            moved += usize::from(!same);
            println!(
                "hud-demo: {:<40} foreground {before:#x} -> {after:#x} {}",
                name.chars().take(40).collect::<String>(),
                if same { "same" } else { "MOVED" }
            );
        }
        println!("hud-demo: done, focus moved {moved} times");
        app.exit(0);
    });
}
