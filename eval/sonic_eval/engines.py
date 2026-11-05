"""Speech engines: audio in, Roman Urdu / Hinglish text out, with timing and token usage.

Every engine reuses one keep-alive HTTP client per provider (as the real app will), so latency is measured
on a warm connection: from the first byte of the request to the full response, including the upload.
"""

from __future__ import annotations

import base64
import os
import time
from dataclasses import dataclass, field

import httpx

from .audio import EncodedAudio
from .config import load_prompt, wrap_transcript
from .ratelimit import RateLimiter

TIMEOUT = httpx.Timeout(30.0, connect=10.0)
MAX_RETRIES = 3


@dataclass
class EngineResult:
    text: str = ""
    latency_ms: float = 0.0
    stages_ms: dict[str, float] = field(default_factory=dict)
    raw_asr: str | None = None
    tokens_in: int = 0
    tokens_audio: int = 0
    tokens_out: int = 0
    refine_tokens_in: int = 0
    refine_tokens_out: int = 0
    retries: int = 0
    error: str | None = None
    unformatted: str | None = None  # why an optional format step failed (the engine's own text was kept)


class EngineError(Exception):
    pass


class Engine:
    """Base class. Subclasses implement `_run`, which fills an EngineResult."""

    def __init__(self, spec: dict, provider: dict, client: httpx.Client, limiter: RateLimiter,
                 word_list: list[str] | None = None, refiner: Refiner | None = None) -> None:
        self.spec = spec
        self.id: str = spec["id"]
        self.provider = provider
        self.client = client
        self.limiter = limiter
        self.word_list = word_list or []
        self.key = os.environ.get(provider.get("key_env", ""), "")
        self.refiner = refiner
        # The refiner is the format step (D-030) after an engine whose text is already Roman: if it fails,
        # that text is used unformatted, as in the app.
        self.optional_refine = False

    def transcribe(self, audio: EncodedAudio, sentence_id: str = "") -> EngineResult:
        result = EngineResult()
        try:
            self._run(audio, result, sentence_id)
            if self.refiner and result.text.strip():
                result.raw_asr = result.text
                try:
                    result.text = self.refiner.run(result.text, result, self.word_list)
                except (EngineError, httpx.HTTPError) as exc:
                    if not self.optional_refine:
                        raise
                    result.unformatted = str(exc)
            result.text = clean_output(result.text)
        except EngineError as exc:
            result.error = str(exc)
        except httpx.HTTPError as exc:
            result.error = f"network: {exc.__class__.__name__}: {exc}"
        result.latency_ms = round(sum(result.stages_ms.values()), 1)
        return result

    def cost_usd(self, r: EngineResult, duration_s: float) -> float:
        return self.refiner.cost_usd(r) if self.refiner else 0.0

    def _run(self, audio: EncodedAudio, result: EngineResult, sentence_id: str = "") -> None:
        raise NotImplementedError

    def _send(self, stage: str, model: str, result: EngineResult, request: httpx.Request) -> dict:
        """Rate-limit, send, retry on 429/5xx; record the time of the successful attempt only."""
        key = f"{self.spec['provider']}:{model}"
        for attempt in range(MAX_RETRIES + 1):
            self.limiter.wait(key, float(self.spec.get("rpm") or self.provider.get("rpm", 10)))
            start = time.perf_counter()
            resp = self.client.send(request)
            elapsed = (time.perf_counter() - start) * 1000
            if resp.status_code == 429 or resp.status_code >= 500:
                if attempt == MAX_RETRIES:
                    raise EngineError(f"{stage}: HTTP {resp.status_code} after {attempt} retries: {_short(resp)}")
                result.retries += 1
                self.limiter.back_off(key, _retry_after(resp, attempt))
                continue
            if resp.status_code >= 400:
                raise EngineError(f"{stage}: HTTP {resp.status_code}: {_short(resp)}")
            result.stages_ms[stage] = round(elapsed, 1)
            return resp.json()
        raise EngineError(f"{stage}: gave up")  # pragma: no cover


