// Module editor: a module drawn from SpDrS60 tiles on a grid, as in Rocrail's editor, and
// saved as its module file (schema version 3), the one source of everything about the
// module. The grid is the module's real size divided by SPDRS60_CELL_MM (50 mm), rounded.
// A tile has a place (x, y), a turn (0 to 3, a quarter clockwise each), a mirror (flipped
// across its track, before turning) and what it stands for: a turnout's or signal's name
// (id), the track it belongs to (track), its detection block (block), its device (device,
// in the module's device list with its address) and a text (name plates, train numbers).
// Ends are found where track leaves the grid; they get a name and an end profile.
"use strict";

const SVG_NS = "http://www.w3.org/2000/svg";
const CELL = SPDRS60_SIZE;
const STORE = "hive-rail-module";
const SCHEMA_VERSION = 3;
const WITH_TEXT = new Set(["plate", "train-number"]);
// The signal tiles: which real signal each shows, the aspects it can show (Ril 301) and
// the lamps it may have. Combinations show both signals' aspects; the distant signal under
// a main signal is dark while the main shows Hp 0.
// viessmann: the Viessmann H0 models (LED) that look like the tile's signal.
const SIGNALS = {
  // H/V main signals, one per kind
  "signal-hv-block": { face: "hv-block", viessmann: "4011, 4721 (multiplex)", palette: "hp1", options: [], aspects: ["hp0", "hp1"] },
  "signal-hv-entry": { face: "hv-entry", viessmann: "4012, 4722 (multiplex)", palette: "hp2", options: [], aspects: ["hp0", "hp1", "hp2"] },
  "signal-hv-departure": { face: "hv-departure", viessmann: "4013, 4723 (multiplex)", palette: "hp1", options: ["one-red"], aspects: ["hp0", "hp1", "hp2", "hp0 sh1"] },
  "signal-hv-block-distant": { face: "hv-block-distant", viessmann: "4014, 4724 (multiplex)", palette: "hp1 vr1", options: [],
    aspects: ["hp0", "hp1 vr0", "hp1 vr1", "hp1 vr2"] },
  "signal-hv-entry-distant": { face: "hv-entry-distant", viessmann: "4015, 4725 (multiplex)", palette: "hp1 vr1", options: [],
    aspects: ["hp0", "hp1 vr0", "hp1 vr1", "hp1 vr2", "hp2 vr0", "hp2 vr1", "hp2 vr2"] },
  "signal-hv-departure-distant": { face: "hv-departure-distant", viessmann: "4016, 4726 (multiplex)", palette: "hp1 vr1", options: ["one-red"],
    aspects: ["hp0", "hp1 vr0", "hp1 vr1", "hp1 vr2", "hp2 vr0", "hp2 vr1", "hp2 vr2", "hp0 sh1"] },
  "signal-hv-distant": { face: "hv-distant", viessmann: "4010, 4720 (multiplex); with the marker light 4030, 4730 (multiplex)", palette: "vr1", options: ["extra-lamp"], aspects: ["vr0", "vr1", "vr2"] },
  "signal-ks-main": { face: "ks-main", viessmann: "4042 entry, 4043 departure (Sh 1)", palette: "ks1", options: ["sh-lamp"], aspects: ["hp0", "ks1", "hp0 sh1"] },
  "signal-ks-multi": { face: "ks-multi", viessmann: "4045 entry, 4046 departure (Sh 1)", palette: "ks2", options: ["sh-lamp", "extra-lamp"], aspects: ["hp0", "ks1", "ks2", "hp0 sh1"] },
  "signal-ks-distant": { face: "ks-distant", viessmann: "4040", palette: "ks1", options: ["extra-lamp"], aspects: ["ks1", "ks2"] },
  "signal-sh": { face: "sh-light", viessmann: "4017, 4727 (multiplex) high; 4018, 4728 (multiplex) low", palette: "sh1", options: [], aspects: ["sh0", "sh1"] },
};
const OPTION_NAMES = {
  "sh-lamp": "Sh 1 (departure signal)",
  "one-red": "Hp 0 with one red (both reds on one output)",
  "extra-lamp": "Upper white (Zusatzlicht, short braking distance)",
};
// The lamp an aspect needs, where a signal may be without it.
const ASPECT_LAMPS = { sh1: "sh-lamp" };
const ASPECT_NAMES = { hp0: "Hp 0", hp1: "Hp 1", hp2: "Hp 2", ks1: "Ks 1", ks2: "Ks 2", vr0: "Vr 0", vr1: "Vr 1", vr2: "Vr 2", sh0: "Sh 0", sh1: "Sh 1" };
const aspectName = (aspect) => aspect.split(" ").map((a) => ASPECT_NAMES[a]).join(" + ");
let faceAspect = null; // the aspect the selected signal's face shows
// The device a tile switches or reports, by tile: its type in the module's device list.
const DEVICE_TYPES = {
  "turnout-right": "turnout", "turnout-left": "turnout", "double-slip": "turnout",
  "sensor": "occupancy", "sensor-curve": "occupancy", "level-crossing": "crossing", "district-border": "relay",
  lamp: "light", windmill: "animation", action: "animation",
  ...Object.fromEntries(Object.keys(SIGNALS).map((tile) => [tile, "signal"])),
};
// Tiles with a name of their own: turnouts, signals, level crossings.
const NAMED = new Set(["turnout-right", "turnout-left", "double-slip", "level-crossing", "sensor", "sensor-curve",
  "lamp", "windmill", "action", ...Object.keys(SIGNALS)]);
