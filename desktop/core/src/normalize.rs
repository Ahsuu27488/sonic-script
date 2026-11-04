//! Cleans Whisper's text before it is pasted (M1 plan step 1): script guard (Urdu / Devanagari letters to
//! Roman), phantom filter (D-007), the user's word list (D-014) and spacing, per line so formatted lists
//! keep their lines (D-030). The rules are data in `spec/normalizer/rules.json` and the test cases are in
//! `spec/normalizer/cases.json`, shared with the Android app; the order of the steps is described in the
//! rules file.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

const RULES: &str = include_str!("../../../spec/normalizer/rules.json");
const WHISPER_HINT: &str = include_str!("../../../spec/prompts/whisper-hint-v1.txt");

// Urdu letters whose reading depends on their place in the word.
const ALEF: char = '\u{0627}';
const ALEF_MADDA: char = '\u{0622}';
const WAW: char = '\u{0648}';
const YEH: char = '\u{06CC}';
const ARABIC_YEH: char = '\u{064A}';
const BARI_YEH: char = '\u{06D2}';
const GOL_HEH: char = '\u{06C1}';
const ARABIC_HEH: char = '\u{0647}';
// Devanagari signs that change the letter before them.
const VIRAMA: char = '\u{094D}';
const NUKTA: char = '\u{093C}';

/// The result of [`Normalizer::clean`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleaned {
    /// Text to paste: empty, or ending in one space so the next dictation does not glue on.
    pub text: String,
    /// So much Urdu / Devanagari script that the letter table only gives a rough draft: romanize the raw
    /// text with the LLM step instead.
    pub needs_llm: bool,
    /// The whole output was a known Whisper invention (the hint echoed, "Thanks for watching"), so `text`
    /// is empty.
    pub phantom: bool,
}

#[derive(Deserialize)]
struct Rules {
    remove: Vec<String>,
    replace: HashMap<String, String>,
    needs_llm_ratio: f64,
    urdu_words: HashMap<String, String>,
    urdu_letters: HashMap<String, String>,
    urdu_marks: HashMap<String, String>,
    hindi_words: HashMap<String, String>,
    devanagari_consonants: HashMap<String, String>,
    devanagari_vowels: HashMap<String, String>,
    devanagari_matras: HashMap<String, String>,
    devanagari_signs: HashMap<String, String>,
    devanagari_nukta: HashMap<String, String>,
    phantom_phrases: Vec<String>,
    filler_words: Vec<String>,
    fuzzy_min_consonants: usize,
}

/// A word-list entry: one word, or several words matched together ("Claude Code").
enum Entry {
    Word {
        text: String,
        lower: String,
        skeleton: Option<String>,
    },
    Phrase {
        text: String,
        lower: Vec<String>,
    },
}

pub struct Normalizer {
    remove: HashSet<char>,
    replace: HashMap<char, String>,
    needs_llm_ratio: f64,
    urdu_words: HashMap<String, String>,
    urdu_letters: HashMap<char, String>,
    urdu_marks: HashMap<char, String>,
    hindi_words: HashMap<String, String>,
    dev_consonants: HashMap<char, String>,
    dev_vowels: HashMap<char, String>,
    dev_matras: HashMap<char, String>,
    dev_signs: HashMap<char, String>,
    dev_nukta: HashMap<String, String>,
    /// Word sequences that are pure Whisper inventions: the hint's last sentences, phantom phrases.
    phantoms: Vec<Vec<String>>,
    fillers: HashSet<String>,
    fuzzy_min_consonants: usize,
    words: Vec<Entry>,
}

impl Default for Normalizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Normalizer {
    /// The built-in rules and the default engine's Whisper hint, with an empty word list.
    pub fn new() -> Self {
        Self::from_spec(RULES, WHISPER_HINT)
    }