class GeminiAudio(Engine):
    """One call: audio + instruction -> Roman text."""

    def _run(self, audio: EncodedAudio, result: EngineResult, sentence_id: str = "") -> None:
        if not self.key:
            raise EngineError(f"missing {self.provider['key_env']} (add it to eval/.env)")
        model = self.spec["model"]
        body = {
            "contents": [{"role": "user", "parts": [
                {"text": load_prompt(self.spec["prompt"], self.word_list)},
                {"inline_data": {"mime_type": audio.mime, "data": base64.b64encode(audio.data).decode()}},
            ]}],
            "generationConfig": self.spec.get("generation_config", {"temperature": 0}),
        }
        req = self.client.build_request(
            "POST", f"{self.provider['base_url']}/models/{model}:generateContent",
            json=body, headers={"x-goog-api-key": self.key})
        data = self._send("gemini", model, result, req)
        cands = data.get("candidates") or []
        if not cands:
            raise EngineError(f"gemini: no candidates ({data.get('promptFeedback', 'no feedback')})")
        parts = cands[0].get("content", {}).get("parts", [])
        result.text = "".join(p.get("text", "") for p in parts if not p.get("thought"))
        usage = data.get("usageMetadata", {})
        result.tokens_in = int(usage.get("promptTokenCount", 0))
        result.tokens_out = int(usage.get("candidatesTokenCount", 0))
        for d in usage.get("promptTokensDetails", []):
            if d.get("modality") == "AUDIO":
                result.tokens_audio = int(d.get("tokenCount", 0))

    def cost_usd(self, r: EngineResult, duration_s: float) -> float:
        p = self.spec.get("price", {})
        own = (r.tokens_in * p.get("audio_in_per_m", 0) + r.tokens_out * p.get("out_per_m", 0)) / 1e6
        return own + super().cost_usd(r, duration_s)


class GeminiTranscribe(GeminiAudio):
    """Google's speech-to-text model (`gemini-3.5-transcribe`, the Gemini app's dictation): audio only, no
    instruction. The text comes back in `audioTranscription`, in the speaker's own script (Devanagari or
    Urdu for Hindi / Urdu speech), so it is paired with a `refine` step."""

    def _run(self, audio: EncodedAudio, result: EngineResult, sentence_id: str = "") -> None:
        if not self.key:
            raise EngineError(f"missing {self.provider['key_env']} (add it to eval/.env)")
        model = self.spec["model"]
        body = {"contents": [{"role": "user", "parts": [
            {"inline_data": {"mime_type": audio.mime, "data": base64.b64encode(audio.data).decode()}},
        ]}]}
        req = self.client.build_request(
            "POST", f"{self.provider['base_url']}/models/{model}:generateContent",
            json=body, headers={"x-goog-api-key": self.key})
        data = self._send("transcribe", model, result, req)
        cands = data.get("candidates") or []
        parts = cands[0].get("content", {}).get("parts", []) if cands else []
        result.text = " ".join(p.get("audioTranscription", {}).get("text", "") or p.get("text", "")
                               for p in parts).strip()
        usage = data.get("usageMetadata", {})
        result.tokens_in = int(usage.get("promptTokenCount", 0))
        result.tokens_out = int(usage.get("candidatesTokenCount", 0))


class GroqWhisper(Engine):
    """An OpenAI-style `/audio/transcriptions` call (Groq Whisper, OpenAI gpt-4o-transcribe), optionally
    steered by a Roman Urdu hint prompt."""

    def _whisper(self, audio: EncodedAudio, result: EngineResult) -> str:
        if not self.key:
            raise EngineError(f"missing {self.provider['key_env']} (add it to eval/.env)")
        model = self.spec["whisper_model"]
        form = {"model": model, "temperature": "0", "response_format": "json"}
        if self.spec.get("language"):
            form["language"] = self.spec["language"]
        if self.spec.get("hint"):
            form["prompt"] = load_prompt(self.spec["hint"])
        req = self.client.build_request(
            "POST", f"{self.provider['base_url']}/audio/transcriptions",
            data=form, files={"file": (f"audio.{audio.ext}", audio.data, audio.mime)},
            headers={"Authorization": f"Bearer {self.key}"})
        return str(self._send("whisper", model, result, req).get("text", "")).strip()

    def _run(self, audio: EncodedAudio, result: EngineResult, sentence_id: str = "") -> None:
        result.text = self._whisper(audio, result)

    def cost_usd(self, r: EngineResult, duration_s: float) -> float:
        # Groq bills Whisper with a 10-second minimum per request (provider `min_billed_s`); OpenAI has none.
        billed = max(duration_s, float(self.provider.get("min_billed_s", 0)))
        own = billed / 3600 * self.spec.get("price", {}).get("audio_per_hour", 0)
        return own + super().cost_usd(r, duration_s)


