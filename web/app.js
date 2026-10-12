// The page draws what the core returns and decides nothing. It builds the
// DOM from the core's view model (ui()), forwards keyboard, mouse and
// transport events, and keeps the two settings strings in browser storage.
import init, { Game, render_assets } from "./pkg/tool_shim.js";
import { createRenderer } from "./render.js";

const el = (id) => document.getElementById(id);
const make = (tag, cls, text) => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
};

let game;
let hints = {};
let lastText = "";
let ui = null;
let hovered = null;
let renderer = null;
let assets = null;
let terrainStamp = null;
let icons = {};
let iconsFor = null;
let fogStamp = null;

function readStore(key) {
  try {
    return localStorage.getItem(key) || "";
  } catch (e) {
    return "";
  }
}

function writeStore(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch (e) {
    // Storage can be blocked; settings then last for this visit only.
  }
}

// ---- menus ----

function pairs(rows) {
  const table = make("table", "pairs");
  rows.forEach((r) => {
    const tr = make("tr");
    const label = make("td", "", r[0]);
    if (hints[r[0]]) label.dataset.term = r[0];
    tr.append(label, make("td", "", r[1]));
    table.append(tr);
  });
  return table;
}

function field(item) {
  const wrap = make("div", "field");
  const label = make("label", "", item.label);
  if (item.term && hints[item.term]) label.dataset.term = item.term;
  if (hints[item.label]) label.dataset.term = item.label;
  let input;
  if (item.type === "select") {
    input = make("select");
    item.options.forEach((o) => {
      const opt = make("option", "", o.text);
      opt.value = o.value;
      input.append(opt);
    });
    input.value = item.value;
    input.addEventListener("change", () => send(item.id, input.value));
  } else if (item.type === "code") {
    input = make("textarea");
    input.value = item.value;
    input.addEventListener("input", () => send(item.id, input.value));
  } else {
    input = make("input");
    input.value = item.value;
    if (item.placeholder) input.placeholder = item.placeholder;
    input.addEventListener("input", () => send(item.id, input.value));
  }
  input.id = item.id;
  label.htmlFor = item.id;
  wrap.append(label, input);
  return wrap;
}

// A select with no label of its own, for a table cell.
function bareSelect(item) {
  const input = make("select");
  item.options.forEach((o) => {
    const opt = make("option", "", o.text);
    opt.value = o.value;
    input.append(opt);
  });
  input.value = item.value;
  input.id = item.id;
  input.addEventListener("change", () => send(item.id, input.value));
  return input;
}

// A table with a head row; each cell is text or an item (a select, a button).
function table(item) {
  const box = make("table", "cells");
  const head = make("tr");
  item.head.forEach((h) => {
    const th = make("th", "", h);
    if (hints[h]) th.dataset.term = h;
    head.append(th);
  });
  box.append(head);
  item.rows.forEach((r) => {
    const tr = make("tr");
    r.forEach((cell) => {
      const td = make("td");
      if (typeof cell === "string") td.textContent = cell;
      else td.append(cell.bare ? bareSelect(cell) : element(cell));
      tr.append(td);
    });
    box.append(tr);
  });
  return box;
}

