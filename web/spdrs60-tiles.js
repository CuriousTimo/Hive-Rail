// Made by tools/spdrs60_tiles.py from assets/spdrs60; do not edit.
const SPDRS60_SIZE = 32;
const SPDRS60_CELL_MM = 50;
const SPDRS60_STYLE = "\n  .sp-tile { fill: #c8c8c8; stroke: #a4a4a4; stroke-width: 0.4; }\n  .sp-bar { fill: none; stroke: #141414; stroke-width: 5; stroke-linecap: butt; stroke-linejoin: miter; }\n  .sp-band { fill: #141414; }\n  .sp-lamp { fill: none; stroke: #6f6f6f; stroke-width: 2; stroke-linecap: butt; stroke-linejoin: round; }\n  .route .sp-lamp { stroke: #f0d24a; }\n  .occupied .sp-lamp { stroke: #e2271c; }\n  .straight .sp-lamp.diverging, .diverging .sp-lamp.straight { stroke: #6f6f6f; }\n  .sp-pos { fill: none; stroke: #8a7a2a; stroke-width: 0.8; }\n  .straight .sp-pos.straight, .diverging .sp-pos.diverging { fill: #f0d24a; stroke: #6b5a10; }\n  .occupied.straight .sp-pos.straight, .occupied.diverging .sp-pos.diverging { fill: #e2271c; stroke: #6b1410; }\n  .sp-point { fill: #141414; stroke: #e8e8e8; stroke-width: 1.1; }\n  .sp-head { fill: #141414; }\n  .sp-mast { fill: none; stroke: #141414; stroke-width: 1.3; }\n  .sp-foot { fill: none; stroke: #8a7a2a; stroke-width: 0.8; }\n  .locked .sp-foot { fill: #f0d24a; }\n  .sp-road { fill: #707070; }\n  .sp-lc-lamp { fill: none; stroke: #8a7a2a; stroke-width: 0.8; }\n  .closed .sp-lc-lamp { fill: #f0d24a; }\n  .sp-joint { stroke: #c8c8c8; stroke-width: 1.2; }\n  .sp-district-a { fill: var(--district-a, #20c5b5); }\n  .sp-district-b { fill: var(--district-b, #a77bff); }\n  .sp-pf { display: none; fill: #e9e6dc; }\n  .sp-pf-edge { display: none; stroke: #f5f1e6; stroke-width: 1.1; stroke-dasharray: 3 2; }\n  .platform-top .sp-pf.sp-pf-top, .platform-bottom .sp-pf.sp-pf-bottom,\n  .platform-top .sp-pf-edge.sp-pf-top, .platform-bottom .sp-pf-edge.sp-pf-bottom { display: inline; }\n  .sp-plate { fill: #f6f6f6; stroke: #8e8e8e; stroke-width: 0.4; }\n  .sp-plate-text { fill: #141414; font: 7.5px sans-serif; text-anchor: middle; }\n  .sp-number-box { fill: #111111; stroke: #3c3c3c; stroke-width: 0.8; }\n  .sp-number { fill: #ff3b2f; font: bold 7.5px monospace; text-anchor: middle; }\n  .sp-post { fill: none; stroke: #3c3c3c; stroke-width: 1.4; stroke-linecap: round; }\n  .sp-bulb { fill: #e4e4e4; stroke: #3c3c3c; stroke-width: 0.8; }\n  .sp-glow { display: none; fill: #ffd34d; opacity: 0.35; }\n  .sp-socket { fill: #8e8e8e; stroke: #3c3c3c; stroke-width: 0.6; }\n  .sp-thread { fill: none; stroke: #3c3c3c; stroke-width: 0.6; }\n  .on .sp-bulb { fill: #ffd34d; }\n  .on .sp-glow { display: inline; }\n  .sp-mill { fill: #8b6b4a; stroke: #3c2e20; stroke-width: 0.8; }\n  .sp-sails { stroke: #3c2e20; stroke-width: 1; fill: #f3eee2; }\n  .on .sp-sails { animation: sp-spin 3s linear infinite; transform-origin: 16px 11px; }\n  .sp-action-box { fill: #1d2a3a; stroke: #5a6b80; stroke-width: 1; }\n  .sp-action-icon { fill: #8fa2b8; }\n  .on .sp-action-box { stroke: #ffd34d; stroke-width: 1.6; }\n  .on .sp-action-icon { fill: #ffd34d; }\n  @keyframes sp-spin { to { transform: rotate(360deg); } }\n  .sp-sensor-free { fill: #34d058; stroke: #141414; stroke-width: 0.9; }\n  .sp-sensor-on { display: none; }\n  .occupied .sp-sensor-free { display: none; }\n  .occupied .sp-sensor-on { display: inline; }\n  .sp-sensor-glow { fill: #c80000; stroke: #141414; stroke-width: 0.9; }\n  .sp-sensor-core { fill: #ff2a1a; }\n  .sp-sensor-hot { fill: #ff8a00; }\n\n  .sf-screen { fill: #0d0d0d; stroke: #3a3a3a; stroke-width: 0.8; }\n  .sf-mast { fill: #8a8a8a; }\n  .sf-lamp { fill: #262626; stroke: #3c3c3c; stroke-width: 0.6; }\n  .sf-option { display: none; }\n  .sh-lamp .sf-option.sh-lamp, .extra-lamp .sf-option.extra-lamp { display: inline; }\n  /* H/V main */\n  .hp0 .sf-red1, .hp0 .sf-red2 { fill: #ff2a1f; }\n  .hp0.sh1 .sf-red2, .one-red.hp0 .sf-red2 { fill: #262626; }\n  .hp1 .sf-green, .hp2 .sf-green { fill: #2fe05a; }\n  .hp2 .sf-yellow { fill: #ffbf1a; }\n  .sh1 .sf-white { fill: #f6f6f6; }\n  /* H/V distant */\n  .vr0 .sf-ll.sf-vr-yellow, .vr0 .sf-ur.sf-vr-yellow, .vr2 .sf-ll.sf-vr-yellow { fill: #ffbf1a; }\n  .vr1 .sf-ll.sf-vr-green, .vr1 .sf-ur.sf-vr-green, .vr2 .sf-ur.sf-vr-green { fill: #2fe05a; }\n  .short .sf-extra { fill: #f6f6f6; }\n  .hp0 .sf-on-main .sf-vr-yellow, .hp0 .sf-on-main .sf-vr-green { fill: #262626; }\n  /* Ks */\n  .hp0 .sf-ks-red { fill: #ff2a1f; }\n  .ks1 .sf-ks-green { fill: #2fe05a; }\n  .ks2 .sf-ks-yellow { fill: #ffbf1a; }\n  /* light shunting signal */\n  .sh0 .sf-sh-red, .hp0 .sf-sh-red { fill: #ff2a1f; }\n";
const SPDRS60_TILES = {
 "track-straight": {
  "title": "Track, straight; with platform above, below or both (platform-top, platform-bottom)",
  "body": "<rect class=\"sp-pf sp-pf-top\" x=\"0\" y=\"0\" width=\"32\" height=\"10.5\"/>\n  <rect class=\"sp-pf sp-pf-bottom\" x=\"0\" y=\"21.5\" width=\"32\" height=\"10.5\"/>\n  <path class=\"sp-pf-edge sp-pf-top\" d=\"M0 9.5 H32\"/>\n  <path class=\"sp-pf-edge sp-pf-bottom\" d=\"M0 22.5 H32\"/>\n  <path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <path class=\"sp-lamp\" d=\"M9 16 H23\"/>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "track-curve": {
  "title": "Track, 45-degree curve: from the middle of the left edge to the middle of the bottom edge",
  "body": "<polygon class=\"sp-band\" points=\"0,13.5 18.5,32 13.5,32 0,18.5\"/>\n  <path class=\"sp-lamp\" d=\"M4.5 20.5 L11.5 27.5\"/>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "S0"
   ]
  }
 },
 "turnout-right": {
  "title": "Turnout, right hand (Weiche rechts): trunk at the left, straight to the right, diverging down (to the right when running right); the point at the left",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <polygon class=\"sp-band\" points=\"0,13.5 18.5,32 13.5,32 0,18.5\"/>\n  <path class=\"sp-lamp straight\" d=\"M10 16 H25\"/>\n  <path class=\"sp-lamp diverging\" d=\"M7 22.5 L12 27.5\"/>\n  <circle class=\"sp-pos straight\" cx=\"24\" cy=\"9\" r=\"1.8\"/>\n  <circle class=\"sp-pos diverging\" cx=\"22\" cy=\"26\" r=\"1.8\"/>\n  <circle class=\"sp-point\" cx=\"5\" cy=\"16\" r=\"3.2\"/>",
  "cells": 1,
  "paths": {
   "straight": [
    "W0",
    "E0"
   ],
   "diverging": [
    "W0",
    "S0"
   ]
  }
 },
 "turnout-left": {
  "title": "Turnout, left hand (Weiche links): trunk at the left, straight to the right, diverging up (to the left when running right); the point at the left",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <polygon class=\"sp-band\" points=\"0,13.5 13.5,0 18.5,0 0,18.5\"/>\n  <path class=\"sp-lamp straight\" d=\"M10 16 H25\"/>\n  <path class=\"sp-lamp diverging\" d=\"M7 9.5 L12 4.5\"/>\n  <circle class=\"sp-pos straight\" cx=\"24\" cy=\"23\" r=\"1.8\"/>\n  <circle class=\"sp-pos diverging\" cx=\"22\" cy=\"6\" r=\"1.8\"/>\n  <circle class=\"sp-point\" cx=\"5\" cy=\"16\" r=\"3.2\"/>",
  "cells": 1,
  "paths": {
   "straight": [
    "W0",
    "E0"
   ],
   "diverging": [
    "W0",
    "N0"
   ]
  }
 },
 "crossing": {
  "title": "Crossing (Kreuzung), two tiles wide: straight track and a 45-degree track from the top of the right tile to the bottom of the left tile",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H64\"/>\n  <polygon class=\"sp-band\" points=\"45.5,0 13.5,32 18.5,32 50.5,0\"/>\n  <path class=\"sp-lamp\" d=\"M6 16 H22 M42 16 H58\"/>\n  <path class=\"sp-lamp\" d=\"M45 3 L39 9 M25 23 L19 29\"/>",
  "cells": 2,
  "paths": {
   "straight": [
    "W0",
    "E1"
   ],
   "diagonal": [
    "N1",
    "S0"
   ]
  }
 },
 "double-slip": {
  "title": "Double slip (Doppelkreuzungsweiche), two tiles wide: a crossing with both slip curves; the point in the middle",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H64\"/>\n  <polygon class=\"sp-band\" points=\"45.5,0 13.5,32 18.5,32 50.5,0\"/>\n  <path class=\"sp-lamp straight\" d=\"M5 16 H21 M43 16 H59\"/>\n  <path class=\"sp-lamp straight\" d=\"M46 2 L40 8 M24 24 L18 30\"/>\n  <path class=\"sp-lamp diverging\" d=\"M5 12.5 Q22 12.5 36 6.5 M28 25.5 Q42 19.5 59 19.5\"/>\n  <circle class=\"sp-pos straight\" cx=\"52\" cy=\"26\" r=\"1.8\"/>\n  <circle class=\"sp-pos diverging\" cx=\"12\" cy=\"6\" r=\"1.8\"/>\n  <circle class=\"sp-point\" cx=\"32\" cy=\"16\" r=\"3.4\"/>",
  "cells": 2,
  "paths": {
   "straight": [
    "W0",
    "E1"
   ],
   "diagonal": [
    "N1",
    "S0"
   ],
   "slip-up": [
    "W0",
    "N1"
   ],
   "slip-down": [
    "S0",
    "E1"
   ]
  }
 },
 "buffer-stop": {
  "title": "Buffer stop (Prellbock) at the right end",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H25\"/>\n  <rect class=\"sp-head\" x=\"25\" y=\"9.5\" width=\"3.5\" height=\"13\"/>\n  <path class=\"sp-lamp\" d=\"M7 16 H20\"/>",
  "cells": 1,
  "paths": {
   "end": [
    "W0"
   ]
  }
 },
 "sensor": {
  "title": "Track with an occupancy sensor (Rückmelder): a green dot while free, red while occupied; it can lie along a platform",
  "body": "<rect class=\"sp-pf sp-pf-top\" x=\"0\" y=\"0\" width=\"32\" height=\"10.5\"/>\n  <rect class=\"sp-pf sp-pf-bottom\" x=\"0\" y=\"21.5\" width=\"32\" height=\"10.5\"/>\n  <path class=\"sp-pf-edge sp-pf-top\" d=\"M0 9.5 H32\"/>\n  <path class=\"sp-pf-edge sp-pf-bottom\" d=\"M0 22.5 H32\"/>\n  <path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <path class=\"sp-lamp\" d=\"M4 16 H10 M22 16 H28\"/>\n  <circle class=\"sp-sensor-free\" cx=\"16\" cy=\"16\" r=\"3.6\"/>\n  <g class=\"sp-sensor-on\">\n    <circle class=\"sp-sensor-glow\" cx=\"16\" cy=\"16\" r=\"6.2\"/>\n    <circle class=\"sp-sensor-core\" cx=\"16\" cy=\"16\" r=\"4.2\"/>\n    <circle class=\"sp-sensor-hot\" cx=\"15\" cy=\"15\" r=\"1.8\"/>\n  </g>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "sensor-curve": {
  "title": "Curve with an occupancy sensor: the dot in the middle of the curve",
  "body": "<polygon class=\"sp-band\" points=\"0,13.5 18.5,32 13.5,32 0,18.5\"/>\n  <circle class=\"sp-sensor-free\" cx=\"8\" cy=\"24\" r=\"3.6\"/>\n  <g class=\"sp-sensor-on\">\n    <circle class=\"sp-sensor-glow\" cx=\"8\" cy=\"24\" r=\"6.2\"/>\n    <circle class=\"sp-sensor-core\" cx=\"8\" cy=\"24\" r=\"4.2\"/>\n    <circle class=\"sp-sensor-hot\" cx=\"7\" cy=\"23\" r=\"1.8\"/>\n  </g>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "S0"
   ]
  }
 },
 "signal-hv-block": {
  "title": "H/V block signal for trains running right: H/V block signal (Blocksignal): Hp 0 (one red), Hp 1",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <path class=\"sp-lamp\" d=\"M5 16 H27\"/>\n  <circle class=\"sp-foot\" cx=\"2.2\" cy=\"24.40\" r=\"1.5\"/>\n  <g class=\"face\" transform=\"translate(28.50 19.4) rotate(90) scale(0.2500)\">\n<rect class=\"sf-mast\" x=\"18\" y=\"76\" width=\"4\" height=\"24\"/>\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"58\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"25.5\" cy=\"58\" r=\"4.4\"/>\n  </g>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "signal-hv-entry": {
  "title": "H/V entry signal for trains running right: H/V entry signal (Einfahrsignal): Hp 0 (one red), Hp 1, Hp 2",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <path class=\"sp-lamp\" d=\"M5 16 H27\"/>\n  <circle class=\"sp-foot\" cx=\"2.2\" cy=\"24.40\" r=\"1.5\"/>\n  <g class=\"face\" transform=\"translate(28.50 19.4) rotate(90) scale(0.2500)\">\n<rect class=\"sf-mast\" x=\"18\" y=\"76\" width=\"4\" height=\"24\"/>\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"25.5\" cy=\"12\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"58\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-yellow\" cx=\"25.5\" cy=\"58\" r=\"4.4\"/>\n  </g>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "signal-hv-departure": {
  "title": "H/V departure signal for trains running right: H/V departure signal (Ausfahrsignal): Hp 0 (two reds), Hp 1, Hp 2, Hp 0 + Sh 1",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <path class=\"sp-lamp\" d=\"M5 16 H27\"/>\n  <circle class=\"sp-foot\" cx=\"2.2\" cy=\"24.40\" r=\"1.5\"/>\n  <g class=\"face\" transform=\"translate(28.50 19.4) rotate(90) scale(0.2500)\">\n<rect class=\"sf-mast\" x=\"18\" y=\"76\" width=\"4\" height=\"24\"/>\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"11.5\" cy=\"15\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"29\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red2\" cx=\"28.5\" cy=\"29\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"29\" cy=\"41\" r=\"2.6\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"11\" cy=\"55\" r=\"2.6\"/>\n  <circle class=\"sf-lamp sf-yellow\" cx=\"11.5\" cy=\"65\" r=\"4.4\"/>\n  </g>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "signal-hv-block-distant": {
  "title": "H/V block signal with the distant signal below it on the same mast, for trains running right; two tiles wide: H/V block signal with the distant signal below it on the same mast: the main signal's aspects, with Vr 0, Vr 1 or Vr 2 (dark at Hp 0)",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H64\"/>\n  <path class=\"sp-lamp\" d=\"M5 16 H59\"/>\n  <circle class=\"sp-foot\" cx=\"2.2\" cy=\"25.60\" r=\"1.5\"/>\n  <g class=\"face\" transform=\"translate(42.25 19.4) rotate(90) scale(0.2583)\">\n<rect class=\"sf-mast\" x=\"22\" y=\"76\" width=\"4\" height=\"74\"/>\n  <g transform=\"translate(4 0)\">\n\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"58\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"25.5\" cy=\"58\" r=\"4.4\"/>\n  </g>\n  <g class=\"sf-on-main\" transform=\"translate(2 94) scale(0.75)\">\n\n  <circle class=\"sf-screen sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"4.6\"/>\n  <path class=\"sf-screen\" d=\"M31 11 H56 L60 15 V23 L28 55 H4 L0 51 V42 Z\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-yellow\" cx=\"41\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-green\" cx=\"51\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-yellow\" cx=\"10\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-green\" cx=\"20\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"2.2\"/>\n  </g>\n  </g>",
  "cells": 2,
  "paths": {
   "through": [
    "W0",
    "E1"
   ]
  }
 },
 "signal-hv-entry-distant": {
  "title": "H/V entry signal with the distant signal below it on the same mast, for trains running right; two tiles wide: H/V entry signal with the distant signal below it on the same mast: the main signal's aspects, with Vr 0, Vr 1 or Vr 2 (dark at Hp 0)",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H64\"/>\n  <path class=\"sp-lamp\" d=\"M5 16 H59\"/>\n  <circle class=\"sp-foot\" cx=\"2.2\" cy=\"25.60\" r=\"1.5\"/>\n  <g class=\"face\" transform=\"translate(42.25 19.4) rotate(90) scale(0.2583)\">\n<rect class=\"sf-mast\" x=\"22\" y=\"76\" width=\"4\" height=\"74\"/>\n  <g transform=\"translate(4 0)\">\n\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"25.5\" cy=\"12\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"58\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-yellow\" cx=\"25.5\" cy=\"58\" r=\"4.4\"/>\n  </g>\n  <g class=\"sf-on-main\" transform=\"translate(2 94) scale(0.75)\">\n\n  <circle class=\"sf-screen sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"4.6\"/>\n  <path class=\"sf-screen\" d=\"M31 11 H56 L60 15 V23 L28 55 H4 L0 51 V42 Z\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-yellow\" cx=\"41\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-green\" cx=\"51\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-yellow\" cx=\"10\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-green\" cx=\"20\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"2.2\"/>\n  </g>\n  </g>",
  "cells": 2,
  "paths": {
   "through": [
    "W0",
    "E1"
   ]
  }
 },
 "signal-hv-departure-distant": {
  "title": "H/V departure signal with the distant signal below it on the same mast, for trains running right; two tiles wide: H/V departure signal with the distant signal below it on the same mast: the main signal's aspects, with Vr 0, Vr 1 or Vr 2 (dark at Hp 0)",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H64\"/>\n  <path class=\"sp-lamp\" d=\"M5 16 H59\"/>\n  <circle class=\"sp-foot\" cx=\"2.2\" cy=\"25.60\" r=\"1.5\"/>\n  <g class=\"face\" transform=\"translate(42.25 19.4) rotate(90) scale(0.2583)\">\n<rect class=\"sf-mast\" x=\"22\" y=\"76\" width=\"4\" height=\"74\"/>\n  <g transform=\"translate(4 0)\">\n\n  <path class=\"sf-screen\" d=\"M6 0 H34 L40 6 V76 H0 V6 Z\"/>\n  <circle class=\"sf-lamp sf-green\" cx=\"11.5\" cy=\"15\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red1\" cx=\"11.5\" cy=\"29\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-red2\" cx=\"28.5\" cy=\"29\" r=\"4.4\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"29\" cy=\"41\" r=\"2.6\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"11\" cy=\"55\" r=\"2.6\"/>\n  <circle class=\"sf-lamp sf-yellow\" cx=\"11.5\" cy=\"65\" r=\"4.4\"/>\n  </g>\n  <g class=\"sf-on-main\" transform=\"translate(2 94) scale(0.75)\">\n\n  <circle class=\"sf-screen sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"4.6\"/>\n  <path class=\"sf-screen\" d=\"M31 11 H56 L60 15 V23 L28 55 H4 L0 51 V42 Z\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-yellow\" cx=\"41\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-green\" cx=\"51\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-yellow\" cx=\"10\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-green\" cx=\"20\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"2.2\"/>\n  </g>\n  </g>",
  "cells": 2,
  "paths": {
   "through": [
    "W0",
    "E1"
   ]
  }
 },
 "signal-hv-distant": {
  "title": "H/V distant signal for trains running right: H/V light distant signal (Lichtvorsignal): Vr 0, Vr 1, Vr 2; the white Zusatzlicht as an option",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <path class=\"sp-lamp\" d=\"M5 16 H27\"/>\n  <circle class=\"sp-foot\" cx=\"2.2\" cy=\"25.60\" r=\"1.5\"/>\n  <g class=\"face\" transform=\"translate(15.90 19.4) rotate(90) scale(0.1938)\">\n<rect class=\"sf-mast sf-own-mast\" x=\"28\" y=\"52\" width=\"4\" height=\"12\"/>\n  <circle class=\"sf-screen sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"4.6\"/>\n  <path class=\"sf-screen\" d=\"M31 11 H56 L60 15 V23 L28 55 H4 L0 51 V42 Z\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-yellow\" cx=\"41\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ur sf-vr-green\" cx=\"51\" cy=\"17.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-yellow\" cx=\"10\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-ll sf-vr-green\" cx=\"20\" cy=\"48.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"22\" cy=\"18\" r=\"2.2\"/>\n  </g>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "signal-ks-main": {
  "title": "Ks main signal for trains running right: Ks main signal: red, the green below it; Hp 0, Ks 1; Sh 1 (departure signals)",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <path class=\"sp-lamp\" d=\"M5 16 H27\"/>\n  <circle class=\"sp-foot\" cx=\"2.2\" cy=\"24.96\" r=\"1.5\"/>\n  <g class=\"face\" transform=\"translate(28.50 19.4) rotate(90) scale(0.2778)\">\n<rect class=\"sf-mast\" x=\"18\" y=\"77.5\" width=\"4\" height=\"12.5\"/>\n  <rect class=\"sf-screen\" x=\"0\" y=\"0\" width=\"40\" height=\"77.5\" rx=\"1.5\"/>\n  <circle class=\"sf-lamp sf-ks-red\" cx=\"20\" cy=\"20\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-ks-green\" cx=\"20\" cy=\"35.8\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"20\" cy=\"50.6\" r=\"2.3\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"7.2\" cy=\"61.7\" r=\"2.3\"/>\n  </g>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "signal-ks-multi": {
  "title": "Ks multi-section signal for trains running right: Ks multi-section signal (Mehrabschnittssignal): Hp 0, Ks 1, Ks 2; Sh 1 (departure signals); the upper white",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <path class=\"sp-lamp\" d=\"M5 16 H27\"/>\n  <circle class=\"sp-foot\" cx=\"2.2\" cy=\"24.96\" r=\"1.5\"/>\n  <g class=\"face\" transform=\"translate(28.50 19.4) rotate(90) scale(0.2778)\">\n<rect class=\"sf-mast\" x=\"18\" y=\"77.5\" width=\"4\" height=\"12.5\"/>\n  <rect class=\"sf-screen\" x=\"0\" y=\"0\" width=\"40\" height=\"77.5\" rx=\"1.5\"/>\n  <circle class=\"sf-lamp sf-ks-red\" cx=\"20\" cy=\"20\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-ks-green\" cx=\"10.3\" cy=\"35.8\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-ks-yellow\" cx=\"29.7\" cy=\"35.8\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"10.3\" cy=\"9.1\" r=\"2.3\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"20\" cy=\"50.6\" r=\"2.3\"/>\n  <circle class=\"sf-lamp sf-white sf-option sh-lamp\" cx=\"7.2\" cy=\"61.7\" r=\"2.3\"/>\n  </g>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "signal-ks-distant": {
  "title": "Ks distant signal for trains running right: Ks distant signal (Ks-Vorsignal): Ks 1 (green), Ks 2 (yellow); the upper white",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <path class=\"sp-lamp\" d=\"M5 16 H27\"/>\n  <circle class=\"sp-foot\" cx=\"2.2\" cy=\"24.96\" r=\"1.5\"/>\n  <g class=\"face\" transform=\"translate(28.50 19.4) rotate(90) scale(0.2778)\">\n<rect class=\"sf-mast\" x=\"18\" y=\"77.5\" width=\"4\" height=\"12.5\"/>\n  <rect class=\"sf-screen\" x=\"0\" y=\"0\" width=\"40\" height=\"77.5\" rx=\"1.5\"/>\n  <circle class=\"sf-lamp sf-ks-green\" cx=\"10.3\" cy=\"35.8\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-ks-yellow\" cx=\"29.7\" cy=\"35.8\" r=\"4.7\"/>\n  <circle class=\"sf-lamp sf-extra sf-option extra-lamp\" cx=\"10.3\" cy=\"9.1\" r=\"2.3\"/>\n  </g>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "signal-sh": {
  "title": "Light shunting signal (Lichtsperrsignal) for trains running right: Light shunting signal (Lichtsperrsignal): Sh 0 (two reds), Sh 1 (two whites rising to the right)",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <path class=\"sp-lamp\" d=\"M5 16 H27\"/>\n  <circle class=\"sp-foot\" cx=\"2.2\" cy=\"25.60\" r=\"1.5\"/>\n  <g class=\"face\" transform=\"translate(12.28 19.4) rotate(90) scale(0.2583)\">\n<rect class=\"sf-mast\" x=\"22\" y=\"28\" width=\"4\" height=\"6\"/>\n  <path class=\"sf-screen\" d=\"M7 0 H41 L48 7 V28 H0 V7 Z\"/>\n  <circle class=\"sf-lamp sf-sh-red\" cx=\"11\" cy=\"8.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-sh-red\" cx=\"37\" cy=\"8.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"24\" cy=\"8.5\" r=\"4.2\"/>\n  <circle class=\"sf-lamp sf-white\" cx=\"11\" cy=\"20\" r=\"4.2\"/>\n  </g>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "level-crossing": {
  "title": "Level crossing (Bahnübergang): the road across the track, lamps lit while closed",
  "body": "<rect class=\"sp-road\" x=\"11.5\" y=\"0\" width=\"9\" height=\"32\"/>\n  <path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <path class=\"sp-lamp\" d=\"M3 16 H9 M23 16 H29\"/>\n  <circle class=\"sp-lc-lamp\" cx=\"6\" cy=\"6\" r=\"1.9\"/>\n  <circle class=\"sp-lc-lamp\" cx=\"26\" cy=\"26\" r=\"1.9\"/>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "district-border": {
  "title": "District border (Hive-Rail): a section joint with each side's district colour",
  "body": "<path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <path class=\"sp-lamp\" d=\"M3 16 H13 M19 16 H29\"/>\n  <path class=\"sp-joint\" d=\"M16 12 V20\"/>\n  <rect class=\"sp-district-a\" x=\"3\" y=\"22.5\" width=\"10\" height=\"3\"/>\n  <rect class=\"sp-district-b\" x=\"19\" y=\"22.5\" width=\"10\" height=\"3\"/>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "train-number": {
  "title": "Train number field (Zugnummernmelder) on the track; the page sets its number; it can lie along a platform",
  "body": "<rect class=\"sp-pf sp-pf-top\" x=\"0\" y=\"0\" width=\"32\" height=\"10.5\"/>\n  <rect class=\"sp-pf sp-pf-bottom\" x=\"0\" y=\"21.5\" width=\"32\" height=\"10.5\"/>\n  <path class=\"sp-pf-edge sp-pf-top\" d=\"M0 9.5 H32\"/>\n  <path class=\"sp-pf-edge sp-pf-bottom\" d=\"M0 22.5 H32\"/>\n  <path class=\"sp-bar\" d=\"M0 16 H32\"/>\n  <rect class=\"sp-number-box\" x=\"2\" y=\"11\" width=\"28\" height=\"10\" rx=\"1.2\"/>\n  <text class=\"sp-number\" x=\"16\" y=\"18.8\"></text>",
  "cells": 1,
  "paths": {
   "through": [
    "W0",
    "E0"
   ]
  }
 },
 "lamp": {
  "title": "Lamp (street or building light): lit while on",
  "body": "<circle class=\"sp-glow\" cx=\"16\" cy=\"12\" r=\"12\"/>\n  <path class=\"sp-bulb\" d=\"M12.5 19.2 A8 8 0 1 1 19.5 19.2 L19 22 H13 Z\"/>\n  <rect class=\"sp-socket\" x=\"13\" y=\"22\" width=\"6\" height=\"5\" rx=\"0.6\"/>\n  <path class=\"sp-thread\" d=\"M13 23.7 H19 M13 25.4 H19\"/>\n  <path class=\"sp-socket\" d=\"M14.5 27 H17.5 L16.6 29 H15.4 Z\"/>",
  "cells": 1,
  "paths": {}
 },
 "windmill": {
  "title": "Windmill: its sails turn while it runs",
  "body": "<path class=\"sp-mill\" d=\"M12 29 L14 13 H18 L20 29 Z\"/>\n  <g class=\"sp-sails\">\n    <path d=\"M16 11 L16 1.5 L19 2.5 Z M16 11 L25.5 11 L24.5 14 Z M16 11 L16 20.5 L13 19.5 Z M16 11 L6.5 11 L7.5 8 Z\"/>\n    <circle cx=\"16\" cy=\"11\" r=\"1.4\" fill=\"#3c2e20\"/>\n  </g>",
  "cells": 1,
  "paths": {}
 },
 "action": {
  "title": "Action (a Viessmann eMotion scene or another animated accessory): lit while it runs",
  "body": "<rect class=\"sp-action-box\" x=\"5\" y=\"5\" width=\"22\" height=\"22\" rx=\"4\"/>\n  <path class=\"sp-action-icon\" d=\"M13 10 L22 16 L13 22 Z\"/>\n  <path class=\"sp-action-icon\" d=\"M8.5 9 l1 2 l2 1 l-2 1 l-1 2 l-1 -2 l-2 -1 l2 -1 Z\"/>",
  "cells": 1,
  "paths": {}
 },
 "plate": {
  "title": "Name plate (track, signal or turnout name) in its own cell; the page sets its text",
  "body": "<rect class=\"sp-plate\" x=\"2\" y=\"10\" width=\"28\" height=\"12\"/>\n  <text class=\"sp-plate-text\" x=\"16\" y=\"18.6\"></text>",
  "cells": 1,
  "paths": {}
 }
};
