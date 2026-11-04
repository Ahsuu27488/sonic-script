//! Puts the dictated text at the cursor of whatever app is in front (M1 plan step 7). The text goes through
//! the clipboard: the user's clipboard is saved, the text is set (kept out of Win+V history and cloud
//! clipboard), Ctrl+V is sent (Ctrl+Shift+V in a terminal), and 300 ms later the user's clipboard comes back,
//! unless something else was copied in between. A window running as administrator does not accept keys from a
//! normal app (UIPI): there the text is only copied and the user pastes it.
//!
//! Text pasted into a console stays in its input buffer until something reads it, and PowerShell would run it
//! as commands after SonicScript quits; so after every paste our own console's input is cleared (the paste may
//! have gone there), and in old console windows line breaks become spaces (each line would be a command).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, GlobalFree, HANDLE, HWND};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::Console::{
    FlushConsoleInputBuffer, GetStdHandle, STD_INPUT_HANDLE,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
};

/// How long the target app gets to read the clipboard before the user's own clipboard comes back (plan step 7).
const RESTORE_AFTER: Duration = Duration::from_millis(300);
/// How long to wait for the user to let go of Shift / Alt / Ctrl / Win, which would change what Ctrl+V does.
const MODIFIERS_WAIT: Duration = Duration::from_secs(1);
/// Clipboard data bigger than this is not saved (it would be lost; restoring it would be slow anyway).
const SAVE_LIMIT: usize = 64 << 20;

/// One paste at a time: two dictations finishing together would swap each other's clipboard.
static BUSY: Mutex<()> = Mutex::new(());

/// What happened to the text.
#[derive(Debug, PartialEq, Eq)]
pub enum Pasted {
    /// Typed at the cursor; the user's clipboard is back.
    Pasted,
    /// Left on the clipboard for the user to paste with Ctrl+V, and why it was not pasted.
    Copied(&'static str),
}

/// Puts `text` at the cursor of the window in front. Errors only when the clipboard could not be used at all.
pub fn paste(text: &str) -> Result<Pasted, String> {
    let _one = BUSY.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: plain Win32 call.
    let target = unsafe { GetForegroundWindow() };
    let class = class_name(target);
    let text = for_window(text, &class);
    if target.is_null() {
        set_text(&text)?;
        return Ok(Pasted::Copied("no window is in front"));
    }
    if elevated(target) && !self_elevated() {
        set_text(&text)?;
        return Ok(Pasted::Copied("the window in front runs as administrator"));
    }
    let saved = save()?;
    let ours = set_text(&text)?;
    if !wait_for_modifiers() {
        return Ok(Pasted::Copied(
            "a key (Shift, Alt, Ctrl or Win) was still held down",
        ));
    }
    if !send(paste_keys(&class)) {
        return Ok(Pasted::Copied("Windows did not accept the keystroke"));
    }
    std::thread::sleep(RESTORE_AFTER);
    // Something copied in the meantime is the user's newer clipboard: leave it.
    // SAFETY: plain Win32 call.
    if unsafe { GetClipboardSequenceNumber() } == ours {
        restore(&saved)?;
    }
    flush_own_console();
    Ok(Pasted::Pasted)
}

/// The keys that paste in a window of this class: Ctrl+Shift+V in terminals, Ctrl+V everywhere else.
fn paste_keys(class: &str) -> &'static [u16] {
    if is_terminal(class) {
        &[VK_CONTROL, VK_SHIFT, VK_V]
    } else {
        &[VK_CONTROL, VK_V]
    }
}

fn is_terminal(class: &str) -> bool {
    matches!(
        class,
        "CASCADIA_HOSTING_WINDOW_CLASS" | "ConsoleWindowClass"
    )
}

/// Windows line breaks; an old console window (conhost) gets spaces instead, because it would run each pasted
/// line as a command. Windows Terminal keeps the lines (it asks before a multi-line paste).
fn for_window(text: &str, class: &str) -> String {
    if class == "ConsoleWindowClass" {
        text.lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    }
}

/// Formats whose data is a GDI handle or owner-drawn, not a block of memory: they cannot be copied as bytes.
/// Windows makes CF_BITMAP / CF_ENHMETAFILE again from the DIB / metafile-picture formats that are kept.
fn keep_format(format: u32) -> bool {
    const NOT_MEMORY: [u32; 8] = [2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E];
    !NOT_MEMORY.contains(&format) && !(0x0300..=0x03FF).contains(&format)
}

/// The app in front: its program's file name (`WhatsApp.exe`) and window class, for the dictation history.
pub fn app_in_front() -> Option<(String, String)> {
    // SAFETY: plain Win32 call.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    Some((exe_name(hwnd).unwrap_or_default(), class_name(hwnd)))
}

