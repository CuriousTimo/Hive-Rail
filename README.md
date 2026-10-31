# Hive-Rail

<p align="center">
	<img src="assets/hive-rail-logo.svg" alt="Hive-Rail logo: a rail junction inside a honeycomb hexagon" width="420">
</p>

Hive-Rail is being built to help separate groups of model railway modules operate together as one layout. Instead of relying on one central computer, the plan is for a small computer to look after each powered section of track and share information with the computers looking after neighboring sections.

> **Hive-Rail: distributed train tracking that keeps working until the last mesh node dies.** Each node follows the trains of its own district, block by block, also trains driven by hand without a route; every node keeps a copy of where all trains are; and a train takes its identity along when it crosses into the next district. That is why Hive-Rail needs to know the modules put together in each district. See [Distributed train tracking](https://github.com/CuriousTimo/Hive-Rail/wiki/Hive%E2%80%90Rail:-Architecture-and-Design-Specification#10-distributed-train-tracking).

The nodes speak [RocNet over Zenoh](https://github.com/CuriousTimo/Hive-Rail/wiki/RocNet-on-the-Mesh). Each node connects its own command station and equipment through an adapter: DCC-EX (expected in most clusters), LocoNet centrals such as the Intellibox, and Rocrail. Support for FRED/Fredi walk-around throttles has started: the shared loco register and throttle numbers work; dispatch is next.

> **EARLY BETA: USE AT YOUR OWN RISK.** This software has not been tested on real hardware. It is not production-ready or a safety system. Do not connect it to or use it to control a live layout.

This is a very early simulation and networking prototype. Each node runs a simulated command station, or drives a DCC-EX command station over USB serial or WiFi with `--dccex`; it has been tested with DCC-EX on an ESP32 and on the 2021 node board prototype on USB power only, so no track has been powered. There is no connection to a booster, track sensor, signal, or relay. It cannot drive a train or protect a layout. Rocrail or JMRI can connect to a node over LocoNet TCP for power and the fast clock. A node can also join a LocoNet central's bus through a serial interface, such as the RS232 port of an Intellibox I; this is not yet tested with a real Intellibox. A node or the dispatcher can read its options from a TOML config file. Its RocNet support carries power, short and clock frames over Zenoh; it is not connected to a local RocNet transport yet.

Zenoh peer scouting discovers nodes on a single local network using `224.0.0.224:7446`, distinct from RocNet/RocRail's `224.0.0.1:4321`, which a node joins only when it runs the RocNet UDP adapter. Peer discovery and heartbeat recovery have been smoke-tested between macOS, Windows, and Linux computers on one LAN. Cross-subnet discovery has not been tested.

## Intended use

Think of each future Hive-Rail node as a small signal box for one part of a modular layout. Its intended job is to:

- Keep track of which neighboring sections are connected and responding.
- Leave a boundary signal at **STOP** when a neighbor is missing or a power problem is reported.
- Coordinate a locomotive handoff between separately powered sections. The design aims to prevent two boosters from powering the same rail at once.
- Allow only one controller at a time to drive each locomotive.
- Give every district its own power on/off and report its power state and short circuits to every node, so the rest of the layout keeps running. Share one fast clock across all nodes.
- Later: serve FRED/Fredi throttles on every module as one distributed command station.

These are goals, not finished features. Today this beta can check a module profile, check a simple booster-exclusivity example, exchange network heartbeats between computers, switch simulated district power (with a global emergency stop and a global go), report simulated shorts to every node, run one fast clock across the mesh, keep one loco register for the whole layout (translating Rocrail's or a central's own slots by loco address), drive a DCC-EX command station over USB or WiFi, and join a LocoNet bus through a node board or a serial interface. A lite node on an ESP32 (the M5StickC test device) joins the mesh as a district. It cannot yet carry out the railway-control jobs above.

## Intended computers

- **Development, simulation, and observer computers:** Windows 10 or later, macOS, and Linux.
- **Computers intended to live with the layout:** Linux single-board computers such as Raspberry Pi or Orange Pi, connected to suitable railway control hardware.

These are intended targets, not a claim of full tested compatibility. GitHub CI builds and tests on Windows, macOS, and Linux; networked runtime has been smoke-tested on all three desktop platforms on one LAN. Raspberry Pi/Orange Pi builds have not been tested, and no command station, booster, sensor, or other real railway hardware has been tested.

The detailed design, unresolved engineering questions, [RocNet mesh design](https://github.com/CuriousTimo/Hive-Rail/wiki/RocNet-on-the-Mesh), [DCC-EX adapter](https://github.com/CuriousTimo/Hive-Rail/wiki/DCC-EX-Adapter), [LocoNet adapter](https://github.com/CuriousTimo/Hive-Rail/wiki/LocoNet-Adapter), [transition zone specification](https://github.com/CuriousTimo/Hive-Rail/wiki/Transition-Zone-Specification) and [RocNet compatibility matrix](https://github.com/CuriousTimo/Hive-Rail/wiki/RocNet-Compatibility-Matrix) are in the [Hive-Rail wiki](https://github.com/CuriousTimo/Hive-Rail/wiki).

## Planned phases

The roadmap follows the [wiki design specification](https://github.com/CuriousTimo/Hive-Rail/wiki/Hive%E2%80%90Rail:-Architecture-and-Design-Specification). Check items as they are completed; a phase is complete only when its gate is met.

- [ ] **Phase 0: Bench spike**
	- [ ] **0.1** Bench-test the transition zone (latching relay and entry sensor) with real decoders, including decoder keep-alive behavior.
	- [ ] **0.2** Capture and record DCC-EX short-circuit output for the firmware and shield in use.
	- [ ] **0.3** Measure Zenoh heartbeat and liveliness detection times on Ethernet and Wi-Fi.
	- [ ] **0.4** Record results and resolve the Phase 0 open questions in the wiki.
- [ ] **Phase 1: Core data**
	- [x] **1.1** Load and deserialize the version 1 module profile JSON.
	- [x] **1.2** Check the schema version, required identity fields, duplicate port IDs, and independent transfer-block sensor/relay requirements.
	- [ ] **1.3** Complete validation for all profile, module, port, and device constraints. Module and layout files (schema version 2, [Module and Layout Files](https://github.com/CuriousTimo/Hive-Rail/wiki/Module-and-Layout-Files)) load and are checked with `hive-rail --check-layout <layout.json>`: track graph, blocks, devices, stations, end profiles and track counts at each connection, every module in one district, address clashes within a district (LocoNet sensors and switches apart), district borders only single or double track, a transfer block on each border track, changed module files. `examples/layout/` holds a double-track meeting with a station with a passing siding and a border between two districts.
	- [ ] **1.8** Module library, district files from several owners (shared by email, USB stick, SSH, NFS or SMB), export and import, publishing on the mesh, and the planning team's lock ([Module and Layout Files §9](https://github.com/CuriousTimo/Hive-Rail/wiki/Module-and-Layout-Files#9-the-meeting-step-by-step)).
	- [ ] **1.4** Implement two-sided binding confirmation and link status.
	- [ ] **1.5** Build the topology graph and handle node restarts and loss.
	- [ ] **1.6** Add atomic configuration writes, backup recovery, and tests.
	- [x] **1.7** Read a node's or the dispatcher's options from a TOML config file (`--config`); every command line flag works in it and the command line wins. The first part of the [node file](https://github.com/CuriousTimo/Hive-Rail/wiki/Module-and-Layout-Files#4-the-node-file); examples in `examples/config/`.
- [ ] **Phase T: Distributed train tracking** (the core idea; after Phase 1, before Phase 5; see [Architecture §10](https://github.com/CuriousTimo/Hive-Rail/wiki/Hive%E2%80%90Rail:-Architecture-and-Design-Specification#10-distributed-train-tracking))
	- [ ] **T.1** Give the module profile blocks, occupancy sensors and turnouts, and build each district's block graph from its modules (with 1.3 to 1.5).
	- [ ] **T.2** Follow trains within a district: placing, moving into the next block by direction and turnout positions, freeing the old block, gaps between detected blocks, ghosts and ambiguities to the operator.
	- [ ] **T.3** Replicate the train picture (loco, district, block, speed, direction, functions) on every node, with versions; only the owning district changes a train; merge after a split or restart.
	- [ ] **T.4** Save the train picture to disk, so the first node to start after all were off reads it back and the operator confirms it. The loco register can be saved first.
	- [ ] **T.5** Name the train at the border from the tracking, the loco register or the operator, and carry its identity through the handoff (with Phase 5).
	- [ ] **T.6** Pass: a train followed through a simulated district and across a simulated border, by route and driven by hand; the picture survives a node restart and a network split.
- [ ] **Phase 2: Simulator**
	- [ ] **2.1** Model virtual nodes, sensors, turnouts, boosters, and transfer blocks. Trains run on a layout's track graph: `hive-rail --simulate-layout <layout.json>` runs two trains shuttling between the example's termini, setting turnouts as a route would, waiting for occupied track, closing and opening level crossings (closed until the last train has passed), and crossing the district border both ways; every block, sensor and turnout event carries its LocoNet address. Not yet connected to the nodes.
	- [x] **2.2** Implement and unit-test a local booster-exclusivity check for fixed sample rail segments.
	- [ ] **2.3** Enforce rail power exclusivity after every simulated event.
	- [ ] **2.4** Add deterministic network, sensor, short-circuit, crash, and restart fault injection.
	- [ ] **2.5** Run seeded scenarios and verify the simulation invariants.
- [ ] **Phase 3: Mesh and safety**
	- [x] **3.1** Exchange Zenoh heartbeat messages between two nodes; smoke-tested over loopback.
	- [x] **3.2** Verify heartbeat timeout and recovery between macOS, Windows, and Linux nodes on one LAN; cross-subnet behavior remains untested.
	- [x] **3.3** Add native Zenoh multicast peer scouting; smoke-tested across three desktop OSes on one LAN.
	- [ ] **3.4** Implement confirmed node bindings and link status.
	- [ ] **3.5** Keep boundary signals at Stop unless every proceed condition is fresh and healthy.
	- [ ] **3.6** Add the SafetyActor and priority emergency command path.
	- [ ] **3.7** Test short-circuit propagation and record measured timing.
	- [x] **3.8** Relay and decode RocNet locomotive frames from Windows and Linux to macOS over Zenoh.
	- [ ] **3.9** Test bidirectional RocNet message carriage and message/request behavior over Zenoh.
- [ ] **Phase 4: RocNet mesh** (current focus, in this order)
	- [x] **4.1** Switch power per district and report every district's power and short circuits to all nodes as RocNet status events; tested with Rocrail over LocoNet TCP.
	- [x] **4.2** Share one fast clock across the mesh (RocNet clock group), commanded by Hive-Rail, shown on every node and written to LocoNet slot 123; tested between two nodes. Rocrail following the clock is still to be tested.
	- [ ] **4.3** Keep the CLI view clear: node and power overview, event pane, decoded RocNet and LocoNet log, `--plain` output. Partly done: dashboard with district power, fast clock and neighbour table; decoded log lines.
	- [ ] **4.4** Carry binary RocNet frames on routed Zenoh keys, with origin metadata for loop prevention.
	- [ ] **4.5** Add the RocNet UDP adapter for Rocrail, with one bridge per LAN segment.
	- [x] **4.6** Drive the district's locos on DCC-EX from the loco register (`<t>` speed and direction, `<F>` F0 to F8, `<- cab>` once freed); tested with the transfer test tool on DCC-EX v5.6.6. See the [DCC-EX adapter](https://github.com/CuriousTimo/Hive-Rail/wiki/DCC-EX-Adapter#31-locos).
	- [ ] **4.7** Pass stage 1: MacBook and Windows nodes with Rocrail.
- [ ] **Phase 5: Handoff and lease**
	- [ ] **5.1** Implement locomotive district ownership and leases, with LocoNet throttles exempt from the dead-man timeout.
	- [ ] **5.2** Implement the transition zone state machine, seam arbitration, and the latching relay driver.
	- [ ] **5.3** Test both travel directions, reversals, timeouts, and faults F1-F8.
	- [ ] **5.4** Pass the simulation exit criteria before connecting hardware.
- [ ] **Phase 6: Hardware drivers**
	- [ ] **6.1** Implement and replay-test the DCC-EX serial driver using recorded transcripts. Power, emergency stop, short (overload) reports and the fast clock are done over USB serial or WiFi TCP and tested on an ESP32 without motor shield; on the 2021 board power follows the MAIN tracks, including its stop and go buttons; loco commands and a recorded real short are not.
	- [ ] **6.2** Add the LocoNet adapter (codec, serial interface and LocoNet over TCP); pass stage 2 with an Intellibox I (65000) and IntelliSwitch. Codec, LocoNet over TCP (text and binary clients), the serial interface (`--loconet-serial`: the Intellibox I's RS232 port in LocoNet mode, or a LocoBuffer) and LocoNet over UDP multicast (`--loconet-udp`: a GCA101 LocoBuffer-UDP) are done; the serial interface is not yet tested with a real Intellibox; the UDP interface is tested with a real GCA101, an Intellibox I and an IB-Switch (receiving and sending). Stage 2 through the GCA101 on 30 October 2026: district power both ways, taking and releasing locos on the Intellibox's slot server, a conflict with the Intellibox's own throttle, the purge and the keep-alive, and turnout answers when the node is the command station; see [Stage 2 on an Intellibox I](#stage-2-on-an-intellibox-i).
	- [ ] **6.3** Bench-test on an isolated layout section before any wider trial.
	- [ ] **6.4** Add local RocNet adapters for compatible module equipment.
	- [ ] **6.5** Build the [Hive-Rail node board](https://github.com/CuriousTimo/Hive-Rail/wiki/Hive-Rail-Node-Board): an ESP32 with DCC-EX, LocoNet-B and LocoNet-T, transfer relays and sensor, stop and go buttons and status LEDs, linked to Hive-Rail on a Pi or PC; prototype on the 2021 ESP32 LocoNet Command Station board. Step 1 is done: stock DCC-EX with the board definition, a Hive-Rail add-on with its own OLED screen (fast clock, district power, mesh, slots, last loco, link status), the status LEDs and the stop and go buttons (`firmware/node-board/`, built with `firmware/node-board/build.sh`).
	- [ ] **6.6** Build the **Hive-Rail board v2** in phases B0 to B8 ([node board §13](https://github.com/CuriousTimo/Hive-Rail/wiki/Hive-Rail-Node-Board#13-phases)): a full DCC-EX command station with LocoNet-T and -B, CDE and B-BUS booster outputs, multiMAUS and the Hive-Rail lite node, on two ESP32s. Usable at home on its own, with Rocrail or JMRI, or as a booster behind a Z21 or Intellibox (with a DCC-to-LocoNet bridge), and at meetings as a lite node or with a Pi. Schematic draft and parts list: [`hardware/board-v2/`](hardware/board-v2/).
	- [x] **6.7** Prove the lite node (phase B0): zenoh-pico on an ESP32 sends a heartbeat that the dispatcher shows as a district. Done 2026-10-31 on an M5StickC ([`firmware/node-lite`](firmware/node-lite/), [Node-Lite on the M5StickC](https://github.com/CuriousTimo/Hive-Rail/wiki/Node-Lite-M5StickC)): joins as a lite node through a full node (`--listen tcp/0.0.0.0:7447`), about 177 KB heap free and 6 % / 15 % CPU, district power on its buttons, global stop and go both ways, updates over Wi-Fi. Lite nodes never take the shared roles.
	- [ ] **6.9** Optional **current budget admission** ([Transition Zone §10](https://github.com/CuriousTimo/Hive-Rail/wiki/Transition-Zone-Specification#10-current-budget-admission-optional)), for districts that measure their current: a district near its limit holds inbound trains at the border (FIFO, or priority by train type with aging). Steps: DCC-EX current (`<JI>`, `<JG>`) in the heartbeat and on the dispatcher; then the layout simulator; then the real handover.
	- [ ] **6.8** Lite node next steps on the M5StickC. Done 2026-10-31: **lite servers** (`--lite-server` on a node or the dispatcher: Zenoh in router mode, found by the lite nodes' scouting) with failover between them, tested without a missed heartbeat; the district's layout over the mesh (`--layout <file>` answers `hive/<session>/layout/district/<name>`); the simulator stops the trains of any district without power; the three-district bench layout `examples/layout/bench-m5.json`. Next: track faked by the layout simulator as the M5StickC's hardware; a scenario driver injecting shorts, stop and go, global stop and go and clock changes; load balancing across lite servers.
- [ ] **Phase 7: Peripherals**
	- [x] **7.1** Add a terminal dashboard for node, neighbor heartbeat state, district power and fast clock.
	- [ ] **7.2** Add operator controls after the safety model is implemented.
	- [ ] **7.3** Add ESP32 accessory controls and displays after validating the gateway approach.
	- [ ] **7.4** Add other non-safety-critical displays and timetable features.
	- [ ] **7.6** Web view of the layout on every node: the modules placed by joining their ends and drawn from their geometry (SVG), live blocks, turnouts, level crossings, building lights and trains; `?modules=` for a screen next to one station. Running on the layout simulator (`--web-sim`); still to come: the node's real data from the mesh, a symbol library with the owners' own SVGs, and a schematic track diagram in the SpDrS60 style for the dispatcher.
	- [x] **7.5** Add a dispatcher screen for the Zugleiter: every district, the fast clock and the layout's loco register with a sync check per node, and the events of every district marked with its name and RocNet address; `--show-locos` prints the register once.
- [ ] **Phase 8: FRED throttles** (started; the [node board](https://github.com/CuriousTimo/Hive-Rail/wiki/Hive-Rail-Node-Board) makes Hive-Rail the only slot master on each module LocoNet)
	- [x] **8.1** Share one **loco register** across the mesh, by loco address. Locos on a throttle bus get a layout-wide **throttle number** from one allocator (the lowest online node), so a FRED plugged in again on another module gets its loco back. LocoNet connections with their own slot master (Rocrail's slot server, a central) keep their own slots and are translated by address; tested with three nodes and Rocrail.
	- [x] **8.7** Take a loco that enters a district on that district's slot master (null move; once the slot master confirms it, the slot is marked with the node's throttle ID and gets the register's speed and functions; kept alive every 50 s) and release it when it leaves; tested with Rocrail's slot server, an Intellibox I and the `--transfer-loco` test tool. Rocrail only drives locos in its loco table; a central does not have that limit. See the [LocoNet adapter](https://github.com/CuriousTimo/Hive-Rail/wiki/LocoNet-Adapter).
	- [ ] **8.8** Answer turnout questions as the command station, so an IB-Switch or a Märklin Keyboard 6040 on [Marklin-6040-LocoNet](https://github.com/CuriousTimo/Arduino-ModelRail/tree/master/Marklin-6040-LocoNet) lights up on a node board with DCC-EX: `SW_STATE`, `SW_REQ` and `SW_ACK` answered as an Intellibox I does, positions learned on every bus (also from `SW_REP`), switch commands sent to DCC-EX as `<a>`. Built and tested as the command station for a LocoNet over TCP client with the IB-Switch's recorded messages; still to test with a real IB-Switch on a bus with Hive-Rail as the only command station, and `<a>` with a real accessory decoder (L-Q11). See [Stage 2 on an Intellibox I](#stage-2-on-an-intellibox-i).
	- [ ] **8.9** As the slot master, refuse a null move on a slot another throttle holds, as the Intellibox does (`LONG_ACK 3A 00`); check against a real throttle or an Intellibox in IB-Control mode (L-Q10).
	- [ ] **8.2** Complete the throttle flow: **dispatch** across connections (a FRED cannot select a loco, so it takes a loco dispatched by Rocrail, a DT100 or another throttle), purging idle throttle numbers (the LocoNet PE says about 200 s; an Intellibox I purges an untouched slot after about 2 minutes), slot updates for throttles with a display.
	- [ ] **8.3** Make throttle number allocation safe during a network split (blocks per node, as in the FRED design §7).
	- [ ] **8.4** Route a throttle's speed and functions to the district that holds the loco (needs 4.6 and 5.1).
	- [ ] **8.5** Measure the slot response time with a real FRED through the node board and the network.
	- [ ] **8.6** Walk-around test with real FREDs and Fredis.
	- [ ] **8.10** Roco's Z21 LAN protocol in the node, for the Z21 app and the wlanMAUS (software only).
	- [ ] **8.11** multiMAUS on a cable: the node as XpressNet master on the board's RS-485 port, commands by loco address into the loco register.

Phase 0 measurements are gates, not paperwork: timing assumptions and hardware behavior must be verified before safety or handoff features are treated as ready for a live layout.

## Requirements

- Rust stable toolchain with `cargo` and `rustc`
- Two computers on a network that permits TCP connections between them

## Build and test

```sh
cargo test
cargo run -- --mode simulation
cargo run -- --validate-profile examples/module_profile.json
```

**Config file:** `--config <file.toml>` reads the options from a TOML file instead of the command line. Each key is a flag without its dashes and a table is a flag prefix, so `[loconet]` with `serial = "/dev/cu.usbserial-0001"` is `--loconet-serial /dev/cu.usbserial-0001`; `true` and `false` switch an option on or off. Options on the command line win over the file, so one file serves several nodes (`--config bench.toml --id module-2`). [`examples/config/`](examples/config/) has one file per setup and a reference:

| File | Setup |
|---|---|
| [`hive-rail.toml`](examples/config/hive-rail.toml) | Every option with its default and what it does |
| [`intellibox.toml`](examples/config/intellibox.toml) | An Intellibox I (65000) as the slot master on LocoNet, through its RS232 port |
| [`gca101.toml`](examples/config/gca101.toml) | A LocoNet bus over UDP multicast through a GCA101 LocoBuffer-UDP (224.0.0.1:1235) |
| [`dccex-serial.toml`](examples/config/dccex-serial.toml) | DCC-EX over USB serial, with LocoNet over TCP for Rocrail or JMRI |
| [`dccex-wifi.toml`](examples/config/dccex-wifi.toml) | DCC-EX over WiFi (TCP), with LocoNet over TCP for Rocrail or JMRI |
| [`dispatcher.toml`](examples/config/dispatcher.toml) | The dispatcher screen for the Zugleiter, read-only |

```sh
cargo run -- --config examples/config/intellibox.toml
```

GitHub Actions runs formatting, Clippy, tests, and a build on Linux, macOS, and Windows for pull requests, beta tags, and manual runs (**Run workflow** on GitHub). Pushes to `main` do not run CI; run the checks locally instead. These checks cover desktop software only; they do not test Raspberry Pi or Orange Pi boards or any real railway hardware.

### Local checks and builds

On a Mac, the same checks and the cross-builds run locally without Docker. They need the Rust targets `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` and `x86_64-pc-windows-gnu`, plus [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild) with Zig (`pip3 install --user ziglang`) and, for packages, `cargo-deb`.

```sh
scripts/check.sh     # fmt, Clippy, tests, then release and test binaries in dist/<target>/
scripts/package.sh   # macOS arm64, Windows x86_64 and amd64/arm64 .deb in dist/packages/
```

`dist/` and `target/` are ignored by git. The Linux binaries need glibc 2.30 or later.

### Node board firmware

The [node board](https://github.com/CuriousTimo/Hive-Rail/wiki/Hive-Rail-Node-Board) runs stock DCC-EX with a Hive-Rail add-on from `firmware/node-board/`: the board definition and module name (`config.h`), the stop and go buttons (`myAutomation.h`), and the add-on with its own OLED screen and the status LEDs (`mySetup.h`, `myHiveRail.cpp`). A node with `--dccex` finds the add-on by itself and sends it a status line once a second, which is also the board's link heartbeat. The add-on also runs a **LocoNet buffer**: the board's LocoNet port (UART2, IO16/IO17) as an LbServer on TCP port 1234, next to DCC-EX. A node joins that bus with `--loconet-connect <board>[:1234]`; there the node is the slot master by default (`--loconet-connect-slots node`), for FREDs. Built, not yet tested on a powered LocoNet. DCC-EX itself is an unmodified checkout next to this repository; the script copies the add-on in and builds with PlatformIO.

```sh
git clone https://github.com/DCC-EX/CommandStation-EX.git ../CommandStation-EX
pip3 install --user platformio
firmware/node-board/build.sh                                    # build
firmware/node-board/build.sh --upload /dev/cu.usbserial-0001    # build and flash
```

Pushing a tag such as `v0.1.0-beta.1` runs the same checks, builds a native desktop executable on each platform, and publishes a GitHub prerelease with the executables and SHA-256 checksums. The macOS executable is ad-hoc signed to match the project's local Tauri test setup; it is not Developer ID signed or notarized. Gatekeeper may still require manual approval before opening a downloaded copy. These are simulation-only early-beta builds, not hardware-ready software.

## Run two networked simulation nodes

Start one node on each computer on the same LAN. Both must use the same session name and distinct node IDs. In peer mode, Zenoh's native multicast scouting discovers peers automatically; manual endpoints are optional overrides.

When run in an interactive terminal, node mode opens a dashboard with the fast clock, this district's power and the last power request, and the LocoNet status. The node table lists this node first, then the discovered neighbors with their RocNet ID, heartbeat age/state, sequence, and reported power state. Under each node it lists the locos driven through that node's LocoNet by address, with direction, speed and F0 to F4. Use `Up`/`Down` to select a node and `x` (or `Delete`) to remove a stale neighbor; it returns if its heartbeat resumes. The **Raw CLI output** pane retains up to 200 lines; use `PgUp`/`PgDn` to scroll, `Home` for the oldest line, and `End` to follow the latest. Press `q`, `Esc`, or `Ctrl+C` to exit. Add `--plain` for log output in scripts or non-interactive terminals.

On macOS:

```sh
cargo run -- --mode node --session bench --id MacBook
```

On Windows PowerShell:

```powershell
cargo run -- --mode node --session bench --id Windows
```

Allow Zenoh peer traffic through each computer's firewall, including multicast scouting at `224.0.0.224:7446` and the peer transport. This is intentionally separate from RocNet/RocRail's UDP multicast `224.0.0.1:4321`; Hive-Rail does not use RocRail's group for node discovery. The nodes publish a heartbeat every 250 ms on `hive/{session}/node/{id}/hb`. If a known peer's heartbeat is missing for 750 ms, the node logs it as stale. These are draft starting values from the wiki, not validated safety timings.

To send the RocNet identify request after a peer is online:

```sh
cargo run -- --mode node --session bench --id MacBook --rocnet-address 1 --identify
```

To relay the RocNet reference's unicast speed frame to locomotive address `291`:

```sh
cargo run -- --mode node --session bench --id MacBook --send-rocnet-hex '@0001230001020203304140'
```

The receiver prints the decoded frame. No physical RocNet UDP, TCP, MQTT, or serial interface is connected yet. Each node needs its own `--rocnet-address` (default 100); the dashboard warns when two nodes share one.

## District power and fast clock with Rocrail over LocoNet TCP

Every node runs a simulated command station for its own district. There is no layout-wide power switch: a power request only switches the district of the node that receives it, so the other districts keep running. The one exception is the global emergency stop (`E`), sent as a RocNet halt on `hive/{session}/rn/cs/all`: every district that is on stops its trains. The **global go** (`G`), a power-on request on the same key, ends it: every district in emergency stop goes back on, while districts that are off or shorted stay as they are. A district can also resume on its own with `1`. The dashboard's and the dispatcher's `GLOBAL` line shows `normal`, or who stopped the layout and when. Each node reports its district's power as a RocNet status event on `hive/{session}/rn/cs/node/{id}` and in its heartbeat, and every other node shows it in its neighbour table.

On the Windows PC, start the node with a LocoNet over TCP port for Rocrail:

```powershell
hive-rail.exe --mode node --session bench --id Windows --rocnet-address 102 --loconet-tcp
```

On the MacBook:

```sh
cargo run -- --mode node --session bench --id MacBook --rocnet-address 101
```

In Rocrail, add a LocoNet command station using LocoNet over TCP with host `localhost` and port `1234` (the default; `--loconet-tcp <port>` picks another). The node accepts both the LbServer text format and raw binary LocoNet, detected per client. **Rocrail settings:** assign the loco to this LocoNet interface (Rocrail only sends a loco over the interface it belongs to), keep Rocrail's own **slot server on**, and turn its LocoNet **clock sync off** (Hive-Rail commands the fast clock). With `--loconet-tcp-slots external`, the default, the slots on this connection belong to its own slot master: Rocrail's slot server, or a LocoNet central such as an Intellibox, a DR5000 or a Digitrax command station. The node answers no slot requests there; it learns which local slot holds which loco from the slot master's replies and enters the loco in the layout's **loco register** by address. Use `--loconet-tcp-slots node` for clients without their own slot master, such as simple throttles and the simulation: the node then answers the slot requests itself with the layout's throttle numbers. Rocrail's power on, power off and emergency stop then switch the district of the node it is connected to; the other node shows the change. In either dashboard:

| Key | Action |
| --- | --- |
| `1` | Power on for this node's district |
| `0` | Power off for this node's district |
| `h` | Emergency stop for this node's district |
| `E` | Global emergency stop: every district that is on halts its trains (power stays on) |
| `G` | Global go: every district in emergency stop back on (off and shorted districts stay as they are); one district alone resumes with `1` |
| `s` | Simulate a short in this node's district; other nodes show `SHORT` for it |
| `c` | Fast clock run or pause, on every node |
| `+` / `-` | Fast clock rate up or down (1 to 60) |
| `]` / `[` | Fast clock one hour forward or back |

**Fast clock:** Hive-Rail commands the clock. Start it with `c` on any node; every node shows it in the dashboard header and writes it to its LocoNet clients as slot 123, so Rocrail and throttles follow. Turn off Rocrail's own LocoNet clock sync: clock writes from LocoNet clients are ignored and reported.

**DCC-EX:** add `--dccex <port>` to drive a real DCC-EX command station instead of the simulated one:

```sh
cargo run -- --mode node --session bench --id MacBook --rocnet-address 101 --dccex /dev/cu.usbserial-0001
```

Other targets: `--dccex /dev/cu.usbserial-0001` (macOS), `--dccex /dev/ttyUSB0` (Linux), `--dccex COM3` (Windows) or `--dccex 192.168.4.1:2560` (WiFi). The district then follows DCC-EX: power on goes to it as `<1 MAIN>` (a local programming track stays off), power off as `<0>` and emergency stop as `<!>`. The district's power is that of the MAIN tracks; their reports and overload messages come back as `ON`, `OFF` or `SHORT`, and DCC-EX shows the fast clock on its display. A power change Hive-Rail did not request, from a button or a WiThrottle, is reported to every node and sent to the LocoNet clients as GPON/GPOFF. Opening the serial port resets an ESP32, which boots with power off. Without a motor shield DCC-EX has no tracks, so it does not confirm power changes and its display keeps showing `PWR Off`. See the [DCC-EX adapter](https://github.com/CuriousTimo/Hive-Rail/wiki/DCC-EX-Adapter).

**Intellibox over its serial port:** `--loconet-serial <port>` joins a LocoNet bus through a serial LocoNet interface instead of a node board. On an Intellibox I (65000) set SO 2 = 4 (LocoNet on the interface instead of P50X), SO 5 = 1 (one stop bit) and SO 6 = 255 (the stop button does not drop CTS); the port then runs at 19200 baud with RTS/CTS, the defaults of `--loconet-serial-baud` and `--loconet-serial-flow`. A Mac needs a USB-RS232 adapter and a cable that carries RTS and CTS. The Intellibox is the slot master there (`--loconet-connect-slots external`, the default on a serial interface). The Intellibox I has no fast clock: add `--loconet-fast-clock off` so the node does not write slot 123 to it; its LocoNet over TCP clients still get the clock. The Intellibox does not echo what the node sends; an interface that does, such as a LocoBuffer, has its echo dropped. The serial connection itself is not yet tested with a real Intellibox; the Intellibox's behaviour on LocoNet is, through a GCA101 (see [Stage 2 on an Intellibox I](#stage-2-on-an-intellibox-i)).

```sh
cargo run -- --mode node --session bench --id MacBook --rocnet-address 101 --loconet-serial /dev/cu.usbserial-0001 --loconet-fast-clock off
```

**LocoNet over UDP (GCA101):** `--loconet-udp` joins a LocoNet bus over UDP multicast, as the [GCA101 LocoBuffer-UDP](https://www.rocrail-wiki.net/doku.php?id=mgv101-en) (MGV101) and Rocrail's LNUDP use it: each datagram to the group 224.0.0.1, port 1235, carries raw LocoNet messages, and the GCA101 passes them between the network and its LocoNet port. Give a group to use another one (`--loconet-udp 239.1.1.1:1235`). With Wi-Fi and Ethernet both connected, `--loconet-udp-interface <this computer's address>` picks the network the GCA101 is on. There is no connection: the node shows the bus as joined once it is in the group, and a GCA101 that is switched off is not noticed. The node renews its membership every half minute, so it keeps receiving after the computer moves to another Wi-Fi access point; some access points do not pass multicast between Wi-Fi and cable at all. The port is shared, so Rocrail or a monitor on the same computer can listen too. The node's own datagrams are not looped back to it; on macOS and Linux that also means other programs on the same computer do not see them directly, only the GCA101's bus echo if it sends one (the node drops that echo itself). As on the serial interface, the command station on the GCA101's LocoNet is the slot master. **Only one GCA101 per network:** every GCA101 uses the same multicast group, so two in one network would mix their LocoNet buses into one. That is a limit of the network, not of Hive-Rail, and it is not supported.

Tested on 30 October 2026 with a GCA101, an Intellibox I and an IB-Switch: the node received everything on that LocoNet (power on and off, switch commands, the IB-Switch's and the Intellibox's turnout questions and their answers, loco slots) and its own slot request reached the Intellibox, which answered. The log names the sender of each message (`from bus UDP 224.0.0.1:1235 (192.168.0.200:1235)`), so the GCA101's address can be found even when it is outside the computer's own network range: multicast reaches it on the same network all the same. The GCA101 also sends a datagram that is not LocoNet when its LocoNet changes, its announcement (`71 FF`, its address and netmask, `MV101`); the node logs it as one line, `not LocoNet: GCA101 announcement, 192.168.0.200 "MV101"`. What the IB-Switch and the Intellibox sent is written up, byte by byte, in [How an Uhlenbrock IB-Switch works on LocoNet](https://github.com/CuriousTimo/Arduino-ModelRail/blob/master/Marklin-6040-LocoNet/docs/ib-switch-analysis.md). [`examples/config/gca101.toml`](examples/config/gca101.toml) has the settings.

```sh
cargo run -- --mode node --session bench --id MacBook --rocnet-address 101 --loconet-udp --loconet-fast-clock off
```

### Stage 2 on an Intellibox I

Tested on 30 October 2026 through the GCA101, with an Intellibox I (65000) as the command station and an IB-Switch on its LocoNet, a node `MacBook` on the bus and a simulated node `Neighbour`; loco 4444 was moved between them with `--transfer-loco`. What the Intellibox did, and what the node does because of it:

| Test | The Intellibox | The node |
| --- | --- | --- |
| District power | Follows the node's `GPON` and `GPOFF` (its slots' track byte `07` and `06`); answers `GPON` with its own `GPON` | Follows the Intellibox's STOP and GO |
| Taking loco 4444 | `LOCO_ADR` → often `81 7E` (BUSY) first, then the slot, IDLE or COMMON. The null move → the slot IN_USE | Waits for that confirmation before it sends speed and functions; then writes the slot with its **throttle ID** (`H` and its RocNet address, `48 65` for 101), which the Intellibox keeps (`LONG_ACK 6F 7F`). No answer to `LOCO_ADR`: asked again every second, at most 20 times, as JMRI does |
| A loco on the Intellibox's own throttle (loco 3) | Refuses a null move on an IN_USE slot: `LONG_ACK 3A 00` | Not taken: a slot it saw driven, or with another throttle's ID, is a conflict (`WARNING … already IN USE`); a refused null move is one too, and nothing is sent to that slot. Several null moves at once are answered in order, so a refusal belongs to the oldest open one |
| Restart while holding a loco | The slot stays IN_USE, with the node's ID | Finds its own ID in the slot and carries on without a null move (which the Intellibox would refuse) |
| Releasing | `B5 <slot> 10` makes the slot COMMON; its speed steps and ID stay | Releases as COMMON, keeping the slot's speed steps |
| Purge | A COMMON or IN_USE slot nobody touches becomes IDLE after about 2 minutes (the LocoNet PE says 200 s). One that only had a null move stayed IN_USE for over 11 minutes | Keep-alive every 50 s, as JMRI's throttles |
| Fast clock | Slot 123 is an empty slot; a written time is not kept: **no fast clock** | `--loconet-fast-clock off` for the Intellibox; the node keeps the clock for its LocoNet over TCP clients |
| Slot 0 | Its ID bytes read `49 42`, "IB" | — |

**Turnouts, with the node as the command station.** On a side the node is the slot master of (`--loconet-connect-slots node` on a node board with DCC-EX, `--loconet-tcp-slots node` for simple throttles), the node now also answers for turnouts as the Intellibox does, so an IB-Switch or a Märklin Keyboard 6040 on [Marklin-6040-LocoNet](https://github.com/CuriousTimo/Arduino-ModelRail/tree/master/Marklin-6040-LocoNet) lights up there:

| Message | The node answers |
| --- | --- |
| `OPC_SW_STATE` (`BC`): where is this turnout? | `LONG_ACK 3C 30` closed or `3C 50` thrown, from the last switch command; a turnout never switched is thrown |
| `OPC_SW_REQ` (`B0`) | Nothing when taken; `LONG_ACK 30 00` (refused) without track power. The position is remembered either way, as on the Intellibox |
| `OPC_SW_ACK` (`BD`) | `LONG_ACK 3D 7F`, or `3D 00` without track power |
| `OPC_SW_REP` (`B1`), a decoder's output report | Remembered as the turnout's position |

The positions are learned from every switch command on either side. With `--dccex`, a switch command taken with track power on is also sent to DCC-EX as `<a turnout 1|0>` (closed 1), so the turnout really switches; not yet tested with a real accessory decoder. A Digitrax command station's start-up interrogate (switch commands to turnouts 1017 to 1020) is ignored, as JMRI does. Tested as the command station for a LocoNet over TCP client.

**One command station per bus.** Only one device may answer slot and turnout questions on a LocoNet: either the Intellibox (the node `external`, the default on a serial or UDP interface), or Hive-Rail (`node`, on a node board with DCC-EX). A test that made the node the command station for its TCP clients while the Intellibox was the one on the bus got two answers to every question, and once they differed. An Intellibox with the firmware for IB-Control mode works as a control unit on another command station; Hive-Rail as its slot server is still to be tested.

A short is reported, not spread: only the affected district loses power, and Rocrail sees power off if it is connected to that node. The neighbouring districts will later hold their boundary signals at Stop. Allow TCP port 1234 through the Windows firewall if Rocrail runs on another computer.

## Dispatcher screen (Zugleiter)

At a FREMO meeting the **Zugleiter** (train controller, or dispatcher) oversees the whole layout, while each **Fahrdienstleiter** runs a station. Hive-Rail has a screen for each: the node dashboard at a module, and the dispatcher screen for the whole session, on any computer in the same LAN:

```sh
cargo run -- --mode dispatcher --session bench            # full screen; --plain for a log
cargo run -- --config examples/config/dispatcher.toml     # the same from a config file
cargo run -- --show-locos --session bench                 # print the loco register once
cargo run -- --transfer-loco 4444 --to MacBook --session bench --speed 40 --lights  # test: put a loco into a district
```

The dispatcher shows every district (heartbeat, power, locos), the fast clock, the allocator and clock master (★), and the **loco register of the whole layout**: loco, throttle number (for locos on a throttle bus), status, speed, direction, F0 to F4, the node whose LocoNet drives it, and its version. Every node puts a checksum of its copy of the register in its heartbeat; the dispatcher shows per node whether it is in sync, and `LOCO REGISTER DIFFERS` if a copy stays different for more than 2 s. Below them, **the events of every district**, each marked with the district and its RocNet address (`15:20:48 MacBook (101)  Took loco 4444 on … slot master: slot 2, in use`): the same `EVENT` lines as on each node's own screen, published on `hive/<session>/node/<id>/event`. Its header has a `GLOBAL` line (`normal`, or `STOP by <who> at <time>` during a global stop), and it can send the global emergency stop `E` and the global go `G`, and set the fast clock (`c`, `+`/`-`, `[`/`]`). A lite node (a microcontroller such as the [Node-Lite M5StickC](https://github.com/CuriousTimo/Hive-Rail/wiki/Node-Lite-M5StickC)) shows as `199 → 101`, carried by node 101, with the register `lite: none`. The dispatcher sends no heartbeat, hands out no throttle numbers and is never the clock master; neither is a lite node. The node dashboard also marks the allocator with ★ and shows the register sync in its status line.

**Lite nodes** (microcontrollers, such as the [Node-Lite M5StickC](https://github.com/CuriousTimo/Hive-Rail/wiki/Node-Lite-M5StickC)) reach the mesh through **lite servers**: a node or the dispatcher started with `--lite-server` runs Zenoh as a router on `tcp/0.0.0.0:7447` (or `--listen`), and lite nodes find it by scouting, so they need no addresses. With two or more lite servers, a lite node moves to another when its server stops. Add `--layout <layout.json>` and the node also answers each district's request for its part of the layout:

```sh
cargo run -- --mode node --session bench --id MacBook --rocnet-address 101 --lite-server --layout examples/layout/bench-m5.json
cargo run -- --mode dispatcher --session bench --lite-server --listen tcp/0.0.0.0:7448
```

JMRI's LocoNet slot monitor works on connections where the node is the slot master (`--loconet-tcp-slots node`): the node answers slot reads from the throttle numbers, free slots included.

## Web view of the layout

A node serves a web page with the layout drawn from its module files, live: occupied blocks, turnout positions, level crossings, building lights and the trains with their names, plus an event log. Most nodes run headless under the layout, so the page is opened from any laptop, tablet or phone on the network; a node with a screen can show it in a browser's kiosk mode (for example `chromium --kiosk http://localhost:8080/?modules=timo-adorf`). For now it runs the layout simulator behind it:

```sh
cargo run -- --check-layout examples/layout/bench-double-track.json      # check a layout and its modules
cargo run -- --simulate-layout examples/layout/bench-double-track.json   # the simulation as text
cargo run -- --web-sim examples/layout/bench-double-track.json           # the same, live at http://localhost:8080
```

`--port` picks another port and `--speed 4` runs the simulation four times as fast. Scroll to zoom, drag to move, double-click to fit. `?modules=timo-adorf,timo-2e96-west` in the address shows only those modules, for a screen next to one station. The page is plain HTML, CSS and SVG built into the program (`web/`), with the live state as Server-Sent Events (`/events`) and the layout as JSON (`/api/layout`).

## Simulate four dual-track modules

`scripts/simulate-dual-track.sh` starts four nodes on this computer, `module-1` to `module-4`, as dual-track FREMO modules joined into a loop. Loco 3 and loco 10 run clockwise on track 1, loco 23 and loco 1234 counter-clockwise on track 2. A LocoNet throttle in the script drives each loco through the LocoNet over TCP port of the module it is on (12341 to 12344) and hands it to the next module on its track every few seconds: the receiving module takes the loco, and two seconds later the sending module stops it and frees its slot.

Two faults are injected on the way round: `module-3` is switched off from its LocoNet and on again, and `module-2` trips with a simulated short and is switched on again afterwards. A train only moves on when both districts have power; otherwise it waits at the boundary, as the boundary signals are meant to enforce later.

```sh
scripts/simulate-dual-track.sh              # observer dashboard; q stops the simulation
scripts/simulate-dual-track.sh --headless   # two laps, then PASS/FAIL checks
```

The headless run checks the node logs: the switched-off and shorted districts change state and recover, every other node sees each change, the shorted node tells its LocoNet clients power off, the other districts keep power, and no loco enters a district without power. It exits non-zero if a check fails. `DWELL`, `OVERLAP`, `LAPS`, `SESSION` and `BASE_PORT` change the timing, length, session and ports. With `--plain`, a node reads the dashboard keys one per line from piped stdin, which is how the script triggers the short. The handoff is driven by the throttle, not by Hive-Rail: the nodes have no handoff protocol yet.

This prototype currently tests Zenoh peer connectivity, heartbeat freshness, simulated district power and short reporting, and the fast clock. It does not yet test link-binding confirmation, command leases, handoff state machines, partitions, or the simulation invariants across nodes. Do not treat it as a safety system or use it to control a layout.