//! WebSocket <-> UDP relay carrying RakNet datagrams for the browser client.
//!
//! One WebSocket connection = one ephemeral UDP socket `connect()`ed to the target
//! named in the URL query (`/?target=host:port`). Every binary WebSocket message is
//! sent as exactly one UDP datagram, and every received datagram is forwarded as
//! exactly one binary WebSocket message. Nothing is reframed, batched or buffered.
//!
//! CONTRACT: the target must pass the allowlist (`Config::allow_hosts` textual host
//! match, `Config::allow_ports` port ranges) or the HTTP upgrade is refused with 403.

use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::Message;

/// Largest UDP payload we can receive.
const MAX_DATAGRAM: usize = 65_536;

#[derive(Clone, Debug)]
pub struct Config {
    pub listen: SocketAddr,
    /// Allowed target hosts, compared textually (case-insensitive, IPv6 brackets stripped).
    pub allow_hosts: Vec<String>,
    /// Allowed target port ranges, inclusive.
    pub allow_ports: Vec<(u16, u16)>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: "0.0.0.0:2290".parse().unwrap(),
            allow_hosts: vec!["127.0.0.1".to_string()],
            allow_ports: vec![(2190, 2192), (30000, 30100)],
        }
    }
}

/// Parse "2190-2192,30000-30100,4000" into inclusive ranges.
pub fn parse_port_ranges(s: &str) -> Result<Vec<(u16, u16)>, String> {
    let mut out = Vec::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (part, part),
        };
        let a: u16 = a.parse().map_err(|_| format!("bad port {a:?} in {s:?}"))?;
        let b: u16 = b.parse().map_err(|_| format!("bad port {b:?} in {s:?}"))?;
        if a > b {
            return Err(format!("empty port range {part:?}"));
        }
        out.push((a, b));
    }
    if out.is_empty() {
        return Err(format!("no ports in {s:?}"));
    }
    Ok(out)
}

/// Minimal percent-decoding for query values (`127.0.0.1%3A2190`).
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Extract `target` from a query string.
fn query_target(query: Option<&str>) -> Option<String> {
    query?
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| *k == "target")
        .and_then(|(_, v)| percent_decode(v))
}

/// Split "host:port" / "[v6]:port" and check it against the allowlist.
/// Returns the (host, port) to resolve on success.
pub fn check_target(cfg: &Config, target: &str) -> Result<(String, u16), String> {
    let (host, port) = target
        .rsplit_once(':')
        .ok_or_else(|| format!("target {target:?} is not host:port"))?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port: u16 = port
        .parse()
        .map_err(|_| format!("target {target:?} has a bad port"))?;
    if host.is_empty() {
        return Err(format!("target {target:?} has no host"));
    }
    if !cfg.allow_hosts.iter().any(|h| {
        h.trim_start_matches('[')
            .trim_end_matches(']')
            .eq_ignore_ascii_case(host)
    }) {
        return Err(format!("host {host:?} not allowed"));
    }
    if !cfg.allow_ports.iter().any(|&(a, b)| (a..=b).contains(&port)) {
        return Err(format!("port {port} not allowed"));
    }
    Ok((host.to_string(), port))
}