class GroqWhisperLLM(GroqWhisper):
    """Two calls: Whisper (Urdu script) -> small LLM (romanize + restore English words)."""

    def _run(self, audio: EncodedAudio, result: EngineResult, sentence_id: str = "") -> None:
        asr = self._whisper(audio, result)
        result.raw_asr = asr
        if not asr:
            return
        model = self.spec["llm_model"]
        body = {
            "model": model,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": load_prompt(self.spec["prompt"], self.word_list)},
                {"role": "user", "content": wrap_transcript(asr)},
            ],
            **self.spec.get("llm_extra", {}),
        }
        req = self.client.build_request(
            "POST", f"{self.provider['base_url']}/chat/completions",
            json=body, headers={"Authorization": f"Bearer {self.key}"})
        data = self._send("llm", model, result, req)
        result.text = str(data["choices"][0]["message"].get("content") or "")
        usage = data.get("usage", {})
        result.tokens_in = int(usage.get("prompt_tokens", 0))
        result.tokens_out = int(usage.get("completion_tokens", 0))

    def cost_usd(self, r: EngineResult, duration_s: float) -> float:
        p = self.spec.get("price", {})
        llm = (r.tokens_in * p.get("in_per_m", 0) + r.tokens_out * p.get("out_per_m", 0)) / 1e6
        return super().cost_usd(r, duration_s) + llm


class Refiner:
    """Second step after any engine (spec `refine`): an OpenAI-style chat model turns the transcript into
    clean Roman Urdu / Hinglish with the given prompt, on its own provider (e.g. Groq after Gemini)."""

    def __init__(self, spec: dict, provider: dict, client: httpx.Client, limiter: RateLimiter) -> None:
        self.spec = spec
        # Reuses Engine._send for rate limits, retries and timing; the stage is named "refine".
        self._sender = Engine({"id": "refine", "provider": spec["provider"], "rpm": spec.get("rpm")},
                              provider, client, limiter)

    def run(self, text: str, result: EngineResult, word_list: list[str]) -> str:
        """The step, checked as in the app (D-038): an answer `checked` refuses is asked for once more, then
        given up as an error."""
        prompt = load_prompt(self.spec["prompt"], word_list)
        first, why = checked(text, self._ask(text, result, prompt), prompt)
        if why is None:
            return first
        result.retries += 1
        again, why2 = checked(text, self._ask(text, result, prompt), prompt)
        if why2 is None:
            return again
        raise EngineError(f"the format answer looked wrong twice ({why}, then {why2})")

    def _ask(self, text: str, result: EngineResult, prompt: str) -> str:
        s = self._sender
        if not s.key:
            raise EngineError(f"missing {s.provider['key_env']} (add it to eval/.env)")
        model = self.spec["model"]
        body = {
            "model": model,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": prompt},
                {"role": "user", "content": wrap_transcript(text)},
            ],
            **self.spec.get("extra", {}),
        }
        req = s.client.build_request(
            "POST", f"{s.provider['base_url']}/chat/completions",
            json=body, headers={"Authorization": f"Bearer {s.key}"})
        data = s._send("refine", model, result, req)
        usage = data.get("usage", {})
        result.refine_tokens_in += int(usage.get("prompt_tokens", 0))
        result.refine_tokens_out += int(usage.get("completion_tokens", 0))
        return str(data["choices"][0]["message"].get("content") or "")

    def cost_usd(self, r: EngineResult) -> float:
        p = self.spec.get("price", {})
        return (r.refine_tokens_in * p.get("in_per_m", 0) + r.refine_tokens_out * p.get("out_per_m", 0)) / 1e6


def checked(said: str, answer: str, prompt: str) -> tuple[str, str | None]:
    """The text step's answer and why it must not be used (None when it is fine), as the app's
    `client::checked` (D-038): never empty for real speech, never its own instructions, never far longer or
    shorter than the transcript (an answer to a spoken question or order). Copied tags are removed."""
    out = answer.strip().removeprefix("<transcript>").removesuffix("</transcript>").strip()
    n_said, n_wrote = len(said.split()), len(out.split())
    if n_wrote == 0:
        return out, (None if n_said < 4 else "empty")
    copied = sum(1 for line in (ln.strip() for ln in prompt.splitlines()) if len(line) >= 30 and line in out)
    if copied >= 2:  # one copied line is fine: a speaker may say one of the prompt's examples
        return out, "its own instructions"
    if n_wrote > 2 * n_said + 10:
        return out, "far longer than what was said"
    if n_said >= 15 and n_wrote * 10 < n_said * 3:  # short take-backs may shrink a lot
        return out, "far shorter than what was said"
    return out, None


