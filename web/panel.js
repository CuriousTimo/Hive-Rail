// Panel editor: SpDrS60 tiles placed on a grid, as in Rocrail's editor. A tile has a place
// (x, y), a turn (0 to 3, a quarter clockwise each), a mirror (flipped across its track),
// an optional text (name plates, train numbers) and an optional link to the layout (a block,
// turnout, signal or level crossing), which a live panel will follow.
"use strict";

const SVG_NS = "http://www.w3.org/2000/svg";
const CELL = SPDRS60_SIZE;
const STORE = "hive-rail-panel";
const WITH_TEXT = new Set(["plate", "train-number"]);
// What a tile can be linked to, by tile.
// The signal tiles: which real signal each shows, the aspects it can show (Ril 301) and
// the lamps it may have. Combinations show both signals' aspects; the distant signal under
// a main signal is dark while the main shows Hp 0.
const SIGNALS = {
  "signal-hv-main": { face: "hv-main", palette: "hp1", options: ["sh-lamp", "zs1-lamp"],
    aspects: ["hp0", "hp1", "hp2", "hp0 sh1", "hp0 zs1"] },
  "signal-hv-main-distant": { face: "hv-main-distant", palette: "hp1 vr1", options: ["sh-lamp", "zs1-lamp", "extra-lamp"],
    aspects: ["hp0", "hp1 vr0", "hp1 vr1", "hp1 vr2", "hp2 vr0", "hp2 vr1", "hp2 vr2", "hp0 sh1", "hp0 zs1"] },
  "signal-hv-distant": { face: "hv-distant", palette: "vr1", options: ["extra-lamp"], aspects: ["vr0", "vr1", "vr2"] },
  "signal-ks-main": { face: "ks-main", palette: "ks1", options: ["sh-lamp", "zs1-lamp"], aspects: ["hp0", "ks1", "hp0 sh1", "hp0 zs1"] },
  "signal-ks-multi": { face: "ks-multi", palette: "ks2", options: ["sh-lamp", "zs1-lamp"], aspects: ["hp0", "ks1", "ks2", "hp0 sh1", "hp0 zs1"] },
  "signal-ks-distant": { face: "ks-distant", palette: "ks1", options: [], aspects: ["ks1", "ks2"] },
  "signal-sh": { face: "sh-light", palette: "sh1", options: [], aspects: ["sh0", "sh1"] },
};
const OPTION_NAMES = { "sh-lamp": "Sh 1 lamps (white)", "zs1-lamp": "Zs 1 lamps", "extra-lamp": "Zusatzlicht (short braking distance)" };
const ASPECT_NAMES = { hp0: "Hp 0", hp1: "Hp 1", hp2: "Hp 2", ks1: "Ks 1", ks2: "Ks 2", vr0: "Vr 0", vr1: "Vr 1", vr2: "Vr 2", sh0: "Sh 0", sh1: "Sh 1", zs1: "Zs 1" };
const aspectName = (aspect) => aspect.split(" ").map((a) => ASPECT_NAMES[a]).join(" + ");
let faceAspect = null; // the aspect the selected signal's face shows
// What a tile can be linked to, by tile.
const LINK_KINDS = {
  "turnout-right": ["turnout"], "turnout-left": ["turnout"], "double-slip": ["turnout"],
  "level-crossing": ["crossing"], "sensor": ["block", "sensor"], "sensor-curve": ["block", "sensor"],
  ...Object.fromEntries(Object.keys(SIGNALS).map((tile) => [tile, ["signal"]])),
};
const DEFAULT_LINK = ["block"];
// The palette, in the order a panel is usually drawn.
const PALETTE = ["track-straight", "track-curve", "sensor", "sensor-curve", "turnout-right", "turnout-left",
  "buffer-stop", "crossing", "double-slip", ...Object.keys(SIGNALS), "level-crossing", "district-border", "train-number", "plate"];

let panel = { schema: "hive-rail-panel/1", name: "Panel", width: 40, height: 24, tiles: [] };
let tool = null; // the palette tile being placed, or null to select
let toolTurn = 0;
let toolMirror = false;
let selected = null; // index into panel.tiles
let zoom = 1.5;
let painting = false;
let layoutIds = { block: [], turnout: [], signal: [], crossing: [] };
let signalAspects = {}; // layout signal id -> its aspects, such as ["red", "yellow", "green"]

