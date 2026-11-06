// The pop-up mic (M2 phase 5). A click on the ring starts or stops a recording (as a tap of the dictation key);
// the small x turns the pill off in this app. Rust calls popupRecording(bool) when a recording starts or ends.
"use strict";
(() => {
  const invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
  const bar = document.querySelector(".bar");
  const mic = document.getElementById("mic");
  const off = document.getElementById("off");

  // mousedown, not click: the press must not wait for the button to come up (it feels slow on a mic).
  mic.addEventListener("mousedown", (e) => {
    if (e.button !== 0 || !invoke) return;
    e.preventDefault();
    invoke("popup_click").then((started) => {
      // Dictation is paused or not running: say so with a small shake instead of nothing.
      if (!started) {
        bar.classList.remove("nope");
        void bar.offsetWidth;
        bar.classList.add("nope");
      }
    });
  });
  // The x must not take the focus either (its press would move it from the user's field).
  off.addEventListener("mousedown", (e) => e.preventDefault());
  off.addEventListener("click", () => invoke && invoke("popup_not_here"));

  window.popupRecording = (on) => {
    bar.classList.toggle("recording", on);
    mic.setAttribute("aria-pressed", String(on));
    mic.setAttribute("aria-label", on ? "Stop dictating" : "Dictate");
  };
})();