function element(item) {
  if (item.type === "button") {
    const b = make("button", "button" + (item.active ? " active" : ""), item.text);
    b.type = "button";
    b.id = item.id;
    b.disabled = item.disabled === true;
    if (item.term && hints[item.term]) b.dataset.term = item.term;
    b.addEventListener("click", () => {
      // Copy: the core gives the text; putting it on the clipboard is the page's job.
      if (item.copy !== undefined && navigator.clipboard) navigator.clipboard.writeText(item.copy).catch(() => {});
      send(item.id, "");
    });
    return b;
  }
  if (item.type === "table") return table(item);
  if (item.type === "list") return pairs(item.rows);
  if (item.type === "toggles") {
    const wrap = make("div", "field");
    const box = make("div", "toggles");
    item.items.forEach((t) => {
      const label = make("label");
      const check = make("input");
      check.type = "checkbox";
      check.checked = t.value;
      check.id = t.id;
      check.addEventListener("change", () => send(t.id, check.checked ? "on" : "off"));
      label.append(check, make("span", "", t.label));
      box.append(label);
    });
    wrap.classList.add("toggles-field");
    wrap.append(make("label", "", item.label), box);
    return wrap;
  }
  if (item.type === "slider") {
    // The core gives the range and step; the page only shows them. The
    // value is sent when the slider is released, so a redraw never ends
    // a drag.
    const wrap = make("div", "field slider");
    const label = make("label", "", item.label);
    if (hints[item.label]) label.dataset.term = item.label;
    const input = make("input");
    input.type = "range";
    input.id = item.id;
    input.setAttribute("min", item.min);
    input.setAttribute("max", item.max);
    input.setAttribute("step", item.step);
    input.value = item.value;
    const shown = make("span", "slider-value", item.text);
    input.addEventListener("change", () => send(item.id, input.value));
    label.htmlFor = item.id;
    wrap.append(label, input, shown);
    return wrap;
  }
  if (item.type === "keybind") {
    const wrap = make("div", "keybind");
    const b = make("button", "", item.key);
    b.type = "button";
    b.id = item.id;
    b.addEventListener("click", () => send(item.id, ""));
    wrap.append(make("span", "", item.label), b);
    return wrap;
  }
  return field(item);
}

function leaf(item) {
  const node = el("leaf-template").content.firstElementChild.cloneNode(true);
  node.querySelector(".leaf-text").textContent = item.text;
  node.id = item.id;
  node.addEventListener("click", () => node.classList.add("dragged"));
  node.addEventListener("animationend", (e) => {
    if (e.animationName === "leaf-away") send(item.id, "");
  });
  return node;
}

function drawMenus() {
  el("title").textContent = ui.title;
  const focused = document.activeElement && document.activeElement.id;
  const caret = focused && document.activeElement.selectionStart;
  const panels = el("panels");
  panels.replaceChildren();
  ui.panels.forEach((p) => {
    const box = make("div", "panel " + p.kind);
    if (p.single) box.classList.add("single");
    if (p.heading) box.append(make("h2", "", p.heading));
    p.items.forEach((item) => box.append(p.kind === "leaves" ? leaf(item) : element(item)));
    panels.append(box);
  });
  if (focused && el(focused)) {
    el(focused).focus();
    if (caret !== null && caret !== undefined && el(focused).setSelectionRange) {
      el(focused).setSelectionRange(caret, caret);
    }
  }
}

// ---- the match ----
// The view model changes every tick, so the HUD is updated in place: each
// element is kept while its key stays, and only its text, classes and
// picture change. A button under the pointer is never replaced, so clicks
// and hover (tooltips) survive the updates.

function setText(node, text) {
  if (node.textContent !== text) node.textContent = text;
}

function setClass(node, cls) {
  if (node.className !== cls) node.className = cls;
}

function setTerm(node, term) {
  if (term && hints[term]) node.dataset.term = term;
  else delete node.dataset.term;
}

// One child of `box` per item, in order. `create` makes a new child;
// `update` brings it up to date. Listeners read `node.item`, the latest item.
function sync(box, items, keyOf, create, update) {
  const old = new Map();
  Array.from(box.children).forEach((c) => old.set(c.dataset.key, c));
  items.forEach((item, i) => {
    const key = keyOf(item, i);
    let node = old.get(key);
    if (node) {
      old.delete(key);
    } else {
      node = create();
      node.dataset.key = key;
    }
    node.item = item;
    update(node, item);
    if (box.children[i] !== node) box.insertBefore(node, box.children[i] || null);
  });
  old.forEach((node) => node.remove());
}

const byIndex = (item, i) => String(i);

function setIcon(img, key) {
  const src = icons[key] || "";
  if (img.getAttribute("src") !== src) {
    if (src) img.src = src;
    else img.removeAttribute("src");
  }
  img.hidden = !src;
}

function newRes() {
  const box = make("div", "res");
  box.append(make("span", "name"), make("span", "value"), make("span", "income"));
  return box;
}

