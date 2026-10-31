#!/usr/bin/env python3
"""Draws Hive-Rail's own SVGs of German light signals as they look on the track (Ril 301):
the signal screens with their lamps where the real ones have them (assets/signals/*.svg),
a preview page, and web/signal-faces.js for the web pages.

Each signal type is drawn once; its aspect is a class on the <svg> root (or the group a page
draws it in), by its Ril 301 name: hp0, hp1, hp2, vr0, vr1, vr2, ks1, ks2, sh0, sh1, zs1.
Lamps that a signal may or may not have are options, also classes: sh-lamp (Sh 1 on a main
signal), zs1-lamp (Zs 1), extra-lamp (the distant signal's white Zusatzlicht for a shortened
braking distance, lit with the class "short").

  hv-main      H/V light main signal (Lichthauptsignal): tall screen with bevelled top
               corners; green top left, two reds side by side in the middle (Hp 0 shows
               both; with Sh 1 only the left one), yellow bottom left (Hp 2: green over
               yellow); Sh 1 two whites rising to the right; Zs 1 three whites in an upward
               triangle in a small head below.
  hv-distant   H/V light distant signal (Lichtvorsignal): a screen tilted rising to the
               right; lamps lower left and upper right: Vr 0 yellow-yellow, Vr 1 green-green,
               Vr 2 yellow (lower left) and green (upper right); the white Zusatzlicht on a
               tab at the upper left.
  ks-main      Ks main signal: red over green; Sh 1 white lower left and centre; Zs 1 the
               centre white, flashing.
  ks-multi     Ks multi-section signal: red top centre, green middle left, yellow middle
               right (Ks 2 is its distant function); Sh 1 and Zs 1 as on the Ks main signal.
  hv-main-distant  the H/V main signal with the H/V distant signal below it on the same mast;
               the distant is dark while the main shows Hp 0.
  ks-distant   Ks distant signal: green left, yellow right, side by side.
  sh-light     Light shunting signal (Lichtsperrsignal): two reds side by side for Sh 0 (Hp 0),
               two whites rising to the right for Sh 1.

Usage: tools/signal_faces.py   (writes assets/signals/ and web/signal-faces.js)
"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "signals"
OUT.mkdir(parents=True, exist_ok=True)

STYLE = """
  .sf-screen { fill: #0d0d0d; stroke: #3a3a3a; stroke-width: 0.8; }
  .sf-mast { fill: #8a8a8a; }
  .sf-lamp { fill: #262626; stroke: #3c3c3c; stroke-width: 0.6; }
  .sf-option { display: none; }
  .sh-lamp .sf-option.sh-lamp, .zs1-lamp .sf-option.zs1-lamp, .extra-lamp .sf-option.extra-lamp { display: inline; }
  /* H/V main */
  .hp0 .sf-red1, .hp0 .sf-red2 { fill: #ff2a1f; }
  .hp0.sh1 .sf-red2 { fill: #262626; }
  .hp1 .sf-green, .hp2 .sf-green { fill: #2fe05a; }
  .hp2 .sf-yellow { fill: #ffbf1a; }
  .sh1 .sf-white { fill: #f6f6f6; }
  .zs1 .sf-zs1 { fill: #f6f6f6; }
  /* H/V distant */
  .vr0 .sf-ll, .vr0 .sf-ur, .vr2 .sf-ll { fill: #ffbf1a; }
  .vr1 .sf-ll, .vr1 .sf-ur, .vr2 .sf-ur { fill: #2fe05a; }
  .short .sf-extra { fill: #f6f6f6; }
  .hp0 .sf-on-main .sf-ll, .hp0 .sf-on-main .sf-ur { fill: #262626; }
  /* Ks */
  .hp0 .sf-ks-red { fill: #ff2a1f; }
  .ks1 .sf-ks-green { fill: #2fe05a; }
  .ks2 .sf-ks-yellow { fill: #ffbf1a; }
  .zs1 .sf-ks-zs1 { fill: #f6f6f6; animation: sf-blink 1s steps(2, start) infinite; }
  /* light shunting signal */
  .sh0 .sf-sh-red, .hp0 .sf-sh-red { fill: #ff2a1f; }
  @keyframes sf-blink { to { visibility: hidden; } }
"""

FACES = {}


def face(name, title, width, height, body):
    FACES[name] = {"title": title, "width": width, "height": height, "body": body.strip()}
    (OUT / f"{name}.svg").write_text(f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width * 2}" height="{height * 2}" class="">
  <title>{title}</title>
  <style>{STYLE}  </style>
{body}
</svg>
""")


def lamp(cx, cy, r, classes):
    return f'  <circle class="sf-lamp {classes}" cx="{cx}" cy="{cy}" r="{r}"/>\n'


# H/V main: a 40 x 76 screen with bevelled top corners; the Zs 1 head below it.
face("hv-main", "H/V light main signal (Lichthauptsignal): Hp 0 (two reds), Hp 1, Hp 2; Sh 1 and Zs 1 as options", 40, 100,
     '  <rect class="sf-mast" x="18" y="76" width="4" height="24"/>\n'
     '  <path class="sf-screen" d="M6 0 H34 L40 6 V76 H0 V6 Z"/>\n'
     + lamp(11.5, 15, 4.4, "sf-green")
     + lamp(11.5, 29, 4.4, "sf-red1")
     + lamp(28.5, 29, 4.4, "sf-red2")
     + lamp(29, 41, 2.6, "sf-white sf-option sh-lamp")
     + lamp(11, 55, 2.6, "sf-white sf-option sh-lamp")
     + lamp(11.5, 65, 4.4, "sf-yellow")
     + '  <g class="sf-option zs1-lamp">\n'
     '    <rect class="sf-screen" x="8" y="78" width="24" height="14" rx="2"/>\n'
     + "  " + lamp(20, 82, 2.2, "sf-zs1")
     + "  " + lamp(13, 88, 2.2, "sf-zs1")
     + "  " + lamp(27, 88, 2.2, "sf-zs1")
     + "  </g>")

# H/V distant: a 60 x 26 screen with bevelled corners, tilted 45 degrees rising to the right.
face("hv-distant", "H/V light distant signal (Lichtvorsignal): Vr 0, Vr 1, Vr 2; the white Zusatzlicht as an option", 64, 64,
     '  <g transform="rotate(-45 32 32)">\n'
     '    <circle class="sf-screen sf-option extra-lamp" cx="32" cy="14" r="5"/>\n'
     '    <path class="sf-screen" d="M8 19 H56 L62 25 V39 L56 45 H8 L2 39 V25 Z"/>\n'
     + "  " + lamp(14, 32, 4.6, "sf-ll")
     + "  " + lamp(50, 32, 4.6, "sf-ur")
     + "  " + lamp(32, 14, 2.4, "sf-extra sf-option extra-lamp")
     + "  </g>")

# Ks main: red over green.
face("ks-main", "Ks main signal: red over green; Hp 0, Ks 1; Sh 1 and Zs 1 as options", 40, 66,
     '  <rect class="sf-mast" x="18" y="52" width="4" height="14"/>\n'
     '  <rect class="sf-screen" x="0" y="0" width="40" height="52" rx="1.5"/>\n'
     + lamp(20, 11, 4.4, "sf-ks-red")
     + lamp(20, 25, 4.4, "sf-ks-green")
     + lamp(20, 38, 2.4, "sf-white sf-ks-zs1 sf-option sh-lamp zs1-lamp")
     + lamp(7, 46, 2.4, "sf-white sf-option sh-lamp"))

# Ks multi-section signal: red top centre, green and yellow side by side below it.
face("ks-multi", "Ks multi-section signal (Mehrabschnittssignal): Hp 0, Ks 1, Ks 2; Sh 1 and Zs 1 as options", 40, 56,
     '  <rect class="sf-mast" x="18" y="46" width="4" height="10"/>\n'
     '  <rect class="sf-screen" x="0" y="0" width="40" height="46" rx="1.5"/>\n'
     + lamp(20, 10, 4.4, "sf-ks-red")
     + lamp(9, 24, 4.4, "sf-ks-green")
     + lamp(31, 24, 4.4, "sf-ks-yellow")
     + lamp(20, 33, 2.4, "sf-white sf-ks-zs1 sf-option sh-lamp zs1-lamp")
     + lamp(7, 41, 2.4, "sf-white sf-option sh-lamp"))

# Ks distant: green left, yellow right.
face("ks-distant", "Ks distant signal (Ks-Vorsignal): Ks 1 (green), Ks 2 (yellow)", 40, 40,
     '  <rect class="sf-mast" x="18" y="30" width="4" height="10"/>\n'
     '  <rect class="sf-screen" x="0" y="0" width="40" height="30" rx="1.5"/>\n'
     + lamp(11, 15, 4.4, "sf-ks-green")
     + lamp(29, 15, 4.4, "sf-ks-yellow"))

# Light shunting signal: two reds side by side, two whites rising to the right.
face("sh-light", "Light shunting signal (Lichtsperrsignal): Sh 0 (two reds), Sh 1 (two whites rising to the right)", 32, 36,
     '  <rect class="sf-mast" x="14" y="28" width="4" height="8"/>\n'
     '  <path class="sf-screen" d="M4 0 H28 L32 4 V28 H0 V4 Z"/>\n'
     + lamp(8, 9, 3.4, "sf-sh-red")
     + lamp(24, 9, 3.4, "sf-sh-red")
     + lamp(23, 15, 2.4, "sf-white")
     + lamp(9, 23, 2.4, "sf-white"))

# H/V main with the H/V distant signal below it on the same mast; the distant is dark while
# the main signal shows Hp 0.
face("hv-main-distant", "H/V main signal with the distant signal below it on the same mast: Hp 0 (distant dark), Hp 1 or Hp 2 with Vr 0, Vr 1 or Vr 2", 48, 150,
     '  <rect class="sf-mast" x="22" y="76" width="4" height="74"/>\n'
     '  <g transform="translate(4 0)">\n' + FACES["hv-main"]["body"].replace('<rect class="sf-mast" x="18" y="76" width="4" height="24"/>', "") + "\n  </g>\n"
     '  <g class="sf-on-main" transform="translate(0 94) scale(0.75)">\n' + FACES["hv-distant"]["body"] + "\n  </g>")

STATES = {
    "hv-main": ["hp0", "hp1", "hp2", "sh-lamp hp0 sh1", "zs1-lamp hp0 zs1", "sh-lamp zs1-lamp"],
    "hv-distant": ["vr0", "vr1", "vr2", "extra-lamp vr0 short", ""],
    "ks-main": ["hp0", "ks1", "sh-lamp hp0 sh1", "zs1-lamp hp0 zs1"],
    "ks-multi": ["hp0", "ks1", "ks2", "sh-lamp hp0 sh1", "zs1-lamp hp0 zs1"],
    "hv-main-distant": ["hp0 vr1", "hp1 vr0", "hp1 vr1", "hp2 vr2", "extra-lamp hp1 vr0 short"],
    "ks-distant": ["ks1", "ks2", ""],
    "sh-light": ["sh0", "sh1", ""],
}


def write_preview():
    rows = []
    for name, info in FACES.items():
        cells = "".join(
            f'<figure><svg viewBox="0 0 {info["width"]} {info["height"]}" style="height:{info["height"] * 2}px" class="{state}">{info["body"]}</svg>'
            f'<figcaption>{state.replace("sh-lamp", "").replace("zs1-lamp", "").replace("extra-lamp", "").strip() or "dark"}</figcaption></figure>'
            for state in STATES[name])
        rows.append(f'<section><h2>{name}</h2><p>{info["title"]}</p><div class="row">{cells}</div></section>')
    (OUT / "preview.html").write_text(f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Signal faces</title>
<style>
  body {{ font: 14px system-ui, sans-serif; margin: 16px; background: #dfe3e6; color: #1d1d1d; }}
  section {{ margin-bottom: 18px; }} h2 {{ font-size: 15px; margin: 0; }} p {{ margin: 2px 0 8px; color: #555; }}
  .row {{ display: flex; gap: 22px; flex-wrap: wrap; align-items: flex-end; }}
  figure {{ margin: 0; text-align: center; font-size: 12px; color: #444; }} svg {{ display: block; margin: 0 auto 4px; }}
  {STYLE}
</style></head><body>
<h1>Hive-Rail signal faces</h1>
<p>German light signals as they look on the track (Ril 301), each drawn once, the aspect as a class.</p>
{''.join(rows)}
</body></html>
""")


def write_js():
    (ROOT / "web" / "signal-faces.js").write_text(
        "// Made by tools/signal_faces.py; do not edit.\n"
        f"const SIGNAL_FACE_STYLE = {json.dumps(STYLE)};\n"
        f"const SIGNAL_FACES = {json.dumps(FACES, indent=1)};\n")


if __name__ == "__main__":
    write_preview()
    write_js()
    print(f"{len(FACES)} signal faces and preview.html in {OUT}")
