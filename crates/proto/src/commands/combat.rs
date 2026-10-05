//! Combat: skill commands (both directions), hits, kills, deaths, revives, status
//! effects, ground (location) effects and traps.
//!
//! SOURCE: experimental dsor/combat.py (encode_hit, encode_kill, encode_revive,
//!   encode_resurrect, decode_resurrect, encode_show_death_dialog, encode_target_skill,
//!   encode_bullet_skill, encode_skill_stop, decode_skill_use, decode_target_points,
//!   decode_sustained_skill, decode_respawn), dsor/world.py (the relayed skill command,
//!   World._relay_skill), dsor/statuseffect.py, dsor/location.py, dsor/traps.py,
//!   server.py (_quick_resurrect, _handle_resurrect, _handle_respawn).
//!
//! SKILL COMMANDS AND THEIR ACTOR SLOT. Every SkillCommand class (73-77; 79 has its
//! own) has the same body in both directions; the client sends the body alone, the
//! server sends it followed by the command's slot-9 data, which starts with the usual
//! 32-bit actor and continues (the shared actor slot 0x9F9178): u32 +100 impact tick,
//! u32 +72, u32 +104, u32 +108, a vector4 at +80 (position + rate), one bit at +96;
//! TargetPointBulletSkill (76) and ShiftedSkill (77) read one more u32 into +0x4C
//! (their slot 9 readers 0xA5CBAF, 0xA5C2D8). That data sits BETWEEN the actor and the
//! 0xFF, so it is read by `ServerCommand::decode_actor_slot_tail` after the framework
//! has read the actor, and stored in the command's `server` field (None on the client
//! side).

use dsor_raknet::{BitReader, BitWriter};

use super::wire::{counted, list, Body, Raw, ReadExt, WriteExt, MOST};
use super::{DecodeError, ServerCommand};

fn f32_list(r: &mut BitReader<'_>, most: u32, what: &'static str) -> Result<Vec<f32>, DecodeError> {
    counted(r, most, what, |r| r.f32())
}

fn write_f32_list(w: &mut BitWriter, values: &[f32]) {
    w.count(values.len());
    for &v in values {
        w.f32(v);
    }
}

fn vec4(r: &mut BitReader<'_>) -> Result<[f32; 4], DecodeError> {
    Ok([r.f32()?, r.f32()?, r.f32()?, r.f32()?])
}

fn write_vec4(w: &mut BitWriter, v: [f32; 4]) {
    for f in v {
        w.f32(f);
    }
}

/// A u32 count, then that many damage-type bytes (Hit and Kill).
fn damage_types(r: &mut BitReader<'_>) -> Result<Vec<u8>, DecodeError> {
    counted(r, MOST, "damage type count", |r| r.u8())
}

fn write_damage_types(w: &mut BitWriter, types: &[u8]) {
    w.count(types.len());
    for &t in types {
        w.u8(t);
    }
}

// ── skill commands ──────────────────────────────────────────────────────────

/// SkillCommand's own five fields, which every skill command starts with.
/// EVIDENCE: sub_9F9292 (writer) / sub_9F90E6 (reader): 16, 16, 32, 32, 32 bits
///   (dsor/combat.py SKILL_USE_*, decode_skill_use).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SkillBase {
    /// Zero passes the client's high-water check.
    pub high_water: u16,
    /// The skill's wire index (its _Template_Skill row minus one).
    pub skill_id: u16,
    /// The aim in radians, [-pi, pi].
    pub heading: f32,
    /// The caster's game tick when the skill started.
    pub start_tick: u32,
    /// +0xC8 in the server's dataclass; meaning not established.
    pub unknown_u32_0: u32,
}

impl Body for SkillBase {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { high_water: r.u16()?, skill_id: r.u16()?, heading: r.f32()?, start_tick: r.u32()?, unknown_u32_0: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u16(self.high_water);
        w.u16(self.skill_id);
        w.f32(self.heading);
        w.u32(self.start_tick);
        w.u32(self.unknown_u32_0);
    }
}

