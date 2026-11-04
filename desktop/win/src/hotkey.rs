//! The global dictation key (M1 plan step 5, D-015): a low-level keyboard hook on its own thread. The hook
//! runs the rules in `sonicscript_core::keys` and only sends the resulting action down a channel. Windows
//! silently removes a low-level hook that is slow to answer, so nothing else happens in the callback.
//! Only Enter and Esc are ever swallowed, and only while recording (the rules decide). Keys a program sends
//! (marked injected, like the app's own paste) are passed on untouched.

use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use sonicscript_core::keys::{Action, Key, Machine};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_RETURN};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    SetWindowsHookExW, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
};

struct Hook {
    /// Virtual-key code of the dictation key (left/right specific: `VK_RCONTROL`, not `VK_CONTROL`).
    vk: u32,
    machine: Mutex<Machine>,
    tx: Sender<Action>,
}

static HOOK: OnceLock<Hook> = OnceLock::new();

/// Installs the hook on a new thread that pumps its messages for the life of the process. Call once.
pub fn spawn(vk: u16, tx: Sender<Action>) -> Result<(), String> {
    HOOK.set(Hook {
        vk: u32::from(vk),
        machine: Mutex::new(Machine::new()),
        tx,
    })
    .map_err(|_| "the key hook is already installed".to_string())?;
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // SAFETY: plain Win32 calls; `proc` matches HOOKPROC and lives for the whole program.
        let hook = unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(proc),
                GetModuleHandleW(std::ptr::null()),
                0,
            )
        };
        let ok = !hook.is_null();
        let _ = ready_tx.send(if ok {
            Ok(())
        } else {
            Err(format!(
                "could not install the keyboard hook: {}",
                std::io::Error::last_os_error()
            ))
        });
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

unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && let Some(hook) = HOOK.get()
    {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        // Keys sent by a program (our own Ctrl+V, step 7) are not the user's: they never reach the rules.
        if event.flags & LLKHF_INJECTED != 0 {
            // SAFETY: passes the event on unchanged.
            return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
        }
        let key = match event.vkCode {
            vk if vk == hook.vk => Key::Dictation,
            vk if vk == u32::from(VK_RETURN) => Key::Enter,
            vk if vk == u32::from(VK_ESCAPE) => Key::Esc,
            _ => Key::Other,
        };
        let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let reply = match hook.machine.lock() {
            Ok(mut m) if down => m.down(key, event.time),
            Ok(mut m) => m.up(key, event.time),
            Err(_) => Default::default(),
        };
        if let Some(action) = reply.action {
            let _ = hook.tx.send(action);
        }
        if reply.swallow {
            return 1;
        }
    }
    // SAFETY: passes the event on unchanged; the hook handle argument is ignored by Windows.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}
