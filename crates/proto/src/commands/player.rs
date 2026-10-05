//! The local player, other players, character service, map entry and hand-offs, and the
//! client's session-level requests and telemetry.
//!
//! SOURCE: experimental dsor/newplayer.py (35, 39, 40), dsor/mapentry2018.py (29, 283,
//!   176, 141), dsor/messages.py (121 build_server_handoff, 72 build_exit_info),
//!   dsor/selection2018.py (144), dsor/generation2018.py (143), dsor/combat.py (133
//!   encode_player_level, 134 encode_xp_changed), dsor/mapinstance.py (137
//!   encode_currency_changed), dsor/events.py (233 encode_schedule_2018);
//!   client side dsor/travel.py (104, 105), dsor/logout.py (107), dsor/telemetry.py
//!   (120, 238, 243, 244, 245), dsor/messages.py read_named_signal (282), server.py
//!   _describe_entity (34).
//!
//! CONTRACT: a list whose entry layout the server never writes (it always sends the
//!   count 0) is decoded only when empty; any other count is refused as Invalid
//!   rather than guessed.

use dsor_raknet::{BitReader, BitWriter};

use super::wire::{counted, list, Body, ReadExt, WriteExt, MOST};
use super::DecodeError;

fn invalid(what: &'static str, value: i64) -> DecodeError {
    DecodeError::Invalid { what, value }
}

/// A u32 count that must be zero: the entry layout is not established.
fn empty_list(r: &mut BitReader<'_>, what: &'static str) -> Result<(), DecodeError> {
    let n = r.u32()?;
    if n != 0 {
        return Err(invalid(what, n as i64));
    }
    Ok(())
}

fn strings(r: &mut BitReader<'_>, what: &'static str) -> Result<Vec<String>, DecodeError> {
    counted(r, MOST, what, |r| r.string())
}

fn write_strings(w: &mut BitWriter, v: &[String]) {
    w.count(v.len());
    for s in v {
        w.string(s);
    }
}

fn words(r: &mut BitReader<'_>, what: &'static str) -> Result<Vec<u32>, DecodeError> {
    counted(r, MOST, what, |r| r.u32())
}

fn write_words(w: &mut BitWriter, v: &[u32]) {
    w.count(v.len());
    for &x in v {
        w.u32(x);
    }
}

fn fixed_u32<const N: usize>(r: &mut BitReader<'_>) -> Result<[u32; N], DecodeError> {
    let mut out = [0u32; N];
    for v in &mut out {
        *v = r.u32()?;
    }
    Ok(out)
}

fn fixed_f32<const N: usize>(r: &mut BitReader<'_>) -> Result<[f32; N], DecodeError> {
    let mut out = [0f32; N];
    for v in &mut out {
        *v = r.f32()?;
    }
    Ok(out)
}

fn fixed_u8<const N: usize>(r: &mut BitReader<'_>) -> Result<[u8; N], DecodeError> {
    let mut out = [0u8; N];
    for v in &mut out {
        *v = r.u8()?;
    }
    Ok(out)
}

fn fixed_bool<const N: usize>(r: &mut BitReader<'_>) -> Result<[bool; N], DecodeError> {
    let mut out = [false; N];
    for v in &mut out {
        *v = r.bit()?;
    }
    Ok(out)
}

// ---------------------------------------------------------------- server: map entry

/// InstanceConfigClientInfoCommand (29): the map server's answer to the client's
/// ready signal. u32 (refused above 1), two bools.
/// SOURCE: dsor/mapentry2018.py instance_info; reader sub_9C8ED6 (vtable 0xFFE4AC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InstanceConfigClientInfo {
    pub unknown_u32_0: u32,
    pub unknown_flag_1: bool,
    pub unknown_flag_2: bool,
}

impl Body for InstanceConfigClientInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let unknown_u32_0 = r.u32()?;
        if unknown_u32_0 > 1 {
            return Err(invalid("InstanceConfigClientInfo u32", unknown_u32_0 as i64));
        }
        Ok(Self { unknown_u32_0, unknown_flag_1: r.bit()?, unknown_flag_2: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unknown_u32_0);
        w.bit(self.unknown_flag_1);
        w.bit(self.unknown_flag_2);
    }
}

/// AchievementInfoCommand (283): a u32 count (0..=1000000) then the achievements.
/// The server always sends none; the entry layout is not established, so only an
/// empty list decodes. SOURCE: dsor/mapentry2018.py achievements; reader sub_8FDB23.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AchievementInfo;

impl Body for AchievementInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        empty_list(r, "achievement count (entry layout unknown)")?;
        Ok(Self)
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(0);
    }
}

/// GuildStatusCommand (176), not actor-scoped: i8 status (-1..3), u32 guild, string,
/// u32, bit, u32, u32, then a u32 member count (always 0 from this server; member
/// layout not established). SOURCE: dsor/mapentry2018.py guild_status; reader
/// sub_A22B7D.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GuildStatus {
    pub status: i8,
    pub guild: u32,
    pub name: String,
    pub unknown_u32_0: u32,
    pub unknown_flag_1: bool,
    pub unknown_u32_2: u32,
    pub unknown_u32_3: u32,
}

impl Body for GuildStatus {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let status = r.i8()?;
        if !(-1..=3).contains(&status) {
            return Err(invalid("guild status", status as i64));
        }
        let v = Self {
            status,
            guild: r.u32()?,
            name: r.string()?,
            unknown_u32_0: r.u32()?,
            unknown_flag_1: r.bit()?,
            unknown_u32_2: r.u32()?,
            unknown_u32_3: r.u32()?,
        };
        empty_list(r, "guild member count (member layout unknown)")?;
        Ok(v)
    }
    fn encode(&self, w: &mut BitWriter) {
        w.i8(self.status);
        w.u32(self.guild);
        w.string(&self.name);
        w.u32(self.unknown_u32_0);
        w.bit(self.unknown_flag_1);
        w.u32(self.unknown_u32_2);
        w.u32(self.unknown_u32_3);
        w.u32(0);
    }
}

