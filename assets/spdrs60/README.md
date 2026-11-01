# SpDrS60 panel tiles

Hive-Rail's own drawings of the panel (Stelltisch) of the Siemens SpDrS60 relay interlocking, for a schematic track diagram in the style of the Deutsche Bundesbahn's signal boxes. They are drawn from the prototype, not taken from another program, and are licensed like the rest of Hive-Rail (Apache-2.0).

The tiles are made by `tools/spdrs60_tiles.py`; change the generator, not the SVG files, and run it again. It also writes `web/spdrs60-tiles.js` for the panel editor. Open `preview.html` to see every tile in its states, a small terminus and a joining test.

## Drawing a module

The tiles are squares on a grid, as in Rocrail's editor. A module's grid is its real size in cells of 50 mm (`CELL_MM` in the generator): about the 46 mm between the tracks of H0 double track, so parallel tracks lie one cell apart. All turns are 45 degrees, as on the real panel. The module editor in the web view (`/panel`) draws a module: choose a tile, click or drag on the grid; <kbd>R</kbd> turns, <kbd>M</kbd> mirrors, <kbd>Del</kbd> removes. It opens and saves the module file (schema version 3), the one source of everything about the module:

```json
{ "schema_version": 3, "module_id": "timo-adorf", "friendly_name": "Adorf", "owner": "Timo", "scale": "H0",
  "size_mm": [1800, 546],
  "ends": [ { "id": "B", "side": "right", "profile": "2E96", "tracks": ["in", "out"] } ],
  "tracks": [ { "name": "in", "direction": "B>A" } ],
  "tiles": [ { "tile": "turnout-left", "x": 26, "y": 5, "turn": 2, "mirror": false,
               "id": "3", "block": "throat", "device": "point_3" },
             { "tile": "signal-hv-departure", "x": 24, "y": 5, "turn": 0, "id": "N1", "track": "1",
               "block": "adorf_1_e", "device": "signal_n1" } ],
  "stations": [ { "name": "Adorf", "code": "Ad", "tracks": [ { "name": "1", "track": "1", "platform": true } ] } ],
  "blocks": [ { "id": "adorf_1_e", "track": "1", "detected_rail": "outer",
                "sections": [ { "device": "sensor_1_h_b", "role": "stop", "for": "A>B" } ] } ],
  "devices": [ { "id": "signal_n1", "type": "signal", "bus": "loconet", "address": 121, "decoder": "Viessmann 5229",
                 "aspects": ["hp0", "hp1", "hp2"] } ] }
```

