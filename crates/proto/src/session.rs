//! From the launcher's credentials to standing on a map: login -> character service ->
//! login again with the chosen character -> map server, following the server's
//! hand-offs.
//!
//! EVIDENCE: the 2018 client's own sequence in session-walk4 (tools/session_messages.py):
//!   2190 login:      0x82 Login, we 0x8A short (0x83), 0x84/121 "127.0.0.1:2192", 0x88, we 0x15
//!   2192 characters: 0x82, we 0x8A short, 0x86 "a0000_char", 0x88, we 0x8D + 0x1B,
//!                    0x84/144 roster, we 0x8B/144 select, 0x84/144 op 5,
//!                    0x84/121 "" (= back to login), we 0x15
//!   2190 login:      we 0x8A long (character), 0x88, 0x84/121 "127.0.0.1:30000"
//!   map:             0x82 Map, we 0x8A long, 0x86 map, 0x88, we 0x8D, the world arrives
//!
//! No I/O: the caller owns the transport. When the session moves to another service it
//! emits `SessionEvent::Connect(addr)`; the caller then routes datagrams to and from
//! that address only.

use std::collections::VecDeque;
use std::net::SocketAddrV4;
#[cfg(not(target_arch = "wasm32"))]
use std::net::ToSocketAddrs;

use dsor_raknet::{Connection, DisconnectReason, Event, Reliability};

