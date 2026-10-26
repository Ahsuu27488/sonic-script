# SonicScript

**Bolo, hum likh denge.** Dictation that types the way Pakistan and India actually text.

Speak Urdu, Hindi, English, or all three mixed, and get clean **Roman Urdu / Hinglish** at your cursor, with
English words kept in English:

> *"PR merge kar do aur deployment check kar lo."*

Built-in tools (Windows `Win + H`, Gboard, Apple Dictation) write Urdu or Devanagari script and garble the English
words in between. SonicScript fixes that.

> **Status: early development.** Nothing to install yet. We are measuring which free cloud speech engine works best
> (Milestone 0). See [blueprint.md](blueprint.md) for the plan.

## Planned

- **Windows:** hold `Right Ctrl`, speak, release, and the text appears in any app (VS Code, Chrome, Slack, Claude
  Code and Codex in the terminal).
- **Android:** a voice keyboard with one big mic button.
- **Output modes:** Roman Urdu / Hinglish, English, Urdu script, Devanagari.
- **Free:** bring your own free Groq or Gemini API key. No account, no server, no telemetry.

## Repository

| Path | What |
|---|---|
| [blueprint.md](blueprint.md) | Product plan and milestones |
| [DESIGN.md](DESIGN.md) | Design system (the Voice Ring) |
| `docs/` | Project memory: status, decisions, architecture, changelog |

## License

Apache-2.0 (proposed, not final yet).
