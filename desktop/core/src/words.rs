//! The user's own words ("My words", plan `docs/plans/2026-11-04-word-list.md`): names, projects, brands and
//! spellings a speech model cannot guess. One plain format everywhere (the file, a chatbot's answer, a pasted
//! list), read by a forgiving parser so a list from anywhere works:
//!
//! ```text
//! # Group name             (comment)
//! Taskvane = Taskman, Task vein          (after "=": how it gets misheard; it is replaced by the word)
//! Claude
//! ```
//!
//! The list is used in three places: the local cleanup (`normalize.rs`: exact, mishearing and sound-alike
//! replacements), the Gemini audio prompt (the first [`AUDIO_PROMPT_MAX`] words) and the format step (the
//! words that come close to the transcript when the list is long). Never in the Whisper prompt (D-014).

use std::collections::HashSet;

use crate::normalize::skeleton;

/// Words sent with the audio to Gemini (Flash, Flash-Lite): more cost tokens and time for little gain.
pub const AUDIO_PROMPT_MAX: usize = 300;
/// Words sent to the format step (Groq, 8K tokens a minute): up to this many the whole list goes; a longer
/// list sends only the words that come close to the transcript.
pub const FORMAT_PROMPT_MAX: usize = 40;

/// The guidance at the top of a new `words.txt` (the "My words" box until the settings page exists).
pub const GUIDE: &str = "\
# My words: names, projects, brands and spellings that SonicScript should always write your way.
#
# One word or name per line. Optional: after \"=\", how it gets misheard (comma-separated); those are
# replaced by your spelling.
#
#   Claude = McCloud, Cloud
#   Taskvane = Taskman, Task vein
#   Visual Studio Code
#   hun = hoon                (a spelling you prefer)
#
# Lines starting with # are notes. Lists from anywhere also work (bullets, numbers, commas).
# More ways to add words: sonicscript --words-prompt (ask your AI assistant), --words-add <file>,
# --words-paste (from the clipboard), --words-suggest (from your dictation history).
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The spelling to write.
    pub word: String,
    /// What it gets misheard as; replaced by [`Self::word`].
    pub heard_as: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WordList {
    pub entries: Vec<Entry>,
}

/// What a list from somewhere gave: the words, and the lines that were not taken as words (shown to the user).
#[derive(Debug, Default)]
pub struct Parsed {
    pub list: WordList,
    pub skipped: Vec<String>,
}

/// What adding a list to the user's list would change.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Merge {
    /// New words (with their mishearings).
    pub new: Vec<Entry>,
    /// Words already in the list that get new mishearings: (word, the new ones).
    pub more_heard_as: Vec<Entry>,
    /// Words already in the list with nothing new.
    pub already: usize,
}

impl Merge {
    pub fn is_empty(&self) -> bool {
        self.new.is_empty() && self.more_heard_as.is_empty()
    }

    /// The lines to append to the file.
    pub fn lines(&self) -> String {
        self.new
            .iter()
            .chain(&self.more_heard_as)
            .map(|e| e.line() + "\n")
            .collect()
    }
}

impl Entry {
    /// The entry in the file format.
    pub fn line(&self) -> String {
        if self.heard_as.is_empty() {
            self.word.clone()
        } else {
            format!("{} = {}", self.word, self.heard_as.join(", "))
        }
    }
}

/// Reads a list in our format or close to it: bullets, numbers, quotes, bold, code fences, "Word: note" and
/// "Word - note" are cleaned up; lines without "=" may hold several words split by commas; headings
/// ("Here is your list:") and sentences are skipped. Duplicates are merged (first spelling wins).
pub fn parse(text: &str) -> Parsed {
    let mut out = Parsed::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("```") {
            continue;
        }
        let line = strip_marker(line);
        let line = line.trim_matches(|c: char| c == '*' || c == '_').trim();
        if line.is_empty() {
            continue;
        }
        let (word_part, heard) = match split_heard(line) {
            Some((w, h)) => (w, Some(h)),
            None => (line, None),
        };
        let words: Vec<&str> = match heard {
            Some(_) => vec![word_part],
            None if word_part.ends_with(':') => {
                out.skipped.push(raw.trim().to_string());
                continue;
            }
            None => {
                let note_cut = cut_note(word_part);
                note_cut.split([',', ';']).collect()
            }
        };
        let heard_as: Vec<String> = heard
            .map(|h| h.split([',', ';', '/']).filter_map(clean).collect())
            .unwrap_or_default();
        let mut took = false;
        for w in words {
            match clean(w) {
                Some(word) => {
                    out.list.add(Entry {
                        word,
                        heard_as: heard_as.clone(),
                    });
                    took = true;
                }
                None if !w.trim().is_empty() => out.skipped.push(w.trim().to_string()),
                None => {}
            }
        }
        if !took && heard.is_some() {
            out.skipped.push(raw.trim().to_string());
        }
    }
    out
}