fn exe_name(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: plain Win32 calls; `buf` is writable for `len` characters; the handle is closed below.
    let ok = unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let ok =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(process);
        ok
    };
    if ok == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    path.rsplit('\\').next().map(str::to_string)
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: `buf` is writable for its length; a null window gives 0.
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// The window's process runs as administrator, or its token cannot be read (then keys would not reach it either).
fn elevated(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    // SAFETY: plain Win32 calls; the handle is closed below.
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return true;
        }
        let answer = token_elevated(process).unwrap_or(true);
        CloseHandle(process);
        answer
    }
}

fn self_elevated() -> bool {
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing.
    token_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false)
}

fn token_elevated(process: HANDLE) -> Option<bool> {
    let mut token: HANDLE = std::ptr::null_mut();
    let mut info = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut len = 0u32;
    // SAFETY: `info` is a TOKEN_ELEVATION of the size given; the token handle is closed below.
    unsafe {
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut info as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        (ok != 0).then_some(info.TokenIsElevated != 0)
    }
}

/// Waits until no modifier is held (the dictation key itself is up by now: the text comes after the key-up).
fn wait_for_modifiers() -> bool {
    let start = Instant::now();
    loop {
        // SAFETY: plain Win32 call; the high bit means "down now".
        let down = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .any(|&vk| unsafe { GetAsyncKeyState(i32::from(vk)) } < 0);
        if !down {
            return true;
        }
        if start.elapsed() >= MODIFIERS_WAIT {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Presses `keys` in order and releases them in reverse, as one input batch.
fn send(keys: &[u16]) -> bool {
    let event = |vk: u16, flags: u32| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|&vk| event(vk, 0))
        .chain(keys.iter().rev().map(|&vk| event(vk, KEYEVENTF_KEYUP)))
        .collect();
    // SAFETY: `inputs` is a valid array of INPUT of the length and size given.
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    sent as usize == inputs.len()
}

/// Discards anything waiting in our own console's input (a paste that landed in SonicScript's window).
fn flush_own_console() {
    // SAFETY: plain Win32 calls; without a console the handle is invalid and the call just fails.
    unsafe {
        FlushConsoleInputBuffer(GetStdHandle(STD_INPUT_HANDLE));
    }
}

/// The clipboard, opened; closed again when dropped. Another app may hold it for a moment, so opening retries.
struct Clipboard;

impl Clipboard {
    fn open() -> Result<Self, String> {
        for _ in 0..50 {
            // SAFETY: plain Win32 call; no owner window (the data is set at once, never rendered later).
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return Ok(Clipboard);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(format!(
            "could not open the clipboard: {}",
            std::io::Error::last_os_error()
        ))
    }

    /// Empties the clipboard and sets each (format, bytes) pair.
    fn replace(&self, items: &[(u32, Vec<u8>)]) -> Result<(), String> {
        // SAFETY: the clipboard is open (self).
        if unsafe { EmptyClipboard() } == 0 {
            return Err(format!(
                "could not empty the clipboard: {}",
                std::io::Error::last_os_error()
            ));
        }
        for (format, bytes) in items {
            // SAFETY: the block is allocated with room for `bytes` (at least 1 byte), filled, unlocked, then
            // handed to the clipboard, which owns it from then on; it is freed here only if that fails.
            unsafe {
                let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
                if mem.is_null() {
                    return Err("out of memory for the clipboard".into());
                }
                let ptr = GlobalLock(mem).cast::<u8>();
                if ptr.is_null() {
                    GlobalFree(mem);
                    return Err("could not lock clipboard memory".into());
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
                GlobalUnlock(mem);
                if SetClipboardData(*format, mem).is_null() {
                    GlobalFree(mem);
                    return Err(format!(
                        "could not set the clipboard: {}",
                        std::io::Error::last_os_error()
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: the clipboard was opened by `open`.
        unsafe {
            CloseClipboard();
        }
    }
}

/// Puts `text` on the clipboard for the user to paste (kept out of clipboard history, like a dictation).
pub fn copy(text: &str) -> Result<(), String> {
    set_text(&text.replace("\r\n", "\n").replace('\n', "\r\n")).map(|_| ())
}

/// The text on the clipboard (`--words-paste`).
pub fn clipboard_text() -> Result<String, String> {
    let _open = Clipboard::open()?;
    // SAFETY: the clipboard is open; the handle stays the clipboard's (only read, never freed); the text is a
    // NUL-terminated UTF-16 string within the block's size.
    unsafe {
        let mem = GetClipboardData(u32::from(CF_UNICODETEXT));
        if mem.is_null() {
            return Err("there is no text on the clipboard: copy the list first".into());
        }
        let ptr = GlobalLock(mem).cast::<u16>();
        if ptr.is_null() {
            return Err("could not read the clipboard".into());
        }
        let max = GlobalSize(mem) / 2;
        let units = std::slice::from_raw_parts(ptr, max);
        let len = units.iter().position(|&u| u == 0).unwrap_or(max);
        let text = String::from_utf16_lossy(&units[..len]);
        GlobalUnlock(mem);
        Ok(text)
    }
}

/// Every format on the clipboard that is a block of memory (text, rich text, HTML, images as DIB, files).
fn save() -> Result<Vec<(u32, Vec<u8>)>, String> {
    let _open = Clipboard::open()?;
    let mut items = Vec::new();
    let mut total = 0usize;
    let mut format = 0u32;
    loop {
        // SAFETY: the clipboard is open; 0 starts the list and 0 ends it.
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        if !keep_format(format) {
            continue;
        }
        // SAFETY: the clipboard is open; the handle stays the clipboard's (only read, never freed).
        unsafe {
            let mem = GetClipboardData(format);
            if mem.is_null() {
                continue;
            }
            let size = GlobalSize(mem);
            if size == 0 || total + size > SAVE_LIMIT {
                continue;
            }
            let ptr = GlobalLock(mem).cast::<u8>();
            if ptr.is_null() {
                continue;
            }
            items.push((format, std::slice::from_raw_parts(ptr, size).to_vec()));
            GlobalUnlock(mem);
            total += size;
        }
    }
    Ok(items)
}

fn restore(items: &[(u32, Vec<u8>)]) -> Result<(), String> {
    Clipboard::open()?.replace(items)
}

/// Sets the text, marked so clipboard history and cloud clipboard skip it; returns the clipboard's sequence
/// number afterwards, to see later whether anything else was copied.
fn set_text(text: &str) -> Result<u32, String> {
    let utf16: Vec<u8> = text
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    let zero = 0u32.to_le_bytes().to_vec();
    let mut items = vec![(u32::from(CF_UNICODETEXT), utf16)];
    for (name, data) in [
        ("ExcludeClipboardContentFromMonitorProcessing", Vec::new()),
        ("CanIncludeInClipboardHistory", zero.clone()),
        ("CanUploadToCloudClipboard", zero),
    ] {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is a NUL-terminated UTF-16 string.
        let format = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
        if format != 0 {
            items.push((format, data));
        }
    }
    Clipboard::open()?.replace(&items)?;
    // SAFETY: plain Win32 call (after the clipboard is closed again).
    Ok(unsafe { GetClipboardSequenceNumber() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminals_paste_with_ctrl_shift_v_everything_else_with_ctrl_v() {
        assert_eq!(
            paste_keys("CASCADIA_HOSTING_WINDOW_CLASS"),
            &[VK_CONTROL, VK_SHIFT, VK_V]
        );
        assert_eq!(
            paste_keys("ConsoleWindowClass"),
            &[VK_CONTROL, VK_SHIFT, VK_V]
        );
        assert_eq!(paste_keys("Chrome_WidgetWin_1"), &[VK_CONTROL, VK_V]);
        assert_eq!(paste_keys(""), &[VK_CONTROL, VK_V]);
    }

    #[test]
    fn lists_keep_their_lines_except_in_an_old_console() {
        let list = "Aaj ke kaam:\n- Doodh lana\n- Bill pay karna ";
        assert_eq!(
            for_window(list, "Chrome_WidgetWin_1"),
            "Aaj ke kaam:\r\n- Doodh lana\r\n- Bill pay karna "
        );
        assert_eq!(
            for_window(list, "CASCADIA_HOSTING_WINDOW_CLASS"),
            "Aaj ke kaam:\r\n- Doodh lana\r\n- Bill pay karna "
        );
        assert_eq!(
            for_window(list, "ConsoleWindowClass"),
            "Aaj ke kaam: - Doodh lana - Bill pay karna"
        );
        assert_eq!(for_window("a\r\nb", "Notepad"), "a\r\nb");
    }

    #[test]
    fn handle_formats_are_not_saved_as_bytes() {
        assert!(keep_format(u32::from(CF_UNICODETEXT)));
        assert!(keep_format(8)); // CF_DIB: images survive as a DIB
        assert!(keep_format(15)); // CF_HDROP: copied files
        assert!(keep_format(0xC0FF)); // registered formats (HTML, rich text)
        assert!(!keep_format(2)); // CF_BITMAP
        assert!(!keep_format(14)); // CF_ENHMETAFILE
        assert!(!keep_format(0x0300)); // CF_GDIOBJFIRST
    }

    /// Uses the real clipboard: run by hand (`cargo test -p sonicscript -- --ignored`), not in CI.
    #[test]
    #[ignore]
    fn the_users_clipboard_comes_back_after_our_text() {
        let wide = |s: &str| -> Vec<u8> {
            s.encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect()
        };
        let before = save().unwrap(); // the tester's real clipboard, put back at the end
        Clipboard::open()
            .unwrap()
            .replace(&[(13, wide("mine"))])
            .unwrap();
        let saved = save().unwrap();
        let seq = set_text("dictated").unwrap();
        // SAFETY: plain Win32 call.
        assert_eq!(seq, unsafe { GetClipboardSequenceNumber() });
        restore(&saved).unwrap();
        let back = save().unwrap();
        restore(&before).unwrap();
        let text = back
            .iter()
            .find(|(f, _)| *f == 13)
            .map(|(_, b)| b.clone())
            .unwrap();
        assert_eq!(text, wide("mine"));
    }
}