function drawBar(m) {
  sync(el("resources"), m.hud.resources.concat(m.hud.stats), (r) => r.key, newRes, (box, r) => {
    setClass(box, "res " + r.key);
    const [name, value, income] = box.children;
    setText(name, r.label);
    setText(value, r.value);
    setText(income, r.income || "");
    if (r.income) {
      setTerm(income, r.term);
      setTerm(name, "");
    } else {
      setTerm(name, r.term);
      setTerm(income, "");
    }
  });
  sync(el("queue"), m.queue, (q) => q.key, newQueueItem, (item, q) => {
    const [img, rank, short, count, fill] = item.children;
    setClass(item, "qitem " + q.kind);
    setIcon(img, q.icon);
    setText(rank, q.rank);
    setText(short, q.short);
    setText(count, q.count);
    fill.style.width = q.progress;
    if (tipAnchor === item) showTip(q.label, q.rows, item);
  });
  sync(el("flags"), m.hud.flags, byIndex, () => make("span", "flag"), (flag, f) => {
    setText(flag, f.text);
    setTerm(flag, f.term);
  });
  sync(el("controls"), m.controls, (c) => c.id, newControl, (b, c) => {
    setText(b, c.text);
    setClass(b, "control" + (c.active ? " active" : ""));
  });
  setText(el("clock"), m.hud.clock);
  sync(el("notices"), m.notices, byIndex, () => make("div", "notice"), (n, v) => setText(n, v.text));
}

function newQueueItem() {
  const item = make("button", "qitem");
  item.type = "button";
  item.append(iconImage(""), make("span", "rank"), make("span", "short"), make("span", "count"), make("span", "fill"));
  item.addEventListener("click", () => send(item.item.id, ""));
  item.addEventListener("mouseenter", () => showTip(item.item.label, item.item.rows, item));
  item.addEventListener("mouseleave", hideTip);
  return item;
}

function newControl() {
  const b = make("button", "control");
  b.type = "button";
  b.addEventListener("click", () => send(b.item.id, ""));
  return b;
}

function newTab() {
  const b = make("button", "tab");
  b.type = "button";
  b.append(make("span", "name"), make("span", "k"));
  b.addEventListener("click", () => send(b.item.id, ""));
  return b;
}

function newGridButton() {
  const b = make("button", "gbtn");
  b.type = "button";
  b.append(iconImage(""), make("span", "icon-text"), make("span", "label"), make("span", "k"), make("span", "tier"));
  b.addEventListener("click", (e) => {
    if (!b.item.empty) sendMods(b.item.id, e);
  });
  b.addEventListener("mouseenter", () => {
    if (!b.item.empty) showTip(b.item.label, tipRows(b.item), b);
  });
  b.addEventListener("mouseleave", hideTip);
  return b;
}

function tipRows(g) {
  return g.rows.concat([["", g.state_text]]);
}

function drawGrid(m) {
  el("grid").hidden = m.spectate;
  sync(el("tabs"), m.tabs, (t) => t.id, newTab, (b, t) => {
    setClass(b, "tab" + (t.active ? " active" : ""));
    setText(b.children[0], t.label);
    setText(b.children[1], t.key);
  });
  sync(el("buttons"), m.grid, (g) => g.id, newGridButton, (b, g) => {
    const [img, iconText, label, key, tier] = b.children;
    if (g.empty) {
      setClass(b, "gbtn empty");
      if (tipAnchor === b) hideTip();
      return;
    }
    setClass(b, "gbtn " + g.state + (g.active ? " active" : ""));
    setIcon(img, g.icon);
    setText(iconText, g.icon_text);
    setText(label, g.label);
    setText(key, g.key);
    setText(tier, g.tier);
    if (tipAnchor === b) showTip(g.label, tipRows(g), b);
  });
}

let overlayText = "";

function drawOverlay(m) {
  const overlay = el("overlay");
  const box = m.end || m.menu;
  overlay.hidden = !box;
  // Rebuilt only when it changes, so its buttons stay under the pointer.
  const text = box ? JSON.stringify(box) : "";
  if (!box || text === overlayText) return;
  overlayText = text;
  overlay.replaceChildren();
  const panel = make("div", "panel");
  if (box.title) panel.append(make("h2", "", box.title));
  if (box.heading) panel.append(make("h2", "", box.heading));
  if (box.rows) panel.append(pairs(box.rows));
  if (box.graph) {
    panel.classList.add("end");
    panel.append(endBody(box));
  }
  const buttons = box.items || [box.button];
  buttons.forEach((b) => panel.append(element(b)));
  overlay.append(panel);
}

