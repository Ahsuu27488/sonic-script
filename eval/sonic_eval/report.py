"""Self-contained HTML report for a benchmark run, styled with the SonicScript design tokens (DESIGN.md)."""

from __future__ import annotations

import html
import os
import time
from pathlib import Path

from .bench import FORMAT_ORDER, percentile, pick_format, read_rows, recommend, summarise
from .config import AUDIO_DIR, TOKENS_CSS, Sentence, load_sentences
from .scoring import NON_LATIN, has_term

TARGET_MS = 1200
FONTS = ("https://fonts.googleapis.com/css2?family=Geist:wght@400;500;600&family=Geist+Mono:wght@400;500"
         "&family=Noto+Nastaliq+Urdu&family=Noto+Sans+Devanagari&display=swap")

CSS = """
*{box-sizing:border-box}
html{-webkit-text-size-adjust:100%}
body{margin:0;background:var(--bg);color:var(--text-1);font:400 15px/1.55 var(--font-ui);
  -webkit-font-smoothing:antialiased}
.wrap{width:min(100% - 2rem,1180px);margin:0 auto;padding:var(--space-10) 0 var(--space-20)}
header{display:flex;flex-wrap:wrap;align-items:center;gap:var(--space-3) var(--space-6);margin-bottom:var(--space-8)}
.brand{display:flex;align-items:center;gap:var(--space-3);font-weight:600;font-size:var(--text-lg)}
.ring{width:28px;height:28px;border-radius:50%;border:2px solid var(--accent);box-shadow:var(--ring-glow);
  position:relative;flex:none}
.ring::after{content:"";position:absolute;inset:6px;border-radius:50%;background:var(--accent);opacity:.9}
.meta{color:var(--text-3);font:400 var(--text-sm)/1.4 var(--font-mono)}
h1{font-size:clamp(1.75rem,1.2rem + 2vw,2.5rem);line-height:1.15;letter-spacing:-.02em;margin:0 0 var(--space-3);
  font-weight:600;text-wrap:balance}
h2{font-size:var(--text-xl);letter-spacing:-.01em;margin:var(--space-14) 0 var(--space-2);font-weight:600}
.lede{color:var(--text-2);max-width:68ch;margin:0}
.nw{white-space:nowrap;font-family:var(--font-mono);font-weight:500;letter-spacing:-.03em}
.hero{background:var(--surface-1);border:1px solid var(--border);border-radius:var(--radius-md);
  padding:clamp(1.25rem,3vw,2rem);box-shadow:var(--shadow-raise)}
.picks{display:grid;gap:var(--space-3);grid-template-columns:repeat(auto-fit,minmax(220px,1fr));margin-top:var(--space-6)}
.pick{background:var(--surface-2);border:1px solid var(--border);border-radius:var(--radius-sm);padding:var(--space-4)}
.pick .k{color:var(--text-3);font-size:var(--text-sm)}
.pick .v{font:500 var(--text-lg)/1.3 var(--font-mono);margin-top:2px;overflow-wrap:anywhere}
.pick.main{border-color:var(--accent);background:linear-gradient(var(--accent-soft),var(--accent-soft)),var(--surface-2)}
.note{color:var(--text-2);font-size:var(--text-md);margin:var(--space-2) 0 var(--space-4);max-width:75ch}
.scroll{overflow-x:auto;border:1px solid var(--border);border-radius:var(--radius-md);background:var(--surface-1)}
table{border-collapse:collapse;width:100%;font-size:var(--text-md)}
th,td{padding:10px 14px;text-align:right;white-space:nowrap;border-bottom:1px solid var(--border)}
th{color:var(--text-3);font-weight:500;font-size:var(--text-sm);position:sticky;top:0;background:var(--surface-1)}
td:first-child,th:first-child,td.l,th.l{text-align:left}
tr:last-child td{border-bottom:0}
tbody tr:hover td{background:var(--surface-2)}
.num{font-family:var(--font-mono);font-variant-numeric:tabular-nums}
.chip{display:inline-block;padding:1px 8px;border-radius:var(--radius-pill);font:500 var(--text-xs)/1.6 var(--font-mono)}
.good{background:var(--ok-soft);color:var(--ok)}.mid{background:var(--warn-soft);color:var(--warn)}
.bad{background:var(--danger-soft);color:var(--danger)}
tr.best td{background:var(--accent-soft)}
.chart{background:var(--surface-1);border:1px solid var(--border);border-radius:var(--radius-md);padding:var(--space-4)}
.chart svg{width:100%;height:auto;display:block}
.chart text{fill:var(--text-3);font:400 11px var(--font-mono)}
.chart .lbl{fill:var(--text-2);font:500 12px var(--font-ui)}
details.s{background:var(--surface-1);border:1px solid var(--border);border-radius:var(--radius-sm);margin:var(--space-2) 0}
details.s summary{cursor:pointer;padding:12px 16px;display:flex;gap:var(--space-3);align-items:baseline;list-style:none}
details.s summary::-webkit-details-marker{display:none}
details.s summary:focus-visible{outline:2px solid var(--accent);outline-offset:2px;border-radius:var(--radius-sm)}
details.s[open] summary{border-bottom:1px solid var(--border)}
.sid{font:500 var(--text-sm) var(--font-mono);color:var(--text-3);flex:none;width:5.5em}
.stext{overflow-wrap:anywhere}
.body{padding:var(--space-4);display:grid;gap:var(--space-3)}
audio{width:100%;max-width:420px;height:36px}
.out{display:grid;grid-template-columns:minmax(0,14em) minmax(0,1fr) auto;gap:var(--space-1) var(--space-4);
  align-items:baseline;font-size:var(--text-md)}
.out .e{color:var(--text-3);font:400 var(--text-sm) var(--font-mono);overflow-wrap:anywhere}
.out .t{overflow-wrap:anywhere}
.raw{color:var(--text-3);font-family:var(--font-urdu),var(--font-devanagari),var(--font-ui);line-height:2}
mark{background:var(--danger-soft);color:var(--danger);border-radius:4px;padding:0 3px}
.miss{color:var(--danger);font-size:var(--text-sm)}
.filters{display:flex;flex-wrap:wrap;gap:var(--space-2);margin:var(--space-3) 0}
.filters button{font:500 var(--text-sm) var(--font-ui);color:var(--text-2);background:var(--surface-2);
  border:1px solid var(--border);border-radius:var(--radius-pill);padding:6px 14px;min-height:36px;cursor:pointer;
  transition:background var(--dur-fast) var(--ease-out),color var(--dur-fast) var(--ease-out)}
.filters button:hover{background:var(--surface-3);color:var(--text-1)}
.filters button:active{transform:scale(.97)}
.filters button[aria-pressed=true]{background:var(--accent);color:var(--on-accent);border-color:transparent}
.filters button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
footer{margin-top:var(--space-14);color:var(--text-3);font-size:var(--text-sm)}
@media (max-width:720px){
  .out{grid-template-columns:1fr}
  .out .e{margin-top:var(--space-2)}
  th,td{padding:8px 10px}
}
"""

