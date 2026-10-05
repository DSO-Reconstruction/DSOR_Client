//! Replay captured sessions through the command codecs.
//!
//! Server -> client: every 0x84 / 0x85 game message must decode to its end (at most
//! seven padding bits over) with no Unknown command. Client -> server: every 0x8B
//! message must decode and re-encode to the same bits.
//!
//! The captures are the experimental server's DSOR_DATAGRAM_LOG (one JSON line per
//! datagram, both directions). They are not in the repository: the directory is
//! $DSOR_SESSIONS, default ~/Documents/Drakensang/logs; a missing capture is skipped.

use std::collections::{BTreeMap, HashMap, HashSet};

use dsor_proto::commands::{command_name, decode_message, encode_chain, encode_single, ClientCommand, ServerCommand};
use dsor_raknet::wire::parse_frames;

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

/// A reassembled game message.
struct Message {
    from_server: bool,
    frame: u64,
    data: Vec<u8>,
    bits: usize,
}

/// Every game message (first byte >= 0x80) of a capture, both directions, reliable
/// duplicates dropped and split messages joined.
fn messages(text: &str) -> Vec<Message> {
    let mut out = Vec::new();
    let mut seen: HashSet<(String, String, bool, u32)> = HashSet::new();
    let mut splits: HashMap<(String, String, bool, u16), BTreeMap<u32, (Vec<u8>, usize)>> = HashMap::new();
    for line in text.lines() {
        let (Some(conn), Some(port), Some(dir), Some(h), Some(frame)) = (
            field(line, "conn"),
            field(line, "server_port"),
            field(line, "from_server"),
            field(line, "hex"),
            field(line, "frame"),
        ) else {
            continue;
        };
        let from_server = dir == "true";
        let raw = hex(h);
        // Offline messages, ACKs and NAKs carry no frames.
        if raw.is_empty() || raw[0] & 0x80 == 0 || raw[0] & 0x40 != 0 || raw[0] & 0x20 != 0 {
            continue;
        }
        let Ok(frames) = parse_frames(&raw, 4) else { continue };
        for f in frames {
            if f.reliability.is_reliable()
                && !seen.insert((conn.to_string(), port.to_string(), from_server, f.reliable_index))
            {
                continue;
            }
            let (data, bits) = match f.split {
                None => (f.payload, f.bit_length),
                Some(s) => {
                    let key = (conn.to_string(), port.to_string(), from_server, s.id);
                    let parts = splits.entry(key.clone()).or_default();
                    parts.insert(s.index, (f.payload, f.bit_length));
                    if parts.len() < s.count as usize {
                        continue;
                    }
                    let parts = splits.remove(&key).unwrap();
                    let bits = parts.values().map(|p| p.1).sum();
                    (parts.into_values().flat_map(|p| p.0).collect(), bits)
                }
            };
            if data.first().is_some_and(|&b| b >= 0x80) {
                out.push(Message { from_server, frame: frame.parse().unwrap_or(0), data, bits });
            }
        }
    }
    out
}

fn sessions_dir() -> String {
    std::env::var("DSOR_SESSIONS")
        .unwrap_or_else(|_| format!("{}/Documents/Drakensang/logs", std::env::var("HOME").unwrap_or_default()))
}

#[derive(Default)]
struct Coverage {
    total: usize,
    clean: usize,
    unknown: usize,
    failed: usize,
    roundtrip_mismatch: usize,
    commands: usize,
    unknown_ids: BTreeMap<u16, usize>,
    failures: BTreeMap<String, (usize, String)>,
}

impl Coverage {
    fn percent(&self) -> f64 {
        if self.total == 0 {
            100.0
        } else {
            100.0 * self.clean as f64 / self.total as f64
        }
    }
}

fn server_coverage(msgs: &[Message]) -> Coverage {
    let mut c = Coverage::default();
    for m in msgs.iter().filter(|m| m.from_server && matches!(m.data[0], 0x84 | 0x85)) {
        c.total += 1;
        match decode_message(&m.data, m.bits) {
            Ok(commands) => {
                c.commands += commands.len();
                let unknown: Vec<u16> = commands.iter().filter(|c| c.0.is_unknown()).map(|c| c.0.id()).collect();
                for id in &unknown {
                    *c.unknown_ids.entry(*id).or_default() += 1;
                }
                if !unknown.is_empty() {
                    c.unknown += 1;
                    continue;
                }
                c.clean += 1;
                // Re-encode: the decoded fields must reproduce the wire bits.
                let with_actors: Vec<(ServerCommand, u32)> =
                    commands.iter().map(|(cmd, a)| (cmd.clone(), a.unwrap_or(0))).collect();
                let (bytes, bits) = if m.data[0] == 0x84 {
                    encode_single(&with_actors[0].0, with_actors[0].1)
                } else {
                    encode_chain(&with_actors)
                };
                if bits > m.bits || m.bits - bits > 7 || bytes[..] != m.data[..bytes.len()] {
                    c.roundtrip_mismatch += 1;
                    let key = format!("roundtrip {}", commands.iter().map(|c| c.0.id().to_string()).collect::<Vec<_>>().join(","));
                    c.failures.entry(key).or_insert((0, format!("frame {}", m.frame))).0 += 1;
                }
            }
            Err(e) => {
                c.failed += 1;
                let key = match &e {
                    dsor_proto::commands::DecodeError::InCommand { id, .. } => {
                        format!("{id} {}", command_name(*id).unwrap_or("?"))
                    }
                    other => format!("{other:?}"),
                };
                let entry = c.failures.entry(key).or_insert((0, format!("frame {}: {e}", m.frame)));
                entry.0 += 1;
            }
        }
    }
    c
}

