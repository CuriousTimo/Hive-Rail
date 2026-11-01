#!/usr/bin/env python3
"""Converts the example modules (now backup/examples-layout-v2/*.json, schema version 2) into tile modules
(schema version 3) in examples/layout/: each module drawn from SpDrS60 tiles on its grid
of 50 mm cells, with German signals placed and named as on the DB (Ril 301) and occupancy
sections laid out as model railroaders do for software block control.

Conventions used throughout:
- Names follow DB / FREMO practice. Kilometres rise from Adorf (west) to Bdorf (east).
  Signals: entry signals (Einfahrsignale) A in the rising direction and F in the falling one;
  exit signals (Ausfahrsignale) N (rising) and P (falling) followed by the track number;
  block signals numbered, odd rising and even falling. A main signal with the next signal's
  distant on its mast is the combined H/V tile; its "distant" names the signal it announces,
  letters in lowercase (a, f, n3/n4). A terminus entry signal has no distant: the exit
  signals behind it face the other way.
- Turnouts are numbered per station from its entry in kilometre direction: a crossover is a
  pair (1/2), a double slip's halves are a and b (4a/4b).
- Occupancy sections are named by track and kind: H a stop section (Halt) with the end it
  lies at (1 H-A), B a braking section, S the open line, T a transfer section and R an
  edge section at a district border; a station throat W with its turnouts (W 1-4). Level
  crossings are BÜ with their name; their switch-on and switch-off contacts ESK and ASK.
- Right-hand running: on double track, track 1 (eastbound, A>B) is the south track (row 6),
  track 2 (westbound, B>A) the north track (row 5); signals stand to the right of their track.
- Occupancy: a station track has three sections, a stop section at each end (ending just
  before its signal or buffer stop) and a braking section in the middle; a station throat has
  a section of its own; a passing loop track (one way) has a braking section and a stop
  section; the open line one section per track and module.
- Addresses are kept from the version 2 files; new sections get free addresses in their
  district. Some addresses are used in two or three districts on purpose, to test that
  Hive-Rail keeps each district's addresses apart (every district has its own LocoNet):
  signals 121-125 (Adorf and Bdorf), 131 (block signals 13, 14, 16), 133 (Cdorf A and
  block signal 15); sections 201/202 (the line modules) and 241-247 (Adorf and Bdorf).
  Within one district every address is unique.

Usage: backup/tools/convert_modules_v3.py   (writes examples/layout/*.json)
"""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
SRC = ROOT / "backup" / "examples-layout-v2"
OUT = ROOT / "examples" / "layout"
OUT.mkdir(parents=True, exist_ok=True)
CELL_MM = 50
CELLS = json.loads((ROOT / "assets" / "spdrs60" / "tiles.json").read_text())["tiles"]

H_MAIN = ["hp0", "hp1", "hp2"]
H_MAIN_DISTANT = ["hp0", "hp1", "hp2", "vr0", "vr1", "vr2"]


def ident(prefix, name):
    """A device id from a name, as the module editor makes them: "1 H-A" -> sensor_1_h_a."""
    return f"{prefix}_{re.sub(r'[^a-z0-9]+', '_', name.lower()).strip('_')}"


