//! monitor-agent: reports host metrics to a monitor hub over WebSocket.

mod collect;

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::{Sink, SinkExt, StreamExt};
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

use collect::Collector;
use std::fs::OpenOptions;
use std::io::Write;

fn log(msg: impl std::fmt::Display) {
    let text = msg.to_string();
    eprintln!("{text}");
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(dir) = exe_path.parent() {
            let log_file = dir.join("agent.log");
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_file) {
                let _ = writeln!(f, "{text}");
            }
        }
    }
}

struct Args {
    server: String,
    token: String,
    interval: u64,
    /// Permits plain HTTP to a hub reached at ip:port with no TLS in front.
    insecure: bool,
}

fn usage() -> ! {
    eprintln!(
        "monitor-agent {}\n\n\
         Usage: monitor-agent --server <url> --token <token> [options]\n\n\
         Options:\n  \
           --server <url>       Hub base URL, e.g. https://hub.example.com\n  \
           --token <token>      Node token from the hub panel\n  \
           --interval <secs>    Report interval (default 1)\n  \
           --insecure           Allow plain ws:// to a remote hub; the token\n  \
                                travels in the clear. Only for a hub reached\n  \
                                at ip:port with no TLS in front.\n",
        env!("CARGO_PKG_VERSION")
    );
    std::process::exit(2)
}

fn parse_args() -> Result<Args> {
    let (mut server, mut token, mut interval, mut insecure) = (None, None, 1u64, false);
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| usage());
        match arg.as_str() {
            "--server" => server = Some(value()),
            "--token" => token = Some(value()),
            "--interval" => interval = value().parse().unwrap_or_else(|_| usage()),
            "--insecure" => insecure = true,
            "-h" | "--help" => usage(),
            other => bail!("unknown argument: {other}"),
        }
    }
    let server = server.or_else(|| std::env::var("MONITOR_SERVER").ok()).unwrap_or_else(|| usage());
    let token = token.or_else(|| std::env::var("MONITOR_TOKEN").ok()).unwrap_or_else(|| usage());
    Ok(Args {
        server,
        token,
        interval: interval.clamp(1, 3600),
        insecure,
    })
}

fn ws_url(server: &str, insecure: bool) -> Result<String> {
    let base = server.trim_end_matches('/');
    let scheme = if insecure { "ws" } else { "wss" };
    let base = match base.split_once("://") {
        Some(("https", rest)) => format!("wss://{rest}"),
        Some(("http", rest)) => format!("ws://{rest}"),
        Some(("wss" | "ws", _)) => base.to_owned(),
        _ => format!("{scheme}://{base}"),
    };

    let authority = base.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("");
    if authority.contains('@') {
        bail!("server URL must not contain '@'");
    }
    if base.starts_with("ws://") && !insecure && !is_loopback(&base) {
        bail!(
            "refusing plaintext ws:// to a remote hub; the token would travel in the clear. \
             Pass --insecure if that hub really has no TLS"
        );
    }
    Ok(format!("{base}/api/agent/ws"))
}

fn is_loopback(url: &str) -> bool {
    let authority = url.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("");
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    };
    host.parse::<std::net::IpAddr>().map_or(host == "localhost", |ip| ip.is_loopback())
}

#[derive(Deserialize)]
struct Rpc {
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Deserialize, Clone, Debug)]
struct PingTask {
    id: i64,
    target: String,
    interval: u64,
}

fn notify(method: &str, params: serde_json::Value) -> Message {
    Message::Text(
        serde_json::json!({"jsonrpc": "2.0", "method": method, "params": params}).to_string().into(),
    )
}

async fn send(
    ws: &mut (impl Sink<Message, Error = WsError> + Unpin),
    m: Message,
    budget: Duration,
) -> Result<()> {
    tokio::time::timeout(budget, ws.send(m))
        .await
        .map_err(|_| anyhow!("write stalled for {}s", budget.as_secs()))?
        .context("write")
}

