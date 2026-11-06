//! SonicScript for Windows, the tray app (M2 plan phase 2, D-044): dictation as the CLI runs it, without a
//! console window, with a tray icon that shows its state and a menu (pause, settings, history folder, start
//! with Windows, quit). A debug build keeps its console: dictation's M1 lines are printed there.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod tray;

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
struct Tray {
    status: Mutex<Status>,
    headline: MenuItem<Wry>,
    pause: CheckMenuItem<Wry>,
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
    }
}

fn main() {
    tauri::Builder::default()
        // A second start opens the first one's settings instead of a second dictation (Review Focus 4).
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            open_settings(app)
        }))
        .setup(|app| {
            build_tray(app.handle())?;
            start_dictation(app.handle().clone());
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
    let quit = MenuItem::with_id(app, "quit", "Quit SonicScript", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &headline,
            &PredefinedMenuItem::separator(app)?,
            &pause,
            &settings,
            &history,
            &start,
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
            "settings" => open_settings(app),
            "history" => open_history(app),
            "autostart" => {
                let on = start.is_checked().unwrap_or(false);
                if let Err(e) = autostart::set(on) {
                    problem(app, e);
                }
                let _ = start.set_checked(autostart::enabled());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    app.manage(Tray {
        status: Mutex::new(status),
        headline,
        pause,
    });
    Ok(())
}

fn icon(s: &Status) -> Image<'static> {
    Image::new_owned(tray::icon(s.shown()), ICON_SIZE, ICON_SIZE)
}

/// Puts the status on the tray icon, its tooltip and the menu's first line.
fn redraw(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(t) = handle.try_state::<Tray>() else {
            return;
        };
        let Ok(s) = t.status.lock().map(|s| s.clone()) else {
            return;
        };
        if let Some(tray) = handle.tray_by_id(TRAY) {
            let _ = tray.set_icon(Some(icon(&s)));
            let _ = tray.set_tooltip(Some(s.tooltip()));
        }
        let _ = t.headline.set_text(s.headline());
        let _ = t.pause.set_checked(s.paused);
    });
}

/// Shows a problem that is not a dictation event (a menu action failed).
fn problem(app: &AppHandle, why: String) {
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
        .inner_size(560.0, 500.0)
        .resizable(true)
        .build();
    if let Err(e) = built {
        problem(app, format!("could not open the settings window: {e}"));
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
        let (settings, notes) = settings::load();
        cue::set_enabled(settings.sound);
        let result =
            key::choose_from(&settings, None, settings.format).and_then(|(mut choice, more)| {
                for note in notes.into_iter().chain(more) {
                    sink.send(Event::Warning(note));
                }
                choice.words = Some(Arc::new(mywords::load()));
                dictate::run(choice, settings.history, sink.clone())
            });
        if let Err(e) = result {
            sink.send(Event::Failed {
                lines: vec![format!("Dictation stopped: {e}")],
            });
        }
    });
}
