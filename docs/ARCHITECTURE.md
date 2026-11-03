# Architecture (as built)

> The map of what actually exists. Update it whenever modules, data, interfaces, commands or flows change.
> Only real paths and names: every one here must exist in the code.

## Runtime

The Milestone 0 benchmark is a Python command-line tool run by hand on the owner's PC. It calls the Groq and
Gemini HTTP APIs directly with the user's keys; there is no SonicScript server. The M1 Windows app is being
built in Rust in `desktop/`: so far only the platform-free text cleanup (`desktop/core`), no app binary yet.
The planned apps are described in `blueprint.md` §3 and §5.

```
record ──► spec/fixtures/audio/<id>.wav (16 kHz mono, trimmed)
bench  ──► encode (ffmpeg → Opus, cached) ──► engine (HTTP) ──► score ──► eval/results/<run>/results.jsonl
report ──► eval/results/<run>/report.html (self-contained, uses design/tokens.css)
```

## Code map

| Path | Responsibility |
|---|---|
| `LICENSE` | MIT (D-012) |
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
| `eval/sonic_eval/bench.py` | `run` (resumable), `rescore`, `summarise`, `pick_format`, `recommend`, presets |
| `eval/sonic_eval/report.py` | `build`: HTML report (recommendation, tables, latency chart, per-sentence view) |
| `eval/sonic_eval/__main__.py` | CLI: `record`, `probe`, `bench`, `report`, `rescore` |
| `eval/tests/` | Offline tests (mocked HTTP, synthetic audio, fake engines) |
| `spec/normalizer/` | `rules.json` (text cleanup rules as data) and `cases.json` (shared test cases), D-019 |
| `desktop/Cargo.toml` | Rust workspace (M1); members: `core` |
| `desktop/core/src/normalize.rs` | `Normalizer::new().with_word_list(..).clean(raw) -> Cleaned { text, needs_llm, phantom }`: script guard, phantom filter, word list, spacing; reads `spec/normalizer/rules.json` and `spec/prompts/whisper-hint-v1.txt` at build time |
| `desktop/core/examples/clean.rs` | Runs the cleanup on stdin lines, prints JSON lines (for re-scoring with `eval`) |
| `.claude/settings.json`, `.graphifyignore` | Graphify hook for Claude Code and what the graph skips (D-010); graph in `graphify-out/` (git-ignored) |

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
2. `bench.run` encodes all clips, skips finished jobs, then runs one thread pool per provider (`concurrency`).
3. Per job: `audio.encode` → `Engine.transcribe` (`_send` rate-limits, times the successful attempt, retries
   429/5xx) → `scoring.score` → one JSON line.
4. `summarise` → `recommend` / `pick_format` → `report.build`.

## Configuration

- `GROQ_API_KEY`, `GEMINI_API_KEY`: `eval/.env` first, then the process environment, then the Windows user
  environment (`HKCU\Environment`) (`config.load_env`).
- Model ids, prices, rpm budgets and per-provider `concurrency`: `spec/providers.json`. Presets: `bench.PRESETS`.

## Text cleanup (desktop/core)

```
Whisper text ─► remove / replace chars ─► count scripts (> 30% non-Latin → needs_llm)
             ─► romanize Urdu / Devanagari runs (word table, else letters) ─► phantom? → ""
             ─► word list (exact, then sound skeleton) ─► spacing + trailing space
```

## Commands

```
git config core.hooksPath .githooks        # once per clone
cd eval && pip install -e ".[dev]"         # needs ffmpeg on PATH
python -m sonic_eval record                # --list-devices, --device N, --only ID, --redo, --free N
python -m sonic_eval probe
python -m sonic_eval bench --preset compression
python -m sonic_eval bench --preset engines [--formats opus16]
python -m sonic_eval report --run m0
python -m sonic_eval rescore --run m0      # re-score saved outputs after a sentences.json fix (no API calls)
python -m ruff check . && python -m pytest -q
cd desktop && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cargo run -q --example clean -- [words.txt] < texts.txt   # (in desktop/) cleanup on one text per line
pip install --user graphifyy               # code graph (D-010); Scripts folder is on the user PATH
GRAPHIFY_BACKEND=none graphify extract . --code-only && graphify cluster-only . --no-label   # first build
graphify update .                          # after code changes; graphify query "..." to ask
```
