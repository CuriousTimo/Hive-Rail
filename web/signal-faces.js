// Made by tools/signal_faces.py; do not edit.
const SIGNAL_FACE_STYLE = "\n  .sf-screen { fill: #0d0d0d; stroke: #3a3a3a; stroke-width: 0.8; }\n  .sf-mast { fill: #8a8a8a; }\n  .sf-lamp { fill: #262626; stroke: #3c3c3c; stroke-width: 0.6; }\n  .sf-option { display: none; }\n  .sh-lamp .sf-option.sh-lamp, .zs1-lamp .sf-option.zs1-lamp, .extra-lamp .sf-option.extra-lamp { display: inline; }\n  /* H/V main */\n  .hp0 .sf-red1, .hp0 .sf-red2 { fill: #ff2a1f; }\n  .hp0.sh1 .sf-red2 { fill: #262626; }\n  .hp1 .sf-green, .hp2 .sf-green { fill: #2fe05a; }\n  .hp2 .sf-yellow { fill: #ffbf1a; }\n  .sh1 .sf-white { fill: #f6f6f6; }\n  .zs1 .sf-zs1 { fill: #f6f6f6; }\n  /* H/V distant */\n  .vr0 .sf-ll, .vr0 .sf-ur, .vr2 .sf-ll { fill: #ffbf1a; }\n  .vr1 .sf-ll, .vr1 .sf-ur, .vr2 .sf-ur { fill: #2fe05a; }\n  .short .sf-extra { fill: #f6f6f6; }\n  .hp0 .sf-on-main .sf-ll, .hp0 .sf-on-main .sf-ur { fill: #262626; }\n  /* Ks */\n  .hp0 .sf-ks-red { fill: #ff2a1f; }\n  .ks1 .sf-ks-green { fill: #2fe05a; }\n  .ks2 .sf-ks-yellow { fill: #ffbf1a; }\n  .zs1 .sf-ks-zs1 { fill: #f6f6f6; animation: sf-blink 1s steps(2, start) infinite; }\n  /* light shunting signal */\n  .sh0 .sf-sh-red, .hp0 .sf-sh-red { fill: #ff2a1f; }\n  @keyframes sf-blink { to { visibility: hidden; } }\n";
const SIGNAL_FACES = {
 "hv-main": {
  "title": "H/V light main signal (Lichthauptsignal): Hp 0 (two reds), Hp 1, Hp 2; Sh 1 and Zs 1 as options",
  "width": 40,
  "height": 100,
  "body": "<rect class=\"sf-mast\" x=\"18\" y=\"76\" width=\"4\" height=\"24\"/>\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"11.5\" cy=\"15\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"29\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red2\" cx=\"28.5\" cy=\"29\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"29\" cy=\"41\" r=\"2.6\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"11\" cy=\"55\" r=\"2.6\"/>\n  <circle class=\"sf-lamp sf-yellow\" cx=\"11.5\" cy=\"65\" r=\"4.4\"/>\n  <g class=\"sf-option zs1-lamp\">\n    <rect class=\"sf-screen\" x=\"8\" y=\"78\" width=\"24\" height=\"14\" rx=\"2\"/>\n    <circle class=\"sf-lamp sf-zs1\" cx=\"20\" cy=\"82\" r=\"2.2\"/>\n    <circle class=\"sf-lamp sf-zs1\" cx=\"13\" cy=\"88\" r=\"2.2\"/>\n    <circle class=\"sf-lamp sf-zs1\" cx=\"27\" cy=\"88\" r=\"2.2\"/>\n  </g>"
 },
 "hv-distant": {
  "title": "H/V light distant signal (Lichtvorsignal): Vr 0, Vr 1, Vr 2; the white Zusatzlicht as an option",
  "width": 64,
  "height": 64,
  "body": "<g transform=\"rotate(-45 32 32)\">\n    <circle class=\"sf-screen sf-option extra-lamp\" cx=\"32\" cy=\"14\" r=\"5\"/>\n    <path class=\"sf-screen\" d=\"M8 19 H56 L62 25 V39 L56 45 H8 L2 39 V25 Z\"/>\n    <circle class=\"sf-lamp sf-ll\" cx=\"14\" cy=\"32\" r=\"4.6\"/>\n    <circle class=\"sf-lamp sf-ur\" cx=\"50\" cy=\"32\" r=\"4.6\"/>\n    <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"32\" cy=\"14\" r=\"2.4\"/>\n  </g>"
 },
 "ks-main": {
  "title": "Ks main signal: red over green; Hp 0, Ks 1; Sh 1 and Zs 1 as options",
  "width": 40,
  "height": 66,
  "body": "<rect class=\"sf-mast\" x=\"18\" y=\"52\" width=\"4\" height=\"14\"/>\n  <rect class=\"sf-screen\" x=\"0\" y=\"0\" width=\"40\" height=\"52\" rx=\"1.5\"/>\n  <circle class=\"sf-lamp sf-ks-red\" cx=\"20\" cy=\"11\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-ks-green\" cx=\"20\" cy=\"25\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-white sf-ks-zs1 sf-option sh-lamp zs1-lamp\" cx=\"20\" cy=\"38\" r=\"2.4\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"7\" cy=\"46\" r=\"2.4\"/>"
 },
 "ks-multi": {
  "title": "Ks multi-section signal (Mehrabschnittssignal): Hp 0, Ks 1, Ks 2; Sh 1 and Zs 1 as options",
  "width": 40,
  "height": 56,
  "body": "<rect class=\"sf-mast\" x=\"18\" y=\"46\" width=\"4\" height=\"10\"/>\n  <rect class=\"sf-screen\" x=\"0\" y=\"0\" width=\"40\" height=\"46\" rx=\"1.5\"/>\n  <circle class=\"sf-lamp sf-ks-red\" cx=\"20\" cy=\"10\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-ks-green\" cx=\"9\" cy=\"24\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-ks-yellow\" cx=\"31\" cy=\"24\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-white sf-ks-zs1 sf-option sh-lamp zs1-lamp\" cx=\"20\" cy=\"33\" r=\"2.4\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"7\" cy=\"41\" r=\"2.4\"/>"
 },
 "ks-distant": {
  "title": "Ks distant signal (Ks-Vorsignal): Ks 1 (green), Ks 2 (yellow)",
  "width": 40,
  "height": 40,
  "body": "<rect class=\"sf-mast\" x=\"18\" y=\"30\" width=\"4\" height=\"10\"/>\n  <rect class=\"sf-screen\" x=\"0\" y=\"0\" width=\"40\" height=\"30\" rx=\"1.5\"/>\n  <circle class=\"sf-lamp sf-ks-green\" cx=\"11\" cy=\"15\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-ks-yellow\" cx=\"29\" cy=\"15\" r=\"4.4\"/>"
 },
 "sh-light": {
  "title": "Light shunting signal (Lichtsperrsignal): Sh 0 (two reds), Sh 1 (two whites rising to the right)",
  "width": 32,
  "height": 36,
  "body": "<rect class=\"sf-mast\" x=\"14\" y=\"28\" width=\"4\" height=\"8\"/>\n  <path class=\"sf-screen\" d=\"M4 0 H28 L32 4 V28 H0 V4 Z\"/>\n  <circle class=\"sf-lamp sf-sh-red\" cx=\"8\" cy=\"9\" r=\"3.4\"/>\n  <circle class=\"sf-lamp sf-sh-red\" cx=\"24\" cy=\"9\" r=\"3.4\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"23\" cy=\"15\" r=\"2.4\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"9\" cy=\"23\" r=\"2.4\"/>"
 },
 "hv-main-distant": {
  "title": "H/V main signal with the distant signal below it on the same mast: Hp 0 (distant dark), Hp 1 or Hp 2 with Vr 0, Vr 1 or Vr 2",
  "width": 48,
  "height": 150,
  "body": "<rect class=\"sf-mast\" x=\"22\" y=\"76\" width=\"4\" height=\"74\"/>\n  <g transform=\"translate(4 0)\">\n\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"11.5\" cy=\"15\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"29\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red2\" cx=\"28.5\" cy=\"29\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"29\" cy=\"41\" r=\"2.6\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"11\" cy=\"55\" r=\"2.6\"/>\n  <circle class=\"sf-lamp sf-yellow\" cx=\"11.5\" cy=\"65\" r=\"4.4\"/>\n  <g class=\"sf-option zs1-lamp\">\n    <rect class=\"sf-screen\" x=\"8\" y=\"78\" width=\"24\" height=\"14\" rx=\"2\"/>\n    <circle class=\"sf-lamp sf-zs1\" cx=\"20\" cy=\"82\" r=\"2.2\"/>\n    <circle class=\"sf-lamp sf-zs1\" cx=\"13\" cy=\"88\" r=\"2.2\"/>\n    <circle class=\"sf-lamp sf-zs1\" cx=\"27\" cy=\"88\" r=\"2.2\"/>\n  </g>\n  </g>\n  <g class=\"sf-on-main\" transform=\"translate(0 94) scale(0.75)\">\n<g transform=\"rotate(-45 32 32)\">\n    <circle class=\"sf-screen sf-option extra-lamp\" cx=\"32\" cy=\"14\" r=\"5\"/>\n    <path class=\"sf-screen\" d=\"M8 19 H56 L62 25 V39 L56 45 H8 L2 39 V25 Z\"/>\n    <circle class=\"sf-lamp sf-ll\" cx=\"14\" cy=\"32\" r=\"4.6\"/>\n    <circle class=\"sf-lamp sf-ur\" cx=\"50\" cy=\"32\" r=\"4.6\"/>\n    <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"32\" cy=\"14\" r=\"2.4\"/>\n  </g>\n  </g>"
 }
};
