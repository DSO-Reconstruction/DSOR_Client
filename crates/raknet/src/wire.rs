//! RakNet 4.035's wire formats: addresses, the offline handshake, the datagram
//! envelope, acknowledgements and frames.
//!
//! EVIDENCE: every layout here is checked against the real 2018 client's own bytes in
//! the captured sessions (tests below quote them), and mirrors the experimental
//! server's raknet/ package, which that client accepts.

use std::net::{Ipv4Addr, SocketAddrV4};

/// The 16 bytes every offline message carries.
pub const OFFLINE_MAGIC: [u8; 16] = [
    0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78,
];
/// RAKNET_PROTOCOL_VERSION of 4.035, announced in OPEN_CONNECTION_REQUEST_1.
pub const PROTOCOL_VERSION: u8 = 5;
/// IPv4 + UDP header bytes; a datagram of size S probes an MTU of S + 28.
pub const UDP_IPV4_OVERHEAD: usize = 28;
/// RakNet's internal address slots in CONNECTION_REQUEST_ACCEPTED / NEW_INCOMING.
pub const MAX_INTERNAL_IDS: usize = 10;

pub mod id {
    pub const CONNECTED_PING: u8 = 0x00;
    pub const UNCONNECTED_PING: u8 = 0x01;
    pub const CONNECTED_PONG: u8 = 0x03;
    pub const OPEN_CONNECTION_REQUEST_1: u8 = 0x05;
    pub const OPEN_CONNECTION_REPLY_1: u8 = 0x06;
    pub const OPEN_CONNECTION_REQUEST_2: u8 = 0x07;
    pub const OPEN_CONNECTION_REPLY_2: u8 = 0x08;
    pub const CONNECTION_REQUEST: u8 = 0x09;
    pub const CONNECTION_REQUEST_ACCEPTED: u8 = 0x10;
    pub const CONNECTION_ATTEMPT_FAILED: u8 = 0x11;
    pub const ALREADY_CONNECTED: u8 = 0x12;
    pub const NEW_INCOMING_CONNECTION: u8 = 0x13;
    pub const NO_FREE_INCOMING_CONNECTIONS: u8 = 0x14;
    pub const DISCONNECTION_NOTIFICATION: u8 = 0x15;
    pub const CONNECTION_LOST: u8 = 0x16;
    pub const CONNECTION_BANNED: u8 = 0x17;
    pub const INCOMPATIBLE_PROTOCOL_VERSION: u8 = 0x19;
    pub const TIMESTAMP: u8 = 0x1B;
    pub const USER_PACKET_ENUM: u8 = 0x84;
}

pub mod flag {
    pub const IS_VALID: u8 = 0x80;
    pub const IS_ACK: u8 = 0x40;
    /// With IS_ACK: a bandwidth figure follows. Without: the datagram is a NAK.
    pub const IS_NAK: u8 = 0x20;
    pub const HAS_B_AND_AS: u8 = 0x20;
    pub const NEEDS_B_AND_AS: u8 = 0x04;
    /// What the 2018 client puts on its data datagrams (captured: 0x84).
    pub const CLIENT_DATA: u8 = IS_VALID | NEEDS_B_AND_AS;
}

/// RakNet's delivery guarantee, the top three bits of a frame's flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Reliability {
    Unreliable = 0,
    UnreliableSequenced = 1,
    Reliable = 2,
    ReliableOrdered = 3,
    ReliableSequenced = 4,
    UnreliableWithAckReceipt = 5,
    ReliableWithAckReceipt = 6,
    ReliableOrderedWithAckReceipt = 7,
}

impl Reliability {
    pub fn from_bits(v: u8) -> Self {
        match v & 7 {
            0 => Self::Unreliable,
            1 => Self::UnreliableSequenced,
            2 => Self::Reliable,
            3 => Self::ReliableOrdered,
            4 => Self::ReliableSequenced,
            5 => Self::UnreliableWithAckReceipt,
            6 => Self::ReliableWithAckReceipt,
            _ => Self::ReliableOrderedWithAckReceipt,
        }
    }
    pub fn is_reliable(self) -> bool {
        matches!(
            self,
            Self::Reliable
                | Self::ReliableOrdered
                | Self::ReliableSequenced
                | Self::ReliableWithAckReceipt
                | Self::ReliableOrderedWithAckReceipt
        )
    }
    pub fn is_sequenced(self) -> bool {
        matches!(self, Self::UnreliableSequenced | Self::ReliableSequenced)
    }
    /// Sequenced reliabilities carry an ordering index too.
    pub fn is_ordered(self) -> bool {
        matches!(
            self,
            Self::UnreliableSequenced
                | Self::ReliableOrdered
                | Self::ReliableSequenced
                | Self::ReliableOrderedWithAckReceipt
        )
    }
}