fn remaining(last_frame: Instant) -> Duration {
    HUB_SILENCE.saturating_sub(last_frame.elapsed())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = parse_args()?;
    let url = ws_url(&args.server, args.insecure)?;

    let mut collector = Collector::new();
    let mut wait = 0u64;

    loop {
        let mut connected = None;
        if let Err(e) = session(&url, &args.token, &mut collector, args.interval, &mut connected).await {
            log(format!("session ended: {e:#}"));
        }
        wait = reconnect_wait(wait, connected.map_or(Duration::ZERO, |t: Instant| t.elapsed()));
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
}

fn reconnect_wait(previous: u64, lasted: Duration) -> u64 {
    if lasted >= Duration::from_secs(30) {
        1
    } else {
        (previous * 2).clamp(1, 60)
    }
}

const CONNECT_DEADLINE: Duration = Duration::from_secs(120);
const DIAL_FALLBACK: Duration = Duration::from_secs(5);
const MAX_MESSAGE: usize = 64 * 1024;
const HUB_SILENCE: Duration = Duration::from_secs(90);

async fn session(
    url: &str,
    token: &str,
    collector: &mut Collector,
    interval: u64,
    connected: &mut Option<Instant>,
) -> Result<()> {
    let mut request = url.into_client_request()?;
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().context("token is not header-safe")?);
    let config =
        WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
    let uri = request.uri();
    let host = uri
        .host()
        .context("server URL has no host")?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = uri.port_u16().unwrap_or(if uri.scheme_str() == Some("wss") { 443 } else { 80 });

    let facts = collector.facts();
    let behind_nat = facts.ipv4.parse().is_ok_and(|ip| !collect::is_public(ip));
    let connect = async {
        let stream = dial(&host, port, behind_nat).await?;
        let peer = stream.peer_addr()?;
        let (ws, _) = tokio_tungstenite::client_async_tls_with_config(request, stream, Some(config), None)
            .await
            .context("handshake")?;
        anyhow::Ok((ws, peer))
    };
    let (mut ws, peer) = tokio::time::timeout(CONNECT_DEADLINE, connect)
        .await
        .with_context(|| format!("no connection after {}s", CONNECT_DEADLINE.as_secs()))??;
    log(format!("connected to {peer}"));
    *connected = Some(Instant::now());

    let mut last_frame = Instant::now();

    send(&mut ws, notify("hello", serde_json::to_value(facts)?), remaining(last_frame)).await?;

    let (result_tx, mut result_rx) = mpsc::channel::<Message>(64);
    let mut ping_tasks: Vec<(PingTask, tokio::task::JoinHandle<()>)> = Vec::new();
    let mut ticker = tokio::time::interval(Duration::from_secs(interval));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let result = loop {
        tokio::select! {
            _ = ticker.tick() => {
                let m = serde_json::to_value(collector.collect())?;
                if let Err(e) = send(&mut ws, notify("report", m), remaining(last_frame)).await { break Err(e); }
            }
            _ = tokio::time::sleep(remaining(last_frame)) => {
                break Err(anyhow!("no frame from the hub in {}s", HUB_SILENCE.as_secs()));
            }
            Some(msg) = result_rx.recv() => {
                if let Err(e) = send(&mut ws, msg, remaining(last_frame)).await { break Err(e); }
            }
            incoming = ws.next() => {
                last_frame = Instant::now();
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(rpc) = serde_json::from_str::<Rpc>(&text) {
                            if rpc.method == "ping.tasks" {
                                if let Ok(tasks) = serde_json::from_value::<Vec<PingTask>>(rpc.params) {
                                    respawn_ping_tasks(&mut ping_tasks, tasks, &result_tx);
                                }
                            }
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => break Err(e.into()),
                    None => break Ok(()),
                }
            }
        }
    };

    for (_, handle) in ping_tasks {
        handle.abort();
    }
    result
}

async fn dial(host: &str, port: u16, prefer_v4: bool) -> Result<TcpStream> {
    let mut addrs: Vec<std::net::SocketAddr> =
        tokio::net::lookup_host((host, port)).await.with_context(|| format!("resolve {host}"))?.collect();
    if prefer_v4 {
        addrs.sort_by_key(|a| !a.is_ipv4());
    }
    connect_first(&addrs).await.with_context(|| format!("connect {host}"))
}

async fn connect_first(addrs: &[std::net::SocketAddr]) -> Result<TcpStream> {
    let mut failures = Vec::new();
    for (i, addr) in addrs.iter().enumerate() {
        let attempt = TcpStream::connect(addr);
        let result = if i + 1 < addrs.len() {
            tokio::time::timeout(DIAL_FALLBACK, attempt)
                .await
                .unwrap_or_else(|_| Err(std::io::ErrorKind::TimedOut.into()))
        } else {
            attempt.await
        };
        match result {
            Ok(stream) => return Ok(stream),
            Err(e) => failures.push(format!("{addr}: {e}")),
        }
    }
    if failures.is_empty() {
        bail!("no address");
    }
    bail!("{}", failures.join("; "))
}

const MAX_PING_TASKS: usize = 64;

fn respawn_ping_tasks(
    running: &mut Vec<(PingTask, tokio::task::JoinHandle<()>)>,
    mut wanted: Vec<PingTask>,
    tx: &mpsc::Sender<Message>,
) {
    if wanted.len() > MAX_PING_TASKS {
        log(format!("hub asked for {} ping tasks, running {MAX_PING_TASKS}", wanted.len()));
        wanted.truncate(MAX_PING_TASKS);
    }
    running.retain(|(task, handle)| {
        let keep =
            wanted.iter().any(|w| w.id == task.id && w.target == task.target && w.interval == task.interval);
        if !keep {
            handle.abort();
        }
        keep
    });
    for task in wanted {
        if running.iter().any(|(t, _)| t.id == task.id) {
            continue;
        }
        let (tx, spawned) = (tx.clone(), task.clone());
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(spawned.interval.clamp(5, 3600)));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut said = false;
            loop {
                ticker.tick().await;
                let Some(latency) = tcp_ping(&spawned.target).await else {
                    if !std::mem::replace(&mut said, true) {
                        log(format!(
                            "{}: name resolution runs past {}ms",
                            spawned.target,
                            HANDSHAKE_DEADLINE.as_millis()
                        ));
                    }
                    continue;
                };
                let msg =
                    notify("ping.result", serde_json::json!({"task_id": spawned.id, "latency_ms": latency}));
                if tx.send(msg).await.is_err() {
                    return;
                }
            }
        });
        running.push((task, handle));
    }
}

const HANDSHAKE_DEADLINE: Duration = Duration::from_millis(900);
const MAX_PING_ADDRS: usize = 3;

async fn tcp_ping(target: &str) -> Option<i32> {
    let Ok(resolved) = tokio::time::timeout(HANDSHAKE_DEADLINE, tokio::net::lookup_host(target)).await else {
        return None;
    };
    let Ok(addresses) = resolved else { return Some(-1) };
    Some(handshake(addresses).await)
}

async fn handshake(addresses: impl Iterator<Item = std::net::SocketAddr>) -> i32 {
    for address in addresses.take(MAX_PING_ADDRS) {
        let started = std::time::Instant::now();
        if let Ok(Ok(_)) = tokio::time::timeout(HANDSHAKE_DEADLINE, TcpStream::connect(address)).await {
            return started.elapsed().as_millis().min(i32::MAX as u128) as i32;
        }
    }
    -1
}
