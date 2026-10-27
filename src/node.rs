use crate::rocnet::RocNetFrame;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, error::Error, time::Duration};
use tokio::time::{self, Instant, MissedTickBehavior};
use zenoh::config::Config;

const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(250);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(750);
const ZENOH_SCOUTING_ADDRESS: &str = "224.0.0.224:7446";
const ROCNET_UDP_MULTICAST: &str = "224.0.0.1:4321";

#[derive(Debug)]
struct NodeArgs {
    session: String,
    id: String,
    listen: Option<String>,
    connect: Option<String>,
    rocnet_address: Option<u16>,
    identify: bool,
    send_rocnet_hex: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Heartbeat {
    node: String,
    seq: u64,
    power: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RocNetMeshEnvelope {
    origin_node: String,
    frame_ascii_hex: String,
}

pub async fn run(args: Vec<String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let options = parse_args(args)?;
    let mut config = Config::default();
    config.insert_json5("mode", "\"peer\"")?;
    config.insert_json5("scouting/multicast/enabled", "true")?;
    config.insert_json5(
        "scouting/multicast/address",
        &format!("\"{ZENOH_SCOUTING_ADDRESS}\""),
    )?;

    if let Some(endpoint) = &options.listen {
        config.insert_json5("listen/endpoints", &format!("[\"{endpoint}\"]"))?;
    }
    if let Some(endpoint) = &options.connect {
        config.insert_json5("connect/endpoints", &format!("[\"{endpoint}\"]"))?;
    }

    let session = zenoh::open(config).await?;
    let key = format!("hive/{}/node/{}/hb", options.session, options.id);
    let subscriber_key = format!("hive/{}/node/*/hb", options.session);
    let publisher = session.declare_publisher(key).await?;
    let subscriber = session.declare_subscriber(subscriber_key).await?;
    let rocnet_key = format!("hive/{}/rocnet/frame", options.session);
    let rocnet_publisher = session.declare_publisher(rocnet_key.clone()).await?;
    let rocnet_subscriber = session.declare_subscriber(rocnet_key).await?;
    let mut peers: HashMap<String, (Instant, bool)> = HashMap::new();
    let mut sequence = 0_u64;
    let mut pending_frame = if options.identify {
        let address = options
            .rocnet_address
            .ok_or("--identify requires --rocnet-address")?;
        Some(RocNetFrame::stationary_identify_request(0, address))
    } else if let Some(frame) = options.send_rocnet_hex.as_deref() {
        Some(RocNetFrame::from_ascii_hex(frame)?)
    } else {
        None
    };
    let mut heartbeat = time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);

    println!(
        "node {} joined session {}; heartbeat key: hive/{}/node/{}/hb",
        options.id, options.session, options.session, options.id
    );
    println!(
        "Zenoh multicast scouting: {ZENOH_SCOUTING_ADDRESS} (RocNet UDP multicast {ROCNET_UDP_MULTICAST} is not used for Hive-Rail discovery)"
    );
    println!("simulation only; no local RocNet transport or hardware is connected");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("shutting down node {}", options.id);
                break;
            }
            _ = heartbeat.tick() => {
                sequence += 1;
                let message = Heartbeat {
                    node: options.id.clone(),
                    seq: sequence,
                    power: "Normal".to_owned(),
                };
                publisher.put(serde_json::to_vec(&message)?).await?;
                refresh_peer_states(&mut peers);
                if let Some(frame) = pending_frame.take_if(|_| peers.values().any(|(_, online)| *online)) {
                    let envelope = RocNetMeshEnvelope {
                        origin_node: options.id.clone(),
                        frame_ascii_hex: frame.to_ascii_hex(),
                    };
                    rocnet_publisher.put(serde_json::to_vec(&envelope)?).await?;
                    println!("published RocNet frame over Zenoh: {}", envelope.frame_ascii_hex);
                }
            }
            sample = subscriber.recv_async() => {
                let sample = sample?;
                let payload = sample.payload().to_bytes();
                let message: Heartbeat = serde_json::from_slice(payload.as_ref())?;
                if message.node != options.id {
                    let was_online = peers
                        .get(&message.node)
                        .map(|(_, online)| *online)
                        .unwrap_or(false);
                    peers.insert(message.node.clone(), (Instant::now(), true));
                    if !was_online {
                        println!(
                            "peer {} online (seq {}, power {})",
                            message.node, message.seq, message.power
                        );
                    }
                }
            }
            sample = rocnet_subscriber.recv_async() => {
                let sample = sample?;
                let payload = sample.payload().to_bytes();
                let envelope: RocNetMeshEnvelope = serde_json::from_slice(payload.as_ref())?;
                if envelope.origin_node != options.id {
                    let frame = RocNetFrame::from_ascii_hex(&envelope.frame_ascii_hex)?;
                    println!(
                        "RocNet frame from {} via Zenoh: net={}, recipient={}, sender={}, group={}, code={}, data={:02X?}",
                        envelope.origin_node,
                        frame.network_id(),
                        frame.recipient_id(),
                        frame.sender_id(),
                        frame.group(),
                        frame.code(),
                        frame.data()
                    );
                }
            }
        }
    }

    Ok(())
}