    fn from_spec(rules: &str, hint: &str) -> Self {
        let r: Rules =
            serde_json::from_str(rules).expect("spec/normalizer/rules.json must be valid");
        let sentences: Vec<Vec<String>> = hint
            .split(['.', '!', '?'])
            .map(words_of)
            .filter(|w| !w.is_empty())
            .collect();
        // Whisper reads the hint as the text spoken just before, so on silence it repeats the hint's
        // tail. Only runs that end with the last sentence count: the earlier ones are real dictation too
        // ("PR merge kar do aur deployment check kar lo" is test sentence mx-01).
        let mut phantoms: Vec<Vec<String>> = (0..sentences.len())
            .map(|i| sentences[i..].concat())
            .collect();
        phantoms.extend(r.phantom_phrases.iter().map(|p| words_of(p)));
        Self {
            remove: r.remove.iter().map(|s| one_char(s)).collect(),
            replace: by_char(r.replace),
            needs_llm_ratio: r.needs_llm_ratio,
            urdu_words: r.urdu_words,
            urdu_letters: by_char(r.urdu_letters),
            urdu_marks: by_char(r.urdu_marks),
            hindi_words: r.hindi_words,
            dev_consonants: by_char(r.devanagari_consonants),
            dev_vowels: by_char(r.devanagari_vowels),
            dev_matras: by_char(r.devanagari_matras),
            dev_signs: by_char(r.devanagari_signs),
            dev_nukta: r.devanagari_nukta,
            phantoms,
            fillers: r.filler_words.into_iter().collect(),
            fuzzy_min_consonants: r.fuzzy_min_consonants,
            words: Vec::new(),
        }
    }

    /// Adds the user's word list (`words.txt`, the format in [`crate::words`]): each word, and each "heard as"
    /// mishearing as an exact replacement by that word.
    pub fn with_word_list(self, list: &str) -> Self {
        self.with_words(&crate::words::WordList::from_text(list))
    }

    pub fn with_words(mut self, list: &crate::words::WordList) -> Self {
        for entry in &list.entries {
            let text = entry.word.clone();
            let lower = words_of(&text);
            match lower.len() {
                0 => {}
                1 => {
                    let skeleton = skeleton(&text)
                        .filter(|(_, consonants)| *consonants >= self.fuzzy_min_consonants)
                        .map(|(s, _)| s);
                    let lower = lower.into_iter().next().unwrap_or_default();
                    self.words.push(Entry::Word {
                        text: text.clone(),
                        lower,
                        skeleton,
                    });
                }
                _ => self.words.push(Entry::Phrase {
                    text: text.clone(),
                    lower,
                }),
            }
            for heard in &entry.heard_as {
                let lower = words_of(heard);
                if !lower.is_empty() {
                    self.words.push(Entry::Phrase {
                        text: text.clone(),
                        lower,
                    });
                }
            }
        }
        // Longest phrases first, so "Claude Code" wins over a single-word "Claude".
        self.words.sort_by_key(|e| match e {
            Entry::Phrase { lower, .. } => std::cmp::Reverse(lower.len()),
            Entry::Word { .. } => std::cmp::Reverse(1),
        });
        self
    }

