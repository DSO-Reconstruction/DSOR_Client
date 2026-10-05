//! Groups, notifications, factions, the PvP flag, and the social commands the client
//! sends.
//!
//! SOURCE: experimental dsor/groups.py (encode_group_status, encode_member,
//!   encode_invitation_response, encode_invite_notification, encode_kick_notice and the
//!   client decoders), dsor/pvp.py (encode_pvp_flag, encode_faction_info,
//!   decode_pvp_flag), server.py Service._handle_group; rust-18 src/dsor/groups.rs and
//!   pvp.rs carry the same layouts.
//! CONTRACT: a command class has ONE reader and ONE writer (vtable slots 5 and 4), so
//!   an id both sides send shares one struct unless evidence shows otherwise.
//! UNKNOWN: client commands 175, 185 and 191 have no server decoder and no traced
//!   writer here; their bodies are kept verbatim (`unknown_body`).

use dsor_raknet::{BitReader, BitWriter};

use super::wire::{counted, Body, Raw, ReadExt, WriteExt, MOST};
use super::DecodeError;

/// A u16 length then raw bytes (the notification guid is not text).
fn read_blob(r: &mut BitReader<'_>) -> Result<Vec<u8>, DecodeError> {
    let n = r.u16()? as usize;
    Ok(r.read_bytes(n)?)
}

fn write_blob(w: &mut BitWriter, data: &[u8]) {
    w.u16(data.len() as u16);
    w.write_bytes(data);
}

/// Everything left in a client message, verbatim (0x8B bodies run to the end).
fn rest(r: &mut BitReader<'_>) -> Result<Raw, DecodeError> {
    let n = r.remaining();
    r.raw(n)
}

/// FactionInfoCommand (131): replace an actor's whole GameFaction.
/// EVIDENCE: reader 0x9A711D -> sub_A4C4E8 (u32, u32, s8 type, bool, bool); handler
///   0x515C24 copies it onto the actor named by `owner` (0x515CAA), not the trailer.
///   SOURCE: dsor/pvp.py encode_faction_info. 74 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FactionInfo {
    /// 0 while unflagged; a flagged player carries its own actor id.
    pub faction_id: u32,
    /// The actor this faction belongs to.
    pub owner: u32,
    pub kind: i8,
    /// Faction +0xC: both sides set and FactionIds differ -> hostile (sub_995A1C).
    pub pvp: bool,
    /// Faction +0xD, meaning not established.
    pub unknown_bool_0: bool,
}

impl Body for FactionInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { faction_id: r.u32()?, owner: r.u32()?, kind: r.i8()?, pvp: r.bit()?, unknown_bool_0: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.faction_id);
        w.u32(self.owner);
        w.i8(self.kind);
        w.bit(self.pvp);
        w.bit(self.unknown_bool_0);
    }
}

/// PVP::PVPFlagCommand (204), both directions: a bool then the world tick.
/// EVIDENCE: reader 0x9DD28C, writer 0x9DDBA6; handler 0x519244 -> SetPvPFlag 0x51DFC9
///   (tick stored at player +0x644 when on). The client's HandleSetPVPFlag 0x4E439F
///   sends NOT its current flag, then the tick. SOURCE: dsor/pvp.py encode_pvp_flag,
///   decode_pvp_flag. 33 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PvpFlag {
    pub flag: bool,
    pub tick: u32,
}

impl Body for PvpFlag {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { flag: r.bit()?, tick: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.bit(self.flag);
        w.u32(self.tick);
    }
}

/// One NotificationCommand entry.
/// EVIDENCE: entry reader 0x8FD574: u8 type, guid (u16 length + bytes), u32, u32
///   (echoed as the answer's b), u32 (echoed as its a), str JSON, str timestamp.
///   Type 1 is a group invite (0xA1EB67 "groupinvite"); its JSON keys are read by the
///   dialog 0x53E46E. SOURCE: dsor/groups.py encode_invite_notification.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotificationEntry {
    pub note_type: u8,
    /// 16 random bytes as the server sends it.
    pub guid: Vec<u8>,
    pub unknown_u32_0: u32,
    /// For a group invite: the group the invite names (the answer's b).
    pub group_id: u32,
    /// For a group invite: the inviter's player id (the answer's a).
    pub inviter_id: u32,
    /// JSON parameters (playerName, playerLevel, playerClass, honorPoints, groupName,
    /// playerGender, titleType, achievementTitle for an invite).
    pub params: String,
    /// "YYYY-MM-DD HH:MM:SS".
    pub timestamp: String,
}