A tile has its place (`x`, `y`), `turn` (0 to 3 quarters clockwise) and `mirror` (flipped across its track, before turning), and what it stands for: `id` (a turnout's number, a signal's or sensor's name), `track` (the track it belongs to), `block` (its occupancy section), `device` (in the device list with its bus, address and decoder), `text` (name plates, train numbers), `options` (a signal's optional lamps, a track's platforms) and `distant` (the signal a combined or distant signal announces). The editor finds the ends where track leaves the grid; an end has a name, a profile and its tracks. A terminus whose other tracks end at buffer stops has one end. The loader (`src/tile_module.rs`) derives the track pieces, their lengths (from the real size), turnouts, devices' places, blocks and the map's drawing from the tiles; the tile paths it follows are in `tiles.json`, made by the generator. `?module=<address>` opens a module file in the editor; the web view's module folder (`examples/layout/`, or `--modules <dir>`) is in its Modules list.

**The joining rule.** Track meets a tile edge only at the middle of the edge, always over the same span (13.5 to 18.5 of 32). A diagonal is a band whose ends are cut along the tile edges over exactly that span. So any two tiles placed next to each other join flush: a curve meets a straight track without a step, and curves in a row make one straight diagonal.

## Tiles

| File | What it shows |
| --- | --- |
| `track-straight.svg` | Straight track: a thin black bar with a short lamp in the middle; with platform above, below or both (the options `platform-top`, `platform-bottom`), the palette's "platform track" |
| `track-curve.svg` | 45° curve from the middle of the left edge to the middle of the bottom edge; turned and mirrored for the other directions, and in a row for a diagonal |
| `turnout-right.svg` | Right-hand turnout: trunk at the left, straight to the right, diverging down; the point at the left, a small position lamp for each leg |
| `turnout-left.svg` | Left-hand turnout: the same, diverging up |
| `crossing.svg` | Crossing, two tiles wide: straight track and a 45° track |
| `double-slip.svg` | Double slip, two tiles wide: a crossing with both slip curves |
| `buffer-stop.svg` | Buffer stop at the right end |
| `sensor.svg` | Track with an occupancy sensor: a green dot in the middle while free, a larger red dot while occupied; it can lie along a platform |
| `sensor-curve.svg` | The curve with an occupancy sensor in its middle |
| `signal-hv-block.svg`, `signal-hv-entry.svg`, `signal-hv-departure.svg` | H/V block, entry and departure signal, each as the Viessmann model has its lamps |
| `signal-hv-block-distant.svg`, `signal-hv-entry-distant.svg`, `signal-hv-departure-distant.svg` | The same with the H/V distant signal below on the same mast, two tiles wide; the distant is dark while the main shows Hp 0 |
| `signal-hv-distant.svg` | H/V distant signal |
| `signal-ks-main.svg` | Ks main signal (red over green) |
| `signal-ks-multi.svg` | Ks multi-section signal (red over green and yellow) |
| `signal-ks-distant.svg` | Ks distant signal (green and yellow side by side) |
| `signal-sh.svg` | Light shunting signal |

The signal tiles show the real signal (`assets/signals/`, made by `tools/signal_faces.py`) small, lying along the track below it, the right side for trains running right: the mast at the left with the locked lamp at its foot, the screen towards the right, the lamps where the real signal has them. The real panel's push buttons (for setting routes by pressing a start and an end button) are left out: Hive-Rail's panel does not set routes that way.
| `level-crossing.svg` | Level crossing: the road across the track, lamps lit while closed |
| `district-border.svg` | Hive-Rail's district border: a section joint with each side's district colour (`--district-a`, `--district-b`) |
| `train-number.svg` | Train number field on the track (its text is set by the page); it can lie along a platform |
| `lamp.svg` | Lamp (street or building light): lit while on (`on`) |
| `windmill.svg` | Windmill: its sails turn while it runs (`on`) |
| `action.svg` | Action, a Viessmann eMotion scene or another animated accessory: lit while it runs (`on`) |
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
| `on` | A lamp is lit, a windmill turns, an action runs (on the map: lamps from 18:00 to 07:00 on the fast clock, windmills in daylight) |
| `locked` | A signal's locked lamp at its foot (yellow) |

Signal aspects by their Ril 301 names; the signal tile lights the lamps the real signal shows:

| Class | Aspect |
| --- | --- |
| `hp0` | Hp 0, stop (one red; two side by side on an H/V departure signal; a distant signal under it goes dark) |
| `hp1`, `hp2` | Hp 1 proceed, Hp 2 proceed slowly (green; green over yellow) |
| `ks1`, `ks2` | Ks 1 proceed, Ks 2 proceed, expect stop |
| `vr0`, `vr1`, `vr2` | Vr 0 expect stop, Vr 1 expect proceed, Vr 2 expect proceed slowly |
| `sh0`, `sh1` | Sh 0 stop, Sh 1 shunting allowed |

Lamps a signal may or may not have are classes too: `sh-lamp` (Sh 1 on a Ks signal), `one-red` (an H/V departure signal's Hp 0 with one red), `extra-lamp` (the upper white, the distant signal's Zusatzlicht, lit with `short`). A combined signal takes both aspects, such as `hp1 vr0`.

Classes combine, for example `route diverging` or `sh-lamp hp0 sh1`. A straight track, sensor or train number along a platform also takes `platform-top`, `platform-bottom` or both: platform bands from the track to the tile's edge, so two tracks a row apart with their facing halves make an island platform. On the map and in the editor every turnout, signal and sensor has its name in small text beside it, on the side away from a signal's head or a turnout's diverging leg.

## About other SpDrS60 drawings

Rocrail's SpDrS60 theme and the spdrs60 program draw the same prototype, so the tiles look alike. Their files are GPL and are not used here, neither copied nor traced.
