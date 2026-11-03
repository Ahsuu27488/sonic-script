# SonicScript

**Bolo, hum likh denge.** Dictation that types the way Pakistan and India actually text.

Speak Urdu, Hindi, English, or all three mixed, and get clean **Roman Urdu / Hinglish** at your cursor, with
English words kept in English:

> *"PR merge kar do aur deployment check kar lo."*

Built-in tools (Windows `Win + H`, Gboard, Apple Dictation) write Urdu or Devanagari script and garble the English
words in between. SonicScript fixes that.

> **Status: early development.** Nothing to install yet. The engine benchmark (Milestone 0) is done: Groq Whisper
> is the fast default, Gemini the accurate option. The Windows app (Milestone 1, Rust) is being built. See
> [blueprint.md](blueprint.md) for the plan.

## Planned

- **Windows:** tap `Right Ctrl` (or a key of your choice), speak, tap again or press Enter, and the text appears
  in any app (VS Code, Chrome, Slack, Claude Code and Codex in the terminal). Holding the key works too.
- **Android:** a voice keyboard with one big mic button.
- **Output modes:** Roman Urdu / Hinglish, English, Urdu script, Devanagari.
- **Free:** bring your own free Groq or Gemini API key. No account, no server, no telemetry.

## Repository

| Path | What |
|---|---|
| [blueprint.md](blueprint.md) | Product plan and milestones |
| [DESIGN.md](DESIGN.md) | Design system (the Voice Ring) |
| `eval/` | Engine benchmark (Python) |
| `desktop/` | Windows app (Rust, in progress) |
| `spec/` | Prompts, model ids, test sentences and text-cleanup rules shared by every app |
| `docs/` | Project memory: status, decisions, architecture, changelog |

## License

MIT ([LICENSE](LICENSE)).