/// The server-only part of a skill command's slot 9, after the actor.
/// EVIDENCE: shared actor slot 0x9F9178 (dsor/combat.py encode_target_skill,
///   encode_bullet_skill; dsor/world.py relay). GameSkill::Setup (0x8EB7BE) ends the
///   skill at start + unblock_frame (+104) and start + motion_unblock_frame (+108).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SkillServerData {
    /// +100: the tick the blow lands (start + hit frame, or start + flight).
    pub impact_tick: u32,
    /// +72: the hit frame, or the loop-start frame (a bullet's execute phase).
    pub phase_frame: u32,
    /// +104 / +108: the two unblock frames.
    pub unblock_frame: u32,
    pub motion_unblock_frame: u32,
    /// +80: the caster's position in the description frame, then a rate (1.0).
    pub position: [f32; 4],
    /// +96: one skips movement modulation and transform correction.
    pub flag: bool,
    /// +0x4C, TargetPointBulletSkill (76) and ShiftedSkill (77) only: the shift (the
    /// ticks from the phase end to the impact).
    pub shift: Option<u32>,
}

impl SkillServerData {
    fn decode(r: &mut BitReader<'_>, shifted: bool) -> Result<Self, DecodeError> {
        Ok(Self {
            impact_tick: r.u32()?,
            phase_frame: r.u32()?,
            unblock_frame: r.u32()?,
            motion_unblock_frame: r.u32()?,
            position: vec4(r)?,
            flag: r.bit()?,
            shift: if shifted { Some(r.u32()?) } else { None },
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.impact_tick);
        w.u32(self.phase_frame);
        w.u32(self.unblock_frame);
        w.u32(self.motion_unblock_frame);
        write_vec4(w, self.position);
        w.bit(self.flag);
        if let Some(s) = self.shift {
            w.u32(s);
        }
    }
}

/// SkillCommand (73): a skill with no target, both directions. Body = SkillBase.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Skill {
    pub base: SkillBase,
    /// Server -> client only (slot 9 after the actor).
    pub server: Option<SkillServerData>,
}

impl Body for Skill {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { base: SkillBase::decode(r)?, server: None })
    }
    fn encode(&self, w: &mut BitWriter) {
        self.base.encode(w)
    }
}

/// TargetSkillCommand (74): SkillBase then the target actor (+28).
/// EVIDENCE: writer 0xA5C7EC / reader 0xA5C7B7 (dsor/combat.py encode_target_skill).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TargetSkill {
    pub base: SkillBase,
    /// 0xFFFFFFFF (or 0) when fired at nothing.
    pub target: u32,
    pub server: Option<SkillServerData>,
}

impl Body for TargetSkill {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { base: SkillBase::decode(r)?, target: r.u32()?, server: None })
    }
    fn encode(&self, w: &mut BitWriter) {
        self.base.encode(w);
        w.u32(self.target);
    }
}

/// BulletSkillCommand (75): SkillBase, the step vector4 (+176), the orbit byte (+196).
/// EVIDENCE: writer 0xA344BF / reader 0xA34464 (dsor/combat.py encode_bullet_skill).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BulletSkill {
    pub base: SkillBase,
    /// The bullet's step per tick; only its direction is used (w is 0).
    pub step: [f32; 4],
    /// Orbiting direction 0 or 1 (2 = "Random" must not be sent).
    pub orbit: u8,
    pub server: Option<SkillServerData>,
}

impl Body for BulletSkill {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { base: SkillBase::decode(r)?, step: vec4(r)?, orbit: r.u8()?, server: None })
    }
    fn encode(&self, w: &mut BitWriter) {
        self.base.encode(w);
        write_vec4(w, self.step);
        w.u8(self.orbit);
    }
}

/// sub_9F920A: one u8 count, then that many float3 (the clicked points).
fn points(r: &mut BitReader<'_>) -> Result<Vec<[f32; 3]>, DecodeError> {
    let n = r.u8()? as usize;
    list(r, n, |r| r.vec3())
}

fn write_points(w: &mut BitWriter, points: &[[f32; 3]]) {
    w.u8(points.len() as u8);
    for &p in points {
        w.vec3(p);
    }
}

/// TargetPointBulletSkillCommand (76): BulletSkill's body, then the target points
/// (u8 count + float3s, sub_9F920A).
/// EVIDENCE: reader 0xA5CB83 reads the points (dsor/combat.py decode_target_points);
///   every captured client 76 is 368 bits = 264 (BulletSkill) + 8 + 96, so the points
///   follow the bullet fields, not SkillBase (decode_target_points' offset 16 holds for
///   77 only).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TargetPointBulletSkill {
    pub base: SkillBase,
    pub step: [f32; 4],
    pub orbit: u8,
    pub points: Vec<[f32; 3]>,
    pub server: Option<SkillServerData>,
}