class Module:
    def __init__(self, v2_name, size_mm=None):
        self.v2 = json.loads((SRC / f"{v2_name}.json").read_text())
        self.size_mm = size_mm or [self.v2["length_mm"], 546]
        self.w = round(self.size_mm[0] / CELL_MM)
        self.h = round(self.size_mm[1] / CELL_MM)
        self.tiles = {}
        self.devices = {}
        self.blocks = []
        self.ends = []
        self.tracks = []

    # --- drawing ---
    def put(self, tile, x, y, turn=0, mirror=False, **fields):
        cells = CELLS[tile]["cells"]
        for i in range(cells):
            cx, cy = (x + i, y) if turn % 2 == 0 else (x, y + i)
            assert 0 <= cx < self.w and 0 <= cy < self.h, (self.v2["module_id"], tile, cx, cy)
            # A tile replaces what it covers.
            for key in [k for k, t in self.tiles.items() if (cx, cy) in t["_cells"]]:
                del self.tiles[key]
        t = {"tile": tile, "x": x, "y": y, "turn": turn, "mirror": mirror}
        t.update({k: v for k, v in fields.items() if v is not None})
        t["_cells"] = {((x + i, y) if turn % 2 == 0 else (x, y + i)) for i in range(cells)}
        self.tiles[(x, y)] = t
        return t

    def run(self, y, x0, x1, track, block=None, vertical=False):
        """Straight track from x0 to x1 (inclusive), on row y (or column y when vertical)."""
        for x in range(min(x0, x1), max(x0, x1) + 1):
            if vertical:
                self.put("track-straight", y, x, 1, track=track, block=block)
            else:
                self.put("track-straight", x, y, track=track, block=block)

    # --- devices ---
    def device(self, device_id, type_, address, **extra):
        self.devices[device_id] = {"id": device_id, "type": type_, "bus": "loconet", "address": address, **extra}
        return device_id

    def signal(self, tile, x, y, turn, name, address, track, block=None, options=None, aspects=None, distant=None):
        device = self.device(ident("signal", name), "signal", address,
                             aspects=aspects or (H_MAIN_DISTANT if tile == "signal-hv-main-distant" else H_MAIN))
        return self.put(tile, x, y, turn, id=name, track=track, block=block, device=device, options=options,
                        distant=distant)

    def sensor(self, x, y, name, address, track, block, turn=0, type_="occupancy"):
        device = self.device(ident("sensor", name), type_, address)
        return self.put("sensor", x, y, turn, id=name, track=track, block=block, device=device)

    def turnout(self, tile, x, y, turn, mirror, name, address, track=None, block=None):
        device = self.device(ident("point", name), "turnout", address)
        return self.put(tile, x, y, turn, mirror, id=name, track=track, block=block, device=device)

    # --- writing ---
    def write(self, **extra):
        v2 = self.v2
        tiles = sorted(self.tiles.values(), key=lambda t: (t["y"], t["x"]))
        for t in tiles:
            del t["_cells"]
        used = {t.get("device") for t in tiles}
        module = {
            "schema_version": 3,
            "module_id": v2["module_id"],
            "friendly_name": v2["friendly_name"],
            "owner": v2.get("owner"),
            "scale": v2.get("scale", "H0"),
            "size_mm": self.size_mm,
            "ends": self.ends,
            "tracks": self.tracks,
            "tiles": tiles,
            "stations": extra.get("stations", []),
            "passing_loops": extra.get("passing_loops", []),
            "blocks": self.blocks,
            "crossings": extra.get("crossings", []),
            "devices": [d for d in self.devices.values() if d["id"] in used or d["id"] in extra.get("keep", [])],
        }
        (OUT / f"{v2['module_id']}.json").write_text(json.dumps(module, indent=1, ensure_ascii=False) + "\n")
        return module


def ends_2e96(m, left="A", right="B", left_tracks=("2", "1"), right_tracks=("2", "1")):
    """The usual line module ends: A at the left, B at the right, tracks top to bottom."""
    m.ends = [
        {"id": left, "side": "left", "profile": "2E96", "tracks": list(left_tracks)},
        {"id": right, "side": "right", "profile": "2E96", "tracks": list(right_tracks)},
    ]
    m.tracks = [{"name": "1", "direction": "A>B"}, {"name": "2", "direction": "B>A"}]


