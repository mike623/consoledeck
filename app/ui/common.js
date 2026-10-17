// Shared by the Calibrate, Actions and Cheat Sheet windows.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

/** Physical 3-3-4 layout, numbered left-top to right-bottom (same as deck.py). */
const LAYOUT = [[1, 2, 3], [4, 5, 6], [7, 8, 9, 10]];
const GESTURES = [["press", "Press"], ["long", "Long Press"], ["double", "Double Press"]];

/** Key in deck.json's buttons: "3", "3.long", "3.double", and "p2.3", "p2.3.long" on page 2. */
const buttonKey = (n, g, page = 1) => (page > 1 ? `p${page}.` : "") + (g === "press" ? `${n}` : `${n}.${g}`);
const pageOf = (key) => +(/^p(\d+)\./.exec(key)?.[1] ?? 1);
/** Highest page with a button set, in the default buttons or the app's profile (same as the deck). */
const pageCount = (config, appId) =>
  Math.max(1, ...[...Object.keys(config.buttons), ...Object.keys(config.profiles[appId]?.buttons ?? {})].map(pageOf));

/** Front app's profile first, then the default buttons. */
function actionFor(config, n, gesture, appId, page = 1) {
  const k = buttonKey(n, gesture, page);
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

/** Installed plugins by id (a button's "type" is its plugin id), and input language names. */
let plugins = {};
const names = { input: {} };
const loadPlugins = async () => { plugins = Object.fromEntries((await invoke("list_plugins")).map((p) => [p.id, p])); };
const namesReady = Promise.all([
  loadPlugins(),
  invoke("input_sources").then((list) => (names.input = Object.fromEntries(list))),
]);

/** Built into ConsoleDeck: the deck itself handles them, so they're there even without the System plugin. */
const CORE_SYSTEM = [{ value: "editor", label: "Open ConsoleDeck Editor" },
  { value: "nextpage", label: "Next Page", note: "Switches every button to the next page; after the last page comes page 1. Put a page button on each page to get back." },
  { value: "prevpage", label: "Previous Page" },
  { value: "cheatsheet", label: "Show Cheat Sheet", note: "Shows every button's actions over the app in front. Set it as a Long Press: it stays up while you hold the button." }];

/** The plugin behind an action type, or undefined when it isn't installed. */
function pluginFor(type) {
  if (type !== "system") return plugins[type];
  const p = plugins.system ?? { id: "system", name: "System", fields: [{ id: "action", label: "Action", type: "select", options: [] }] };
  const [first, ...rest] = p.fields;
  return { ...p, fields: [{ ...first, options: [...(first.options ?? []), ...CORE_SYSTEM] }, ...rest] };
}

const OS = IS_MAC ? "mac" : "windows";
const forThisOS = (x) => !x.platforms?.length || x.platforms.includes(OS);
/** This OS's variant of a run. */
const osRun = (r) => (r?.[OS] ? osRun(r[OS]) : r);
/** What pressing the button runs: the chosen option's own run, else the plugin's. */
const runOf = (p, a) => osRun(p.fields[0]?.options?.find((o) => o.value === a.value)?.run ?? p.run);

/** Field values by id: the first field is the action's value, the rest its values map. */
function actionValues(p, a) {
  const v = { ...(a.values ?? {}) };
  if (p.fields[0]) v[p.fields[0].id] = a.value ?? "";
  return v;
}

/** A field's value as the deck shows it. */
function showValue(field, v) {
  switch (field?.type) {
    case "keys": return comboSymbols(v);
    case "password": return "••••••";
    case "url": try { return new URL(v.includes("://") ? v : "https://" + v).host; } catch { return v; }
    case "select": return field.options?.find((o) => o.value === v)?.label
      ?? (field.optionsFrom?.[0] === "inputSources" ? names.input[v] : null) ?? v;
    default: return v;
  }
}

/** Short form for a key on the deck grid and cheat sheet. */
function shortLabel(a) {
  const v = a.value ?? "";
  const p = pluginFor(a.type);
  if (p && !p.fields.length) return p.label ?? p.name; // nothing to fill in, e.g. CPU Usage
  if (a.type === "none" || !v) return "–";
  if (a.type === "repeat") return "⟳ " + comboSymbols(v);
  if (!p) return `⚠ ${a.type}`; // its plugin was removed
  const values = actionValues(p, a);
  const shown = (id) => showValue(p.fields.find((f) => f.id === id), values[id] ?? "");
  return p.label ? p.label.replace(/\{([\w-]+)\}/g, (_, id) => shown(id)) : shown(p.fields[0]?.id);
}

/** Calls render now, whenever the deck's state changes, and after plugins are installed or removed. */
async function live(render) {
  await namesReady;
  const refresh = async () => render(await invoke("get_state"));
  await listen("state", refresh);
  await listen("plugins", async () => { await loadPlugins(); refresh(); });
  return refresh();
}
