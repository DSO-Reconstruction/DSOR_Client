//! Headless login against a live server: prints each step until the map's world arrives.
//!
//! cargo run -p dsor-proto --example login -- 127.0.0.1:2190 <account> <session-uuid> [character]

use std::net::UdpSocket;
use std::time::{Duration, Instant};

use dsor_proto::identity::Credentials;
use dsor_proto::session::{Session, SessionEvent};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let login = args.get(1).map(String::as_str).unwrap_or("127.0.0.1:2190").parse().expect("login addr");
    let creds = Credentials::parse(
        args.get(2).map(String::as_str).unwrap_or("107909470"),
        args.get(3).map(String::as_str).unwrap_or("0123456789abcdef0123456789abcdef"),
    )
    .expect("credentials");
    let wanted: Option<u32> = args.get(4).and_then(|c| c.parse().ok());
    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u64;
    let mut session = Session::new(login, creds, 0x0660_0001_a136_24d9 ^ 0x5a5a, now());
    let mut socket: Option<UdpSocket> = None;
    let mut buf = [0u8; 2048];
    let mut commands = 0usize;
    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline {
        while let Some(ev) = session.poll_event() {
            match ev {
                SessionEvent::Connect { target, farewell } => {
                    if let Some(s) = &socket {
                        for d in farewell {
                            let _ = s.send(&d);
                        }
                    }
                    let s = UdpSocket::bind("0.0.0.0:0").unwrap();
                    s.connect(target).unwrap();
                    s.set_nonblocking(true).unwrap();
                    socket = Some(s);
                    println!("[{:>5} ms] connect {target}", now());
                }
                SessionEvent::Roster(r) => {
                    println!("[{:>5} ms] roster: {:?}", now(), r.iter().map(|e| (&e.name, e.character, e.level)).collect::<Vec<_>>());
                    let pick = wanted.or_else(|| r.first().map(|e| e.character));
                    match pick {
                        Some(c) => session.select_character(c, now()),
                        None => {
                            println!("no character on this account");
                            return;
                        }
                    }
                }
                SessionEvent::Commands { data, .. } => {
                    commands += 1;
                    if commands <= 25 {
                        let id = if data.len() >= 3 { u16::from_le_bytes([data[1], data[2]]) } else { 0 };
                        println!("[{:>5} ms] command {:#04x}/{} ({} bytes)", now(), data[0], id, data.len());
                    }
                }
                other => println!("[{:>5} ms] {other:?}", now()),
            }
        }
        if let Some(s) = &socket {
            loop {
                match s.recv(&mut buf) {
                    Ok(n) => session.receive(&buf[..n], now()),
                    Err(_) => break,
                }
            }
        }
        session.update(now());
        out.clear();
        session.drain_outgoing(&mut out);
        if let Some(s) = &socket {
            for d in &out {
                let _ = s.send(d);
            }
        }
        if commands > 200 {
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    println!("stage {:?}, {} game commands received, rtt {:.1} ms", session.stage(), commands, session.connection().stats.rtt_ms);
}