# --- Adorf: terminus at the west end; the line leaves at B (right) ---------------------
def adorf_tiles(m, names, addr):
    """Adorf's track plan, as drawn for Adorf (line at the right, buffers at the left).
    Rows: 5 track 1 / in, 6 track 2 / out, 7 track 3. Platforms lie on the track tiles: the
    house platform above track 1, the island platform between tracks 2 and 3 (track 2's
    lower half and track 3's upper half). Bdorf is this turned 180 degrees."""
    W = m.w  # 36
    # Line tracks at the right: 'in' on row 5 (westbound arrivals), 'out' on row 6.
    m.run(5, 32, W - 1, "in", block="throat")
    m.run(6, 32, W - 1, "out", block="throat")
    # The entry signal (westbound into Adorf): a terminus entry, so no distant on its mast.
    m.signal("signal-hv-main", 33, 5, 2, names["entry"], addr["entry"], "in", block="throat")
    # Turnouts numbered from the entry: the crossover 1/2 (in to out, for departures from
    # track 1), 3 to track 1 and down to the double slip 4a/4b, which leads to tracks 2 and 3.
    m.turnout("turnout-right", 31, 5, 0, False, "1", addr["t1"], block="throat")
    m.turnout("turnout-right", 31, 6, 2, False, "2", addr["t2"], block="throat")
    m.run(5, 27, 30, "a1", block="throat")
    m.run(6, 27, 30, "b1", block="throat")
    m.turnout("turnout-left", 26, 5, 2, False, "3", addr["t3"], block="throat")
    device = m.device(ident("point", "4"), "turnout", addr["t4"], second_address=addr["t4"] + 1)
    m.put("double-slip", 25, 6, 0, False, id="4", block="throat", device=device)
    m.put("track-curve", 25, 7, 1, False, track="3", block="throat")
    # Platform tracks, from their buffer stops (west) to the throat (east), each in three
    # occupancy sections drawn on their tiles: a short stop section (H) at each platform
    # end, ending at the buffer stop or the exit signal, and the braking section (B) in
    # between. A train coming in stops at the buffer stop, in the west stop section.
    station = names["station"].lower()
    sections = (("w", 2, 5, 3, "stop", "B>A", f"H-{names['west_end']}"),
                ("b", 6, 17, 11, "brake", None, "B"),
                ("e", 18, None, 21, "stop", "A>B", f"H-{names['east_end']}"))
    for row, track, east_end, exit_x in ((5, "1", 25, 24), (6, "2", 24, 23), (7, "3", 24, 23)):
        for key, x0, x1, sensor_x, role, direction, suffix in sections:
            block = f"{station}_{track}_{key}"
            m.run(row, x0, x1 if x1 is not None else east_end, track, block=block)
            name = f"{track} {suffix}"
            m.sensor(sensor_x, row, name, addr[f"occ{track}{'' if key == 'b' else key}"], track, block)
            m.blocks.append({"id": block, "track": track, "detected_rail": "outer",
                             "sections": [{"device": ident("sensor", name), "role": role,
                                           **({"for": direction} if direction else {})}]})
        m.put("buffer-stop", 1, row, 2, False, track=track, block=f"{station}_{track}_w")
        # The exit signal at the platform's east end, with Sh 1 for shunting.
        m.signal("signal-hv-main", exit_x, row, 0, f"{names['exit']}{track}", addr[f"exit{track}"], track,
                 block=f"{station}_{track}_e", options=["sh-lamp"])
    # The throat: one section of its own.
    m.sensor(29, 6, "W 1-4", addr["throat"], "b1", "throat")
    m.blocks.append({"id": "throat", "sections": [{"device": ident("sensor", "W 1-4"), "role": "throat"}], "detected_rail": "outer"})
    # Platforms up to the buffer stops: the house platform above track 1, the island
    # platform between tracks 2 and 3.
    for row, sides, x0, x1 in ((5, ["platform-top"], 2, 21), (6, ["platform-bottom"], 2, 20), (7, ["platform-top"], 2, 20)):
        for x in range(x0, x1 + 1):
            t = m.tiles.get((x, row))
            if t and t["tile"] in ("track-straight", "sensor"):
                t["options"] = sides
    # Track numbers beside the buffer stops, the station's name below.
    for row, track in ((5, "1"), (6, "2"), (7, "3")):
        m.put("plate", 0, row, text=track)
    m.put("plate", 13, 9, text=names["station"])


def rotate_180(m):
    """Turns a drawn module half round (Bdorf from Adorf), keeping double track rows: a tile
    at row y goes to row 11 - y, so rows 5 and 6 swap, and the left end becomes the right."""
    tiles = list(m.tiles.values())
    m.tiles = {}
    for t in tiles:
        cells = CELLS[t["tile"]]["cells"]
        x = m.w - t["x"] - (cells if t["turn"] % 2 == 0 else 1)
        y = 11 - t["y"] - (0 if t["turn"] % 2 == 0 else cells - 1)
        fields = {k: v for k, v in t.items() if k not in ("tile", "x", "y", "turn", "mirror", "_cells")}
        # A name plate keeps reading upright.
        turn = t["turn"] if t["tile"] == "plate" else (t["turn"] + 2) % 4
        m.put(t["tile"], x, y, turn, t["mirror"], **fields)


