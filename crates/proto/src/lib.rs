//! The 2018 game protocol on top of RakNet.
//!
//! - `identity`: the client's 0x8A hello, account and session.
//! - `messages`: the envelopes (0x82, 0x84, 0x85, 0x86, 0x88, 0x8B, 0x8D, 0x1B).
//! - `commands`: every game command, decoded and encoded field by field.
//! - `session`: login -> character service -> map, across the server's hand-offs.

pub mod commands;
pub mod identity;
pub mod messages;
pub mod session;

pub use dsor_raknet::{BitReader, BitWriter};