/// Malformed input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError(pub String);

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for WireError {}

fn err<T>(msg: impl Into<String>) -> Result<T, WireError> {
    Err(WireError(msg.into()))
}

fn need(buf: &[u8], at: usize, n: usize) -> Result<(), WireError> {
    if buf.len() < at + n {
        return err(format!("need {n} bytes at {at}, have {}", buf.len()));
    }
    Ok(())
}

pub fn read_u24le(buf: &[u8], at: usize) -> u32 {
    buf[at] as u32 | (buf[at + 1] as u32) << 8 | (buf[at + 2] as u32) << 16
}

pub fn put_u24le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&[v as u8, (v >> 8) as u8, (v >> 16) as u8]);
}

// ---------------------------------------------------------------- addresses

/// RakNet's SystemAddress: version 4, the IPv4 bytes COMPLEMENTED, port big-endian.
pub fn put_address(out: &mut Vec<u8>, addr: SocketAddrV4) {
    out.push(4);
    out.extend(addr.ip().octets().iter().map(|b| !b));
    out.extend_from_slice(&addr.port().to_be_bytes());
}

pub fn read_address(buf: &[u8], at: usize) -> Result<(SocketAddrV4, usize), WireError> {
    need(buf, at, 7)?;
    if buf[at] != 4 {
        return err(format!("address version {} (only IPv4)", buf[at]));
    }
    let ip = Ipv4Addr::new(!buf[at + 1], !buf[at + 2], !buf[at + 3], !buf[at + 4]);
    let port = u16::from_be_bytes([buf[at + 5], buf[at + 6]]);
    Ok((SocketAddrV4::new(ip, port), at + 7))
}

/// The padding RakNet writes in unused internal-address slots.
pub fn put_unassigned(out: &mut Vec<u8>) {
    out.extend_from_slice(&[4, 0xff, 0xff, 0xff, 0xff, 0, 0]);
}

// ---------------------------------------------------------------- offline

/// OPEN_CONNECTION_REQUEST_1, zero-padded so the whole datagram probes `mtu`.
pub fn open_connection_request_1(mtu: usize) -> Vec<u8> {
    let size = mtu.saturating_sub(UDP_IPV4_OVERHEAD).max(18);
    let mut out = Vec::with_capacity(size);
    out.push(id::OPEN_CONNECTION_REQUEST_1);
    out.extend_from_slice(&OFFLINE_MAGIC);
    out.push(PROTOCOL_VERSION);
    out.resize(size, 0);
    out
}

pub struct Reply1 {
    pub server_guid: u64,
    pub security: bool,
    pub mtu: u16,
}

pub fn parse_open_connection_reply_1(buf: &[u8]) -> Result<Reply1, WireError> {
    need(buf, 0, 1 + 16 + 8 + 1 + 2)?;
    if buf[0] != id::OPEN_CONNECTION_REPLY_1 || buf[1..17] != OFFLINE_MAGIC {
        return err("not an OPEN_CONNECTION_REPLY_1");
    }
    Ok(Reply1 {
        server_guid: u64::from_be_bytes(buf[17..25].try_into().unwrap()),
        security: buf[25] != 0,
        mtu: u16::from_be_bytes([buf[26], buf[27]]),
    })
}

/// OPEN_CONNECTION_REQUEST_2: the address dialled, the agreed MTU, our GUID.
pub fn open_connection_request_2(server: SocketAddrV4, mtu: u16, guid: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(34);
    out.push(id::OPEN_CONNECTION_REQUEST_2);
    out.extend_from_slice(&OFFLINE_MAGIC);
    put_address(&mut out, server);
    out.extend_from_slice(&mtu.to_be_bytes());
    out.extend_from_slice(&guid.to_be_bytes());
    out
}

pub struct Reply2 {
    pub server_guid: u64,
    pub client_address: SocketAddrV4,
    pub mtu: u16,
    pub security: bool,
}

