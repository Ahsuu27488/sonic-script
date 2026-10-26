# SonicScript design system

One design language for every SonicScript surface: the Windows/Linux HUD and settings window, the Android voice
keyboard, the benchmark report and, later, the website. Tokens live in `design/tokens.css` (source of truth);
native apps copy the same values.

## Direction

- **For:** developers and everyday chatters in Pakistan and India who think in Urdu/Hindi and type in Roman script,
  often while talking to AI agents (Claude Code, Codex) in a terminal.
- **Feeling:** a quiet, premium instrument. It appears the moment you speak, shows it is *listening*, and gets out of
  the way. Fast and calm, never flashy.
- **Signature idea: the Voice Ring.** A soft ring of light that breathes with your voice level. It is the HUD, the
  Android mic button, the app icon and the website hero. One shape, everywhere; nothing else competes with it.
- **Mode:** Operate (HUD, settings, keyboard, report) = restrained and fast. The website (later) = Experience, where
  the ring becomes a shader hero.

## Colour (OKLCH)

Dark is the default (the HUD floats over everything; dark reads well on any background). Light theme exists for the
Android keyboard and the settings window when the system is light.

| Token | Dark | Light | Use |
|---|---|---|---|
| `--bg` | `oklch(0.14 0.01 270)` | `oklch(0.985 0.004 80)` | page / window background |
| `--surface-1` | `oklch(0.17 0.01 270)` | `oklch(0.965 0.006 80)` | cards, panels |
| `--surface-2` | `oklch(0.20 0.012 270)` | `oklch(0.94 0.008 80)` | raised: inputs, keys |
| `--surface-3` | `oklch(0.23 0.014 270)` | `oklch(0.91 0.01 80)` | hover, pressed |
| `--border` | white 8% | black 9% | hairlines |
| `--text-1` / `-2` / `-3` | white 92 / 64 / 44% | `oklch(0.20 0.01 270)` 100 / 68 / 50% | text levels |
| **`--accent`** | `oklch(0.80 0.14 70)` "Ember" | `oklch(0.62 0.15 60)` | the Voice Ring, primary action, focus |
| `--accent-soft` | accent at 16% | accent at 14% | glows, selected rows |
| `--ok` | `oklch(0.78 0.13 160)` | `oklch(0.55 0.12 160)` | inserted / pass |
| `--warn` | `oklch(0.82 0.13 95)` | `oklch(0.60 0.13 85)` | rate limit, slow |
| `--danger` | `oklch(0.70 0.17 25)` | `oklch(0.55 0.19 25)` | error / fail |

- **One accent: Ember** (warm amber). Warm = human voice; it stands apart from the purple/blue AI look and from
  "recording red", which reads as an alarm. It is not a national or political colour on either side of the border.
- Never: gradient text, purple-to-blue gradients, neon borders. The only glow is the Voice Ring.

## Type

- **UI:** Geist (400/500/600). **Numbers, latencies, keys, code:** Geist Mono, always `font-variant-numeric:
  tabular-nums`. **Website headlines only:** Instrument Serif as the accent face.
- **Script fonts for output previews:** Noto Nastaliq Urdu (Urdu script mode, `dir="rtl"`, line-height 2.0) and
  Noto Sans Devanagari (Hindi mode). Roman text uses Geist. Bundle them in apps (all OFL).
- Scale (rem): 0.75 / 0.8125 / 0.875 / 1 / 1.125 / 1.375 / 1.75 / 2.25, display `clamp(2.25rem, 1.2rem + 4.5vw, 5.5rem)`.
  App body 14-15 px, report body 15-16 px, inputs never below 16 px on touch.

## Shape, space, depth

- Spacing: 4 px base: 4, 8, 12, 16, 20, 24, 32, 40, 56, 80.
- Radius: 6 (chips, keys), 10 (inputs, buttons), 16 (cards, panels), 999 (HUD pill, mic button). Nested radii are
  concentric (inner = outer - padding).
- Shadows: dark theme uses light (1 px top highlight + soft ambient), not heavy drop shadows.
- **Glass only on the floating layer:** the HUD pill (Windows acrylic via Tauri `window-vibrancy`, CSS
  `backdrop-filter` fallback) and modal sheets. Never glass-on-glass, never behind long text. Solid `--surface-2`
  when Windows "Transparency effects" is off or `prefers-reduced-transparency`.