def adorf():
    m = Module("timo-adorf")
    addr = {"entry": 125, "t1": 11, "t2": 12, "t3": 21, "t4": 22, "exit1": 121, "exit2": 122, "exit3": 123,
            # Kept: 203 (track 1), 205 (track 2), 204 (track 3) as the braking sections.
            "occ1": 203, "occ2": 205, "occ3": 204,
            "occ1w": 241, "occ1e": 242, "occ2w": 243, "occ2e": 244, "occ3w": 245, "occ3e": 246, "throat": 247}
    adorf_tiles(m, {"station": "Adorf", "entry": "F", "exit": "N", "west_end": "A", "east_end": "B"}, addr)
    # A terminus: every platform track ends at a buffer stop; the line leaves at B.
    m.ends = [{"id": "B", "side": "right", "profile": "2E96", "tracks": ["in", "out"]}]
    m.tracks = [{"name": "in", "direction": "B>A"}, {"name": "out", "direction": "A>B"}]
    return m.write(stations=[{"name": "Adorf", "code": "Ad", "tracks": [
        {"name": "1", "track": "1", "platform": True}, {"name": "2", "track": "2", "platform": True},
        {"name": "3", "track": "3", "platform": True}]}])


def bdorf():
    m = Module("anna-bdorf")
    addr = {"entry": 125, "t1": 13, "t2": 14, "t3": 23, "t4": 24, "exit1": 121, "exit2": 122, "exit3": 123,
            "occ1": 205, "occ2": 206, "occ3": 207,
            "occ1w": 241, "occ1e": 242, "occ2w": 243, "occ2e": 244, "occ3w": 245, "occ3e": 246, "throat": 247}
    # Drawn as Adorf and turned round: what is drawn at the west end ends up at end B.
    adorf_tiles(m, {"station": "Bdorf", "entry": "A", "exit": "P", "west_end": "B", "east_end": "A"}, addr)
    rotate_180(m)
    # Turned round, the stop sections swap ends: the one drawn west is now east.
    for block in m.blocks:
        for section in block["sections"]:
            if "for" in section:
                section["for"] = {"B>A": "A>B", "A>B": "B>A"}[section["for"]]
    m.ends = [{"id": "A", "side": "left", "profile": "2E96", "tracks": ["out", "in"]}]
    m.tracks = [{"name": "in", "direction": "A>B"}, {"name": "out", "direction": "B>A"}]
    return m.write(stations=[{"name": "Bdorf", "code": "Bd", "tracks": [
        {"name": "1", "track": "1", "platform": True}, {"name": "2", "track": "2", "platform": True},
        {"name": "3", "track": "3", "platform": True}]}])


