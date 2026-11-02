# Working on Hive-Rail

Hive-Rail is distributed train tracking for modular model railways: a node per district, sharing what it knows over a Zenoh mesh. The **design** lives in the [wiki](https://github.com/CuriousTimo/Hive-Rail/wiki); the **code** is documented in the source itself, with rustdoc.

## Where to start

| You want | Look at |
| --- | --- |
| What the system is meant to do, and why | The wiki: [Architecture](https://github.com/CuriousTimo/Hive-Rail/wiki/Hive%E2%80%90Rail:-Architecture-and-Design-Specification), [RocNet on the Mesh](https://github.com/CuriousTimo/Hive-Rail/wiki/RocNet-on-the-Mesh), [District Simulators and Train Tracking](https://github.com/CuriousTimo/Hive-Rail/wiki/District-Simulators-and-Train-Tracking) |
| The code: every module, what it does and how a report travels through them | The code documentation: `cargo doc --no-deps --document-private-items --open`, or on GitHub Pages once it is set up (below). The overview is at the top of `src/main.rs` |
| What works today and what is next | The roadmap in the [README](README.md) |
| How to run it | The README, and `--help` |

## Build and test

You need a stable Rust toolchain.

```sh
cargo test                     # unit tests
scripts/check.sh               # what CI checks: fmt, Clippy, tests, docs; then the cross-builds
cargo doc --no-deps --document-private-items --open
```

`scripts/check.sh` needs extra Rust targets and cargo-zigbuild for the cross-builds (see the README); the first part, fmt, Clippy, tests and docs, is what must pass before a change goes in:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --document-private-items
```

### Running nodes on simulated track

The district simulator plays one district's hardware as a Hive-Rail node board would, and real nodes run on it unchanged. The step tests start a small bench, drive a loco and check the logs:

```sh
cargo build
python3 tools/district_sim_step1.py    # one district
python3 tools/district_sim_step6.py    # two districts, the border held while the neighbour is off
```

Each step test is short; the bench they share is `tools/district_sim_harness.py`. `scripts/simulate-dual-track.sh --headless` runs four nodes as dual-track modules, and `scripts/bench-m5.sh` the three-district bench with the web view.

## House rules

- **Write for the next person.** Every module starts with a `//!` paragraph on what it is for; public items get a `///` line when the name does not say it all. Plain English, short sentences, the model railway words a module owner uses (district, block, turnout, border). Match the comments around you; do not narrate the code.
- **Clippy and rustdoc clean,** with `-D warnings`; `cargo fmt` as is.
- **Tests next to the code** (`#[cfg(test)] mod tests`), on the example layout in `examples/layout/` where they need one. Older tests still read `backup/examples-layout-v2/`; new ones should not.
- **The layout simulator (`src/layout_sim.rs`) keeps its behaviour.** It is the demo, the web view's engine and the reference. New simulation goes into the district simulator beside it.
- **Any bus.** A district's own bus can be LocoNet today and CAN or RocNet modules later: name devices by district, bus, address space and address (`src/district_track.rs`), never by a LocoNet number alone.
- **RocNet for what a RocNet tool should understand** (power, the clock, sensors), JSON over Zenoh for Hive-Rail's own state (heartbeats, the loco register, the train picture). See the wiki's Architecture §2.2.
- **Never guess on the track.** Where the node cannot know which train it is, it reports and holds; it does not pick one.
- **Safety is not claimed.** This is a beta; nothing here protects a live layout yet. Say what is tested and how.

## The code documentation on GitHub Pages

`.github/workflows/docs.yml` builds the documentation on every push to `main` that changes the code and publishes it on GitHub Pages. It needs one setting, once: **Settings → Pages → Source: GitHub Actions**.