impl NotificationEntry {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            note_type: r.u8()?,
            guid: read_blob(r)?,
            unknown_u32_0: r.u32()?,
            group_id: r.u32()?,
            inviter_id: r.u32()?,
            params: r.string()?,
            timestamp: r.string()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.note_type);
        write_blob(w, &self.guid);
        w.u32(self.unknown_u32_0);
        w.u32(self.group_id);
        w.u32(self.inviter_id);
        w.string(&self.params);
        w.string(&self.timestamp);
    }
}

/// NotificationCommand (145), server -> client: the invitee's popup.
/// EVIDENCE: reader 0xA1FB21: u8 type (1 carries notifications), then entries.
///   SOURCE: dsor/groups.py encode_invite_notification, which writes type 1, a u16 1,
///   then one entry.
/// UNKNOWN: the u16 is read here as the entry count (the server only ever writes 1
///   with one entry); the layout for types other than 1 is not established.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Notification {
    pub kind: u8,
    pub entries: Vec<NotificationEntry>,
}

impl Body for Notification {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let kind = r.u8()?;
        let n = r.u16()? as usize;
        let mut entries = Vec::with_capacity(n.min(64));
        for _ in 0..n {
            entries.push(NotificationEntry::decode(r)?);
        }
        Ok(Self { kind, entries })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.kind);
        w.u16(self.entries.len() as u16);
        for e in &self.entries {
            e.encode(w);
        }
    }
}

/// One PlayerSocialInfo row of a GroupStatus.
/// EVIDENCE: reader 0xA617CB: u32, u32, u32 playerId, u32 actorId, str name, str map
///   atom, u8 class, u16 level, u16, u32, u16, bool (row not greyed), bool online,
///   bool in group, bool, str timestamp, u8 enum 0..13, u32 colour index. GroupWidget
///   0x6931FA draws HP/mana from the entity named by `actor` (0 hides the bars).
///   SOURCE: dsor/groups.py encode_member.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupMember {
    pub unknown_u32_0: u32,
    pub unknown_u32_1: u32,
    pub player_id: u32,
    /// The member's actor on the RECIPIENT's map, 0 when elsewhere.
    pub actor: u32,
    pub name: String,
    pub map_name: String,
    pub character_class: u8,
    pub level: u16,
    pub unknown_u16_2: u16,
    pub unknown_u32_3: u32,
    pub unknown_u16_4: u16,
    /// Row not greyed.
    pub active: bool,
    pub online: bool,
    pub in_group: bool,
    pub unknown_bool_5: bool,
    pub timestamp: String,
    /// The reader refuses a value above 13.
    pub unknown_enum_6: u8,
    /// Colour index (joining order).
    pub slot: u32,
}

impl GroupMember {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let m = Self {
            unknown_u32_0: r.u32()?,
            unknown_u32_1: r.u32()?,
            player_id: r.u32()?,
            actor: r.u32()?,
            name: r.string()?,
            map_name: r.string()?,
            character_class: r.u8()?,
            level: r.u16()?,
            unknown_u16_2: r.u16()?,
            unknown_u32_3: r.u32()?,
            unknown_u16_4: r.u16()?,
            active: r.bit()?,
            online: r.bit()?,
            in_group: r.bit()?,
            unknown_bool_5: r.bit()?,
            timestamp: r.string()?,
            unknown_enum_6: r.u8()?,
            slot: r.u32()?,
        };
        if m.unknown_enum_6 > 13 {
            return Err(DecodeError::Invalid { what: "group member enum", value: m.unknown_enum_6 as i64 });
        }
        Ok(m)
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unknown_u32_0);
        w.u32(self.unknown_u32_1);
        w.u32(self.player_id);
        w.u32(self.actor);
        w.string(&self.name);
        w.string(&self.map_name);
        w.u8(self.character_class);
        w.u16(self.level);
        w.u16(self.unknown_u16_2);
        w.u32(self.unknown_u32_3);
        w.u16(self.unknown_u16_4);
        w.bit(self.active);
        w.bit(self.online);
        w.bit(self.in_group);
        w.bit(self.unknown_bool_5);
        w.string(&self.timestamp);
        w.u8(self.unknown_enum_6);
        w.u32(self.slot);
    }
}

