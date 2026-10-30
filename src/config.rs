//! `--config <file>`: options from a TOML file instead of the command line.
//!
//! Each key is the flag without its dashes, and a table is a flag prefix, so the file needs
//! no parser of its own and every flag works in it:
//!
//! ```toml
//! mode = "node"
//! session = "bench"          # --session bench
//! plain = true               # --plain
//!
//! [loconet]
//! serial = "/dev/cu.usbserial-0001"   # --loconet-serial /dev/cu.usbserial-0001
//! fast-clock = false                  # --loconet-fast-clock off
//! ```
//!
//! The file's flags come before the command line's, so a flag given on the command line
//! wins.

use std::{fs, path::Path};
use thiserror::Error;
use toml::{Table, Value};

/// Flags that take no value: `true` gives the flag, `false` leaves it out. Every other
/// flag given `true` or `false` gets `on` or `off`.
const SWITCHES: &[&str] = &["--identify", "--plain", "--loconet-tcp", "--loconet-udp"];

/// Commands that run on their own and take no config file.
const COMMANDS: &[&str] = &[
    "--show-locos",
    "--transfer-loco",
    "--validate-profile",
    "--check-layout",
    "--simulate-layout",
    "--help",
    "-h",
    "--mode=simulation",
];

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid config {path}: {source}")]
    Parse {
        path: String,
        source: toml::de::Error,
    },
    #[error("config {path}: {message}")]
    Invalid { path: String, message: String },
}

/// The command line with `--config <file>` replaced by the file's flags; unchanged
/// without `--config`.
pub fn expand(args: Vec<String>) -> Result<Vec<String>, ConfigError> {
    let Some(index) = args.iter().position(|arg| arg == "--config") else {
        return Ok(args);
    };
    let mut args = args;
    let Some(path) = args.get(index + 1).cloned() else {
        return Err(ConfigError::Invalid {
            path: String::new(),
            message: "expected a file after --config".to_owned(),
        });
    };
    args.drain(index..=index + 1);
    let text = fs::read_to_string(Path::new(&path)).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;
    merge(&text, args).map_err(|error| match error {
        ConfigError::Parse { source, .. } => ConfigError::Parse { path, source },
        ConfigError::Invalid { message, .. } => ConfigError::Invalid { path, message },
        other => other,
    })
}

/// Puts the flags from `text` after the mode and before the command line's own flags.
fn merge(text: &str, args: Vec<String>) -> Result<Vec<String>, ConfigError> {
    let invalid = |message: String| ConfigError::Invalid {
        path: String::new(),
        message,
    };
    let mut table: Table = text.parse().map_err(|source| ConfigError::Parse {
        path: String::new(),
        source,
    })?;
    let mode = match table.remove("mode") {
        Some(Value::String(mode)) => Some(mode),
        Some(_) => return Err(invalid("mode must be a string".to_owned())),
        None => None,
    };
    let mut flags = Vec::new();
    add_flags(&table, "--", &mut flags).map_err(invalid)?;

    if let Some(command) = args.first().filter(|arg| COMMANDS.contains(&arg.as_str())) {
        return Err(invalid(format!(
            "--config works with --mode node and --mode dispatcher, not {command}"
        )));
    }
    // A mode on the command line wins over the file's.
    let (mode, rest) = match args.first().map(String::as_str) {
        Some("--mode") => {
            let mode = args.get(1).cloned();
            (mode, args.into_iter().skip(2).collect())
        }
        _ => (mode, args),
    };
    let mode = mode.ok_or_else(|| invalid("no mode; add mode = \"node\"".to_owned()))?;
    Ok(["--mode".to_owned(), mode]
        .into_iter()
        .chain(flags)
        .chain(rest)
        .collect())
}

fn add_flags(table: &Table, prefix: &str, flags: &mut Vec<String>) -> Result<(), String> {
    for (key, value) in table {
        let flag = format!("{prefix}{key}");
        match value {
            Value::Table(inner) => add_flags(inner, &format!("{flag}-"), flags)?,
            Value::Boolean(on) if SWITCHES.contains(&flag.as_str()) => {
                if *on {
                    flags.push(flag);
                }
            }
            Value::Boolean(on) => {
                flags.push(flag);
                flags.push(if *on { "on" } else { "off" }.to_owned());
            }
            Value::String(text) => {
                flags.push(flag);
                flags.push(text.clone());
            }
            Value::Integer(number) => {
                flags.push(flag);
                flags.push(number.to_string());
            }
            _ => return Err(format!("{key} must be a string, a number, true or false")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::merge;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    const BENCH: &str = r#"
        mode = "node"
        session = "bench"
        id = "MacBook"
        rocnet-address = 101
        plain = true

        [loconet]
        serial = "/dev/cu.usbserial-0001"
        fast-clock = false
        tcp = true
    "#;

    #[test]
    fn turns_keys_and_tables_into_flags() {
        let merged = merge(BENCH, Vec::new()).unwrap();
        assert_eq!(merged[..2], args("--mode node"));
        let mut flags = merged[2..].to_vec();
        flags.sort();
        let mut expected = args(
            "--id MacBook --loconet-fast-clock off --loconet-serial /dev/cu.usbserial-0001 \
             --loconet-tcp --plain --rocnet-address 101 --session bench",
        );
        expected.sort();
        assert_eq!(flags, expected);
    }

    #[test]
    fn puts_the_command_line_last_so_it_wins() {
        let merged = merge(BENCH, args("--mode dispatcher --session club")).unwrap();
        assert_eq!(merged[..2], args("--mode dispatcher"));
        assert_eq!(merged[merged.len() - 2..], args("--session club"));
        // Flags alone keep the file's mode.
        let merged = merge(BENCH, args("--id MacBook-2")).unwrap();
        assert_eq!(merged[..2], args("--mode node"));
        assert_eq!(merged[merged.len() - 2..], args("--id MacBook-2"));
    }

    #[test]
    fn refuses_what_it_cannot_turn_into_flags() {
        assert!(merge("session = \"bench\"", Vec::new()).is_err()); // no mode
        assert!(merge(BENCH, args("--show-locos")).is_err());
        assert!(merge("mode = \"node\"\nports = [1, 2]", Vec::new()).is_err());
        assert!(merge("mode = \"node\"\nsession = ", Vec::new()).is_err());
    }
}