// Common accessory and feedback decoders, offered for a device's decoder type.
const DECODERS = {
  turnout: ["Uhlenbrock 67300 (switch decoder)", "Viessmann 5229 (motor turnout decoder)", "ESU SwitchPilot", "ESU SwitchPilot Servo",
    "Digikeijs DR4018", "Lenz LS150", "LDT S-DEC-4", "DCC-EX servo (EX-IOExpander)", "Arduino (own)"],
  signal: ["Viessmann 5229", "Uhlenbrock 67300", "LDT LS-DEC", "ESU SignalPilot", "Digikeijs DR4018", "MobaLedLib", "Arduino (own)"],
  occupancy: ["Digikeijs DR4088LN-CS (current sensing)", "Uhlenbrock 63320", "Uhlenbrock 63340", "LDT RM-GB-8-N", "Blücher GBM16XN", "Arduino (own)"],
  crossing: ["Viessmann 5229", "MobaLedLib", "Arduino (own)"],
  relay: ["Hive-Rail node board relay", "Viessmann 5552 (relay module)", "Arduino (own)"],
  light: ["Viessmann 5229", "MobaLedLib", "Uhlenbrock 67300", "Arduino (own)"],
  animation: ["Viessmann eMotion", "Viessmann 5229", "MobaLedLib", "Arduino (own)"],
};
const BUSES = { loconet: "LocoNet", dcc: "DCC accessory" };
// The palette, in the order a module is usually drawn.
const PALETTE = ["track-straight", "track-curve", "sensor", "sensor-curve", "turnout-right", "turnout-left",
  "buffer-stop", "crossing", "double-slip", ...Object.keys(SIGNALS), "level-crossing", "district-border", "platform-track", "train-number", "lamp", "windmill", "action", "plate"];
// Palette entries that place a tile with options already set.
const PRESETS = { "platform-track": { tile: "track-straight", options: ["platform-top", "platform-bottom"] } };
// Straight tiles that can lie along a platform, above, below or both.
const PLATFORM_TILES = new Set(["track-straight", "sensor", "train-number"]);
const PLATFORM_OPTIONS = [["platform-top", "Platform above"], ["platform-bottom", "Platform below"]];
const SIDES = ["left", "top", "right", "bottom"];
const DEFAULT_END = { left: "A", right: "B", top: "C", bottom: "D" };

function emptyModule() {
  return {
    schema_version: SCHEMA_VERSION, module_id: "new-module", friendly_name: "New module", owner: "", scale: "H0",
    size_mm: [900, 546], ends: [], tiles: [], stations: [], blocks: [], devices: [],
  };
}

let module = emptyModule();
let tool = null; // the palette tile being placed, or null to select
let toolTurn = 0;
let toolMirror = false;
let toolOptions = []; // options the tile being placed gets (a platform track's platforms)
let toolEntry = null; // the palette entry chosen
let selected = null; // index into module.tiles
let zoom = 1.5;
let painting = false;
let pointerCell = null; // the cell under the pointer, for the ghost of the tile being placed

const board = document.getElementById("board");
const gridWidth = () => Math.max(1, Math.round(module.size_mm[0] / SPDRS60_CELL_MM));
const gridHeight = () => Math.max(1, Math.round(module.size_mm[1] / SPDRS60_CELL_MM));

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

// A port of a tile's path ("S0": the bottom edge of the first cell, as drawn) where it lies
// on the grid: its cell and the side of the cell, after mirroring and turning.
const TURN_SIDE = { W: "N", N: "E", E: "S", S: "W" };
function portOnGrid(t, port) {
  const n = SPDRS60_TILES[t.tile]?.cells || 1;
  let side = port[0];
  const i = Number(port.slice(1));
  if (t.mirror) side = { N: "S", S: "N" }[side] || side;
  for (let k = 0; k < t.turn % 4; k++) side = TURN_SIDE[side];
  const cell = [[i, 0], [0, i], [n - 1 - i, 0], [0, n - 1 - i]][t.turn % 4];
  return { x: t.x + cell[0], y: t.y + cell[1], side };
}

