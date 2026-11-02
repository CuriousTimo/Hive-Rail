//! The web view: a small built-in web server on the node, opened from any laptop, tablet
//! or phone on the network (most nodes run headless under the layout; a node with a
//! screen can show it in a browser's kiosk mode). It serves the page (`web/`), the layout
//! to draw (`/api/layout`) and the live state (`/events`, Server-Sent Events).
//!
//! `--web-sim` runs the layout simulator behind it, in real time.

use crate::layout_sim::LayoutSim;
use crate::layout_view::ViewBuilder;
use crate::topology::Topology;
use crate::{layout, layout_sim};
use std::collections::VecDeque;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::{self, Duration, MissedTickBehavior};

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const APP_CSS: &str = include_str!("../web/app.css");
// The panel editor (SpDrS60 tiles on a grid) and its tiles, made by tools/spdrs60_tiles.py.
const PANEL_HTML: &str = include_str!("../web/panel.html");
const PANEL_JS: &str = include_str!("../web/panel.js");
const PANEL_CSS: &str = include_str!("../web/panel.css");
const TILES_JS: &str = include_str!("../web/spdrs60-tiles.js");
// German light signals as they look on the track, made by tools/signal_faces.py.
const SIGNAL_FACES_JS: &str = include_str!("../web/signal-faces.js");
/// Event lines kept for the page's log.
const LOG_LINES: usize = 40;

struct Shared {
    layout_json: String,
    state: watch::Receiver<String>,
    /// The folder of module files (schema version 3) the module editor opens and lists.
    modules: std::path::PathBuf,
}

/// `--web-sim <layout.json> [--port 8080] [--speed 1] [--modules <dir>]`.
pub async fn run_sim(args: Vec<String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = args.into_iter();
    let path = args
        .next()
        .ok_or("expected a layout file after --web-sim")?;
    let path = Path::new(&path).to_path_buf();
    let mut port = 8080_u16;
    let mut speed = 1.0_f64;
    let mut session = None;
    let mut modules = std::path::PathBuf::from("examples/layout");
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("expected a value after {flag}"))
        };
        match flag.as_str() {
            "--port" => port = value()?.parse().map_err(|_| "invalid --port")?,
            "--speed" => speed = value()?.parse().map_err(|_| "invalid --speed")?,
            "--session" => session = Some(value()?),
            "--modules" => modules = value()?.into(),
            _ => return Err(format!("unknown --web-sim option: {flag}").into()),
        }
    }
    let (layout, topology, view, layout_json) = load_layout(&path)?;
    let mut sim = LayoutSim::new(layout, topology);
    layout_sim::example_trains(&mut sim);

    // On the mesh: the trains go into the session's loco register, in the district their
    // head is in, so the dispatcher and the district nodes follow them.
    let mut mesh = match &session {
        Some(session) => {
            let mut publisher =
                crate::dispatcher::LocoPublisher::open(session, "simulator").await?;
            for train in &sim.trains {
                let district = topology
                    .piece(&train.head.piece)
                    .map(|p| p.district.clone())
                    .unwrap_or_default();
                publisher
                    .put(train.address, &district, running_step(train), true, true)
                    .await?;
            }
            println!("simulated trains are in session {session}'s loco register; nodes named after the districts follow them");
            Some(publisher)
        }
        None => None,
    };
    // Each train's direction as published: it changes when the train reverses.
    let mut forward: std::collections::HashMap<String, bool> =
        sim.trains.iter().map(|t| (t.name.clone(), true)).collect();
    // What the loco register was last told of each train: district, speed step, direction.
    let mut published = std::collections::HashMap::new();

    let (publish, server) = start_server(port, layout_json, modules, &layout.file.meeting).await?;

    let step = 0.1;
    let mut tick = time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut log: VecDeque<String> = VecDeque::new();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            followed = follow_clock(mesh.as_mut()) => followed?,
            _ = tick.tick() => {
                if let Some(publisher) = mesh.as_mut() {
                    publisher.drain();
                    // Each district's power, from its node's heartbeats; stops last fast minutes
                    // at the mesh clock's rate.
                    sim.district_power = publisher.district_power();
                    if publisher.clock.known() {
                        sim.clock_rate = f64::from(publisher.clock.rate());
                    }
                }
                for (time, event) in sim.step(step * speed) {
                    if let Some(publisher) = mesh.as_mut() {
                        // A train reverses at each stop: its direction changes.
                        if let layout_sim::SimEvent::Departed { train, .. } = &event {
                            if let Some(direction) = forward.get_mut(train) {
                                *direction = !*direction;
                            }
                        }
                        publish_event(publisher, layout, &sim, topology, &event).await?;
                        if let layout_sim::SimEvent::Block { district, occupied, address: Some(address), .. } = &event {
                            publisher.sim_sensor(district, *address, *occupied).await?;
                        }
                    }
                    let line = format!("{:>7.1} s  {event}", time);
                    if log.len() == LOG_LINES {
                        log.pop_front();
                    }
                    log.push_back(line);
                }
                if let Some(publisher) = mesh.as_mut() {
                    publish_trains(publisher, &sim, topology, &forward, &mut published).await?;
                }
                let snapshot = view.snapshot(&sim, topology);
                let mut json = serde_json::to_value(&snapshot)?;
                json["log"] = serde_json::Value::from(log.iter().cloned().collect::<Vec<_>>());
                // The layout's fast clock, as the nodes keep it (only on the mesh).
                if let Some(publisher) = &mesh {
                    json["clock"] = clock_json(&publisher.clock);
                }
                let _ = publish.send(json.to_string());
            }
        }
    }
    server.abort();
    Ok(())
}