JS = """
const btns=[...document.querySelectorAll('.filters button')];
btns.forEach(b=>b.addEventListener('click',()=>{
  btns.forEach(x=>x.setAttribute('aria-pressed',x===b));
  const c=b.dataset.cat;
  document.querySelectorAll('details.s').forEach(d=>{d.hidden=!(c==='all'||d.dataset.cat===c||(c==='issues'&&d.dataset.issue==='1'))});
}));
"""


def build(run_dir: Path, audio_dir: Path = AUDIO_DIR, sentences: list[Sentence] | None = None) -> Path:
    rows = read_rows(run_dir / "results.jsonl")
    if not rows:
        raise SystemExit(f"No results in {run_dir}")
    sentences = sentences or load_sentences()
    by_id = {s.id: s for s in sentences}
    summary = summarise(rows)
    rec = recommend(summary, TARGET_MS)
    engines = sorted({s["engine"] for s in summary})
    fmt_pick = {e: pick_format(summary, e) for e in engines}
    out = run_dir / "report.html"
    rel_audio = Path(os.path.relpath(audio_dir, run_dir)).as_posix()

    parts = [_head(run_dir.name)]
    parts.append(_hero(rec, fmt_pick, rows, len({r["sentence"] for r in rows})))
    parts.append(_summary_table(summary, rec))
    parts.append(_compression(summary, engines, fmt_pick))
    main_fmt = main_formats(summary)
    parts.append(_latency_chart(rows, main_fmt, rec))
    parts.append(_sentences(rows, by_id, main_fmt, rel_audio))
    parts.append(f"<footer>Generated {time.strftime('%Y-%m-%d %H:%M')} by <code>python -m sonic_eval report</code>."
                 f" Latency is measured from this PC on a warm connection, upload included.</footer>")
    parts.append(f"</div><script>{JS}</script></body></html>")
    out.write_text("\n".join(parts), encoding="utf-8")
    return out