// ---- the end screen: graph tabs, the core's graph, every colony's summary ----

// SVG nodes are made in the namespace of index.html's graph template, so
// the page names no URL.
function svgNode(tag, attrs) {
  const ns = el("graph-template").content.firstElementChild.namespaceURI;
  const node = document.createElementNS(ns, tag);
  Object.keys(attrs).forEach((k) => node.setAttribute(k, attrs[k]));
  return node;
}

function endBody(box) {
  const body = make("div", "end-body");
  const left = make("div", "end-graph");
  const tabs = make("div", "end-tabs");
  box.tabs.forEach((t) => {
    const b = make("button", "tab" + (t.active ? " active" : ""), t.text);
    b.type = "button";
    b.id = t.id;
    if (hints[t.text]) b.dataset.term = t.text;
    b.addEventListener("click", () => send(t.id, ""));
    tabs.append(b);
  });
  const g = box.graph;
  const svg = svgNode("svg", { viewBox: g.view_box, width: g.width, height: g.height, class: "graph" });
  svg.append(svgNode("path", { d: g.grid, class: "graph-grid" }));
  svg.append(svgNode("path", { d: g.axis, class: "graph-axis" }));
  g.lines.forEach((l) => svg.append(svgNode("path", { d: l.d, stroke: l.colour, class: "graph-line" })));
  g.labels.forEach((l) => {
    const t = svgNode("text", { x: l.x, y: l.y, "text-anchor": l.anchor, class: "graph-label" });
    t.textContent = l.text;
    svg.append(t);
  });
  left.append(tabs, make("h3", "", g.title), svg);
  body.append(left, summaryTable(box));
  return body;
}

function summaryTable(box) {
  const table = make("table", "pairs summary");
  const head = make("tr");
  head.append(make("td"));
  box.columns.forEach((c) => {
    const th = make("th");
    const swatch = make("span", "swatch");
    swatch.style.background = c.colour;
    th.append(swatch, make("span", "", c.text));
    head.append(th);
  });
  table.append(head);
  box.summary.forEach((r) => {
    const tr = make("tr");
    const label = make("td", "", r.label);
    if (hints[r.label]) label.dataset.term = r.label;
    tr.append(label);
    r.values.forEach((v) => tr.append(make("td", "", v)));
    table.append(tr);
  });
  return table;
}

// ---- tooltips and term hints ----

// Above the anchor when there is room, else below it; always inside the
// window at the sides.
function place(box, anchor) {
  const r = anchor.getBoundingClientRect();
  const w = box.offsetWidth;
  const h = box.offsetHeight;
  let top = r.top - h;
  if (top < 0) top = r.bottom;
  if (top + h > window.innerHeight) top = window.innerHeight - h;
  if (top < 0) top = 0;
  let left = r.left;
  if (left + w > window.innerWidth) left = window.innerWidth - w;
  if (left < 0) left = 0;
  box.style.left = left + "px";
  box.style.top = top + "px";
}

let tipAnchor = null;

function showTip(title, rows, anchor) {
  const tip = el("tooltip");
  tip.replaceChildren(make("h3", "", title), pairs(rows));
  tip.hidden = false;
  tipAnchor = anchor;
  place(tip, anchor);
}

function hideTip() {
  tipAnchor = null;
  el("tooltip").hidden = true;
}

document.addEventListener("mouseover", (e) => {
  const target = e.target.closest && e.target.closest("[data-term]");
  const hint = el("hint");
  if (target && hints[target.dataset.term]) {
    hint.textContent = target.dataset.term + ": " + hints[target.dataset.term];
    hint.hidden = false;
    place(hint, target);
  } else {
    hint.hidden = true;
  }
});

// ---- input ----

function send(id, value) {
  game.act(id, value, false, false);
}

// Alt also gives Train 10: browsers never let a page catch Ctrl+W.
function sendMods(id, e) {
  game.act(id, "", e.ctrlKey || e.altKey || e.metaKey, e.shiftKey);
}

function typing(e) {
  const t = e.target && e.target.tagName;
  return t === "INPUT" || t === "TEXTAREA" || t === "SELECT";
}

