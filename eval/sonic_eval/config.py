"""Paths, the shared spec files (sentences, prompts, providers) and API keys."""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SPEC = REPO / "spec"
PROMPTS = SPEC / "prompts"
FIXTURES = SPEC / "fixtures"
AUDIO_DIR = FIXTURES / "audio"
RESULTS = REPO / "eval" / "results"
MODELS_FILE = SPEC / "models.json"
TOKENS_CSS = REPO / "design" / "tokens.css"
ENV_FILE = REPO / "eval" / ".env"


@dataclass(frozen=True)
class Sentence:
    id: str
    category: str
    text: str
    english: list[str] = field(default_factory=list)
    say: str = ""  # what to say when it differs from the text (format items), or how to record (silence items)
    format_check: dict | None = None  # format items (D-030): list kind, items, absent words, digits to find

    @property
    def is_silence(self) -> bool:
        return not self.text


def load_sentences(fixtures: Path = FIXTURES) -> list[Sentence]:
    """The scripted set, plus free-speech sentences recorded with `record --free` (kept with the audio)."""
    items = json.loads((fixtures / "sentences.json").read_text(encoding="utf-8"))["sentences"]
    free = fixtures / "audio" / "free.json"
    if free.exists():
        items += json.loads(free.read_text(encoding="utf-8"))
    return [Sentence(i["id"], i["category"], i["text"], list(i.get("english", [])), i.get("say", ""),
                     i.get("format")) for i in items]


def load_providers(path: Path = SPEC / "providers.json") -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def load_prompt(name: str, word_list: list[str] | None = None) -> str:
    text = (PROMPTS / name).read_text(encoding="utf-8")
    words = ""
    if word_list:
        words = "Spell these words exactly like this when you hear them: " + ", ".join(word_list) + ".\n\n"
    return text.replace("{word_list}", words).strip()


def wrap_transcript(text: str) -> str:
    """The user message of a text step (format / refine): the transcript in `<transcript>` tags, marked as
    speech to rewrite (D-038; the app's `spec::wrap_transcript`)."""
    template = (PROMPTS / "transcript-wrap-v1.txt").read_text(encoding="utf-8").strip()
    return template.replace("{transcript}", text.strip())


KEY_NAMES = ("GROQ_API_KEY", "GEMINI_API_KEY", "OPENAI_API_KEY")


def load_env(path: Path = ENV_FILE) -> None:
    """Keys come from eval/.env, then the process environment, then (Windows) the user's saved variables.

    Reading the saved user variables means a key set with SetEnvironmentVariable(..., "User") works at once,
    without restarting the terminal or app that launched us. A key in eval/.env wins over both, so the
    benchmark can use its own key while the global one stays in use by other apps.
    """
    if path.exists():
        for line in path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, value = line.split("=", 1)
            value = value.strip().strip('"').strip("'")
            if value:
                os.environ[key.strip()] = value
    _load_windows_user_env()


def _load_windows_user_env() -> None:
    if os.name != "nt":
        return
    import winreg

    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment") as reg:
            for name in KEY_NAMES:
                if os.environ.get(name):
                    continue
                try:
                    value, _ = winreg.QueryValueEx(reg, name)
                except FileNotFoundError:
                    continue
                if value:
                    os.environ[name] = str(value)
    except OSError:
        pass