# --- Cdorf: a passing loop over two modules ----------------------------------------------
def cdorf_west():
    """West half: double track at A (left); at B (right) the four loop tracks, top to
    bottom 4, 2, 1, 3 (station tracks 1 to 4). Eastbound: entry A on track 1, C1 into the
    siding 3. Westbound: exits P2 (track 2) and P1 (siding 4) before C2."""
    m = Module("timo-cdorf-west")
    W = m.w  # 24
    m.run(6, 0, 5, "1a")
    m.signal("signal-hv-main-distant", 2, 6, 0, "A", 133, "1a", distant="n3/n4")
    m.turnout("turnout-right", 6, 6, 0, False, "1", 31)
    m.run(6, 7, W - 1, "1", block="cw_1")
    m.put("track-curve", 6, 7, 1, True, track="3", block="cw_3")
    m.run(7, 7, W - 1, "3", block="cw_3")
    m.run(5, 0, 5, "2a")
    m.turnout("turnout-left", 6, 5, 0, False, "2", 32)
    m.run(5, 7, W - 1, "2", block="cw_2")
    m.put("track-curve", 6, 4, 3, False, track="4", block="cw_4")
    m.run(4, 7, W - 1, "4", block="cw_4")
    # Westbound exits, just past the turnouts' points.
    m.signal("signal-hv-main", 8, 5, 2, "P2", 137, "2", block="cw_2")
    m.signal("signal-hv-main", 8, 4, 2, "P1", 138, "4", block="cw_4")
    # Occupancy: per loop track a section on each half; here the eastbound braking sections
    # (tracks 1 and 3) and the westbound stop sections (tracks 2 and 4).
    # Sections are named by Cdorf's station track numbers (1 to 4, top to bottom).
    for row, track, block, name, address, role, direction in (
            (6, "1", "cw_1", "3 B", 221, "brake", "A>B"), (7, "3", "cw_3", "4 B", 222, "brake", "A>B"),
            (5, "2", "cw_2", "2 H-A", 223, "stop", "B>A"), (4, "4", "cw_4", "1 H-A", 224, "stop", "B>A")):
        m.sensor(15, row, name, address, track, block)
        m.blocks.append({"id": block, "track": track, "detected_rail": "outer",
                         "sections": [{"device": ident("sensor", name), "role": role, "for": direction}]})
    m.ends = [{"id": "A", "side": "left", "profile": "2E96", "tracks": ["2a", "1a"]},
              {"id": "B", "side": "right", "profile": "CDORF4", "tracks": ["4", "2", "1", "3"]}]
    m.tracks = [{"name": n, "direction": d} for n, d in
                (("1a", "A>B"), ("1", "A>B"), ("3", "A>B"), ("2", "B>A"), ("4", "B>A"), ("2a", "B>A"))]
    return m.write(passing_loops=cdorf_loop())


def cdorf_east():
    """East half: the west half mirrored left to right. Eastbound exits N3 (track 1) and N4
    (siding 3) before C3; westbound entry F on track 2, C4 into the siding 4."""
    m = Module("timo-cdorf-east")
    W = m.w
    m.run(6, 0, W - 7, "1", block="ce_1")
    m.put("track-curve", W - 7, 7, 1, False, track="3", block="ce_3")
    m.run(7, 0, W - 8, "3", block="ce_3")
    m.turnout("turnout-right", W - 7, 6, 2, True, "3", 33)
    m.run(6, W - 6, W - 1, "1b")
    m.run(5, 0, W - 8, "2", block="ce_2")
    m.turnout("turnout-left", W - 7, 5, 2, True, "4", 34)
    m.put("track-curve", W - 7, 4, 0, False, track="4", block="ce_4")
    m.run(4, 0, W - 8, "4", block="ce_4")
    m.run(5, W - 6, W - 1, "2b")
    m.signal("signal-hv-main-distant", W - 4, 5, 2, "F", 135, "2b", distant="p1/p2")
    m.signal("signal-hv-main", W - 9, 6, 0, "N3", 139, "1", block="ce_1")
    m.signal("signal-hv-main", W - 9, 7, 0, "N4", 140, "3", block="ce_3")
    for row, track, block, name, address, role, direction in (
            (6, "1", "ce_1", "3 H-B", 225, "stop", "A>B"), (7, "3", "ce_3", "4 H-B", 226, "stop", "A>B"),
            (5, "2", "ce_2", "2 B", 227, "brake", "B>A"), (4, "4", "ce_4", "1 B", 228, "brake", "B>A")):
        m.sensor(8, row, name, address, track, block)
        m.blocks.append({"id": block, "track": track, "detected_rail": "outer",
                         "sections": [{"device": ident("sensor", name), "role": role, "for": direction}]})
    m.ends = [{"id": "A", "side": "left", "profile": "CDORF4", "tracks": ["4", "2", "1", "3"]},
              {"id": "B", "side": "right", "profile": "2E96", "tracks": ["2b", "1b"]}]
    m.tracks = [{"name": n, "direction": d} for n, d in
                (("1", "A>B"), ("3", "A>B"), ("1b", "A>B"), ("2b", "B>A"), ("2", "B>A"), ("4", "B>A"))]
    return m.write(passing_loops=cdorf_loop())


def cdorf_loop():
    return [{"name": "Cdorf", "code": "Cd", "tracks": [
        {"name": "1", "track": "4", "passing": True}, {"name": "2", "track": "2"},
        {"name": "3", "track": "1"}, {"name": "4", "track": "3", "passing": True}]}]