pub fn parse_open_connection_reply_2(buf: &[u8]) -> Result<Reply2, WireError> {
    need(buf, 0, 1 + 16 + 8)?;
    if buf[0] != id::OPEN_CONNECTION_REPLY_2 || buf[1..17] != OFFLINE_MAGIC {
        return err("not an OPEN_CONNECTION_REPLY_2");
    }
    let server_guid = u64::from_be_bytes(buf[17..25].try_into().unwrap());
    let (client_address, at) = read_address(buf, 25)?;
    need(buf, at, 3)?;
    Ok(Reply2 {
        server_guid,
        client_address,
        mtu: u16::from_be_bytes([buf[at], buf[at + 1]]),
        security: buf[at + 2] != 0,
    })
}

// ---------------------------------------------------------------- datagrams

/// Flags byte plus 24-bit little-endian sequence number.
pub fn datagram_header(out: &mut Vec<u8>, flags: u8, sequence: u32) {
    out.push(flags);
    put_u24le(out, sequence & 0xFF_FFFF);
}

/// An ACK or NAK: flags, u16 big-endian record count, then per record a "single"
/// byte and one or two 24-bit numbers. Sorted, contiguous runs coalesced.
pub fn ack_datagram(sequences: &mut Vec<u32>, nak: bool) -> Vec<u8> {
    sequences.sort_unstable();
    sequences.dedup();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for &s in sequences.iter() {
        match ranges.last_mut() {
            Some((_, hi)) if *hi + 1 == s => *hi = s,
            _ => ranges.push((s, s)),
        }
    }
    let mut out = Vec::with_capacity(3 + ranges.len() * 7);
    out.push(flag::IS_VALID | if nak { flag::IS_NAK } else { flag::IS_ACK });
    out.extend_from_slice(&(ranges.len() as u16).to_be_bytes());
    for (lo, hi) in ranges {
        out.push((lo == hi) as u8);
        put_u24le(&mut out, lo);
        if lo != hi {
            put_u24le(&mut out, hi);
        }
    }
    out
}

/// The sequence numbers an ACK/NAK names. `at` is just past the flags byte.
pub fn parse_ack_ranges(buf: &[u8], mut at: usize, bandwidth: bool) -> Result<Vec<(u32, u32)>, WireError> {
    if bandwidth {
        at += 4;
    }
    need(buf, at, 2)?;
    let count = u16::from_be_bytes([buf[at], buf[at + 1]]) as usize;
    at += 2;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        need(buf, at, 4)?;
        let single = buf[at] != 0;
        let lo = read_u24le(buf, at + 1);
        at += 4;
        let hi = if single {
            lo
        } else {
            need(buf, at, 3)?;
            let hi = read_u24le(buf, at);
            at += 3;
            hi
        };
        out.push((lo, hi));
    }
    Ok(out)
}

// ---------------------------------------------------------------- frames

pub const SPLIT_FLAG: u8 = 0x10;
/// flags(1) + bits(2) + reliable(3) + sequencing(3) + ordering(3+1) + split(4+2+4).
pub const MAX_FRAME_HEADER: usize = 23;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub reliability: Reliability,
    pub bit_length: usize,
    pub reliable_index: u32,
    pub sequencing_index: u32,
    pub ordering_index: u32,
    pub ordering_channel: u8,
    pub split: Option<Split>,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Split {
    pub count: u32,
    pub id: u16,
    pub index: u32,
}

impl Frame {
    pub fn header_len(&self) -> usize {
        let r = self.reliability;
        3 + if r.is_reliable() { 3 } else { 0 }
            + if r.is_sequenced() { 3 } else { 0 }
            + if r.is_ordered() { 4 } else { 0 }
            + if self.split.is_some() { 10 } else { 0 }
    }

    pub fn encoded_len(&self) -> usize {
        self.header_len() + self.payload.len()
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        let r = self.reliability;
        out.push(((r as u8) << 5) | if self.split.is_some() { SPLIT_FLAG } else { 0 });
        out.extend_from_slice(&(self.bit_length as u16).to_be_bytes());
        if r.is_reliable() {
            put_u24le(out, self.reliable_index);
        }
        if r.is_sequenced() {
            put_u24le(out, self.sequencing_index);
        }
        if r.is_ordered() {
            put_u24le(out, self.ordering_index);
            out.push(self.ordering_channel);
        }
        if let Some(s) = self.split {
            out.extend_from_slice(&s.count.to_be_bytes());
            out.extend_from_slice(&s.id.to_be_bytes());
            out.extend_from_slice(&s.index.to_be_bytes());
        }
        out.extend_from_slice(&self.payload);
    }
}

