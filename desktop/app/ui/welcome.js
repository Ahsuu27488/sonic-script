// First run (M2 phase 7). Saves keys with the settings window's commands (prefs.rs); keys never come back
// to the page, only "ends in 1a2b". User text only ever goes into textContent.
"use strict";
(() => {
  const tauri = window.__TAURI__;
  const call = (cmd, args) =>
    tauri ? tauri.core.invoke(cmd, args) : Promise.reject(new Error("Open this page from SonicScript."));
  const $ = (id) => document.getElementById(id);
  const say = (id, text, tone = "") => {
    const r = $(id);
    r.textContent = text;
    r.className = "result" + (tone ? " " + tone : "");
  };
  const LINKS = { gemini: "https://aistudio.google.com/apikey", groq: "https://console.groq.com/keys" };

  function ready(keys) {
    const g = keys.find((k) => k.provider === "gemini");
    const on = !!(g && g.source);
    $("step-key").classList.toggle("done", on);
    $("step-try").setAttribute("aria-disabled", on ? "false" : "true");
    $("try").disabled = !on;
    const line = (k) => "Key ready" + (k.source.last4 ? ", ends in " + k.source.last4 : "") + ".";
    if (on && !$("gemini").value) say("gemini-result", line(g), "ok");
    const q = keys.find((k) => k.provider === "groq");
    if (q && q.source) say("groq-result", line(q), "ok");
    return on;
  }

  // Tests the pasted key first; a refused key is not saved. When the check itself fails (no internet),
  // the key is saved anyway and the user is told.
  async function saveKey(provider, button) {
    const input = $(provider);
    const value = input.value.trim();
    const out = provider + "-result";
    if (!value) return say(out, "Paste the key first.", "bad");
    button.disabled = true;
    say(out, "Checking the key…");
    let note = "It works.";
    try {
      await call("key_test", { provider, value });
    } catch (e) {
      const why = String(e);
      if (why.startsWith("This key was refused")) {
        button.disabled = false;
        return say(out, why, "bad");
      }
      note = "Could not check it now (" + why + "), saved anyway.";
    }
    try {
      const keys = await call("key_save", { provider, value });
      input.value = "";
      input.type = "password";
      $(provider + "-show").textContent = "Show";
      ready(keys);
      say(out, "Saved. " + note, note === "It works." ? "ok" : "");
      if (provider === "gemini") $("try").focus();
    } catch (e) {
      say(out, String(e), "bad");
    }
    button.disabled = false;
  }

  for (const p of ["gemini", "groq"]) {
    $(p + "-get").addEventListener("click", () => call("open_link", { url: LINKS[p] }).catch((e) => say(p + "-result", String(e), "bad")));
    $(p + "-show").addEventListener("click", () => {
      const input = $(p);
      const hidden = input.type === "password";
      input.type = hidden ? "text" : "password";
      $(p + "-show").textContent = hidden ? "Hide" : "Show";
    });
    $(p + "-save").addEventListener("click", (e) => saveKey(p, e.currentTarget));
    $(p).addEventListener("keydown", (e) => {
      if (e.key === "Enter") saveKey(p, $(p + "-save"));
    });
  }

  $("try").addEventListener("input", () => {
    if ($("try").value.trim()) {
      say("try-result", "It works. Use the same key in any app: chat, email, a terminal.", "ok");
    }
  });

  $("done").addEventListener("click", () => call("welcome_done").catch(() => window.close()));

  call("prefs_state")
    .then((s) => {
      $("key-cap").textContent = s.key.label;
      if (!ready(s.keys)) $("gemini").focus();
    })
    .catch(() => {});
})();