# --- line modules -------------------------------------------------------------------------
def line_module(v2_name, signals=(), sensors=(), borders=(), crossing=None, blocks=()):
    """A straight double track module: track 2 (westbound) on row 5, track 1 (eastbound) on
    row 6. signals: (name, x, track, address, distant); sensors: (name, x, track, address, type);
    borders: (track, x, relay address); crossing: (name, x, lights, barriers, sensors);
    blocks: (id, track, sensor name, role, direction)."""
    m = Module(v2_name)
    W = m.w
    block_of = {track: block for block, track, *_ in blocks}
    m.run(5, 0, W - 1, "2", block=block_of.get("2"))
    m.run(6, 0, W - 1, "1", block=block_of.get("1"))
    for name, x, track, address, distant in signals:
        eastbound = track == "1"
        m.signal("signal-hv-main-distant", x, 6 if eastbound else 5, 0 if eastbound else 2, name, address, track,
                 block=block_of.get(track), distant=distant)
    for name, x, track, address, type_ in sensors:
        m.sensor(x, 6 if track == "1" else 5, name, address, track, block_of.get(track), type_=type_)
    for track, x, relay in borders:
        device = m.device(f"relay_t{track}", "relay", relay)
        m.put("district-border", x, 6 if track == "1" else 5, 0, track=track, block=block_of.get(track), device=device)
    crossings = []
    if crossing:
        name, x, lights, barriers, close_open = crossing
        device = m.device("crossing_lc", "crossing", lights, barriers=barriers)
        for row, track in ((5, "2"), (6, "1")):
            m.put("level-crossing", x, row, 0, id=f"BÜ {name}", track=track, block=block_of.get(track), device=device)
        crossings = [{"id": "lc", "name": name, "device": device,
                      "close": [ident("sensor", n) for n in close_open["close"]],
                      "open": [ident("sensor", n) for n in close_open["open"]]}]
    for block, track, sensor, role, direction in blocks:
        # A transfer section is cut in both rails: either district may feed it.
        m.blocks.append({"id": block, "track": track, "detected_rail": "both" if role == "transfer" else "outer",
                         "sections": [{"device": ident("sensor", sensor), **({"role": role} if role else {}),
                                       **({"for": direction} if direction else {})}]})
    ends_2e96(m)
    return m.write(crossings=crossings)


def line_modules():
    # Adorf's line module: block signals 11 (eastbound, announcing Cdorf's entry A) and 12
    # (westbound, announcing Adorf's entry F); the level crossing Feldweg.
    line_module("timo-2e96-west",
                signals=(("11", 15, "1", 127, "a"), ("12", 1, "2", 129, "f")),
                sensors=(("1 S", 4, "1", 201, "occupancy"), ("2 S", 13, "2", 202, "occupancy"),
                         ("ESK 1", 2, "1", 211, "sensor"), ("ASK 1", 12, "1", 212, "sensor"),
                         ("ESK 2", 16, "2", 213, "sensor"), ("ASK 2", 6, "2", 214, "sensor")),
                crossing=("Feldweg", 9, 111, 112, {"close": ["ESK 1", "ESK 2"], "open": ["ASK 1", "ASK 2"]}),
                blocks=(("line_1", "1", "1 S", None, None), ("line_2", "2", "2 S", None, None)))
    # District borders: transfer sections (T) with their relays, edge sections (R), block
    # signals at the borders, and the handover detectors (IR).
    line_module("timo-2e96-border",
                signals=(("13", 15, "1", 131, "15"),),
                sensors=(("1 R", 4, "1", 206, "occupancy"), ("2 T", 4, "2", 207, "occupancy"),
                         ("IR 2", 16, "2", 17, "sensor")),
                borders=(("2", 9, 27),),
                blocks=(("edge_1", "1", "1 R", "edge", "A>B"), ("t2", "2", "2 T", "transfer", "B>A")))
    line_module("m5-2e96-in",
                signals=(("14", 1, "2", 131, "f"),),
                sensors=(("1 T", 4, "1", 201, "occupancy"), ("2 R", 13, "2", 202, "occupancy"),
                         ("IR 1", 1, "1", 17, "sensor")),
                borders=(("1", 9, 27),),
                blocks=(("t1", "1", "1 T", "transfer", "A>B"), ("edge_2", "2", "2 R", "edge", "B>A")))
    line_module("m5-2e96-out",
                signals=(("15", 15, "1", 133, "17"),),
                sensors=(("1 R", 4, "1", 206, "occupancy"), ("2 T", 4, "2", 207, "occupancy"),
                         ("IR 2", 16, "2", 18, "sensor")),
                borders=(("2", 9, 28),),
                blocks=(("edge_1", "1", "1 R", "edge", "A>B"), ("t2", "2", "2 T", "transfer", "B>A")))
    line_module("anna-2e96-border",
                signals=(("16", 1, "2", 131, "14"),),
                sensors=(("1 T", 4, "1", 201, "occupancy"), ("2 R", 13, "2", 202, "occupancy"),
                         ("IR 1", 1, "1", 17, "sensor")),
                borders=(("1", 9, 27),),
                blocks=(("t1", "1", "1 T", "transfer", "A>B"), ("edge_2", "2", "2 R", "edge", "B>A")))