/// GroupStatusCommand (160), NOT actor-scoped: the whole group as one recipient sees
/// it; group 0 with no members is "no group".
/// EVIDENCE: reader 0xA224EE (s8, u32 group, u32 leader, u32, bool, u32 count +
///   entries 0xA1FFBA, str atom, bool, str atom); handler 0x57153B; slot 9 reads no
///   actor. SOURCE: dsor/groups.py encode_group_status (type 0, the full update).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupStatus {
    pub kind: i8,
    pub group_id: u32,
    pub leader_id: u32,
    pub unknown_u32_0: u32,
    pub recipient_leads: bool,
    pub members: Vec<GroupMember>,
    pub unknown_string_1: String,
    pub unknown_bool_2: bool,
    pub unknown_string_3: String,
}

impl Body for GroupStatus {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            kind: r.i8()?,
            group_id: r.u32()?,
            leader_id: r.u32()?,
            unknown_u32_0: r.u32()?,
            recipient_leads: r.bit()?,
            members: counted(r, MOST, "group member count", GroupMember::decode)?,
            unknown_string_1: r.string()?,
            unknown_bool_2: r.bit()?,
            unknown_string_3: r.string()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.i8(self.kind);
        w.u32(self.group_id);
        w.u32(self.leader_id);
        w.u32(self.unknown_u32_0);
        w.bit(self.recipient_leads);
        w.count(self.members.len());
        for m in &self.members {
            m.encode(w);
        }
        w.string(&self.unknown_string_1);
        w.bit(self.unknown_bool_2);
        w.string(&self.unknown_string_3);
    }
}

/// GroupInvitationResponseCommand (165), both directions.
/// Server -> inviter: code 0 joinedYourGroup, 1 didNotJoinYourGroup, 2
///   isAlreadyInAGroup, 3 isNotLoggedIn, 4 unknownPlayer, 5 leaderRightsRequired
///   (handler 0x5709BB), with the other player's name.
/// Client (invitee): AcceptGroupInvite 0x56BB3D sends code 0, a = the notification's
///   inviter id, b = its group id; RefuseGroupInvite 0x574E80 code 1 (2 when grouped).
/// EVIDENCE: reader 0xA22017 (s8 code, u32, u32, str name). SOURCE: dsor/groups.py
///   encode_invitation_response, decode_invitation_response.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupInvitationResponse {
    pub code: i8,
    /// The inviter's player id when the client answers; 0 from the server.
    pub a: u32,
    /// The group id when the client answers; 0 from the server.
    pub b: u32,
    pub name: String,
}

impl Body for GroupInvitationResponse {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { code: r.i8()?, a: r.u32()?, b: r.u32()?, name: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.i8(self.code);
        w.u32(self.a);
        w.u32(self.b);
        w.string(&self.name);
    }
}

/// GroupKickCommand (168), both directions: the kicked member's name and two u32.
/// Leader -> server: decode_kick reads the name (0x56EEAB). Server -> kicked player:
///   handler 0x5711E7 shows youWereKicked and ignores the fields.
/// SOURCE: dsor/groups.py encode_kick_notice (str, u32 0, u32 0), decode_kick.
/// UNKNOWN: whether the client's own writer also emits the two u32 (no capture).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupKick {
    pub name: String,
    pub unknown_u32_0: u32,
    pub unknown_u32_1: u32,
}

impl Body for GroupKick {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { name: r.string()?, unknown_u32_0: r.u32()?, unknown_u32_1: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.name);
        w.u32(self.unknown_u32_0);
        w.u32(self.unknown_u32_1);
    }
}

