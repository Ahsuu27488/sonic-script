# Wispr Flow: how it formats text, and where SonicScript can do better

> Research note, 2026-11-03. Sources: Wispr Flow's own help center (docs.wisprflow.ai), its site and
> TechCrunch; reviews only where the docs are silent. Features change often: re-check before building on a
> detail. This is input for the formatting plan, not a decision (decisions go in DECISIONS.md).

## What Wispr Flow does to the words

| Feature | What it does | Notes |
|---|---|---|
| **Auto Cleanup** (Style → Auto Cleanup, beta) | Levels: **None** (raw transcript, fillers and false starts kept), **Light** (default: removes fillers, fixes grammar), **Medium** (may reword for clarity and brevity). A **High** level (rewrite for polish) existed and was removed; its users were moved to Medium. | On Android the same thing is "Smart Formatting", always on, no switch. |
| **Backtrack** | Drops what you take back when you say "actually", "scratch that": "let's meet at 5pm, actually let's do 6pm" → "let's meet at 6pm". Only clear self-corrections. | Off when cleanup is None. |
| **Smart Formatting** | Punctuation from pauses; spoken "period / comma / question mark" become symbols unless clearly content; numbers become digits (units, currency, times, %, degrees); implied lists, "first ... finally" turned into list items; paragraph breaks at topic changes; email greetings formatted; contractions expanded ("gonna" → "going to"); spoken emoji names → emoji; a spelled-out word replaces a similar-sounding earlier word. | Can over-delete (a leading "so", deliberate repetition). |
| **Styles** | Per app category: personal messages, work messages, email, other; each Formal / Casual / Very casual (punctuation, capitals, final period in chats). | **English only.** |
| **Dictionary** | Personal and team-shared words, starred for priority, "correct a misspelling" entries. | Like our word list (D-014, D-019). |
| **Snippets** | Trigger phrase → saved text (address, links, templates). | Personal and team. |
| **Command Mode** (Pro, Mac / Windows) | Hold a second shortcut, speak an instruction ("make this more formal"); Flow sends the text box's contents + selection with it and pastes the result. | No "Hey Flow" needed. |
| **Transforms** (beta) | Select text, press a shortcut: a preset ("Polish") or your own prompt rewrites it. | Keyboard-driven Command Mode. |
| **Press Enter command** | Say a command to send (press Enter) after dictating. | Experimental, paid. |

## Languages (the gap we are built for)

- **Hinglish exists** (since the India launch, May 2026): "writes Hindi in Latin letters and formats English
  normally". It is **not detected**: the user must pick Hinglish in settings; Hindi + English selected does
  **not** give Latin-script Hindi.
- **Urdu** is selectable but cannot be identified from speech; with Hindi selected, Flow checks whether the speech
  is Urdu so that "Urdu speech comes out in Urdu" (Urdu script). **Roman Urdu is not mentioned anywhere.**
- **One language per dictation, not per word.** Real South Asian speech switches mid-sentence.
- Styles (formal / casual) work in **English only**; the formatting rules (fillers, contractions, numbers) are
  written for English.
- Pricing: Command Mode, Press Enter and other features are paid (Pro); we are free with the user's own key
  (D-002).

## Where SonicScript can be better ("Wispr Flow for South Asia, then everyone")

1. **Roman Urdu and Hinglish, detected per word, with no setting.** Our engines already do this (Gemini 97%,
   D-028 / D-029); Flow needs a manual switch and has no Roman Urdu at all.
2. **Backtrack in Urdu / Hindi**: "nahi nahi", "matlab", "sorry", "ek minute", "acha nahi" as take-back words:
   "kal 5 baje milte hain, nahi nahi 6 baje" → "kal 6 baje milte hain".
3. **South Asian fillers**: "matlab", "yaani", "woh", "haan to", "acha" (only when it is a filler: "acha laga"
   stays).
4. **South Asian numbers**: "dhai baje" → 2:30, "saadhe teen" → 3:30, "paune paanch" → 4:45, "paanch sau" →
   500, "do lakh" → 2,00,000 (lakh / crore grouping as an option), "Rs" / "PKR" / "₹" currency.
5. **Roman spelling style**: Roman Urdu has no standard spelling. Let users pick their own ("hai" / "hy",
   "nahi" / "nhi", "kya" / "kia", "main" / "mein") and keep it consistent; Flow has nothing like it.
6. ~~Styles per app~~: the owner decided against per-app styles (2026-11-03): one smart formatter for every
   app, formatting (lists) only when the speech is clearly a list.
7. **Output script choice**: Roman (default), Urdu script, Devanagari, or English translation of what was said.
8. **Command Mode in mixed language**: "isko formal kar do", "English mein translate karo", "chhota karo"; free
   with the user's key.