    pub fn clean(&self, raw: &str) -> Cleaned {
        let mut text = String::with_capacity(raw.len());
        for c in raw.chars() {
            if self.remove.contains(&c) {
                continue;
            }
            match self.replace.get(&c) {
                Some(r) => text.push_str(r),
                None => text.push(c),
            }
        }
        let (mut latin, mut other) = (0usize, 0usize);
        for c in text.chars().filter(|c| c.is_alphabetic()) {
            match script(c) {
                Script::Other => latin += 1,
                Script::Arabic | Script::Devanagari => other += 1,
            }
        }
        let mut needs_llm = other as f64 > self.needs_llm_ratio * (latin + other) as f64;
        let (text, unknown) = self.romanize(&text);
        needs_llm |= unknown;
        if self.is_phantom(&text) {
            return Cleaned {
                text: String::new(),
                needs_llm: false,
                phantom: true,
            };
        }
        // Line by line: a formatted list (D-030) keeps one item per line.
        let mut text = self
            .apply_words(&text)
            .lines()
            .map(tidy_spacing)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            text.push(' ');
        }
        Cleaned {
            text,
            needs_llm,
            phantom: false,
        }
    }

    /// Replaces every run of Urdu or Devanagari characters with Roman letters. Also returns whether a
    /// character had no rule (it is dropped).
    fn romanize(&self, text: &str) -> (String, bool) {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let mut unknown = false;
        let mut i = 0;
        while i < chars.len() {
            let kind = script(chars[i]);
            let mut j = i + 1;
            while j < chars.len() && script(chars[j]) == kind {
                j += 1;
            }
            let run = &chars[i..j];
            match kind {
                Script::Arabic => out.push_str(&self.urdu(run, &mut unknown)),
                Script::Devanagari => out.push_str(&self.hindi(run, &mut unknown)),
                Script::Other => out.extend(run),
            }
            i = j;
        }
        (out, unknown)
    }

    fn urdu(&self, run: &[char], unknown: &mut bool) -> String {
        let letters: Vec<char> = run
            .iter()
            .copied()
            .filter(|c| !self.urdu_marks.contains_key(c))
            .collect();
        if let Some(word) = self.urdu_words.get(&letters.iter().collect::<String>()) {
            return word.clone();
        }
        let mut out = String::new();
        let mut k = 0; // position among the letters, marks not counted
        for &c in run {
            if let Some(mark) = self.urdu_marks.get(&c) {
                out.push_str(mark);
                continue;
            }
            let first = k == 0;
            let last = k + 1 == letters.len();
            let prev = k.checked_sub(1).map(|p| letters[p]);
            let next = letters.get(k + 1).copied();
            k += 1;
            let by_place = match c {
                WAW if first => Some("w"),
                YEH | ARABIC_YEH if first || matches!(next, Some(ALEF | WAW)) => Some("y"),
                GOL_HEH | ARABIC_HEH if last && !prev.is_none_or(is_urdu_vowel) => Some("a"),
                _ => None,
            };
            match by_place.or_else(|| self.urdu_letters.get(&c).map(String::as_str)) {
                Some(s) => out.push_str(s),
                None => *unknown = true,
            }
        }
        out
    }

    fn hindi(&self, run: &[char], unknown: &mut bool) -> String {
        if let Some(word) = self.hindi_words.get(&run.iter().collect::<String>()) {
            return word.clone();
        }
        struct Syllable {
            consonant: String,
            vowel: Option<String>,
            inherent: bool, // still carries the built-in 'a'
            tail: String,   // nasal sign or visarga
        }
        let bare = |consonant: &str, vowel: Option<&String>, inherent| Syllable {
            consonant: consonant.to_string(),
            vowel: vowel.cloned(),
            inherent,
            tail: String::new(),
        };
        let mut syl: Vec<Syllable> = Vec::new();
        for &c in run {
            if let Some(v) = self.dev_consonants.get(&c) {
                syl.push(bare(v, None, true));
            } else if let Some(v) = self.dev_vowels.get(&c) {
                syl.push(bare("", Some(v), false));
            } else if let Some(v) = self.dev_matras.get(&c) {
                match syl.last_mut() {
                    Some(s) if s.inherent => {
                        s.vowel = Some(v.clone());
                        s.inherent = false;
                    }
                    _ => syl.push(bare("", Some(v), false)),
                }
            } else if c == VIRAMA {
                if let Some(s) = syl.last_mut() {
                    s.inherent = false;
                }
            } else if c == NUKTA {
                if let Some(s) = syl.last_mut()
                    && let Some(n) = self.dev_nukta.get(&s.consonant)
                {
                    s.consonant = n.clone();
                }
            } else if let Some(v) = self.dev_signs.get(&c) {
                match syl.last_mut() {
                    Some(s) => s.tail.push_str(v),
                    None => syl.push(bare(v, None, false)),
                }
            } else {
                *unknown = true;
            }
        }
        // Schwa deletion: the built-in 'a' is dropped at the end of a word and between a vowel and a
        // consonant that has its own vowel (daa-l-te, not daa-la-te).
        let n = syl.len();
        let mut out = String::new();
        let mut prev_voiced = false;
        for (i, s) in syl.iter().enumerate() {
            let keep_a = s.inherent
                && (n == 1
                    || !s.tail.is_empty()
                    || syl.get(i + 1).is_some_and(|next| {
                        let next_voiced = next.vowel.is_some() || (next.inherent && i + 2 < n);
                        !(prev_voiced && next_voiced)
                    }));
            out.push_str(&s.consonant);
            match &s.vowel {
                Some(v) => out.push_str(v),
                None if keep_a => out.push('a'),
                None => {}
            }
            out.push_str(&s.tail);
            prev_voiced = s.vowel.is_some() || keep_a;
        }
        out
    }

    /// True when the whole text is a Whisper invention: the hint's last sentence(s) or a phantom phrase
    /// (within 1 word in 4), or filler words only.
    fn is_phantom(&self, text: &str) -> bool {
        let words = words_of(text);
        if words.is_empty() {
            return false;
        }
        if words.iter().all(|w| self.fillers.contains(w)) {
            return true;
        }
        self.phantoms
            .iter()
            .any(|p| levenshtein(&words, p) <= p.len() / 4)
    }

    fn apply_words(&self, text: &str) -> String {
        if self.words.is_empty() {
            return text.to_string();
        }
        let tokens = tokens(text);
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        let mut i = 0;
        while i < tokens.len() {
            let found = self
                .exact_match(text, &tokens[i..])
                .or_else(|| self.fuzzy_match(&text[tokens[i].0..tokens[i].1]));
            match found {
                Some((len, replacement)) => {
                    out.push_str(&text[copied..tokens[i].0]);
                    out.push_str(replacement);
                    copied = tokens[i + len - 1].1;
                    i += len;
                }
                None => i += 1,
            }
        }
        out.push_str(&text[copied..]);
        out
    }

    /// An entry spelled the same apart from case, starting at the first token. Returns the number of
    /// tokens it covers and the list's spelling.
    fn exact_match(&self, text: &str, tokens: &[(usize, usize)]) -> Option<(usize, &str)> {
        let lower = |k: usize| text[tokens[k].0..tokens[k].1].to_lowercase();
        self.words.iter().find_map(|entry| match entry {
            Entry::Word {
                text: t, lower: l, ..
            } => (lower(0) == *l).then_some((1, t.as_str())),
            Entry::Phrase { text: t, lower: l } => {
                let matched = l.len() <= tokens.len()
                    && l.iter().enumerate().all(|(k, w)| {
                        lower(k) == *w
                            && (k == 0
                                || text[tokens[k - 1].1..tokens[k].0]
                                    .chars()
                                    .all(char::is_whitespace))
                    });
                matched.then_some((l.len(), t.as_str()))
            }
        })
    }

    /// A single-word entry that sounds the same ("Jaminai" for "Gemini", "Codec" for "Codex").
    fn fuzzy_match(&self, token: &str) -> Option<(usize, &str)> {
        let (sk, _) = skeleton(token)?;
        self.words.iter().find_map(|entry| match entry {
            Entry::Word {
                text,
                skeleton: Some(s),
                ..
            } if *s == sk => Some((1, text.as_str())),
            _ => None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Arabic,
    Devanagari,
    Other,
}

fn script(c: char) -> Script {
    match c as u32 {
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => {
            Script::Arabic
        }
        0x0900..=0x097F => Script::Devanagari,
        _ => Script::Other,
    }
}

fn is_urdu_vowel(c: char) -> bool {
    matches!(c, ALEF | ALEF_MADDA | WAW | YEH | ARABIC_YEH | BARI_YEH)
}

/// How a Latin word sounds, roughly: consonants folded by sound, every vowel run one `V`, doubled
/// letters once. "Gemini" and "Jaminai" both give `jVmVnV`. Also returns the number of consonants.
/// `None` when the word is not plain ASCII letters.
pub(crate) fn skeleton(word: &str) -> Option<(String, usize)> {
    if word.is_empty() || !word.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let w = word.to_ascii_lowercase().into_bytes();
    let mut out = String::new();
    let mut consonants = 0;
    let mut i = 0;
    while i < w.len() {
        let next = w.get(i + 1).copied();
        let (sym, step) = match (w[i], next) {
            (b's', Some(b'h')) => ('S', 2),
            (b'c', Some(b'h')) => ('C', 2),
            (b'p', Some(b'h')) => ('f', 2),
            (b'c', Some(b'k')) => ('k', 2),
            (b'c', Some(b'e' | b'i' | b'y')) => ('s', 1),
            (b'c' | b'q' | b'x', _) => ('k', 1),
            (b'g', Some(b'e' | b'i')) => ('j', 1),
            (b'w', _) => ('v', 1),
            (b'h', _) if i > 0 => ('\0', 1), // aspiration or silent: "bhai" ~ "bai"
            (b'y', _) if i == 0 => ('y', 1),
            (b'a' | b'e' | b'i' | b'o' | b'u' | b'y', _) => ('V', 1),
            (c, _) => (c as char, 1),
        };
        i += step;
        if sym == '\0' || out.ends_with(sym) {
            continue;
        }
        if sym != 'V' {
            consonants += 1;
        }
        out.push(sym);
    }
    Some((out, consonants))
}

/// Lowercase words of letters and digits; punctuation is ignored.
fn words_of(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Byte ranges of the words (runs of letters and digits) in `text`.
fn tokens(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    out
}

/// Within one line: one space between words, none before `, . ? ! ; :`, one after `, ; ? !` when a letter follows.
fn tidy_spacing(text: &str) -> String {
    let mut joined = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !joined.is_empty() && !word.starts_with([',', '.', '?', '!', ';', ':']) {
            joined.push(' ');
        }
        joined.push_str(word);
    }
    let mut out = String::with_capacity(joined.len() + 8);
    let mut chars = joined.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if matches!(c, ',' | ';' | '?' | '!') && chars.peek().is_some_and(|n| n.is_alphabetic()) {
            out.push(' ');
        }
    }
    out
}

fn levenshtein<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, y) in b.iter().enumerate() {
            cur.push(
                (prev[j + 1] + 1)
                    .min(cur[j] + 1)
                    .min(prev[j] + usize::from(x != y)),
            );
        }
        prev = cur;
    }
    prev[b.len()]
}

