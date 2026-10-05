//! The envelopes around game commands, as the 2018 client sees them.
//!
//! EVIDENCE: experimental dsor/messages.py and the captured sessions.
//!   0x82 server -> client  service identity: one string ("DrasaOnlineLoginServer",
//!                          "DrasaCharacterService", "DrasaOnlineMapServer")
//!   0x86 server -> client  map assignment: the map name TWICE, u32 rule set, u8
//!   0x88 server -> client  "go ahead": the client answers 0x8D
//!   0x8D client -> server  ready (sent after 0x88, and again as the world arrives)
//!   0x84 server -> client  one game command; 0x84/121 is the hand-off to host:port
//!   0x85 server -> client  chained game commands
//!   0x8B client -> server  one game command
//!   0x1B both              ID_TIMESTAMP-wrapped clock sync: time u64, 0x83, u32 clock

use dsor_raknet::{BitReader, BitWriter};

pub const SERVICE_IDENTITY: u8 = 0x82;
pub const COMMAND: u8 = 0x84;
pub const COMMANDS: u8 = 0x85;
pub const MAP_ASSIGNMENT: u8 = 0x86;
pub const PROCEED: u8 = 0x88;
pub const CLIENT_HELLO: u8 = 0x8A;
pub const CLIENT_COMMAND: u8 = 0x8B;
pub const READY: u8 = 0x8D;
pub const TIMESTAMP: u8 = 0x1B;
pub const CLOCK: u8 = 0x83;

/// 0x84/121: the server sends the client elsewhere.
pub const HANDOFF: u16 = 121;
/// CharacterSelectionCommand, both directions on the character service.
pub const CHARACTER_SELECTION: u16 = 144;

#[derive(Clone, Debug, PartialEq)]
pub enum Envelope {
    Identity(String),
    MapAssignment { name: String, rule_set: u32 },
    Proceed,
    /// 0x84/121; an empty target means "back to the login server".
    Handoff { target: String },
    /// A clock reading: the server's game tick (25 per second).
    Clock { tick: u32 },
    /// 0x84/0x85 game commands, left for `commands`.
    Commands { data: Vec<u8>, bits: usize },
    Other { id: u8, data: Vec<u8> },
}

pub fn classify(data: &[u8], bits: usize) -> Envelope {
    let Some(&id) = data.first() else {
        return Envelope::Other { id: 0, data: Vec::new() };
    };
    let mut r = BitReader::with_bit_len(data, bits);
    let _ = r.read_u8();
    match id {
        SERVICE_IDENTITY => r
            .read_string()
            .map(Envelope::Identity)
            .unwrap_or(Envelope::Other { id, data: data.to_vec() }),
        MAP_ASSIGNMENT => {
            let parsed = (|| {
                let name = r.read_string().ok()?;
                let _again = r.read_string().ok()?;
                let rule_set = r.read_u32().unwrap_or(0);
                Some(Envelope::MapAssignment { name, rule_set })
            })();
            parsed.unwrap_or(Envelope::Other { id, data: data.to_vec() })
        }
        PROCEED => Envelope::Proceed,
        TIMESTAMP if data.len() >= 14 && data[9] == CLOCK => Envelope::Clock {
            tick: u32::from_le_bytes(data[10..14].try_into().unwrap()),
        },
        COMMAND if data.len() >= 3 && u16::from_le_bytes([data[1], data[2]]) == HANDOFF => {
            let _ = r.read_u16();
            Envelope::Handoff { target: r.read_string().unwrap_or_default() }
        }
        COMMAND | COMMANDS => Envelope::Commands { data: data.to_vec(), bits },
        _ => Envelope::Other { id, data: data.to_vec() },
    }
}

