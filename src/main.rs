//! # Hive-Rail
//!
//! Distributed train tracking for modular model railways: a small computer (a **node**)
//! looks after each powered **district** of a layout, and the nodes share what they know
//! over a **mesh** ([Zenoh](https://zenoh.io) on the LAN), speaking RocNet where a RocNet
//! tool such as Rocrail should understand it and JSON for Hive-Rail's own state. The
//! design is in the [wiki](https://github.com/CuriousTimo/Hive-Rail/wiki); this
//! documentation is the code's side of it.
//!
//! ## What the program runs
//!
//! One binary, chosen by its first option (see `--help`):
//!
//! | Option | What runs | Module |
//! | --- | --- | --- |
//! | `--mode node` | A district's node: power, the fast clock, the loco register, its command station and LocoNet, its own track and the trains on it | [`node`] |
//! | `--mode dispatcher` | The read-only screen of the whole layout (FREMO: Zugleiter) | [`dispatcher`] |
//! | `--web-sim` | The layout simulator with its web view and module editor | [`web`], [`layout_sim`] |
//! | `--sim-district` | One district's hardware, as a node board, for a node to run on | [`district_sim`] |
//! | `--check-layout`, `--simulate-layout` | A layout's checks, and the simulation as text | [`layout`], [`layout_sim`] |
//! | `--place-train`, `--transfer-loco`, `--sim-hw`, `--show-locos` | Test and operator tools on the mesh | [`dispatcher`] |
//!
//! ## The modules, by layer
//!
//! - **The layout:** [`layout`] reads module and layout files and checks them;
//!   [`tile_module`] derives a module's track from its SpDrS60 tiles (schema version 3);
//!   [`topology`] joins every module into one track graph and knows each device;
//!   [`layout_share`] sends each district its part; [`profile`] is the older module profile.
//! - **A node's own district:** [`district_track`] names what each bus address is;
//!   [`train_tracking`] follows trains block by block and hands them over at a border.
//! - **The mesh:** [`node`] (heartbeats, power, the global stop, sensors, the trains);
//!   [`locos`] (the loco register shared by every node); [`clock`] (the fast clock);
//!   [`rocnet`] (RocNet frames); [`power`] (district power states).
//! - **Hardware adapters:** [`dccex`] (DCC-EX over USB or WiFi); [`loconet`] (the LocoNet
//!   codec) with [`lbserver`], [`lbclient`], [`lnserial`] and [`lnudp`] (LocoNet over TCP,
//!   a serial interface, UDP multicast); [`turnouts`] (answering turnout questions as a
//!   command station does).
//! - **Screens:** [`ui`] (the node's terminal dashboard), [`dispatcher`], [`web`] and
//!   [`layout_view`] (the web view).
//! - **Simulation:** [`layout_sim`] plays a whole layout, logic included;
//!   [`district_sim`] plays one district's hardware and leaves the logic to the nodes;
//!   [`simulation`] is the first booster-exclusivity check (`--mode simulation`).
//!
//! ## How an occupancy report travels
//!
//! A detector on a district's LocoNet reports `OPC_INPUT_REP` → the node's LocoNet adapter
//! decodes it ([`loconet`]) → [`node`] reports it on the mesh as a RocNet sensor report and,
//! with `--layout`, [`district_track`] names it ("block timo-adorf/adorf_1_b occupied") →
//! [`train_tracking`] moves the train that can have entered that block, or reports a ghost
//! → the node publishes its train picture, which every other node keeps; a train reaching
//! a border is taken over by the neighbour's node, and held before it while that border is
//! not clear.
//!
//! ## Working on it
//!
//! `scripts/check.sh` runs what CI runs (fmt, Clippy, tests); `tools/district_sim_step1.py`
//! to `step6.py` run nodes on district simulators. See `CONTRIBUTING.md`.

mod clock;
mod config;
mod dccex;
mod dispatcher;
mod district_sim;
mod district_track;
mod layout;
mod layout_share;
mod layout_sim;
mod layout_view;
mod lbclient;
mod lbserver;
mod lnserial;
mod lnudp;
mod loconet;
mod locos;
mod node;
mod power;
mod profile;
mod rocnet;
mod sim_operators;
mod simulation;
mod tile_module;
mod topology;
mod train_tracking;
mod turnouts;
mod ui;
mod web;