/// GroupListCommand (159), client -> server: type 0 asks for the GroupStatus, type 2
/// promotes the player id in the THIRD u32.
/// EVIDENCE: writer 0xA234A1; senders 0x57566E (type 0), 0x56D09B (type 2).
///   SOURCE: dsor/groups.py decode_group_list. 104 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GroupList {
    pub kind: u8,
    pub unknown_u32_0: u32,
    pub unknown_u32_1: u32,
    /// The new leader's player id for type 2.
    pub target: u32,
}

impl Body for GroupList {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { kind: r.u8()?, unknown_u32_0: r.u32()?, unknown_u32_1: r.u32()?, target: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.kind);
        w.u32(self.unknown_u32_0);
        w.u32(self.unknown_u32_1);
        w.u32(self.target);
    }
}

/// GroupInviteCommand (163), client -> server: own name, invitee name, then a u32.
/// EVIDENCE: writer 0xA232F5 (dsor/groups.py decode_invite reads the two strings);
///   the captured payloads (session-merged*) carry 32 more bits, always 0:
///   `8b a300 1100 "DSOReconstruction" 0400 "simo" 00000000`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupInvite {
    pub inviter_name: String,
    pub invitee_name: String,
    pub unknown_u32_0: u32,
}

impl Body for GroupInvite {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { inviter_name: r.string()?, invitee_name: r.string()?, unknown_u32_0: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.inviter_name);
        w.string(&self.invitee_name);
        w.u32(self.unknown_u32_0);
    }
}

/// PVP::JoinMatchQueueCommand (218), client -> server: a u32, the queue name, a byte.
/// EVIDENCE: captured only (no server decoder): `8b da00 00000000 0b00 "BestOf3_1v1"
///   00`, 144 body bits. Field meanings not established.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JoinMatchQueue {
    pub unknown_u32_0: u32,
    pub queue: String,
    pub unknown_u8_1: u8,
}

impl Body for JoinMatchQueue {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { unknown_u32_0: r.u32()?, queue: r.string()?, unknown_u8_1: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unknown_u32_0);
        w.string(&self.queue);
        w.u8(self.unknown_u8_1);
    }
}

/// GuildFoundationCommand (177), client -> server: 48 bits, the guild name, one bit.
/// EVIDENCE: captured only (no server decoder): `8b b100 000000000000 0600 "qsdqsd"`
///   + 1 bit (113 body bits) and the 7-letter variant (121 bits). How the leading 48
///   zero bits split into fields is NOT established; kept as u32 + u16.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GuildFoundation {
    pub unknown_u32_0: u32,
    pub unknown_u16_1: u16,
    pub name: String,
    pub unknown_bool_2: bool,
}

impl Body for GuildFoundation {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { unknown_u32_0: r.u32()?, unknown_u16_1: r.u16()?, name: r.string()?, unknown_bool_2: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unknown_u32_0);
        w.u16(self.unknown_u16_1);
        w.string(&self.name);
        w.bit(self.unknown_bool_2);
    }
}

macro_rules! verbatim_body {
    ($($(#[$m:meta])* $name:ident;)*) => {$(
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq, Default)]
        pub struct $name {
            /// The body to the end of the 0x8B message; layout not established.
            pub unknown_body: Raw,
        }
        impl Body for $name {
            fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
                Ok(Self { unknown_body: rest(r)? })
            }
            fn encode(&self, w: &mut BitWriter) {
                self.unknown_body.write(w)
            }
        }
    )*};
}

verbatim_body! {
    /// GuildListCommand (175), client -> server. Captured: 144 body bits, seventeen
    /// zero bytes then 0xFF. No server decoder; layout NOT established.
    GuildList;
    /// BuddyListCommand (185), client -> server. Captured: 57 zero body bits. No
    /// server decoder; layout NOT established.
    BuddyList;
    /// GroupQueryCommand (191), client -> server, sent while ungrouped and left
    /// unanswered (server.py _handle_group). Captured: 184 body bits, 0x10 at body
    /// byte 6, otherwise zero. Layout NOT established.
    GroupQuery;
}
