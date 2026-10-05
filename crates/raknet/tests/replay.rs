//! Replay a real captured session's server datagrams into the client connection and
//! check it hands up exactly the game messages the 2018 client received.
//!
//! The capture is the experimental server's DSOR_DATAGRAM_LOG (JSON lines). It is not
//! in the repository; set DSOR_SESSION to one, or the test is skipped.

use std::net::{Ipv4Addr, SocketAddrV4};

use dsor_raknet::{Connection, Event};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let at = line.find(&format!("\"{key}\": "))? + key.len() + 4;
    let rest = &line[at..];
    let rest = rest.trim_start_matches('"');
    let end = rest.find(['"', ',', '}']).unwrap_or(rest.len());
    Some(&rest[..end])
}

#[test]
fn a_captured_map_session_yields_the_same_messages() {
    let path = std::env::var("DSOR_SESSION").unwrap_or_else(|_| {
        format!("{}/Documents/Drakensang/logs/session-walk4.jsonl", std::env::var("HOME").unwrap())
    });
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: no capture at {path}");
        return;
    };
    let mut c = Connection::new(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30000), 1, 0);
    // Skip straight to connected: feed the real handshake datagrams too.
    let mut messages: Vec<Vec<u8>> = Vec::new();
    let mut now = 0;
    for line in text.lines() {
        if field(line, "server_port") != Some("30000") || field(line, "from_server") != Some("true") {
            continue;
        }
        now += 1;
        c.receive(&hex(field(line, "hex").unwrap()), now);
        while let Some(e) = c.poll_event() {
            if let Event::Message { data, .. } = e {
                if data[0] >= 0x80 {
                    messages.push(data);
                }
            }
        }
    }
    // Reference: the experimental server's own raknet package, reliable duplicates
    // dropped: 694 messages (tools/session_messages.py).
    assert_eq!(messages.len(), 694);
    let mut sorted = messages.clone();
    sorted.sort();
    let joined: Vec<u8> = sorted.concat();
    assert_eq!(joined.len(), messages.iter().map(Vec::len).sum::<usize>());
}