/// Every frame of a data datagram's body (`at` just past the 4-byte header).
pub fn parse_frames(buf: &[u8], mut at: usize) -> Result<Vec<Frame>, WireError> {
    let mut frames = Vec::new();
    while at < buf.len() {
        need(buf, at, 3)?;
        let flags = buf[at];
        let reliability = Reliability::from_bits(flags >> 5);
        let bit_length = u16::from_be_bytes([buf[at + 1], buf[at + 2]]) as usize;
        at += 3;
        let mut frame = Frame {
            reliability,
            bit_length,
            reliable_index: 0,
            sequencing_index: 0,
            ordering_index: 0,
            ordering_channel: 0,
            split: None,
            payload: Vec::new(),
        };
        if reliability.is_reliable() {
            need(buf, at, 3)?;
            frame.reliable_index = read_u24le(buf, at);
            at += 3;
        }
        if reliability.is_sequenced() {
            need(buf, at, 3)?;
            frame.sequencing_index = read_u24le(buf, at);
            at += 3;
        }
        if reliability.is_ordered() {
            need(buf, at, 4)?;
            frame.ordering_index = read_u24le(buf, at);
            frame.ordering_channel = buf[at + 3];
            at += 4;
        }
        if flags & SPLIT_FLAG != 0 {
            need(buf, at, 10)?;
            frame.split = Some(Split {
                count: u32::from_be_bytes(buf[at..at + 4].try_into().unwrap()),
                id: u16::from_be_bytes([buf[at + 4], buf[at + 5]]),
                index: u32::from_be_bytes(buf[at + 6..at + 10].try_into().unwrap()),
            });
            at += 10;
        }
        let bytes = bit_length.div_ceil(8);
        need(buf, at, bytes)?;
        frame.payload = buf[at..at + bytes].to_vec();
        at += bytes;
        frames.push(frame);
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitstream::tests::hex;

    #[test]
    fn request_2_matches_the_real_client() {
        // session-walk4 frame 3: dialled 127.0.0.1:2190, MTU 1292, GUID 06600001a13624d9
        let got = open_connection_request_2(
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 2190),
            1292,
            0x06600001a13624d9,
        );
        assert_eq!(
            got,
            hex("0700ffff00fefefefefdfdfdfd123456780480fffffe088e050c06600001a13624d9")
        );
        assert_eq!(open_connection_request_1(1292).len(), 1264);
    }

    #[test]
    fn replies_parse() {
        let r1 = parse_open_connection_reply_1(&hex(
            "0600ffff00fefefefefdfdfdfd1234567800063fb2731c5c7900050c",
        ))
        .unwrap();
        assert_eq!((r1.server_guid, r1.mtu, r1.security), (0x00063fb2731c5c79, 1292, false));
        let r2 = parse_open_connection_reply_2(&hex(
            "0800ffff00fefefefefdfdfdfd1234567800063fb2731c5c790480fffffe9a16050c00",
        ))
        .unwrap();
        assert_eq!(r2.client_address, SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0x9a16));
        assert_eq!(r2.mtu, 1292);
    }

    #[test]
    fn acks_match_the_real_client() {
        let mut s = vec![3, 1, 2];
        assert_eq!(ack_datagram(&mut s, false), hex("c0000100010000030000"));
        let mut one = vec![0];
        assert_eq!(ack_datagram(&mut one, false), hex("c0000101000000"));
        assert_eq!(parse_ack_ranges(&hex("c0000100010000030000"), 1, false).unwrap(), vec![(1, 3)]);
    }

    #[test]
    fn frames_round_trip_the_real_new_incoming_connection() {
        // session-walk4 frame 9 (client): reliable ordered, index 1, order 0.
        let raw = hex("840100006002f001000000000000130480fffffe088e");
        let frames_head = &raw[4..];
        assert_eq!(frames_head[0] >> 5, 3);
        let f = Frame {
            reliability: Reliability::ReliableOrdered,
            bit_length: 752,
            reliable_index: 1,
            sequencing_index: 0,
            ordering_index: 0,
            ordering_channel: 0,
            split: None,
            payload: vec![0; 94],
        };
        let mut out = Vec::new();
        f.write(&mut out);
        assert_eq!(&out[..10], &raw[4..14]);
        let back = parse_frames(&out, 0).unwrap();
        assert_eq!(back, vec![f]);
    }
}