class FakeEngine(Engine):
    """Offline stand-in for tests and dry runs: returns the expected text after a simulated delay."""

    expected: dict[str, str] = {}

    def _run(self, audio: EncodedAudio, result: EngineResult, sentence_id: str = "") -> None:
        delay = float(self.spec.get("delay_ms", 5))
        time.sleep(delay / 1000)
        result.stages_ms["fake"] = delay
        result.text = self.spec.get("text") or self.expected.get(sentence_id, "")


KINDS: dict[str, type[Engine]] = {
    "gemini-audio": GeminiAudio,
    "gemini-transcribe": GeminiTranscribe,
    "whisper-only": GroqWhisper,
    "whisper-llm": GroqWhisperLLM,
    "fake": FakeEngine,
}


def build_engines(providers: dict, ids: list[str] | None, limiter: RateLimiter,
                  word_list: list[str] | None = None,
                  format_model: str | None = None) -> tuple[list[Engine], dict[str, httpx.Client]]:
    """`format_model` runs the spec's `format` step (D-030) with that Groq model after every engine, in place
    of its own refine step; the engine ids get `+fmt-<size>` (e.g. `gemini-3.5-flash+fmt-120b`)."""
    specs = providers["engines"]
    if ids:
        known = {s["id"] for s in specs}
        unknown = [i for i in ids if i not in known]
        if unknown:
            raise SystemExit(f"unknown engine(s): {', '.join(unknown)}. Known: {', '.join(sorted(known))}")
        specs = [s for s in specs if s["id"] in ids]
    clients: dict[str, httpx.Client] = {}

    def client_for(name: str) -> httpx.Client:
        if name not in clients:
            clients[name] = httpx.Client(timeout=TIMEOUT, http2=False)
        return clients[name]

    engines = []
    for spec in specs:
        prov_name = spec.get("provider", "fake")
        provider = providers.get("providers", {}).get(prov_name, {"rpm": 6000})
        refiner = None
        r = spec.get("refine")
        if format_model:
            r = {**providers["format"], "model": format_model}
        if r:
            refiner = Refiner(r, providers["providers"][r["provider"]], client_for(r["provider"]), limiter)
        engine = KINDS[spec["kind"]](spec, provider, client_for(prov_name), limiter, word_list, refiner)
        if format_model:
            engine.id += f"+fmt-{size_tag(format_model)}"
            engine.optional_refine = not spec.get("refine")
        engines.append(engine)
    return engines, clients


def size_tag(model: str) -> str:
    """openai/gpt-oss-120b -> 120b (the format step's model, in engine ids)."""
    return model.rsplit("-", 1)[-1]


def warm_up(providers: dict, clients_by_provider: dict[str, httpx.Client]) -> None:
    """Open the TLS connection before timing anything (the app does this when the hotkey goes down)."""
    for name, client in clients_by_provider.items():
        prov = providers["providers"].get(name)
        if not prov:
            continue
        key = os.environ.get(prov["key_env"], "")
        headers = {"x-goog-api-key": key} if name == "gemini" else {"Authorization": f"Bearer {key}"}
        try:
            client.get(f"{prov['base_url']}/models", headers=headers)
        except httpx.HTTPError:
            pass


NO_SPEECH = "[no speech]"


def clean_output(text: str) -> str:
    """Light cleanup every platform will do: trim, drop wrapping quotes/code fences, collapse spaces (line
    breaks stay: formatted lists, D-030; empty lines go), and turn the prompt's no-speech marker into empty text."""
    t = text.strip()
    if t.lower().strip(". ") == NO_SPEECH:
        return ""
    if t.startswith("```") and t.endswith("```"):
        t = t.strip("`").strip()
    if len(t) >= 2 and t[0] == t[-1] and t[0] in "\"'":
        t = t[1:-1].strip()
    lines = (" ".join(line.split()) for line in t.splitlines())
    return "\n".join(line for line in lines if line)


def _retry_after(resp: httpx.Response, attempt: int) -> float:
    try:
        return max(1.0, float(resp.headers.get("retry-after", "")))
    except ValueError:
        return 2.0 * (2**attempt)


def _short(resp: httpx.Response) -> str:
    text = resp.text.strip().replace("\n", " ")
    return text[:300]