impl Body for TargetPointBulletSkill {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { base: SkillBase::decode(r)?, step: vec4(r)?, orbit: r.u8()?, points: points(r)?, server: None })
    }
    fn encode(&self, w: &mut BitWriter) {
        self.base.encode(w);
        write_vec4(w, self.step);
        w.u8(self.orbit);
        write_points(w, &self.points);
    }
}

/// ShiftedSkillCommand (77): SkillBase then the target points (u8 count + float3s).
/// EVIDENCE: reader 0xA5C2B5 -> sub_9F920A (dsor/combat.py decode_target_points).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShiftedSkill {
    pub base: SkillBase,
    pub points: Vec<[f32; 3]>,
    pub server: Option<SkillServerData>,
}

impl Body for ShiftedSkill {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { base: SkillBase::decode(r)?, points: points(r)?, server: None })
    }
    fn encode(&self, w: &mut BitWriter) {
        self.base.encode(w);
        write_points(w, &self.points);
    }
}

/// SustainedSkillCommand (79), client -> server: a channel start.
/// SkillBase, then u16 / f32 / u32 repeating the skill index, heading and tick
/// (writer 0xA5C57C, reader 0xA5C470), then what the server reads as the command's
/// actor slot (actor, end tick, first-hit delay, interval, ... per
/// dsor/combat.py decode_sustained_skill, slot 8 0xA5C5EB).
/// UNKNOWN: the slot's full layout (the server's offsets and its listed field order
///   disagree, and no 79 has been captured), so everything after the 26-byte body is
///   kept verbatim in `actor_slot`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClientSustainedSkill {
    pub base: SkillBase,
    pub skill_id_again: u16,
    pub heading_again: f32,
    pub start_tick_again: u32,
    /// Opaque: the actor slot as sent (layout not established).
    pub actor_slot: Raw,
}

impl ClientSustainedSkill {
    /// The caster, the first u32 of the slot (dsor/combat.py SUSTAINED_ACTOR_OFFSET).
    pub fn actor(&self) -> Option<u32> {
        let d = &self.actor_slot.data;
        (self.actor_slot.bits >= 32).then(|| u32::from_le_bytes([d[0], d[1], d[2], d[3]]))
    }
}

impl Body for ClientSustainedSkill {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let base = SkillBase::decode(r)?;
        let skill_id_again = r.u16()?;
        let heading_again = r.f32()?;
        let start_tick_again = r.u32()?;
        let n = r.remaining();
        Ok(Self { base, skill_id_again, heading_again, start_tick_again, actor_slot: r.raw(n)? })
    }
    fn encode(&self, w: &mut BitWriter) {
        self.base.encode(w);
        w.u16(self.skill_id_again);
        w.f32(self.heading_again);
        w.u32(self.start_tick_again);
        self.actor_slot.write(w);
    }
}

/// SkillStopCommand (80), server -> client: end a sustained skill.
/// EVIDENCE: writer 0xA34156 / reader 0xA3410A; handler sub_5E35D3 finds the skill by
///   owner, start tick and wire index (dsor/combat.py encode_skill_stop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SkillStop {
    /// The skill's wire index.
    pub skill_id: u16,
    /// The tick the channel started on (79's start tick).
    pub start_tick: u32,
}

impl Body for SkillStop {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { skill_id: r.u16()?, start_tick: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u16(self.skill_id);
        w.u32(self.start_tick);
    }
}

impl ServerCommand {
    /// Read what a command's slot 9 holds past the 32-bit actor (the skill commands'
    /// server data). Called by the decoder right after the actor.
    pub(crate) fn decode_actor_slot_tail(&mut self, r: &mut BitReader<'_>) -> Result<(), DecodeError> {
        match self {
            Self::Skill(c) => c.server = Some(SkillServerData::decode(r, false)?),
            Self::TargetSkill(c) => c.server = Some(SkillServerData::decode(r, false)?),
            Self::BulletSkill(c) => c.server = Some(SkillServerData::decode(r, false)?),
            Self::TargetPointBulletSkill(c) => c.server = Some(SkillServerData::decode(r, true)?),
            Self::ShiftedSkill(c) => c.server = Some(SkillServerData::decode(r, true)?),
            _ => {}
        }
        Ok(())
    }

