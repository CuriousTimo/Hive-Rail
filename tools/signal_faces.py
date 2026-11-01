#!/usr/bin/env python3
"""Draws Hive-Rail's own SVGs of German light signals as they look on the track (Ril 301):
the signal screens with their lamps where the real ones have them (assets/signals/*.svg),
a preview page, and web/signal-faces.js for the web pages.

The set is the DB light signals one can buy for H0 with LEDs (Viessmann's H/V and Ks
signals and light shunting signals), each drawn as the model is: its screen, its lamps where
the model has them.
Each signal type is drawn once; its aspect is a class on the <svg> root (or the group a page
draws it in), by its Ril 301 name: hp0, hp1, hp2, vr0, vr1, vr2, ks1, ks2, sh0, sh1.
Lamps that a signal may or may not have are options, also classes: sh-lamp (Sh 1 on a Ks
main signal: a departure signal), extra-lamp (the upper white: the distant signal's
Zusatzlicht for a shortened braking distance, lit with the class "short"), one-red (an H/V
departure signal showing Hp 0 with one red, as when both reds are wired to one output).

  hv-block     H/V block signal (Viessmann 4011): red and green side by side in the lower row.
  hv-entry     H/V entry signal (4012): green at the top, red and yellow side by side below.
  hv-departure H/V departure signal (4013): green top left, two reds side by side (Hp 0
               shows both, with Sh 1 only the left one), yellow bottom left, Sh 1 two whites
               rising to the right.
  hv-distant   H/V light distant signal (Lichtvorsignal): a screen tilted rising to the
               right with four lamps, a green and a yellow at its lower left and at its upper
               right: Vr 0 both yellow, Vr 1 both green, Vr 2 yellow (lower left) and green
               (upper right); the white Zusatzlicht on a tab at the upper left.
  ks-main      Ks main signal: red over green; a departure signal has Sh 1, two whites
               lower left and centre.
  ks-multi     Ks multi-section signal: red top centre, green middle left, yellow middle
               right (Ks 2 is its distant function); Sh 1 as on the Ks main signal; the
               upper white at the upper left, as on the Ks distant signal.
  hv-block-distant, hv-entry-distant, hv-departure-distant  the H/V main signals with the
               H/V distant signal below them on the same mast (4014, 4015, 4016); the
               distant is dark while the main shows Hp 0.
  ks-distant   Ks distant signal: green left, yellow right, side by side.
  sh-light     Light shunting signal (Lichtsperrsignal): a wide screen, two reds at the ends
               of its top row for Sh 0 (Hp 0), two whites rising to the right for Sh 1 (one
               between the reds, one below the left red).

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
  .sh-lamp .sf-option.sh-lamp, .extra-lamp .sf-option.extra-lamp { display: inline; }
  /* H/V main */
  .hp0 .sf-red1, .hp0 .sf-red2 { fill: #ff2a1f; }
  .hp0.sh1 .sf-red2, .one-red.hp0 .sf-red2 { fill: #262626; }
  .hp1 .sf-green, .hp2 .sf-green { fill: #2fe05a; }
  .hp2 .sf-yellow { fill: #ffbf1a; }
  .sh1 .sf-white { fill: #f6f6f6; }
  /* H/V distant */
  .vr0 .sf-ll.sf-vr-yellow, .vr0 .sf-ur.sf-vr-yellow, .vr2 .sf-ll.sf-vr-yellow { fill: #ffbf1a; }
  .vr1 .sf-ll.sf-vr-green, .vr1 .sf-ur.sf-vr-green, .vr2 .sf-ur.sf-vr-green { fill: #2fe05a; }
  .short .sf-extra { fill: #f6f6f6; }
  .hp0 .sf-on-main .sf-vr-yellow, .hp0 .sf-on-main .sf-vr-green { fill: #262626; }
  /* Ks */
  .hp0 .sf-ks-red { fill: #ff2a1f; }
  .ks1 .sf-ks-green { fill: #2fe05a; }
  .ks2 .sf-ks-yellow { fill: #ffbf1a; }
  /* light shunting signal */
  .sh0 .sf-sh-red, .hp0 .sf-sh-red { fill: #ff2a1f; }
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


# H/V main signals: a 40 x 76 screen with bevelled top corners, as on Viessmann's models;
# where the lamps sit depends on the kind of signal.
HV_MAST = '  <rect class="sf-mast" x="18" y="76" width="4" height="24"/>\n'
HV_SCREEN = '  <path class="sf-screen" d="M6 0 H34 L40 6 V76 H0 V6 Z"/>\n'

# Block signal (Viessmann 4011): red and green side by side in the lower row.
face("hv-block", "H/V block signal (Blocksignal): Hp 0 (one red), Hp 1", 40, 100,
     HV_MAST + HV_SCREEN
     + lamp(11.5, 58, 4.4, "sf-red1")
     + lamp(25.5, 58, 4.4, "sf-green"))

# Entry signal (Viessmann 4012): green at the top, red and yellow side by side below, the
# yellow under the green.
face("hv-entry", "H/V entry signal (Einfahrsignal): Hp 0 (one red), Hp 1, Hp 2", 40, 100,
     HV_MAST + HV_SCREEN
     + lamp(25.5, 12, 4.4, "sf-green")
     + lamp(11.5, 58, 4.4, "sf-red1")
     + lamp(25.5, 58, 4.4, "sf-yellow"))

# Departure signal (Viessmann 4013): green top left, two reds side by side, yellow bottom
# left, Sh 1 two whites rising to the right. Hp 0 shows both reds (the option one-red: only
# the left one, as when both are wired to one output); with Sh 1 the left one stays lit.
face("hv-departure", "H/V departure signal (Ausfahrsignal): Hp 0 (two reds), Hp 1, Hp 2, Hp 0 + Sh 1", 40, 100,
     HV_MAST + HV_SCREEN
     + lamp(11.5, 15, 4.4, "sf-green")
     + lamp(11.5, 29, 4.4, "sf-red1")
     + lamp(28.5, 29, 4.4, "sf-red2")
     + lamp(29, 41, 2.6, "sf-white")
     + lamp(11, 55, 2.6, "sf-white")
     + lamp(11.5, 65, 4.4, "sf-yellow"))

# H/V distant: a screen running diagonally, rising to the right, with horizontal top and
# bottom edges; four lamps in two horizontal pairs, yellow left and green right, one pair at
# the upper right and one at the lower left (Vr 0 both yellow, Vr 1 both green, Vr 2 the
# lower yellow and the upper green); the Zusatzlicht on a tab at the upper left; the mast
# below. Lamp places after the public-domain drawings Vr0/Vr1/Vr2_Licht.svg (Commons).
face("hv-distant", "H/V light distant signal (Lichtvorsignal): Vr 0, Vr 1, Vr 2; the white Zusatzlicht as an option", 64, 64,
     '  <rect class="sf-mast sf-own-mast" x="28" y="52" width="4" height="12"/>\n'
     '  <circle class="sf-screen sf-option extra-lamp" cx="22" cy="18" r="4.6"/>\n'
     '  <path class="sf-screen" d="M31 11 H56 L60 15 V23 L28 55 H4 L0 51 V42 Z"/>\n'
     + lamp(41, 17.5, 4.2, "sf-ur sf-vr-yellow")
     + lamp(51, 17.5, 4.2, "sf-ur sf-vr-green")
     + lamp(10, 48.5, 4.2, "sf-ll sf-vr-yellow")
     + lamp(20, 48.5, 4.2, "sf-ll sf-vr-green")
     + lamp(22, 18, 2.2, "sf-extra sf-option extra-lamp"))

# Ks screens: tall (64 x 125 cm), lamps where the fully equipped Ks screen has them (after the
# public-domain drawing Ks-Schirm_Hp0_Sh1_Ra12.svg, Commons, scaled to 40 wide): red top
# centre, green and yellow left and right in the middle (a Ks main signal: its green below
# the red), Sh 1 the two whites centre below and lower left, the upper white (Zusatzlicht or
# Kennlicht) upper left.
KS_SCREEN = '  <rect class="sf-screen" x="0" y="0" width="40" height="77.5" rx="1.5"/>\n'
KS_MAST = '  <rect class="sf-mast" x="18" y="77.5" width="4" height="12.5"/>\n'
KS_WHITES = (lamp(20, 50.6, 2.3, "sf-white sf-option sh-lamp")
             + lamp(7.2, 61.7, 2.3, "sf-white sf-option sh-lamp"))

face("ks-main", "Ks main signal: red, the green below it; Hp 0, Ks 1; Sh 1 (departure signals)", 40, 90,
     KS_MAST + KS_SCREEN
     + lamp(20, 20, 4.7, "sf-ks-red")
     + lamp(20, 35.8, 4.7, "sf-ks-green")
     + KS_WHITES)

face("ks-multi", "Ks multi-section signal (Mehrabschnittssignal): Hp 0, Ks 1, Ks 2; Sh 1 (departure signals); the upper white", 40, 90,
     KS_MAST + KS_SCREEN
     + lamp(20, 20, 4.7, "sf-ks-red")
     + lamp(10.3, 35.8, 4.7, "sf-ks-green")
     + lamp(29.7, 35.8, 4.7, "sf-ks-yellow")
     + lamp(10.3, 9.1, 2.3, "sf-extra sf-option extra-lamp")
     + KS_WHITES)

face("ks-distant", "Ks distant signal (Ks-Vorsignal): Ks 1 (green), Ks 2 (yellow); the upper white", 40, 90,
     KS_MAST + KS_SCREEN
     + lamp(10.3, 35.8, 4.7, "sf-ks-green")
     + lamp(29.7, 35.8, 4.7, "sf-ks-yellow")
     + lamp(10.3, 9.1, 2.3, "sf-extra sf-option extra-lamp"))

# Light shunting signal (Lichtsperrsignal): a wide screen with bevelled top corners; two
# reds at the left and the right of the top row (Sh 0, now shown as Hp 0), a white between
# them and a white below the left red (Sh 1: the two whites, rising to the right). After the
# public-domain drawing Sh0_Licht1.svg (Commons).
face("sh-light", "Light shunting signal (Lichtsperrsignal): Sh 0 (two reds), Sh 1 (two whites rising to the right)", 48, 34,
     '  <rect class="sf-mast" x="22" y="28" width="4" height="6"/>\n'
     '  <path class="sf-screen" d="M7 0 H41 L48 7 V28 H0 V7 Z"/>\n'
     + lamp(11, 8.5, 4.2, "sf-sh-red")
     + lamp(37, 8.5, 4.2, "sf-sh-red")
     + lamp(24, 8.5, 4.2, "sf-white")
     + lamp(11, 20, 4.2, "sf-white"))

# H/V main signals with the H/V distant signal below them on the same mast (Viessmann 4014,
# 4015, 4016); the distant is dark while the main signal shows Hp 0, and has no Zusatzlicht.
for kind, model in (("block", "block signal"), ("entry", "entry signal"), ("departure", "departure signal")):
    face(f"hv-{kind}-distant", f"H/V {model} with the distant signal below it on the same mast: the main signal's aspects, with Vr 0, Vr 1 or Vr 2 (dark at Hp 0)", 48, 150,
         '  <rect class="sf-mast" x="22" y="76" width="4" height="74"/>\n'
         '  <g transform="translate(4 0)">\n' + FACES[f"hv-{kind}"]["body"].replace(HV_MAST.strip(), "") + "\n  </g>\n"
         '  <g class="sf-on-main" transform="translate(2 94) scale(0.75)">\n'
         + FACES["hv-distant"]["body"].replace('<rect class="sf-mast sf-own-mast" x="28" y="52" width="4" height="12"/>', "") + "\n  </g>")

STATES = {
    "hv-block": ["hp0", "hp1", ""],
    "hv-entry": ["hp0", "hp1", "hp2"],
    "hv-departure": ["hp0", "one-red hp0", "hp1", "hp2", "hp0 sh1"],
    "hv-distant": ["vr0", "vr1", "vr2", "extra-lamp vr0 short", ""],
    "ks-main": ["hp0", "ks1", "sh-lamp hp0 sh1"],
    "ks-multi": ["hp0", "ks1", "ks2", "extra-lamp ks2 short", "sh-lamp hp0 sh1"],
    "hv-block-distant": ["hp0 vr1", "hp1 vr0", "hp1 vr1", "hp1 vr2"],
    "hv-entry-distant": ["hp0 vr1", "hp1 vr1", "hp2 vr0", "hp2 vr2"],
    "hv-departure-distant": ["hp0 vr1", "hp1 vr0", "hp2 vr2", "hp0 sh1"],
    "ks-distant": ["ks1", "ks2", "extra-lamp ks2 short", ""],
    "sh-light": ["sh0", "sh1", ""],
}


def write_preview():
    rows = []
    for name, info in FACES.items():
        cells = "".join(
            f'<figure><svg viewBox="0 0 {info["width"]} {info["height"]}" style="height:{info["height"] * 2}px" class="{state}">{info["body"]}</svg>'
            f'<figcaption>{state.replace("sh-lamp", "").replace("one-red", "(one red)").replace("extra-lamp", "").strip() or "dark"}</figcaption></figure>'
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