/// SpecialOfferCommand (141): three bits each followed by a struct only when set
/// (always clear here; struct layouts not established), a bit, a string, four u32
/// counts (always 0; entry layouts not established).
/// SOURCE: dsor/mapentry2018.py special_offer; reader sub_A1DB44 (vtable 0x102E214).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpecialOffer {
    pub unknown_flag_3: bool,
    pub text: String,
}

impl Body for SpecialOffer {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        for _ in 0..3 {
            if r.bit()? {
                return Err(invalid("special offer present (struct layout unknown)", 1));
            }
        }
        let v = Self { unknown_flag_3: r.bit()?, text: r.string()? };
        for _ in 0..4 {
            empty_list(r, "special offer count (entry layout unknown)")?;
        }
        Ok(v)
    }
    fn encode(&self, w: &mut BitWriter) {
        for _ in 0..3 {
            w.bit(false);
        }
        w.bit(self.unknown_flag_3);
        w.string(&self.text);
        for _ in 0..4 {
            w.u32(0);
        }
    }
}

/// SwitchMapCommand (121), not actor-scoped: the host:port to connect to next, then
/// one bit -- set (0x80) toward the character service, clear toward a map server.
/// SOURCE: dsor/messages.py build_server_handoff (HANDOFF_TRAILER_CHARACTER /
///   HANDOFF_TRAILER_MAP); the trailer clears client+0x1A78 (0x52262D) and the login
/// state enters CharacterSelectionState (server.py _finish_due_logouts).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SwitchMap {
    pub target: String,
    pub character_service: bool,
}

impl Body for SwitchMap {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { target: r.string()?, character_service: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.target);
        w.bit(self.character_service);
    }
}

/// One exit: entity name, destination, a small enum (-1..13), a 16-byte Guid written
/// as a u16 length then the bytes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Exit {
    pub name: String,
    pub destination: String,
    pub value: i32,
    pub guid: Vec<u8>,
}

/// ExitInfoCommand (72): u32 count, then per exit name, destination, i32, guid blob.
/// SOURCE: dsor/messages.py build_exit_info; reader 0x9A70EE / sub_A5ACA6.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExitInfo {
    pub exits: Vec<Exit>,
}

impl Body for ExitInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let exits = counted(r, MOST, "exit count", |r| {
            let name = r.string()?;
            let destination = r.string()?;
            let value = r.i32()?;
            let n = r.u16()? as usize;
            let guid = r.read_bytes(n)?;
            Ok(Exit { name, destination, value, guid })
        })?;
        Ok(Self { exits })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.exits.len());
        for e in &self.exits {
            w.string(&e.name);
            w.string(&e.destination);
            w.i32(e.value);
            w.u16(e.guid.len() as u16);
            w.write_bytes(&e.guid);
        }
    }
}

// ---------------------------------------------------------------- character service

/// One roster equipment entry: template, slot, colour (4 floats), a bit.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SelectionEquipment {
    pub template: String,
    pub slot: String,
    pub colour: [f32; 4],
    pub unknown_flag_0: bool,
}

/// One roster character, seventeen fields in reader order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SelectionCharacter {
    pub name: String,
    pub map_name: String,
    pub player_class: u32,
    pub gender: u32,
    pub hair: u8,
    pub beard: u8,
    pub body: u8,
    pub variation: u8,
    pub character: u32,
    pub unknown_u32_0: u32,
    pub andermant: u32,
    pub unknown_u32_1: u32,
    pub experience: u32,
    pub level: u32,
    /// 4x4 matrix (sub_C4A76A).
    pub transform: [f32; 16],
    pub unknown_flag_0: bool,
    pub unknown_flag_1: bool,
}

/// CharacterSelectionCommand (144), not actor-scoped, the SAME layout both ways
/// (writer sub_A1CEB7, reader sub_A1C7FD, vtable 0x102DB28): u8 operation (<= 10),
/// u8 denyReason (<= 7), u32 character, u32 capacity, u32 free, bit, u32, string,
/// u32 character count (<= 100) + characters, u32 equipment count (<= 20) + entries.
/// SOURCE: dsor/selection2018.py encode_selection / decode_selection_request.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CharacterSelection {
    pub operation: u8,
    pub deny_reason: u8,
    pub character: u32,
    pub capacity: u32,
    pub free: u32,
    pub unknown_flag_0: bool,
    pub unknown_u32_3: u32,
    pub unknown_text_0: String,
    pub characters: Vec<SelectionCharacter>,
    pub equipment: Vec<SelectionEquipment>,
}