/// A checked layout for the web view, kept for the whole run, with its track graph, how
/// to draw it, and the drawing as the page loads it (`/api/layout`).
#[allow(clippy::type_complexity)]
fn load_layout(
    path: &Path,
) -> Result<
    (
        &'static layout::Layout,
        &'static Topology,
        ViewBuilder,
        String,
    ),
    Box<dyn Error + Send + Sync>,
> {
    let layout = layout::load(path, path.parent().unwrap_or(Path::new(".")))?;
    if !layout.check().errors.is_empty() {
        return Err(format!(
            "the layout has errors; run --check-layout {}",
            path.display()
        )
        .into());
    }
    let layout: &'static layout::Layout = Box::leak(Box::new(layout));
    let topology: &'static Topology = Box::leak(Box::new(Topology::build(layout)));
    let view = ViewBuilder::new(layout, topology);
    let layout_json = serde_json::to_string(&view.layout(layout, topology))?;
    Ok((layout, topology, view, layout_json))
}

/// Serves the page on `port`: what is sent on the returned channel goes to every page as
/// the live state.
async fn start_server(
    port: u16,
    layout_json: String,
    modules: std::path::PathBuf,
    meeting: &str,
) -> Result<(watch::Sender<String>, tokio::task::JoinHandle<()>), Box<dyn Error + Send + Sync>> {
    let (publish, state) = watch::channel(String::new());
    let shared = Arc::new(Shared {
        layout_json,
        state,
        modules,
    });
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    println!("Hive-Rail web view of {meeting} on http://localhost:{port} (and this computer's address on the network)");
    Ok((publish, tokio::spawn(accept(listener, shared))))
}

/// The layout's fast clock for the page.
fn clock_json(clock: &crate::clock::FastClock) -> serde_json::Value {
    let minute = clock.minute_of_day(tokio::time::Instant::now());
    serde_json::json!({
        "known": clock.known(),
        "time": format!("{:02}:{:02}", minute / 60, minute % 60),
        "minute": minute,
        "running": clock.running(),
        "rate": clock.rate(),
    })
}

