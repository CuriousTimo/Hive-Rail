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
/// Event lines kept for the page's log.
const LOG_LINES: usize = 40;

struct Shared {
    layout_json: String,
    state: watch::Receiver<String>,
}

/// `--web-sim <layout.json> [--port 8080] [--speed 1]`.
pub async fn run_sim(args: Vec<String>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = args.into_iter();
    let path = args
        .next()
        .ok_or("expected a layout file after --web-sim")?;
    let path = Path::new(&path).to_path_buf();
    let mut port = 8080_u16;
    let mut speed = 1.0_f64;
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("expected a value after {flag}"))
        };
        match flag.as_str() {
            "--port" => port = value()?.parse().map_err(|_| "invalid --port")?,
            "--speed" => speed = value()?.parse().map_err(|_| "invalid --speed")?,
            _ => return Err(format!("unknown --web-sim option: {flag}").into()),
        }
    }
    let layout = layout::load(&path, path.parent().unwrap_or(Path::new(".")))?;
    let report = layout.check();
    if !report.errors.is_empty() {
        return Err(format!(
            "the layout has errors; run --check-layout {}",
            path.display()
        )
        .into());
    }
    // The simulation borrows the layout and its graph for the whole run.
    let layout: &'static layout::Layout = Box::leak(Box::new(layout));
    let topology: &'static Topology = Box::leak(Box::new(Topology::build(layout)));
    let view = ViewBuilder::new(layout, topology);
    let layout_json = serde_json::to_string(&view.layout(layout, topology))?;
    let mut sim = LayoutSim::new(layout, topology);
    layout_sim::example_trains(&mut sim);

    let (publish, state) = watch::channel(String::new());
    let shared = Arc::new(Shared { layout_json, state });
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    println!("Hive-Rail web view of {} on http://localhost:{port} (and this computer's address on the network)", layout.file.meeting);
    let server = tokio::spawn(accept(listener, Arc::clone(&shared)));

    let step = 0.1;
    let mut tick = time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut log: VecDeque<String> = VecDeque::new();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tick.tick() => {
                for (time, event) in sim.step(step * speed) {
                    let line = format!("{:>7.1} s  {event}", time);
                    if log.len() == LOG_LINES {
                        log.pop_front();
                    }
                    log.push_back(line);
                }
                let snapshot = view.snapshot(&sim, topology);
                let mut json = serde_json::to_value(&snapshot)?;
                json["log"] = serde_json::Value::from(log.iter().cloned().collect::<Vec<_>>());
                let _ = publish.send(json.to_string());
            }
        }
    }
    server.abort();
    Ok(())
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