impl Body for CharacterSelection {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let operation = r.u8()?;
        if operation > 0x0A {
            return Err(invalid("selection operation", operation as i64));
        }
        let deny_reason = r.u8()?;
        if deny_reason > 7 {
            return Err(invalid("selection denyReason", deny_reason as i64));
        }
        let character = r.u32()?;
        let capacity = r.u32()?;
        let free = r.u32()?;
        let unknown_flag_0 = r.bit()?;
        let unknown_u32_3 = r.u32()?;
        let unknown_text_0 = r.string()?;
        let characters = counted(r, 100, "character amount", |r| {
            Ok(SelectionCharacter {
                name: r.string()?,
                map_name: r.string()?,
                player_class: r.u32()?,
                gender: r.u32()?,
                hair: r.u8()?,
                beard: r.u8()?,
                body: r.u8()?,
                variation: r.u8()?,
                character: r.u32()?,
                unknown_u32_0: r.u32()?,
                andermant: r.u32()?,
                unknown_u32_1: r.u32()?,
                experience: r.u32()?,
                level: r.u32()?,
                transform: fixed_f32::<16>(r)?,
                unknown_flag_0: r.bit()?,
                unknown_flag_1: r.bit()?,
            })
        })?;
        let equipment = counted(r, 20, "equipment amount", |r| {
            Ok(SelectionEquipment {
                template: r.string()?,
                slot: r.string()?,
                colour: fixed_f32::<4>(r)?,
                unknown_flag_0: r.bit()?,
            })
        })?;
        Ok(Self {
            operation,
            deny_reason,
            character,
            capacity,
            free,
            unknown_flag_0,
            unknown_u32_3,
            unknown_text_0,
            characters,
            equipment,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.operation);
        w.u8(self.deny_reason);
        w.u32(self.character);
        w.u32(self.capacity);
        w.u32(self.free);
        w.bit(self.unknown_flag_0);
        w.u32(self.unknown_u32_3);
        w.string(&self.unknown_text_0);
        w.count(self.characters.len());
        for c in &self.characters {
            w.string(&c.name);
            w.string(&c.map_name);
            w.u32(c.player_class);
            w.u32(c.gender);
            w.u8(c.hair);
            w.u8(c.beard);
            w.u8(c.body);
            w.u8(c.variation);
            w.u32(c.character);
            w.u32(c.unknown_u32_0);
            w.u32(c.andermant);
            w.u32(c.unknown_u32_1);
            w.u32(c.experience);
            w.u32(c.level);
            for f in c.transform {
                w.f32(f);
            }
            w.bit(c.unknown_flag_0);
            w.bit(c.unknown_flag_1);
        }
        w.count(self.equipment.len());
        for e in &self.equipment {
            w.string(&e.template);
            w.string(&e.slot);
            for f in e.colour {
                w.f32(f);
            }
            w.bit(e.unknown_flag_0);
        }
    }
}

/// CharacterGenerationCommand (143), the SAME body both ways (reader 0xA1BF46, writer
/// 0xA1C26F, vtable 0x102D7FC); the server's copy is followed by the actor, the
/// client's is not. u8 operation (<= 13), u8 denyReason (<= 13), i8 class (<= 4),
/// i8 gender (<= 2), u32 character, u32 slot, u8 x4 appearance, string name, i32 x4,
/// f32 x16 matrix, two bits, string text, string token.
/// SOURCE: dsor/generation2018.py encode_generation / decode_generation.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CharacterGeneration {
    pub operation: u8,
    pub deny_reason: u8,
    pub player_class: i8,
    pub gender: i8,
    pub character: u32,
    pub slot: u32,
    pub hair: u8,
    pub beard: u8,
    pub body: u8,
    pub variation: u8,
    pub name: String,
    pub words: [u32; 4],
    pub matrix: [f32; 16],
    pub flag: bool,
    pub request_flag: bool,
    pub text: String,
    pub token: String,
}

impl Body for CharacterGeneration {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let operation = r.u8()?;
        let deny_reason = r.u8()?;
        if operation > 0x0D {
            return Err(invalid("generation operation", operation as i64));
        }
        if deny_reason > 0x0D {
            return Err(invalid("generation denyReason", deny_reason as i64));
        }
        let player_class = r.i8()?;
        let gender = r.i8()?;
        if player_class > 4 {
            return Err(invalid("generation class", player_class as i64));
        }
        if gender > 2 {
            return Err(invalid("generation gender", gender as i64));
        }
        Ok(Self {
            operation,
            deny_reason,
            player_class,
            gender,
            character: r.u32()?,
            slot: r.u32()?,
            hair: r.u8()?,
            beard: r.u8()?,
            body: r.u8()?,
            variation: r.u8()?,
            name: r.string()?,
            words: fixed_u32::<4>(r)?,
            matrix: fixed_f32::<16>(r)?,
            flag: r.bit()?,
            request_flag: r.bit()?,
            text: r.string()?,
            token: r.string()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.operation);
        w.u8(self.deny_reason);
        w.i8(self.player_class);
        w.i8(self.gender);
        w.u32(self.character);
        w.u32(self.slot);
        w.u8(self.hair);
        w.u8(self.beard);
        w.u8(self.body);
        w.u8(self.variation);
        w.string(&self.name);
        for v in self.words {
            w.u32(v);
        }
        for f in self.matrix {
            w.f32(f);
        }
        w.bit(self.flag);
        w.bit(self.request_flag);
        w.string(&self.text);
        w.string(&self.token);
    }
}

// ---------------------------------------------------------------- server: players

/// One entry of the table dictionary at NewPlayer +404 (sub_A3225B): two strings and
/// seven u32.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableEntry {
    pub key: String,
    pub name: String,
    pub values: [u32; 7],
}