// A tile's name in small text beside it, on the side away from what the tile draws there
// (a signal's head, a turnout's diverging leg); in grid units (32 per cell).
const LABEL_SIDE = {
  "turnout-right": "N", "turnout-left": "S", "double-slip": "N", sensor: "N", "sensor-curve": "N",
  lamp: "S", windmill: "S", action: "S",
  ...Object.fromEntries(Object.keys(SPDRS60_TILES).filter((n) => n.startsWith("signal-")).map((n) => [n, "N"])),
};

function tileLabel(t) {
  let side = LABEL_SIDE[t.tile];
  if (!side || !(t.name ?? t.id)) return null;
  if (t.mirror) side = { N: "S", S: "N" }[side] || side;
  for (let k = 0; k < (t.turn || 0) % 4; k++) side = { W: "N", N: "E", E: "S", S: "W" }[side];
  const cells = SPDRS60_TILES[t.tile]?.cells || 1;
  const [w, h] = (t.turn || 0) % 2 === 0 ? [cells, 1] : [1, cells];
  const [cx, cy] = [(t.x + w / 2) * 32, (t.y + h / 2) * 32];
  const [hw, hh] = [(w * 32) / 2, (h * 32) / 2];
  return {
    text: t.name ?? t.id,
    ...{ N: { x: cx, y: cy - hh + 6.5, anchor: "middle" }, S: { x: cx, y: cy + hh - 1.5, anchor: "middle" },
      W: { x: cx - hw + 1.5, y: cy + 2, anchor: "start" }, E: { x: cx + hw - 1.5, y: cy + 2, anchor: "end" } }[side],
  };
}

const STEP = { W: [-1, 0], E: [1, 0], N: [0, -1], S: [0, 1] };
const OPPOSITE = { W: "E", E: "W", N: "S", S: "N" };

// Every port of every tile, with what it meets: another tile's port, the grid's edge (an
// end of the module), or nothing (an open end, drawn in red).
function ports() {
  const all = [];
  module.tiles.forEach((t, index) => {
    // A buffer stop's path has one port: where its track comes in.
    for (const [path, ends] of Object.entries(SPDRS60_TILES[t.tile]?.paths || {})) {
      for (const port of ends) all.push({ index, path, ...portOnGrid(t, port) });
    }
  });
  const at = new Map(all.map((p) => [`${p.x},${p.y},${p.side}`, p]));
  const [w, h] = [gridWidth(), gridHeight()];
  for (const p of all) {
    const [dx, dy] = STEP[p.side];
    const [nx, ny] = [p.x + dx, p.y + dy];
    if (nx < 0 || ny < 0 || nx >= w || ny >= h) {
      p.meets = "edge";
      p.edge = { W: "left", E: "right", N: "top", S: "bottom" }[p.side];
    } else {
      p.meets = at.has(`${nx},${ny},${OPPOSITE[p.side]}`) ? "tile" : "open";
    }
  }
  return all;
}

// The module's ends: the grid edges track leaves by, each with its tracks in order.
function endsFound() {
  const bySide = {};
  for (const p of ports().filter((p) => p.meets === "edge")) (bySide[p.edge] ||= []).push(p);
  return SIDES.filter((side) => bySide[side]).map((side) => ({
    side,
    tracks: bySide[side].sort((a, b) => (side === "left" || side === "right" ? a.y - b.y : a.x - b.x))
      .map((p) => module.tiles[p.index].track || "?"),
  }));
}

// Each found end gets its entry in the module (a name and a profile), kept when it exists.
function syncEnds() {
  const found = endsFound().map((e) => e.side);
  module.ends = module.ends.filter((e) => found.includes(e.side));
  for (const side of found) {
    if (!module.ends.some((e) => e.side === side)) module.ends.push({ id: DEFAULT_END[side], side, profile: "" });
  }
  module.ends.sort((a, b) => SIDES.indexOf(a.side) - SIDES.indexOf(b.side));
}

// The transform that places a tile: turned a quarter at a time inside its own box, mirrored
// across its track first.
function placement(t) {
  const w = (SPDRS60_TILES[t.tile]?.cells || 1) * CELL;
  const turn = ["", `translate(${CELL} 0) rotate(90)`, `translate(${w} ${CELL}) rotate(180)`, `translate(0 ${w}) rotate(270)`][t.turn % 4];
  const mirror = t.mirror ? `translate(0 ${CELL}) scale(1 -1)` : "";
  return `translate(${t.x * CELL} ${t.y * CELL}) ${turn} ${mirror}`.trim();
}

