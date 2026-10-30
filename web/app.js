// Hive-Rail web view: draws the layout from /api/layout as SVG and keeps it live from
// /events (Server-Sent Events). Layout coordinates are millimetres with y up; the screen
// has y down, so every point is drawn at (x, -y).

const SVG = "http://www.w3.org/2000/svg";
// Train colours stay clear of the track colours: red occupied, yellow reserved, blue
// transfer, green and orange turnouts.
const TRAIN_COLOURS = ["#ff8fe0", "#f2f2f2", "#c9a6ff", "#9bd8ff", "#d4ff6b", "#ffb3a7"];
// One colour per district: its name on the map, and the transfer blocks it feeds.
const DISTRICT_COLOURS = ["#20c5b5", "#a77bff", "#e08a3c", "#5fa8ff", "#e45c9c", "#9ccf4a"];
const LAMP = { red: "#ff4d4d", yellow: "#ffd34d", green: "#52d68a" };

const svg = document.getElementById("map");
const layer = (name) => document.getElementById(`layer-${name}`);
const elements = { blocks: new Map(), pieces: new Map(), feeds: new Map(), turnouts: new Map(), crossings: new Map(), signals: new Map() };
let districtColour = () => "var(--muted)";
let fit = { x: 0, y: 0, w: 1000, h: 500 };
let view = { ...fit };

function el(name, attributes, parent) {
  const node = document.createElementNS(SVG, name);
  for (const [key, value] of Object.entries(attributes)) node.setAttribute(key, value);
  if (parent) parent.appendChild(node);
  return node;
}

const flip = ([x, y]) => [x, -y];
const pointsAttr = (points) => points.map((p) => flip(p).map((v) => v.toFixed(1)).join(",")).join(" ");

function text(content, [x, y], className, parent, anchor = "middle") {
  const node = el("text", { x: x.toFixed(1), y: (-y).toFixed(1), class: className, "text-anchor": anchor }, parent);
  node.textContent = content;
  return node;
}