/// NewPlayerCommand (35): the local player. 58 fields in reader order.
/// SOURCE: dsor/newplayer.py encode_new_player; reader sub_9A8502 (vtable 0xfee2e8
///   slot 5), writer 0x9AC4B8; meanings from HandleNewPlayerCommand 0x543AF6.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewPlayer {
    /// Six bools at +16..+21; index 2 is the admin byte, 3 the PvP flag.
    pub leading_flags: [bool; 6],
    /// u32 enum at +24, refused above 2.
    pub mode: u32,
    /// The two 32-byte appearance blocks at +28 and +60 (8 u32 each).
    pub appearance: [u32; 8],
    pub appearance_extra: [u32; 8],
    /// Bools at +92, +93.
    pub middle_flags: [bool; 2],
    /// i8 enum at +96, -1..2.
    pub kind: i8,
    /// Spawn position (+112) and heading (+128).
    pub position: [f32; 3],
    pub heading: f32,
    /// +132.
    pub name: String,
    /// Six u8 at +196..+201: class, gender, hair, beard, body, variation.
    pub parts: [u8; 6],
    /// +204: _Template_Player row (the class).
    pub template_row: u32,
    /// +208.
    pub secondary: String,
    /// +212 current health, +216 current resource (whole parts).
    pub words_212: [u32; 2],
    /// i8 enum at +220 (-1..5): the armament.
    pub rank: i8,
    /// +224, 74 bits: the GameFaction -- faction id, owner actor, i8 type (-1..3),
    /// two bools.
    pub composite_words: [u32; 2],
    pub composite_enum: i8,
    pub composite_flags: [bool; 2],
    /// +240..+268: [0, level, experience, level floor, level ceiling, 0, 0, 0].
    pub words_240: [u32; 8],
    /// +272: the portal's unlock bits. +292: unnamed.
    pub words_272: Vec<u32>,
    pub words_292: Vec<u32>,
    /// +312.
    pub strings_312: Vec<String>,
    /// +332..+352.
    pub words_332: [u32; 6],
    /// +356..+372: the wallet.
    pub currencies: [u32; 5],
    /// +376.
    pub long_376: u64,
    /// +384: (string, u64) entries.
    pub entries_384: Vec<(String, u64)>,
    /// +404 table (sub_A3340A): 22 u32, a dictionary, 4 x 7 u32.
    pub table_leading: [u32; 22],
    pub table_dictionary: Vec<TableEntry>,
    pub table_groups: [u32; 28],
    /// +632: (u32, u32, i8 -1..30) entries.
    pub entries_632: Vec<(u32, u32, i8)>,
    /// +652, -1 by default.
    pub identifier: i32,
    /// +656.
    pub block_656: [u32; 8],
    /// u8 enum at +688, 0..2.
    pub state: u8,
    /// +692.
    pub tertiary: String,
    /// +696.
    pub block_696: [u32; 8],
}

impl Body for NewPlayer {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let leading_flags = fixed_bool::<6>(r)?;
        let mode = r.u32()?;
        if mode > 2 {
            return Err(invalid("NewPlayer mode", mode as i64));
        }
        let appearance = fixed_u32::<8>(r)?;
        let appearance_extra = fixed_u32::<8>(r)?;
        let middle_flags = fixed_bool::<2>(r)?;
        let kind = r.i8()?;
        let position = fixed_f32::<3>(r)?;
        let heading = r.f32()?;
        let name = r.string()?;
        let parts = fixed_u8::<6>(r)?;
        let template_row = r.u32()?;
        let secondary = r.string()?;
        let words_212 = fixed_u32::<2>(r)?;
        let rank = r.i8()?;
        let composite_words = fixed_u32::<2>(r)?;
        let composite_enum = r.i8()?;
        let composite_flags = fixed_bool::<2>(r)?;
        let words_240 = fixed_u32::<8>(r)?;
        let words_272 = words(r, "NewPlayer +272 count")?;
        let words_292 = words(r, "NewPlayer +292 count")?;
        let strings_312 = strings(r, "NewPlayer +312 count")?;
        let words_332 = fixed_u32::<6>(r)?;
        let currencies = fixed_u32::<5>(r)?;
        let long_376 = r.u64()?;
        let entries_384 = counted(r, MOST, "NewPlayer +384 count", |r| Ok((r.string()?, r.u64()?)))?;
        let table_leading = fixed_u32::<22>(r)?;
        let table_dictionary = counted(r, MOST, "NewPlayer table dictionary count", |r| {
            Ok(TableEntry { key: r.string()?, name: r.string()?, values: fixed_u32::<7>(r)? })
        })?;
        let table_groups = fixed_u32::<28>(r)?;
        let entries_632 = counted(r, MOST, "NewPlayer +632 count", |r| Ok((r.u32()?, r.u32()?, r.i8()?)))?;
        let identifier = r.i32()?;
        let block_656 = fixed_u32::<8>(r)?;
        let state = r.u8()?;
        if state > 2 {
            return Err(invalid("NewPlayer state", state as i64));
        }
        let tertiary = r.string()?;
        let block_696 = fixed_u32::<8>(r)?;
        Ok(Self {
            leading_flags,
            mode,
            appearance,
            appearance_extra,
            middle_flags,
            kind,
            position,
            heading,
            name,
            parts,
            template_row,
            secondary,
            words_212,
            rank,
            composite_words,
            composite_enum,
            composite_flags,
            words_240,
            words_272,
            words_292,
            strings_312,
            words_332,
            currencies,
            long_376,
            entries_384,
            table_leading,
            table_dictionary,
            table_groups,
            entries_632,
            identifier,
            block_656,
            state,
            tertiary,
            block_696,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        for b in self.leading_flags {
            w.bit(b);
        }
        w.u32(self.mode);
        for v in self.appearance.iter().chain(&self.appearance_extra) {
            w.u32(*v);
        }
        for b in self.middle_flags {
            w.bit(b);
        }
        w.i8(self.kind);
        w.vec3(self.position);
        w.f32(self.heading);
        w.string(&self.name);
        for p in self.parts {
            w.u8(p);
        }
        w.u32(self.template_row);
        w.string(&self.secondary);
        for v in self.words_212 {
            w.u32(v);
        }
        w.i8(self.rank);
        for v in self.composite_words {
            w.u32(v);
        }
        w.i8(self.composite_enum);
        for b in self.composite_flags {
            w.bit(b);
        }
        for v in self.words_240 {
            w.u32(v);
        }
        write_words(w, &self.words_272);
        write_words(w, &self.words_292);
        write_strings(w, &self.strings_312);
        for v in self.words_332.iter().chain(&self.currencies) {
            w.u32(*v);
        }
        w.u64(self.long_376);
        w.count(self.entries_384.len());
        for (s, v) in &self.entries_384 {
            w.string(s);
            w.u64(*v);
        }
        for v in self.table_leading {
            w.u32(v);
        }
        w.count(self.table_dictionary.len());
        for e in &self.table_dictionary {
            w.string(&e.key);
            w.string(&e.name);
            for v in e.values {
                w.u32(v);
            }
        }
        for v in self.table_groups {
            w.u32(v);
        }
        w.count(self.entries_632.len());
        for (a, b, c) in &self.entries_632 {
            w.u32(*a);
            w.u32(*b);
            w.i8(*c);
        }
        w.i32(self.identifier);
        for v in self.block_656 {
            w.u32(v);
        }
        w.u8(self.state);
        w.string(&self.tertiary);
        for v in self.block_696 {
            w.u32(v);
        }
    }
}

/// One worn slot of a remote player (NewRemotePlayer +192, RemotePlayerInfo): the
/// slot as u8 (sub_9C33E6), a counted list of skin parts, a bit and, when set, four
/// colour floats (sub_9CF056).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WornSlot {
    pub slot: u8,
    pub skin_parts: Vec<String>,
    pub colour: Option<[f32; 4]>,
}

