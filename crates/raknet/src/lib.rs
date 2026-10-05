//! RakNet 4.035, client side, without sockets.
//!
//! The 2018 Drakensang Online client speaks RakNet 4.035 (protocol 5). This crate
//! reproduces its side of the conversation byte for byte -- the handshake, the
//! datagram and frame layers, reliability, ordering and splitting -- and leaves I/O to
//! the caller, so the same connection runs over UDP natively and over the WebSocket
//! relay in the browser.

pub mod bitstream;
pub mod client;
pub mod wire;

pub use bitstream::{BitReader, BitWriter, Overrun};
pub use client::{Connection, DisconnectReason, Event, Stats};
pub use wire::Reliability;