function drawLayout(layout) {
  document.getElementById("meeting").textContent = layout.meeting;
  document.title = `Hive-Rail – ${layout.meeting}`;
  districtColour = (district) => DISTRICT_COLOURS[Math.max(0, layout.districts.indexOf(district)) % DISTRICT_COLOURS.length];
  document.getElementById("districts").innerHTML = layout.districts
    .map((d) => `<li><span class="swatch" style="background:${districtColour(d)}"></span>Fed by ${escapeHtml(d)}</li>`)
    .join("");

  for (const module of layout.modules) {
    const outline = el("polygon", { points: pointsAttr(module.outline), class: "module" }, layer("modules"));
    const title = el("title", {}, outline);
    title.textContent = `${module.name} (${module.id}${module.owner ? ", " + module.owner : ""}), district ${module.district}`;
    text(module.id, module.label, "module-label", layer("labels"));
  }
  // The district names, above the first module of each district.
  for (const district of layout.districts) {
    const first = layout.modules.find((m) => m.district === district);
    if (first) {
      const top = Math.max(...first.outline.map((p) => p[1]));
      const left = Math.min(...first.outline.map((p) => p[0]));
      text(`District ${district}`, [left + 20, top + 60], "district-label", layer("labels"), "start").style.fill = districtColour(district);
    }
  }
  for (const platform of layout.platforms) {
    el("polygon", { points: pointsAttr(platform), class: "platform" }, layer("platforms"));
  }
  for (const crossing of layout.crossings) {
    if (crossing.road.length) el("polyline", { points: pointsAttr(crossing.road), class: "road" }, layer("roads"));
  }
  for (const building of layout.buildings) {
    const shape = el("polygon", { points: pointsAttr(building.outline), class: `building${building.lights ? " lit" : ""}` }, layer("buildings"));
    // The name shows on hover; written on the map it would cover the track.
    el("title", {}, shape).textContent = `${building.name}: ${building.lights} light(s)`;
  }
  for (const piece of layout.pieces) {
    el("polyline", { points: pointsAttr(piece.points), class: "track" }, layer("track"));
    // Shown in yellow while reserved for a train's route.
    const reserved = el("polyline", { points: pointsAttr(piece.points), class: "reserved-track" }, layer("track"));
    elements.pieces.set(piece.id, reserved);
  }
  for (const block of layout.blocks) {
    if (block.role === "transfer") {
      // A casing in the colour of the district feeding the block, with its name beside it.
      const casing = el("polyline", { points: pointsAttr(block.points), class: "feed" }, layer("blocks"));
      const [a, b] = [block.points[0] || [0, 0], block.points[block.points.length - 1] || [0, 0]];
      const middle = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
      // Beside the block, on the side away from the middle of its module (outside the
      // double track), a little down so the text sits clear of the casing.
      const module = layout.modules.find((m) => block.id.startsWith(`${m.id}/`));
      const centre = module ? module.outline.reduce((s, p) => [s[0] + p[0] / module.outline.length, s[1] + p[1] / module.outline.length], [0, 0]) : middle;
      const length = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1;
      let normal = [-(b[1] - a[1]) / length, (b[0] - a[0]) / length];
      if ((middle[0] - centre[0]) * normal[0] + (middle[1] - centre[1]) * normal[1] < 0) normal = [-normal[0], -normal[1]];
      const away = normal[1] < 0 ? 58 : 34;
      const label = text("", [middle[0] + normal[0] * away, middle[1] + normal[1] * away], "feed-label", layer("labels"));
      elements.feeds.set(block.id, { casing, label });
    }
    const line = el("polyline", { points: pointsAttr(block.points), class: `block${block.role === "transfer" ? " transfer" : ""}` }, layer("blocks"));
    el("title", {}, line).textContent = `Block ${block.id} (${block.district}${block.role ? ", " + block.role : ""})`;
    elements.blocks.set(block.id, { line, piece: block.piece });
  }
  for (const crossing of layout.crossings) {
    const group = el("g", {}, layer("crossings"));
    if (crossing.at.length) {
      // Along the road, and across it.
      let along = [0, 1];
      if (crossing.road.length >= 2) {
        const [a, b] = [crossing.road[0], crossing.road[crossing.road.length - 1]];
        const length = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1;
        along = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
      }
      const across = [-along[1], along[0]];
      const base = crossing.at[0];
      const offsets = crossing.at.map((p) => (p[0] - base[0]) * along[0] + (p[1] - base[1]) * along[1]);
      // A barrier on each side of the tracks, its arm across the road, with the warning
      // lights at the road's edge next to it.
      for (const offset of [Math.min(...offsets) - 75, Math.max(...offsets) + 75]) {
        const centre = [base[0] + along[0] * offset, base[1] + along[1] * offset];
        const end = (sign, distance) => flip([centre[0] + across[0] * distance * sign, centre[1] + across[1] * distance * sign]);
        const [x1, y1] = end(-1, 45);
        const [x2, y2] = end(1, 45);
        el("line", { x1, y1, x2, y2, class: "barrier" }, group);
        const [lx, ly] = end(1, 62);
        el("circle", { cx: lx, cy: ly, r: 12, class: "crossing" }, group);
      }
    }
    el("title", {}, group).textContent = `Level crossing ${crossing.name}`;
    elements.crossings.set(crossing.id, group);
  }
  for (const signal of layout.signals) {
    // By the track on the right of the trains it faces, the lamps in a row along the
    // track (stop nearest the train), a foot across at the far end.
    const [dx, dy] = signal.direction;
    const right = [dy, -dx];
    const at = (along, side) => flip([signal.at[0] + dx * along + right[0] * side, signal.at[1] + dy * along + right[1] * side]);
    const group = el("g", { class: "signal" }, layer("signals"));
    const count = signal.aspects.length;
    // Close by the track: between two tracks of a station there is room on the platform.
    const side = 28;
    const [hx1, hy1] = at(0, side);
    const [hx2, hy2] = at(-16 * (count - 1), side);
    el("line", { x1: hx1, y1: hy1, x2: hx2, y2: hy2, class: "signal-head" }, group);
    const [fx1, fy1] = at(-16 * (count - 1) - 14, side - 12);
    const [fx2, fy2] = at(-16 * (count - 1) - 14, side + 12);
    el("line", { x1: fx1, y1: fy1, x2: fx2, y2: fy2, class: "signal-foot" }, group);
    const lamps = new Map();
    // Red nearest the track's danger point, then yellow, then green.
    signal.aspects.forEach((aspect, index) => {
      const [x, y] = at(-16 * index, side);
      lamps.set(aspect, el("circle", { cx: x, cy: y, r: 6.5, class: "lamp" }, group));
    });
    const title = el("title", {}, group);
    title.textContent = `Signal ${signal.name} (${signal.district}, LocoNet ${signal.address})`;
    elements.signals.set(signal.id, { lamps, title, name: signal.name, district: signal.district, address: signal.address });
  }
  for (const turnout of layout.turnouts) {
    const [x, y] = flip(turnout.at);
    const dot = el("circle", { cx: x, cy: y, r: 18, class: "turnout straight" }, layer("turnouts"));
    el("title", {}, dot).textContent = `Turnout ${turnout.id}`;
    text(turnout.name, [turnout.at[0], turnout.at[1] + 30], "turnout-label", layer("labels"));
    elements.turnouts.set(turnout.id, dot);
  }
  // Station names below the front edge of their module.
  for (const station of layout.stations) {
    const module = layout.modules.find((m) => m.id === station.module);
    const bottom = module ? Math.min(...module.outline.map((p) => p[1])) : station.at[1];
    text(station.text, [station.at[0], bottom - 150], "station-label", layer("labels"));
  }

  // The whole layout, or only the modules named in the address: ?modules=timo-adorf
  // (for a screen next to one station).
  const wanted = new URLSearchParams(location.search).get("modules");
  const shown = wanted ? layout.modules.filter((m) => wanted.split(",").includes(m.id)) : [];
  let [minX, minY, maxX, maxY] = layout.bounds;
  if (shown.length) {
    const points = shown.flatMap((m) => m.outline);
    [minX, minY, maxX, maxY] = [Math.min(...points.map((p) => p[0])), Math.min(...points.map((p) => p[1])), Math.max(...points.map((p) => p[0])), Math.max(...points.map((p) => p[1]))];
  }
  const margin = 150;
  fit = { x: minX - margin, y: -maxY - margin - 100, w: maxX - minX + 2 * margin, h: maxY - minY + 2 * margin + 250 };
  view = { ...fit };
  applyView();
}