/// `--web-mesh <layout.json> --session <name> [--port 8080] [--truth] [--modules <dir>]`:
/// the web view from the mesh instead of the layout simulator: the blocks, turnouts and
/// boundary signals each node publishes of its district, the trains they follow, their
/// events and the fast clock. With `--truth`, the district simulators' trains over them.
pub async fn run_mesh(args: Vec<String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = args.into_iter();
    let path = args
        .next()
        .ok_or("expected a layout file after --web-mesh")?;
    let path = Path::new(&path).to_path_buf();
    let (mut port, mut session, mut truth) = (8080_u16, None, false);
    let mut modules = std::path::PathBuf::from("examples/layout");
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("expected a value after {flag}"))
        };
        match flag.as_str() {
            "--port" => port = value()?.parse().map_err(|_| "invalid --port")?,
            "--session" => session = Some(value()?),
            "--modules" => modules = value()?.into(),
            "--truth" => truth = true,
            _ => return Err(format!("unknown --web-mesh option: {flag}").into()),
        }
    }
    let session = session.ok_or("--web-mesh needs --session <name>")?;
    let (layout, topology, view, layout_json) = load_layout(&path)?;
    let mut observer = crate::dispatcher::LocoPublisher::open(&session, "web-mesh").await?;
    let zenoh = zenoh::open(crate::node::zenoh_config(None, None, false)?).await?;
    let tracks_in = zenoh
        .declare_subscriber(format!("hive/{session}/node/*/track"))
        .await?;
    let trains_in = zenoh
        .declare_subscriber(format!("hive/{session}/node/*/trains"))
        .await?;
    let events_in = zenoh
        .declare_subscriber(format!("hive/{session}/node/*/event"))
        .await?;
    let truth_in = zenoh
        .declare_subscriber(format!("hive-sim/{session}/world/truth/*"))
        .await?;
    let (publish, server) = start_server(port, layout_json, modules, &layout.file.meeting).await?;
    println!(
        "drawn from session {session}'s nodes{}",
        if truth {
            ", with the simulators' trains over them"
        } else {
            ""
        }
    );

    let mut tracks: std::collections::BTreeMap<String, crate::district_track::TrackState> =
        Default::default();
    let mut pictures: std::collections::BTreeMap<String, crate::train_tracking::TrainPicture> =
        Default::default();
    let mut bodies: std::collections::BTreeMap<
        String,
        (crate::district_sim::Body, tokio::time::Instant),
    > = Default::default();
    let mut log: VecDeque<String> = VecDeque::new();
    let started = tokio::time::Instant::now();
    let mut tick = time::interval(Duration::from_millis(200));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            followed = observer.follow_clock() => followed?,
            sample = tracks_in.recv_async() => {
                if let Ok(state) = serde_json::from_slice::<crate::district_track::TrackState>(&sample?.payload().to_bytes()) {
                    tracks.insert(state.district.clone(), state);
                }
            }
            sample = trains_in.recv_async() => {
                if let Ok(picture) = serde_json::from_slice::<crate::train_tracking::TrainPicture>(&sample?.payload().to_bytes()) {
                    pictures.insert(picture.district.clone(), picture);
                }
            }
            sample = events_in.recv_async() => {
                let sample = sample?;
                let node = sample.key_expr().as_str().split('/').nth(3).unwrap_or("?").to_owned();
                let line = format!("{node}: {}", String::from_utf8_lossy(&sample.payload().to_bytes()));
                if log.len() == LOG_LINES {
                    log.pop_front();
                }
                log.push_back(line);
            }
            sample = truth_in.recv_async() => {
                if let Ok(wire) = serde_json::from_slice::<crate::district_sim::BodyWire>(&sample?.payload().to_bytes()) {
                    bodies.insert(wire.id.clone(), (crate::district_sim::Body::from_wire(&wire), tokio::time::Instant::now()));
                }
            }
            _ = tick.tick() => {
                observer.drain();
                // A body the simulators no longer tell of is gone.
                bodies.retain(|_, (_, seen)| seen.elapsed() < Duration::from_secs(2));
                let shown: Vec<crate::district_sim::Body> =
                    if truth { bodies.values().map(|(body, _)| body.clone()).collect() } else { Vec::new() };
                let snapshot = view.mesh_snapshot(started.elapsed().as_secs_f64(), topology, tracks.values(), pictures.values(), &shown);
                let mut json = serde_json::to_value(&snapshot)?;
                json["log"] = serde_json::Value::from(log.iter().cloned().collect::<Vec<_>>());
                json["clock"] = clock_json(&observer.clock);
                let _ = publish.send(json.to_string());
            }
        }
    }
    server.abort();
    Ok(())
}

