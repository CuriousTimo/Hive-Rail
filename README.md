# Hive-Rail

<p align="center">
	<img src="assets/hive-rail-logo.svg" alt="Hive-Rail logo: a rail junction inside a honeycomb hexagon" width="420">
</p>

Hive-Rail is being built to help separate groups of model railway modules operate together as one layout. Instead of relying on one central computer, the plan is for a small computer to look after each powered section of track and share information with the computers looking after neighboring sections.

The nodes speak [RocNet over Zenoh](https://github.com/CuriousTimo/Hive-Rail/wiki/RocNet-on-the-Mesh). Each node connects its own command station and equipment through an adapter: DCC-EX (expected in most clusters), LocoNet centrals such as the Intellibox, and Rocrail. Support for FRED/Fredi walk-around throttles is designed but deferred.

> **EARLY BETA: USE AT YOUR OWN RISK.** This software has not been tested on real hardware. It is not production-ready or a safety system. Do not connect it to or use it to control a live layout.

This is a very early simulation and networking prototype. Each node runs a simulated command station; it has no connection to a real command station, booster, track sensor, signal, or relay. It cannot drive a train or protect a layout. Rocrail or JMRI can connect to a node over LocoNet TCP for power and the fast clock. Its RocNet support carries power, short and clock frames over Zenoh; it is not connected to a local RocNet transport yet.

Zenoh peer scouting discovers nodes on a single local network using `224.0.0.224:7446`, distinct from RocNet/RocRail's `224.0.0.1:4321`, which a node joins only when it runs the RocNet UDP adapter. Peer discovery and heartbeat recovery have been smoke-tested between macOS, Windows, and Linux computers on one LAN. Cross-subnet discovery has not been tested.

## Intended use

Think of each future Hive-Rail node as a small signal box for one part of a modular layout. Its intended job is to:

- Keep track of which neighboring sections are connected and responding.
- Leave a boundary signal at **STOP** when a neighbor is missing or a power problem is reported.
- Coordinate a locomotive handoff between separately powered sections. The design aims to prevent two boosters from powering the same rail at once.
- Allow only one controller at a time to drive each locomotive.
- Give every district its own power on/off and report its power state and short circuits to every node, so the rest of the layout keeps running. Share one fast clock across all nodes.
- Later: serve FRED/Fredi throttles on every module as one distributed command station.

These are goals, not finished features. Today this beta can check a module profile, check a simple booster-exclusivity example, exchange network heartbeats between computers, switch simulated district power (with a global emergency stop), report simulated shorts to every node, run one fast clock across the mesh, and talk LocoNet over TCP with Rocrail. It cannot yet carry out the railway-control jobs above.

## Intended computers

- **Development, simulation, and observer computers:** Windows 10 or later, macOS, and Linux.
- **Computers intended to live with the layout:** Linux single-board computers such as Raspberry Pi or Orange Pi, connected to suitable railway control hardware.

These are intended targets, not a claim of full tested compatibility. GitHub CI builds and tests on Windows, macOS, and Linux; networked runtime has been smoke-tested on all three desktop platforms on one LAN. Raspberry Pi/Orange Pi builds have not been tested, and no command station, booster, sensor, or other real railway hardware has been tested.

The detailed design, unresolved engineering questions, [RocNet mesh design](https://github.com/CuriousTimo/Hive-Rail/wiki/RocNet-on-the-Mesh), [LocoNet adapter](https://github.com/CuriousTimo/Hive-Rail/wiki/LocoNet-Adapter), [transition zone specification](https://github.com/CuriousTimo/Hive-Rail/wiki/Transition-Zone-Specification) and [RocNet compatibility matrix](https://github.com/CuriousTimo/Hive-Rail/wiki/RocNet-Compatibility-Matrix) are in the [Hive-Rail wiki](https://github.com/CuriousTimo/Hive-Rail/wiki).

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
	- [ ] **1.3** Complete validation for all profile, module, port, and device constraints.
	- [ ] **1.4** Implement two-sided binding confirmation and link status.
	- [ ] **1.5** Build the topology graph and handle node restarts and loss.
	- [ ] **1.6** Add atomic configuration writes, backup recovery, and tests.
- [ ] **Phase 2: Simulator**
	- [ ] **2.1** Model virtual nodes, sensors, turnouts, boosters, and transfer blocks.
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
	- [ ] **4.6** Route loco velocity and function commands to the district owner's simulated DCC-EX.
	- [ ] **4.7** Pass stage 1: MacBook and Windows nodes with Rocrail.
- [ ] **Phase 5: Handoff and lease**
	- [ ] **5.1** Implement locomotive district ownership and leases, with LocoNet throttles exempt from the dead-man timeout.
	- [ ] **5.2** Implement the transition zone state machine, seam arbitration, and the latching relay driver.
	- [ ] **5.3** Test both travel directions, reversals, timeouts, and faults F1-F8.
	- [ ] **5.4** Pass the simulation exit criteria before connecting hardware.
- [ ] **Phase 6: Hardware drivers**
	- [ ] **6.1** Implement and replay-test the DCC-EX serial driver using recorded transcripts.
	- [ ] **6.2** Add the LocoNet adapter (codec, USB serial and LocoNet over TCP); pass stage 2 with an Intellibox Basic and IntelliSwitch. Codec and LocoNet over TCP (text and binary clients) are done; USB serial is not.
	- [ ] **6.3** Bench-test on an isolated layout section before any wider trial.
	- [ ] **6.4** Add local RocNet adapters for compatible module equipment.
- [ ] **Phase 7: Peripherals**
	- [x] **7.1** Add a terminal dashboard for node, neighbor heartbeat state, district power and fast clock.
	- [ ] **7.2** Add operator controls after the safety model is implemented.
	- [ ] **7.3** Add ESP32 accessory controls and displays after validating the gateway approach.
	- [ ] **7.4** Add other non-safety-critical displays and timetable features.
- [ ] **Phase 8: FRED throttles** (deferred)
	- [ ] **8.1** Act as slot master on the module LocoNet: throttle slots, dispatch, purge, reconnecting FREDs.
	- [ ] **8.2** Walk-around test with real FREDs and Fredis.

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

GitHub Actions runs formatting, Clippy, tests, and a build on Linux, macOS, and Windows for pull requests, beta tags, and manual runs (**Run workflow** on GitHub). Pushes to `main` do not run CI; run the checks locally instead. These checks cover desktop software only; they do not test Raspberry Pi or Orange Pi boards or any real railway hardware.

### Local checks and builds

On a Mac, the same checks and the cross-builds run locally without Docker. They need the Rust targets `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` and `x86_64-pc-windows-gnu`, plus [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild) with Zig (`pip3 install --user ziglang`) and, for packages, `cargo-deb`.

```sh
scripts/check.sh     # fmt, Clippy, tests, then release and test binaries in dist/<target>/
scripts/package.sh   # macOS arm64, Windows x86_64 and amd64/arm64 .deb in dist/packages/
```

`dist/` and `target/` are ignored by git. The Linux binaries need glibc 2.30 or later.

Pushing a tag such as `v0.1.0-beta.1` runs the same checks, builds a native desktop executable on each platform, and publishes a GitHub prerelease with the executables and SHA-256 checksums. The macOS executable is ad-hoc signed to match the project's local Tauri test setup; it is not Developer ID signed or notarized. Gatekeeper may still require manual approval before opening a downloaded copy. These are simulation-only early-beta builds, not hardware-ready software.

## Run two networked simulation nodes

Start one node on each computer on the same LAN. Both must use the same session name and distinct node IDs. In peer mode, Zenoh's native multicast scouting discovers peers automatically; manual endpoints are optional overrides.

When run in an interactive terminal, node mode opens a dashboard with the fast clock, this district's power and the last power request, and the LocoNet status. The node table lists this node first, then the discovered neighbors with their RocNet ID, heartbeat age/state, sequence, and reported power state. Under each node it lists the locos driven through that node's LocoNet by address, with direction, speed and F0 to F4. Use `Up`/`Down` to select a node and `x` (or `Delete`) to remove a stale neighbor; it returns if its heartbeat resumes. The **Raw CLI output** pane retains up to 200 lines; use `PgUp`/`PgDn` to scroll, `Home` for the oldest line, and `End` to follow the latest. Press `q`, `Esc`, or `Ctrl+C` to exit. Add `--plain` for log output in scripts or non-interactive terminals.

On macOS:

```sh
cargo run -- --mode node --session bench --id mac
```

On Windows PowerShell:

```powershell
cargo run -- --mode node --session bench --id windows
```

Allow Zenoh peer traffic through each computer's firewall, including multicast scouting at `224.0.0.224:7446` and the peer transport. This is intentionally separate from RocNet/RocRail's UDP multicast `224.0.0.1:4321`; Hive-Rail does not use RocRail's group for node discovery. The nodes publish a heartbeat every 250 ms on `hive/{session}/node/{id}/hb`. If a known peer's heartbeat is missing for 750 ms, the node logs it as stale. These are draft starting values from the wiki, not validated safety timings.

To send the RocNet identify request after a peer is online:

```sh
cargo run -- --mode node --session bench --id mac --rocnet-address 1 --identify
```

To relay the RocNet reference's unicast speed frame to locomotive address `291`:

```sh
cargo run -- --mode node --session bench --id mac --send-rocnet-hex '@0001230001020203304140'
```

The receiver prints the decoded frame. No physical RocNet UDP, TCP, MQTT, or serial interface is connected yet. Each node needs its own `--rocnet-address` (default 100); the dashboard warns when two nodes share one.

## District power and fast clock with Rocrail over LocoNet TCP

Every node runs a simulated command station for its own district. There is no layout-wide power switch: a power request only switches the district of the node that receives it, so the other districts keep running. The one exception is the global emergency stop (`E`), sent as a RocNet halt on `hive/{session}/rn/cs/all`: every district stops its trains and then resumes on its own. Each node reports its district's power as a RocNet status event on `hive/{session}/rn/cs/node/{id}` and in its heartbeat, and every other node shows it in its neighbour table.

On the Windows PC, start the node with a LocoNet over TCP port for Rocrail:

```powershell
hive-rail.exe --mode node --session bench --id windows --rocnet-address 102 --loconet-tcp
```

On the MacBook:

```sh
cargo run -- --mode node --session bench --id mac --rocnet-address 101
```

In Rocrail, add a LocoNet command station using LocoNet over TCP with host `localhost` and port `1234` (the default; `--loconet-tcp <port>` picks another). The node accepts both the LbServer text format and raw binary LocoNet, detected per client. The node is the slot master for its district: it answers `LOCO_ADR` and assigns the slots, so turn off Rocrail's own LocoNet slot server; otherwise Rocrail never asks for a loco address and the dashboard cannot show it. Rocrail's power on, power off and emergency stop then switch the district of the node it is connected to; the other node shows the change. In either dashboard:

| Key | Action |
| --- | --- |
| `1` | Power on for this node's district |
| `0` | Power off for this node's district |
| `h` | Emergency stop for this node's district |
| `E` | Global emergency stop: every district halts its trains (power stays on); each district resumes with `1` |
| `s` | Simulate a short in this node's district; other nodes show `SHORT` for it |
| `c` | Fast clock run or pause, on every node |
| `+` / `-` | Fast clock rate up or down (1 to 60) |
| `]` / `[` | Fast clock one hour forward or back |

**Fast clock:** Hive-Rail commands the clock. Start it with `c` on any node; every node shows it in the dashboard header and writes it to its LocoNet clients as slot 123, so Rocrail and throttles follow. Turn off Rocrail's own LocoNet clock sync: clock writes from LocoNet clients are ignored and reported.

A short is reported, not spread: only the affected district loses power, and Rocrail sees power off if it is connected to that node. The neighbouring districts will later hold their boundary signals at Stop. Allow TCP port 1234 through the Windows firewall if Rocrail runs on another computer.

## Simulate four dual-track modules

`scripts/simulate-dual-track.sh` starts four nodes on this computer, `module-1` to `module-4`, as dual-track FREMO modules joined into a loop. Loco 3 and loco 10 run clockwise on track 1, loco 23 and loco 1234 counter-clockwise on track 2. A LocoNet throttle in the script drives each loco through the LocoNet over TCP port of the module it is on (12341 to 12344) and hands it to the next module on its track every few seconds: the receiving module takes the loco, and two seconds later the sending module stops it and frees its slot.

Two faults are injected on the way round: `module-3` is switched off from its LocoNet and on again, and `module-2` trips with a simulated short and is switched on again afterwards. A train only moves on when both districts have power; otherwise it waits at the boundary, as the boundary signals are meant to enforce later.

```sh
scripts/simulate-dual-track.sh              # observer dashboard; q stops the simulation
scripts/simulate-dual-track.sh --headless   # two laps, then PASS/FAIL checks
```

The headless run checks the node logs: the switched-off and shorted districts change state and recover, every other node sees each change, the shorted node tells its LocoNet clients power off, the other districts keep power, and no loco enters a district without power. It exits non-zero if a check fails. `DWELL`, `OVERLAP`, `LAPS`, `SESSION` and `BASE_PORT` change the timing, length, session and ports. With `--plain`, a node reads the dashboard keys one per line from piped stdin, which is how the script triggers the short. The handoff is driven by the throttle, not by Hive-Rail: the nodes have no handoff protocol yet.

This prototype currently tests Zenoh peer connectivity, heartbeat freshness, simulated district power and short reporting, and the fast clock. It does not yet test link-binding confirmation, command leases, handoff state machines, partitions, or the simulation invariants across nodes. Do not treat it as a safety system or use it to control a layout.