# zenoh-pico on an ESP32: phase B0 test

Does a microcontroller join the Hive-Rail mesh? This firmware is a zenoh-pico client on an
ESP32 that sends a Hive-Rail heartbeat (`hive/<session>/node/<id>/hb`, the Rust nodes' JSON,
every 250 ms) and subscribes to the other nodes' heartbeats. See the
[node board](https://github.com/CuriousTimo/Hive-Rail/wiki/Hive-Rail-Node-Board), phase B0.

## Run it

1. Copy `include/secrets.example.h` to `include/secrets.h` (not committed) and fill in the
   Wi-Fi network and the address of the computer running a node.
2. On that computer, start a node that listens for the ESP32:
   `hive-rail --mode node --session bench --id MacBook --rocnet-address 101 --listen tcp/0.0.0.0:7447`
3. Build and flash: `pio run -e m5stick-c -t upload` (or `-e esp32dev`), then `pio device monitor`.
   zenoh-pico's build needs CMake (`pip install cmake` next to PlatformIO).

## Result (2026-10-31, M5StickC: ESP32-PICO-D4, zenoh-pico 1.10.1, Rust zenoh 1.10.1)

| Question | Result |
| --- | --- |
| Does zenoh-pico on an original ESP32 talk to the Rust nodes? | **Yes.** Client over TCP to a node; same release (1.10.1) on both sides |
| Do the nodes see the ESP32 as a district? | **Yes.** The node logs `Peer ESP32-lite online (RocNet 199, seq 1, power OFF)` and keeps it online at 250 ms heartbeats (at 1 s it went stale after 3 missed heartbeats, as designed) |
| Does the rest of the mesh see it? | **Yes.** `--show-locos`, a separate peer not connected to the ESP32, reports 2 nodes: the node passes the client's messages on |
| Does the ESP32 receive the mesh? | **Yes.** Every heartbeat of the node, 4 per second |
| Memory | Static RAM 47 KB of 320 KB; free heap with Wi-Fi and the open session about **210 KB**, steady (lowest 207 KB) |
| Flash | **1.2 MB, 92 % of the default 1.25 MB app partition.** A full lite node needs the 3 MB "huge app" partition layout (4 MB flash) |

**Found:** the ESP32 became the session's **allocator and clock master** (`--show-locos`:
"allocator ESP32-lite"), because the shared roles go to the online node with the lowest id and
"ESP32-lite" sorts first. A lite node that cannot do those jobs must not get them: its heartbeat
needs a flag (for example `"lite": true`) that full nodes use to leave it out of the shared roles
(wiki NB-Q12).
