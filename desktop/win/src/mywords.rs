//! "My words" for the command-line app (D-034, plan `docs/plans/2026-11-04-word-list.md`) until the settings
//! page: the list is `%APPDATA%\SonicScript\words.txt` (created with the format guide at the top), and these
//! commands fill it, each showing what it found and asking before anything is added:
//!
//! - `--words`: opens the file in Notepad (type or paste words there);
//! - `--words-prompt`: copies the prompt for the user's own AI assistant (Claude, ChatGPT, Gemini, ...), which
//!   writes their word list from what it knows about them, or collects words they dictate to it;
//! - `--words-add <file>...`: adds a list from text files (any list works: bullets, commas, a chatbot answer);
//! - `--words-paste`: the same from the clipboard (copy the chatbot's answer, then run this);
//! - `--words-suggest`: names that came up in the dictation history and are not in the list yet.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use sonicscript_core::words::{self, Merge, WordList};

use crate::inject;

const ASSISTANT_PROMPT: &str = include_str!("../../../spec/prompts/words-assistant-v1.md");

/// `%APPDATA%\SonicScript\words.txt`.
pub fn path() -> Result<PathBuf, String> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or("APPDATA is not set")?;
    Ok(base.join("SonicScript").join("words.txt"))
}

/// The file, created with the format guide when it does not exist yet.
fn ensure() -> Result<PathBuf, String> {
    let p = path()?;
    if !p.exists() {
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(&p, words::GUIDE).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(p)
}

/// The user's list for dictation; an empty list when there is no file yet.
pub fn load() -> WordList {
    path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| WordList::from_text(&t))
        .unwrap_or_default()
}

/// One line for the start of dictation.
pub fn summary(list: &WordList) -> String {
    match list.len() {
        0 => "No words of your own yet: sonicscript --words to add names and terms it should get right."
            .into(),
        n => format!("Your words: {n} (sonicscript --words to edit; restart after a change)."),
    }
}

/// Runs a `--words*` command; `None` when `args` is not one.
pub fn command(args: &[String]) -> Option<Result<(), String>> {
    let rest = &args[1..];
    Some(match (args.first()?.as_str(), rest) {
        ("--words", []) => open(),
        ("--words-prompt", []) => prompt(),
        ("--words-add", files) if !files.is_empty() => add_files(files),
        ("--words-paste", []) => {
            inject::clipboard_text().and_then(|t| add_text(&t, "the clipboard"))
        }
        ("--words-suggest", []) => suggest(),
        _ => return None,
    })
}

fn open() -> Result<(), String> {
    let p = ensure()?;
    let n = load().len();
    println!("Your words: {n}, in {}", p.display());
    println!(
        "Opening it in Notepad. One word per line; \"Word = how it gets misheard\" also works."
    );
    println!("Save the file, then restart SonicScript to use the changes.");
    std::process::Command::new("notepad.exe")
        .arg(&p)
        .spawn()
        .map_err(|e| format!("could not open Notepad: {e}"))?;
    Ok(())
}

fn prompt() -> Result<(), String> {
    let text = ASSISTANT_PROMPT.trim();
    inject::copy(text)?;
    println!("{text}\n");
    println!("The prompt above is on your clipboard. Next:");
    println!(
        "  1. Paste it into the AI assistant you use every day (Claude, ChatGPT, Gemini, ...)."
    );
    println!(
        "     It writes your word list from what it knows about you. Or type \"collect\" and say your"
    );
    println!("     words to it one by one, then \"done\".");
    println!("  2. Copy its answer and run: sonicscript --words-paste");
    Ok(())
}

fn add_files(files: &[String]) -> Result<(), String> {
    let mut text = String::new();
    for f in files {
        text += &std::fs::read_to_string(Path::new(f)).map_err(|e| format!("{f}: {e}"))?;
        text.push('\n');
    }
    add_text(&text, &files.join(", "))
}

/// Shows what `text` adds to the list and appends it when the user says yes.
fn add_text(text: &str, from: &str) -> Result<(), String> {
    let parsed = words::parse(text);
    let mine = load();
    let merge = mine.merge(&parsed.list);
    println!(
        "From {from}: {} words found, {} new, {} with new mishearings, {} already in your list.",
        parsed.list.len(),
        merge.new.len(),
        merge.more_heard_as.len(),
        merge.already
    );
    if !parsed.skipped.is_empty() {
        println!("  Not taken as words (sentences or headings):");
        for s in parsed.skipped.iter().take(10) {
            println!("    {s}");
        }
    }
    if merge.is_empty() {
        println!("Nothing to add.");
        return Ok(());
    }
    show(&merge);
    if ask("Add them to your list? [Y/n] ")?
        .trim()
        .eq_ignore_ascii_case("n")
    {
        println!("Nothing added.");
        return Ok(());
    }
    append(&merge)
}

fn show(merge: &Merge) {
    for e in &merge.new {
        println!("  + {}", e.line());
    }
    for e in &merge.more_heard_as {
        println!("  ~ {}", e.line());
    }
}