def _head(name: str) -> str:
    tokens = TOKENS_CSS.read_text(encoding="utf-8") if TOKENS_CSS.exists() else ""
    return f"""<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Engine Benchmark · SonicScript</title>
<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="{FONTS}">
<style>{tokens}{CSS}</style></head><body><div class="wrap">
<header><div class="brand"><span class="ring" aria-hidden="true"></span>SonicScript</div>
<span class="meta">M0 engine benchmark · run {html.escape(name)}</span></header>"""


def _hero(rec: dict | None, fmt_pick: dict, rows: list[dict], n_sent: int) -> str:
    if not rec:
        return ('<section class="hero"><h1>No engine passed yet</h1><p class="lede">Every engine either had '
                'errors or wrote non-Latin script. Check the errors in the table below.</p></section>')
    b, f = rec["best"], rec["fallback"]
    target = "within" if rec["within_target"] else "<strong>outside</strong>"
    fmt = fmt_pick.get(b["engine"]) or b["fmt"]
    picks = [("Default engine", b["engine"], True),
             ("Fallback engine", f["engine"] if f else "needs a run with the other provider", False),
             ("Audio format", fmt, False),
             ("Latency p50 / p95", f"{b['p50']} / {b['p95']} ms", False),
             ("Accuracy · English kept", f"{b['accuracy']:.1%} · {b['english']:.0%}", False),
             ("Cost per 1,000 dictations", f"${b['cost_per_1k']:.2f}", False)]
    cards = "".join(f'<div class="pick{" main" if m else ""}"><div class="k">{k}</div>'
                    f'<div class="v">{html.escape(str(v))}</div></div>' for k, v, m in picks)
    return (f'<section class="hero"><h1>Use <span class="nw">{html.escape(b["engine"])}</span> for Roman Urdu dictation</h1>'
            f'<p class="lede">Best quality among engines that kept Latin script and had no errors, {target} the '
            f'{TARGET_MS} ms p95 target. Based on {len(rows)} requests over {n_sent} sentences.</p>'
            f'<div class="picks">{cards}</div></section>')


def _chip(v: float, good: float, mid: float, pct: bool = True, invert: bool = False) -> str:
    cls = ("good" if v <= good else "mid" if v <= mid else "bad") if invert else \
          ("good" if v >= good else "mid" if v >= mid else "bad")
    text = f"{v:.0%}" if pct else f"{v:,.0f}"
    return f'<span class="chip {cls}">{text}</span>'


def _summary_table(summary: list[dict], rec: dict | None) -> str:
    best = (rec["best"]["engine"], rec["best"]["fmt"]) if rec else None
    body = []
    for s in summary:
        cls = ' class="best"' if (s["engine"], s["fmt"]) == best else ""
        err = f'<span class="chip bad">{s["errors"]}</span>' if s["errors"] else '<span class="num">0</span>'
        body.append(
            f'<tr{cls}><td>{html.escape(s["engine"])}</td><td class="l num">{s["fmt"]}</td>'
            f'<td class="num">{s["n"]}</td><td>{err}</td>'
            f'<td>{_chip(s["script_ok"], .99, .95)}</td><td>{_chip(s["english"], .95, .85)}</td>'
            f'<td>{_chip(s["accuracy"], .9, .8)}</td>'
            f'<td>{_chip(s["silence_ok"], 1, .5) if s["silence_ok"] is not None else "–"}</td>'
            f'<td>{_chip(s["p50"], 700, 1200, pct=False, invert=True)}</td>'
            f'<td>{_chip(s["p95"], 1200, 2000, pct=False, invert=True)}</td>'
            f'<td class="num">{s["kb"]}</td><td class="num">${s["cost_per_1k"]:.3f}</td></tr>')
    return ('<h2>All engines</h2><p class="note">Latin = outputs fully in Roman script. English = expected English '
            'words kept in English spelling. Accuracy = 1 − character error rate after folding spelling variants '
            '(kar do = kardo, hai = hay). Quiet = silence clips that correctly produced no text (made-up text there '
            'would be typed into your app). Latency in ms, speech clips only.</p><div class="scroll"><table><thead><tr>'
            '<th>Engine</th><th class="l">Format</th><th>Runs</th><th>Errors</th><th>Latin</th><th>English</th>'
            '<th>Accuracy</th><th>Quiet</th><th>p50</th><th>p95</th><th>KB</th><th>$ / 1k</th></tr></thead><tbody>'
            + "".join(body) + "</tbody></table></div>")