function tileGroup(t, parent, extraClass = "") {
  const options = (t.options || []).join(" ");
  // A name plate has no track to turn: it always reads upright.
  const placed = t.tile === "plate" ? { ...t, turn: 0, mirror: false } : t;
  const g = el("g", { transform: placement(placed), class: `${extraClass} ${options} ${lampTest()}`.trim() }, parent);
  g.innerHTML = SPDRS60_TILES[t.tile].body;
  for (const text of g.querySelectorAll("text")) {
    if (t.text) text.textContent = t.text;
    // A train number's digits are turned back upright on a tile turned round or mirrored.
    const [x, y] = [Number(text.getAttribute("x")), Number(text.getAttribute("y")) - 3];
    if (placed.turn % 4 === 2) text.setAttribute("transform", `rotate(180 ${x} ${y})`);
    if (placed.mirror) text.setAttribute("transform", `${text.getAttribute("transform") || ""} translate(0 ${2 * y}) scale(1 -1)`.trim());
  }
  return g;
}

function lampTest() {
  return document.getElementById("lamp-test").value;
}

function tileAt(x, y) {
  return module.tiles.findIndex((t) => cellsOf(t).some(([cx, cy]) => cx === x && cy === y));
}

const MARGIN = 14; // room around the grid for the end names

function render() {
  const [gw, gh] = [gridWidth(), gridHeight()];
  const w = gw * CELL;
  const h = gh * CELL;
  board.setAttribute("viewBox", `${-MARGIN} ${-MARGIN} ${w + 2 * MARGIN} ${h + 2 * MARGIN}`);
  board.setAttribute("width", (w + 2 * MARGIN) * zoom);
  board.setAttribute("height", (h + 2 * MARGIN) * zoom);
  board.replaceChildren();
  el("style", {}, board).textContent = SPDRS60_STYLE;
  const defs = el("defs", {}, board);
  const pattern = el("pattern", { id: "grid", width: CELL, height: CELL, patternUnits: "userSpaceOnUse" }, defs);
  el("rect", { class: "sp-tile", x: 0, y: 0, width: CELL, height: CELL }, pattern);
  el("rect", { x: 0, y: 0, width: w, height: h, fill: "url(#grid)" }, board);
  // Where the module's boxes meet (`boxes` in the module file): no turnout or signal there.
  let joint = 0;
  for (const box of (module.boxes || []).slice(0, -1)) {
    joint += box.length_mm;
    const x = (joint / SPDRS60_CELL_MM) * CELL;
    el("line", { class: "box-joint", x1: x, y1: 0, x2: x, y2: h }, board);
  }
  module.tiles.forEach((t) => tileGroup(t, board));
  for (const t of module.tiles) {
    const label = tileLabel(t);
    if (!label) continue;
    const node = el("text", { x: label.x, y: label.y, class: "tile-label", "text-anchor": label.anchor }, board);
    node.textContent = label.text;
  }
  // Ends at the grid's edge, named; open track ends in red.
  syncEnds();
  for (const p of ports()) {
    const [cx, cy] = [p.x * CELL + CELL / 2, p.y * CELL + CELL / 2];
    const [dx, dy] = STEP[p.side];
    const [ex, ey] = [cx + (dx * CELL) / 2, cy + (dy * CELL) / 2];
    if (p.meets === "open") el("circle", { class: "open-end", cx: ex, cy: ey, r: 3.5 }, board);
    if (p.meets === "edge") {
      const end = module.ends.find((e) => e.side === p.edge);
      const label = el("text", { class: "end-label", x: ex + dx * 7, y: ey + dy * 7 + 3, "text-anchor": "middle" }, board);
      label.textContent = end?.id || "?";
    }
  }
  if (selected !== null && module.tiles[selected]) {
    for (const [x, y] of cellsOf(module.tiles[selected])) {
      el("rect", { class: "selected-frame", x: x * CELL + 1, y: y * CELL + 1, width: CELL - 2, height: CELL - 2 }, board);
    }
  }
  el("g", { id: "overlay" }, board);
  if (pointerCell) showGhost(...pointerCell);
  document.getElementById("grid-size").textContent = `${gw} × ${gh} cells of ${SPDRS60_CELL_MM} mm`;
  save();
}

// The tile under the pointer while placing: a ghost of what a click puts there.
function showGhost(x, y) {
  const overlay = document.getElementById("overlay");
  if (!overlay) return;
  overlay.replaceChildren();
  if (x < 0 || y < 0 || x >= gridWidth() || y >= gridHeight()) return;
  if (tool) tileGroup({ tile: tool, x, y, turn: toolTurn, mirror: toolMirror, options: toolOptions }, overlay, "ghost");
  el("rect", { class: "cursor", x: x * CELL + 0.5, y: y * CELL + 0.5, width: CELL - 1, height: CELL - 1 }, overlay);
}

