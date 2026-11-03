"""How good is an output? Script check, English words kept, and spelling-tolerant accuracy.

Roman Urdu has no standard spelling ("kar do" / "kardo", "hai" / "hay"), so plain WER would punish correct
output. `canon` folds common variants together before comparing.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

# Arabic/Urdu (incl. presentation forms) and Devanagari blocks.
NON_LATIN = re.compile(r"[\u0600-\u06FF\u0750-\u077F\u08A0-\u08FF\uFB50-\uFDFF\uFE70-\uFEFF\u0900-\u097F]")
WORD = re.compile(r"[a-z0-9]+")
# A list line: "- item", "* item", "• item", or "1. item" / "1) item".
LIST_ITEM = re.compile(r"^\s*(?:([-*•])|(\d+)[.)])\s+\S")
NUMBER = re.compile(r"\d(?:[\d,:]*\d)?")  # 6, 2:30, 2,50,000 (a trailing comma or full stop is not part of it)

# Whole-word spelling variants -> one form.
VARIANTS = {
    "hy": "hai", "hay": "hai", "he": "hai", "hain": "hain", "hen": "hain", "hein": "hain",
    "nahin": "nahi", "nai": "nahi", "nhi": "nahi", "nahee": "nahi",
    "men": "mein", "mien": "mein",  # not "main": in Urdu that is "I", a different word
    "bohot": "bohat", "bahut": "bohat", "bhot": "bohat", "bht": "bohat",
    "acha": "acha", "achha": "acha", "accha": "acha", "acchha": "acha",
    "kia": "kya", "kiya": "kya",
    "yeh": "ye", "yah": "ye", "woh": "wo", "vo": "wo", "voh": "wo",
    "ke": "ke", "kay": "ke", "ki": "ki", "ko": "ko",
    "thik": "theek", "thek": "theek",
    "phir": "phir", "fir": "phir",
    "abhi": "abhi", "abi": "abhi",
    "rupay": "rupay", "rupees": "rupay", "rupaye": "rupay", "rupee": "rupay", "rs": "rupay",
}


@dataclass(frozen=True)
class Score:
    script_ok: bool
    english_found: int
    english_total: int
    accuracy: float  # 1 - character error rate on canonical text, 0..1
    wer: float  # word error rate on canonical words, informational

    @property
    def english_rate(self) -> float:
        return 1.0 if self.english_total == 0 else self.english_found / self.english_total


def score(output: str, expected: str, english: list[str]) -> Score:
    if not expected:  # silence clip: the only right answer is no text
        empty = not output.strip()
        return Score(script_ok=empty or not NON_LATIN.search(output), english_found=0, english_total=0,
                     accuracy=1.0 if empty else 0.0, wer=0.0 if empty else 1.0)
    found = sum(1 for term in english if has_term(output, term))
    exp_words = canon_words(expected)
    out_words = canon_words(output)
    cer = _levenshtein(list("".join(exp_words)), list("".join(out_words))) / max(1, len("".join(exp_words)))
    wer = _levenshtein(exp_words, out_words) / max(1, len(exp_words))
    return Score(
        script_ok=not NON_LATIN.search(output) and bool(output.strip()),
        english_found=found,
        english_total=len(english),
        accuracy=round(max(0.0, 1.0 - cer), 4),
        wer=round(wer, 4),
    )


@dataclass(frozen=True)
class FormatScore:
    """Smart formatting (D-030) on a `format` clip: each check that the sentence asks for passes or fails."""

    list_ok: bool  # the right kind of list with the right number of items, or no list when none is expected
    absent_ok: bool  # taken-back parts and fillers are gone
    digits_ok: bool  # numbers, amounts and times written as expected (2,50,000, 2:30)
    checks: int
    passed: int

    @property
    def rate(self) -> float:
        return self.passed / self.checks if self.checks else 1.0


def format_score(output: str, check: dict) -> FormatScore:
    """`check` is a sentence's `format` block: list (dash / ordered / any / None), items, absent, digits."""
    list_ok = _list_ok(output, check.get("list"), int(check.get("items") or 0))
    absent, digits = check.get("absent") or [], check.get("digits") or []
    numbers = set(NUMBER.findall(output))
    absent_ok = not any(n in numbers if n[0].isdigit() else _has_phrase(output, n) for n in absent)
    digits_ok = all(d in numbers for d in digits)
    results = [list_ok] + ([absent_ok] if absent else []) + ([digits_ok] if digits else [])
    return FormatScore(list_ok, absent_ok, digits_ok, len(results), sum(results))


def list_items(text: str) -> list[tuple[str, int | None]]:
    """The list lines of a text as (kind, number): ("dash", None) or ("ordered", 3)."""
    items = []
    for line in text.splitlines():
        m = LIST_ITEM.match(line)
        if m:
            items.append(("dash", None) if m.group(1) else ("ordered", int(m.group(2))))
    return items


def _list_ok(output: str, kind: str | None, count: int) -> bool:
    items = list_items(output)
    if kind is None:
        return not items
    kinds = {k for k, _ in items}
    if len(items) != count or len(kinds) != 1:
        return False
    if kinds == {"ordered"} and [n for _, n in items] != list(range(1, count + 1)):
        return False
    return kind == "any" or kinds == {kind}


def _has_phrase(output: str, phrase: str) -> bool:
    return re.search(rf"(?<![a-z0-9]){re.escape(phrase.lower())}(?![a-z0-9])", output.lower()) is not None


def has_term(output: str, term: str) -> bool:
    """Case-insensitive whole-word match; a plural/singular 's' difference still counts."""
    words = set(WORD.findall(output.lower()))
    t = term.lower()
    return t in words or t + "s" in words or (t.endswith("s") and t[:-1] in words)


def canon(word: str) -> str:
    w = VARIANTS.get(word, word)
    w = w.replace("ee", "i").replace("oo", "u").replace("aa", "a")
    w = re.sub(r"(.)\1+", r"\1", w)  # doubled letters: "acchha" -> "acha"
    w = re.sub(r"(?<=[aeiou])h$", "", w)  # trailing h after a vowel: "yeh" -> "ye"
    return w


def canon_words(text: str) -> list[str]:
    return [canon(w) for w in WORD.findall(text.lower().replace(",", ""))]


def _levenshtein(a: list[str], b: list[str]) -> int:
    if len(a) < len(b):
        a, b = b, a
    prev = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        cur = [i]
        for j, y in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (x != y)))
        prev = cur
    return prev[-1]
