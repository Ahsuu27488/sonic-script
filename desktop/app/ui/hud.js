// The HUD Voice Ring (M2 task 3.2). Rust calls hudShow(view) / hudHide(); views come from desktop/app/src/hud.rs.
"use strict";
(() => {
  const pill = document.getElementById("pill");
  const label = document.getElementById("label");
  const timer = document.getElementById("timer");
  const glow = pill.querySelector(".glow");
  const ring = pill.querySelector(".ring");
  const shimmer = pill.querySelector(".shimmer");
  const invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
  const reduced = matchMedia("(prefers-reduced-motion: reduce)");

  // Attack ~40 ms, release ~180 ms (DESIGN.md "Motion").
  const ATTACK = 40;
  const RELEASE = 180;

  let kind = null;
  let started = 0;
  let level = 0; // smoothed
  let target = 0; // the last level from Rust
  let asking = false;
  let last = 0;
  let raf = 0;

  function paint() {
    const l = kind === "listening" ? level : 0.15;
    if (reduced.matches) {
      glow.style.setProperty("--o", (0.25 + 0.75 * l).toFixed(3));
    } else {
      glow.style.setProperty("--s", (0.3 + 0.7 * l).toFixed(3));
      glow.style.setProperty("--o", (0.45 + 0.55 * l).toFixed(3));
    }
    ring.style.setProperty("--h", (l * 1.2).toFixed(3));
  }

  function frame(now) {
    raf = 0;
    if (kind !== "listening") return;
    const dt = last ? Math.min(now - last, 100) : 16;
    last = now;
    if (invoke && !asking) {
      asking = true;
      invoke("level")
        .then((v) => { target = Number(v) || 0; }, () => {})
        .finally(() => { asking = false; });
    }
    const tau = target > level ? ATTACK : RELEASE;
    level += (target - level) * (1 - Math.exp(-dt / tau));
    paint();
    const s = Math.max(0, Math.floor((now - started) / 1000));
    const text = Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0");
    if (timer.textContent !== text) timer.textContent = text;
    raf = requestAnimationFrame(frame);
  }

  window.hudShow = (view) => {
    const was = kind;
    kind = view.kind;
    pill.dataset.kind = view.kind;
    pill.dataset.tone = view.tone;
    label.textContent = view.label;
    if (kind === "listening" && was !== "listening") {
      started = performance.now();
      level = 0;
      target = 0;
      last = 0;
      timer.textContent = "0:00";
    }
    if (kind === "writing") {
      // One shimmer per "Writing…".
      shimmer.style.animation = "none";
      void shimmer.offsetWidth;
      shimmer.style.animation = "";
    }
    paint();
    pill.hidden = false;
    requestAnimationFrame(() => pill.classList.add("in"));
    if (kind === "listening" && !raf) raf = requestAnimationFrame(frame);
  };

  window.hudHide = () => {
    kind = null;
    pill.classList.remove("in");
  };

  // A view sent before this script ran (a fast start-up failure) waits here.
  if (window.__hudPending) {
    window.hudShow(window.__hudPending);
    window.__hudPending = null;
  }
})();