#[derive(Default)]
struct ClientCoverage {
    total: usize,
    clean: usize,
    unknown: BTreeMap<u16, usize>,
    failures: BTreeMap<u16, (usize, String)>,
}

fn client_coverage(msgs: &[Message]) -> ClientCoverage {
    let mut c = ClientCoverage::default();
    for m in msgs.iter().filter(|m| !m.from_server && m.data[0] == 0x8B) {
        c.total += 1;
        let id = u16::from_le_bytes([m.data[1], m.data.get(2).copied().unwrap_or(0)]);
        match ClientCommand::decode(&m.data, m.bits) {
            Ok(ClientCommand::Unknown { id, .. }) => *c.unknown.entry(id).or_default() += 1,
            Ok(cmd) => {
                let (bytes, bits) = cmd.encode();
                if bits == m.bits && bytes == m.data {
                    c.clean += 1;
                } else {
                    c.failures.entry(id).or_insert((0, format!("frame {}: re-encoded {bits} bits differ", m.frame))).0 += 1;
                }
            }
            Err(e) => c.failures.entry(id).or_insert((0, format!("frame {}: {e}", m.frame))).0 += 1,
        }
    }
    c
}

fn report(name: &str) -> Option<(Coverage, ClientCoverage)> {
    let path = format!("{}/session-{name}.jsonl", sessions_dir());
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: no capture at {path}");
        return None;
    };
    let msgs = messages(&text);
    let s = server_coverage(&msgs);
    let c = client_coverage(&msgs);
    eprintln!(
        "session-{name}: server {}/{} messages clean ({:.2}%), {} commands; {} with Unknown, {} failed, {} re-encode mismatches",
        s.clean, s.total, s.percent(), s.commands, s.unknown, s.failed, s.roundtrip_mismatch
    );
    for (id, n) in &s.unknown_ids {
        eprintln!("  unknown server command {id} {} x{n}", command_name(*id).unwrap_or("?"));
    }
    for (k, (n, first)) in &s.failures {
        eprintln!("  failed {k} x{n} (first: {first})");
    }
    let unknown: usize = c.unknown.values().sum();
    eprintln!(
        "session-{name}: client {}/{} messages decode and re-encode exactly, {} Unknown",
        c.clean, c.total, unknown
    );
    for (id, n) in &c.unknown {
        eprintln!("  unknown client command {id} {} x{n}", command_name(*id).unwrap_or("?"));
    }
    for (id, (n, first)) in &c.failures {
        eprintln!("  client {id} {} failed x{n} (first: {first})", command_name(*id).unwrap_or("?"));
    }
    Some((s, c))
}

/// The two reference sessions: every server message decodes completely.
#[test]
fn reference_sessions_decode_completely() {
    for name in ["walk4", "combat1"] {
        let Some((s, c)) = report(name) else { continue };
        assert_eq!(s.clean, s.total, "session-{name}: not every server message decoded cleanly");
        assert_eq!(s.roundtrip_mismatch, 0, "session-{name}: re-encoding changed bits");
        assert!(c.failures.is_empty(), "session-{name}: client commands failed");
    }
}

/// Every other capture, reported but not required (run with --nocapture to read it).
#[test]
fn all_sessions_report() {
    let Ok(dir) = std::fs::read_dir(sessions_dir()) else {
        eprintln!("skipped: no session directory");
        return;
    };
    let mut names: Vec<String> = dir
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| Some(n.strip_prefix("session-")?.strip_suffix(".jsonl")?.to_string()))
        .filter(|n| n != "walk4" && n != "combat1")
        .collect();
    names.sort();
    if std::env::var("DSOR_ALL_SESSIONS").is_err() {
        eprintln!("set DSOR_ALL_SESSIONS=1 to replay {} more captures", names.len());
        return;
    }
    for name in names {
        report(&name);
    }
}
