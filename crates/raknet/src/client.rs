//! One RakNet connection from the client's side, with no I/O.
//!
//! Feed it every datagram that arrives (`receive`), call `update` every frame with the
//! current time, and send whatever `drain_outgoing` yields. It does the offline
//! handshake, CONNECTION_REQUEST / NEW_INCOMING_CONNECTION, pings, reliability
//! (acks, resends, duplicate suppression), ordering per channel, sequencing and split
//! reassembly, and packs outgoing frames into MTU-sized datagrams.
//!
//! Time is passed in (milliseconds, any monotonic origin) so the same code runs
//! natively and in the browser.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddrV4;

use crate::wire::{self, flag, id, Frame, Reliability, Split};

/// MTUs probed in order, as RakNet does: the 2018 client's first probe is 1292.
const MTU_PROBES: [usize; 3] = [1292, 1200, 576];
const PROBE_ATTEMPTS: u32 = 4;
const PROBE_INTERVAL_MS: u64 = 500;
const HANDSHAKE_TIMEOUT_MS: u64 = 10_000;
const PING_INTERVAL_MS: u64 = 5_000;
const SILENCE_TIMEOUT_MS: u64 = 15_000;
const MIN_RTO_MS: u64 = 100;
const MAX_RTO_MS: u64 = 2_000;
const ORDERING_CHANNELS: usize = 32;
/// Reliable indices behind the window base are duplicates; this many ahead are held.
const RELIABLE_WINDOW: u32 = 1 << 23;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// CONNECTION_REQUEST_ACCEPTED arrived; game messages may flow.
    Connected,
    /// A complete game (or RakNet-level) message, in delivery order.
    Message { data: Vec<u8>, bits: usize, reliability: Reliability },
    /// The connection ended: refused, lost, timed out or closed by the server.
    Disconnected(DisconnectReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisconnectReason {
    HandshakeTimeout,
    Refused(u8),
    Closed,
    TimedOut,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Probing { probe: usize, attempts: u32 },
    Requesting2 { attempts: u32 },
    Connecting,
    Connected,
    Closed,
}

struct Retained {
    datagram: Vec<u8>,
    sent_at: u64,
    attempts: u32,
}

#[derive(Default)]
struct Channel {
    /// The next ordering index to deliver.
    expected: u32,
    held: HashMap<u32, (Vec<u8>, usize, Reliability)>,
    /// Highest sequencing index delivered, for sequenced frames.
    highest_sequence: Option<u32>,
}

/// Counters for the debug overlay and tuning.
#[derive(Default, Debug, Clone, Copy)]
pub struct Stats {
    pub datagrams_sent: u64,
    pub datagrams_received: u64,
    pub resends: u64,
    pub duplicates: u64,
    pub rtt_ms: f32,
}

pub struct Connection {
    server: SocketAddrV4,
    guid: u64,
    state: State,
    started: u64,
    last_send_offline: u64,
    last_heard: u64,
    last_ping: u64,
    mtu: usize,
    server_guid: u64,
    local_address: Option<SocketAddrV4>,

    out: Vec<Vec<u8>>,
    pending: VecDeque<Frame>,
    next_sequence: u32,
    next_reliable: u32,
    next_ordering: [u32; ORDERING_CHANNELS],
    next_sequencing: [u32; ORDERING_CHANNELS],
    next_split_id: u16,
    sent: HashMap<u32, Retained>,
    rto_ms: u64,
    srtt: Option<f32>,

    acks_due: Vec<u32>,
    received_base: u32,
    received_above: std::collections::HashSet<u32>,
    splits: HashMap<u16, (u32, Vec<Option<Vec<u8>>>, usize, Frame)>,
    channels: Vec<Channel>,
    events: VecDeque<Event>,
    pub stats: Stats,
}

impl Connection {
    /// A connection to `server`; `guid` is this peer's RakNetGUID (the 2018 client keeps
    /// ONE for all its connections).
    pub fn new(server: SocketAddrV4, guid: u64, now: u64) -> Self {
        let mut c = Self {
            server,
            guid,
            state: State::Probing { probe: 0, attempts: 0 },
            started: now,
            last_send_offline: 0,
            last_heard: now,
            last_ping: now,
            mtu: MTU_PROBES[0],
            server_guid: 0,
            local_address: None,
            out: Vec::new(),
            pending: VecDeque::new(),
            next_sequence: 0,
            next_reliable: 0,
            next_ordering: [0; ORDERING_CHANNELS],
            next_sequencing: [0; ORDERING_CHANNELS],
            next_split_id: 0,
            sent: HashMap::new(),
            rto_ms: 300,
            srtt: None,
            acks_due: Vec::new(),
            received_base: 0,
            received_above: Default::default(),
            splits: HashMap::new(),
            channels: (0..ORDERING_CHANNELS).map(|_| Channel::default()).collect(),
            events: VecDeque::new(),
            stats: Stats::default(),
        };
        c.send_probe(now);
        c
    }

    pub fn server(&self) -> SocketAddrV4 {
        self.server
    }

    pub fn is_connected(&self) -> bool {
        self.state == State::Connected
    }

    pub fn is_closed(&self) -> bool {
        self.state == State::Closed
    }

    pub fn mtu(&self) -> usize {
        self.mtu
    }

    /// Datagrams to put on the wire, oldest first.
    pub fn drain_outgoing(&mut self, into: &mut Vec<Vec<u8>>) {
        into.append(&mut self.out);
    }

    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// Queue a message. It goes out on the next `update` (or `flush`).
    pub fn send(&mut self, payload: &[u8], bits: usize, reliability: Reliability, channel: u8) {
        if self.state == State::Closed {
            return;
        }
        let channel = (channel as usize % ORDERING_CHANNELS) as u8;
        let budget = self.mtu - wire::UDP_IPV4_OVERHEAD - 4 - wire::MAX_FRAME_HEADER;
        let ordering_index = if reliability.is_ordered() && !reliability.is_sequenced() {
            let i = self.next_ordering[channel as usize];
            self.next_ordering[channel as usize] = (i + 1) & 0xFF_FFFF;
            i
        } else {
            self.next_ordering[channel as usize]
        };
        let sequencing_index = if reliability.is_sequenced() {
            let i = self.next_sequencing[channel as usize];
            self.next_sequencing[channel as usize] = (i + 1) & 0xFF_FFFF;
            i
        } else {
            0
        };
        if payload.len() <= budget {
            let f = self.frame(payload.to_vec(), bits, reliability, channel, ordering_index, sequencing_index, None);
            self.pending.push_back(f);
            return;
        }
        // CONTRACT: a split message is reliable, whatever was asked (RakNet upgrades it).
        let reliability = match reliability {
            Reliability::Unreliable => Reliability::Reliable,
            Reliability::UnreliableSequenced => Reliability::ReliableSequenced,
            r => r,
        };
        let id = self.next_split_id;
        self.next_split_id = self.next_split_id.wrapping_add(1);
        let pieces: Vec<&[u8]> = payload.chunks(budget - 10).collect();
        let count = pieces.len() as u32;
        for (index, piece) in pieces.into_iter().enumerate() {
            let f = self.frame(
                piece.to_vec(),
                piece.len() * 8,
                reliability,
                channel,
                ordering_index,
                sequencing_index,
                Some(Split { count, id, index: index as u32 }),
            );
            self.pending.push_back(f);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn frame(
        &mut self,
        payload: Vec<u8>,
        bits: usize,
        reliability: Reliability,
        channel: u8,
        ordering_index: u32,
        sequencing_index: u32,
        split: Option<Split>,
    ) -> Frame {
        let reliable_index = if reliability.is_reliable() {
            let i = self.next_reliable;
            self.next_reliable = (i + 1) & 0xFF_FFFF;
            i
        } else {
            0
        };
        Frame {
            reliability,
            bit_length: bits,
            reliable_index,
            sequencing_index,
            ordering_index,
            ordering_channel: channel,
            split,
            payload,
        }
    }

    /// Close politely: DISCONNECTION_NOTIFICATION, then closed.
    pub fn disconnect(&mut self, now: u64) {
        if self.state == State::Connected {
            self.send(&[id::DISCONNECTION_NOTIFICATION], 8, Reliability::ReliableOrdered, 0);
            self.flush(now);
        }
        self.close(DisconnectReason::Local);
    }

    fn close(&mut self, reason: DisconnectReason) {
        if self.state != State::Closed {
            self.state = State::Closed;
            self.events.push_back(Event::Disconnected(reason));
        }
    }

    /// Timers: handshake retries, pings, acks, resends, and the pending frames.
    pub fn update(&mut self, now: u64) {
        match self.state {
            State::Closed => return,
            State::Probing { .. } | State::Requesting2 { .. } => {
                if now - self.started > HANDSHAKE_TIMEOUT_MS {
                    self.close(DisconnectReason::HandshakeTimeout);
                    return;
                }
                if now - self.last_send_offline >= PROBE_INTERVAL_MS {
                    self.retry_handshake(now);
                }
                return;
            }
            State::Connecting | State::Connected => {}
        }
        if now - self.last_heard > SILENCE_TIMEOUT_MS {
            self.close(DisconnectReason::TimedOut);
            return;
        }
        if self.state == State::Connected && now - self.last_ping >= PING_INTERVAL_MS {
            self.last_ping = now;
            let mut p = vec![id::CONNECTED_PING];
            p.extend_from_slice(&now.to_be_bytes());
            self.send(&p, p.len() * 8, Reliability::Unreliable, 0);
        }
        // Resends, oldest first.
        let mut due: Vec<u32> = self
            .sent
            .iter()
            .filter(|(_, r)| now - r.sent_at >= self.rto_ms * (1 << r.attempts.min(4)) as u64 / 2)
            .map(|(s, _)| *s)
            .collect();
        due.sort_unstable();
        for s in due {
            if let Some(r) = self.sent.get_mut(&s) {
                r.sent_at = now;
                r.attempts += 1;
                self.out.push(r.datagram.clone());
                self.stats.resends += 1;
                self.stats.datagrams_sent += 1;
            }
        }
        self.flush(now);
    }

    /// Acks and every pending frame, packed into datagrams.
    pub fn flush(&mut self, now: u64) {
        if !self.acks_due.is_empty() {
            let mut acks = std::mem::take(&mut self.acks_due);
            self.out.push(wire::ack_datagram(&mut acks, false));
        }
        let budget = self.mtu - wire::UDP_IPV4_OVERHEAD;
        while !self.pending.is_empty() {
            let sequence = self.next_sequence;
            self.next_sequence = (sequence + 1) & 0xFF_FFFF;
            let mut datagram = Vec::with_capacity(budget);
            wire::datagram_header(&mut datagram, flag::CLIENT_DATA, sequence);
            let mut reliable = false;
            while let Some(f) = self.pending.front() {
                if datagram.len() > 4 && datagram.len() + f.encoded_len() > budget {
                    break;
                }
                let f = self.pending.pop_front().unwrap();
                reliable |= f.reliability.is_reliable();
                f.write(&mut datagram);
            }
            if reliable {
                self.sent.insert(sequence, Retained { datagram: datagram.clone(), sent_at: now, attempts: 0 });
            }
            self.out.push(datagram);
            self.stats.datagrams_sent += 1;
        }
    }

    fn send_probe(&mut self, now: u64) {
        if let State::Probing { probe, .. } = self.state {
            self.mtu = MTU_PROBES[probe];
            self.out.push(wire::open_connection_request_1(self.mtu));
            self.last_send_offline = now;
        }
    }

    fn retry_handshake(&mut self, now: u64) {
        match self.state {
            State::Probing { probe, attempts } => {
                let (probe, attempts) = if attempts + 1 >= PROBE_ATTEMPTS && probe + 1 < MTU_PROBES.len() {
                    (probe + 1, 0)
                } else {
                    (probe, attempts + 1)
                };
                self.state = State::Probing { probe, attempts };
                self.send_probe(now);
            }
            State::Requesting2 { attempts } => {
                self.state = State::Requesting2 { attempts: attempts + 1 };
                self.out.push(wire::open_connection_request_2(self.server, self.mtu as u16, self.guid));
                self.last_send_offline = now;
            }
            _ => {}
        }
    }

    /// One datagram from the server.
    pub fn receive(&mut self, datagram: &[u8], now: u64) {
        if datagram.is_empty() || self.state == State::Closed {
            return;
        }
        self.last_heard = now;
        self.stats.datagrams_received += 1;
        let first = datagram[0];
        if first & flag::IS_VALID == 0 {
            self.receive_offline(datagram, now);
            return;
        }
        if first & flag::IS_ACK != 0 {
            if let Ok(ranges) = wire::parse_ack_ranges(datagram, 1, first & flag::HAS_B_AND_AS != 0) {
                for (lo, hi) in ranges {
                    for s in lo..=hi {
                        if let Some(r) = self.sent.remove(&s) {
                            if r.attempts == 0 {
                                self.sample_rtt(now - r.sent_at);
                            }
                        }
                    }
                }
            }
            return;
        }
        if first & flag::IS_NAK != 0 {
            if let Ok(ranges) = wire::parse_ack_ranges(datagram, 1, false) {
                for (lo, hi) in ranges {
                    for s in lo..=hi {
                        if let Some(r) = self.sent.get_mut(&s) {
                            r.sent_at = now;
                            r.attempts += 1;
                            self.out.push(r.datagram.clone());
                            self.stats.resends += 1;
                        }
                    }
                }
            }
            return;
        }
        if datagram.len() < 4 {
            return;
        }
        self.acks_due.push(wire::read_u24le(datagram, 1));
        let frames = match wire::parse_frames(datagram, 4) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("malformed datagram from {}: {e}", self.server);
                return;
            }
        };
        for frame in frames {
            self.receive_frame(frame, now);
        }
    }

    fn sample_rtt(&mut self, rtt: u64) {
        let rtt = rtt as f32;
        let srtt = match self.srtt {
            None => rtt,
            Some(s) => s * 0.875 + rtt * 0.125,
        };
        self.srtt = Some(srtt);
        self.stats.rtt_ms = srtt;
        self.rto_ms = ((srtt * 2.0) as u64 + 30).clamp(MIN_RTO_MS, MAX_RTO_MS);
    }

    fn receive_offline(&mut self, datagram: &[u8], now: u64) {
        match (self.state, datagram[0]) {
            (State::Probing { .. }, id::OPEN_CONNECTION_REPLY_1) => {
                let Ok(r) = wire::parse_open_connection_reply_1(datagram) else { return };
                self.server_guid = r.server_guid;
                // CONTRACT: never exceed what the server accepted.
                self.mtu = (r.mtu as usize).min(self.mtu);
                self.state = State::Requesting2 { attempts: 0 };
                self.out.push(wire::open_connection_request_2(self.server, self.mtu as u16, self.guid));
                self.last_send_offline = now;
            }
            (State::Requesting2 { .. }, id::OPEN_CONNECTION_REPLY_2) => {
                let Ok(r) = wire::parse_open_connection_reply_2(datagram) else { return };
                self.mtu = (r.mtu as usize).min(self.mtu);
                self.local_address = Some(r.client_address);
                self.state = State::Connecting;
                // CONNECTION_REQUEST: our GUID, our time, no security. Reliable (captured:
                // the 2018 client sends it with reliability 2).
                let mut p = vec![id::CONNECTION_REQUEST];
                p.extend_from_slice(&self.guid.to_be_bytes());
                p.extend_from_slice(&now.to_be_bytes());
                p.push(0);
                self.send(&p, p.len() * 8, Reliability::Reliable, 0);
                self.flush(now);
            }
            (_, id::INCOMPATIBLE_PROTOCOL_VERSION)
            | (_, id::NO_FREE_INCOMING_CONNECTIONS)
            | (_, id::CONNECTION_BANNED)
            | (_, id::ALREADY_CONNECTED) => self.close(DisconnectReason::Refused(datagram[0])),
            _ => {}
        }
    }

    fn receive_frame(&mut self, frame: Frame, now: u64) {
        if frame.reliability.is_reliable() && !self.first_delivery(frame.reliable_index) {
            self.stats.duplicates += 1;
            return;
        }
        let (payload, bits, frame) = match frame.split {
            None => {
                let bits = frame.bit_length;
                (frame.payload.clone(), bits, frame)
            }
            Some(split) => {
                let entry = self.splits.entry(split.id).or_insert_with(|| {
                    (split.count, vec![None; split.count as usize], 0, frame.clone())
                });
                if (split.index as usize) < entry.1.len() && entry.1[split.index as usize].is_none() {
                    entry.1[split.index as usize] = Some(frame.payload.clone());
                    entry.2 += 1;
                }
                if entry.2 < entry.0 as usize {
                    return;
                }
                let (_, pieces, _, first) = self.splits.remove(&split.id).unwrap();
                let whole: Vec<u8> = pieces.into_iter().flatten().flatten().collect();
                let bits = whole.len() * 8;
                (whole, bits, first)
            }
        };
        if frame.reliability.is_sequenced() {
            let ch = &mut self.channels[frame.ordering_channel as usize % ORDERING_CHANNELS];
            if let Some(h) = ch.highest_sequence {
                let ahead = frame.sequencing_index.wrapping_sub(h) & 0xFF_FFFF;
                if ahead == 0 || ahead >= RELIABLE_WINDOW {
                    return;
                }
            }
            ch.highest_sequence = Some(frame.sequencing_index);
            self.deliver(payload, bits, frame.reliability, now);
            return;
        }
        if frame.reliability.is_ordered() {
            let channel = frame.ordering_channel as usize % ORDERING_CHANNELS;
            let expected = self.channels[channel].expected;
            if frame.ordering_index != expected {
                let ahead = frame.ordering_index.wrapping_sub(expected) & 0xFF_FFFF;
                if ahead < RELIABLE_WINDOW {
                    self.channels[channel].held.insert(frame.ordering_index, (payload, bits, frame.reliability));
                }
                return;
            }
            self.deliver(payload, bits, frame.reliability, now);
            let mut next = (expected + 1) & 0xFF_FFFF;
            while let Some((p, b, r)) = self.channels[channel].held.remove(&next) {
                self.deliver(p, b, r, now);
                next = (next + 1) & 0xFF_FFFF;
            }
            self.channels[channel].expected = next;
            return;
        }
        self.deliver(payload, bits, frame.reliability, now);
    }

    fn first_delivery(&mut self, index: u32) -> bool {
        let ahead = index.wrapping_sub(self.received_base) & 0xFF_FFFF;
        if ahead >= RELIABLE_WINDOW || self.received_above.contains(&index) {
            return false;
        }
        if ahead == 0 {
            self.received_base = (self.received_base + 1) & 0xFF_FFFF;
            while self.received_above.remove(&self.received_base) {
                self.received_base = (self.received_base + 1) & 0xFF_FFFF;
            }
        } else {
            self.received_above.insert(index);
        }
        true
    }

    /// RakNet's own messages are handled here; everything else goes up.
    fn deliver(&mut self, data: Vec<u8>, bits: usize, reliability: Reliability, now: u64) {
        let Some(&first) = data.first() else { return };
        match first {
            id::CONNECTION_REQUEST_ACCEPTED if self.state == State::Connecting => {
                // Our address as seen, system index, 10 internal addresses, then the
                // two times. Answer NEW_INCOMING_CONNECTION, as the 2018 client does.
                let server_time = data
                    .len()
                    .checked_sub(16)
                    .map(|at| u64::from_be_bytes(data[at + 8..at + 16].try_into().unwrap()))
                    .unwrap_or(0);
                let mut p = vec![id::NEW_INCOMING_CONNECTION];
                wire::put_address(&mut p, self.server);
                if let Some(local) = self.local_address {
                    wire::put_address(&mut p, local);
                } else {
                    wire::put_unassigned(&mut p);
                }
                for _ in 1..wire::MAX_INTERNAL_IDS {
                    wire::put_unassigned(&mut p);
                }
                p.extend_from_slice(&server_time.to_be_bytes());
                p.extend_from_slice(&now.to_be_bytes());
                self.send(&p, p.len() * 8, Reliability::ReliableOrdered, 0);
                self.state = State::Connected;
                self.last_ping = now;
                self.flush(now);
                self.events.push_back(Event::Connected);
            }
            id::CONNECTED_PING if data.len() >= 9 => {
                let mut p = vec![id::CONNECTED_PONG];
                p.extend_from_slice(&data[1..9]);
                p.extend_from_slice(&now.to_be_bytes());
                self.send(&p, p.len() * 8, Reliability::Unreliable, 0);
            }
            id::CONNECTED_PONG if data.len() >= 9 => {
                let sent = u64::from_be_bytes(data[1..9].try_into().unwrap());
                if now >= sent {
                    self.sample_rtt(now - sent);
                }
            }
            id::DISCONNECTION_NOTIFICATION | id::CONNECTION_LOST => self.close(DisconnectReason::Closed),
            _ => self.events.push_back(Event::Message { data, bits, reliability }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::tests::hex;
    use std::net::Ipv4Addr;

    fn server() -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, 2190)
    }

    /// Drive the handshake with the server's real replies from session-walk4.
    #[test]
    fn handshake_against_the_captured_server() {
        let mut c = Connection::new(server(), 0x06600001a13624d9, 1000);
        let mut out = Vec::new();
        c.drain_outgoing(&mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 1264, "OCR1 probes MTU 1292 like the real client");

        c.receive(&hex("0600ffff00fefefefefdfdfdfd1234567800063fb2731c5c7900050c"), 1010);
        out.clear();
        c.drain_outgoing(&mut out);
        assert_eq!(out, vec![hex("0700ffff00fefefefefdfdfdfd123456780480fffffe088e050c06600001a13624d9")]);

        c.receive(&hex("0800ffff00fefefefefdfdfdfd1234567800063fb2731c5c790480fffffe9a16050c00"), 1020);
        out.clear();
        c.drain_outgoing(&mut out);
        // 84 000000 | 40 0090 000000 | 09 guid time 00
        assert_eq!(&out[0][..9], &hex("840000004000900000")[..]);
        assert_eq!(out[0][10], 0x09);
        assert_eq!(&out[0][11..19], &0x06600001a13624d9u64.to_be_bytes());

        // The server's ack, then its CONNECTION_REQUEST_ACCEPTED (frame 7).
        c.receive(&hex("c0000101000000"), 1030);
        c.receive(&hex("8000000060030000000000000000100480fffffe9a160000043f57fed1088e04ffffffff000004ffffffff000004ffffffff000004ffffffff000004ffffffff000004ffffffff000004ffffffff000004ffffffff000004ffffffff000000000000006ad7210000000000001c22"), 1040);
        assert_eq!(c.poll_event(), Some(Event::Connected));
        out.clear();
        c.drain_outgoing(&mut out);
        // An ACK for datagram 0, then NEW_INCOMING_CONNECTION, reliable ordered index 0.
        assert_eq!(out[0], hex("c0000101000000"));
        assert_eq!(out[1][4] >> 5, 3);
        assert_eq!(out[1][14], 0x13);
        assert_eq!(out[1].len(), 4 + 10 + 94);
    }

    #[test]
    fn ordered_messages_are_held_until_their_turn_and_splits_reassemble() {
        let mut c = Connection::new(server(), 1, 0);
        c.state = State::Connected;
        let mk = |seq: u32, rel: u32, ord: u32, payload: &[u8], split: Option<Split>| {
            let mut d = Vec::new();
            wire::datagram_header(&mut d, 0x80, seq);
            Frame {
                reliability: Reliability::ReliableOrdered,
                bit_length: payload.len() * 8,
                reliable_index: rel,
                sequencing_index: 0,
                ordering_index: ord,
                ordering_channel: 0,
                split,
                payload: payload.to_vec(),
            }
            .write(&mut d);
            d
        };
        c.receive(&mk(1, 1, 1, &[0x84, 2], None), 1);
        assert_eq!(c.poll_event(), None, "index 1 waits for index 0");
        c.receive(&mk(0, 0, 0, &[0x84, 1], None), 2);
        let got: Vec<_> = std::iter::from_fn(|| c.poll_event()).collect();
        assert_eq!(got.len(), 2);
        assert!(matches!(&got[0], Event::Message { data, .. } if data == &vec![0x84, 1]));
        // duplicate is dropped
        c.receive(&mk(2, 0, 0, &[0x84, 1], None), 3);
        assert_eq!(c.poll_event(), None);
        // a split in two pieces
        let s = |i| Some(Split { count: 2, id: 7, index: i });
        c.receive(&mk(4, 3, 2, &[0x85, 0xAA], s(1)), 4);
        c.receive(&mk(3, 2, 2, &[0x85, 0x55], s(0)), 5);
        assert!(matches!(c.poll_event(), Some(Event::Message { data, .. }) if data == vec![0x85, 0x55, 0x85, 0xAA]));
    }

    #[test]
    fn big_messages_split_and_everything_fits_the_mtu() {
        let mut c = Connection::new(server(), 1, 0);
        c.state = State::Connected;
        c.mtu = 576;
        c.drain_outgoing(&mut Vec::new()); // the MTU probe sent by new()
        let big = vec![0x8B; 3000];
        c.send(&big, big.len() * 8, Reliability::ReliableOrdered, 0);
        c.flush(0);
        let mut out = Vec::new();
        c.drain_outgoing(&mut out);
        assert!(out.len() >= 6);
        assert!(out.iter().all(|d| d.len() <= 576 - 28));
        let total: usize = out
            .iter()
            .flat_map(|d| wire::parse_frames(d, 4).unwrap())
            .map(|f| f.payload.len())
            .sum();
        assert_eq!(total, 3000);
    }
}