const board = document.getElementById("board");

function el(name, attributes = {}, parent = null) {
  const node = document.createElementNS(SVG_NS, name);
  for (const [key, value] of Object.entries(attributes)) node.setAttribute(key, value);
  if (parent) parent.appendChild(node);
  return node;
}

// The cells a tile covers: two-cell tiles lie along their track, across when turned.
function cellsOf(t) {
  const n = SPDRS60_TILES[t.tile]?.cells || 1;
  return Array.from({ length: n }, (_, i) => (t.turn % 2 === 0 ? [t.x + i, t.y] : [t.x, t.y + i]));
}

// The transform that places a tile: turned a quarter at a time inside its own box, mirrored
// across its track first.
function placement(t) {
  const w = (SPDRS60_TILES[t.tile]?.cells || 1) * CELL;
  const turn = [
    "",
    `translate(${CELL} 0) rotate(90)`,
    `translate(${w} ${CELL}) rotate(180)`,
    `translate(0 ${w}) rotate(270)`,
  ][t.turn % 4];
  const mirror = t.mirror ? `translate(0 ${CELL}) scale(1 -1)` : "";
  return `translate(${t.x * CELL} ${t.y * CELL}) ${turn} ${mirror}`.trim();
}

function tileGroup(t, parent, extraClass = "") {
  const options = (t.options || []).join(" ");
  const g = el("g", { transform: placement(t), class: `${extraClass} ${options} ${lampTest()}`.trim() }, parent);
  g.innerHTML = SPDRS60_TILES[t.tile].body;
  if (t.text) for (const text of g.querySelectorAll("text")) text.textContent = t.text;
  return g;
}

function lampTest() {
  return document.getElementById("lamp-test").value;
}

function tileAt(x, y) {
  return panel.tiles.findIndex((t) => cellsOf(t).some(([cx, cy]) => cx === x && cy === y));
}

function render() {
  const w = panel.width * CELL;
  const h = panel.height * CELL;
  board.setAttribute("viewBox", `0 0 ${w} ${h}`);
  board.setAttribute("width", w * zoom);
  board.setAttribute("height", h * zoom);
  board.replaceChildren();
  el("style", {}, board).textContent = SPDRS60_STYLE;
  const defs = el("defs", {}, board);
  const pattern = el("pattern", { id: "grid", width: CELL, height: CELL, patternUnits: "userSpaceOnUse" }, defs);
  el("rect", { class: "sp-tile", x: 0, y: 0, width: CELL, height: CELL }, pattern);
  el("rect", { x: 0, y: 0, width: w, height: h, fill: "url(#grid)" }, board);
  panel.tiles.forEach((t) => tileGroup(t, board));
  if (selected !== null && panel.tiles[selected]) {
    for (const [x, y] of cellsOf(panel.tiles[selected])) {
      el("rect", { class: "selected-frame", x: x * CELL + 1, y: y * CELL + 1, width: CELL - 2, height: CELL - 2 }, board);
    }
  }
  el("g", { id: "overlay" }, board);
  save();
}

// The tile under the pointer while placing: a ghost of what a click puts there.
function showGhost(x, y) {
  const overlay = document.getElementById("overlay");
  if (!overlay) return;
  overlay.replaceChildren();
  if (x < 0 || y < 0 || x >= panel.width || y >= panel.height) return;
  if (tool) {
    tileGroup({ tile: tool, x, y, turn: toolTurn, mirror: toolMirror }, overlay, "ghost");
  }
  el("rect", { class: "cursor", x: x * CELL + 0.5, y: y * CELL + 0.5, width: CELL - 1, height: CELL - 1 }, overlay);
}

function cellFromEvent(event) {
  const box = board.getBoundingClientRect();
  return [Math.floor((event.clientX - box.left) / (CELL * zoom)), Math.floor((event.clientY - box.top) / (CELL * zoom))];
}

function place(x, y) {
  const t = { tile: tool, x, y, turn: toolTurn, mirror: toolMirror };
  const cells = cellsOf(t);
  if (cells.some(([cx, cy]) => cx >= panel.width || cy >= panel.height)) return;
  // A new tile replaces whatever it covers.
  panel.tiles = panel.tiles.filter((other) => !cellsOf(other).some(([ox, oy]) => cells.some(([cx, cy]) => cx === ox && cy === oy)));
  panel.tiles.push(t);
  selected = panel.tiles.length - 1;
  render();
  showProperties();
}