function bindInput() {
  ["keydown", "keyup"].forEach((type) => {
    window.addEventListener(type, (e) => {
      if (typing(e)) return;
      // In a match, and while Keybinds waits for a key, keys such as Tab
      // (All Ants), Space and Enter never move focus or press a button.
      if (ui && (ui.screen === "match" || ui.keys_captured) && !e.ctrlKey) e.preventDefault();
      // Space presses a focused button when it comes back up: the key just
      // bound must not press its Keybinds button again.
      if (ui && ui.keys_captured && document.activeElement) document.activeElement.blur();
      if (e.altKey) e.preventDefault();
      if (e.repeat) return;
      game.key(e.code, type === "keydown", e.ctrlKey || e.altKey || e.metaKey, e.shiftKey);
    });
  });
  const map = el("map");
  map.addEventListener("mousedown", (e) => {
    game.mouse("mousedown", e.button, e.offsetX, e.offsetY, e.shiftKey);
  });
  // Moves and releases are read over the whole window, in map pixels, so
  // Edge Pan works where the top bar, grid or minimap cover the map's edge.
  ["mouseup", "mousemove"].forEach((type) => {
    window.addEventListener(type, (e) => {
      if (!ui || ui.screen !== "match") return;
      const r = map.getBoundingClientRect();
      game.mouse(type, e.button, e.clientX - r.left, e.clientY - r.top, e.shiftKey);
    });
  });
  map.addEventListener("contextmenu", (e) => e.preventDefault());
  map.addEventListener("wheel", (e) => {
    e.preventDefault();
    game.wheel(e.deltaY);
  }, { passive: false });
  const mini = el("minimap");
  mini.addEventListener("mousedown", (e) => jumpTo(e, mini));
  mini.addEventListener("mousemove", (e) => {
    if (e.buttons) jumpTo(e, mini);
  });
  window.addEventListener("resize", resize);
  resize();
}

function jumpTo(e, mini) {
  game.jump(e.offsetX, e.offsetY, mini.clientWidth, mini.clientHeight);
}

function resize() {
  const map = el("map");
  map.width = map.clientWidth;
  map.height = map.clientHeight;
  game.resize(map.width, map.height);
  const mini = el("minimap");
  mini.width = mini.clientWidth;
  mini.height = mini.clientWidth;
  fogStamp = null;
}

// ---- drawing: the core lays out every frame; the page replays it ----

// Icons: the core's draw lists painted once per match into small images.
function buildIcons() {
  const data = JSON.parse(game.icons());
  const canvas = document.createElement("canvas");
  const painter = createRenderer(canvas, assets);
  icons = {};
  Object.keys(data.lists).forEach((key) => {
    canvas.width = data.size;
    canvas.height = data.size;
    painter.draw(Int32Array.from(data.lists[key]));
    icons[key] = canvas.toDataURL();
  });
}

function iconImage(key) {
  const img = make("img", "icon");
  img.alt = "";
  if (icons[key]) img.src = icons[key];
  return img;
}

function drawMatch() {
  if (!renderer) renderer = createRenderer(el("map"), assets);
  const ts = game.terrain_stamp();
  if (ts !== terrainStamp) {
    buildIcons();
    lastText = "";
    const size = game.terrain_size();
    renderer.setImage(assets.images.terrain, size[0], size[1], game.terrain());
    terrainStamp = ts;
    resize();
  }
  const fs = game.fog_stamp();
  if (fs !== fogStamp) {
    const size = game.fog_size();
    renderer.setImage(assets.images.fog, size[0], size[1], game.fog());
    const mini = el("minimap");
    const bytes = new Uint8ClampedArray(game.minimap(mini.width));
    if (bytes.length) mini.getContext("2d").putImageData(new ImageData(bytes, mini.width, mini.width), 0, 0);
    fogStamp = fs;
  }
  renderer.draw(game.frame());
}

// ---- transport ----
// online.js is imported only when a player hosts or joins (PLAN decision 18).

let online = null;

function onNet(e) {
  game.net_event(e.kind, e.peer || "", e.text || "", !!e.host);
}

