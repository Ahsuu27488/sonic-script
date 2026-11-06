//! The HUD Voice Ring's window (M2 task 3.2): built hidden at start, shown without taking focus near the
//! bottom of the monitor the user works on, and fed the views of `hud.rs`. The page (`ui/hud.*`) draws the
//! pill; the window itself is a transparent, click-through, topmost rectangle wider than the widest pill, so
//! the pill's width animates in CSS with no window resize.
//! Focus never moves (plan Review Focus 5): the window is `WS_EX_NOACTIVATE`, shown with
//! `SW_SHOWNOACTIVATE` and placed with `SWP_NOACTIVATE`, so the paste still goes to the user's app.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sonicscript::events::Event;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::hud::{Change, Hud};

pub const LABEL: &str = "hud";
/// The window, in logical pixels: the widest pill (360) plus room for its shadow.
const WIDTH: f64 = 420.0;
const HEIGHT: f64 = 72.0;
/// The pill's exit (`--dur-exit` 160 ms) runs before the window hides.
const EXIT_MS: u64 = 200;

/// The HUD's state between events; `generation` grows with every change, so an old hide timer never hides a
/// newer view.
#[derive(Default)]
pub struct HudState {
    hud: Mutex<Hud>,
    generation: AtomicU64,
}

/// Builds the hidden HUD window once (showing it later is then instant).
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("hud.html".into()))
        .title("SonicScript")
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
        .build()?;
    w.set_ignore_cursor_events(true)?;
    native::no_activate(&w);
    app.manage(HudState::default());
    Ok(())
}

/// Shows one dictation event on the HUD.
pub fn apply(app: &AppHandle, e: &Event) {
    let Some(state) = app.try_state::<HudState>() else {
        return;
    };
    let Ok(mut hud) = state.hud.lock() else {
        return;
    };
    let change = hud.apply(e);
    // Queued while the lock is held: shows and hides reach the main thread in the order of their generations.
    put(app, &state, change);
}

/// Forgets the recording and answers in flight (dictation stopped), so its failure can show.
pub fn reset(app: &AppHandle) {
    if let Some(state) = app.try_state::<HudState>()
        && let Ok(mut hud) = state.hud.lock()
    {
        hud.reset();
    }
}

/// Puts one change on the window. Call with the `Hud` lock held.
fn put(app: &AppHandle, state: &HudState, change: Change) {
    if change == Change::None {
        return;
    }
    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    match change {
        Change::None => {}
        Change::Show(view) => {
            let json = serde_json::to_string(&view).unwrap_or_default();
            // Before the page has loaded, the view waits for it (`hud.js` shows `__hudPending`).
            let js = format!(
                "(v => window.hudShow ? window.hudShow(v) : (window.__hudPending = v))({json})"
            );
            let clickable = view.action.is_some();
            on_main(app, generation, move |w| {
                // Click-through, except while the pill has a button ("Try again"); then only the pill takes
                // clicks (the page sends its area: `hud_hit_area`), the rest of the window passes them on.
                if !clickable {
                    native::hit_area(w, None);
                }
                let _ = w.set_ignore_cursor_events(!clickable);
                let _ = w.eval(js);
                native::show(w);
            });
            if let Some(ms) = view.hide_after_ms {
                expire_later(app.clone(), generation, ms);
            }
        }
        Change::Hide => {
            on_main(app, generation, |w| {
                let _ = w.eval("window.hudHide && window.hudHide()");
            });
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(EXIT_MS));
                on_main(&app, generation, |w| {
                    native::hide(w);
                    native::hit_area(w, None);
                    let _ = w.set_ignore_cursor_events(true);
                });
            });
        }
    }
}

/// After `ms`, a passing view gives way to what comes next (a waiting note, "Writing…") or the pill leaves,
/// unless another view came meanwhile.
fn expire_later(app: AppHandle, generation: u64, ms: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        let Some(state) = app.try_state::<HudState>() else {
            return;
        };
        let Ok(mut hud) = state.hud.lock() else {
            return;
        };
        if state.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let change = match hud.next() {
            Some(v) => Change::Show(v),
            None => Change::Hide,
        };
        put(&app, &state, change);
    });
}

/// Runs `f` on the main thread if no newer change came before it got there (the main thread runs the
/// queued closures in order, so the check and the window call cannot be split by another change).
fn on_main(app: &AppHandle, generation: u64, f: impl FnOnce(&WebviewWindow) + Send + 'static) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let current = handle
            .try_state::<HudState>()
            .is_some_and(|s| s.generation.load(Ordering::SeqCst) == generation);
        if current && let Some(w) = handle.get_webview_window(LABEL) {
            f(&w);
        }
    });
}

/// The HUD's "Try again": sends the kept recording again and pastes it (the HUD never took the focus).
#[tauri::command]
pub fn retry_last() -> bool {
    sonicscript::dictate::retry(false)
}

