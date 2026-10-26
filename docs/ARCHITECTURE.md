# Architecture (as built)

> The map of what actually exists. Update it whenever modules, data, interfaces, commands or flows change.
> Only real paths and names: every one here must exist in the code.

## Runtime

Only the Milestone 0 benchmark exists: a Python command-line tool run by hand on the owner's PC. It calls the
Groq and Gemini HTTP APIs directly with the user's keys; there is no SonicScript server. The planned apps are
described in `blueprint.md` §3 and §5.

```
record ──► spec/fixtures/audio/<id>.wav (16 kHz mono, trimmed)
bench  ──► encode (ffmpeg → Opus, cached) ──► engine (HTTP) ──► score ──► eval/results/<run>/results.jsonl
report ──► eval/results/<run>/report.html (self-contained, uses design/tokens.css)
```

## Code map

| Path | Responsibility |
|---|---|
| `blueprint.md` | Product plan (v1); `archive/blueprint-v0.md` is the original draft |
| `DESIGN.md`, `design/tokens.css` | Design system and its tokens (the report inlines the CSS) |
| `spec/fixtures/sentences.json` | Test set: id, category, text, expected English words, `say` for silence items |
| `spec/prompts/` | `transcribe-roman-v1.md` (audio → Roman, Gemini), `romanize-v1.md` (Whisper text → Roman, LLM), `whisper-hint-v1.txt` (Whisper prompt) |
| `spec/providers.json` | Providers (base URL, key env var, rpm budget), engines (model ids, prompt, price), formats |
| `eval/sonic_eval/config.py` | Repo paths, loaders for the spec files, key loading (`load_env`) |
| `eval/sonic_eval/audio.py` | WAV read/write, `trim_silence`, `encode` (wav / opusNN via ffmpeg, cached) |
| `eval/sonic_eval/record.py` | Terminal recorder (`record_set`, `record_free`), level meter, playback |
| `eval/sonic_eval/engines.py` | `GeminiAudio`, `GroqWhisper`, `GroqWhisperLLM`, `FakeEngine`; retries, `clean_output` |
| `eval/sonic_eval/ratelimit.py` | `RateLimiter`: spaces requests per provider:model, backs off after 429 |
| `eval/sonic_eval/scoring.py` | `score`: Latin-script check, English words kept, spelling-tolerant accuracy |
| `eval/sonic_eval/bench.py` | `run` (resumable), `summarise`, `pick_format`, `recommend`, presets |
| `eval/sonic_eval/report.py` | `build`: HTML report (recommendation, tables, latency chart, per-sentence view) |
| `eval/sonic_eval/__main__.py` | CLI: `record`, `probe`, `bench`, `report` |
| `eval/tests/` | Offline tests (mocked HTTP, synthetic audio, fake engines) |

## Data

- `spec/fixtures/audio/*.wav` and `free.json`: the owner's recordings and free-speech transcripts. Git-ignored
  (D-004). Written by `record`, read by `bench`, `probe`, `report`.
- `eval/results/<run>/results.jsonl`: one JSON line per request (engine, fmt, sentence, repeat, text, raw ASR,
  latency and stages, bytes, tokens, cost, error, score fields). Appended by `bench`; the latest line per key wins.
  Git-ignored. `eval/results/cache/<fmt>/` holds encoded audio.

## Interfaces

- Engine contract: `Engine.transcribe(EncodedAudio, sentence_id) -> EngineResult` (text, latency_ms, stages_ms,
  raw_asr, tokens, retries, error). Errors are returned, never raised.
- Gemini: `POST {base}/models/{model}:generateContent`, header `x-goog-api-key`, audio as `inline_data`.
- Groq: `POST {base}/audio/transcriptions` (multipart) and `POST {base}/chat/completions`, `Authorization: Bearer`.

## How a benchmark run flows

1. `__main__.main` (`bench`) → `build_engines` + `warm_up` (opens the TLS connections first).
2. `bench.run` builds jobs sentence-major (providers alternate, so rate-limit waits overlap), skips finished ones.
3. Per job: `audio.encode` → `Engine.transcribe` (`_send` rate-limits, times the successful attempt, retries
   429/5xx) → `scoring.score` → one JSON line.
4. `summarise` → `recommend` / `pick_format` → `report.build`.

## Configuration

- `GROQ_API_KEY`, `GEMINI_API_KEY`: process environment, then the Windows user environment (`HKCU\Environment`),
  then `eval/.env` (`config.load_env`).
- Model ids, prices and rpm budgets: `spec/providers.json`. Presets: `bench.PRESETS`.

## Commands

```
git config core.hooksPath .githooks        # once per clone
cd eval && pip install -e ".[dev]"         # needs ffmpeg on PATH
python -m sonic_eval record                # --list-devices, --device N, --only ID, --redo, --free N
python -m sonic_eval probe
python -m sonic_eval bench --preset compression
python -m sonic_eval bench --preset engines [--formats opus16]
python -m sonic_eval report --run m0
python -m ruff check . && python -m pytest -q
```