use std::{env, path::Path, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = config::expand(env::args().skip(1).collect())?.into_iter();
    match args.next().as_deref() {
        None | Some("--mode=simulation") => {
            simulation::run()?;
            Ok(())
        }
        Some("--mode") => match args.next().as_deref() {
            Some("simulation") => {
                simulation::run()?;
                Ok(())
            }
            Some("node") => node::run(args.collect()).await,
            Some("dispatcher") => dispatcher::run(args.collect()).await,
            _ => Err("mode must be 'simulation', 'node' or 'dispatcher'".into()),
        },
        Some("--show-locos") => dispatcher::show_locos(args.collect()).await,
        Some("--transfer-loco") => dispatcher::transfer_loco(args.collect()).await,
        Some("--sim-hw") => dispatcher::sim_hw(args.collect()).await,
        Some("--place-train") => dispatcher::place_train(args.collect()).await,
        Some("--check-layout") => check_layout(args.collect()),
        Some("--simulate-layout") => simulate_layout(args.collect()),
        Some("--web-sim") => web::run_sim(args.collect()).await,
        Some("--web-mesh") => web::run_mesh(args.collect()).await,
        Some("--sim-district") => district_sim::run(args.collect()).await,
        Some("--sim-operators") => sim_operators::run(args.collect()).await,
        Some("--validate-profile") => {
            let path = args.next().ok_or("expected a profile path")?;
            let profile = profile::load(Path::new(&path))?;
            println!(
                "profile valid: {} ({})",
                profile.friendly_name, profile.cluster_id
            );
            Ok(())
        }
        Some("--help") | Some("-h") => {
            print_help();
            Ok(())
        }
        Some(command) => {
            Err(format!("unknown command: {command}\n\nRun with --help for usage.").into())
        }
    }
}

/// `--check-layout <layout.json> [--modules <dir>]`: loads a layout and its module files
/// (by default next to the layout file) and reports what is wrong, before the meeting.
fn check_layout(args: Vec<String>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = args.into_iter();
    let path = args
        .next()
        .ok_or("expected a layout file after --check-layout")?;
    let path = Path::new(&path);
    let mut modules_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--modules" => {
                modules_dir = args
                    .next()
                    .ok_or("expected a directory after --modules")?
                    .into()
            }
            _ => return Err(format!("unknown --check-layout option: {flag}").into()),
        }
    }
    let layout = layout::load(path, &modules_dir)?;
    let report = layout.check();
    println!(
        "{}: {} modules, {} districts",
        layout.file.meeting,
        layout.file.modules.len(),
        layout.file.districts.len()
    );
    for district in &layout.file.districts {
        println!(
            "  district {}: {}",
            district.node,
            district.modules.join(", ")
        );
    }
    for border in layout.borders() {
        println!(
            "  border {} – {} ({} – {})",
            border.a, border.b, border.districts.0, border.districts.1
        );
    }
    println!("Module checksums (for the layout file):");
    for (id, (_, sum)) in &layout.modules {
        println!("  {id}: {sum}");
    }
    for warning in &report.warnings {
        println!("WARNING {warning}");
    }
    for error in &report.errors {
        println!("ERROR   {error}");
    }
    if report.errors.is_empty() {
        println!("layout valid ({} warnings)", report.warnings.len());
        Ok(())
    } else {
        Err(format!(
            "{} errors, {} warnings",
            report.errors.len(),
            report.warnings.len()
        )
        .into())
    }
}

/// `--simulate-layout <layout.json> [--seconds N]`: runs the example trains on a layout
/// and prints what happens: blocks, sensors, turnouts, level crossings, borders.
fn simulate_layout(args: Vec<String>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = args.into_iter();
    let path = args
        .next()
        .ok_or("expected a layout file after --simulate-layout")?;
    let path = Path::new(&path);
    let mut seconds = 300.0;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--seconds" => {
                seconds = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or("expected a number of seconds after --seconds")?
            }
            _ => return Err(format!("unknown --simulate-layout option: {flag}").into()),
        }
    }
    let layout = layout::load(path, path.parent().unwrap_or(Path::new(".")))?;
    let report = layout.check();
    if !report.errors.is_empty() {
        return Err(format!(
            "the layout has errors; run --check-layout {}",
            path.display()
        )
        .into());
    }
    let topology = topology::Topology::build(&layout);
    let mut sim = layout_sim::LayoutSim::new(&layout, &topology);
    layout_sim::example_trains(&mut sim);
    for train in &sim.trains {
        println!(
            "{:>7.1} s  {} (loco {}, {} mm) starts on {}",
            0.0, train.name, train.address, train.length_mm, train.head.piece
        );
    }
    let step = 0.1;
    for _ in 0..(seconds / step) as usize {
        for (time, event) in sim.step(step) {
            println!("{time:>7.1} s  {event}");
        }
    }
    Ok(())
}

