//! `dsor-relay`: WebSocket <-> UDP relay for the browser client.
//!
//! Usage:
//!   dsor-relay [--listen 0.0.0.0:2290] [--allow-host 127.0.0.1]... [--allow-ports 2190-2192,30000-30100]
//!
//! Each WebSocket names its UDP target in the query: ws://host:2290/?target=127.0.0.1:2190

use dsor_relay::{parse_port_ranges, Config};
use std::process::ExitCode;

const USAGE: &str = "usage: dsor-relay [--listen ADDR] [--allow-host HOST]... [--allow-ports RANGES]
  --listen ADDR         WebSocket listen address (default 0.0.0.0:2290)
  --allow-host HOST     UDP target host allowed (repeatable; default 127.0.0.1)
  --allow-ports RANGES  UDP target ports allowed (default 2190-2192,30000-30100)
Logging: RUST_LOG=info (default) / debug";

fn parse_args() -> Result<Config, String> {
    let mut cfg = Config::default();
    let mut hosts: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let (key, inline) = match a.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let mut val = |name: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match key.as_str() {
            "--listen" => {
                let v = val("--listen")?;
                cfg.listen = v.parse().map_err(|e| format!("--listen {v}: {e}"))?;
            }
            "--allow-host" => hosts.push(val("--allow-host")?),
            "--allow-ports" => cfg.allow_ports = parse_port_ranges(&val("--allow-ports")?)?,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if !hosts.is_empty() {
        cfg.allow_hosts = hosts;
    }
    Ok(cfg)
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cfg = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(async move {
        let listener = match tokio::net::TcpListener::bind(cfg.listen).await {
            Ok(l) => l,
            Err(e) => {
                log::error!("bind {}: {e}", cfg.listen);
                return ExitCode::FAILURE;
            }
        };
        log::info!(
            "listening on ws://{} (hosts {:?}, ports {:?})",
            cfg.listen,
            cfg.allow_hosts,
            cfg.allow_ports
        );
        dsor_relay::serve(listener, cfg.into()).await;
        ExitCode::SUCCESS
    })
}