    /// Write what follows the actor in slot 9 (mirror of decode_actor_slot_tail). A
    /// skill command without server data gets the defaults.
    pub(crate) fn encode_actor_slot_tail(&self, w: &mut BitWriter) {
        let (server, shifted) = match self {
            Self::Skill(c) => (c.server, false),
            Self::TargetSkill(c) => (c.server, false),
            Self::BulletSkill(c) => (c.server, false),
            Self::TargetPointBulletSkill(c) => (c.server, true),
            Self::ShiftedSkill(c) => (c.server, true),
            _ => return,
        };
        let mut s = server.unwrap_or_default();
        if shifted && s.shift.is_none() {
            s.shift = Some(0);
        }
        if !shifted {
            s.shift = None;
        }
        s.encode(w);
    }
}

// ── blows, deaths, revives ──────────────────────────────────────────────────

/// HitCommand (115): one blow and the victim's health after it (the actor is the
/// victim). EVIDENCE: the 2018 reader 0x9A788A (vtable 0xFEE2A8 slot 5) -- tick, damage
/// types, two bits (+40 blocked, +41 critical), seven int32, a float, one bit; handler
/// 0x516453 (dsor/combat.py encode_hit).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hit {
    pub tick: u32,
    pub damage_types: Vec<u8>,
    pub blocked: bool,
    pub critical: bool,
    /// +44, +48: SetHitPoints / SetMaxHitPoints (whole numbers).
    pub victim_health: i32,
    pub victim_max_health: i32,
    pub damage: i32,
    /// 3 in every real hit observed.
    pub kind: i32,
    pub attacker: u32,
    /// Whose blow the floating number belongs to (the attacker).
    pub combat_value_owner: u32,
    pub heavy_until_tick: u32,
    pub combat_value: f32,
    /// +76.
    pub immune: bool,
}

impl Body for Hit {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            tick: r.u32()?,
            damage_types: damage_types(r)?,
            blocked: r.bit()?,
            critical: r.bit()?,
            victim_health: r.i32()?,
            victim_max_health: r.i32()?,
            damage: r.i32()?,
            kind: r.i32()?,
            attacker: r.u32()?,
            combat_value_owner: r.u32()?,
            heavy_until_tick: r.u32()?,
            combat_value: r.f32()?,
            immune: r.bit()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.tick);
        write_damage_types(w, &self.damage_types);
        w.bit(self.blocked);
        w.bit(self.critical);
        w.i32(self.victim_health);
        w.i32(self.victim_max_health);
        w.i32(self.damage);
        w.i32(self.kind);
        w.u32(self.attacker);
        w.u32(self.combat_value_owner);
        w.u32(self.heavy_until_tick);
        w.f32(self.combat_value);
        w.bit(self.immune);
    }
}

/// KillCommand (116): what makes an actor die (the actor is the victim).
/// EVIDENCE: dsor/combat.py encode_kill; handler HandleKillCommand 0x51787E matches
///   kill_tick / bullet_index against the killer's bullet skill.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Kill {
    pub tick: u32,
    pub damage_types: Vec<u8>,
    /// +0x48 / +0x4C: meaning not traced (the server sends 75 and 0).
    pub unknown_u32_0: u32,
    pub unknown_u32_1: u32,
    pub killer: u32,
    /// Where the victim dies, world units.
    pub position: [f32; 3],
    /// +0x50: the killing skill's start tick.
    pub kill_tick: u32,
    /// +0x54: the killing skill's bullet index, 0xFFFFFFFF for none.
    pub bullet_index: u32,
    /// Whether the body is removed (consumer not traced).
    pub despawn: bool,
}

impl Body for Kill {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            tick: r.u32()?,
            damage_types: damage_types(r)?,
            unknown_u32_0: r.u32()?,
            unknown_u32_1: r.u32()?,
            killer: r.u32()?,
            position: r.vec3()?,
            kill_tick: r.u32()?,
            bullet_index: r.u32()?,
            despawn: r.bit()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.tick);
        write_damage_types(w, &self.damage_types);
        w.u32(self.unknown_u32_0);
        w.u32(self.unknown_u32_1);
        w.u32(self.killer);
        w.vec3(self.position);
        w.u32(self.kill_tick);
        w.u32(self.bullet_index);
        w.bit(self.despawn);
    }
}

