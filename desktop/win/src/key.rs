//! Where API keys come from: this process's environment first, then the Windows user environment
//! (`HKCU\Environment`), which a console opened before the key was set does not see yet. Same order as
//! `eval` `load_env`, without `eval/.env`. Keys are never printed or logged.

use std::time::Duration;

use sonicscript_core::client::{Chain, Client};
use sonicscript_core::pipeline::Transcribe;
use sonicscript_core::spec;

/// What the app runs: engines in order of preference (D-029).
pub type Engines = Chain<Client>;

/// The engines to run, in order, each with how long to wait for it before also asking the next.
pub struct Choice {
    pub links: Vec<(spec::Engine, Duration)>,
}

/// `--engine <id>` runs that engine alone (to measure or try one); without it, the spec's chain.
pub fn choose(id: Option<&str>) -> Result<Choice, String> {
    let links = match id {
        None => spec::chain()
            .into_iter()
            .map(|l| (l.engine, l.wait))
            .collect(),
        Some(id) => vec![(
            spec::engine(id).ok_or_else(|| {
                format!(
                    "unknown engine {id}; the app can run: {}",
                    spec::engine_ids().join(", ")
                )
            })?,
            Duration::ZERO,
        )],
    };
    Ok(Choice { links })
}

/// Clients for the chosen engines. In a chain, an engine whose key is missing is left out with a one-line
/// notice (stderr); only when none is left is it an error.
pub fn engines(choice: Choice) -> Result<Engines, String> {
    let many = choice.links.len() > 1;
    let mut links = Vec::new();
    let mut first_error = None;
    for (engine, wait) in choice.links {
        let id = engine.id().to_string();
        match Client::new(engine, read) {
            Ok(client) => links.push((client, wait)),
            Err(e) => {
                if many {
                    eprintln!("  Note: {id} left out ({e}).");
                }
                first_error.get_or_insert(format!("{id}: {e}"));
            }
        }
    }
    if links.is_empty() {
        return Err(first_error.unwrap_or_default());
    }
    Ok(Chain::new(links))
}

/// "a, then b after 4 s, then c after 2 s more" for the console.
pub fn describe(engines: &Engines) -> String {
    let mut out = engines.first().id().to_string();
    let mut waits: Vec<Duration> = Vec::new();
    for (engine, wait) in engines.links() {
        if let Some(&prev) = waits.last() {
            let more = if waits.len() > 1 { " more" } else { "" };
            out += &format!(", then {} after {} s{more}", engine.id(), prev.as_secs());
        }
        waits.push(wait);
    }
    out
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