def _compression(summary: list[dict], engines: list[str], fmt_pick: dict) -> str:
    fmts = [f for f in FORMAT_ORDER if any(s["fmt"] == f for s in summary)]
    if len(fmts) < 2:
        return ""
    idx = {(s["engine"], s["fmt"]): s for s in summary}
    head = "".join(f"<th>{f}</th>" for f in fmts)
    body = []
    for e in engines:
        if sum((e, f) in idx for f in fmts) < 2:
            continue
        cells = []
        for f in fmts:
            s = idx.get((e, f))
            if not s:
                cells.append("<td>–</td>")
                continue
            mark = " ◂ pick" if fmt_pick.get(e) == f else ""
            cells.append(f'<td class="num">{s["accuracy"]:.1%} · {s["kb"]} KB<span class="meta">{mark}</span></td>')
        body.append(f"<tr><td>{html.escape(e)}</td>{''.join(cells)}</tr>")
    if not body:
        return ""
    return ('<h2>How small can the audio be?</h2><p class="note">Accuracy and average upload size per format. '
            'The pick is the smallest format whose accuracy stays within 1 point (and English within 2 points) of '
            'the best format for that engine.</p><div class="scroll"><table><thead><tr><th>Engine</th>'
            f'{head}</tr></thead><tbody>{"".join(body)}</tbody></table></div>')


def main_formats(summary: list[dict]) -> dict[str, str]:
    """Each engine's most-run format (the engines phase), used for the chart and the sentence view."""
    best: dict[str, str] = {}
    for s in sorted(summary, key=lambda s: s["n"]):
        best[s["engine"]] = s["fmt"]
    return best


def _latency_chart(rows: list[dict], main_fmt: dict[str, str], rec: dict | None) -> str:
    """Strip chart: one row per engine (at its main format), dots = requests, ticks = p50/p95."""
    series = []
    for e, f in sorted(main_fmt.items()):
        lat = [r["latency_ms"] for r in rows if r["engine"] == e and r["fmt"] == f and not r["error"]]
        if lat:
            series.append((e, f, lat))
    if not series:
        return ""
    xmax = max(2000.0, max(percentile(lat, 98) for _, _, lat in series) * 1.1)
    w, left, right, rowh, top = 960, 270, 24, 44, 28
    h = top + rowh * len(series) + 30
    x = lambda v: left + min(v, xmax) / xmax * (w - left - right)  # noqa: E731
    svg = [f'<svg viewBox="0 0 {w} {h}" role="img" aria-label="Latency per engine">']
    step = next(st for st in (250, 500, 1000, 2500, 5000, 10000) if xmax / st <= 10)
    for t in range(0, int(xmax) + 1, step):
        svg.append(f'<line x1="{x(t):.1f}" x2="{x(t):.1f}" y1="{top - 8}" y2="{h - 24}" '
                   f'stroke="var(--border)" stroke-width="1"/>'
                   f'<text x="{x(t):.1f}" y="{h - 8}" text-anchor="middle">{t}</text>')
    svg.append(f'<line x1="{x(TARGET_MS):.1f}" x2="{x(TARGET_MS):.1f}" y1="{top - 14}" y2="{h - 24}" '
               f'stroke="var(--accent)" stroke-width="1.5" stroke-dasharray="4 4"/>'
               f'<text x="{x(TARGET_MS) + 6:.1f}" y="{top - 6}" style="fill:var(--accent)">p95 target</text>')
    best = rec["best"]["engine"] if rec else None
    for i, (e, f, lat) in enumerate(series):
        cy = top + rowh * i + rowh / 2
        weight = "600" if e == best else "500"
        svg.append(f'<text class="lbl" x="0" y="{cy + 4:.1f}" style="font-weight:{weight}">{html.escape(e)}<tspan dx="6" style="fill:var(--text-3);font-weight:400">{f}</tspan></text>')
        for v in lat:
            svg.append(f'<circle cx="{x(v):.1f}" cy="{cy:.1f}" r="3" fill="var(--text-2)" fill-opacity=".35"/>')
        for p, col in ((50, "var(--text-1)"), (95, "var(--accent)")):
            px = x(percentile(lat, p))
            svg.append(f'<line x1="{px:.1f}" x2="{px:.1f}" y1="{cy - 11:.1f}" y2="{cy + 11:.1f}" '
                       f'stroke="{col}" stroke-width="2.5" stroke-linecap="round"/>')
    svg.append("</svg>")
    return ('<h2>Latency</h2><p class="note">Each dot is one request (key release → full text back). White tick = '
            'p50, amber tick = p95. Values above the axis are clipped to the right edge.</p>'
            f'<div class="chart">{"".join(svg)}</div>')