use crate::identity::{hello, Credentials};
use crate::messages::{self, classify, Envelope, RosterEntry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Login,
    Characters,
    LoginWithCharacter,
    Map,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionEvent {
    /// Send `farewell` on the current transport (if any), then open one to `target`;
    /// everything after goes there.
    Connect { target: SocketAddrV4, farewell: Vec<Vec<u8>> },
    Service(String),
    /// The character service's roster: answer with `select_character`.
    Roster(Vec<RosterEntry>),
    MapAssigned { name: String, rule_set: u32 },
    /// Game commands on the map (0x84 / 0x85), for `commands::decode_message`.
    Commands { data: Vec<u8>, bits: usize },
    Clock(u32),
    Disconnected(DisconnectReason),
}

pub struct Session {
    credentials: Credentials,
    guid: u64,
    login: SocketAddrV4,
    stage: Stage,
    conn: Connection,
    character: Option<u32>,
    first_login: bool,
    readies: u8,
    events: VecDeque<SessionEvent>,
    started: u64,
    handing_off: bool,
}

/// "host:port" to an IPv4 socket address; a bare host keeps `default_port`.
pub fn resolve(target: &str, default_port: u16) -> Option<SocketAddrV4> {
    let with_port = if target.contains(':') { target.to_string() } else { format!("{target}:{default_port}") };
    if let Ok(a) = with_port.parse::<SocketAddrV4>() {
        return Some(a);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        with_port.to_socket_addrs().ok()?.find_map(|a| match a {
            std::net::SocketAddr::V4(v4) => Some(v4),
            _ => None,
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

impl Session {
    pub fn new(login: SocketAddrV4, credentials: Credentials, guid: u64, now: u64) -> Self {
        let mut events = VecDeque::new();
        events.push_back(SessionEvent::Connect { target: login, farewell: Vec::new() });
        Self {
            credentials,
            guid,
            login,
            stage: Stage::Login,
            conn: Connection::new(login, guid, now),
            character: None,
            first_login: true,
            readies: 0,
            events,
            started: now,
            handing_off: false,
        }
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn character(&self) -> Option<u32> {
        self.character
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn poll_event(&mut self) -> Option<SessionEvent> {
        self.events.pop_front()
    }

    /// Datagrams for the CURRENT target. Poll events first: a `Connect` changes it.
    pub fn drain_outgoing(&mut self, into: &mut Vec<Vec<u8>>) {
        self.conn.drain_outgoing(into);
    }

    /// The game clock (25 ticks per second) the client keeps since it started.
    pub fn local_tick(&self, now: u64) -> u32 {
        ((now - self.started) / 40) as u32
    }

    /// Pick the character to play (from the roster).
    pub fn select_character(&mut self, character: u32, now: u64) {
        self.character = Some(character);
        let (bytes, bits) = messages::select_character(character);
        self.conn.send(&bytes, bits, Reliability::ReliableOrdered, 0);
        self.conn.flush(now);
    }

    /// A game command (0x8B ...) to the current map server.
    pub fn send_command(&mut self, payload: &[u8], bits: usize) {
        self.conn.send(payload, bits, Reliability::ReliableOrdered, 0);
    }

    /// A game command with its own reliability: the 2018 client sends MoveCommand
    /// unreliable sequenced (captured: reliability 1, one per tick).
    pub fn send_command_with(&mut self, payload: &[u8], bits: usize, reliability: Reliability) {
        self.conn.send(payload, bits, reliability, 0);
    }

    /// Say goodbye to the current server (DISCONNECTION_NOTIFICATION).
    pub fn disconnect(&mut self, now: u64) {
        self.handing_off = true;
        self.conn.disconnect(now);
    }

    pub fn update(&mut self, now: u64) {
        self.conn.update(now);
        self.pump(now);
    }

    pub fn receive(&mut self, datagram: &[u8], now: u64) {
        self.conn.receive(datagram, now);
        self.pump(now);
    }

    fn pump(&mut self, now: u64) {
        while let Some(event) = self.conn.poll_event() {
            match event {
                Event::Connected => {
                    let long = match self.stage {
                        Stage::Login | Stage::Characters => None,
                        Stage::LoginWithCharacter | Stage::Map => self.character,
                    };
                    let p = hello(&self.credentials, long, self.first_login);
                    self.first_login = false;
                    self.conn.send(&p, p.len() * 8, Reliability::ReliableOrdered, 0);
                    self.conn.flush(now);
                }
                Event::Message { data, bits, .. } => self.on_message(&data, bits, now),
                Event::Disconnected(reason) => {
                    if !self.handing_off {
                        self.events.push_back(SessionEvent::Disconnected(reason));
                    }
                }
            }
        }
    }

    fn ready(&mut self, now: u64) {
        self.conn.send(&[messages::READY], 8, Reliability::ReliableOrdered, 0);
        if self.readies == 0 {
            let sync = messages::clock_sync(now, self.local_tick(now));
            self.conn.send(&sync, sync.len() * 8, Reliability::ReliableOrdered, 0);
        }
        self.readies = self.readies.saturating_add(1);
        self.conn.flush(now);
    }

    fn on_message(&mut self, data: &[u8], bits: usize, now: u64) {
        match classify(data, bits) {
            Envelope::Identity(name) => {
                // CONTRACT: the service says what it is; the stage follows it, not the
                //   order of hand-offs. FAILURE: a login server that remembers the
                //   account sends it straight to the map (experimental
                //   Sessions.character_of), and the world was read as a roster.
                match name.as_str() {
                    "DrasaOnlineMapServer" => self.stage = Stage::Map,
                    "DrasaCharacterService" => self.stage = Stage::Characters,
                    _ => {}
                }
                self.events.push_back(SessionEvent::Service(name))
            }
            Envelope::MapAssignment { name, rule_set } => {
                self.events.push_back(SessionEvent::MapAssigned { name, rule_set })
            }
            Envelope::Proceed => {
                // The login server says "proceed" without wanting a ready; the services
                // that hold a world (characters, map) do.
                if matches!(self.stage, Stage::Characters | Stage::Map) {
                    self.ready(now);
                }
            }
            Envelope::Clock { tick } => self.events.push_back(SessionEvent::Clock(tick)),
            Envelope::Handoff { target } => self.hand_off(&target, now),
            Envelope::Commands { data, bits } => {
                if self.stage == Stage::Characters {
                    if let Some((operation, roster)) = messages::read_roster(&data, bits) {
                        if operation == 1 {
                            self.events.push_back(SessionEvent::Roster(roster));
                        }
                    }
                    return;
                }
                // The real client says ready again as InstanceConfig (29) and its own
                // NewPlayer (35) arrive (session-walk4 frames 105-109).
                if self.stage == Stage::Map && self.readies < 3 && data.len() >= 3 {
                    let id = u16::from_le_bytes([data[1], data[2]]);
                    if id == 29 || id == 35 {
                        self.ready(now);
                    }
                }
                self.events.push_back(SessionEvent::Commands { data, bits });
            }
            Envelope::Other { .. } => {}
        }
    }

    fn hand_off(&mut self, target: &str, now: u64) {
        let next = if target.is_empty() {
            Some(self.login)
        } else {
            resolve(target, self.login.port())
        };
        let Some(next) = next else {
            log::warn!("hand-off to {target:?}, which does not resolve");
            return;
        };
        self.stage = match (self.stage, self.character.is_some()) {
            (Stage::Login, false) => Stage::Characters,
            (Stage::Characters, _) => Stage::LoginWithCharacter,
            (Stage::Login, true) | (Stage::LoginWithCharacter, _) => Stage::Map,
            // A map-to-map switch (portals, travel) goes straight to the next map.
            (Stage::Map, _) => Stage::Map,
        };
        self.handing_off = true;
        self.conn.disconnect(now);
        while self.conn.poll_event().is_some() {}
        // The goodbye still has to reach the OLD server: it travels with the event.
        let mut farewell = Vec::new();
        self.conn.drain_outgoing(&mut farewell);
        self.conn = Connection::new(next, self.guid, now);
        self.readies = 0;
        self.handing_off = false;
        self.events.push_back(SessionEvent::Connect { target: next, farewell });
    }
}