/// ReviveCommand (118): stand a dead actor up. One u32 (+0x10): a cooldown, 0 for none.
/// EVIDENCE: reader 0x9A9E69; handler 0x519D15 (dsor/combat.py encode_revive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Revive {
    pub cooldown: u32,
}

impl Body for Revive {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { cooldown: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.cooldown)
    }
}

/// ResurrectCommand (119), both directions: u32 +0x10, u32 +0x14, u8 state.
/// State: 0 cancel, 1 request, 2 accept, 3 decline. Server -> client: +0x10 is the
/// receiving (dead) player, the actor the originator. Client -> server: a request names
/// the target at +0x10; the dialog's answer names itself at +0x10 and the originator at
/// +0x14. EVIDENCE: reader 0x9A9E08, handler 0x519BFF (dsor/combat.py encode_resurrect,
/// decode_resurrect).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Resurrect {
    pub target: u32,
    pub originator: u32,
    pub state: u8,
}

impl Body for Resurrect {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { target: r.u32()?, originator: r.u32()?, state: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.target);
        w.u32(self.originator);
        w.u8(self.state);
    }
}

super::movement::empty_body! {
    /// ShowDeathDialogCommand (138): open the local player's death dialog. Empty body.
    /// EVIDENCE: reader 0x9A9F8B, handler 0x519FA6 (dsor/combat.py encode_show_death_dialog).
    ShowDeathDialog;
}

/// RespawnCommand (106), client -> server: two bits, +0x10 then +0x11.
/// (1, 0) = at the map's respawn point (paid), (0, 1) = instant (paid), (0, 0) free.
/// EVIDENCE: reader 0x9A9DBE / writer 0x9AD907; DeathDialogWidget::HandleEvent 0x655CEC
///   (dsor/combat.py decode_respawn). Not yet captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Respawn {
    pub at_respawn_point: bool,
    pub instant: bool,
}

impl Body for Respawn {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { at_respawn_point: r.bit()?, instant: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.bit(self.at_respawn_point);
        w.bit(self.instant);
    }
}

/// QuickResurrectGroupMemberCommand (297), client -> server: the dead mate's actor.
/// EVIDENCE: writer 0x9AD52D, reader 0x9A992D (server.py _quick_resurrect).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QuickResurrectGroupMember {
    pub target: u32,
}

impl Body for QuickResurrectGroupMember {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { target: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.target)
    }
}

// ── status effects ──────────────────────────────────────────────────────────

/// The tail of a status-effect element, present when its second flag is set.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatusEffectTail {
    /// The effect's parameters (u32 count, then float32s).
    pub parameters: Vec<f32>,
    pub more: bool,
    /// -1 with `more` clear means no vectors.
    pub byte: i8,
    /// Present when `more` or byte != -1: six floats when byte != 0, seven when 0.
    pub vectors: Option<Vec<f32>>,
}

/// One effect on one actor. EVIDENCE: element reader 0xA37C75 (two flag bits; the
/// second says the tail follows) per dsor/statuseffect.py.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatusEffectElement {
    /// The effect's _Template_StatusEffect row minus one.
    pub index: u16,
    /// Field 0: the instance handle the client keys "add or extend" on.
    pub instance: u32,
    /// Fields 1 and 3: end and start ticks (25 per second).
    pub end: u32,
    /// Field 2: per effect (0/25/50/75), meaning not established.
    pub second: u32,
    pub start: u32,
    /// Field 4: zero almost always.
    pub spare: u32,
    /// Field 5: duration in ticks (end - start).
    pub duration: u32,
    /// Field 6: per effect (1/2/3/5/100), meaning not established.
    pub sixth: u32,
    /// Field 7: the actor holding the effect.
    pub holder: u32,
    pub flag_0: bool,
    pub tail: Option<StatusEffectTail>,
}

/// StatusEffectCommand (82): the effects running on an actor.
/// A header bit, a u32 count, then the elements (dsor/statuseffect.py encode/decode).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatusEffect {
    pub header: bool,
    pub elements: Vec<StatusEffectElement>,
}

