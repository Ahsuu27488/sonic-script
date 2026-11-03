//! Where the Groq key comes from: this process's environment first, then the Windows user environment
//! (`HKCU\Environment`), which a console opened before the key was set does not see yet. Same order as
//! `eval` `load_env`, without `eval/.env`. The key is never printed or logged.

use sonicscript_core::groq::{Groq, GroqError};
use sonicscript_core::spec::WhisperEngine;

pub fn groq(engine: WhisperEngine) -> Result<Groq, GroqError> {
    let var = engine.provider.key_env.clone();
    let key = std::env::var(&var)
        .ok()
        .filter(|k| !k.trim().is_empty())
        .or_else(|| user_env(&var))
        .unwrap_or_default();
    Groq::new(engine, key)
}

#[cfg(windows)]
fn user_env(name: &str) -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Environment")
        .ok()?
        .get_value(name)
        .ok()
}

#[cfg(not(windows))]
fn user_env(_name: &str) -> Option<String> {
    None
}