9. **Open source, free, BYOK** (D-002, D-012): no subscription, keys stay on the PC.

## What it means for our pipeline

- Formatting needs a language model; it belongs in the prompts: `spec/prompts/transcribe-roman-v1.md` (Gemini
  Flash / Flash-Lite, audio → text) and `spec/prompts/romanize-v1.md` (the Groq refine step after Transcribe).
  The same rules must be in both, or the text style changes when the chain falls back (D-029).
- The last engine, Groq Whisper, has no language model step: only the local cleanup (`normalize.rs`) applies,
  so its text will be less formatted. A refine step on it would cost ~1 s (run `m0`
  `groq-whisper-hint+gptoss20b`).
- Every rule needs test sentences in `spec/fixtures/sentences.json` (backtrack, numbers, fillers, lists) and a
  score for it in `eval`, so a prompt change is measured, not guessed.

## Feasibility test: lists and smart formatting (2026-11-03)

The owner's direction: **no per-app styles**; one smart formatter that turns speech into a list when the speaker
clearly lists things, and leaves ordinary speech as sentences. Wispr does lists with Smart Formatting (spoken
"one / two", "first / second" → list) plus per-app context (app category, text around the cursor), which we skip.

Test: `format-test/format_test.py` (prompt `format-test/format-draft.md`), 13 raw Roman Urdu / Hinglish /
English / Urdu-script transcripts, text only (no audio), results in `format-test/results-2026-11-03.txt`.

| Case | gpt-oss-20b (Groq) | gpt-oss-120b (Groq) | Gemini 3.5 Flash-Lite |
|---|---|---|---|
| Grocery, to-do, rules, English groceries (lists) | all lists | all lists | all lists |
| Weekend plans (list) | list | missed (one sentence) | 503 |
| Ordered steps (chai) | bullets, kept "Phir" | bullets | numbered (best) |
| Take-back ("nahi nahi chhe baje") | right | right | right |
| Lakh amount, "dhai baje" | 2,50,000 · 2:30 | same | same ("2:30 baje") |
| Fillers ("matlab ... woh na") | removed "matlab" only | right | none removed |
| Urdu-script input | romanized + list | romanized + list | romanized + list |
| Not a list, two items only | stayed sentences | stayed sentences | stayed sentences |
| Typical time | ~0.9 s | ~0.7 s | 13-29 s (Google slow that hour) |

Result: **it works**: a short prompt with two examples gets lists, take-backs, South Asian numbers and Urdu-script
romanizing right in about 1 s on Groq. The first draft without examples (and with per-app rules) missed half the
lists and left Urdu script unromanized, so the examples matter. `gpt-oss-120b` was the most reliable on the
small details; a real benchmark (spoken test sentences with lists, scored) should decide between 20b and 120b.

Where it would run in the chain (D-029): the Transcribe path already has the Groq refine step, so the rules go in
`romanize-v1.md`; Gemini Flash / Flash-Lite would get the same rules in `transcribe-roman-v1.md` (no extra
request) or the refine step (+~1 s); Groq Whisper (last resort) has no language model step today.

## Sources

- [Auto Cleanup: control how much Flow edits your dictation](https://docs.wisprflow.ai/articles/4283510616-auto-cleanup-control-how-much-flow-edits-your-dictation-beta)
- [How do I use Smart Formatting & Backtrack](https://docs.wisprflow.ai/articles/5373093536-how-do-i-use-smart-formatting-and-backtrack)
- [Why Wispr Flow sometimes removes or changes words on Android (Smart Formatting)](https://docs.wisprflow.ai/articles/6568835559-why-wispr-flow-sometimes-removes-or-changes-words-on-android-smart-formatting)
- [Use Flow with multiple languages](https://docs.wisprflow.ai/articles/3191899797-use-flow-with-multiple-languages)
- [Navigating the Wispr Flow app](https://docs.wisprflow.ai/articles/5096240724-navigating-the-wispr-flow-app-desktop-ios-and-android)
- [How to use Command Mode](https://docs.wisprflow.ai/articles/4816967992-how-to-use-command-mode)
- [Rewrite text with Transforms and custom prompts](https://docs.wisprflow.ai/articles/8068950331-how-to-use-transforms-beta)
- [What's new](https://wisprflow.ai/whats-new), [Features](https://wisprflow.ai/features), [Wispr Flow India](https://wisprflow.ai/india)
- [TechCrunch: Voice AI in India is hard; Wispr Flow is betting on it anyway (2026-05-09)](https://techcrunch.com/2026/05/09/voice-ai-in-india-is-hard-wispr-flow-is-betting-on-it-anyway/)
- [BusinessWire launch release (backtrack example)](https://www.businesswire.com/news/home/20240930079034/en)
