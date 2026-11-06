//! The global dictation key (M1 plan steps 5 and 6, D-015, D-018, D-037): low-level keyboard (and, for a mouse
//! side button, mouse) hooks on their own thread. The hook runs the rules in `sonicscript_core::keys` and only
//! sends the resulting action down a channel. Windows silently removes a low-level hook that is slow to answer,
//! so nothing else happens in the callback.
//! Enter and Esc are swallowed only while recording (the rules decide). The dictation key itself reaches the app
//! when it is Ctrl / Shift / Alt / Win (shortcuts keep working; a lone Alt / Win tap is masked so no menu opens)
//! and never when it is a key or button that does something alone (D-037). Keys a program sends (marked
//! injected, like the app's own paste) are passed on untouched.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};

use sonicscript_core::keys::{Action, Key, Machine};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_RETURN};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, HOOKPROC, KBDLLHOOKSTRUCT, LLKHF_INJECTED,
    LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, SetWindowsHookExW, WH_KEYBOARD_LL, WH_MOUSE_LL,
    WM_KEYDOWN, WM_SYSKEYDOWN, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use crate::setkey::{DictationKey, PassOn, Trigger};

struct Hook {
    key: DictationKey,
    machine: Mutex<Machine>,
    tx: Sender<Action>,
    /// The dictation key is down (to tell a new press from auto-repeat).
    held: AtomicBool,
    /// Asks the mask thread to send the masking keystroke (Alt / Win, D-037).
    mask: Option<Sender<()>>,
    /// Dictation is paused (the tray's "Pause dictation", M2 task 2.2): every key and button passes on
    /// untouched and the rules do not run.
    paused: AtomicBool,
}

static HOOK: OnceLock<Hook> = OnceLock::new();

/// Installs the hooks on a new thread that pumps their messages for the life of the process. Call once.
pub fn spawn(key: DictationKey, tx: Sender<Action>) -> Result<(), String> {
    let mask = (key.pass_on == PassOn::Masked).then(|| {
        let (mask_tx, mask_rx) = std::sync::mpsc::channel::<()>();
        // Sent from here, not from inside the hook, so it lands after the key-down it masks.
        std::thread::spawn(move || {
            while mask_rx.recv().is_ok() {
                crate::inject::send(&[crate::setkey::MASK_VK]);
            }
        });
        mask_tx
    });
    HOOK.set(Hook {
        key,
        machine: Mutex::new(Machine::new()),
        tx,
        held: AtomicBool::new(false),
        mask,
        paused: AtomicBool::new(false),
    })
    .map_err(|_| "the key hook is already installed".to_string())?;
    let mouse = matches!(key.trigger, Trigger::Mouse(_));
    install(Some(keyboard), mouse.then_some(mouse_proc as _))
}

/// Installs the given hooks on their own thread and waits until they are in place.
fn install(keyboard_proc: HOOKPROC, mouse_proc: HOOKPROC) -> Result<(), String> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let set = |id, proc: HOOKPROC| {
            // SAFETY: plain Win32 call; `proc` matches HOOKPROC and lives for the whole program.
            let hook =
                unsafe { SetWindowsHookExW(id, proc, GetModuleHandleW(std::ptr::null()), 0) };
            if hook.is_null() {
                Err(format!(
                    "could not install the key hook: {}",
                    std::io::Error::last_os_error()
                ))
            } else {
                Ok(())
            }
        };
        let result = set(WH_KEYBOARD_LL, keyboard_proc).and_then(|()| match mouse_proc {
            Some(_) => set(WH_MOUSE_LL, mouse_proc),
            None => Ok(()),
        });
        let ok = result.is_ok();
        let _ = ready_tx.send(result);
        if ok {
            // A low-level hook is called on the thread that installed it, while that thread waits here.
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            // SAFETY: `msg` is a valid MSG to write into; GetMessageW returns 0 on WM_QUIT, -1 on error.
            while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {}
        }
    });
    ready_rx
        .recv()
        .map_err(|_| "the key hook thread stopped".to_string())?
}

/// The app ended the recording itself (5-minute cap, mic failed): resets the key rules so Enter and Esc
/// are no longer swallowed.
pub fn force_stop() -> Option<Action> {
    HOOK.get()?.machine.lock().ok()?.force_stop()
}

/// Pauses or resumes dictation (M2 task 2.2); false when the hook is not installed yet. While paused the
/// dictation key does nothing and reaches the app like any key; a recording running when it is paused is
/// cancelled.
pub fn set_paused(paused: bool) -> bool {
    match HOOK.get() {
        Some(hook) => {
            pause(hook, paused);
            true
        }
        None => false,
    }
}

fn pause(hook: &Hook, paused: bool) {
    let Ok(mut machine) = hook.machine.lock() else {
        return;
    };
    hook.paused.store(paused, Ordering::Relaxed);
    if paused && machine.force_stop().is_some() {
        let _ = hook.tx.send(Action::Cancel);
    }
    // Starts clean either way: a key held across the switch must not finish a press the rules never saw.
    *machine = Machine::new();
    hook.held.store(false, Ordering::Relaxed);
}

