mod clock;
mod lbserver;
mod loconet;
mod node;
mod power;
mod profile;
mod rocnet;
mod simulation;
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
    let mut args = env::args().skip(1);
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
            _ => Err("mode must be 'simulation' or 'node'".into()),
        },
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

fn print_help() {
    println!(
        "Hive-Rail simulation scaffold\n\n\
         Usage:\n\
         cargo run -- --mode simulation\n\
         cargo run -- --mode node --session <name> --id <id> [--listen <endpoint> | --connect <endpoint>]\n\
         cargo run -- --mode node --session <name> --id <id> --rocnet-address <0-65535> --identify\n\
         cargo run -- --mode node --session <name> --id <id> --send-rocnet-hex <@frame>\n\
         cargo run -- --mode node --session <name> --id <id> --loconet-tcp [port]\n\
         cargo run -- --mode node --session <name> --id <id> --plain\n\
         cargo run -- --validate-profile <path>\n\
         cargo run -- --help\n\n\
         Interactive node mode shows a neighbor/heartbeat dashboard; --plain keeps log output.\n\
         Dashboard keys: 1 district power on, 0 district power off, h district emergency stop,\n\
         E global emergency stop (every district), s simulate short,\n\
         c fast clock run/pause, + - clock rate, [ ] clock hour back/forward.\n\
         --loconet-tcp serves LocoNet over TCP for Rocrail or JMRI, on port 1234 unless given;\n\
         --rocnet-address sets this node's RocNet ID (default 100).\n\
         Node mode uses Zenoh LAN peer scouting and can exchange simulation RocNet frames.\n\
         No local RocNet transport or railway hardware is connected."
    );
}
