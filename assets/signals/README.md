# Signal faces

German light signals as they look on the track (Ril 301), drawn by Hive-Rail: each signal type once, its lamps where the real signal has them. The SpDrS60 panel tiles (`assets/spdrs60/`) show a signal as the panel does, as a symbol with a red and a green lamp; these faces show the signal itself, for the web view and the panel editor (a selected signal shows its face and aspects).

Made by `tools/signal_faces.py`, which also writes `web/signal-faces.js`; change the generator, not the SVG files. Open `preview.html` to see every face in its aspects.

| File | Signal | Lamps |
| --- | --- | --- |
| `hv-main.svg` | H/V light main signal (Lichthauptsignal) | Tall screen, bevelled top corners: green top left, two reds side by side in the middle, yellow bottom left. Hp 0 shows both reds (with Sh 1 only the left one), Hp 1 green, Hp 2 green over yellow. Sh 1: two whites rising to the right. Zs 1: three whites in an upward triangle, in a small head below |
| `hv-distant.svg` | H/V light distant signal (Lichtvorsignal) | Screen tilted rising to the right, lamps lower left and upper right: Vr 0 yellow and yellow, Vr 1 green and green, Vr 2 yellow (lower left) and green (upper right). The white Zusatzlicht for a shortened braking distance on a tab at the upper left |
| `ks-main.svg` | Ks main signal | Red top centre, green middle left, yellow middle right. Sh 1: white lower left and centre. Zs 1: the centre white, flashing |
| `ks-distant.svg` | Ks distant signal | Green left, yellow right |
| `sh-light.svg` | Light shunting signal (Lichtsperrsignal) | Sh 0: two reds side by side. Sh 1: two whites rising to the right |

## Aspects and options

The aspect is a class, by its Ril 301 name: `hp0`, `hp1`, `hp2`, `vr0`, `vr1`, `vr2`, `ks1`, `ks2`, `sh0`, `sh1`, `zs1` (Sh 1 and Zs 1 together with `hp0`). Lamps a signal may or may not have are classes too: `sh-lamp`, `zs1-lamp`, and `extra-lamp` for the Zusatzlicht (lit with `short`).

Lamp positions follow the descriptions and diagrams of German signalling (for example sh1.org's signal pages and the German Wikipedia articles on the H/V and Ks signal systems); real heads vary a little in shape and lamp arrangement.
