mod node;
mod profile;
mod simulation;

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
         cargo run -- --mode node --session <name> --id <id> (--listen <endpoint> | --connect <endpoint>)\n\
         cargo run -- --validate-profile <path>\n\
         cargo run -- --help\n\n\
         Node mode exchanges simulation heartbeats only. No hardware drivers are connected."
    );
}