## Motion

| Token | Value | Use |
|---|---|---|
| `--dur-fast` | 120 ms | hover, press |
| `--dur-ui` | 200 ms | HUD appear, state change |
| `--dur-exit` | 160 ms | HUD hide (exits faster than entries) |
| `--ease-out` | `cubic-bezier(0.22, 1, 0.36, 1)` | entries |
| `--ease-in-out` | `cubic-bezier(0.65, 0, 0.35, 1)` | morphs |
| spring | stiffness 420, damping 32 | pill width change (native) |

- The ring follows mic RMS, smoothed (attack ~40 ms, release ~180 ms), drawn with transform/opacity only, 60 fps.
- Press feedback `scale(0.97)`. No looping animations except the ring while listening and a subtle shimmer while
  processing.
- `prefers-reduced-motion` (and Android "Remove animations"): the ring becomes a static ring whose opacity follows
  the level; no shimmer.

## Surfaces

### HUD pill (Windows/Linux)
- 44 px tall, radius 999, bottom-centre of the active monitor by default (12% from the bottom), or near the caret
  when the focus pill (M5) knows it. Never takes focus; click-through except on the pill.
- States, one at a time:
  1. **Listening:** Voice Ring (left, 28 px) + live timer in Geist Mono. Width 132 px.
  2. **Processing:** ring holds still, a soft shimmer runs once around it; label "Writing…".
  3. **Inserted:** a 600 ms tick in `--ok`, then the pill exits.
  4. **Copied (injection failed):** "Copied: press Ctrl+V" for 2.5 s.
  5. **Error / offline / rate limited:** `--danger`/`--warn` dot + one plain sentence + the action ("Retrying on
     Groq…", "Add a Gemini key in Settings").
- Optional live preview (v2 streaming): text grows to the right, max 520 px, fades at the left edge.

### Settings window (Tauri)
- Operate mode. 760 x 540 default, resizable to 560 px wide (sidebar becomes a top tab row).
- Sections: **General** (hotkey, output mode, start with Windows), **Engines & keys** (provider, key field with
  show/hide + "Test key", fallback order), **Words** (personal word list, spelling style), **History** (last 50,
  copy / re-insert / delete, off switch), **About** (version, privacy summary, links).
- Hotkey picker: a key-cap control showing `Right Ctrl`; press "Change" then the new key.

### Android voice keyboard (IME)
- Height 264 dp (matches Gboard so the app does not jump). Follows system light/dark.
- Centre: **88 dp mic button = the Voice Ring.** Tap to start/stop, or hold to talk (setting).
- Row under it: switch-keyboard key, mode chip (`Roman · English · اردو · हिंदी`), space, backspace (hold to
  repeat), enter. Above the mic: the last result with "undo".
- Haptics: light tick on start, success tick on insert.

### Benchmark report (M0)
- Read/Operate. Single self-contained HTML file. Hero line with the recommended engine, then a summary table
  (sortable feel, tabular numbers, pass/fail chips), a latency strip chart per engine, then a per-sentence table
  with the audio player, expected text and each engine's output with mistakes highlighted.
- Reflows to cards below 720 px; tables scroll inside their container.

### Website (after launch, M4)
- Experience mode: the Voice Ring as a WebGL/CSS shader hero reacting to a demo clip, the side-by-side
  "Win+H vs SonicScript" demo as the scroll story, glass nav pill. Built later with the full design-craft pass.

## Copy

- Plain, short, friendly. Buttons are verbs ("Test key", "Re-insert"). Errors say what happened and what to do.
- Product voice may use a little Roman Urdu/Hinglish in onboarding ("Bolo, hum likh denge."), never in errors.

## Accessibility

- Contrast 4.5:1 for text, 3:1 for the ring against both glass and solid fallbacks. Visible focus ring
  (2 px `--accent`, 2 px offset) in settings and the report.
- HUD states are also announced (Windows UI Automation live region / Android `announceForAccessibility`).
- Every control reachable by keyboard; touch targets 48 dp on Android.
