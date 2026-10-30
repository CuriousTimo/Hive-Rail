mod clock;
mod config;
mod dccex;
mod dispatcher;
mod layout;
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
mod simulation;
mod topology;
mod turnouts;
mod ui;

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
        Some("--check-layout") => check_layout(args.collect()),
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
         cargo run -- --mode dispatcher --session <name> [--plain]\n\
         cargo run -- --show-locos --session <name>\n\
         cargo run -- --transfer-loco <address> --to <node> --session <name> [--speed 0-126] [--forward|--reverse] [--lights|--no-lights]\n\
         cargo run -- --validate-profile <path>\n\
         cargo run -- --check-layout <layout.json> [--modules <dir>]\n\
         cargo run -- --config <file.toml> [more options]\n\
         cargo run -- --help\n\n\
         --config reads the options from a TOML file: each key is a flag without its dashes, a [table] a flag\n\
           prefix ([loconet] serial = ... is --loconet-serial ...); options on the command line win.\n\
         Interactive node mode shows a neighbor/heartbeat dashboard; --plain keeps log output.\n\
         Dashboard keys: 1 district power on, 0 district power off, h district emergency stop,\n\
         E global emergency stop (every district), s simulate short,\n\
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
         --loconet-fast-clock on|off: write the fast clock (slot 123) to the bus (default on;\n\
           off for a central without one, such as an Intellibox I).\n\
         Dispatcher mode (FREMO: Zugleiter) shows every district, the fast clock and the layout's\n\
         loco register, read-only; --show-locos prints the register once.\n\
         Node mode uses Zenoh LAN peer scouting and can exchange simulation RocNet frames.\n\
         No local RocNet transport is connected yet."
    );
}