/// Accept WebSocket connections forever.
pub async fn serve(listener: TcpListener, cfg: Arc<Config>) {
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let cfg = cfg.clone();
                tokio::spawn(async move { handle(stream, peer, cfg).await });
            }
            Err(e) => {
                log::warn!("accept: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
}

fn reject(status: StatusCode, msg: String) -> ErrorResponse {
    let mut r = ErrorResponse::new(Some(msg));
    *r.status_mut() = status;
    r
}

async fn handle(stream: TcpStream, peer: SocketAddr, cfg: Arc<Config>) {
    // No Nagle: every datagram leaves as soon as it is written.
    if let Err(e) = stream.set_nodelay(true) {
        log::warn!("{peer}: set_nodelay: {e}");
    }

    let mut checked: Option<(String, u16)> = None;
    let callback = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        let Some(target) = query_target(req.uri().query()) else {
            return Err(reject(
                StatusCode::BAD_REQUEST,
                "missing ?target=host:port".into(),
            ));
        };
        match check_target(&cfg, &target) {
            Ok(t) => {
                checked = Some(t);
                Ok(resp)
            }
            Err(e) => {
                log::warn!("{peer}: rejected target {target:?}: {e}");
                Err(reject(StatusCode::FORBIDDEN, e))
            }
        }
    };
    let ws = match tokio_tungstenite::accept_hdr_async(stream, callback).await {
        Ok(ws) => ws,
        Err(e) => {
            log::debug!("{peer}: handshake failed: {e}");
            return;
        }
    };
    let Some((host, port)) = checked else { return };

    let addr = match tokio::net::lookup_host((host.as_str(), port)).await {
        Ok(mut it) => match it.next() {
            Some(a) => a,
            None => {
                log::warn!("{peer}: {host}:{port} resolved to nothing");
                return;
            }
        },
        Err(e) => {
            log::warn!("{peer}: resolve {host}:{port}: {e}");
            return;
        }
    };
    let bind: SocketAddr = if addr.is_ipv4() {
        "0.0.0.0:0".parse().unwrap()
    } else {
        "[::]:0".parse().unwrap()
    };
    let udp = match UdpSocket::bind(bind).await {
        Ok(u) => u,
        Err(e) => {
            log::warn!("{peer}: udp bind: {e}");
            return;
        }
    };
    if let Err(e) = udp.connect(addr).await {
        log::warn!("{peer}: udp connect {addr}: {e}");
        return;
    }
    let local = udp.local_addr().ok();
    log::info!("{peer}: connected -> udp {addr} (local {local:?})");

    let (mut ws_tx, mut ws_rx) = ws.split();
    let mut buf = vec![0u8; MAX_DATAGRAM];
    let (mut up_n, mut up_b, mut down_n, mut down_b) = (0u64, 0u64, 0u64, 0u64);
    let reason: String = loop {
        tokio::select! {
            msg = ws_rx.next() => match msg {
                Some(Ok(Message::Binary(data))) => {
                    match udp.send(&data).await {
                        Ok(_) => { up_n += 1; up_b += data.len() as u64; }
                        // ICMP errors from a previous datagram surface here; keep going.
                        Err(e) => log::debug!("{peer}: udp send: {e}"),
                    }
                }
                Some(Ok(Message::Close(_))) | None => break "ws closed".into(),
                Some(Ok(Message::Text(_))) => log::debug!("{peer}: ignoring text message"),
                Some(Ok(_)) => {} // ping/pong handled by tungstenite
                Some(Err(e)) => break format!("ws error: {e}"),
            },
            r = udp.recv(&mut buf) => match r {
                Ok(n) => {
                    down_n += 1;
                    down_b += n as u64;
                    if let Err(e) = ws_tx.send(Message::Binary(buf[..n].to_vec())).await {
                        break format!("ws send: {e}");
                    }
                }
                // e.g. ECONNREFUSED while the server port is closed; not fatal.
                Err(e) => log::debug!("{peer}: udp recv: {e}"),
            },
        }
    };
    let _ = ws_tx.close().await;
    drop(udp);
    log::info!(
        "{peer}: disconnected from {addr} ({reason}); up {up_n} dgrams/{up_b} B, down {down_n} dgrams/{down_b} B"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports() {
        assert_eq!(
            parse_port_ranges("2190-2192,30000-30100").unwrap(),
            vec![(2190, 2192), (30000, 30100)]
        );
        assert_eq!(parse_port_ranges("80").unwrap(), vec![(80, 80)]);
        assert!(parse_port_ranges("5-1").is_err());
        assert!(parse_port_ranges("x").is_err());
    }

    #[test]
    fn targets() {
        let cfg = Config::default();
        assert!(check_target(&cfg, "127.0.0.1:2190").is_ok());
        assert!(check_target(&cfg, "127.0.0.1:30100").is_ok());
        assert!(check_target(&cfg, "127.0.0.1:2193").is_err());
        assert!(check_target(&cfg, "10.0.0.1:2190").is_err());
        assert!(check_target(&cfg, "127.0.0.1").is_err());
        assert_eq!(
            query_target(Some("a=b&target=127.0.0.1%3A2190")).as_deref(),
            Some("127.0.0.1:2190")
        );
    }
}
