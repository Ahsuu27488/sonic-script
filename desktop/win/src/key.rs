//! Where API keys come from: this process's environment first, then the Windows user environment
//! (`HKCU\Environment`), which a console opened before the key was set does not see yet. Same order as
//! `eval` `load_env`, without `eval/.env`. Keys are never printed or logged.

use sonicscript_core::api::ApiError;
use sonicscript_core::client::Client;
use sonicscript_core::spec::{self, Engine};

/// The engine to use: `--engine <id>` when given, else the spec's default.
pub fn engine(id: Option<&str>) -> Result<Engine, String> {
    match id {
        None => Ok(spec::default_engine()),
        Some(id) => spec::engine(id).ok_or_else(|| {
            format!(
                "unknown engine {id}; the app can run: {}",
                spec::engine_ids().join(", ")
            )
        }),
    }
}

pub fn client(engine: Engine) -> Result<Client, ApiError> {
    Client::new(engine, read)
}

fn read(var: &str) -> String {
    std::env::var(var)
        .ok()
        .filter(|k| !k.trim().is_empty())
        .or_else(|| user_env(var))
        .unwrap_or_default()
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