fn worn(r: &mut BitReader<'_>) -> Result<Vec<WornSlot>, DecodeError> {
    counted(r, MOST, "worn slot count", |r| {
        let slot = r.u8()?;
        let skin_parts = strings(r, "skin part count")?;
        let colour = if r.bit()? { Some(fixed_f32::<4>(r)?) } else { None };
        Ok(WornSlot { slot, skin_parts, colour })
    })
}

fn write_worn(w: &mut BitWriter, v: &[WornSlot]) {
    w.count(v.len());
    for s in v {
        w.u8(s.slot);
        write_strings(w, &s.skin_parts);
        w.bit(s.colour.is_some());
        if let Some(c) = s.colour {
            for f in c {
                w.f32(f);
            }
        }
    }
}

/// GameFaction composite (74 bits, sub_A4C4E8): faction id, owner actor, i8 type
/// (-1..3), the PvP bool, one more bool. SOURCE: dsor/pvp.py faction_of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Faction {
    pub faction_id: u32,
    pub owner: u32,
    pub kind: i8,
    pub pvp: bool,
    pub unknown_flag_4: bool,
}

impl Faction {
    fn read(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { faction_id: r.u32()?, owner: r.u32()?, kind: r.i8()?, pvp: r.bit()?, unknown_flag_4: r.bit()? })
    }
    fn write(&self, w: &mut BitWriter) {
        w.u32(self.faction_id);
        w.u32(self.owner);
        w.i8(self.kind);
        w.bit(self.pvp);
        w.bit(self.unknown_flag_4);
    }
}

/// NewRemotePlayerCommand (39): another player to draw.
/// SOURCE: dsor/newplayer.py encode_new_remote_player; reader sub_9A8CCB (vtable
///   0xfee368 slot 5), writer sub_9ACB38; handler 0x544FD5.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewRemotePlayer {
    /// +16..+20: index 2 admin, index 3 PvP flag raised.
    pub flags: [bool; 5],
    /// +24, refused above 2.
    pub mode: u32,
    /// +28: visible.
    pub visible: bool,
    pub position: [f32; 3],
    pub heading: f32,
    pub name: String,
    /// +116.
    pub secondary: String,
    /// +180..+185: class, gender, hair, beard, body, variation.
    pub parts: [u8; 6],
    /// +188: the template row.
    pub template_row: u32,
    /// +192: what they wear.
    pub equipment: Vec<WornSlot>,
    /// +216.
    pub strings_216: Vec<String>,
    /// +236 current health, +240 MaxHealthPoints attribute, +244 maximum health.
    pub health: u32,
    pub max_health_attribute: u32,
    pub max_health: u32,
    /// +248.
    pub unknown_u32_248: u32,
    /// +252: armament.
    pub armament: i8,
    /// +256.
    pub faction: Faction,
    /// +272: tick the PvP flag was raised; +276: level.
    pub pvp_tick: u32,
    pub level: u32,
    /// +280, -1 leaves it alone.
    pub unknown_i32_280: i32,
    /// +284.
    pub unknown_u32_284: u32,
    /// +288, enum 0..2.
    pub unknown_u8_288: u8,
    /// +292.
    pub tertiary: String,
}

impl Body for NewRemotePlayer {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let flags = fixed_bool::<5>(r)?;
        let mode = r.u32()?;
        if mode > 2 {
            return Err(invalid("NewRemotePlayer mode", mode as i64));
        }
        Ok(Self {
            flags,
            mode,
            visible: r.bit()?,
            position: fixed_f32::<3>(r)?,
            heading: r.f32()?,
            name: r.string()?,
            secondary: r.string()?,
            parts: fixed_u8::<6>(r)?,
            template_row: r.u32()?,
            equipment: worn(r)?,
            strings_216: strings(r, "NewRemotePlayer +216 count")?,
            health: r.u32()?,
            max_health_attribute: r.u32()?,
            max_health: r.u32()?,
            unknown_u32_248: r.u32()?,
            armament: r.i8()?,
            faction: Faction::read(r)?,
            pvp_tick: r.u32()?,
            level: r.u32()?,
            unknown_i32_280: r.i32()?,
            unknown_u32_284: r.u32()?,
            unknown_u8_288: r.u8()?,
            tertiary: r.string()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        for b in self.flags {
            w.bit(b);
        }
        w.u32(self.mode);
        w.bit(self.visible);
        w.vec3(self.position);
        w.f32(self.heading);
        w.string(&self.name);
        w.string(&self.secondary);
        for p in self.parts {
            w.u8(p);
        }
        w.u32(self.template_row);
        write_worn(w, &self.equipment);
        write_strings(w, &self.strings_216);
        w.u32(self.health);
        w.u32(self.max_health_attribute);
        w.u32(self.max_health);
        w.u32(self.unknown_u32_248);
        w.i8(self.armament);
        self.faction.write(w);
        w.u32(self.pvp_tick);
        w.u32(self.level);
        w.i32(self.unknown_i32_280);
        w.u32(self.unknown_u32_284);
        w.u8(self.unknown_u8_288);
        w.string(&self.tertiary);
    }
}

/// RemotePlayerInfoCommand (40): re-dress and re-state another player in place.
/// string, worn slots, animation set, counted strings, u32 max health, u32, i8
/// armament, faction composite, u32 level, i32 (-1), u8, string.
/// SOURCE: dsor/newplayer.py encode_remote_player_info; reader sub_9A9B5C, writer
///   0x9AD715, handler 0x54607F.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RemotePlayerInfo {
    /// +0x10.
    pub unknown_text_0: String,
    pub equipment: Vec<WornSlot>,
    /// +0x68, never empty.
    pub animation_set: String,
    /// +0xA8.
    pub strings_a8: Vec<String>,
    /// +0xBC: MaxHealthPoints.
    pub max_health: u32,
    /// +0xC0.
    pub unknown_u32_c0: u32,
    /// +0xC4.
    pub armament: i8,
    /// +0xC8.
    pub faction: Faction,
    /// +0xD8.
    pub level: u32,
    /// +0xDC.
    pub unknown_i32_dc: i32,
    /// +0xE0.
    pub unknown_u8_e0: u8,
    /// +0xE4.
    pub unknown_text_e4: String,
}