/// "- ", "* ", "• ", "1. ", "2) " at the start of a line.
fn strip_marker(line: &str) -> &str {
    let l = line
        .trim_start_matches(['-', '*', '•', '+', '–', '—'])
        .trim_start();
    if l.len() != line.len() {
        return l;
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits < 4 {
        let rest = &line[digits..];
        if let Some(r) = rest.strip_prefix(['.', ')']) {
            return r.trim_start();
        }
    }
    line
}

/// "Word = a, b" (also "Word → a" and "Word -> a").
fn split_heard(line: &str) -> Option<(&str, &str)> {
    for sep in ["=", "→", "->"] {
        if let Some((w, h)) = line.split_once(sep) {
            return Some((w.trim(), h.trim()));
        }
    }
    None
}

/// "Word: note", "Word - note", "Word (note)": keeps the word.
fn cut_note(text: &str) -> &str {
    let mut end = text.len();
    for sep in [": ", " - ", " – ", " — ", " ("] {
        if let Some(i) = text.find(sep) {
            end = end.min(i);
        }
    }
    &text[..end]
}

/// A word or name as written in the list, or `None` when it is not one (empty, a sentence, too long).
fn clean(text: &str) -> Option<String> {
    let t = text
        .trim()
        .trim_matches(|c: char| "\"'`*_.,;:!?".contains(c) || c == '“' || c == '”')
        .trim();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let words = t.split(' ').count();
    let ok = !t.is_empty()
        && words <= 5
        && t.chars().count() <= 40
        && t.chars().any(char::is_alphanumeric);
    ok.then_some(t)
}

impl WordList {
    /// The list in a file (our format or close to it). Skipped lines are ignored.
    pub fn from_text(text: &str) -> Self {
        parse(text).list
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn position(&self, word: &str) -> Option<usize> {
        let lower = word.to_lowercase();
        self.entries
            .iter()
            .position(|e| e.word.to_lowercase() == lower)
    }

    /// Adds an entry, or its new mishearings to the same word already in the list.
    fn add(&mut self, entry: Entry) {
        match self.position(&entry.word) {
            Some(i) => {
                let have = &mut self.entries[i].heard_as;
                for h in entry.heard_as {
                    if !have.iter().any(|x| x.eq_ignore_ascii_case(&h)) {
                        have.push(h);
                    }
                }
            }
            None => self.entries.push(entry),
        }
    }

    /// What adding `other` would change (nothing is changed here).
    pub fn merge(&self, other: &WordList) -> Merge {
        let mut m = Merge::default();
        for e in &other.entries {
            match self.position(&e.word) {
                None => m.new.push(e.clone()),
                Some(i) => {
                    let have = &self.entries[i].heard_as;
                    let more: Vec<String> = e
                        .heard_as
                        .iter()
                        .filter(|h| !have.iter().any(|x| x.eq_ignore_ascii_case(h)))
                        .cloned()
                        .collect();
                    if more.is_empty() {
                        m.already += 1;
                    } else {
                        m.more_heard_as.push(Entry {
                            word: self.entries[i].word.clone(),
                            heard_as: more,
                        });
                    }
                }
            }
        }
        m
    }

    /// The words for a prompt's `{word_list}` slot, or "" for an empty list. With a `transcript` and a list
    /// longer than `max`, only the words that come close to it: the word or a mishearing is in it, or a word
    /// in it (or two run together) sounds the same.
    pub fn prompt_line(&self, transcript: Option<&str>, max: usize) -> String {
        let chosen: Vec<&Entry> = match transcript {
            Some(t) if self.entries.len() > max => {
                let near = Near::new(t);
                self.entries
                    .iter()
                    .filter(|e| near.matches(e))
                    .take(max)
                    .collect()
            }
            _ => self.entries.iter().take(max).collect(),
        };
        if chosen.is_empty() {
            return String::new();
        }
        let items: Vec<String> = chosen
            .iter()
            .map(|e| {
                if e.heard_as.is_empty() {
                    e.word.clone()
                } else {
                    format!("{} (may come out as {})", e.word, e.heard_as.join(", "))
                }
            })
            .collect();
        // The same opening as `eval` `load_prompt`.
        format!(
            "Spell these words exactly like this when you hear them: {}.\n\n",
            items.join(", ")
        )
    }
}

/// The transcript's words, for [`WordList::prompt_line`].
struct Near {
    text: String,
    skeletons: HashSet<String>,
}

impl Near {
    fn new(transcript: &str) -> Self {
        let words: Vec<String> = transcript
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect();
        let mut skeletons = HashSet::new();
        for (i, w) in words.iter().enumerate() {
            if let Some((s, _)) = skeleton(w) {
                skeletons.insert(s);
            }
            if let Some(next) = words.get(i + 1)
                && let Some((s, _)) = skeleton(&format!("{w}{next}"))
            {
                skeletons.insert(s);
            }
        }
        Self {
            text: format!(" {} ", words.join(" ")),
            skeletons,
        }
    }

    fn matches(&self, e: &Entry) -> bool {
        let has = |s: &str| {
            let w: Vec<String> = s
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .map(str::to_lowercase)
                .collect();
            !w.is_empty() && self.text.contains(&format!(" {} ", w.join(" ")))
        };
        has(&e.word)
            || e.heard_as.iter().any(|h| has(h))
            || skeleton(&e.word.replace(' ', "")).is_some_and(|(s, _)| self.skeletons.contains(&s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(l: &WordList) -> Vec<&str> {
        l.entries.iter().map(|e| e.word.as_str()).collect()
    }

    #[test]
    fn our_format_with_mishearings_and_notes() {
        let p = parse(&format!(
            "{GUIDE}\n# Projects\nTaskvane = Taskman, Task vein\nClaude\nclaude = McCloud\n\nVS   Code\n"
        ));
        assert_eq!(words(&p.list), ["Taskvane", "Claude", "VS Code"]);
        assert_eq!(p.list.entries[0].heard_as, ["Taskman", "Task vein"]);
        assert_eq!(p.list.entries[1].heard_as, ["McCloud"], "duplicates merge");
        assert!(p.skipped.is_empty(), "{:?}", p.skipped);
    }

    #[test]
    fn a_chatbot_answer_or_any_list_works_too() {
        let p = parse(
            "Here is your personal word list:\n\
             ```\n\
             1. **Ahsan**\n\
             2) Bilal - my cousin\n\
             - SonicScript: the dictation app\n\
             * GitHub, Gemini; Groq\n\
             • \"Lahore\"\n\
             Taskvane → Taskman\n\
             ```\n\
             I hope this helps you set up the app for daily use today.\n",
        );
        assert_eq!(
            words(&p.list),
            [
                "Ahsan",
                "Bilal",
                "SonicScript",
                "GitHub",
                "Gemini",
                "Groq",
                "Lahore",
                "Taskvane"
            ]
        );
        assert_eq!(p.list.entries[7].heard_as, ["Taskman"]);
        assert_eq!(p.skipped.len(), 2, "{:?}", p.skipped);
    }

    #[test]
    fn merging_finds_new_words_and_new_mishearings() {
        let mine = WordList::from_text("Claude = McCloud\nTaskvane");
        let m = mine.merge(&WordList::from_text(
            "claude = Cloud, McCloud\nTaskvane\nRelay",
        ));
        assert_eq!(m.already, 1);
        assert_eq!(m.new.len(), 1);
        assert_eq!(m.lines(), "Relay\nClaude = Cloud\n");
    }

    #[test]
    fn short_lists_go_whole_long_lists_only_near_words() {
        let short = WordList::from_text("Claude = McCloud\nTaskvane");
        assert_eq!(
            short.prompt_line(Some("kuch bhi"), 40),
            "Spell these words exactly like this when you hear them: Claude (may come out as McCloud), Taskvane.\n\n"
        );
        assert_eq!(WordList::default().prompt_line(None, 40), "");

        let mut long = String::new();
        for i in 0..50 {
            long += &format!("Name{i}\n");
        }
        long += "Claude = McCloud\nTaskvane\nGemini\n";
        let long = WordList::from_text(&long);
        let line = long.prompt_line(Some("maine McCloud bola, task vane aur jaminai"), 40);
        assert!(line.contains("Claude (may come out as McCloud)"), "{line}");
        assert!(
            line.contains("Taskvane") && line.contains("Gemini"),
            "{line}"
        );
        assert!(!line.contains("Name1"), "{line}");
    }
}