fn one_char(key: &str) -> char {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => c,
        _ => panic!("spec/normalizer/rules.json: key {key:?} must be one character"),
    }
}

fn by_char(map: HashMap<String, String>) -> HashMap<char, String> {
    map.into_iter().map(|(k, v)| (one_char(&k), v)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Cases {
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        id: String,
        input: String,
        #[serde(default)]
        words: Vec<String>,
        expected: String,
        #[serde(default)]
        needs_llm: bool,
        #[serde(default)]
        phantom: bool,
    }

    #[test]
    fn shared_cases() {
        let cases: Cases =
            serde_json::from_str(include_str!("../../../spec/normalizer/cases.json")).unwrap();
        let mut failures = Vec::new();
        for case in &cases.cases {
            let got = Normalizer::new()
                .with_word_list(&case.words.join("\n"))
                .clean(&case.input);
            let want = if case.expected.is_empty() {
                String::new()
            } else {
                format!("{} ", case.expected)
            };
            if got.text != want || got.needs_llm != case.needs_llm || got.phantom != case.phantom {
                failures.push(format!(
                    "{}:\n  got  {:?} needs_llm={} phantom={}\n  want {:?} needs_llm={} phantom={}",
                    case.id,
                    got.text,
                    got.needs_llm,
                    got.phantom,
                    want,
                    case.needs_llm,
                    case.phantom
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} cases failed:\n{}",
            failures.len(),
            cases.cases.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn skeletons_fold_sounds() {
        let sk = |w| skeleton(w).unwrap().0;
        assert_eq!(sk("Jaminai"), sk("Gemini"));
        assert_eq!(sk("Codec"), sk("Codex"));
        assert_eq!(sk("Grock"), sk("Groq"));
        assert_ne!(sk("code"), sk("Codex"));
        assert_ne!(sk("cold"), sk("Claude"));
        assert_eq!(skeleton("RAM").unwrap().1, 2); // too short for fuzzy matching
        assert_eq!(skeleton("can't"), None);
    }

    #[test]
    fn word_list_parsing() {
        let n = Normalizer::new().with_word_list("# my words\nGemini, Groq\n\n  VS   Code ,\n");
        assert_eq!(n.words.len(), 3);
        assert!(matches!(&n.words[0], Entry::Phrase { text, .. } if text == "VS Code"));
    }

    #[test]
    fn mishearings_in_the_word_list_are_replaced() {
        let n = Normalizer::new().with_word_list("Claude = McCloud, Mc Cloud\nhun = hoon");
        assert_eq!(
            n.clean("Maine claude bola, isne McCloud likha. Main ghar pe hoon.")
                .text
                .trim_end(),
            "Maine Claude bola, isne Claude likha. Main ghar pe hun."
        );
        assert_eq!(n.clean("Mc Cloud").text.trim_end(), "Claude");
    }

    #[test]
    fn whitespace_only_gives_nothing() {
        let got = Normalizer::new().clean("  \n ");
        assert_eq!(
            got,
            Cleaned {
                text: String::new(),
                needs_llm: false,
                phantom: false
            }
        );
    }
}