impl Body for StatusEffect {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let header = r.bit()?;
        let elements = counted(r, MOST, "status effect count", |r| {
            let mut e = StatusEffectElement {
                index: r.u16()?,
                instance: r.u32()?,
                end: r.u32()?,
                second: r.u32()?,
                start: r.u32()?,
                spare: r.u32()?,
                duration: r.u32()?,
                sixth: r.u32()?,
                holder: r.u32()?,
                flag_0: r.bit()?,
                tail: None,
            };
            if r.bit()? {
                let parameters = f32_list(r, MOST, "status effect parameter count")?;
                let more = r.bit()?;
                let byte = r.i8()?;
                let vectors = if more || byte != -1 {
                    let n = if byte != 0 { 6 } else { 7 };
                    Some(list(r, n, |r| r.f32())?)
                } else {
                    None
                };
                e.tail = Some(StatusEffectTail { parameters, more, byte, vectors });
            }
            Ok(e)
        })?;
        Ok(Self { header, elements })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.bit(self.header);
        w.count(self.elements.len());
        for e in &self.elements {
            w.u16(e.index);
            for v in [e.instance, e.end, e.second, e.start, e.spare, e.duration, e.sixth, e.holder] {
                w.u32(v);
            }
            w.bit(e.flag_0);
            w.bit(e.tail.is_some());
            if let Some(t) = &e.tail {
                write_f32_list(w, &t.parameters);
                w.bit(t.more);
                w.i8(t.byte);
                if t.more || t.byte != -1 {
                    let n = if t.byte != 0 { 6 } else { 7 };
                    let v = t.vectors.clone().unwrap_or_default();
                    for i in 0..n {
                        w.f32(v.get(i).copied().unwrap_or(0.0));
                    }
                }
            }
        }
    }
}

// ── location effects ────────────────────────────────────────────────────────

/// NewLocationEffectCommand (64): an effect placed on the ground. One entry.
/// EVIDENCE: reader sub_A63FBB / writer sub_A641EF; handler 0x5F5B76
///   (dsor/location.py, field offsets there).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewLocationEffect {
    /// +0: the id the discard looks up (index & 0xFFFF into a 1000-slot array).
    pub effect_id: u32,
    /// +4: the entity class to factor ("" = SphereEffect).
    pub entity_class: String,
    /// +8: u16 length + bytes (sub_C4A6DB).
    pub guid: Vec<u8>,
    pub unknown_bit_24: bool,
    /// +28: non-zero makes the shape round (no depth on the wire).
    pub kind: u8,
    pub position: [f32; 3],
    pub heading: f32,
    pub radius: f32,
    pub height: f32,
    /// Only on the wire when kind == 0.
    pub depth: Option<f32>,
    /// +96: the status effect to run.
    pub status_effect: String,
    pub unknown_bit_100: bool,
    /// +101: the effect has no end tick.
    pub endless: bool,
    pub end_tick: u32,
    pub unknown_u32_108: u32,
    pub start_tick: u32,
    pub parameters: Vec<f32>,
    /// 1..=55 (the client asserts, 0x9B85A2).
    pub causer_level: i32,
    pub causer: u32,
    /// The block at +132 (sub_A4C4E8): u32, u32, i8 in -1..=3, bit, bit.
    pub block_u32_0: u32,
    pub block_u32_1: u32,
    pub block_i8_2: i8,
    pub block_bit_3: bool,
    pub block_bit_4: bool,
    /// +148: read, not used by the handler.
    pub unknown_u32_148: u32,
}