function applyView() {
  svg.setAttribute("viewBox", `${view.x} ${view.y} ${view.w} ${view.h}`);
}

function formatTime(seconds) {
  const s = Math.floor(seconds);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function update(state) {
  document.getElementById("time").textContent = formatTime(state.time_s);
  const occupied = new Set(state.occupied);
  const reserved = state.reserved || {};
  for (const [id, line] of elements.pieces) line.classList.toggle("on", id in reserved);
  for (const [id, { line, piece }] of elements.blocks) {
    line.classList.toggle("occupied", occupied.has(id));
    line.classList.toggle("reserved", !occupied.has(id) && piece in reserved);
  }
  for (const [id, { casing, label }] of elements.feeds) {
    const district = (state.feeds || {})[id];
    casing.setAttribute("stroke", district ? districtColour(district) : "none");
    label.textContent = district ? `fed by ${district}` : "";
    label.setAttribute("fill", district ? districtColour(district) : "none");
  }
  for (const [id, signal] of elements.signals) {
    const aspect = (state.signals || {})[id] || "red";
    for (const [colour, lamp] of signal.lamps) lamp.setAttribute("fill", colour === aspect ? LAMP[colour] : "var(--lamp-off)");
    signal.title.textContent = `Signal ${signal.name} (${signal.district}, LocoNet ${signal.address}): ${aspect}`;
  }
  for (const [id, dot] of elements.turnouts) {
    const straight = state.turnouts[id] !== false;
    dot.classList.toggle("straight", straight);
    dot.classList.toggle("diverging", !straight);
  }
  const closed = new Set(state.crossings_closed);
  for (const [id, group] of elements.crossings) {
    group.classList.toggle("closed-barrier", closed.has(id));
    for (const dot of group.querySelectorAll("circle")) dot.classList.toggle("closed", closed.has(id));
  }

  const trains = layer("trains");
  trains.replaceChildren();
  const rows = [];
  state.trains.forEach((train, index) => {
    const colour = TRAIN_COLOURS[index % TRAIN_COLOURS.length];
    if (train.points.length) {
      el("polyline", { points: pointsAttr(train.points), class: "train", stroke: colour }, trains);
      const [x, y] = flip(train.points[0]);
      el("circle", { cx: x, cy: y, r: 20, fill: colour, class: "train-head" }, trains);
      const label = el("text", { x, y: y - 40, class: "train-label", fill: colour, "text-anchor": "middle" }, trains);
      label.textContent = train.name;
    }
    rows.push(`<tr><td style="color:${colour}">${escapeHtml(train.name)}</td><td>${train.address}</td><td>${escapeHtml(train.district)}</td><td>${escapeHtml(train.state)}</td></tr>`);
  });
  document.querySelector("#trains tbody").innerHTML = rows.join("");

  const log = document.getElementById("log");
  log.innerHTML = (state.log || []).slice().reverse().map((line) => `<li>${escapeHtml(line)}</li>`).join("");
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}

// Zoom with the wheel around the pointer, move by dragging, fit on double-click.
svg.addEventListener("wheel", (event) => {
  event.preventDefault();
  const box = svg.getBoundingClientRect();
  const fx = (event.clientX - box.left) / box.width;
  const fy = (event.clientY - box.top) / box.height;
  const factor = event.deltaY > 0 ? 1.15 : 1 / 1.15;
  const w = view.w * factor;
  const h = view.h * factor;
  view = { x: view.x + (view.w - w) * fx, y: view.y + (view.h - h) * fy, w, h };
  applyView();
}, { passive: false });

let drag = null;
svg.addEventListener("pointerdown", (event) => {
  drag = { x: event.clientX, y: event.clientY, view: { ...view } };
  svg.setPointerCapture(event.pointerId);
  svg.classList.add("dragging");
});
svg.addEventListener("pointermove", (event) => {
  if (!drag) return;
  const box = svg.getBoundingClientRect();
  const scale = Math.max(view.w / box.width, view.h / box.height);
  view = { ...drag.view, x: drag.view.x - (event.clientX - drag.x) * scale, y: drag.view.y - (event.clientY - drag.y) * scale };
  applyView();
});
const endDrag = () => { drag = null; svg.classList.remove("dragging"); };
svg.addEventListener("pointerup", endDrag);
svg.addEventListener("pointercancel", endDrag);
svg.addEventListener("dblclick", () => { view = { ...fit }; applyView(); });

async function start() {
  const response = await fetch("api/layout");
  drawLayout(await response.json());
  const status = document.getElementById("connection");
  const source = new EventSource("events");
  source.onopen = () => { status.textContent = "live"; status.classList.add("live"); };
  source.onerror = () => { status.textContent = "reconnecting…"; status.classList.remove("live"); };
  source.onmessage = (event) => update(JSON.parse(event.data));
}

start();