fn refresh_peer_states(peers: &mut HashMap<String, (Instant, bool)>) {
    let now = Instant::now();
    for (peer_id, (last_seen, is_online)) in peers.iter_mut() {
        if now.duration_since(*last_seen) >= HEARTBEAT_TIMEOUT && *is_online {
            *is_online = false;
            println!("peer {peer_id} stale after 3 missed heartbeats; boundary state remains Stop");
        }
    }
}

fn parse_args(args: Vec<String>) -> Result<NodeArgs, Box<dyn Error + Send + Sync>> {
    let mut session = None;
    let mut id = None;
    let mut listen = None;
    let mut connect = None;
    let mut rocnet_address = None;
    let mut identify = false;
    let mut send_rocnet_hex = None;
    let mut args = args.into_iter();

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--identify" => identify = true,
            "--session" => session = Some(next_value(&mut args, &flag)?),
            "--id" => id = Some(next_value(&mut args, &flag)?),
            "--listen" => listen = Some(next_value(&mut args, &flag)?),
            "--connect" => connect = Some(next_value(&mut args, &flag)?),
            "--send-rocnet-hex" => send_rocnet_hex = Some(next_value(&mut args, &flag)?),
            "--rocnet-address" => {
                let value = next_value(&mut args, &flag)?;
                rocnet_address = Some(value.parse::<u16>().map_err(|_| {
                    format!("invalid RocNet address '{value}'; expected an integer from 0 to 65535")
                })?);
            }
            _ => return Err(format!("unknown node option: {flag}").into()),
        }
    }

    if listen.is_some() && connect.is_some() {
        return Err("choose either --listen or --connect for this prototype".into());
    }
    if identify && send_rocnet_hex.is_some() {
        return Err("choose either --identify or --send-rocnet-hex".into());
    }
    if identify && rocnet_address.is_none() {
        return Err("--identify requires --rocnet-address".into());
    }

    Ok(NodeArgs {
        session: session.ok_or("missing --session")?,
        id: id.ok_or("missing --id")?,
        listen,
        connect,
        rocnet_address,
        identify,
        send_rocnet_hex,
    })
}

fn next_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    args.next()
        .ok_or_else(|| format!("missing value after {flag}").into())
}

#[cfg(test)]
mod tests {
    use super::{parse_args, ROCNET_UDP_MULTICAST, ZENOH_SCOUTING_ADDRESS};

    #[test]
    fn default_peer_mode_does_not_require_manual_endpoints() {
        let args = parse_args(vec![
            "--session".into(),
            "bench".into(),
            "--id".into(),
            "node-a".into(),
        ])
        .unwrap();

        assert!(args.listen.is_none());
        assert!(args.connect.is_none());
    }

    #[test]
    fn zenoh_scouting_is_separate_from_rocnet_multicast() {
        assert_ne!(ZENOH_SCOUTING_ADDRESS, ROCNET_UDP_MULTICAST);
    }

    #[test]
    fn rocnet_identify_requires_a_local_protocol_address() {
        let missing_address = parse_args(vec![
            "--session".into(),
            "bench".into(),
            "--id".into(),
            "node-a".into(),
            "--identify".into(),
        ]);
        assert!(missing_address.is_err());

        let with_address = parse_args(vec![
            "--session".into(),
            "bench".into(),
            "--id".into(),
            "node-a".into(),
            "--rocnet-address".into(),
            "42".into(),
            "--identify".into(),
        ])
        .unwrap();
        assert!(with_address.identify);
        assert_eq!(with_address.rocnet_address, Some(42));
    }
}
