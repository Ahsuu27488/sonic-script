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
    /// The smart-formatting step run after whichever engine answers (D-030); `None` without `--format` (D-041).
    pub format: Option<spec::Refine>,
    /// The user's words (D-034), for dictation and `--file`; `None` for `--bench` (comparable runs).
    pub words: Option<Arc<WordList>>,
}

/// `--engine <id>` runs that engine alone (to measure or try one); without it, the spec's chain.
/// `format: false` (the default since D-041; `--format` turns it on) gives the exact words.
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

/// The engines from the user's settings (D-045): the main model and its fallbacks, each waited for as long as
/// the spec's chain waits at that place (4 s, then 3 s); no main model = the spec's chain. `--engine` wins
/// over the settings. An id the spec no longer has gives the spec's chain and a notice, never an error (a model
/// renamed after an update must not stop dictation).
pub fn choose_from(
    s: &crate::settings::Settings,
    engine_override: Option<&str>,
    format: bool,
) -> Result<(Choice, Vec<String>), String> {
    if engine_override.is_some() {
        return Ok((choose(engine_override, format)?, Vec::new()));
    }
    let Some(main) = &s.main_model else {
        return Ok((choose(None, format)?, Vec::new()));
    };
    let mut ids: Vec<&str> = Vec::new();
    for id in std::iter::once(main).chain(&s.fallbacks) {
        if !ids.contains(&id.as_str()) {
            ids.push(id);
        }
    }
    let unknown: Vec<&str> = ids
        .iter()
        .copied()
        .filter(|id| spec::engine(id).is_none())
        .collect();
    if !unknown.is_empty() {
        let note = format!(
            "Settings: {} is not a model this version knows (list: sonicscript --models): using the default chain.",
            unknown.join(", ")
        );
        return Ok((choose(None, format)?, vec![note]));
    }
    let waits: Vec<Duration> = spec::chain().iter().map(|l| l.wait).collect();
    let links = ids
        .iter()
        .filter_map(|id| spec::engine(id))
        .enumerate()
        .map(|(i, e)| {
            let wait = waits.get(i).copied().unwrap_or(Duration::from_secs(3));
            (e, wait, false)
        })
        .collect();
    Ok((
        Choice {
            links,
            format: if format { spec::format() } else { None },
            words: None,
        },
        Vec::new(),
    ))
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
    // D-039: an answer in Urdu / Devanagari script is made Roman by this step, or not pasted at all.
    let romanize = spec::romanize().filter(|r| !read(&r.provider.key_env).is_empty());
    let mut links = Vec::new();
    let mut first_error = None;
    for (engine, wait, last_resort) in choice.links {
        let id = engine.id().to_string();
        let client = Client::new(engine, read)
            .and_then(|c| match &format {
                Some(f) => c.with_format(f.clone(), read),
                None => Ok(c),
            })
            .and_then(|c| match &romanize {
                Some(r) => c.with_romanize(r.clone(), read),
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
    let chain = Chain::with_last_resort(links);
    Ok(match rest_file() {
        Some(path) => chain.with_rest_file(path),
        None => chain,
    })
}

/// `%LOCALAPPDATA%\SonicScript\resting.json`: models out of free requests stay skipped after a restart
/// (D-040).
fn rest_file() -> Option<std::path::PathBuf> {
    let dir = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("SonicScript");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("resting.json"))
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

/// A key: the process environment first (a developer's setup keeps working), then Windows Credential Manager
/// (where the app saves keys, D-046), then the Windows user environment.
fn read(var: &str) -> String {
    let env = |v: &str| std::env::var(v).ok();
    let store = |v: &str| crate::keystore::get(v);
    let user = |v: &str| user_env(v);
    read_from(&[&env, &store, &user], var)
}

/// One place a key can come from.
type Source<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The first non-blank value from `sources`, in order; "" when none has one.
fn read_from(sources: &[Source], var: &str) -> String {
    sources
        .iter()
        .find_map(|get| get(var).filter(|k| !k.trim().is_empty()))
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
        // D-030: for the chain and for one engine alike; off by default since D-041 (main.rs --format).
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

    #[test]
    fn environment_beats_credential_manager() {
        let env = |_: &str| Some("from-env".to_string());
        let store = |_: &str| Some("from-store".to_string());
        assert_eq!(read_from(&[&env, &store], "K"), "from-env");
        let blank = |_: &str| Some("  ".to_string());
        assert_eq!(read_from(&[&blank, &store], "K"), "from-store");
    }

    #[test]
    fn credential_manager_beats_user_env() {
        let none = |_: &str| None;
        let store = |_: &str| Some("from-store".to_string());
        let user = |_: &str| Some("from-user".to_string());
        assert_eq!(read_from(&[&none, &store, &user], "K"), "from-store");
        assert_eq!(read_from(&[&none, &none, &user], "K"), "from-user");
        assert_eq!(read_from(&[&none, &none, &none], "K"), "");
    }

    use crate::settings::Settings;

    fn ids(c: &Choice) -> Vec<String> {
        c.links.iter().map(|(e, ..)| e.id().to_string()).collect()
    }

    fn with(main: &str, fallbacks: &[&str]) -> Settings {
        Settings {
            main_model: Some(main.into()),
            fallbacks: fallbacks.iter().map(|s| s.to_string()).collect(),
            ..Settings::default()
        }
    }

    #[test]
    fn default_settings_give_the_spec_chain() {
        let (c, notes) = choose_from(&Settings::default(), None, false).unwrap();
        let spec: Vec<_> = spec::chain()
            .iter()
            .map(|l| (l.engine.id().to_string(), l.wait))
            .collect();
        let got: Vec<_> = c
            .links
            .iter()
            .map(|(e, w, _)| (e.id().to_string(), *w))
            .collect();
        assert_eq!(got, spec);
        assert!(notes.is_empty());
    }

    #[test]
    fn main_and_fallbacks_in_order() {
        let (c, notes) = choose_from(
            &with("gemini-3.5-flash-lite", &["gemini-3.5-flash"]),
            None,
            false,
        )
        .unwrap();
        assert_eq!(ids(&c), ["gemini-3.5-flash-lite", "gemini-3.5-flash"]);
        let waits: Vec<_> = c.links.iter().map(|(_, w, _)| w.as_secs()).collect();
        assert_eq!(waits, [4, 3]);
        assert!(notes.is_empty());
    }

    #[test]
    fn unknown_model_falls_back_to_spec_chain() {
        let (c, notes) = choose_from(&with("gemini-9", &[]), None, false).unwrap();
        assert_eq!(ids(&c), ids(&choose(None, false).unwrap()));
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("gemini-9"), "{notes:?}");
    }

    #[test]
    fn duplicates_are_dropped() {
        let s = with(
            "gemini-3.5-flash",
            &["gemini-3.5-flash", "gemini-3.5-flash-lite"],
        );
        let (c, _) = choose_from(&s, None, false).unwrap();
        assert_eq!(ids(&c), ["gemini-3.5-flash", "gemini-3.5-flash-lite"]);
    }

    #[test]
    fn engine_flag_beats_the_settings() {
        let (c, _) = choose_from(
            &with("gemini-3.5-flash", &[]),
            Some("groq-whisper-hint"),
            true,
        )
        .unwrap();
        assert_eq!(ids(&c), ["groq-whisper-hint"]);
        assert!(c.format.is_some());
    }
}