/// Follows the mesh's fast clock; without the mesh, never returns.
async fn follow_clock(
    publisher: Option<&mut crate::dispatcher::LocoPublisher>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    match publisher {
        Some(publisher) => publisher.follow_clock().await,
        None => std::future::pending().await,
    }
}

/// The speed step (of 126) a running simulated train has in the loco register: its speed on
/// the layout, so the IC shows a higher step than the goods train.
fn running_step(train: &layout_sim::Train) -> u8 {
    (train.speed_mm_s * 0.6).round().clamp(1.0, 126.0) as u8
}

/// Keeps the loco register up to date with the trains: each train's district, speed step
/// (0 while it stands: at a stop, at a red signal, without power) and direction, put on
/// the mesh whenever one of them changes.
async fn publish_trains(
    publisher: &mut crate::dispatcher::LocoPublisher,
    sim: &LayoutSim<'_>,
    topology: &Topology,
    forward: &std::collections::HashMap<String, bool>,
    published: &mut std::collections::HashMap<String, (String, u8, bool)>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    for train in &sim.trains {
        let district = topology
            .piece(&train.head.piece)
            .map(|p| p.district.clone())
            .unwrap_or_default();
        let step = match train.state {
            layout_sim::TrainState::Running => running_step(train),
            _ => 0,
        };
        let direction = forward.get(&train.name).copied().unwrap_or(true);
        let now = (district, step, direction);
        if published.get(&train.name) != Some(&now) {
            publisher
                .put(train.address, &now.0, now.1, now.2, true)
                .await?;
            published.insert(train.name.clone(), now);
        }
    }
    Ok(())
}

/// Sends a simulator event to the dispatcher, as the district it happens in reports it:
/// border crossings, arrivals, departures and level crossings as events, blocks, sensors
/// and turnouts as details.
async fn publish_event(
    publisher: &crate::dispatcher::LocoPublisher,
    layout: &layout::Layout,
    sim: &LayoutSim<'_>,
    topology: &Topology,
    event: &layout_sim::SimEvent,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use layout_sim::SimEvent;
    let train_district = |name: &str| {
        sim.trains
            .iter()
            .find(|t| t.name == name)
            .and_then(|t| topology.piece(&t.head.piece))
            .map(|p| p.district.clone())
            .unwrap_or_default()
    };
    let (district, detail) = match event {
        SimEvent::Block { district, .. }
        | SimEvent::Sensor { district, .. }
        | SimEvent::Turnout { district, .. } => (district.clone(), true),
        SimEvent::Signal { district, .. } => (district.clone(), true),
        SimEvent::Power { district, .. } => (district.clone(), false),
        SimEvent::Yield { train, .. } | SimEvent::PlatformChange { train, .. } => {
            (train_district(train), false)
        }
        SimEvent::Feed { block, district } => {
            let _ = block;
            (district.clone(), false)
        }
        SimEvent::Route { train, .. } => (train_district(train), true),
        SimEvent::Crossing { crossing, .. } => {
            let module = crossing.split('/').next().unwrap_or("");
            (layout.district_of(module).unwrap_or("").to_owned(), false)
        }
        SimEvent::Border { to, .. } => (to.clone(), false),
        SimEvent::Arrived { train, .. } | SimEvent::Departed { train, .. } => {
            (train_district(train), false)
        }
    };
    if district.is_empty() {
        return Ok(());
    }
    // The dispatcher writes the district in front; the text need not repeat it.
    let text = event.to_string();
    let text = text.strip_prefix(&format!("{district}: ")).unwrap_or(&text);
    publisher.event(&district, detail, text).await
}

