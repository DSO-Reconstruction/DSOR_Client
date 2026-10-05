//! Movement, vicinity, discards and the per-actor stats update.
//!
//! SOURCE: experimental dsor/gameplay.py (with_motion, blank_entity_record,
//!   encode_actor_vitals), dsor/combat.py (_encode_actor_list, encode_discard_monster),
//!   dsor/newplayer.py (encode_discard_player).

use dsor_raknet::{BitReader, BitWriter};

use super::wire::{counted, Body, ReadExt, WriteExt, MOST};
use super::DecodeError;

/// MoveCommand (103), both directions: where an actor's motion starts and how long the
/// client dead-reckons it.
///
/// 120 bits: three i16 position fields (world units * 128, x / elevation / y), speed,
/// heading travelled, body facing, u32 start tick, u16 duration in ticks.
/// EVIDENCE: the 2018 MoveCommand reader (vtable 0x1000204 slot 5, 0x9CE976) per
///   dsor/gameplay.py; the reader stores the end tick as start + duration (0x9CEA32),
///   NetworkSmoothMotionProperty::UpdateSmoothingGoals (0x4E66AC) dead-reckons
///   start position + 25 * velocity * seconds, clamped to duration * 0.04 s.
/// The client sends the same 15 bytes in its own MoveCommand (dsor/gameplay.py
///   decode_client_movement: bytes 9..14 there are this tick and duration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Move {
    /// Position, world units * 128 (0x9CEA63 scales by 1/128).
    pub x: i16,
    pub elevation: i16,
    pub y: i16,
    /// 0 standing, 0x40 walking, higher under a movement buff (0x59 = 0x40 * 1.4).
    pub speed: u8,
    /// Heading travelled along, 256ths of a turn clockwise from +y.
    pub heading: u8,
    /// The body's facing, same units.
    pub facing: u8,
    /// Game tick (40 ms) the motion starts at.
    pub start_tick: u32,
    /// Ticks the client dead-reckons before stopping.
    pub duration: u16,
}

impl Move {
    /// World-space position (the wire fields divided by 128).
    pub fn position(&self) -> [f32; 3] {
        [self.x as f32 / 128.0, self.elevation as f32 / 128.0, self.y as f32 / 128.0]
    }
}

impl Body for Move {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            x: r.i16()?,
            elevation: r.i16()?,
            y: r.i16()?,
            speed: r.u8()?,
            heading: r.u8()?,
            facing: r.u8()?,
            start_tick: r.u32()?,
            duration: r.u16()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.i16(self.x);
        w.i16(self.elevation);
        w.i16(self.y);
        w.u8(self.speed);
        w.u8(self.heading);
        w.u8(self.facing);
        w.u32(self.start_tick);
        w.u16(self.duration);
    }
}

/// An empty body: the actor in the trailer is the whole command.
macro_rules! empty_body {
    ($($(#[$m:meta])* $name:ident;)*) => {$(
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        pub struct $name;
        impl Body for $name {
            fn decode(_: &mut BitReader<'_>) -> Result<Self, DecodeError> {
                Ok(Self)
            }
            fn encode(&self, _: &mut BitWriter) {}
        }
    )*};
}
pub(crate) use empty_body;

empty_body! {
    /// DiscardPlayerCommand (36): remove the remote player named by the actor.
    /// SOURCE: dsor/newplayer.py encode_discard_player.
    DiscardPlayer;
    /// DiscardMonsterCommand (49): delete a creature's entity outright (not a death).
    /// SOURCE: dsor/combat.py encode_discard_monster.
    DiscardMonster;
}

/// A u32 count then that many u32 actor ids.
fn actor_list(r: &mut BitReader<'_>) -> Result<Vec<u32>, DecodeError> {
    counted(r, MOST, "actor count", |r| r.u32())
}

fn write_actor_list(w: &mut BitWriter, actors: &[u32]) {
    w.count(actors.len());
    for &a in actors {
        w.u32(a);
    }
}

/// ActorsEnterVicinityCommand (125): these actors are near you; the client requests
/// (ActorRequest 34) any it does not know.
/// SOURCE: dsor/combat.py encode_actors_enter_vicinity.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActorsEnterVicinity {
    pub actors: Vec<u32>,
}

impl Body for ActorsEnterVicinity {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { actors: actor_list(r)? })
    }
    fn encode(&self, w: &mut BitWriter) {
        write_actor_list(w, &self.actors)
    }
}

/// ActorsLeftVicinityCommand (124): these actors are no longer near you (hidden, not
/// removed). SOURCE: dsor/combat.py encode_actors_left_vicinity.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActorsLeftVicinity {
    pub actors: Vec<u32>,
}

impl Body for ActorsLeftVicinity {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { actors: actor_list(r)? })
    }
    fn encode(&self, w: &mut BitWriter) {
        write_actor_list(w, &self.actors)
    }
}

/// ActorStatsUpdateCommand (132): the actor's current health and skill resource.
/// EVIDENCE: the 2018 reader 0x9A68CE reads two floats (sub_C4A652) into the fields
///   SetHealthPoints / SetSkillResource take (dsor/gameplay.py encode_actor_vitals).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ActorStatsUpdate {
    pub health: f32,
    pub resource: f32,
}

impl Body for ActorStatsUpdate {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { health: r.f32()?, resource: r.f32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.f32(self.health);
        w.f32(self.resource);
    }
}
