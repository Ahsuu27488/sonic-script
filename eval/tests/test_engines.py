"""Engines against a mocked HTTP server: request shape, parsing, retries and errors."""

import json

import httpx
import pytest

from sonic_eval.audio import EncodedAudio
from sonic_eval.engines import GeminiAudio, GroqWhisper, GroqWhisperLLM, clean_output
from sonic_eval.ratelimit import RateLimiter

CLIP = EncodedAudio(b"OggS-fake", "audio/ogg", "ogg", "opus16", 3.2)
GEMINI = {"base_url": "https://g.test/v1beta", "key_env": "GEMINI_API_KEY", "rpm": 6000}
GROQ = {"base_url": "https://q.test/openai/v1", "key_env": "GROQ_API_KEY", "rpm": 6000}


@pytest.fixture(autouse=True)
def keys(monkeypatch):
    monkeypatch.setenv("GEMINI_API_KEY", "g-key")
    monkeypatch.setenv("GROQ_API_KEY", "q-key")


def limiter():
    return RateLimiter(sleep=lambda s: None)


def test_gemini_sends_audio_and_prompt_and_reads_text():
    seen = {}

    def handler(req: httpx.Request):
        seen["url"] = str(req.url)
        seen["key"] = req.headers["x-goog-api-key"]
        seen["body"] = json.loads(req.content)
        return httpx.Response(200, json={
            "candidates": [{"content": {"parts": [{"text": "thinking", "thought": True},
                                                  {"text": " PR merge kar do. "}]}}],
            "usageMetadata": {"promptTokenCount": 120, "candidatesTokenCount": 9,
                              "promptTokensDetails": [{"modality": "AUDIO", "tokenCount": 100}]}})

    spec = {"id": "g", "kind": "gemini-audio", "provider": "gemini", "model": "gemini-x",
            "prompt": "transcribe-roman-v1.md", "price": {"audio_in_per_m": 1.0, "out_per_m": 2.0}}
    e = GeminiAudio(spec, GEMINI, httpx.Client(transport=httpx.MockTransport(handler)), limiter(), ["Taskvane"])
    r = e.transcribe(CLIP)
    assert r.error is None
    assert r.text == "PR merge kar do."
    assert seen["url"].endswith("/models/gemini-x:generateContent")
    assert seen["key"] == "g-key"
    parts = seen["body"]["contents"][0]["parts"]
    assert "Roman" in parts[0]["text"] and "Taskvane" in parts[0]["text"]
    assert parts[1]["inline_data"]["mime_type"] == "audio/ogg"
    assert (r.tokens_in, r.tokens_audio, r.tokens_out) == (120, 100, 9)
    assert e.cost_usd(r, 3.2) == pytest.approx((120 * 1.0 + 9 * 2.0) / 1e6)


def test_whisper_llm_chains_both_calls():
    calls = []

    def handler(req: httpx.Request):
        calls.append(req.url.path)
        if req.url.path.endswith("/audio/transcriptions"):
            assert b'name="language"' in req.content and b"ur" in req.content
            return httpx.Response(200, json={"text": "پی آر مرج کر دو"})
        body = json.loads(req.content)
        assert body["messages"][1]["content"] == "پی آر مرج کر دو"
        assert body["reasoning_effort"] == "low"
        return httpx.Response(200, json={"choices": [{"message": {"content": "PR merge kar do."}}],
                                         "usage": {"prompt_tokens": 200, "completion_tokens": 8}})

    spec = {"id": "w", "kind": "whisper-llm", "provider": "groq", "whisper_model": "whisper-large-v3-turbo",
            "language": "ur", "llm_model": "openai/gpt-oss-20b", "llm_extra": {"reasoning_effort": "low"},
            "prompt": "romanize-v1.md", "price": {"audio_per_hour": 0.04, "in_per_m": 0.1, "out_per_m": 0.3}}
    e = GroqWhisperLLM(spec, GROQ, httpx.Client(transport=httpx.MockTransport(handler)), limiter())
    r = e.transcribe(CLIP)
    assert r.error is None
    assert r.text == "PR merge kar do."
    assert r.raw_asr == "پی آر مرج کر دو"
    assert set(r.stages_ms) == {"whisper", "llm"}
    assert len(calls) == 2
    # Whisper bills at least 10 seconds.
    assert e.cost_usd(r, 3.2) == pytest.approx(10 / 3600 * 0.04 + (200 * 0.1 + 8 * 0.3) / 1e6)


def test_rate_limited_request_is_retried():
    n = {"calls": 0}

    def handler(req):
        n["calls"] += 1
        if n["calls"] == 1:
            return httpx.Response(429, headers={"retry-after": "1"}, json={"error": "slow down"})
        return httpx.Response(200, json={"text": "Shukriya bhai"})

    spec = {"id": "h", "kind": "whisper-only", "provider": "groq", "whisper_model": "w", "language": None}
    r = GroqWhisper(spec, GROQ, httpx.Client(transport=httpx.MockTransport(handler)), limiter()).transcribe(CLIP)
    assert r.error is None and r.text == "Shukriya bhai" and r.retries == 1


def test_bad_key_is_reported_not_raised():
    def handler(req):
        return httpx.Response(401, json={"error": {"message": "Invalid API Key"}})

    spec = {"id": "h", "kind": "whisper-only", "provider": "groq", "whisper_model": "w", "language": None}
    r = GroqWhisper(spec, GROQ, httpx.Client(transport=httpx.MockTransport(handler)), limiter()).transcribe(CLIP)
    assert r.error and "HTTP 401" in r.error


def test_missing_key_is_reported(monkeypatch):
    monkeypatch.delenv("GROQ_API_KEY")
    spec = {"id": "h", "kind": "whisper-only", "provider": "groq", "whisper_model": "w", "language": None}
    r = GroqWhisper(spec, GROQ, httpx.Client(), limiter()).transcribe(CLIP)
    assert r.error.startswith("missing GROQ_API_KEY")


def test_clean_output_strips_quotes_and_fences():
    assert clean_output('  "Theek hai."  ') == "Theek hai."
    assert clean_output("```\nPR merge kar do\n```") == "PR merge kar do"
    assert clean_output("a   b\n c") == "a b c"


def test_rate_limiter_spaces_requests():
    t = {"now": 0.0}
    slept = []

    def sleep(s):
        slept.append(s)
        t["now"] += s

    rl = RateLimiter(clock=lambda: t["now"], sleep=sleep)
    rl.wait("k", 30)  # 2 s apart
    rl.wait("k", 30)
    rl.wait("other", 30)  # other keys don't wait
    assert slept == [2.0]


def test_no_speech_marker_becomes_empty_text():
    assert clean_output("[no speech]") == ""
    assert clean_output(" [No speech]. ") == ""
