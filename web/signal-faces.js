// Made by tools/signal_faces.py; do not edit.
const SIGNAL_FACE_STYLE = "\n  .sf-screen { fill: #0d0d0d; stroke: #3a3a3a; stroke-width: 0.8; }\n  .sf-mast { fill: #8a8a8a; }\n  .sf-lamp { fill: #262626; stroke: #3c3c3c; stroke-width: 0.6; }\n  .sf-option { display: none; }\n  .sh-lamp .sf-option.sh-lamp, .extra-lamp .sf-option.extra-lamp { display: inline; }\n  /* H/V main */\n  .hp0 .sf-red1, .hp0 .sf-red2 { fill: #ff2a1f; }\n  .hp0.sh1 .sf-red2, .one-red.hp0 .sf-red2 { fill: #262626; }\n  .hp1 .sf-green, .hp2 .sf-green { fill: #2fe05a; }\n  .hp2 .sf-yellow { fill: #ffbf1a; }\n  .sh1 .sf-white { fill: #f6f6f6; }\n  /* H/V distant */\n  .vr0 .sf-ll.sf-vr-yellow, .vr0 .sf-ur.sf-vr-yellow, .vr2 .sf-ll.sf-vr-yellow { fill: #ffbf1a; }\n  .vr1 .sf-ll.sf-vr-green, .vr1 .sf-ur.sf-vr-green, .vr2 .sf-ur.sf-vr-green { fill: #2fe05a; }\n  .short .sf-extra { fill: #f6f6f6; }\n  .hp0 .sf-on-main .sf-vr-yellow, .hp0 .sf-on-main .sf-vr-green { fill: #262626; }\n  /* Ks */\n  .hp0 .sf-ks-red { fill: #ff2a1f; }\n  .ks1 .sf-ks-green { fill: #2fe05a; }\n  .ks2 .sf-ks-yellow { fill: #ffbf1a; }\n  /* light shunting signal */\n  .sh0 .sf-sh-red, .hp0 .sf-sh-red { fill: #ff2a1f; }\n";
const SIGNAL_FACES = {
 "hv-block": {
  "title": "H/V block signal (Blocksignal): Hp 0 (one red), Hp 1",
  "width": 40,
  "height": 100,
  "body": "<rect class=\"sf-mast\" x=\"18\" y=\"76\" width=\"4\" height=\"24\"/>\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"58\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"25.5\" cy=\"58\" r=\"4.4\"/>"
 },
 "hv-entry": {
  "title": "H/V entry signal (Einfahrsignal): Hp 0 (one red), Hp 1, Hp 2",
  "width": 40,
  "height": 100,
  "body": "<rect class=\"sf-mast\" x=\"18\" y=\"76\" width=\"4\" height=\"24\"/>\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"25.5\" cy=\"12\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"58\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-yellow\" cx=\"25.5\" cy=\"58\" r=\"4.4\"/>"
 },
 "hv-departure": {
  "title": "H/V departure signal (Ausfahrsignal): Hp 0 (two reds), Hp 1, Hp 2, Hp 0 + Sh 1",
  "width": 40,
  "height": 100,
  "body": "<rect class=\"sf-mast\" x=\"18\" y=\"76\" width=\"4\" height=\"24\"/>\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"11.5\" cy=\"15\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"29\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red2\" cx=\"28.5\" cy=\"29\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"29\" cy=\"41\" r=\"2.6\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"11\" cy=\"55\" r=\"2.6\"/>\n  <circle class=\"sf-lamp sf-yellow\" cx=\"11.5\" cy=\"65\" r=\"4.4\"/>"
 },
 "hv-distant": {
  "title": "H/V light distant signal (Lichtvorsignal): Vr 0, Vr 1, Vr 2; the white Zusatzlicht as an option",
  "width": 64,
  "height": 64,
  "body": "<rect class=\"sf-mast sf-own-mast\" x=\"28\" y=\"52\" width=\"4\" height=\"12\"/>\n  <circle class=\"sf-screen sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"4.6\"/>\n  <path class=\"sf-screen\" d=\"M31 11 H56 L60 15 V23 L28 55 H4 L0 51 V42 Z\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-yellow\" cx=\"41\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-green\" cx=\"51\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-yellow\" cx=\"10\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-green\" cx=\"20\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"2.2\"/>"
 },
 "ks-main": {
  "title": "Ks main signal: red, the green below it; Hp 0, Ks 1; Sh 1 (departure signals)",
  "width": 40,
  "height": 90,
  "body": "<rect class=\"sf-mast\" x=\"18\" y=\"77.5\" width=\"4\" height=\"12.5\"/>\n  <rect class=\"sf-screen\" x=\"0\" y=\"0\" width=\"40\" height=\"77.5\" rx=\"1.5\"/>\n  <circle class=\"sf-lamp sf-ks-red\" cx=\"20\" cy=\"20\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-ks-green\" cx=\"20\" cy=\"35.8\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"20\" cy=\"50.6\" r=\"2.3\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"7.2\" cy=\"61.7\" r=\"2.3\"/>"
 },
 "ks-multi": {
  "title": "Ks multi-section signal (Mehrabschnittssignal): Hp 0, Ks 1, Ks 2; Sh 1 (departure signals); the upper white",
  "width": 40,
  "height": 90,
  "body": "<rect class=\"sf-mast\" x=\"18\" y=\"77.5\" width=\"4\" height=\"12.5\"/>\n  <rect class=\"sf-screen\" x=\"0\" y=\"0\" width=\"40\" height=\"77.5\" rx=\"1.5\"/>\n  <circle class=\"sf-lamp sf-ks-red\" cx=\"20\" cy=\"20\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-ks-green\" cx=\"10.3\" cy=\"35.8\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-ks-yellow\" cx=\"29.7\" cy=\"35.8\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"10.3\" cy=\"9.1\" r=\"2.3\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"20\" cy=\"50.6\" r=\"2.3\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"7.2\" cy=\"61.7\" r=\"2.3\"/>"
 },
 "ks-distant": {
  "title": "Ks distant signal (Ks-Vorsignal): Ks 1 (green), Ks 2 (yellow); the upper white",
  "width": 40,
  "height": 90,
  "body": "<rect class=\"sf-mast\" x=\"18\" y=\"77.5\" width=\"4\" height=\"12.5\"/>\n  <rect class=\"sf-screen\" x=\"0\" y=\"0\" width=\"40\" height=\"77.5\" rx=\"1.5\"/>\n  <circle class=\"sf-lamp sf-ks-green\" cx=\"10.3\" cy=\"35.8\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-ks-yellow\" cx=\"29.7\" cy=\"35.8\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"10.3\" cy=\"9.1\" r=\"2.3\"/>"
 },
 "sh-light": {
  "title": "Light shunting signal (Lichtsperrsignal): Sh 0 (two reds), Sh 1 (two whites rising to the right)",
  "width": 48,
  "height": 34,
  "body": "<rect class=\"sf-mast\" x=\"22\" y=\"28\" width=\"4\" height=\"6\"/>\n  <path class=\"sf-screen\" d=\"M7 0 H41 L48 7 V28 H0 V7 Z\"/>\n  <circle class=\"sf-lamp sf-sh-red\" cx=\"11\" cy=\"8.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-sh-red\" cx=\"37\" cy=\"8.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"24\" cy=\"8.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"11\" cy=\"20\" r=\"4.2\"/>"
 },
 "hv-block-distant": {
  "title": "H/V block signal with the distant signal below it on the same mast: the main signal's aspects, with Vr 0, Vr 1 or Vr 2 (dark at Hp 0)",
  "width": 48,
  "height": 150,
  "body": "<rect class=\"sf-mast\" x=\"22\" y=\"76\" width=\"4\" height=\"74\"/>\n  <g transform=\"translate(4 0)\">\n\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"58\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"25.5\" cy=\"58\" r=\"4.4\"/>\n  </g>\n  <g class=\"sf-on-main\" transform=\"translate(2 94) scale(0.75)\">\n\n  <circle class=\"sf-screen sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"4.6\"/>\n  <path class=\"sf-screen\" d=\"M31 11 H56 L60 15 V23 L28 55 H4 L0 51 V42 Z\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-yellow\" cx=\"41\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-green\" cx=\"51\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-yellow\" cx=\"10\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-green\" cx=\"20\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"2.2\"/>\n  </g>"
 },
 "hv-entry-distant": {
  "title": "H/V entry signal with the distant signal below it on the same mast: the main signal's aspects, with Vr 0, Vr 1 or Vr 2 (dark at Hp 0)",
  "width": 48,
  "height": 150,
  "body": "<rect class=\"sf-mast\" x=\"22\" y=\"76\" width=\"4\" height=\"74\"/>\n  <g transform=\"translate(4 0)\">\n\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"25.5\" cy=\"12\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"58\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-yellow\" cx=\"25.5\" cy=\"58\" r=\"4.4\"/>\n  </g>\n  <g class=\"sf-on-main\" transform=\"translate(2 94) scale(0.75)\">\n\n  <circle class=\"sf-screen sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"4.6\"/>\n  <path class=\"sf-screen\" d=\"M31 11 H56 L60 15 V23 L28 55 H4 L0 51 V42 Z\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-yellow\" cx=\"41\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-green\" cx=\"51\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-yellow\" cx=\"10\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-green\" cx=\"20\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"2.2\"/>\n  </g>"
 },
 "hv-departure-distant": {
  "title": "H/V departure signal with the distant signal below it on the same mast: the main signal's aspects, with Vr 0, Vr 1 or Vr 2 (dark at Hp 0)",
  "width": 48,
  "height": 150,
  "body": "<rect class=\"sf-mast\" x=\"22\" y=\"76\" width=\"4\" height=\"74\"/>\n  <g transform=\"translate(4 0)\">\n\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"11.5\" cy=\"15\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"29\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red2\" cx=\"28.5\" cy=\"29\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"29\" cy=\"41\" r=\"2.6\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"11\" cy=\"55\" r=\"2.6\"/>\n  <circle class=\"sf-lamp sf-yellow\" cx=\"11.5\" cy=\"65\" r=\"4.4\"/>\n  </g>\n  <g class=\"sf-on-main\" transform=\"translate(2 94) scale(0.75)\">\n\n  <circle class=\"sf-screen sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"4.6\"/>\n  <path class=\"sf-screen\" d=\"M31 11 H56 L60 15 V23 L28 55 H4 L0 51 V42 Z\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-yellow\" cx=\"41\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-green\" cx=\"51\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-yellow\" cx=\"10\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-green\" cx=\"20\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"2.2\"/>\n  </g>"
 }
};
