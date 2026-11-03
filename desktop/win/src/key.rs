//! Where API keys come from: this process's environment first, then the Windows user environment
//! (`HKCU\Environment`), which a console opened before the key was set does not see yet. Same order as
//! `eval` `load_env`, without `eval/.env`. Keys are never printed or logged.

use sonicscript_core::api::ApiError;
use sonicscript_core::client::{Client, FALLBACK_AFTER, Racer};
use sonicscript_core::spec::{self, Engine};

/// What the app runs: the primary engine and the one asked when it is slow or fails (D-028).
pub type Engines = Racer<Client>;

pub struct Choice {
    pub primary: Engine,
    pub fallback: Option<Engine>,
}

/// `--engine <id>` runs that engine alone (to measure or try one); without it, the spec's default with its
/// fallback.
pub fn choose(id: Option<&str>) -> Result<Choice, String> {
    match id {
        None => Ok(Choice {
            primary: spec::default_engine(),
            fallback: spec::fallback_engine(),
        }),
        Some(id) => spec::engine(id)
            .map(|primary| Choice {
                primary,
                fallback: None,
            })
            .ok_or_else(|| {
                format!(
                    "unknown engine {id}; the app can run: {}",
                    spec::engine_ids().join(", ")
                )
            }),
    }
}

/// Clients for the chosen engines. When one of the pair has no key, the other runs alone and a one-line
/// notice says so (stderr).
pub fn engines(choice: Choice) -> Result<Engines, String> {
    let id = choice.primary.id().to_string();
    let primary = Client::new(choice.primary, read);
    let Some(fallback) = choice.fallback else {
        return primary
            .map(|p| Racer::new(p, None, FALLBACK_AFTER))
            .map_err(|e| format!("{id}: {e}"));
    };
    let fallback_id = fallback.id().to_string();
    match (primary, Client::new(fallback, read)) {
        (Ok(p), Ok(f)) => Ok(Racer::new(p, Some(f), FALLBACK_AFTER)),
        (Ok(p), Err(e)) => {
            eprintln!("  Note: no fallback ({fallback_id}: {e}); using {id} alone.");
            Ok(Racer::new(p, None, FALLBACK_AFTER))
        }
        (Err(e @ ApiError::MissingKey(_)), Ok(f)) => {
            eprintln!("  Note: {id} needs a key ({e}); using {fallback_id} alone.");
            Ok(Racer::new(f, None, FALLBACK_AFTER))
        }
        (Err(e), _) => Err(format!("{id}: {e}")),
    }
}

/// "gemini-3.5-flash-lite, then groq-whisper-hint after 3 s" for the console.
pub fn describe(engines: &Engines) -> String {
    let id = engines.primary().engine().id();
    match engines.fallback() {
        Some(f) => format!(
            "{id}, then {} after {} s",
            f.engine().id(),
            FALLBACK_AFTER.as_secs()
        ),
        None => id.to_string(),
    }
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