fn append(merge: &Merge) -> Result<(), String> {
    let p = ensure()?;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&p)
        .map_err(|e| format!("{}: {e}", p.display()))?;
    let old = std::fs::read_to_string(&p).unwrap_or_default();
    let sep = if old.ends_with('\n') || old.is_empty() {
        ""
    } else {
        "\n"
    };
    write!(f, "{sep}{}", merge.lines()).map_err(|e| format!("{}: {e}", p.display()))?;
    println!(
        "Added. Your list now has {} words ({}). Restart SonicScript to use them.",
        load().len(),
        p.display()
    );
    Ok(())
}

fn ask(question: &str) -> Result<String, String> {
    print!("{question}");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line)
}

/// Names from the dictation history that are not in the list yet, most frequent first; the user picks.
fn suggest() -> Result<(), String> {
    let history = crate::history::History::open()?;
    let index = history.dir().join("history.jsonl");
    let text = std::fs::read_to_string(&index).unwrap_or_default();
    let texts: Vec<String> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["text"].as_str().map(str::to_string))
        .collect();
    let mine = load();
    let found = names(&texts, &mine);
    if found.is_empty() {
        println!(
            "No new names in your {} dictations yet. Dictate for a while, then try again.",
            texts.len()
        );
        return Ok(());
    }
    println!(
        "Names from your {} dictations that are not in your list (times seen):",
        texts.len()
    );
    for (i, (name, n)) in found.iter().enumerate() {
        println!("  {:>2}. {name} ({n})", i + 1);
    }
    println!(
        "Only add the ones spelled right. A wrong one (\"McCloud\" for Claude) is better added as\n\
         \"Claude = McCloud\" with sonicscript --words."
    );
    let answer = ask("Numbers to add (e.g. 1 3 4), \"all\", or Enter for none: ")?;
    let picked: Vec<&str> = if answer.trim().eq_ignore_ascii_case("all") {
        found.iter().map(|(w, _)| w.as_str()).collect()
    } else {
        answer
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|n| n.parse::<usize>().ok())
            .filter_map(|n| found.get(n.wrapping_sub(1)).map(|(w, _)| w.as_str()))
            .collect()
    };
    if picked.is_empty() {
        println!("Nothing added.");
        return Ok(());
    }
    let merge = mine.merge(&WordList::from_text(&picked.join("\n")));
    show(&merge);
    append(&merge)
}

/// Capitalised words inside a sentence (not its first word), or with a capital inside ("GitHub"), seen in
/// the texts and not in the list; at most 30, most frequent first.
fn names(texts: &[String], mine: &WordList) -> Vec<(String, usize)> {
    const COMMON: &[&str] = &[
        "I", "I'm", "I'll", "I've", "OK", "Okay", "Hello", "Hi", "Bro", "Bhai", "Yaar", "Haan",
        "Acha", "Theek", "Sorry", "Thanks", "Please", "English", "Urdu", "Hindi",
    ];
    let have: Vec<String> = mine
        .entries
        .iter()
        .flat_map(|e| std::iter::once(&e.word).chain(&e.heard_as))
        .map(|w| w.to_lowercase())
        .collect();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for text in texts {
        let mut sentence_start = true;
        for raw in text.split_whitespace() {
            let word = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
            let starts = sentence_start;
            sentence_start = raw.ends_with(['.', '?', '!', ':']) || raw.starts_with('-');
            if word.chars().count() < 3 || !word.chars().all(|c| c.is_alphanumeric() || c == '\'') {
                continue;
            }
            let first_upper = word.chars().next().is_some_and(char::is_uppercase);
            let inner_upper = word.chars().skip(1).any(char::is_uppercase);
            if !(inner_upper || (first_upper && !starts)) {
                continue;
            }
            if COMMON.iter().any(|c| c.eq_ignore_ascii_case(word))
                || have.contains(&word.to_lowercase())
            {
                continue;
            }
            *counts.entry(word.to_string()).or_default() += 1;
        }
    }
    let mut found: Vec<(String, usize)> = counts.into_iter().collect();
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    found.truncate(30);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_skip_sentence_starts_common_words_and_the_list() {
        let texts = vec![
            "Dekh, maine Claude bola tha, isne McCloud likha hai. Bhai, McDonald's chal raha hai?"
                .to_string(),
            "Main Claude pe kaam kar raha hu. Okay, GitHub pe PR banao.".to_string(),
        ];
        let mine = WordList::from_text("McCloud");
        let found = names(&texts, &mine);
        assert_eq!(
            found,
            [
                ("Claude".to_string(), 2),
                ("GitHub".to_string(), 1),
                ("McDonald's".to_string(), 1)
            ]
        );
    }

    #[test]
    fn the_assistant_prompt_asks_for_our_format() {
        assert!(ASSISTANT_PROMPT.contains("dictation"));
        assert!(ASSISTANT_PROMPT.contains("Word = how it might be misheard"));
        // What the prompt shows as the format is read by our parser as words.
        let p = words::parse("# Group name\nWord\nWord2 = misheard, another");
        assert_eq!(p.list.len(), 2);
    }
}