impl Body for RemotePlayerInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            unknown_text_0: r.string()?,
            equipment: worn(r)?,
            animation_set: r.string()?,
            strings_a8: strings(r, "RemotePlayerInfo +0xA8 count")?,
            max_health: r.u32()?,
            unknown_u32_c0: r.u32()?,
            armament: r.i8()?,
            faction: Faction::read(r)?,
            level: r.u32()?,
            unknown_i32_dc: r.i32()?,
            unknown_u8_e0: r.u8()?,
            unknown_text_e4: r.string()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.unknown_text_0);
        write_worn(w, &self.equipment);
        w.string(&self.animation_set);
        write_strings(w, &self.strings_a8);
        w.u32(self.max_health);
        w.u32(self.unknown_u32_c0);
        w.i8(self.armament);
        self.faction.write(w);
        w.u32(self.level);
        w.i32(self.unknown_i32_dc);
        w.u8(self.unknown_u8_e0);
        w.string(&self.unknown_text_e4);
    }
}

// ---------------------------------------------------------------- server: progress

/// PlayerLevelUpdateCommand (133): one u32, the level.
/// SOURCE: dsor/combat.py encode_player_level(trailing=False), dsor/vitals.py
///   player_level(padding=0); reader 0x9A93C8 (vtable 0xfee0e8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerLevelUpdate {
    pub level: u32,
}

impl Body for PlayerLevelUpdate {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { level: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.level);
    }
}

/// XPChangedCommand (134): total experience, level, the level's floor, the next
/// level's floor, and a bit (set with a level-up in the samples).
/// SOURCE: dsor/combat.py encode_xp_changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct XpChanged {
    pub total: u32,
    pub level: u32,
    pub floor: u32,
    pub ceiling: u32,
    pub levelled: bool,
}

impl Body for XpChanged {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { total: r.u32()?, level: r.u32()?, floor: r.u32()?, ceiling: r.u32()?, levelled: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.total);
        w.u32(self.level);
        w.u32(self.floor);
        w.u32(self.ceiling);
        w.bit(self.levelled);
    }
}

/// CurrencyChangedCommand (137): five u32 wallet slots (slot 0 andermant -> player
/// +916, slot 1 gold -> +920), a u64, and the notify bit.
/// SOURCE: dsor/mapinstance.py encode_currency_changed; handler 0x513E39.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CurrencyChanged {
    pub wallet: [u32; 5],
    pub unknown_u64_5: u64,
    pub notify: bool,
}

impl Body for CurrencyChanged {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { wallet: fixed_u32::<5>(r)?, unknown_u64_5: r.u64()?, notify: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        for v in self.wallet {
            w.u32(v);
        }
        w.u64(self.unknown_u64_5);
        w.bit(self.notify);
    }
}

/// One event-schedule record (reader sub_A5CD0F): u32 id, string key, three bits
/// (flag, map_local, unknown); when map_local is clear, six u32 blanks, six u32 date
/// parts (y, m, d, h, min, s) and a counted string list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScheduleEntry {
    pub identifier: u32,
    pub key: String,
    pub flag: bool,
    pub map_local: bool,
    pub unknown_bit_2: bool,
    pub blanks: [u32; 6],
    pub date: [u32; 6],
    pub parameters: Vec<String>,
}

/// EventUpdateCommand (233), not actor-scoped: the event schedule, a u32 count of
/// records. SOURCE: dsor/events.py encode_schedule_2018; vtable 0xFFDEAC.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventUpdate {
    pub entries: Vec<ScheduleEntry>,
}

impl Body for EventUpdate {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let entries = counted(r, MOST, "event count", |r| {
            let mut e = ScheduleEntry {
                identifier: r.u32()?,
                key: r.string()?,
                flag: r.bit()?,
                map_local: r.bit()?,
                unknown_bit_2: r.bit()?,
                ..Default::default()
            };
            if !e.map_local {
                e.blanks = fixed_u32::<6>(r)?;
                e.date = fixed_u32::<6>(r)?;
                e.parameters = strings(r, "event parameter count")?;
            }
            Ok(e)
        })?;
        Ok(Self { entries })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.entries.len());
        for e in &self.entries {
            w.u32(e.identifier);
            w.string(&e.key);
            w.bit(e.flag);
            w.bit(e.map_local);
            w.bit(e.unknown_bit_2);
            if !e.map_local {
                for v in e.blanks.iter().chain(&e.date) {
                    w.u32(*v);
                }
                write_strings(w, &e.parameters);
            }
        }
    }
}

// ---------------------------------------------------------------- client commands

/// ActorRequestCommand (34), client: describe this actor to me.
/// SOURCE: server.py DESCRIBE_ENTITY_OPCODE / _describe_entity (u32 actor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActorRequest {
    pub actor: u32,
}

