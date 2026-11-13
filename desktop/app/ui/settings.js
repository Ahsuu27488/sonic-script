// The settings window (M2 phase 4). Talks to desktop/app/src/prefs.rs; every change saves at once and the
// running dictation picks it up. User text only ever goes into textContent, never into HTML.
"use strict";
(() => {
  const tauri = window.__TAURI__;
  const call = (cmd, args) =>
    tauri ? tauri.core.invoke(cmd, args) : Promise.reject(new Error("Open this page from SonicScript."));
  const $ = (id) => document.getElementById(id);
  const el = (tag, props = {}, ...children) => {
    const e = document.createElement(tag);
    Object.assign(e, props);
    for (const c of children) e.append(c);
    return e;
  };

  let state = null;
  let settings = null;

  // ---- toast ----
  let toastTimer = 0;
  function toast(text, bad = false) {
    const t = $("toast");
    t.textContent = text;
    t.classList.toggle("bad", bad);
    t.classList.add("show");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => t.classList.remove("show"), bad ? 4000 : 1600);
  }
  const fail = (e) => toast(String(e && e.message ? e.message : e), true);

  // ---- tabs (arrow keys move between them) ----
  const tabs = [...document.querySelectorAll('[role="tab"]')];
  function select(tab) {
    for (const t of tabs) {
      const on = t === tab;
      t.setAttribute("aria-selected", String(on));
      t.tabIndex = on ? 0 : -1;
      $(t.getAttribute("aria-controls")).hidden = !on;
    }
    if (tab.id === "tab-history") loadHistory();
    if (tab.id === "tab-words") loadWords();
  }
  tabs.forEach((t, i) => {
    t.addEventListener("click", () => select(t));
    t.addEventListener("keydown", (e) => {
      const step = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[e.key];
      if (!step) return;
      e.preventDefault();
      const next = tabs[(i + step + tabs.length) % tabs.length];
      next.focus();
      select(next);
    });
  });

  // ---- saving ----
  async function save(change) {
    const next = { ...settings, ...change };
    try {
      settings = await call("prefs_save", { settings: next });
      toast("Saved");
    } catch (e) {
      fail(e);
    }
    render();
  }

  // ---- general ----
  for (const box of document.querySelectorAll("[data-setting]")) {
    box.addEventListener("change", () => save({ [box.dataset.setting]: box.checked }));
  }
  $("autostart").addEventListener("change", async (e) => {
    try {
      e.target.checked = await call("autostart_set", { on: e.target.checked });
      toast("Saved");
    } catch (err) {
      e.target.checked = !e.target.checked;
      fail(err);
    }
  });

  // The dictation key: "Change", then the next key or side button pressed.
  let picking = false;
  async function stopPicking() {
    picking = false;
    document.body.classList.remove("picking-on");
    $("key-picking").hidden = true;
    $("key-change").textContent = "Change";
    await call("prefs_pause", { picking: false }).catch(() => {});
  }
  async function picked(code) {
    try {
      const k = await call("prefs_key_name", { code });
      await stopPicking();
      await save({ hotkey: k.name });
      state.key = k;
      renderKey();
    } catch (e) {
      toast(String(e), true);
    }
  }
  $("key-change").addEventListener("click", async () => {
    if (picking) return stopPicking();
    picking = true;
    document.body.classList.add("picking-on");
    $("key-picking").hidden = false;
    $("key-change").textContent = "Cancel";
    await call("prefs_pause", { picking: true }).catch(() => {});
  });
  window.addEventListener(
    "keydown",
    (e) => {
      if (!picking) return;
      e.preventDefault();
      e.stopPropagation();
      if (e.code === "Escape") stopPicking();
      else picked(e.code);
    },
    true,
  );
  window.addEventListener(
    "mouseup",
    (e) => {
      if (!picking || (e.button !== 3 && e.button !== 4)) return;
      e.preventDefault();
      picked("Mouse" + e.button);
    },
    true,
  );

  function renderKey() {
    $("key-cap").textContent = state.key.label;
    const w = $("key-warning");
    w.hidden = !state.key.warning;
    w.textContent = state.key.warning ? "Note: " + state.key.warning + "." : "";
  }

  // ---- models & keys ----
  const FROM = {
    environment: "From an environment variable on this PC",
    app: "Saved in SonicScript",
    user_environment: "From your Windows user environment",
  };
  function renderKeys() {
    const box = $("keys");
    box.replaceChildren();
    for (const k of state.keys) {
      const input = el("input", {
        type: "password",
        autocomplete: "off",
        spellcheck: false,
        placeholder: k.source ? "Paste a new key to replace it" : "Paste your " + k.label + " key",
      });
      input.setAttribute("aria-label", k.label + " key");
      const result = el("p", { className: "result" });
      result.setAttribute("role", "status");
      const show = el("button", { className: "btn ghost", textContent: "Show", type: "button" });
      show.addEventListener("click", () => {
        const hidden = input.type === "password";
        input.type = hidden ? "text" : "password";
        show.textContent = hidden ? "Hide" : "Show";
      });
      const saveBtn = el("button", { className: "btn primary", textContent: "Save", type: "button" });
      saveBtn.addEventListener("click", async () => {
        try {
          state.keys = await call("key_save", { provider: k.provider, value: input.value });
          input.value = "";
          renderKeys();
          toast("Key saved");
        } catch (e) {
          result.className = "result bad";
          result.textContent = String(e);
        }
      });
      const test = el("button", { className: "btn", textContent: "Test", type: "button" });
      test.addEventListener("click", async () => {
        test.disabled = true;
        result.className = "result";
        result.textContent = "Checking…";
        try {
          result.textContent = await call("key_test", { provider: k.provider, value: input.value || null });
          result.className = "result ok";
        } catch (e) {
          result.textContent = String(e);
          result.className = "result bad";
        }
        test.disabled = false;
      });
      const actions = [input, show, saveBtn, test];
      const get = el("button", { className: "btn link", textContent: "Get a key", type: "button" });
      get.addEventListener("click", () => call("open_link", { url: k.link }).catch(fail));
      const footer = [get];
      if (k.source && k.source.from === "app") {
        const remove = el("button", { className: "btn ghost danger", textContent: "Remove", type: "button" });
        remove.addEventListener("click", async () => {
          try {
            state.keys = await call("key_delete", { provider: k.provider });
            renderKeys();
            toast("Key removed");
          } catch (e) {
            fail(e);
          }
        });
        footer.push(remove);
      }
      const status = el("p", {
        className: "status " + (k.source ? "ok" : "missing"),
      });
      status.append(
        el("span", { className: "dot" }),
        k.source
          ? FROM[k.source.from] + (k.source.last4 ? ", ends in " + k.source.last4 : "")
          : "Not set" + (k.required ? ": dictation needs this key" : ""),
      );
      const badge = el("span", {
        className: "badge" + (k.required ? " need" : ""),
        textContent: k.required ? "Required" : k.provider === "groq" ? "Optional, makes it better" : "Optional, paid",
      });
      box.append(
        el(
          "div",
          { className: "card key-card" },
          el("div", { className: "head" }, el("h2", { textContent: k.label }), badge),
          status,
          el("div", { className: "key-entry" }, ...actions),
          result,
          el("div", { className: "key-foot" }, ...footer),
        ),
      );
    }
  }

  const DEFAULT = "";
  function modelLine(id) {
    const m = state.models.find((x) => x.id === id);
    if (!m) return "";
    const parts = [];
    if (m.accuracy != null) parts.push(Math.round(m.accuracy * 1000) / 10 + "% accurate on our tests");
    if (m.p50_ms != null) parts.push("typically " + (m.p50_ms / 1000).toFixed(1) + " s");
    parts.push(m.tier === "paid" ? "paid" : "free");
    return parts.join(" · ");
  }
  function fillSelect(select, value, first) {
    select.replaceChildren(el("option", { value: DEFAULT, textContent: first }));
    for (const m of state.models) {
      select.append(el("option", { value: m.id, textContent: m.label + (m.recommended ? " (recommended)" : "") }));
    }
    select.value = value || DEFAULT;
  }
  function renderModels() {
    const f = settings.fallbacks;
    fillSelect($("main-model"), settings.main_model, "Recommended order (Gemini Transcribe, then Flash, then Flash-Lite)");
    fillSelect($("fallback-1"), f[0], "None");
    fillSelect($("fallback-2"), f[1], "None");
    $("main-info").textContent = settings.main_model ? modelLine(settings.main_model) : "The order we measured best.";
    $("fallback-1-info").textContent = modelLine(f[0]);
    $("fallback-2-info").textContent = modelLine(f[1]);
    const custom = !!settings.main_model;
    $("fallback-1").disabled = !custom;
    $("fallback-2").disabled = !custom || !f[0];
  }
  function modelsChanged() {
    const main = $("main-model").value || null;
    const fallbacks = main ? [$("fallback-1").value, $("fallback-2").value].filter(Boolean) : [];
    save({ main_model: main, fallbacks });
  }
  for (const id of ["main-model", "fallback-1", "fallback-2"]) $(id).addEventListener("change", modelsChanged);

  // ---- words ----
  async function loadWords() {
    try {
      $("words-text").value = await call("words_text");
    } catch (e) {
      fail(e);
    }
  }
  $("words-save").addEventListener("click", async () => {
    try {
      const n = await call("words_save", { text: $("words-text").value });
      toast("Saved: " + n + (n === 1 ? " word" : " words"));
    } catch (e) {
      fail(e);
    }
  });
  $("words-prompt").addEventListener("click", () =>
    call("words_prompt_copy").then(() => toast("Prompt copied: paste it into your AI assistant"), fail),
  );
  let pasted = "";
  $("words-paste").addEventListener("click", async () => {
    try {
      const [text, p] = await call("words_preview_clipboard");
      pasted = text;
      const lines = [...p.new.map((l) => "+ " + l), ...p.more_heard_as.map((l) => "~ " + l)];
      $("words-preview-text").textContent =
        p.found === 0
          ? "No words found on the clipboard. Copy your assistant's answer first."
          : p.found + " found: " + p.new.length + " new, " + p.more_heard_as.length +
            " with new mishearings, " + p.already + " already in your list.";
      $("words-preview-list").replaceChildren(...lines.map((l) => el("li", { textContent: l })));
      $("words-add").hidden = lines.length === 0;
      $("words-preview").hidden = false;
    } catch (e) {
      fail(e);
    }
  });
  $("words-add").addEventListener("click", async () => {
    try {
      $("words-text").value = await call("words_add", { text: pasted });
      $("words-preview").hidden = true;
      toast("Added");
    } catch (e) {
      fail(e);
    }
  });
  $("words-cancel").addEventListener("click", () => ($("words-preview").hidden = true));

  // ---- history ----
  $("history-days").addEventListener("change", (e) => {
    const n = Math.max(0, Math.min(365, Math.round(Number(e.target.value) || 0)));
    save({ history_days: n });
  });
  function when(iso) {
    const d = new Date(iso);
    return isNaN(d) ? iso : d.toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
  }
  async function loadHistory() {
    let items = [];
    try {
      items = await call("history_list");
    } catch (e) {
      fail(e);
    }
    $("history-empty").hidden = items.length > 0;
    $("history-list").replaceChildren(...items.map(historyItem));
  }
  function historyItem(it) {
    const li = el("li");
    const btn = (label, fn, cls = "btn") => {
      const b = el("button", { className: cls, textContent: label, type: "button" });
      b.addEventListener("click", fn);
      return b;
    };
    const actions = el(
      "div",
      { className: "actions" },
      btn("Copy", () => call("history_copy", { text: it.text }).then(() => toast("Copied"), fail)),
      btn("Insert", () => call("history_reinsert", { text: it.text }).then(toast, fail)),
    );
    if (it.audio) actions.append(btn("Play", () => call("history_play", { audio: it.audio }).catch(fail)));
    actions.append(
      btn(
        "Delete",
        () => {
          const sure = el("span", { className: "confirm" });
          sure.append(
            btn("Delete for good", async () => {
              try {
                const gone = await call("history_delete", { at: it.at });
                li.remove();
                toast(gone ? "Deleted" : "Already deleted");
              } catch (e) {
                fail(e);
              }
            }, "btn ghost danger"),
            btn("Keep", () => sure.replaceWith(del), "btn ghost"),
          );
          del.replaceWith(sure);
        },
        "btn ghost danger",
      ),
    );
    const del = actions.lastChild;
    li.append(
      el("div", { className: "meta", textContent: when(it.time) + (it.app ? " · " + it.app : "") }),
      el("p", { className: "said", textContent: it.text }),
      actions,
    );
    return li;
  }

  // ---- about ----
  for (const b of document.querySelectorAll("[data-link]")) {
    b.addEventListener("click", () => call("open_link", { url: b.dataset.link }).catch(fail));
  }

  // The apps where the pill's x turned the pop-up mic off.
  function renderOffApps() {
    const apps = settings.popup_off_apps || [];
    $("off-apps").hidden = apps.length === 0;
    $("off-apps-list").replaceChildren(
      ...apps.map((exe) => {
        const on = el("button", { className: "btn ghost", textContent: "Turn on", type: "button" });
        on.setAttribute("aria-label", "Turn the mic on in " + exe);
        on.addEventListener("click", async () => {
          try {
            settings.popup_off_apps = await call("popup_app_on", { exe });
            renderOffApps();
            toast("Saved");
          } catch (e) {
            fail(e);
          }
        });
        return el("li", {}, el("span", { className: "mono", textContent: exe }), on);
      }),
    );
  }

  // ---- updates (D-053) ----
  let updatePoll = 0;
  async function renderUpdate() {
    let u;
    try {
      u = await call("update_state");
    } catch (e) {
      return;
    }
    // The installer's version (what the updater compares).
    $("version").textContent = u.version;
    $("update-line").textContent = u.line;
    $("update-install").hidden = !u.can_install;
    const busy = ["checking", "downloading", "waiting"].includes(u.phase.phase);
    $("update-check").disabled = busy;
    // While a check or a download runs, look again each second.
    clearTimeout(updatePoll);
    if (busy) updatePoll = setTimeout(renderUpdate, 1000);
  }
  $("update-check").addEventListener("click", async () => {
    try {
      await call("update_check");
    } catch (e) {
      fail(e);
    }
    setTimeout(renderUpdate, 300);
  });
  $("update-install").addEventListener("click", async () => {
    try {
      // Installs and restarts (the app exits); during a dictation it waits for it to end.
      await call("update_install");
    } catch (e) {
      fail(e);
    }
    renderUpdate();
  });

  // ---- render ----
  function render() {
    renderOffApps();
    for (const box of document.querySelectorAll("[data-setting]")) box.checked = !!settings[box.dataset.setting];
    $("history-days").value = settings.history_days;
    renderModels();
  }

  async function start() {
    try {
      state = await call("prefs_state");
    } catch (e) {
      fail(e);
      return;
    }
    settings = state.settings;
    $("autostart").checked = state.autostart;
    $("version").textContent = state.version;
    const n = $("notices");
    n.hidden = state.notices.length === 0;
    n.replaceChildren(...state.notices.map((t) => el("p", { textContent: t })));
    renderKey();
    renderKeys();
    render();
    renderUpdate();
  }
  start();
})();