/// The 0x1B clock sync the client sends: RakNet time, then 0x83 and its game tick.
pub fn clock_sync(now_ms: u64, tick: u32) -> Vec<u8> {
    let mut out = vec![TIMESTAMP];
    out.extend_from_slice(&now_ms.to_be_bytes());
    out.push(CLOCK);
    out.extend_from_slice(&tick.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

/// CharacterSelectionCommand from the client: operation 3 picks `character`.
/// EVIDENCE: session-walk4 frame 52, 8b9000 03 00 01000000 then zeros (32 bytes);
/// experimental dsor/selection2018.decode_selection_request reads op, reason,
/// character, capacity, free, a bit, u32, string, two zero counts.
pub fn select_character(character: u32) -> (Vec<u8>, usize) {
    let mut w = BitWriter::new();
    w.write_u8(CLIENT_COMMAND);
    w.write_u16(CHARACTER_SELECTION);
    w.write_u8(3);
    w.write_u8(0);
    w.write_u32(character);
    w.write_u32(0);
    w.write_u32(0);
    w.write_bool(false);
    w.write_u32(0);
    w.write_string("");
    w.write_u32(0);
    w.write_u32(0);
    w.finish()
}

/// One roster entry of the character service's list (0x84/144, operation 1).
#[derive(Clone, Debug, PartialEq)]
pub struct RosterEntry {
    pub name: String,
    pub map_name: String,
    pub player_class: u32,
    pub gender: u32,
    pub character: u32,
    pub level: u32,
}

/// The roster in a 0x84/144. EVIDENCE: experimental dsor/selection2018.encode_selection.
pub fn read_roster(data: &[u8], bits: usize) -> Option<(u8, Vec<RosterEntry>)> {
    let mut r = BitReader::with_bit_len(data, bits);
    if r.read_u8().ok()? != COMMAND || r.read_u16().ok()? != CHARACTER_SELECTION {
        return None;
    }
    let operation = r.read_u8().ok()?;
    let _reason = r.read_u8().ok()?;
    let _character = r.read_u32().ok()?;
    let _capacity = r.read_u32().ok()?;
    let _free = r.read_u32().ok()?;
    let _ = r.read_bool().ok()?;
    let _ = r.read_u32().ok()?;
    let _ = r.read_string().ok()?;
    let count = r.read_u32().ok()?;
    let mut out = Vec::new();
    for _ in 0..count.min(64) {
        let name = r.read_string().ok()?;
        let map_name = r.read_string().ok()?;
        let player_class = r.read_u32().ok()?;
        let gender = r.read_u32().ok()?;
        for _ in 0..4 {
            r.read_u8().ok()?;
        }
        let character = r.read_u32().ok()?;
        for _ in 0..3 {
            r.read_u32().ok()?;
        }
        let _experience = r.read_u32().ok()?;
        let level = r.read_u32().ok()?;
        for _ in 0..16 {
            r.read_u32().ok()?;
        }
        r.read_bool().ok()?;
        r.read_bool().ok()?;
        out.push(RosterEntry { name, map_name, player_class, gender, character, level });
    }
    Some((operation, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn envelopes_from_the_capture() {
        let id = unhex("82160044726173614f6e6c696e654c6f67696e536572766572");
        assert_eq!(classify(&id, id.len() * 8), Envelope::Identity("DrasaOnlineLoginServer".into()));
        let map = unhex("860f0061303230305f6b696e6773636974790f0061303230305f6b696e677363697479030000000000");
        assert_eq!(
            classify(&map, map.len() * 8),
            Envelope::MapAssignment { name: "a0200_kingscity".into(), rule_set: 3 }
        );
        let h = unhex("8479000e003132372e302e302e313a3231393280");
        assert_eq!(classify(&h, h.len() * 8), Envelope::Handoff { target: "127.0.0.1:2192".into() });
        let back = unhex("847900000000");
        assert_eq!(classify(&back, back.len() * 8), Envelope::Handoff { target: String::new() });
    }

    #[test]
    fn selection_is_the_real_clients() {
        let (bytes, _bits) = select_character(1);
        assert_eq!(bytes, unhex("8b90000300010000000000000000000000000000000000000000000000000000"));
    }
}
