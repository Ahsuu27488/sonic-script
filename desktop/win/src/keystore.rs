//! API keys in Windows Credential Manager (D-046, blueprint §5.2): one generic credential per key, named
//! `SonicScript/<ENV_NAME>` (e.g. `SonicScript/GEMINI_API_KEY`), readable only by this Windows user. The
//! settings window (M2) and `sonicscript --key <provider>` write them; `key::read` reads them after the
//! process environment. A key is never printed or logged.

use std::io::{self, BufRead, Write};

/// The credential name for a key's environment variable name.
fn target(env_name: &str) -> Vec<u16> {
    format!("SonicScript/{env_name}")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

#[cfg(windows)]
pub fn get(env_name: &str) -> Option<String> {
    use windows_sys::Win32::Security::Credentials::{
        CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW,
    };
    let name = target(env_name);
    let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: `name` is a NUL-terminated UTF-16 string; on success `cred` points to memory the system owns
    // until `CredFree`, and the blob is `CredentialBlobSize` bytes long.
    unsafe {
        if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) == 0 {
            return None;
        }
        let c = &*cred;
        let bytes =
            std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec();
        CredFree(cred.cast());
        String::from_utf8(bytes)
            .ok()
            .filter(|k| !k.trim().is_empty())
    }
}

#[cfg(windows)]
pub fn set(env_name: &str, value: &str) -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredWriteW,
    };
    let value = value.trim();
    if value.is_empty() {
        return Err("the key is empty".into());
    }
    let mut name = target(env_name);
    let mut blob = value.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: name.as_mut_ptr(),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        // SAFETY: every other field of this plain C struct is valid as zero (no comment, no attributes).
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: `cred` points to live buffers for the duration of the call.
    if unsafe { CredWriteW(&cred, 0) } == 0 {
        return Err(format!(
            "Windows Credential Manager refused to save it ({})",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub fn delete(env_name: &str) -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::{CRED_TYPE_GENERIC, CredDeleteW};
    let name = target(env_name);
    // SAFETY: `name` is a NUL-terminated UTF-16 string.
    if unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
        return Err(format!("no saved key ({})", io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn get(_env_name: &str) -> Option<String> {
    None
}

/// The environment variable a provider's key lives in (`spec/providers.json` `key_env`).
fn env_name(provider: &str) -> Option<&'static str> {
    match provider.to_ascii_lowercase().as_str() {
        "gemini" | "google" => Some("GEMINI_API_KEY"),
        "groq" => Some("GROQ_API_KEY"),
        "openai" => Some("OPENAI_API_KEY"),
        _ => None,
    }
}

/// `--key <gemini|groq|openai> [--delete]`: `None` when the arguments are not this command. The key is typed
/// (or pasted) without being shown.
#[cfg(windows)]
pub fn command(args: &[String]) -> Option<Result<(), String>> {
    let (first, rest) = args.split_first()?;
    if first != "--key" {
        return None;
    }
    Some((|| {
        let (provider, delete_it) = match rest {
            [p] => (p, false),
            [p, d] if d == "--delete" => (p, true),
            _ => return Err("usage: sonicscript --key <gemini|groq|openai> [--delete]".into()),
        };
        let var = env_name(provider)
            .ok_or_else(|| format!("unknown provider {provider}: gemini, groq or openai"))?;
        if delete_it {
            delete(var)?;
            println!("Removed the saved {var}.");
            return Ok(());
        }
        print!("Paste your {provider} key and press Enter (it is not shown): ");
        io::stdout().flush().ok();
        let key = read_hidden().map_err(|e| e.to_string())?;
        set(var, &key)?;
        let tail: String = key
            .trim()
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        println!("\nSaved {var} (…{tail}) in Windows Credential Manager.");
        if std::env::var(var).is_ok_and(|v| !v.trim().is_empty()) {
            println!("  Note: {var} is also set in the environment; that one is used first.");
        }
        Ok(())
    })())
}

/// One line from the console with echo off (so the key does not show on screen).
#[cfg(windows)]
fn read_hidden() -> io::Result<String> {
    use windows_sys::Win32::System::Console::{
        ENABLE_ECHO_INPUT, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE, SetConsoleMode,
    };
    // SAFETY: plain console calls on this process's own input handle; the old mode is put back.
    let (handle, old) = unsafe {
        let h = GetStdHandle(STD_INPUT_HANDLE);
        let mut mode = 0;
        let ok = GetConsoleMode(h, &mut mode) != 0;
        if ok {
            SetConsoleMode(h, mode & !ENABLE_ECHO_INPUT);
        }
        (h, ok.then_some(mode))
    };
    let mut line = String::new();
    let result = io::stdin().lock().read_line(&mut line);
    if let Some(mode) = old {
        // SAFETY: restores the mode read above.
        unsafe { SetConsoleMode(handle, mode) };
    }
    result.map(|_| line.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_map_to_their_variables() {
        assert_eq!(env_name("Gemini"), Some("GEMINI_API_KEY"));
        assert_eq!(env_name("groq"), Some("GROQ_API_KEY"));
        assert_eq!(env_name("openai"), Some("OPENAI_API_KEY"));
        assert_eq!(env_name("mistral"), None);
    }

    /// Manual: writes, reads and deletes `SonicScript/SONICSCRIPT_TEST_KEY` in the real Credential Manager.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn real_credential_round_trip() {
        let var = "SONICSCRIPT_TEST_KEY";
        let _ = delete(var);
        assert_eq!(get(var), None);
        set(var, "test-value-1234").unwrap();
        assert_eq!(get(var).as_deref(), Some("test-value-1234"));
        set(var, "test-value-5678").unwrap();
        assert_eq!(get(var).as_deref(), Some("test-value-5678"));
        delete(var).unwrap();
        assert_eq!(get(var), None);
        assert!(delete(var).is_err());
    }
}