def anna_curve():
    """anna-2e96-east: a 90 degree curve to the right, 873 x 873 mm (17 x 17 cells). The
    double track comes in at A (left, rows 5 and 6) and leaves at B (bottom, columns 10
    and 9: looking out of B, track 2 is on the left). One curve tile per track."""
    m = Module("anna-2e96-east", size_mm=[873, 873])
    H = m.h  # 17
    m.run(5, 0, 9, "2", block="line_2")
    m.put("track-curve", 10, 5, 0, False, track="2", block="line_2")
    m.run(10, 6, H - 1, "2", block="line_2", vertical=True)
    m.run(6, 0, 8, "1", block="line_1")
    m.put("track-curve", 9, 6, 0, False, track="1", block="line_1")
    m.run(9, 7, H - 1, "1", block="line_1", vertical=True)
    # Block signals: 17 eastbound (now heading south) near B, announcing Bdorf's entry A;
    # 18 westbound near A, announcing 16.
    m.signal("signal-hv-main-distant", 9, 14, 1, "17", 127, "1", block="line_1", distant="a")
    m.signal("signal-hv-main-distant", 1, 5, 2, "18", 129, "2", block="line_2", distant="16")
    device = m.device("crossing_lc", "crossing", 111, barriers=112)
    for x, track in ((9, "1"), (10, "2")):
        m.put("level-crossing", x, 11, 1, id="BÜ Dorfstraße", track=track, block=f"line_{track}", device=device)
    for name, x, y, track, address, type_ in (("1 S", 9, 9, "1", 203, "occupancy"), ("2 S", 4, 5, "2", 204, "occupancy"),
                                              ("ESK 1", 9, 8, "1", 211, "sensor"), ("ASK 1", 9, 13, "1", 212, "sensor"),
                                              ("ESK 2", 10, 15, "2", 213, "sensor"), ("ASK 2", 10, 9, "2", 214, "sensor")):
        m.sensor(x, y, name, address, track, f"line_{track}", turn=1 if x in (9, 10) and y > 6 else 0, type_=type_)
    m.blocks = [{"id": "line_1", "track": "1", "detected_rail": "outer", "sections": [{"device": ident("sensor", "1 S"), "role": "entry", "for": "A>B"}]},
                {"id": "line_2", "track": "2", "detected_rail": "outer", "sections": [{"device": ident("sensor", "2 S")}]}]
    m.ends = [{"id": "A", "side": "left", "profile": "2E96", "tracks": ["2", "1"]},
              {"id": "B", "side": "bottom", "profile": "2E96", "tracks": ["1", "2"]}]
    m.tracks = [{"name": "1", "direction": "A>B"}, {"name": "2", "direction": "B>A"}]
    return m.write(crossings=[{"id": "lc", "name": "Dorfstraße", "device": device,
                               "close": [ident("sensor", "ESK 1"), ident("sensor", "ESK 2")],
                               "open": [ident("sensor", "ASK 1"), ident("sensor", "ASK 2")]}])


if __name__ == "__main__":
    adorf()
    bdorf()
    cdorf_west()
    cdorf_east()
    line_modules()
    anna_curve()
    print(f"{len(list(OUT.glob('*.json')))} modules in {OUT}")