function cellFromEvent(event) {
  const box = board.getBoundingClientRect();
  return [
    Math.floor((event.clientX - box.left) / (CELL * zoom) - MARGIN / CELL),
    Math.floor((event.clientY - box.top) / (CELL * zoom) - MARGIN / CELL),
  ];
}

function place(x, y) {
  const t = { tile: tool, x, y, turn: toolTurn, mirror: toolMirror, ...(toolOptions.length ? { options: [...toolOptions] } : {}) };
  const cells = cellsOf(t);
  if (cells.some(([cx, cy]) => cx < 0 || cy < 0 || cx >= gridWidth() || cy >= gridHeight())) return;
  // A new tile replaces whatever it covers.
  module.tiles = module.tiles.filter((other) => !cellsOf(other).some(([ox, oy]) => cells.some(([cx, cy]) => cx === ox && cy === oy)));
  module.tiles.push(t);
  selected = module.tiles.length - 1;
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
  pointerCell = [x, y];
  if (painting && tool && tileAt(x, y) < 0) place(x, y);
  showGhost(x, y);
});
window.addEventListener("pointerup", () => { painting = false; });
board.addEventListener("pointerleave", () => {
  pointerCell = null;
  document.getElementById("overlay")?.replaceChildren();
});

// Turning and mirroring: the selected tile, or the tile about to be placed.
function turn(by) {
  if (selected !== null && !tool) {
    const t = module.tiles[selected];
    t.turn = (t.turn + by + 4) % 4;
  } else {
    toolTurn = (toolTurn + by + 4) % 4;
  }
  render();
  showProperties();
}

function mirror() {
  if (selected !== null && !tool) {
    module.tiles[selected].mirror = !module.tiles[selected].mirror;
  } else {
    toolMirror = !toolMirror;
  }
  render();
  showProperties();
}

function remove() {
  if (selected === null) return;
  module.tiles.splice(selected, 1);
  selected = null;
  pruneDevices();
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
  toolEntry = name;
  tool = PRESETS[name]?.tile || name;
  toolOptions = PRESETS[name]?.options || [];
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
    const info = SPDRS60_TILES[PRESETS[name]?.tile || name];
    const button = document.createElement("button");
    button.type = "button";
    button.dataset.tile = name;
    button.title = info.title + (SIGNALS[name] ? `\nViessmann ${SIGNALS[name].viessmann}` : "");
    if (info.cells === 2) button.classList.add("wide");
    const w = info.cells * CELL;
    button.innerHTML = `<svg viewBox="0 0 ${w} ${CELL}" width="${(w * 40) / CELL}" height="40"><style>${SPDRS60_STYLE}</style>${
      Array.from({ length: info.cells }, (_, i) => `<rect class="sp-tile" x="${i * CELL}" y="0" width="${CELL}" height="${CELL}"/>`).join("")
    }<g class="${SIGNALS[name]?.palette || (PRESETS[name]?.options || []).join(" ")}">${info.body}</g></svg><span>${name.replace(/-/g, " ")}</span>`;
    if (name === "plate") button.querySelector("text").textContent = "Ls 1";
    if (name === "train-number") button.querySelector("text").textContent = "2001";
    button.addEventListener("click", () => chooseTool(name));
    palette.appendChild(button);
  }
}

// --- devices: the module's device list, one entry per tile that has one ---
function deviceOf(t) {
  return module.devices.find((d) => d.id === t.device);
}

// A tile's device id follows its name (or its place): "point_w1", "signal_a1", "sensor_5_3".
function deviceId(t) {
  const type = DEVICE_TYPES[t.tile];
  const name = (t.id || `${t.x}_${t.y}`).toLowerCase().replace(/[^a-z0-9]+/g, "_");
  return `${{ turnout: "point", occupancy: "sensor" }[type] || type}_${name}`;
}

// Sets one of a tile's device fields (bus, address, decoder); the device is made when it is
// first given something and goes again when it has nothing left.
function setDeviceField(t, key, value) {
  let device = deviceOf(t);
  if (!device) {
    device = { id: deviceId(t), type: DEVICE_TYPES[t.tile], bus: "loconet" };
    module.devices.push(device);
    t.device = device.id;
  }
  const number = key === "address";
  if (value === "" || (number && !Number.isInteger(Number(value)))) delete device[key];
  else device[key] = number ? Number(value) : value;
  if (device.address === undefined && !device.decoder) {
    delete t.device;
    pruneDevices();
  }
}

// A renamed tile renames its device.
function renameDevice(t) {
  const device = deviceOf(t);
  const id = deviceId(t);
  if (!device || device.id === id || module.devices.some((d) => d.id === id)) return;
  device.id = id;
  t.device = id;
}

// Devices no tile uses any more go.
function pruneDevices() {
  const used = new Set(module.tiles.map((t) => t.device).filter(Boolean));
  module.devices = module.devices.filter((d) => used.has(d.id));
}