def _sentences(rows: list[dict], by_id: dict[str, Sentence], main_fmt: dict[str, str], rel_audio: str) -> str:
    """Per sentence: audio, expected text and each engine's first output at its main format."""
    by_sent: dict[str, dict[tuple[str, str], dict]] = {}
    for r in sorted(rows, key=lambda r: r["repeat"], reverse=True):
        if main_fmt.get(r["engine"]) != r["fmt"]:
            continue
        by_sent.setdefault(r["sentence"], {})[(r["engine"], r["fmt"])] = r
    cats = sorted({by_id[s].category for s in by_sent if s in by_id})
    filters = ['<button data-cat="all" aria-pressed="true">All</button>',
               '<button data-cat="issues" aria-pressed="false">With issues</button>']
    filters += [f'<button data-cat="{c}" aria-pressed="false">{html.escape(c)}</button>' for c in cats]
    items = []
    for sid in sorted(by_sent, key=lambda s: list(by_id).index(s) if s in by_id else 999):
        s = by_id.get(sid)
        if not s:
            continue
        outs, issue = [], False
        for (engine, fmt), r in sorted(by_sent[sid].items()):
            missing = [t for t in s.english if not has_term(r["text"], t)]
            bad_script = not r["error"] and not r["script_ok"]
            issue = issue or bool(missing) or bad_script or bool(r["error"]) or r["accuracy"] < 0.85
            if r["error"]:
                text = f'<span class="miss">{html.escape(r["error"])}</span>'
            elif s.is_silence and r["text"]:
                text = f'<span class="miss">made up:</span> {_mark_script(r["text"])}'
            elif s.is_silence:
                text = '<span class="meta">(no text: correct)</span>'
            else:
                text = _mark_script(r["text"])
                if missing:
                    text += f' <span class="miss">missing: {html.escape(", ".join(missing))}</span>'
                if r.get("raw_asr"):
                    text += f'<div class="raw" dir="auto">{html.escape(r["raw_asr"])}</div>'
            acc = "" if r["error"] else _chip(r["accuracy"], .9, .8)
            outs.append(f'<div class="e">{html.escape(engine)} · {fmt}</div><div class="t">{text}</div>'
                        f'<div>{acc} <span class="meta">{r["latency_ms"]:.0f} ms</span></div>')
        items.append(
            f'<details class="s" data-cat="{html.escape(s.category)}" data-issue="{int(issue)}">'
            f'<summary><span class="sid">{html.escape(sid)}</span><span class="stext">{html.escape(s.text or s.say)}</span>'
            f'</summary><div class="body"><audio controls preload="none" src="{rel_audio}/{sid}.wav"></audio>'
            f'<div class="out">{"".join(outs)}</div></div></details>')
    return ('<h2>Sentence by sentence</h2><p class="note">Open a sentence to hear it and compare outputs. '
            'Red = non-Latin characters or English words that went missing.</p>'
            f'<div class="filters" role="group" aria-label="Filter sentences">{"".join(filters)}</div>'
            + "".join(items))


def _mark_script(text: str) -> str:
    out, last = [], 0
    for m in NON_LATIN.finditer(text):
        out.append(html.escape(text[last:m.start()]))
        out.append(f"<mark>{html.escape(m.group())}</mark>")
        last = m.end()
    out.append(html.escape(text[last:]))
    return "".join(out)