fn print_help() {
    println!(
        "Hive-Rail simulation scaffold\n\n\
         Usage:\n\
         cargo run -- --mode simulation\n\
         cargo run -- --mode node --session <name> --id <id> [--listen <endpoint> | --connect <endpoint>]\n\
         cargo run -- --mode node --session <name> --id <id> --rocnet-address <0-65535> --identify\n\
         cargo run -- --mode node --session <name> --id <id> --send-rocnet-hex <@frame>\n\
         cargo run -- --mode node --session <name> --id <id> --loconet-tcp [port]\n\
         cargo run -- --mode node --session <name> --id <id> --dccex <serial port | host:2560>\n\
         cargo run -- --mode node --session <name> --id <id> --plain\n\
         cargo run -- --mode node --session <name> --id <id> --layout <layout.json>   (know its own district's track; share each district's part with lite nodes)\n\
         cargo run -- --mode node --session <name> --id <id> --lite-server   (serve lite nodes: a Zenoh router they find by scouting)\n\
         cargo run -- --mode dispatcher --session <name> [--plain] [--listen <endpoint> | --connect <endpoint>] [--lite-server]\n\
         cargo run -- --show-locos --session <name>\n\
         cargo run -- --sim-hw <district> <power_on|power_off|short|global_stop|global_go> --session <name>   (fake hardware of a lite node)\n\
         cargo run -- --place-train <loco> --block <module/block> --session <name>   (put a train on the layout; its district's node follows it)\n\
         cargo run -- --transfer-loco <address> --to <node> --session <name> [--speed 0-126] [--forward|--reverse] [--lights|--no-lights]\n\
         cargo run -- --validate-profile <path>\n\
         cargo run -- --check-layout <layout.json> [--modules <dir>]\n\
         cargo run -- --simulate-layout <layout.json> [--seconds N]\n\
         cargo run -- --web-sim <layout.json> [--port 8080] [--speed 1] [--session <name>] [--modules <dir>]   (web view: http://localhost:8080)\n\
         cargo run -- --sim-district <layout.json> --district <name> [--body <address>@<module>/<piece>[,length=600][,kmh=100][,heading=forward|backward][,along=<mm>]]...\n\
           [--dccex-port 2560] [--loconet-port 1234] [--speed 1] [--session <name>]   (one district's hardware, as a node board;\n\
           with --session, bodies cross into the neighbouring districts' simulators)\n\
         cargo run -- --sim-operators <layout.json> --session <name> --throttle <district>=<host:port>... --train <loco>@<block>,<block>[,speed=64][,dwell=10]...\n\
           (simulated operators: trains back and forth between two blocks, through the nodes)\n\
         cargo run -- --web-mesh <layout.json> --session <name> [--port 8080] [--truth] [--modules <dir>]   (web view from the nodes on the mesh)\n\
         cargo run -- --config <file.toml> [more options]\n\
         cargo run -- --help\n\n\
         --config reads the options from a TOML file: each key is a flag without its dashes, a [table] a flag\n\
           prefix ([loconet] serial = ... is --loconet-serial ...); options on the command line win.\n\
         Interactive node mode shows a neighbor/heartbeat dashboard; --plain keeps log output.\n\
         Dashboard keys: 1 district power on, 0 district power off, h district emergency stop,\n\
         E global emergency stop (every district), G global go (ends it), s simulate short,\n\
         c fast clock run/pause, + - clock rate, [ ] clock hour back/forward.\n\
         With --plain, the same keys are read one per line from piped stdin (q quits).\n\
         --loconet-tcp serves LocoNet over TCP for Rocrail or JMRI, on port 1234 unless given;\n\
         --rocnet-address sets this node's RocNet ID (default 100).\n\
         --dccex drives a DCC-EX command station over USB serial (115200 baud) or WiFi TCP.\n\
         --loconet-connect <host[:port]> joins the LocoNet bus of a node board (LbServer, port 1234).\n\
         --loconet-tcp-slots external|node: who owns the slots of the LbServer clients (default external:\n\
           their own slot master, such as Rocrail's slot server or a central; node: the node, for throttles).\n\
         --loconet-connect-slots node|external: the same for the bus (default node on a node board, for FREDs;\n\
           external on a serial or UDP interface).\n\
         --loconet-serial <port> joins a LocoNet bus through a serial interface instead, such as an Intellibox I\n\
           (SO 2 = 4); --loconet-serial-baud (default 19200) and --loconet-serial-flow hardware|none (default hardware).\n\
         --loconet-udp [group[:port]] joins a LocoNet bus over UDP multicast instead, such as a GCA101\n\
           LocoBuffer-UDP (default 224.0.0.1:1235, as Rocrail's LNUDP); --loconet-udp-interface <ip> picks the\n\
           network when this computer has several (Wi-Fi and Ethernet).\n\
         --show-sensors <turnout=sensor,...> shows sensors on a panel's turnout LEDs (an IB-Switch):\n\
           green while free, red while occupied.\n\
         --loconet-fast-clock on|off: write the fast clock (slot 123) to the bus (default on;\n\
           off for a central without one, such as an Intellibox I).\n\
         Dispatcher mode (FREMO: Zugleiter) shows every district, the fast clock and the layout's\n\
         loco register, read-only; --show-locos prints the register once.\n\
         Node mode uses Zenoh LAN peer scouting and can exchange simulation RocNet frames.\n\
         No local RocNet transport is connected yet."
    );
}