impl Body for ActorRequest {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { actor: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.actor);
    }
}

/// UpdateShowPlayerOverheadIconsCommand (41), client: 33 bits. The server has no
/// decoder; split as u32 + bit from the captured size only (layout NOT established).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UpdateShowPlayerOverheadIcons {
    pub unknown_u32_0: u32,
    pub unknown_flag_1: bool,
}

impl Body for UpdateShowPlayerOverheadIcons {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { unknown_u32_0: r.u32()?, unknown_flag_1: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unknown_u32_0);
        w.bit(self.unknown_flag_1);
    }
}

/// SettingsInfoCommand (43), client: one bit (captured `8b 2b 00 00`, 25 bits). The
/// server has no decoder; meaning not established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SettingsInfo {
    pub unknown_flag_0: bool,
}

impl Body for SettingsInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { unknown_flag_0: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.bit(self.unknown_flag_0);
    }
}

/// UnlockMapCommand (104), client: map id, exit URL, exit id, one bit.
/// SOURCE: dsor/travel.py decode_unlock_map; reader sub_9AA292 (vtable 0xFEF0A8).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnlockMap {
    pub map_id: String,
    pub exit_url: String,
    pub exit_id: String,
    pub unknown_flag_3: bool,
}

impl Body for UnlockMap {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { map_id: r.string()?, exit_url: r.string()?, exit_id: r.string()?, unknown_flag_3: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.map_id);
        w.string(&self.exit_url);
        w.string(&self.exit_id);
        w.bit(self.unknown_flag_3);
    }
}

/// TravelCommand (105), client: travel point (may be empty), destination map, and
/// the pay-with-andermant bit.
/// SOURCE: dsor/travel.py decode; reader 0x9AA22E (vtable 0xFEF0E8).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Travel {
    pub point: String,
    pub destination: String,
    pub pay_with_rc: bool,
}

impl Body for Travel {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { point: r.string()?, destination: r.string()?, pay_with_rc: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.point);
        w.string(&self.destination);
        w.bit(self.pay_with_rc);
    }
}

/// LogoutCommand (107), client: two bits, cancel and (mode != 2) -- clear for
/// switch character. SOURCE: dsor/logout.py decode_logout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Logout {
    pub cancel: bool,
    pub plain: bool,
}

impl Body for Logout {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { cancel: r.bit()?, plain: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.bit(self.cancel);
        w.bit(self.plain);
    }
}

/// StillAliveCommand (120), client heartbeat: one u32 token, the NOT of two
/// nibble-interleaved numbers. SOURCE: dsor/telemetry.py decode_still_alive; reader
/// sub_9AA0F4, builder StillAliveTask::OnTick 0x5234CD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StillAlive {
    pub token: u32,
}

impl StillAlive {
    /// (first, second): the two numbers the token interleaves.
    pub fn numbers(&self) -> (u32, u32) {
        let word = !self.token;
        let (mut first, mut second) = (0, 0);
        for n in 0..4 {
            first |= ((word >> (8 * n)) & 0xF) << (4 * n);
            second |= ((word >> (8 * n + 4)) & 0xF) << (4 * n);
        }
        (first, second)
    }
}

impl Body for StillAlive {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { token: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.token);
    }
}

/// PlayerQueryCommand (189), client: 56 bits. The server has no decoder; split as u32,
/// string, u8 from the captured `01000000 0000 00` only (layout NOT established).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerQuery {
    pub unknown_u32_0: u32,
    pub unknown_text_1: String,
    pub unknown_u8_2: u8,
}

impl Body for PlayerQuery {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { unknown_u32_0: r.u32()?, unknown_text_1: r.string()?, unknown_u8_2: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unknown_u32_0);
        w.string(&self.unknown_text_1);
        w.u8(self.unknown_u8_2);
    }
}

/// A body of N u32 words whose meanings are not established (the server has no
/// decoder; the width is the captured size).
macro_rules! words_body {
    ($($(#[$m:meta])* $name:ident [$n:literal];)*) => {$(
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        pub struct $name {
            pub unknown_u32: [u32; $n],
        }
        impl Body for $name {
            fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
                Ok(Self { unknown_u32: fixed_u32::<$n>(r)? })
            }
            fn encode(&self, w: &mut BitWriter) {
                for v in self.unknown_u32 {
                    w.u32(v);
                }
            }
        }
    )*};
}

words_body! {
    /// GuildMessageOfTheDayRequestCommand (196), client: 32 bits (layout NOT established).
    GuildMessageOfTheDayRequest[1];
    /// ChatClientLoggedInCommand (237), client: 64 bits; the second word counts up
    /// per login in the captures (layout NOT established).
    ChatClientLoggedIn[2];
    /// Ranking::OverallUserRankingInfoCommand (257), client: 128 bits (layout NOT
    /// established; captured `ffffffff 0a000000 00000000 11000000`).
    OverallUserRankingInfo[4];
    /// GauntletNotificationInfoCommand (305), client: 64 bits (layout NOT established).
    GauntletNotificationInfo[2];
}

/// Ranking::OverallLocalUserRankingInfoCommand (259), client: 40 bits, split as u32
/// and u8 from the captured size only (layout NOT established).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OverallLocalUserRankingInfo {
    pub unknown_u32_0: u32,
    pub unknown_u8_1: u8,
}

impl Body for OverallLocalUserRankingInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { unknown_u32_0: r.u32()?, unknown_u8_1: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unknown_u32_0);
        w.u8(self.unknown_u8_1);
    }
}

/// StagingSignalCommand (282), client: a signal by name, then a u8 and a u32 (an
/// actor-shaped value, e.g. 0x00010015). The server reads only the name
/// (dsor/messages.py read_named_signal); the trailing u8 + u32 split is from the
/// captured sizes (all whole bytes) and is NOT established.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StagingSignal {
    pub name: String,
    pub unknown_u8_1: u8,
    pub unknown_u32_2: u32,
}