/// The pill's area in the window's physical pixels, while it has a button: only it takes clicks.
#[tauri::command]
pub fn hud_hit_area(app: AppHandle, x: i32, y: i32, w: i32, h: i32) {
    if let Some(win) = app.get_webview_window(LABEL) {
        native::hit_area(&win, Some((x, y, w, h)));
    }
}

/// The mic level for the ring, 0.0-1.0 (the page asks once a frame while listening).
#[tauri::command]
pub fn level() -> f32 {
    sonicscript::audio::level()
}

#[cfg(windows)]
pub mod native {
    use tauri::WebviewWindow;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        CreateRectRgn, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
        SetWindowRgn,
    };
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, HWND_TOPMOST, SW_HIDE,
        SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetWindowLongPtrW, SetWindowPos, ShowWindow,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    };

    use super::{HEIGHT, WIDTH};

    /// How far up the monitor's work area the pill's window sits: its bottom at 12% (DESIGN.md).
    const FROM_BOTTOM: f64 = 0.12;

    fn hwnd(w: &WebviewWindow) -> Option<HWND> {
        w.hwnd().ok().map(|h| h.0 as HWND)
    }

    /// The styles that keep the window from ever taking focus or showing in Alt+Tab.
    pub fn no_activate(w: &WebviewWindow) {
        let Some(h) = hwnd(w) else { return };
        // SAFETY: a valid window handle of this process; only extended style bits are added.
        unsafe {
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            let add = (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST) as isize;
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex | add);
        }
    }

    /// Where the window goes, in physical pixels: centred on the work area, its bottom `FROM_BOTTOM` up.
    pub fn place(work: RECT, dpi: u32) -> (i32, i32, i32, i32) {
        let scale = f64::from(dpi.max(96)) / 96.0;
        let (w, h) = (
            (WIDTH * scale).round() as i32,
            (HEIGHT * scale).round() as i32,
        );
        let (ww, wh) = (work.right - work.left, work.bottom - work.top);
        let x = work.left + (ww - w) / 2;
        let y = work.bottom - (f64::from(wh) * FROM_BOTTOM).round() as i32 - h;
        (x, y, w, h)
    }

    /// The window the user works in (the paste's target).
    pub fn foreground() -> isize {
        // SAFETY: no arguments; returns a handle or null.
        unsafe { GetForegroundWindow() as isize }
    }

    /// Shows the window on the monitor of the window in front, without activating it.
    pub fn show(w: &WebviewWindow) {
        let Some(h) = hwnd(w) else { return };
        // SAFETY: plain Win32 calls on our own window and the foreground window's monitor.
        unsafe {
            let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return;
            }
            let (mut dx, mut dy) = (96, 96);
            if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) != 0 {
                dx = 96;
            }
            let (x, y, cx, cy) = place(info.rcWork, dx);
            // Twice: moving to a monitor with another scale makes Windows resize it once in between.
            for _ in 0..2 {
                SetWindowPos(h, HWND_TOPMOST, x, y, cx, cy, SWP_NOACTIVATE);
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

    /// Limits the window (drawing and clicks) to a rectangle with a little room for the pill's shadow, or
    /// gives it its whole area back (`None`).
    pub fn hit_area(w: &WebviewWindow, area: Option<(i32, i32, i32, i32)>) {
        let Some(h) = hwnd(w) else { return };
        // SAFETY: our own window; Windows owns the region after SetWindowRgn succeeds.
        unsafe {
            let region = match area {
                Some((x, y, cx, cy)) => CreateRectRgn(x - 8, y - 8, x + cx + 8, y + cy + 8),
                None => std::ptr::null_mut(),
            };
            SetWindowRgn(h, region, 1);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn placed_bottom_centre_in_the_monitors_scale() {
            let work = RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            };
            assert_eq!(place(work, 96), (750, 843, 420, 72));
            // 150% on a second monitor to the right.
            let right = RECT {
                left: 1920,
                top: 0,
                right: 1920 + 2560,
                bottom: 1400,
            };
            let (x, y, w, h) = place(right, 144);
            assert_eq!((w, h), (630, 108));
            assert_eq!(x, 1920 + (2560 - 630) / 2);
            assert_eq!(y, 1400 - 168 - 108);
        }
    }
}

#[cfg(not(windows))]
pub mod native {
    use tauri::WebviewWindow;
    pub fn no_activate(_: &WebviewWindow) {}
    pub fn show(w: &WebviewWindow) {
        let _ = w.show();
    }
    pub fn hide(w: &WebviewWindow) {
        let _ = w.hide();
    }
    pub fn hit_area(_: &WebviewWindow, _: Option<(i32, i32, i32, i32)>) {}
    pub fn foreground() -> isize {
        0
    }
}