board.addEventListener("pointerdown", (event) => {
  const [x, y] = cellFromEvent(event);
  if (tool) {
    painting = true;
    place(x, y);
  } else {
    const index = tileAt(x, y);
    selected = index >= 0 ? index : null;
    render();
    showProperties();
  }
});
board.addEventListener("pointermove", (event) => {
  const [x, y] = cellFromEvent(event);
  if (painting && tool && tileAt(x, y) < 0) place(x, y);
  showGhost(x, y);
});
window.addEventListener("pointerup", () => { painting = false; });
board.addEventListener("pointerleave", () => document.getElementById("overlay")?.replaceChildren());

// Turning and mirroring: the selected tile, or the tile about to be placed.
function turn(by) {
  if (selected !== null && !tool) {
    const t = panel.tiles[selected];
    t.turn = (t.turn + by + 4) % 4;
    render();
    showProperties();
  } else {
    toolTurn = (toolTurn + by + 4) % 4;
  }
}

function mirror() {
  if (selected !== null && !tool) {
    panel.tiles[selected].mirror = !panel.tiles[selected].mirror;
    render();
    showProperties();
  } else {
    toolMirror = !toolMirror;
  }
}

function remove() {
  if (selected === null) return;
  panel.tiles.splice(selected, 1);
  selected = null;
  render();
  showProperties();
}

document.addEventListener("keydown", (event) => {
  if (event.target.closest("input, select, textarea")) return;
  if (event.key === "r" || event.key === "R") turn(event.shiftKey ? -1 : 1);
  else if (event.key === "m" || event.key === "M") mirror();
  else if (event.key === "Delete" || event.key === "Backspace") remove();
  else if (event.key === "Escape") chooseTool(null);
  else return;
  event.preventDefault();
});

// --- palette ---
function chooseTool(name) {
  tool = name;
  if (name) selected = null;
  for (const button of document.querySelectorAll("#palette button")) {
    button.classList.toggle("chosen", (button.dataset.tile || null) === name);
  }
  render();
  showProperties();
}

function buildPalette() {
  const palette = document.getElementById("palette");
  const select = document.createElement("button");
  select.type = "button";
  select.className = "select-tool chosen";
  select.textContent = "Select";
  select.addEventListener("click", () => chooseTool(null));
  palette.appendChild(select);
  for (const name of PALETTE) {
    const info = SPDRS60_TILES[name];
    const button = document.createElement("button");
    button.type = "button";
    button.dataset.tile = name;
    button.title = info.title;
    if (info.cells === 2) button.classList.add("wide");
    const w = info.cells * CELL;
    button.innerHTML = `<svg viewBox="0 0 ${w} ${CELL}" width="${(w * 40) / CELL}" height="40"><style>${SPDRS60_STYLE}</style>${
      Array.from({ length: info.cells }, (_, i) => `<rect class="sp-tile" x="${i * CELL}" y="0" width="${CELL}" height="${CELL}"/>`).join("")
    }<g class="${SIGNALS[name]?.palette || ""}">${info.body}</g></svg><span>${name.replace(/-/g, " ")}</span>`;
    if (name === "plate") button.querySelector("text").textContent = "Ls 1";
    if (name === "train-number") button.querySelector("text").textContent = "2001";
    button.addEventListener("click", () => chooseTool(name));
    palette.appendChild(button);
  }
}

