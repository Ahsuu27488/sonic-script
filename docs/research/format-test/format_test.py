"""Feasibility test: can a text model format lists (and fillers, take-backs, numbers) in Roman Urdu?
Text in, text out; no audio. Prints no keys. Run from eval/:
    python ../docs/research/format-test/format_test.py openai/gpt-oss-20b openai/gpt-oss-120b gemini-3.5-flash-lite"""
import json
import os
import sys
import time
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "eval"))
from sonic_eval.config import load_env  # noqa: E402

load_env()
PROMPT = Path(__file__).with_name("format-draft.md").read_text(encoding="utf-8")
SPEC = json.loads((REPO / "spec/providers.json").read_text(encoding="utf-8"))

CASES = [
    ("grocery", "mujhe market se ye lana hai doodh anday bread aur do kilo aloo"),
    ("todo", "kal ke kaam pehla bank jana hai dusra bijli ka bill jama karwana hai teesra Ali ko call karni hai"),
    ("plans", "is weekend ke plans ye hain friday ko movie saturday ko cricket aur sunday ko family dinner"),
    ("chat", "guys aaj raat game night hai nau baje join karna aur apne mic check kar lena"),
    ("rules", "server ke rules simple hain spam nahi karna kisi ko gaali nahi deni aur memes sirf memes channel mein"),
    ("backtrack", "theek hai kal paanch baje milte hain nahi nahi chhe baje"),
    ("numbers", "bhai do lakh pachaas hazaar rupay bhej do dhai baje tak"),
    ("filler", "matlab yaar woh na main keh raha tha ke kal chhutti hai kya"),
    ("not-a-list", "pehle main socha ke aaj gym jaun lekin phir baarish shuru ho gayi aur main ghar pe hi raha"),
    ("steps", "chai banane ka tareeqa pehle paani ubaalo phir patti daalo phir doodh aur cheeni daalo "
              "aur do minute pakao"),
    ("urdu-script", "یار مجھے تین چیزیں چاہیے لیپ ٹاپ چارجر ماؤس اور ہیڈفون"),
    ("two-items", "meeting ke baad mujhe call karna aur report bhej dena"),
    ("english", "groceries for the week eggs milk spinach and two loaves of bread"),
]


def post(url: str, body: dict, headers: dict) -> dict:
    req = urllib.request.Request(url, data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json",
                                          "User-Agent": "sonicscript-format-test", **headers})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)


def ask(model: str, speech: str) -> str:
    if model.startswith("gemini"):
        base = SPEC["providers"]["gemini"]["base_url"]
        out = post(f"{base}/models/{model}:generateContent",
                   {"system_instruction": {"parts": [{"text": PROMPT}]},
                    "contents": [{"role": "user", "parts": [{"text": f"SPEECH: {speech}"}]}],
                    "generationConfig": {"temperature": 0, "thinkingConfig": {"thinkingLevel": "minimal"}}},
                   {"x-goog-api-key": os.environ["GEMINI_API_KEY"]})
        return out["candidates"][0]["content"]["parts"][0]["text"]
    out = post("https://api.groq.com/openai/v1/chat/completions",
               {"model": model, "temperature": 0, "reasoning_effort": "low",
                "messages": [{"role": "system", "content": PROMPT},
                             {"role": "user", "content": f"SPEECH: {speech}"}]},
               {"Authorization": f"Bearer {os.environ['GROQ_API_KEY']}"})
    return out["choices"][0]["message"]["content"]


models = sys.argv[1:] or ["openai/gpt-oss-20b"]
for name, speech in CASES:
    print(f"=== {name}\nIN : {speech}")
    for model in models:
        t = time.perf_counter()
        try:
            text = ask(model, speech).strip()
        except Exception as e:  # noqa: BLE001
            text = f"ERROR {type(e).__name__}: {str(e)[:120]}"
        print(f"--- {model} ({time.perf_counter() - t:.1f} s)\n{text}")
        time.sleep(5 if model.startswith("gemini") else 2)
    print()
