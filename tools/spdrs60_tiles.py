#!/usr/bin/env python3
"""Draws Hive-Rail's own SpDrS60-style panel tiles (assets/spdrs60/*.svg) and a preview page.

SpDrS60 is the Siemens relay interlocking of the Deutsche Bundesbahn; its control panel
(Stelltisch) is a grid of tiles on a light grey panel: thin black track
with a short lamp in the middle of each tile, signals lying
along the track beside it, white name plates. These are Hive-Rail's own drawings of that
design, made from the prototype (photos and descriptions), not from any other program's files.

Every tile is a square of 32 x 32 units, as in Rocrail's editor, the track along its middle
(y = 16), and is drawn once. Its
state is a class on the <svg> root, so one file serves every state (the page that inlines
it sets the class):
  route | occupied      the track lamps: yellow (route set) or red (occupied); none: dark
  straight | diverging  a turnout's position: lights that leg and its position lamp
  hp0 hp1 hp2 ks1 ks2   a signal's aspect by its Ril 301 name; the signal tiles show the real
  vr0 vr1 vr2           signal (tools/signal_faces.py) small, lying along the track, with its
  sh0 sh1 zs1           lamps lit as on the track
  sh-lamp zs1-lamp extra-lamp  which optional lamps a signal has (Sh 1, Zs 1, Zusatzlicht)
  locked                a signal's locked lamp at its foot (yellow)
  closed                a level crossing is closed
Mirror or rotate a tile with a transform for the other hand or direction.

Usage: tools/spdrs60_tiles.py   (writes assets/spdrs60/ and web/spdrs60-tiles.js)
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import signal_faces  # noqa: E402  the real signals, drawn small on the signal tiles

OUT = Path(__file__).resolve().parent.parent / "assets" / "spdrs60"
OUT.mkdir(parents=True, exist_ok=True)

W, H = 32, 32  # a square tile, as in Rocrail's editor; the track runs along y = 16
# One cell is 50 mm of the real module (H0: about the 46 mm between the tracks of a double
# track line, so parallel tracks lie one cell apart); a module's grid is its real size
# divided by this, rounded to the nearest cell.
CELL_MM = 50

STYLE = """
  .tile { fill: #c8c8c8; stroke: #a4a4a4; stroke-width: 0.4; }
  .bar { fill: none; stroke: #141414; stroke-width: 5; stroke-linecap: butt; stroke-linejoin: miter; }
  .band { fill: #141414; }
  .lamp { fill: none; stroke: #6f6f6f; stroke-width: 2; stroke-linecap: butt; stroke-linejoin: round; }
  .route .lamp { stroke: #f0d24a; }
  .occupied .lamp { stroke: #e2271c; }
  .straight .lamp.diverging, .diverging .lamp.straight { stroke: #6f6f6f; }
  .pos { fill: none; stroke: #8a7a2a; stroke-width: 0.8; }
  .straight .pos.straight, .diverging .pos.diverging { fill: #f0d24a; stroke: #6b5a10; }
  .occupied.straight .pos.straight, .occupied.diverging .pos.diverging { fill: #e2271c; stroke: #6b1410; }
  .point { fill: #141414; stroke: #e8e8e8; stroke-width: 1.1; }
  .head { fill: #141414; }
  .mast { fill: none; stroke: #141414; stroke-width: 1.3; }
  .foot { fill: none; stroke: #8a7a2a; stroke-width: 0.8; }
  .locked .foot { fill: #f0d24a; }
  .road { fill: #707070; }
  .lc-lamp { fill: none; stroke: #8a7a2a; stroke-width: 0.8; }
  .closed .lc-lamp { fill: #f0d24a; }
  .joint { stroke: #c8c8c8; stroke-width: 1.2; }
  .district-a { fill: var(--district-a, #20c5b5); }
  .district-b { fill: var(--district-b, #a77bff); }
  .pf { display: none; fill: #e9e6dc; }
  .pf-edge { display: none; stroke: #f5f1e6; stroke-width: 1.1; stroke-dasharray: 3 2; }
  .platform-top .pf.pf-top, .platform-bottom .pf.pf-bottom,
  .platform-top .pf-edge.pf-top, .platform-bottom .pf-edge.pf-bottom { display: inline; }
  .plate { fill: #f6f6f6; stroke: #8e8e8e; stroke-width: 0.4; }
  .plate-text { fill: #141414; font: 7.5px sans-serif; text-anchor: middle; }
  .number-box { fill: #111111; stroke: #3c3c3c; stroke-width: 0.8; }
  .number { fill: #ff3b2f; font: bold 7.5px monospace; text-anchor: middle; }
  .sensor-free { fill: #34d058; stroke: #141414; stroke-width: 0.9; }
  .sensor-on { display: none; }
  .occupied .sensor-free { display: none; }
  .occupied .sensor-on { display: inline; }
  .sensor-glow { fill: #c80000; stroke: #141414; stroke-width: 0.9; }
  .sensor-core { fill: #ff2a1a; }
  .sensor-hot { fill: #ff8a00; }
"""
STYLE += signal_faces.STYLE

BODIES = {}

# The tiles' own parts get an "sp-" prefix, so a page's styles (such as the layout view's
# .road and .lamp) never reach them; the state classes a page sets stay plain.
PARTS = ["pf", "pf-edge", "pf-top", "pf-bottom", "tile", "bar", "band", "lamp", "pos", "point", "head", "mast", "foot", "road", "lc-lamp", "joint", "district-a", "district-b", "plate", "sensor-free",
         "sensor-on", "sensor-glow", "sensor-core", "sensor-hot",
         "plate-text", "number-box", "number", "option", "white", "zs1-triangle"]


def prefixed(text):
    """Prefixes the parts in style selectors (.lamp) and class attributes (class="lamp x")."""
    for part in sorted(PARTS, key=len, reverse=True):
        text = re.sub(rf"\.{re.escape(part)}(?![\w-])", f".sp-{part}", text)
    def attribute(match):
        return 'class="' + " ".join(f"sp-{c}" if c in PARTS else c for c in match[1].split()) + '"'
    return re.sub(r'class="([^"]*)"', attribute, text)


def tile(name, title, body, classes="", cells=1):
    """A tile `cells` wide (crossings take two), its body drawn for running right."""
    body = prefixed(body)
    BODIES[name] = (title, body, cells)
    w = W * cells
    grid = "".join(f'<rect class="tile" x="{W * i}" y="0" width="{W}" height="{H}"/>' for i in range(cells))
    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {H}" width="{w}" height="{H}" class="{classes}">
  <title>{title}</title>
  <style>{prefixed(STYLE)}  </style>
  {prefixed(grid)}
{body}
</svg>
"""
    (OUT / f"{name}.svg").write_text(svg)
    return name, title


# The joining rule, as in Rocrail's editor: track meets a tile edge only at the middle of the
# edge, always over the same span (PORT, 13.5 to 18.5 of 32), so any two tiles placed next
# to each other join flush. A diagonal is a band whose ends are cut along the tile edges
# over exactly that span: a 45-degree curve from the middle of the left edge to the middle
# of the bottom edge meets a straight track flush, and two diagonals in a row continue
# without a step. A turnout is a straight track with that curve; a crossing or double slip
# is two tiles wide. Signals are drawn for running right; the editor rotates and mirrors.
LO, HI = 13.5, 18.5  # PORT: where track crosses a tile edge


def band(x0, y0, x1, y1):
    """A 45-degree track band from the port on one edge to the port on another: (x0, y0)
    and (x1, y1) are the middles of the two edges, given as a horizontal or vertical edge
    by which coordinate is 0 or the tile size."""
    def ends(x, y):
        # The port's two corners along its edge.
        if x in (0, 32, 64):
            return [(x, y - 2.5), (x, y + 2.5)]
        return [(x - 2.5, y), (x + 2.5, y)]
    a, b = ends(x0, y0), ends(x1, y1)
    # Join the corners so both sides are 45-degree lines.
    diagonal = lambda p, q: abs(p[0] - q[0]) == abs(p[1] - q[1])
    if diagonal(a[0], b[0]) and diagonal(a[1], b[1]):
        points = [a[0], b[0], b[1], a[1]]
    else:
        points = [a[0], b[1], b[0], a[1]]
    return '<polygon class="band" points="' + " ".join(f"{x:g},{y:g}" for x, y in points) + '"/>'


CURVE_DOWN = band(0, 16, 16, 32)  # left edge to bottom edge
CURVE_UP = band(0, 16, 16, 0)  # left edge to top edge
def signal_tile(name, face, cells, title):
    """A signal as it stands on the track, drawn small and lying along the track below it
    (the right side for trains running right): its mast at the left with the locked lamp at
    the foot, its screen towards the right, lamps where the real signal has them. (The real
    panel's push buttons are left out: Hive-Rail's panel does not set routes by buttons.)"""
    info = signal_faces.FACES[face]
    w, h = info["width"], info["height"]
    length = cells * W - 7  # along the track
    scale = min(12.4 / w, length / h)
    top = 19.4
    end = 3.5 + h * scale  # the screen's top, towards the right
    mast_y = top + w / 2 * scale
    body = f"""  <path class="bar" d="M0 16 H{cells * W}"/>
  <path class="lamp" d="M5 16 H{cells * W - 5}"/>
  <circle class="foot" cx="2.2" cy="{mast_y:.2f}" r="1.5"/>
  <g class="face" transform="translate({end:.2f} {top}) rotate(90) scale({scale:.4f})">
{info["body"]}
  </g>"""
    return tile(name, f"{title}: {info['title']}", body, cells=cells)


# A platform (Bahnsteig) along a straight track: bands from the track to the tile's top and
# bottom edges, shown with the classes platform-top and platform-bottom, so platforms of
# neighbouring rows meet: two tracks a row apart with their facing halves form an island.
PLATFORM = """  <rect class="pf pf-top" x="0" y="0" width="32" height="10.5"/>
  <rect class="pf pf-bottom" x="0" y="21.5" width="32" height="10.5"/>
  <path class="pf-edge pf-top" d="M0 9.5 H32"/>
  <path class="pf-edge pf-bottom" d="M0 22.5 H32"/>
"""

TILES = [
    tile("track-straight", "Track, straight; with platform above, below or both (platform-top, platform-bottom)",
         PLATFORM + """  <path class="bar" d="M0 16 H32"/>
  <path class="lamp" d="M9 16 H23"/>"""),
    tile("track-curve", "Track, 45-degree curve: from the middle of the left edge to the middle of the bottom edge",
         f"""  {CURVE_DOWN}
  <path class="lamp" d="M4.5 20.5 L11.5 27.5"/>"""),
    tile("turnout-right", "Turnout, right hand (Weiche rechts): trunk at the left, straight to the right, diverging down (to the right when running right); the point at the left",
         f"""  <path class="bar" d="M0 16 H32"/>
  {CURVE_DOWN}
  <path class="lamp straight" d="M10 16 H25"/>
  <path class="lamp diverging" d="M7 22.5 L12 27.5"/>
  <circle class="pos straight" cx="24" cy="9" r="1.8"/>
  <circle class="pos diverging" cx="22" cy="26" r="1.8"/>
  <circle class="point" cx="5" cy="16" r="3.2"/>""", "straight"),
    tile("turnout-left", "Turnout, left hand (Weiche links): trunk at the left, straight to the right, diverging up (to the left when running right); the point at the left",
         f"""  <path class="bar" d="M0 16 H32"/>
  {CURVE_UP}
  <path class="lamp straight" d="M10 16 H25"/>
  <path class="lamp diverging" d="M7 9.5 L12 4.5"/>
  <circle class="pos straight" cx="24" cy="23" r="1.8"/>
  <circle class="pos diverging" cx="22" cy="6" r="1.8"/>
  <circle class="point" cx="5" cy="16" r="3.2"/>""", "straight"),
    tile("crossing", "Crossing (Kreuzung), two tiles wide: straight track and a 45-degree track from the top of the right tile to the bottom of the left tile",
         f"""  <path class="bar" d="M0 16 H64"/>
  {band(48, 0, 16, 32)}
  <path class="lamp" d="M6 16 H22 M42 16 H58"/>
  <path class="lamp" d="M45 3 L39 9 M25 23 L19 29"/>""", cells=2),
    tile("double-slip", "Double slip (Doppelkreuzungsweiche), two tiles wide: a crossing with both slip curves; the point in the middle",
         f"""  <path class="bar" d="M0 16 H64"/>
  {band(48, 0, 16, 32)}
  <path class="lamp straight" d="M5 16 H21 M43 16 H59"/>
  <path class="lamp straight" d="M46 2 L40 8 M24 24 L18 30"/>
  <path class="lamp diverging" d="M5 12.5 Q22 12.5 36 6.5 M28 25.5 Q42 19.5 59 19.5"/>
  <circle class="pos straight" cx="52" cy="26" r="1.8"/>
  <circle class="pos diverging" cx="12" cy="6" r="1.8"/>
  <circle class="point" cx="32" cy="16" r="3.4"/>""", "straight", cells=2),
    tile("buffer-stop", "Buffer stop (Prellbock) at the right end",
         """  <path class="bar" d="M0 16 H25"/>
  <rect class="head" x="25" y="9.5" width="3.5" height="13"/>
  <path class="lamp" d="M7 16 H20"/>"""),
    tile("sensor", "Track with an occupancy sensor (Rückmelder): a green dot while free, red while occupied; it can lie along a platform",
         PLATFORM + """  <path class="bar" d="M0 16 H32"/>
  <path class="lamp" d="M4 16 H10 M22 16 H28"/>
  <circle class="sensor-free" cx="16" cy="16" r="3.6"/>
  <g class="sensor-on">
    <circle class="sensor-glow" cx="16" cy="16" r="6.2"/>
    <circle class="sensor-core" cx="16" cy="16" r="4.2"/>
    <circle class="sensor-hot" cx="15" cy="15" r="1.8"/>
  </g>"""),
    tile("sensor-curve", "Curve with an occupancy sensor: the dot in the middle of the curve",
         f"""  {CURVE_DOWN}
  <circle class="sensor-free" cx="8" cy="24" r="3.6"/>
  <g class="sensor-on">
    <circle class="sensor-glow" cx="8" cy="24" r="6.2"/>
    <circle class="sensor-core" cx="8" cy="24" r="4.2"/>
    <circle class="sensor-hot" cx="7" cy="23" r="1.8"/>
  </g>"""),
    signal_tile("signal-hv-main", "hv-main", 1, "H/V main signal for trains running right"),
    signal_tile("signal-hv-main-distant", "hv-main-distant", 2, "H/V main signal with the distant signal below it on the same mast, for trains running right; two tiles wide"),
    signal_tile("signal-hv-distant", "hv-distant", 1, "H/V distant signal for trains running right"),
    signal_tile("signal-ks-main", "ks-main", 1, "Ks main signal for trains running right"),
    signal_tile("signal-ks-multi", "ks-multi", 1, "Ks multi-section signal for trains running right"),
    signal_tile("signal-ks-distant", "ks-distant", 1, "Ks distant signal for trains running right"),
    signal_tile("signal-sh", "sh-light", 1, "Light shunting signal (Lichtsperrsignal) for trains running right"),
    tile("level-crossing", "Level crossing (Bahnübergang): the road across the track, lamps lit while closed",
         """  <rect class="road" x="11.5" y="0" width="9" height="32"/>
  <path class="bar" d="M0 16 H32"/>
  <path class="lamp" d="M3 16 H9 M23 16 H29"/>
  <circle class="lc-lamp" cx="6" cy="6" r="1.9"/>
  <circle class="lc-lamp" cx="26" cy="26" r="1.9"/>"""),
    tile("district-border", "District border (Hive-Rail): a section joint with each side's district colour",
         """  <path class="bar" d="M0 16 H32"/>
  <path class="lamp" d="M3 16 H13 M19 16 H29"/>
  <path class="joint" d="M16 12 V20"/>
  <rect class="district-a" x="3" y="22.5" width="10" height="3"/>
  <rect class="district-b" x="19" y="22.5" width="10" height="3"/>"""),
    tile("train-number", "Train number field (Zugnummernmelder) on the track; the page sets its number; it can lie along a platform",
         PLATFORM + """  <path class="bar" d="M0 16 H32"/>
  <rect class="number-box" x="2" y="11" width="28" height="10" rx="1.2"/>
  <text class="number" x="16" y="18.8"></text>"""),
    tile("plate", "Name plate (track, signal or turnout name) in its own cell; the page sets its text",
         """  <rect class="plate" x="2" y="10" width="28" height="12"/>
  <text class="plate-text" x="16" y="18.6"></text>"""),
]

# A preview page: every tile in its states, and a small station drawn from them.
STATES = {
    "turnout-right": ["straight", "diverging", "route straight", "route diverging", "occupied diverging"],
    "turnout-left": ["straight", "diverging", "route straight", "route diverging", "occupied diverging"],
    "double-slip": ["straight", "diverging", "route straight", "route diverging", "occupied straight"],
    "signal-hv-main": ["hp0", "hp1", "hp2", "route hp1 locked", "sh-lamp hp0 sh1", "zs1-lamp hp0 zs1"],
    "signal-hv-main-distant": ["hp0 vr1", "hp1 vr0", "hp1 vr1", "route hp2 vr2", "extra-lamp hp1 vr0 short"],
    "signal-hv-distant": ["vr0", "vr1", "vr2", "extra-lamp vr0 short"],
    "signal-ks-main": ["hp0", "ks1", "sh-lamp hp0 sh1", "zs1-lamp hp0 zs1"],
    "signal-ks-multi": ["hp0", "ks1", "ks2", "route ks2", "sh-lamp hp0 sh1"],
    "signal-ks-distant": ["ks1", "ks2"],
    "signal-sh": ["sh0", "sh1", "route sh1"],
    "sensor-curve": ["", "occupied", "route"],
    "level-crossing": ["", "closed", "occupied closed"],
    "train-number": ["", "occupied", "platform-top platform-bottom occupied"],
    "track-straight": ["", "route", "occupied", "platform-top platform-bottom", "platform-top route", "platform-bottom occupied"],
    "sensor": ["", "occupied", "route", "platform-top platform-bottom", "platform-top platform-bottom occupied"],
}


def inline(name, classes="", transform="", text=""):
    """A tile for the page, with these state classes in place of the file's own."""
    svg = (OUT / f"{name}.svg").read_text()
    head, rest = svg.split(">", 1)
    svg = re.sub(r'class="[^"]*"', f'class="{classes}"', head, count=1) + ">" + rest
    if text:
        svg = re.sub(r'(<text class="sp-(?:plate-text|number)"[^>]*>)</text>', rf"\g<1>{text}</text>", svg)
    if transform:
        svg = svg.replace("<svg ", f'<svg style="transform:{transform}" ', 1)
    return svg


def write_preview():
    rows = []
    for name, title in TILES:
        states = STATES.get(name, ["", "route", "occupied"])
        text = {"plate": "Ls 2 I", "train-number": "2001"}.get(name, "")
        wide = ' class="wide"' if BODIES[name][2] == 2 else ""
        cells = "".join(f'<figure{wide}>{inline(name, s, text=text)}<figcaption>{s or "idle"}</figcaption></figure>' for s in states)
        rows.append(f"<section><h2>{name}</h2><p>{title}</p><div class=row>{cells}</div></section>")
    t = inline
    blank = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}"><rect x="0" y="0" width="{W}" height="{H}" fill="#c8c8c8" stroke="#9c9c9c" stroke-width="0.6"/></svg>'
    # A terminus throat: entry with its signal, a turnout down to platform 1 and platform 2
    # straight on, buffer stops; a route set into platform 1, platform 2 occupied by train
    # 2001. The curve below the turnout is turned to run from the top edge to the right.
    station = [
        [t("district-border"), t("signal-hv-main", "route hp1"), t("turnout-right", "route diverging"), t("train-number", "occupied", text="2001"), t("buffer-stop", "occupied")],
        [blank, blank, t("track-curve", "route", "rotate(90deg) scaleY(-1)"), t("track-straight", "route"), t("buffer-stop", "route")],
        [blank, t("plate", text="A"), t("plate", text="W1"), t("plate", text="1"), blank],
    ]
    grid = "".join(f'<div class="panel-row">{"".join(r)}</div>' for r in station)
    # The joining test: a straight track into a diagonal of curve tiles, two rows down, and
    # out straight again; a left-hand turnout whose branch goes up into a curve and on.
    down = lambda c="": t("track-curve", c)
    on = lambda c="": t("track-curve", c, "rotate(90deg) scaleY(-1)")  # top edge to right edge
    up = lambda c="": t("track-curve", c, "scaleY(-1)")  # left edge to top edge
    under = lambda c="": t("track-curve", c, "rotate(-90deg)")  # bottom edge to right edge
    joins = [
        [t("track-straight"), down("route"), blank, blank, blank],
        [blank, on("route"), down("route"), blank, blank],
        [blank, blank, on("occupied"), t("track-straight", "occupied"), t("buffer-stop", "occupied")],
        [blank, under("route"), t("track-straight", "route"), t("buffer-stop", "route"), blank],
        [t("track-straight", "route"), t("turnout-left", "route diverging"), t("track-straight"), t("buffer-stop"), blank],
    ]
    join_grid = "".join(f'<div class="panel-row">{"".join(r)}</div>' for r in joins)
    page = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>SpDrS60 tiles</title>
<style>
  body {{ font: 14px system-ui, sans-serif; margin: 16px; background: #f3f3f3; color: #1d1d1d; }}
  section {{ margin-bottom: 18px; }}
  h2 {{ font-size: 15px; margin: 0; }} p {{ margin: 2px 0 6px; color: #555; }}
  .row {{ display: flex; gap: 14px; flex-wrap: wrap; }}
  figure {{ margin: 0; text-align: center; font-size: 11px; color: #555; }}
  figure svg {{ width: 96px; height: 96px; display: block; }} figure.wide svg {{ width: 192px; }}
  .panel {{ display: inline-block; border: 1px solid #999; }}
  .panel-row {{ display: flex; }} .panel-row svg {{ width: 72px; height: 72px; display: block; }}
</style></head><body>
<h1>Hive-Rail SpDrS60 tiles</h1>
<p>Hive-Rail's own drawings of the Siemens SpDrS60 panel; each tile once, its state as classes.</p>
<h2>A small terminus throat</h2><div class="panel">{grid}</div>
<h2>Joining test: curves in a row and a turnout's branch</h2><div class="panel">{join_grid}</div>
{''.join(rows)}
</body></html>
"""
    (OUT / "preview.html").write_text(page)


# Where each tile's track runs, for the track graph: a path joins two ports, a port being
# a tile edge (W, N, E, S, as drawn: running right, diverging down) and the cell it is on
# (0, or 1 for the right cell of a two-cell tile): "W0" is the left edge of the left cell.
# A turnout has a path per leg; a buffer stop's path has one port, where the track ends.
# The editor and the module loader turn and mirror the ports with the tile.
STRAIGHT = {"through": ["W0", "E0"]}
PATHS = {
    "track-straight": STRAIGHT,
    "track-curve": {"through": ["W0", "S0"]},
    "turnout-right": {"straight": ["W0", "E0"], "diverging": ["W0", "S0"]},
    "turnout-left": {"straight": ["W0", "E0"], "diverging": ["W0", "N0"]},
    "crossing": {"straight": ["W0", "E1"], "diagonal": ["N1", "S0"]},
    "double-slip": {"straight": ["W0", "E1"], "diagonal": ["N1", "S0"], "slip-up": ["W0", "N1"], "slip-down": ["S0", "E1"]},
    "buffer-stop": {"end": ["W0"]},
    "sensor": STRAIGHT,
    "sensor-curve": {"through": ["W0", "S0"]},
    "signal-hv-main": STRAIGHT,
    "signal-hv-main-distant": {"through": ["W0", "E1"]},
    "signal-hv-distant": STRAIGHT,
    "signal-ks-main": STRAIGHT,
    "signal-ks-multi": STRAIGHT,
    "signal-ks-distant": STRAIGHT,
    "signal-sh": STRAIGHT,
    "level-crossing": STRAIGHT,
    "district-border": STRAIGHT,
    "train-number": STRAIGHT,
    "plate": {},
}


def write_js():
    """The tiles for the web pages: their bodies, titles and the style, once."""
    import json
    assert set(PATHS) == set(BODIES), set(PATHS) ^ set(BODIES)
    tiles = {name: {"title": title, "body": body.strip(), "cells": cells, "paths": PATHS[name]} for name, (title, body, cells) in BODIES.items()}
    # The same for the module loader (src/), without the drawings.
    (OUT / "tiles.json").write_text(json.dumps(
        {"cell_mm": CELL_MM, "tiles": {name: {"cells": t["cells"], "paths": t["paths"]} for name, t in tiles.items()}},
        indent=1) + "\n")
    js = ("// Made by tools/spdrs60_tiles.py from assets/spdrs60; do not edit.\n"
          f"const SPDRS60_SIZE = {W};\n"
          f"const SPDRS60_CELL_MM = {CELL_MM};\n"
          f"const SPDRS60_STYLE = {json.dumps(prefixed(STYLE))};\n"
          f"const SPDRS60_TILES = {json.dumps(tiles, indent=1, ensure_ascii=False)};\n")
    (OUT.parent.parent / "web" / "spdrs60-tiles.js").write_text(js)


if __name__ == "__main__":
    write_js()
    for old in OUT.glob("*.svg"):
        if old.stem not in {name for name, _ in TILES}:
            old.unlink()
    write_preview()
    print(f"{len(TILES)} tiles and preview.html in {OUT}")