async function netRequest(req) {
  if (!online) online = await import("./online.js");
  try {
    if (req.op === "host_room") await online.hostRoom(req.room, onNet);
    else if (req.op === "join_room") await online.joinRoom(req.room, onNet);
    else if (req.op === "host_code") game.net_code("invite", await online.hostByCode(onNet));
    else if (req.op === "invite_code") game.net_code("invite", await online.inviteByCode());
    else if (req.op === "join_code") game.net_code("reply", await online.joinByCode(req.invite, onNet));
    else if (req.op === "accept_reply") await online.acceptAnswer(req.reply);
    else if (req.op === "leave") online.leave();
  } catch (err) {
    game.net_event("error", "", (err && err.message) || "online.signal_failed", false);
  }
}

function flushNet() {
  if (!online) return;
  JSON.parse(game.outbox()).forEach((m) => {
    if (m.to === null) online.broadcast(m.text);
    else online.send(m.to, m.text);
  });
}

// ---- sound: the core makes every sound (WAV bytes); the page only plays ----

let audio = null;
let buffers = {};
let buffersVolume = null;

// Browsers allow sound only after the person's first click or key press.
function wakeAudio() {
  if (!audio) {
    try {
      audio = new AudioContext();
    } catch (e) {
      audio = null;
    }
  } else if (audio.state === "suspended") {
    audio.resume();
  }
}
window.addEventListener("pointerdown", wakeAudio, true);
window.addEventListener("keydown", wakeAudio, true);

async function play(key) {
  if (!audio) return;
  if (buffersVolume !== ui.volume) {
    buffers = {};
    buffersVolume = ui.volume;
  }
  try {
    if (!buffers[key]) {
      const bytes = game.sound(key);
      if (!bytes.length) return;
      buffers[key] = await audio.decodeAudioData(bytes.slice().buffer);
    }
    const source = audio.createBufferSource();
    source.buffer = buffers[key];
    source.connect(audio.destination);
    source.start();
    document.body.dataset.sound = key;
  } catch (e) {
    // A sound that cannot play is skipped; the game goes on.
  }
}

// ---- the loop ----

function refresh() {
  const text = game.ui();
  if (text === lastText) return;
  lastText = text;
  ui = JSON.parse(text);
  if (ui.error) {
    el("menus").hidden = false;
    el("message").textContent = ui.error;
    return;
  }
  if (ui.save) {
    writeStore("defaults", ui.save.defaults);
    writeStore("player", ui.save.player);
  }
  if (ui.navigate) window.location.href = ui.navigate;
  if (ui.sounds) ui.sounds.forEach(play);
  // Messages first: a Leave or Dissolve goes out before the transport closes.
  if (ui.net_requests) {
    flushNet();
    ui.net_requests.forEach(netRequest);
  }
  const inMatch = ui.screen === "match";
  const starting = inMatch && el("match").hidden;
  el("menus").hidden = inMatch;
  el("match").hidden = !inMatch;
  // Every new match gets a view the size of the window.
  if (starting) {
    overlayText = "";
    resize();
  }
  if (inMatch) {
    drawBar(ui.match);
    drawGrid(ui.match);
    drawOverlay(ui.match);
  } else {
    hideTip();
    drawMenus();
  }
  document.body.dataset.screen = ui.screen;
}

function frame(now) {
  game.advance(now);
  refresh();
  if (ui && ui.screen === "match") drawMatch();
  flushNet();
  requestAnimationFrame(frame);
}

async function start() {
  await init();
  const [strings, glossary] = await Promise.all([
    fetch("data/strings.json").then((r) => r.text()),
    fetch("data/glossary.json").then((r) => r.text()),
  ]);
  const entropy = new Uint32Array(1);
  crypto.getRandomValues(entropy);
  game = new Game(strings, glossary, readStore("defaults"), readStore("player"), crypto.randomUUID(), entropy[0]);
  hints = JSON.parse(game.hints());
  // `?codes`: the pasted-code path (Invite Code, Reply Code), for the
  // automated online check only.
  if (new URLSearchParams(location.search).has("codes")) game.act("flag.codes", "on", false, false);
  assets = JSON.parse(render_assets());
  el("tab-icon").href = assets.tab_icon;
  bindInput();
  refresh();
  document.body.dataset.ready = "true";
  requestAnimationFrame(frame);
}

start();
