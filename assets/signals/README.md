# Signal faces

German light signals as they look on the track (Ril 301), drawn by Hive-Rail: the DB light signals one can buy for H0 with LEDs (Viessmann's H/V and Ks signals and light shunting signals; no semaphores yet), each signal type once, its lamps where the real signal has them. The SpDrS60 panel tiles (`assets/spdrs60/`) show a signal as the panel does, as a symbol with a red and a green lamp; these faces show the signal itself, for the web view and the panel editor (a selected signal shows its face and aspects).

Made by `tools/signal_faces.py`, which also writes `web/signal-faces.js`; change the generator, not the SVG files. Open `preview.html` to see every face in its aspects.

| File | Signal | Lamps |
| --- | --- | --- |
| `hv-block.svg` | H/V block signal (Viessmann 4011, 4721) | The tall H/V screen, bevelled top corners; red and green side by side in the lower row. Hp 0 one red, Hp 1 green |
| `hv-entry.svg` | H/V entry signal (4012, 4722) | Green at the top, red and yellow side by side below, the yellow under the green. Hp 0 one red, Hp 1 green, Hp 2 green and yellow |
| `hv-departure.svg` | H/V departure signal (4013, 4723) | Green top left, two reds side by side, yellow bottom left, Sh 1 two whites rising to the right. Hp 0 both reds (option `one-red`: only the left one, as when both reds are wired to one decoder output), Hp 1, Hp 2 green over yellow, Hp 0 + Sh 1 the left red and the whites |
| `hv-distant.svg` | H/V light distant signal (Lichtvorsignal) | A screen running diagonally, rising to the right, with four lamps in two horizontal pairs, yellow left and green right, one pair at the upper right and one at the lower left: Vr 0 both yellow, Vr 1 both green, Vr 2 the lower yellow and the upper green. The white Zusatzlicht for a shortened braking distance (Viessmann's marker light, 4030/4730; option `extra-lamp`) on a tab at the upper left; the mast under the middle |
| `hv-block-distant.svg`, `hv-entry-distant.svg`, `hv-departure-distant.svg` | The H/V main signals with the H/V distant signal below them on the same mast (Viessmann 4014/4724, 4015/4725, 4016/4726) | The main signal as above, the distant below without the Zusatzlicht; the distant is dark while the main shows Hp 0 |
| `ks-main.svg` | Ks main signal | The tall Ks screen (64 × 125 cm). Red top centre, green below it. A departure signal (Viessmann 4043) has Sh 1: two whites rising to the right, lower left and centre (option `sh-lamp`); an entry signal (4042) has not |
| `ks-multi.svg` | Ks multi-section signal (Mehrabschnittssignal) | Red top centre, green middle left, yellow middle right (Ks 2 is its distant function); Sh 1 as on the Ks main signal (Viessmann 4046 departure; 4045 entry without); the upper white (Zusatzlicht or Kennlicht) upper left |
| `ks-distant.svg` | Ks distant signal | Green left, yellow right at mid height, no red; the upper white (Zusatzlicht or Kennlicht) upper left (Viessmann 4040) |
| `sh-light.svg` | Light shunting signal (Lichtsperrsignal) | A wide screen with bevelled top corners: (Viessmann 4017/4727 high, 4018/4728 low) Sh 0 (now Hp 0) the two reds at the ends of the top row; Sh 1 the two whites, rising to the right (one between the reds, one below the left red) |

## Aspects and options

The aspect is a class, by its Ril 301 name: `hp0`, `hp1`, `hp2`, `vr0`, `vr1`, `vr2`, `ks1`, `ks2`, `sh0`, `sh1` (Sh 1 together with `hp0` on a main signal). Lamps a signal may or may not have are classes too: `sh-lamp` (Sh 1 on a Ks main or multi-section signal: a departure signal), `one-red` (an H/V departure signal showing Hp 0 with one red), and `extra-lamp` for the upper white (lit with `short`, on the H/V distant with Vr 0 and Vr 2, on Ks signals with Ks 2). The editor offers only the aspects a signal has the lamps for.

Not drawn, though the models have some of them: Zs 3 / Zs 3v speed indicators, flashing Ks 1, the Kennlicht lit alone. Semaphores and the signals of other railways (SBB, ÖBB) are left out.

Lamp positions follow the drawings of German signals on Wikimedia Commons ([Railway signals in Germany by number](https://commons.wikimedia.org/wiki/Category:Railway_signals_in_Germany_by_number): Hp0_Licht, Vr0/Vr1/Vr2_Licht and Sh0_Licht1, public domain; the Ks screen drawings Ks_(modèle).svg, Ks_h_Ks1.svg, Ks_v_Ks1.svg and Ks_v_Ks2.svg, whose 64 × 124 lamp positions the Ks faces use scaled; the German Wikipedia articles on Schutzsignale, Ks and H/V signals), sh1.org's signal pages and the German Wikipedia articles on the H/V and Ks signal systems. They are drawn here, not copied; real heads vary a little in shape and lamp arrangement.