// --- properties of the selected tile ---
function escapeHtml(value) {
  return String(value ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}

function showProperties() {
  const box = document.getElementById("properties");
  const t = selected !== null ? panel.tiles[selected] : null;
  if (!t) {
    box.innerHTML = tool
      ? `<p class="hint-text">Placing <strong>${escapeHtml(tool)}</strong>, turned ${toolTurn * 90}°${toolMirror ? ", mirrored" : ""}. Click or drag on the panel.</p>`
      : `<p class="hint-text">Nothing selected. With no tile chosen in the palette, click a tile on the panel to select it.</p>`;
    return;
  }
  const kinds = LINK_KINDS[t.tile] || DEFAULT_LINK;
  const link = t.link || { kind: kinds[0], id: "" };
  box.innerHTML = `
    <dl>
      <dt>Tile</dt><dd>${escapeHtml(t.tile)}</dd>
      <dt>Place</dt><dd>${t.x}, ${t.y}</dd>
      <dt>Turned</dt><dd class="row-buttons"><button type="button" data-act="ccw" title="Shift+R">⟲</button><span>${t.turn * 90}°</span><button type="button" data-act="cw" title="R">⟳</button></dd>
      <dt>Mirrored</dt><dd><input type="checkbox" data-field="mirror" ${t.mirror ? "checked" : ""} style="width:auto"></dd>
      ${WITH_TEXT.has(t.tile) ? `<dt>Text</dt><dd><input type="text" data-field="text" value="${escapeHtml(t.text)}"></dd>` : ""}
      ${SIGNALS[t.tile] ? signalFields(t) : ""}
      <dt>Linked to</dt><dd><select data-field="kind">${kinds.map((k) => `<option ${k === link.kind ? "selected" : ""}>${k}</option>`).join("")}</select></dd>
      <dt>Id</dt><dd><input type="text" data-field="id" list="link-ids" value="${escapeHtml(link.id)}" placeholder="module/name"></dd>
    </dl>
    <p class="row-buttons" style="margin-top:12px"><button type="button" data-act="delete">Remove tile</button></p>`;
  fillLinkIds(link.kind);
  box.querySelector('[data-act="cw"]').onclick = () => turn(1);
  box.querySelector('[data-act="ccw"]').onclick = () => turn(-1);
  box.querySelector('[data-act="delete"]').onclick = remove;
  box.querySelector('[data-field="mirror"]').onchange = (e) => { t.mirror = e.target.checked; render(); };
  for (const button of box.querySelectorAll("[data-aspect]")) {
    button.onclick = () => { faceAspect = button.dataset.aspect; showProperties(); };
  }
  for (const input of box.querySelectorAll("[data-option]")) {
    input.onchange = () => {
      const options = new Set(t.options || []);
      if (input.checked) options.add(input.dataset.option); else options.delete(input.dataset.option);
      t.options = [...options];
      render();
    };
  }
  const text = box.querySelector('[data-field="text"]');
  if (text) text.oninput = (e) => { t.text = e.target.value; render(); };
  box.querySelector('[data-field="kind"]').onchange = (e) => {
    t.link = { kind: e.target.value, id: t.link?.id || "" };
    fillLinkIds(e.target.value);
    save();
  };
  box.querySelector('[data-field="id"]').oninput = (e) => {
    const kind = box.querySelector('[data-field="kind"]').value;
    t.link = e.target.value ? { kind, id: e.target.value } : undefined;
    // A signal linked to the layout gets its white lamps when its aspects have them.
    const aspects = kind === "signal" && SIGNALS[t.tile] ? signalAspects[e.target.value] : null;
    if (aspects && aspects.includes("white") && SIGNALS[t.tile].options.includes("sh-lamp")) {
      t.options = [...new Set([...(t.options || []), "sh-lamp"])];
      render();
      showProperties();
      return;
    }
    save();
  };
}

// A signal tile's lamps and how the signal looks on the track, showing the aspect chosen
// below it.
function signalFields(t) {
  const signal = SIGNALS[t.tile];
  if (!signal.aspects.includes(faceAspect)) faceAspect = signal.aspects[0];
  const face = SIGNAL_FACES[signal.face];
  const options = (t.options || []).join(" ");
  return `
    ${signal.options.map((option) => `<dt>${OPTION_NAMES[option]}</dt><dd><input type="checkbox" data-option="${option}" ${(t.options || []).includes(option) ? "checked" : ""} style="width:auto"></dd>`).join("")}
    <dt>On the track</dt><dd><svg class="signal-face" viewBox="0 0 ${face.width} ${face.height}" height="${Math.min(170, face.height * 1.4)}"><style>${SIGNAL_FACE_STYLE}</style><g class="${options} ${faceAspect}${faceAspect.includes("vr") && options.includes("extra-lamp") ? " short" : ""}">${face.body}</g></svg></dd>
    <dt>Aspect</dt><dd class="row-buttons aspect-buttons">${signal.aspects.map((a) => `<button type="button" data-aspect="${a}" class="${a === faceAspect ? "chosen" : ""}">${aspectName(a)}</button>`).join("")}</dd>`;
}

function fillLinkIds(kind) {
  document.getElementById("link-ids").innerHTML = (layoutIds[kind] || []).map((id) => `<option value="${escapeHtml(id)}">`).join("");
}

// --- keeping and moving panels ---
function save() {
  try { localStorage.setItem(STORE, JSON.stringify(panel)); } catch { /* no storage: export instead */ }
}

function load(data) {
  if (!data || !Array.isArray(data.tiles)) throw new Error("not a panel file");
  panel = {
    schema: "hive-rail-panel/1",
    name: String(data.name || "Panel"),
    width: Math.max(4, Number(data.width) || 40),
    height: Math.max(4, Number(data.height) || 24),
    tiles: data.tiles
      .filter((t) => SPDRS60_TILES[t.tile])
      .map((t) => ({
        tile: t.tile, x: t.x | 0, y: t.y | 0, turn: (t.turn | 0) % 4, mirror: !!t.mirror,
        ...(t.text ? { text: String(t.text) } : {}),
        ...(Array.isArray(t.options) && t.options.length ? { options: t.options.map(String) } : {}),
        ...(t.face ? { face: String(t.face) } : {}),
        ...(t.link ? { link: t.link } : {}),
      })),
  };
  document.getElementById("panel-name").value = panel.name;
  document.getElementById("panel-width").value = panel.width;
  document.getElementById("panel-height").value = panel.height;
  selected = null;
  render();
  showProperties();
}

document.getElementById("export").addEventListener("click", () => {
  const ordered = { ...panel, tiles: [...panel.tiles].sort((a, b) => a.y - b.y || a.x - b.x) };
  const blob = new Blob([JSON.stringify(ordered, null, 1) + "\n"], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = `panel-${panel.name.toLowerCase().replace(/[^a-z0-9]+/g, "-")}.json`;
  a.click();
  URL.revokeObjectURL(a.href);
});
document.getElementById("import").addEventListener("change", async (event) => {
  const file = event.target.files[0];
  if (!file) return;
  try { load(JSON.parse(await file.text())); } catch (error) { alert(`Could not load ${file.name}: ${error.message}`); }
  event.target.value = "";
});
document.getElementById("panel-name").addEventListener("input", (e) => { panel.name = e.target.value; save(); });
for (const id of ["panel-width", "panel-height"]) {
  document.getElementById(id).addEventListener("change", () => {
    panel.width = Math.max(4, Number(document.getElementById("panel-width").value) || panel.width);
    panel.height = Math.max(4, Number(document.getElementById("panel-height").value) || panel.height);
    render();
  });
}
document.getElementById("lamp-test").addEventListener("change", render);
document.getElementById("zoom-in").addEventListener("click", () => { zoom = Math.min(4, zoom * 1.25); render(); });
document.getElementById("zoom-out").addEventListener("click", () => { zoom = Math.max(0.5, zoom / 1.25); render(); });

// The layout's blocks, turnouts, signals and level crossings, to link tiles to.
async function loadLayoutIds() {
  try {
    const layout = await (await fetch("api/layout")).json();
    layoutIds = {
      block: (layout.blocks || []).map((b) => b.id),
      turnout: (layout.turnouts || []).map((t) => t.id),
      signal: (layout.signals || []).map((s) => s.id),
      crossing: (layout.crossings || []).map((c) => c.id),
    };
    signalAspects = Object.fromEntries((layout.signals || []).map((s) => [s.id, s.aspects || []]));
  } catch { /* no layout: ids are typed by hand */ }
}

// Opens a panel: one named in the address (?panel=<address of a panel file>), else the one
// this browser kept, else an empty one; ?select=x,y selects a tile.
async function start() {
  buildPalette();
  const from = new URLSearchParams(location.search).get("panel");
  try {
    if (from) load(await (await fetch(from)).json());
    else {
      const kept = localStorage.getItem(STORE);
      if (kept) load(JSON.parse(kept));
      else render();
    }
  } catch (error) {
    render();
    if (from) alert(`Could not open the panel ${from}: ${error.message}`);
  }
  // ?select=x,y selects the tile at that cell.
  const [sx, sy] = (new URLSearchParams(location.search).get("select") || "").split(",").map(Number);
  if (Number.isInteger(sx) && Number.isInteger(sy) && tileAt(sx, sy) >= 0) {
    selected = tileAt(sx, sy);
    render();
  }
  showProperties();
  loadLayoutIds();
}

start();
