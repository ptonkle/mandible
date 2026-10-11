// The page draws what the core returns and decides nothing. It builds the
// DOM from the core's view model (ui()), forwards keyboard, mouse and
// transport events, and keeps the two settings strings in browser storage.
import init, { Game } from "./pkg/tool_shim.js";

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

function element(item) {
  if (item.type === "button") {
    const b = make("button", "button", item.text);
    b.type = "button";
    b.id = item.id;
    b.addEventListener("click", () => send(item.id, ""));
    return b;
  }
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
    wrap.append(make("label", "", item.label), box);
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

function drawBar(m) {
  const res = el("resources");
  res.replaceChildren();
  m.hud.resources.forEach((r) => {
    const box = make("div", "res " + r.key);
    const income = make("span", "income", r.income);
    income.dataset.term = r.term;
    box.append(make("span", "name", r.label), make("span", "value", r.value), income);
    res.append(box);
  });
  [m.hud.population, m.hud.tier].forEach((p) => {
    const box = make("div", "res");
    const name = make("span", "name", p.label);
    if (hints[p.term]) name.dataset.term = p.term;
    box.append(name, make("span", "value", p.value));
    res.append(box);
  });
  const queue = el("queue");
  queue.replaceChildren();
  m.queue.forEach((q) => {
    const item = make("button", "qitem", q.label);
    item.type = "button";
    const fill = make("span", "fill");
    fill.style.width = q.progress;
    item.append(fill);
    item.addEventListener("click", () => send(q.id, ""));
    item.addEventListener("mouseenter", () => showTip(q.label, q.rows, item));
    item.addEventListener("mouseleave", hideTip);
    queue.append(item);
  });
  const flags = el("flags");
  flags.replaceChildren();
  m.hud.flags.forEach((f) => {
    const flag = make("span", "flag", f.text);
    if (f.term && hints[f.term]) flag.dataset.term = f.term;
    flags.append(flag);
  });
  el("clock").textContent = m.hud.clock;
  const notices = el("notices");
  notices.replaceChildren();
  m.notices.forEach((n) => notices.append(make("div", "notice", n.text)));
}

function drawGrid(m) {
  const tabs = el("tabs");
  tabs.replaceChildren();
  m.tabs.forEach((t) => {
    const b = make("button", "tab" + (t.active ? " active" : ""), t.label);
    b.type = "button";
    b.append(make("span", "k", t.key));
    b.addEventListener("click", () => send(t.id, ""));
    tabs.append(b);
  });
  const buttons = el("buttons");
  buttons.replaceChildren();
  m.grid.forEach((g) => {
    const b = make("button", "gbtn");
    b.type = "button";
    if (g.empty) {
      b.classList.add("empty");
    } else {
      b.classList.add(g.state);
      if (g.active) b.classList.add("active");
      b.append(make("span", "label", g.label), make("span", "k", g.key));
      b.addEventListener("click", (e) => sendMods(g.id, e));
      b.addEventListener("mouseenter", () => {
        hovered = g.id;
        showTip(g.label, g.rows.concat([["", g.state_text]]), b);
      });
      b.addEventListener("mouseleave", () => {
        hovered = null;
        hideTip();
      });
      if (hovered === g.id) showTip(g.label, g.rows.concat([["", g.state_text]]), b);
    }
    buttons.append(b);
  });
}

function drawOverlay(m) {
  const overlay = el("overlay");
  const box = m.end || m.menu;
  overlay.hidden = !box;
  if (!box) return;
  overlay.replaceChildren();
  const panel = make("div", "panel");
  if (box.title) panel.append(make("h2", "", box.title));
  if (box.heading) panel.append(make("h2", "", box.heading));
  if (box.rows) panel.append(pairs(box.rows));
  const buttons = box.items || [box.button];
  buttons.forEach((b) => panel.append(element(b)));
  overlay.append(panel);
}

// ---- tooltips and term hints ----

function place(box, anchor) {
  const r = anchor.getBoundingClientRect();
  box.style.left = r.left + "px";
  box.style.top = r.top + "px";
  box.style.transform = "translateY(-100%)";
}

function showTip(title, rows, anchor) {
  const tip = el("tooltip");
  tip.replaceChildren(make("h3", "", title), pairs(rows));
  tip.hidden = false;
  place(tip, anchor);
}

function hideTip() {
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
      if (ui && ui.screen === "match" && !e.ctrlKey) e.preventDefault();
      if (e.altKey) e.preventDefault();
      if (e.repeat) return;
      game.key(e.code, type === "keydown", e.ctrlKey || e.altKey || e.metaKey, e.shiftKey);
    });
  });
  const map = el("map");
  ["mousedown", "mouseup", "mousemove"].forEach((type) => {
    map.addEventListener(type, (e) => {
      game.mouse(type, e.button, e.offsetX, e.offsetY, e.shiftKey);
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
}

// ---- transport ----

let online = null;

async function netRequest(req) {
  if (!online) {
    const module = await import("./online.js");
    online = module;
  }
  online.request(req, {
    event: (kind, peer, text) => game.net_event(kind, peer, text || ""),
    code: (which, code) => game.net_code(which, code),
  });
}

function flushNet() {
  if (!online) return;
  JSON.parse(game.outbox()).forEach((m) => online.send(m.to, m.text));
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
  if (ui.net_requests) ui.net_requests.forEach(netRequest);
  const inMatch = ui.screen === "match";
  el("menus").hidden = inMatch;
  el("match").hidden = !inMatch;
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
  if (renderer && ui && ui.screen === "match") renderer.draw(game);
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
  bindInput();
  refresh();
  document.body.dataset.ready = "true";
  requestAnimationFrame(frame);
}

start();