/// The module files in the folder, as a JSON list of names: ["timo-adorf.json", ...].
fn module_list(dir: &Path) -> String {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .filter(|name| name.ends_with(".json"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    serde_json::to_string(&names).unwrap_or_else(|_| "[]".into())
}

/// One module file by its name; only plain names (letters, digits, - and _) ending in
/// .json, so a request cannot reach outside the folder.
fn module_file(dir: &Path, name: &str) -> Option<String> {
    let stem = name.strip_suffix(".json")?;
    let plain = !stem.is_empty()
        && stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    plain
        .then(|| std::fs::read_to_string(dir.join(name)).ok())
        .flatten()
}

async fn accept(listener: TcpListener, shared: Arc<Shared>) {
    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(serve(stream, Arc::clone(&shared)));
    }
}

/// One request: reads its first line, answers, closes (or streams, for `/events`).
async fn serve(mut stream: TcpStream, shared: Arc<Shared>) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 8192 {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(count) => request.extend_from_slice(&buffer[..count]),
        }
    }
    let text = String::from_utf8_lossy(&request);
    let mut parts = text.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    if method != "GET" {
        let _ = respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            "only GET",
        )
        .await;
        return;
    }
    let path = path.split('?').next().unwrap_or("/");
    let result = match path {
        "/" | "/index.html" => {
            respond(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                INDEX_HTML,
            )
            .await
        }
        "/app.js" => {
            respond(
                &mut stream,
                "200 OK",
                "text/javascript; charset=utf-8",
                APP_JS,
            )
            .await
        }
        "/app.css" => respond(&mut stream, "200 OK", "text/css; charset=utf-8", APP_CSS).await,
        "/panel" | "/panel.html" => {
            respond(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                PANEL_HTML,
            )
            .await
        }
        "/panel.js" => {
            respond(
                &mut stream,
                "200 OK",
                "text/javascript; charset=utf-8",
                PANEL_JS,
            )
            .await
        }
        "/panel.css" => respond(&mut stream, "200 OK", "text/css; charset=utf-8", PANEL_CSS).await,
        "/signal-faces.js" => {
            respond(
                &mut stream,
                "200 OK",
                "text/javascript; charset=utf-8",
                SIGNAL_FACES_JS,
            )
            .await
        }
        "/spdrs60-tiles.js" => {
            respond(
                &mut stream,
                "200 OK",
                "text/javascript; charset=utf-8",
                TILES_JS,
            )
            .await
        }
        "/api/layout" => {
            respond(
                &mut stream,
                "200 OK",
                "application/json",
                &shared.layout_json,
            )
            .await
        }
        "/api/state" => {
            let state = shared.state.borrow().clone();
            respond(&mut stream, "200 OK", "application/json", &state).await
        }
        "/events" => events(&mut stream, shared.state.clone()).await,
        // The module files, for the module editor: a list, and each file by its name.
        "/modules" | "/modules/" => {
            let list = module_list(&shared.modules);
            respond(&mut stream, "200 OK", "application/json", &list).await
        }
        _ if path.starts_with("/modules/") => {
            let name = &path["/modules/".len()..];
            match module_file(&shared.modules, name) {
                Some(text) => respond(&mut stream, "200 OK", "application/json", &text).await,
                None => respond(&mut stream, "404 Not Found", "text/plain", "no such module").await,
            }
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", "not found").await,
    };
    let _ = result;
}

async fn respond(
    stream: &mut TcpStream,
    status: &str,
    kind: &str,
    body: &str,
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await
}

/// Server-Sent Events: the live state, every time it changes, until the page goes.
async fn events(stream: &mut TcpStream, mut state: watch::Receiver<String>) -> std::io::Result<()> {
    stream
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n")
        .await?;
    loop {
        let json = state.borrow_and_update().clone();
        if !json.is_empty() {
            stream
                .write_all(format!("data: {json}\n\n").as_bytes())
                .await?;
        }
        if state.changed().await.is_err() {
            return Ok(());
        }
    }
}