// --- properties: the selected tile, or the module ---
function escapeHtml(value) {
  return String(value ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}

function showProperties() {
  const box = document.getElementById("properties");
  const t = selected !== null ? module.tiles[selected] : null;
  const placing = tool
    ? `<p class="hint-text placing">Placing <strong>${escapeHtml(toolEntry)}</strong>, turned ${toolTurn * 90}°${toolMirror ? ", mirrored" : ""}. Click or drag on the grid; <kbd>R</kbd> turns, <kbd>M</kbd> mirrors, <kbd>Esc</kbd> ends.</p>`
    : "";
  if (!t) {
    if (tool) {
      document.getElementById("properties-title").textContent = "Placing";
      box.innerHTML = placing;
    } else {
      showModule(box);
    }
    return;
  }
  document.getElementById("properties-title").textContent = tool ? "Placed tile" : "Selected tile";
  const device = deviceOf(t);
  const carriesTrack = Object.keys(SPDRS60_TILES[t.tile].paths || {}).length > 0;
  const type = DEVICE_TYPES[t.tile];
  box.innerHTML = `${placing}
    <dl>
      <dt>Tile</dt><dd>${escapeHtml(t.tile)}</dd>
      <dt>Place</dt><dd>${t.x}, ${t.y}</dd>
      <dt>Turned</dt><dd class="row-buttons"><button type="button" data-act="ccw" title="Shift+R">⟲</button><span>${t.turn * 90}°</span><button type="button" data-act="cw" title="R">⟳</button></dd>
      <dt>Mirrored</dt><dd><input type="checkbox" data-field="mirror" ${t.mirror ? "checked" : ""} style="width:auto"></dd>
      ${NAMED.has(t.tile) ? `<dt>Name</dt><dd><input type="text" data-field="id" value="${escapeHtml(t.id)}" placeholder="${{ turnout: "W1", signal: "A, N2", occupancy: "S12", crossing: "BÜ 1" }[type] || ""}"></dd>` : ""}
      ${(t.tile.endsWith("-distant") && t.tile !== "signal-ks-distant") ? `<dt>Distant for</dt><dd><input type="text" data-field="distant" value="${escapeHtml(t.distant)}" placeholder="a, f, n3/n4, 15"></dd>` : ""}
      ${carriesTrack ? `<dt>Track</dt><dd><input type="text" data-field="track" value="${escapeHtml(t.track)}" placeholder="1, 2, siding"></dd>
      <dt>Block</dt><dd><input type="text" data-field="block" value="${escapeHtml(t.block)}" placeholder="adorf_1"></dd>` : ""}
      ${type ? `</dl><h3>Device (${escapeHtml(type)})</h3><dl>
      <dt>Bus</dt><dd><select data-device="bus">${Object.entries(BUSES).map(([key, label]) => `<option value="${key}" ${(device?.bus || "loconet") === key ? "selected" : ""}>${label}</option>`).join("")}</select></dd>
      <dt>Address</dt><dd><input type="number" min="1" max="4096" data-device="address" value="${device?.address ?? ""}" placeholder="${type === "occupancy" ? "sensor" : "switch"} address"></dd>
      <dt>Decoder</dt><dd><input type="text" data-device="decoder" list="decoders" value="${escapeHtml(device?.decoder)}" placeholder="decoder type"><datalist id="decoders">${(DECODERS[type] || []).map((d) => `<option value="${escapeHtml(d)}">`).join("")}</datalist></dd>
      ${device ? `<dt>Device id</dt><dd class="muted">${escapeHtml(device.id)}</dd>` : ""}` : ""}
      ${WITH_TEXT.has(t.tile) ? `<dt>Text</dt><dd><input type="text" data-field="text" value="${escapeHtml(t.text)}"></dd>` : ""}
      ${PLATFORM_TILES.has(t.tile) ? PLATFORM_OPTIONS.map(([option, label]) => `<dt>${label}</dt><dd><input type="checkbox" data-option="${option}" ${(t.options || []).includes(option) ? "checked" : ""} style="width:auto"></dd>`).join("") : ""}
      ${SIGNALS[t.tile] ? signalFields(t) : ""}
    </dl>
    <p class="row-buttons" style="margin-top:12px"><button type="button" data-act="delete">Remove tile</button></p>`;
  box.querySelector('[data-act="cw"]').onclick = () => turn(1);
  box.querySelector('[data-act="ccw"]').onclick = () => turn(-1);
  box.querySelector('[data-act="delete"]').onclick = remove;
  box.querySelector('[data-field="mirror"]').onchange = (e) => { t.mirror = e.target.checked; render(); };
  for (const field of ["id", "track", "block", "text", "distant"]) {
    const input = box.querySelector(`[data-field="${field}"]`);
    if (!input) continue;
    input.oninput = (e) => {
      if (e.target.value) t[field] = e.target.value; else delete t[field];
      if (field === "id") renameDevice(t);
      render();
    };
  }
  for (const input of box.querySelectorAll("[data-device]")) {
    input.onchange = (e) => { setDeviceField(t, input.dataset.device, e.target.value); render(); showProperties(); };
  }
  for (const input of box.querySelectorAll("[data-option]")) {
    input.onchange = () => {
      const options = new Set(t.options || []);
      if (input.checked) options.add(input.dataset.option); else options.delete(input.dataset.option);
      t.options = [...options];
      render();
    };
  }
  for (const button of box.querySelectorAll("[data-aspect]")) {
    button.onclick = () => { faceAspect = button.dataset.aspect; showProperties(); };
  }
}

// A signal tile's lamps and how the signal looks on the track, showing the aspect chosen
// below it.
function signalFields(t) {
  const signal = SIGNALS[t.tile];
  // only the aspects this signal has the lamps for
  const fitted = (lamp) => !lamp || !signal.options.includes(lamp) || (t.options || []).includes(lamp);
  const aspects = signal.aspects.filter((a) => a.split(" ").every((part) => fitted(ASPECT_LAMPS[part])));
  if (!aspects.includes(faceAspect)) faceAspect = aspects[0];
  const face = SIGNAL_FACES[signal.face];
  const options = (t.options || []).join(" ");
  return `
    <dt>Viessmann</dt><dd>${signal.viessmann}</dd>
    ${signal.options.map((option) => `<dt>${OPTION_NAMES[option]}</dt><dd><input type="checkbox" data-option="${option}" ${(t.options || []).includes(option) ? "checked" : ""} style="width:auto"></dd>`).join("")}
    <dt>On the track</dt><dd><svg class="signal-face" viewBox="0 0 ${face.width} ${face.height}" height="${Math.min(170, face.height * 1.4)}"><style>${SIGNAL_FACE_STYLE}</style><g class="${options} ${faceAspect}${/vr|ks2/.test(faceAspect) && options.includes("extra-lamp") ? " short" : ""}">${face.body}</g></svg></dd>
    <dt>Aspect</dt><dd class="row-buttons aspect-buttons">${aspects.map((a) => `<button type="button" data-aspect="${a}" class="${a === faceAspect ? "chosen" : ""}">${aspectName(a)}</button>`).join("")}</dd>`;
}

// The module itself: its ends (found on the grid), its tracks, blocks and devices.
function showModule(box) {
  document.getElementById("properties-title").textContent = "Module";
  const ends = endsFound();
  const tracks = [...new Set(module.tiles.map((t) => t.track).filter(Boolean))].sort();
  const blocks = [...new Set(module.tiles.map((t) => t.block).filter(Boolean))].sort();
  const open = ports().filter((p) => p.meets === "open").length;
  box.innerHTML = `
    <h3>Ends</h3>
    ${ends.length ? ends.map((end) => {
      const entry = module.ends.find((e) => e.side === end.side);
      return `<dl class="end"><dt>${end.side}</dt><dd><input type="text" data-end="${end.side}" data-key="id" value="${escapeHtml(entry?.id)}" size="3"></dd>
        <dt>Profile</dt><dd><input type="text" data-end="${end.side}" data-key="profile" value="${escapeHtml(entry?.profile)}" placeholder="2E96"></dd>
        <dt>Tracks</dt><dd>${end.tracks.map(escapeHtml).join(", ")}</dd></dl>`;
    }).join("") : `<p class="hint-text">No track reaches the edge of the grid yet.</p>`}
    <h3>Drawing</h3>
    <p class="hint-text">${module.tiles.length} tiles; tracks ${tracks.map(escapeHtml).join(", ") || "none named"}; blocks ${blocks.map(escapeHtml).join(", ") || "none"}; ${module.devices.length} devices.
    ${open ? `<br><strong class="warn">${open} open track end${open === 1 ? "" : "s"}</strong> (red): track that meets neither another tile nor the edge.` : ""}</p>
    <p class="hint-text">Click a tile to edit it; choose a tile in the palette to draw.</p>`;
  for (const input of box.querySelectorAll("[data-end]")) {
    input.oninput = (e) => {
      const entry = module.ends.find((x) => x.side === input.dataset.end);
      if (entry) entry[input.dataset.key] = e.target.value;
      render();
    };
  }
}

// --- the module file ---
function save() {
  try { localStorage.setItem(STORE, JSON.stringify(module)); } catch { /* no storage: save the file instead */ }
}

// The module file as saved: tiles in reading order, ends with their tracks in order.
function moduleFile() {
  syncEnds();
  pruneDevices();
  const found = Object.fromEntries(endsFound().map((e) => [e.side, e.tracks]));
  return {
    ...module,
    schema_version: SCHEMA_VERSION,
    ends: module.ends.map((e) => ({ ...e, tracks: found[e.side] || [] })),
    tiles: [...module.tiles].sort((a, b) => a.y - b.y || a.x - b.x),
  };
}

function load(data) {
  if (!data || data.schema_version !== SCHEMA_VERSION || !Array.isArray(data.tiles)) {
    throw new Error(`not a module file of schema version ${SCHEMA_VERSION}`);
  }
  module = {
    ...emptyModule(),
    ...data,
    ends: (data.ends || []).map(({ id, side, profile }) => ({ id, side, profile })),
    tiles: data.tiles
      .filter((t) => SPDRS60_TILES[t.tile])
      .map((t) => ({ ...t, x: t.x | 0, y: t.y | 0, turn: (t.turn | 0) % 4, mirror: !!t.mirror })),
  };
  showModuleFields();
  selected = null;
  render();
  showProperties();
}

function showModuleFields() {
  document.getElementById("module-id").value = module.module_id;
  document.getElementById("module-name").value = module.friendly_name;
  document.getElementById("module-owner").value = module.owner || "";
  document.getElementById("module-length").value = module.size_mm[0];
  document.getElementById("module-depth").value = module.size_mm[1];
}

document.getElementById("export").addEventListener("click", () => {
  const blob = new Blob([JSON.stringify(moduleFile(), null, 1) + "\n"], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = `${module.module_id || "module"}.json`;
  a.click();
  URL.revokeObjectURL(a.href);
});
document.getElementById("import").addEventListener("change", async (event) => {
  const file = event.target.files[0];
  if (!file) return;
  try { load(JSON.parse(await file.text())); } catch (error) { alert(`Could not open ${file.name}: ${error.message}`); }
  event.target.value = "";
});
document.getElementById("new-module").addEventListener("click", () => {
  if (module.tiles.length && !confirm("Start a new module? The current drawing is kept only in a saved file.")) return;
  module = emptyModule();
  showModuleFields();
  selected = null;
  render();
  showProperties();
});
for (const [id, key] of [["module-id", "module_id"], ["module-name", "friendly_name"], ["module-owner", "owner"]]) {
  document.getElementById(id).addEventListener("input", (e) => { module[key] = e.target.value; save(); });
}
for (const id of ["module-length", "module-depth"]) {
  document.getElementById(id).addEventListener("change", () => {
    module.size_mm = [
      Math.max(SPDRS60_CELL_MM, Number(document.getElementById("module-length").value) || module.size_mm[0]),
      Math.max(SPDRS60_CELL_MM, Number(document.getElementById("module-depth").value) || module.size_mm[1]),
    ];
    render();
    showProperties();
  });
}
document.getElementById("lamp-test").addEventListener("change", render);
document.getElementById("zoom-in").addEventListener("click", () => { zoom = Math.min(4, zoom * 1.25); render(); });
document.getElementById("zoom-out").addEventListener("click", () => { zoom = Math.max(0.5, zoom / 1.25); render(); });

// The module files the web server has (its module folder), to open from the list.
async function listServerModules() {
  const select = document.getElementById("server-modules");
  try {
    const names = await (await fetch("modules")).json();
    for (const name of names) {
      const option = document.createElement("option");
      option.value = `modules/${name}`;
      option.textContent = name.replace(/\.json$/, "");
      select.appendChild(option);
    }
  } catch { /* no module folder: open files instead */ }
  select.addEventListener("change", async () => {
    if (!select.value) return;
    if (module.tiles.length && !confirm("Open this module? The current drawing is kept only in a saved file.")) {
      select.value = "";
      return;
    }
    try { load(await (await fetch(select.value)).json()); } catch (error) { alert(`Could not open ${select.value}: ${error.message}`); }
    select.value = "";
  });
}

// Opens a module: one named in the address (?module=<address of a module file>), else the
// one this browser kept, else a new one; ?select=x,y selects a tile.
async function start() {
  buildPalette();
  listServerModules();
  const from = new URLSearchParams(location.search).get("module");
  try {
    if (from) load(await (await fetch(from)).json());
    else {
      const kept = localStorage.getItem(STORE);
      if (kept) load(JSON.parse(kept));
      else { showModuleFields(); render(); }
    }
  } catch (error) {
    showModuleFields();
    render();
    if (from) alert(`Could not open the module ${from}: ${error.message}`);
  }
  const [sx, sy] = (new URLSearchParams(location.search).get("select") || "").split(",").map(Number);
  if (Number.isInteger(sx) && Number.isInteger(sy) && tileAt(sx, sy) >= 0) {
    selected = tileAt(sx, sy);
    render();
  }
  showProperties();
}

start();