impl Body for StagingSignal {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { name: r.string()?, unknown_u8_1: r.u8()?, unknown_u32_2: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.name);
        w.u8(self.unknown_u8_1);
        w.u32(self.unknown_u32_2);
    }
}

/// ChatErrorReportCommand (238), client: u32, i32, ten bits (missing channel masks).
/// SOURCE: dsor/telemetry.py decode_chat_error_report; reader sub_98046E.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChatErrorReport {
    pub unknown_u32_0: u32,
    pub unknown_i32_1: i32,
    pub missing_bits: [bool; 10],
}

impl Body for ChatErrorReport {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { unknown_u32_0: r.u32()?, unknown_i32_1: r.i32()?, missing_bits: fixed_bool::<10>(r)? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unknown_u32_0);
        w.i32(self.unknown_i32_1);
        for b in self.missing_bits {
            w.bit(b);
        }
    }
}

/// ClientAverageFrameInfoTrackingCommand (243), client: u32 instance, u64, u32
/// display width, u32 height, a bit. SOURCE: dsor/telemetry.py decode_average_frame;
/// reader sub_9C89E8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClientAverageFrameInfoTracking {
    pub instance: u32,
    pub unknown_u64_1: u64,
    pub display_u32_0: u32,
    pub display_u32_1: u32,
    pub unknown_flag_4: bool,
}

impl Body for ClientAverageFrameInfoTracking {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            instance: r.u32()?,
            unknown_u64_1: r.u64()?,
            display_u32_0: r.u32()?,
            display_u32_1: r.u32()?,
            unknown_flag_4: r.bit()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.instance);
        w.u64(self.unknown_u64_1);
        w.u32(self.display_u32_0);
        w.u32(self.display_u32_1);
        w.bit(self.unknown_flag_4);
    }
}

/// One movement reset of 244 (reader sub_A5D611): u32, four f32, i32.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MovementReset {
    pub unknown_u32_0: u32,
    pub unknown_f32: [f32; 4],
    pub unknown_i32_5: i32,
}

/// ClientCurrentPositionFrameInfoTrackingCommand (244), client: u32 instance, u64,
/// five f32, counted resets, f32, u32. SOURCE: dsor/telemetry.py
/// decode_position_frame; reader sub_9C8A75.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClientCurrentPositionFrameInfoTracking {
    pub instance: u32,
    pub unknown_u64_1: u64,
    pub average_f32_2: f32,
    pub unknown_f32_3: f32,
    pub unknown_f32_4: f32,
    pub position_f32_5: f32,
    pub position_f32_6: f32,
    pub resets: Vec<MovementReset>,
    pub unknown_f32_8: f32,
    pub unknown_u32_9: u32,
}

impl Body for ClientCurrentPositionFrameInfoTracking {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            instance: r.u32()?,
            unknown_u64_1: r.u64()?,
            average_f32_2: r.f32()?,
            unknown_f32_3: r.f32()?,
            unknown_f32_4: r.f32()?,
            position_f32_5: r.f32()?,
            position_f32_6: r.f32()?,
            resets: counted(r, MOST, "movement reset count", |r| {
                Ok(MovementReset { unknown_u32_0: r.u32()?, unknown_f32: fixed_f32::<4>(r)?, unknown_i32_5: r.i32()? })
            })?,
            unknown_f32_8: r.f32()?,
            unknown_u32_9: r.u32()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.instance);
        w.u64(self.unknown_u64_1);
        for f in [self.average_f32_2, self.unknown_f32_3, self.unknown_f32_4, self.position_f32_5, self.position_f32_6] {
            w.f32(f);
        }
        w.count(self.resets.len());
        for m in &self.resets {
            w.u32(m.unknown_u32_0);
            for f in m.unknown_f32 {
                w.f32(f);
            }
            w.i32(m.unknown_i32_5);
        }
        w.f32(self.unknown_f32_8);
        w.u32(self.unknown_u32_9);
    }
}

/// One client log event (reader sub_A5D378): i32 type (-1..198), u32, counted
/// (string, i32) pairs, counted (string, string) pairs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LogEvent {
    pub event_type: i32,
    pub unknown_u32_1: u32,
    pub numbers: Vec<(String, i32)>,
    pub texts: Vec<(String, String)>,
}

/// ClientLogEventCommand (245), client: counted events, then an i32.
/// SOURCE: dsor/telemetry.py decode_log_event; reader sub_9C8BE4.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClientLogEvent {
    pub events: Vec<LogEvent>,
    pub unknown_i32_1: i32,
}

impl Body for ClientLogEvent {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let events = counted(r, MOST, "log event count", |r| {
            let event_type = r.i32()?;
            if !(-1..=198).contains(&event_type) {
                return Err(invalid("log event type", event_type as i64));
            }
            let unknown_u32_1 = r.u32()?;
            // CONTRACT: sub_A5D378 does not bound these; a negative count reads
            // nothing, which could not be re-encoded, so it is refused here.
            let n = r.count(MOST, "log event number count")?;
            let numbers = list(r, n, |r| Ok((r.string()?, r.i32()?)))?;
            let n = r.count(MOST, "log event text count")?;
            let texts = list(r, n, |r| Ok((r.string()?, r.string()?)))?;
            Ok(LogEvent { event_type, unknown_u32_1, numbers, texts })
        })?;
        Ok(Self { events, unknown_i32_1: r.i32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.events.len());
        for e in &self.events {
            w.i32(e.event_type);
            w.u32(e.unknown_u32_1);
            w.count(e.numbers.len());
            for (s, v) in &e.numbers {
                w.string(s);
                w.i32(*v);
            }
            w.count(e.texts.len());
            for (a, b) in &e.texts {
                w.string(a);
                w.string(b);
            }
        }
        w.i32(self.unknown_i32_1);
    }
}
