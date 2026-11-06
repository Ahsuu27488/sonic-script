//! One dictation per Windows user (M2 plan task 2.2, Review Focus 4): the CLI's dictation and the tray app
//! both take a named mutex before they install the key hook, so two copies never paste twice.

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

/// The mutex name; `Local\` = this sign-in session.
const NAME: &str = r"Local\SonicScript.Dictation";

/// Holds the claim until dropped (or the process ends).
pub struct Guard(HANDLE);

// SAFETY: a mutex handle is a plain kernel handle, usable and closable from any thread.
unsafe impl Send for Guard {}

impl Drop for Guard {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateMutexW and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// Claims dictation for this process; an error when another SonicScript is already dictating.
pub fn claim() -> Result<Guard, String> {
    claim_named(NAME)
}

fn claim_named(name: &str) -> Result<Guard, String> {
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string; no security attributes, not initially owned (the
    // name alone is the claim, so it works across threads and processes).
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
    if handle.is_null() {
        return Err(format!(
            "could not check for another SonicScript: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: plain Win32 call, read right after CreateMutexW.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // SAFETY: our own handle to the existing mutex, closed once.
        unsafe { CloseHandle(handle) };
        return Err(
            "SonicScript is already running (in the tray or another window): \
                    quit that one first."
                .into(),
        );
    }
    Ok(Guard(handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_claim_is_refused() {
        let name = format!(r"Local\SonicScript.Test.{}", std::process::id());
        let first = claim_named(&name).expect("first claim");
        let second = claim_named(&name);
        assert!(second.is_err(), "a second claim must fail");
        assert!(second.err().unwrap().contains("already running"));
        drop(first);
        assert!(claim_named(&name).is_ok(), "free again once dropped");
    }
}
