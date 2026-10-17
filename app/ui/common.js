// Shared by the Calibrate, Actions and Cheat Sheet windows.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

/** Physical 3-3-4 layout, numbered left-top to right-bottom (same as deck.py). */
const LAYOUT = [[1, 2, 3], [4, 5, 6], [7, 8, 9, 10]];
const GESTURES = [["press", "Press"], ["long", "Long Press"], ["double", "Double Press"]];

/** Key in deck.json's buttons: "3", "3.long", "3.double". */
const buttonKey = (n, g) => (g === "press" ? `${n}` : `${n}.${g}`);

/** Front app's profile first, then the default buttons. */
function actionFor(config, n, gesture, appId) {
  const k = buttonKey(n, gesture);
  return config.profiles[appId]?.buttons[k] ?? config.buttons[k] ?? { type: "none" };
}

const el = (tag, attrs = {}, ...children) => {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
    else if (v === true) e.setAttribute(k, "");
    else if (v !== false && v != null) e.setAttribute(k, v);
  }
  e.append(...children.flat().filter((c) => c != null && c !== false));
  return e;
};

/** The deck drawn as buttons; `label` and `cls` decide what each key shows. */
function deckGrid({ label, cls = () => "", onTap }) {
  return el("div", { class: "grid" }, LAYOUT.map((row) =>
    el("div", { class: "row" }, row.map((n) =>
      el("button", { class: `key ${cls(n)}`, disabled: !onTap, onclick: () => onTap?.(n) },
        el("b", {}, `${n}`), el("span", {}, label(n)))))));
}

// Key combos are stored like "shift+cmd+t" (ctrl, opt, shift, cmd = Ctrl, Alt, Shift, Win on Windows).
const MODS = [["ctrl", "⌃", "Ctrl"], ["opt", "⌥", "Alt"], ["shift", "⇧", "Shift"], ["cmd", "⌘", "Win"]];
const SYMBOLS = { space: "Space", return: "↩", tab: "⇥", delete: "⌫", forwarddelete: "⌦", escape: "⎋",
  left: "←", right: "→", up: "↑", down: "↓", home: "↖", end: "↘", pageup: "⇞", pagedown: "⇟" };
const WIN_NAMES = { return: "Enter", delete: "Backspace", forwarddelete: "Delete", escape: "Esc", pageup: "PgUp", pagedown: "PgDn" };
const IS_MAC = navigator.userAgent.includes("Mac");

/** "shift+cmd+t" -> "⇧⌘T" on Mac, "Shift+Win+T" on Windows. */
function comboSymbols(text) {
  const parts = text.toLowerCase().split("+");
  const key = parts.pop();
  const mods = MODS.filter((m) => parts.includes(m[0]));
  if (IS_MAC) return mods.map((m) => m[1]).join("") + (SYMBOLS[key] ?? key.toUpperCase());
  const name = WIN_NAMES[key] ?? (key.length > 1 ? key[0].toUpperCase() + key.slice(1) : key.toUpperCase());
  return [...mods.map((m) => m[2]), name].join("+");
}

const CODE_NAMES = { Space: "space", Enter: "return", Tab: "tab", Backspace: "delete", Delete: "forwarddelete",
  Escape: "escape", Home: "home", End: "end", PageUp: "pageup", PageDown: "pagedown", ArrowLeft: "left",
  ArrowRight: "right", ArrowUp: "up", ArrowDown: "down", Minus: "-", Equal: "=", BracketLeft: "[",
  BracketRight: "]", Backslash: "\\", Semicolon: ";", Quote: "'", Comma: ",", Period: ".", Slash: "/",
  Backquote: "`", IntlBackslash: "§" };

/** Physical key of a keydown -> stored combo, or null for modifiers alone and unknown keys. */
function comboFromEvent(e) {
  const c = e.code;
  const name = /^Key[A-Z]$/.test(c) ? c[3].toLowerCase()
    : /^Digit\d$/.test(c) ? c[5]
    : /^F([1-9]|1[0-2])$/.test(c) ? c.toLowerCase()
    : CODE_NAMES[c];
  if (!name) return null;
  const mods = [e.ctrlKey && "ctrl", e.altKey && "opt", e.shiftKey && "shift", e.metaKey && "cmd"].filter(Boolean);
  return [...mods, name].join("+");
}

/** Names for system actions and input languages, loaded once. */
const names = { system: {}, input: {} };
const namesReady = Promise.all([
  invoke("system_actions").then((list) => (names.system = Object.fromEntries(list))),
  invoke("input_sources").then((list) => (names.input = Object.fromEntries(list))),
]);

/** Short form for a key on the deck grid and cheat sheet. */
function shortLabel(a) {
  const v = a.value ?? "";
  if (a.type === "none" || !v) return "–";
  switch (a.type) {
    case "key": return comboSymbols(v);
    case "repeat": return "⟳ " + comboSymbols(v);
    case "type": return `“${v}”`;
    case "password": return "••••••";
    case "system": return names.system[v] ?? v;
    case "input": return names.input[v] ?? v;
    case "url": try { return new URL(v.includes("://") ? v : "https://" + v).host; } catch { return v; }
    default: return v;
  }
}

/** Calls render now and whenever the deck's state changes. */
async function live(render) {
  await namesReady;
  const refresh = async () => render(await invoke("get_state"));
  await listen("state", refresh);
  return refresh();
}