impl Body for NewLocationEffect {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let effect_id = r.u32()?;
        let entity_class = r.string()?;
        let n = r.u16()? as usize;
        let guid = r.read_bytes(n)?;
        let unknown_bit_24 = r.bit()?;
        let kind = r.u8()?;
        let position = r.vec3()?;
        let heading = r.f32()?;
        let radius = r.f32()?;
        let height = r.f32()?;
        let depth = if kind == 0 { Some(r.f32()?) } else { None };
        let status_effect = r.string()?;
        let unknown_bit_100 = r.bit()?;
        let endless = r.bit()?;
        let end_tick = r.u32()?;
        let unknown_u32_108 = r.u32()?;
        let start_tick = r.u32()?;
        let parameters = f32_list(r, MOST, "location effect parameter count")?;
        let causer_level = r.i32()?;
        let causer = r.u32()?;
        let block_u32_0 = r.u32()?;
        let block_u32_1 = r.u32()?;
        let block_i8_2 = r.i8()?;
        if !(-1..=3).contains(&block_i8_2) {
            return Err(DecodeError::Invalid { what: "location effect block byte", value: block_i8_2 as i64 });
        }
        Ok(Self {
            effect_id,
            entity_class,
            guid,
            unknown_bit_24,
            kind,
            position,
            heading,
            radius,
            height,
            depth,
            status_effect,
            unknown_bit_100,
            endless,
            end_tick,
            unknown_u32_108,
            start_tick,
            parameters,
            causer_level,
            causer,
            block_u32_0,
            block_u32_1,
            block_i8_2,
            block_bit_3: r.bit()?,
            block_bit_4: r.bit()?,
            unknown_u32_148: r.u32()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.effect_id);
        w.string(&self.entity_class);
        w.u16(self.guid.len() as u16);
        w.write_bytes(&self.guid);
        w.bit(self.unknown_bit_24);
        w.u8(self.kind);
        w.vec3(self.position);
        w.f32(self.heading);
        w.f32(self.radius);
        w.f32(self.height);
        if self.kind == 0 {
            w.f32(self.depth.unwrap_or(1.0));
        }
        w.string(&self.status_effect);
        w.bit(self.unknown_bit_100);
        w.bit(self.endless);
        w.u32(self.end_tick);
        w.u32(self.unknown_u32_108);
        w.u32(self.start_tick);
        write_f32_list(w, &self.parameters);
        w.i32(self.causer_level);
        w.u32(self.causer);
        w.u32(self.block_u32_0);
        w.u32(self.block_u32_1);
        w.i8(self.block_i8_2);
        w.bit(self.block_bit_3);
        w.bit(self.block_bit_4);
        w.u32(self.unknown_u32_148);
    }
}

/// DiscardLocationEffectCommand (65): free a ground effect's entity and id slot.
/// EVIDENCE: reader 0xA390E1 (one u32); handler 0x5F560E (dsor/location.py encode_discard).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiscardLocationEffect {
    pub effect_id: u32,
}

impl Body for DiscardLocationEffect {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { effect_id: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.effect_id)
    }
}

// ── traps ───────────────────────────────────────────────────────────────────

/// NewTrapCommand (67): a trap placed by its owner (the actor); the client takes
/// radius, lifetime and release effect from its own _Template_Trap.
/// EVIDENCE: vtable 0x10386E0 slot 5 0xA3919E -> sub_A63CDC; encoder 0xA63E3A
///   (dsor/traps.py _body).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewTrap {
    pub trap_id: u32,
    /// The _Template_Trap row name.
    pub template: String,
    pub unknown_u32_0: u32,
    pub unknown_u32_1: u32,
    /// +0x10: non-zero means no fourth float.
    pub kind: u8,
    pub position: [f32; 3],
    /// Radians.
    pub heading: f32,
    pub scale_0: f32,
    pub scale_1: f32,
    /// Only on the wire when kind == 0.
    pub scale_2: Option<f32>,
}

impl Body for NewTrap {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let trap_id = r.u32()?;
        let template = r.string()?;
        let unknown_u32_0 = r.u32()?;
        let unknown_u32_1 = r.u32()?;
        let kind = r.u8()?;
        let position = r.vec3()?;
        let heading = r.f32()?;
        let scale_0 = r.f32()?;
        let scale_1 = r.f32()?;
        let scale_2 = if kind == 0 { Some(r.f32()?) } else { None };
        Ok(Self { trap_id, template, unknown_u32_0, unknown_u32_1, kind, position, heading, scale_0, scale_1, scale_2 })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.trap_id);
        w.string(&self.template);
        w.u32(self.unknown_u32_0);
        w.u32(self.unknown_u32_1);
        w.u8(self.kind);
        w.vec3(self.position);
        w.f32(self.heading);
        w.f32(self.scale_0);
        w.f32(self.scale_1);
        if self.kind == 0 {
            w.f32(self.scale_2.unwrap_or(1.0));
        }
    }
}

/// DiscardTrapCommand (68): take the trap off the map. One u32, the trap id.
/// EVIDENCE: vtable 0x1038760 slot 5 0xA39110; handler 0x5F6B2D (dsor/traps.py discarded).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiscardTrap {
    pub trap_id: u32,
}

impl Body for DiscardTrap {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { trap_id: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.trap_id)
    }
}

/// TrapTrippedCommand (70): the trap with this id tripped. One u32.
/// EVIDENCE: vtable 0x10387A0 slot 5 0xA39291; handler 0x5F71BA (dsor/traps.py tripped).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrapTripped {
    pub trap_id: u32,
}

impl Body for TrapTripped {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { trap_id: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.trap_id)
    }
}
