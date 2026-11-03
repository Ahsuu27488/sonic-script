//! Runs the text cleanup on stdin, one Whisper output per line, and prints one JSON object per line
//! ({"text", "needs_llm", "phantom"}). Optional argument: a word-list file.
//!
//! cargo run -q --example clean -- [words.txt] < outputs.txt

use std::io::{self, BufRead, Write};

use sonicscript_core::normalize::Normalizer;

fn main() -> io::Result<()> {
    let mut normalizer = Normalizer::new();
    if let Some(path) = std::env::args().nth(1) {
        normalizer = normalizer.with_word_list(&std::fs::read_to_string(path)?);
    }
    let mut out = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let c = normalizer.clean(&line?);
        let json =
            serde_json::json!({ "text": c.text, "needs_llm": c.needs_llm, "phantom": c.phantom });
        writeln!(out, "{json}")?;
    }
    Ok(())
}
