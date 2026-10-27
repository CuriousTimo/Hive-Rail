use serde::{Deserialize, Serialize};
use std::{collections::HashMap, error::Error, time::Duration};
use tokio::time::{self, Instant, MissedTickBehavior};
use zenoh::config::Config;

const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(250);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(750);

#[derive(Debug)]
struct NodeArgs {
    session: String,
    id: String,
    listen: Option<String>,
    connect: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Heartbeat {
    node: String,
    seq: u64,
    power: String,
}

pub async fn run(args: Vec<String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let options = parse_args(args)?;
    let mut config = Config::default();
    config.insert_json5("mode", "\"peer\"")?;

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
    let mut peers: HashMap<String, (Instant, bool)> = HashMap::new();
    let mut sequence = 0_u64;
    let mut heartbeat = time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);

    println!(
        "node {} joined session {}; heartbeat key: hive/{}/node/{}/hb",
        options.id, options.session, options.session, options.id
    );
    println!("simulation only; no hardware drivers are connected");

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
    let mut args = args.into_iter();

    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value after {flag}"))?;
        match flag.as_str() {
            "--session" => session = Some(value),
            "--id" => id = Some(value),
            "--listen" => listen = Some(value),
            "--connect" => connect = Some(value),
            _ => return Err(format!("unknown node option: {flag}").into()),
        }
    }

    if listen.is_none() && connect.is_none() {
        return Err("node mode requires --listen and/or --connect".into());
    }
    if listen.is_some() && connect.is_some() {
        return Err("choose either --listen or --connect for this prototype".into());
    }

    Ok(NodeArgs {
        session: session.ok_or("missing --session")?,
        id: id.ok_or("missing --id")?,
        listen,
        connect,
    })
}
