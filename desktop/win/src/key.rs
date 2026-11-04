//! Where API keys come from: this process's environment first, then the Windows user environment
//! (`HKCU\Environment`), which a console opened before the key was set does not see yet. Same order as
//! `eval` `load_env`, without `eval/.env`. Keys are never printed or logged.

use std::sync::Arc;
use std::time::Duration;

use sonicscript_core::client::{Chain, Client};
use sonicscript_core::normalize::Normalizer;
use sonicscript_core::pipeline::Transcribe;
use sonicscript_core::spec;
use sonicscript_core::words::WordList;

/// What the app runs: engines in order of preference (D-029).
pub type Engines = Chain<Client>;

/// The engines to run, in order, each with how long to wait for it before also asking the next, and whether
/// it is a last resort (asked only after every engine before it failed, D-033).
pub struct Choice {
    pub links: Vec<(spec::Engine, Duration, bool)>,
    /// The smart-formatting step run after whichever engine answers (D-030); `None` with `--no-format`.
    pub format: Option<spec::Refine>,
    /// The user's words (D-034), for dictation and `--file`; `None` for `--bench` (comparable runs).
    pub words: Option<Arc<WordList>>,
}

/// `--engine <id>` runs that engine alone (to measure or try one); without it, the spec's chain.
/// `format: false` (`--no-format`) gives the exact words.
pub fn choose(id: Option<&str>, format: bool) -> Result<Choice, String> {
    let links = match id {
        None => spec::chain()
            .into_iter()
            .map(|l| (l.engine, l.wait, l.last_resort))
            .collect(),
        Some(id) => vec![(
            spec::engine(id).ok_or_else(|| {
                format!(
                    "unknown engine {id}; the app can run: {}",
                    spec::engine_ids().join(", ")
                )
            })?,
            Duration::ZERO,
            false,
        )],
    };
    let format = if format { spec::format() } else { None };
    Ok(Choice {
        links,
        format,
        words: None,
    })
}

/// The text cleanup with the user's words (D-034), when the choice has them.
pub fn normalizer(choice: &Choice) -> Normalizer {
    match &choice.words {
        Some(w) => Normalizer::new().with_words(w),
        None => Normalizer::new(),
    }
}

/// Clients for the chosen engines. In a chain, an engine whose key is missing is left out with a one-line
/// notice (stderr); only when none is left is it an error. Without the format step's key the text comes out
/// unformatted, with a notice.
pub fn engines(choice: Choice) -> Result<Engines, String> {
    let many = choice.links.len() > 1;
    let format = choice.format.filter(|f| {
        let ok = !read(&f.provider.key_env).is_empty();
        if !ok {
            eprintln!(
                "  Note: no smart formatting ({} is not set).",
                f.provider.key_env
            );
        }
        ok
    });
    let mut links = Vec::new();
    let mut first_error = None;
    for (engine, wait, last_resort) in choice.links {
        let id = engine.id().to_string();
        let client = Client::new(engine, read).and_then(|c| match &format {
            Some(f) => c.with_format(f.clone(), read),
            None => Ok(c),
        });
        match client {
            Ok(client) => {
                let client = match &choice.words {
                    Some(w) if !w.is_empty() => client.with_words(w.clone()),
                    _ => client,
                };
                links.push((client, wait, last_resort));
            }
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
    Ok(Chain::with_last_resort(links))
}

/// "a, then b after 4 s, then c after 3 s more, then d if all of them fail" for the console.
pub fn describe(engines: &Engines) -> String {
    let mut out = engines.first().id().to_string();
    let mut waits: Vec<Duration> = Vec::new();
    for (i, (engine, wait)) in engines.links().enumerate() {
        if engines.is_last_resort(i) && i > 0 {
            out += &format!(", then {} if all of them fail", engine.id());
        } else if let Some(&prev) = waits.last() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_is_on_unless_no_format_is_given() {
        // D-030: on by default, for the chain and for one engine alike; --no-format gives the exact words.
        assert!(choose(None, true).unwrap().format.is_some());
        assert!(
            choose(Some("groq-whisper-hint"), true)
                .unwrap()
                .format
                .is_some()
        );
        assert!(choose(None, false).unwrap().format.is_none());
        assert!(choose(Some("nope"), true).is_err());
    }
}
