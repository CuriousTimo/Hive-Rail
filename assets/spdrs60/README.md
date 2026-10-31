# SpDrS60 panel tiles

Hive-Rail's own drawings of the panel (Stelltisch) of the Siemens SpDrS60 relay interlocking, for a schematic track diagram in the style of the Deutsche Bundesbahn's signal boxes. They are drawn from the prototype, not taken from another program, and are licensed like the rest of Hive-Rail (Apache-2.0).

The tiles are made by `tools/spdrs60_tiles.py`; change the generator, not the SVG files, and run it again. It also writes `web/spdrs60-tiles.js` for the panel editor. Open `preview.html` to see every tile in its states, a small terminus and a joining test.

## Placing tiles

The tiles are squares of 32 × 32 units on a grid, as in Rocrail's editor, and are placed, turned a quarter at a time and mirrored. The panel editor in the web view (`/panel`) does this: choose a tile, click or drag on the grid; <kbd>R</kbd> turns, <kbd>M</kbd> mirrors, <kbd>Del</kbd> removes. A panel is saved as a JSON file:

```json
{ "schema": "hive-rail-panel/1", "name": "Adorf", "width": 40, "height": 24,
  "tiles": [ { "tile": "turnout-right", "x": 5, "y": 3, "turn": 2, "mirror": false,
               "link": { "kind": "turnout", "id": "timo-adorf/W1" } } ] }
```

`turn` is 0 to 3 quarters clockwise; `mirror` flips a tile across its track (before turning). `text` is a name plate's or train number's text, `options` a main signal's optional lamps, `link` the block, turnout, signal or level crossing of the layout it shows. `?panel=<address>` opens a panel file in the editor.

**The joining rule.** Track meets a tile edge only at the middle of the edge, always over the same span (13.5 to 18.5 of 32). A diagonal is a band whose ends are cut along the tile edges over exactly that span. So any two tiles placed next to each other join flush: a curve meets a straight track without a step, and curves in a row make one straight diagonal.

## Tiles

| File | What it shows |
| --- | --- |
| `track-straight.svg` | Straight track: a thin black bar with a short lamp in the middle |
| `track-curve.svg` | 45° curve from the middle of the left edge to the middle of the bottom edge; turned and mirrored for the other directions, and in a row for a diagonal |
| `turnout-right.svg` | Right-hand turnout: trunk at the left, straight to the right, diverging down; the point at the left, a small position lamp for each leg |
| `turnout-left.svg` | Left-hand turnout: the same, diverging up |
| `crossing.svg` | Crossing, two tiles wide: straight track and a 45° track |
| `double-slip.svg` | Double slip, two tiles wide: a crossing with both slip curves |
| `buffer-stop.svg` | Buffer stop at the right end |
| `sensor.svg` | Track with an occupancy sensor: a green dot in the middle while free, a larger red dot while occupied |
| `sensor-curve.svg` | The curve with an occupancy sensor in its middle |
| `signal-hv-main.svg` | H/V main signal |
| `signal-hv-main-distant.svg` | H/V main signal with the H/V distant signal below it on the same mast, two tiles wide; the distant is dark while the main shows Hp 0 |
| `signal-hv-distant.svg` | H/V distant signal |
| `signal-ks-main.svg` | Ks main signal (red over green) |
| `signal-ks-multi.svg` | Ks multi-section signal (red over green and yellow) |
| `signal-ks-distant.svg` | Ks distant signal (green and yellow side by side) |
| `signal-sh.svg` | Light shunting signal |

The signal tiles show the real signal (`assets/signals/`, made by `tools/signal_faces.py`) small, lying along the track below it, the right side for trains running right: the mast at the left with the locked lamp at its foot, the screen towards the right, the lamps where the real signal has them. The real panel's push buttons (for setting routes by pressing a start and an end button) are left out: Hive-Rail's panel does not set routes that way.
| `level-crossing.svg` | Level crossing: the road across the track, lamps lit while closed |
| `district-border.svg` | Hive-Rail's district border: a section joint with each side's district colour (`--district-a`, `--district-b`) |
| `train-number.svg` | Train number field on the track (its text is set by the page) |
| `plate.svg` | Name plate in its own cell (track, signal or turnout name; its text is set by the page) |

## States

A tile's state is a class on its `<svg>` root (or on the group a page draws it in). The tiles' own parts are prefixed `sp-` (`sp-lamp`, `sp-road`), so a page's styles never reach them.

| Class | Shows |
| --- | --- |
| *(none)* | Idle: the lamps dark |
| `route` | Route set over the track: lamps yellow |
| `occupied` | Track occupied: lamps red |
| `straight`, `diverging` | A turnout's or double slip's position: lights that leg and its position lamp |
| `closed` | A level crossing is closed |
| `locked` | A signal's locked lamp at its foot (yellow) |

Signal aspects by their Ril 301 names; the signal tile lights the lamps the real signal shows:

| Class | Aspect |
| --- | --- |
| `hp0` | Hp 0, stop (on the H/V main signal two reds side by side; a distant signal under it goes dark) |
| `hp1`, `hp2` | Hp 1 proceed, Hp 2 proceed slowly (green; green over yellow) |
| `ks1`, `ks2` | Ks 1 proceed, Ks 2 proceed, expect stop |
| `vr0`, `vr1`, `vr2` | Vr 0 expect stop, Vr 1 expect proceed, Vr 2 expect proceed slowly |
| `sh0`, `sh1` | Sh 0 stop, Sh 1 shunting allowed |
| `zs1` | Zs 1, the substitute signal |

Lamps a signal may or may not have are classes too: `sh-lamp` (Sh 1), `zs1-lamp` (Zs 1), `extra-lamp` (the distant signal's Zusatzlicht, lit with `short`). A combined signal takes both aspects, such as `hp1 vr0`.

Classes combine, for example `route diverging` or `sh-lamp hp0 sh1`.

## About other SpDrS60 drawings

Rocrail's SpDrS60 theme and the spdrs60 program draw the same prototype, so the tiles look alike. Their files are GPL and are not used here, neither copied nor traced.