/// Runs one event of the dictation key or another key through the rules; true = keep it from the app.
fn handle(hook: &Hook, key: Key, down: bool, time: u32) -> bool {
    if hook.paused.load(Ordering::Relaxed) {
        return false;
    }
    let dictation = key == Key::Dictation;
    if dictation {
        let was_held = hook.held.swap(down, Ordering::Relaxed);
        if down
            && !was_held
            && let Some(mask) = &hook.mask
        {
            let _ = mask.send(());
        }
    }
    let reply = match hook.machine.lock() {
        // Paused while this event waited for the lock (`pause` sets the flag under it): no recording starts.
        Ok(_) if hook.paused.load(Ordering::Relaxed) => return false,
        Ok(mut m) if down => m.down(key, time),
        Ok(mut m) => m.up(key, time),
        Err(_) => Default::default(),
    };
    if let Some(action) = reply.action {
        let _ = hook.tx.send(action);
    }
    reply.swallow || (dictation && hook.key.pass_on == PassOn::Never)
}

unsafe extern "system" fn keyboard(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && let Some(hook) = HOOK.get()
    {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        // Keys sent by a program (our own Ctrl+V and mask key) are not the user's: they never reach the rules.
        if event.flags & LLKHF_INJECTED == 0 {
            let key = match event.vkCode {
                vk if hook.key.trigger == Trigger::Key(vk as u16) => Key::Dictation,
                vk if vk == u32::from(VK_RETURN) => Key::Enter,
                vk if vk == u32::from(VK_ESCAPE) => Key::Esc,
                _ => Key::Other,
            };
            let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            if handle(hook, key, down, event.time) {
                return 1;
            }
        }
    }
    // SAFETY: passes the event on unchanged; the hook handle argument is ignored by Windows.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Which side button a mouse event is about (`XBUTTON1` / `XBUTTON2`), with true for down.
fn x_button(wparam: WPARAM, event: &MSLLHOOKSTRUCT) -> Option<(u16, bool)> {
    let down = match wparam as u32 {
        WM_XBUTTONDOWN => true,
        WM_XBUTTONUP => false,
        _ => return None,
    };
    Some(((event.mouseData >> 16) as u16, down))
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && let Some(hook) = HOOK.get()
    {
        // SAFETY: for WH_MOUSE_LL with HC_ACTION, lparam points to an MSLLHOOKSTRUCT.
        let event = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if event.flags & LLMHF_INJECTED == 0
            && let Some((button, down)) = x_button(wparam, event)
            && hook.key.trigger == Trigger::Mouse(button)
            && handle(hook, Key::Dictation, down, event.time)
        {
            return 1;
        }
    }
    // SAFETY: passes the event on unchanged.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// `--set-key`: the keys and side buttons the user presses, kept from the apps meanwhile (other mouse buttons
/// and movement pass). Lasts for the rest of the process.
static CAPTURE: OnceLock<Mutex<Sender<Trigger>>> = OnceLock::new();

pub fn capture() -> Result<Receiver<Trigger>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    CAPTURE
        .set(Mutex::new(tx))
        .map_err(|_| "the key capture is already running".to_string())?;
    install(Some(capture_keyboard), Some(capture_mouse))?;
    Ok(rx)
}

fn captured(trigger: Trigger) {
    if let Some(Ok(tx)) = CAPTURE.get().map(Mutex::lock) {
        let _ = tx.send(trigger);
    }
}

unsafe extern "system" fn capture_keyboard(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if event.flags & LLKHF_INJECTED == 0 {
            if matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
                captured(Trigger::Key(event.vkCode as u16));
            }
            return 1;
        }
    }
    // SAFETY: passes the event on unchanged.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn capture_mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_MOUSE_LL with HC_ACTION, lparam points to an MSLLHOOKSTRUCT.
        let event = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if event.flags & LLMHF_INJECTED == 0
            && let Some((button, down)) = x_button(wparam, event)
        {
            if down {
                captured(Trigger::Mouse(button));
            }
            return 1;
        }
    }
    // SAFETY: passes the event on unchanged.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn hook(name: &str) -> (Hook, Receiver<Action>) {
        let (tx, rx) = channel();
        let key = crate::setkey::by_name(name).unwrap();
        let hook = Hook {
            key,
            machine: Mutex::new(Machine::new()),
            tx,
            held: AtomicBool::new(false),
            mask: None,
            paused: AtomicBool::new(false),
        };
        (hook, rx)
    }

    #[test]
    fn paused_hook_passes_everything() {
        // F9 is kept from the app when dictation runs (PassOn::Never); paused, it must reach the app.
        let (hook, rx) = hook("F9");
        pause(&hook, true);
        assert!(!handle(&hook, Key::Dictation, true, 0));
        assert!(!handle(&hook, Key::Dictation, false, 50));
        assert!(!handle(&hook, Key::Enter, true, 60));
        assert!(rx.try_recv().is_err(), "no action while paused");
        // Resumed: the same tap starts a recording again and F9 is kept from the app.
        pause(&hook, false);
        assert!(handle(&hook, Key::Dictation, true, 1000));
        assert!(handle(&hook, Key::Dictation, false, 1050));
        assert_eq!(rx.try_recv().ok(), Some(Action::Start));
    }

    #[test]
    fn pause_cancels_a_recording() {
        let (hook, rx) = hook("RightCtrl");
        handle(&hook, Key::Dictation, true, 0);
        handle(&hook, Key::Dictation, false, 50);
        assert_eq!(rx.try_recv().ok(), Some(Action::Start));
        pause(&hook, true);
        assert_eq!(rx.try_recv().ok(), Some(Action::Cancel));
        // Enter is no longer taken from the app.
        assert!(!handle(&hook, Key::Enter, true, 100));
        // Pausing when idle sends nothing.
        pause(&hook, false);
        pause(&hook, true);
        assert!(rx.try_recv().is_err());
    }
}
