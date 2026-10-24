//! Skills: casting them and drawing them as the 2018 client does.
//!
//! Data (tools/export_skills.py, from the client's own files):
//! - `skills/skills.json`: `_Template_Skill` by wire index (row - 1) with its bullet
//!   and its sequences per weapon kind (data/tables/attacksequence.xml);
//! - `skills/sequences.json`: the sequencer documents (sequences/*.pbxml), 25 frames
//!   a second: the caster's animation, effect models (at the actor or on a joint),
//!   point lights, camera shakes, the actor hidden.
//!
//! Casting (the local player): Shift + left button = quick slot 0, right button =
//! slot 1, keys 1-5 = slots 2-6, aimed at the cursor. The command class follows the
//! skill's SkillType, as the real client sends it (experimental server.log, "used
//! ... via ...", 1 400+ casts): Melee -> Skill (73), MeleeTarget -> TargetSkill (74),
//! Ranged -> BulletSkill (75), RangedTargetPoint -> TargetPointBulletSkill (76),
//! Shifted / Teleport / Jump -> ShiftedSkill (77).
//! The aim is the command's radians: the movement facing less half a turn
//! (dsor/combat.py decode_skill_use: 277 real commands, offset 128 of 256).
//! UNVERIFIED: the slot -> button mapping (ui/bottombar.bxml has LMB, RMB and
//! QuickSlot1-7 labelled 1-7); bullets leave at LoopStartFrame; the bullet's step
//! vector length (the server reads only its direction).

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::gltf::Gltf;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::world_serialization::WorldAssetRoot;
use dsor_proto::commands::combat::{self, SkillBase};
use dsor_proto::commands::ClientCommand;
use serde::Deserialize;

use crate::actors::character::{AnimState, Character, CharacterAnim};
use crate::world::nav::{CurrentNav, NavMesh};
use crate::network::net::{cursor_ground, now_ms, LocalPlayer, Net, RemotePlayer};

/// The sequencer's frame rate.
const FPS: f32 = 25.0;
/// A Nebula entity looks down its -Z; our character models look down +Z (players
/// turn with from_rotation_y(atan2(dx, dz))). Everything a sequence places "on the
/// entity" (effects, lights, bullet start offsets) is in the entity's frame.
/// EVIDENCE: mage_fireball_bullet's StartOffset is (0.043, 1.49, -1.788): the hand,
///   1.8 units AHEAD, on -Z; the skill command's aim is the facing less half a turn
///   (dsor/combat.py); the NPCs' level matrices face -Z (crate::actors::npc).
/// FAILURE (2026-10-06): without it the fireball's trail streamed out in front of
///   it and the cast effects sat behind the caster ("pas affiche au bon endroit").
fn entity_rotation(facing: f32) -> Quat {
    Quat::from_rotation_y(facing + std::f32::consts::PI)
}

#[derive(Deserialize, Debug, Clone)]
pub struct BulletDef {
    #[serde(rename = "loop")]
    pub loop_seq: String,
    /// BulletImpactSequence: on each actor a punch-through bullet passes
    /// (`impact_count` above 1), and where a charge stops (carry_casters).
    pub impact: String,
    pub death: String,
    /// Authored flight mode; homing modes retain their target throughout flight.
    pub motion: String,
    pub velocity: f32,
    pub lifetime: f32,
    /// BulletRadius: the hit test's, against the victim's capsule radius (fly_bullets).
    pub radius: f32,
    /// Where it leaves, in the caster's entity frame.
    pub start: [f32; 3],
    /// StaticCollision: walls stop it.
    #[serde(default)]
    pub static_collision: bool,
    /// ImpactCount: how many actors it strikes before it ends (1 for most, 2 for
    /// iceball, 20 for seismic slam's wave).
    #[serde(default = "one")]
    pub impact_count: u32,
}

fn one() -> u32 {
    1
}

#[derive(Deserialize, Debug, Clone)]
pub struct Summon {
    pub id: String,
    pub amount: u32,
    /// From the caster, in its frame (x right, z ahead).
    pub offset: [f32; 3],
    /// Frames after the hit frame.
    pub delay: u32,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SkillDef {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// _Template_Skill.Targeting: Actor, Radius, Angle, Cone or User.
    #[serde(default)]
    pub targeting: String,
    #[serde(default = "scale_timing_default")]
    pub scale_timing: bool,
    pub hit_frame: u32,
    pub loop_start: u32,
    pub unblock: u32,
    pub motion_unblock: u32,
    pub range: f32,
    pub cooldown: f32,
    #[serde(default)]
    pub cooldown_category: String,
    /// The class that has it (CharClass: "mage", ...), empty for monsters.
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub resource_cost: f32,
    #[serde(default)] pub ammo_cost: u32,
    pub execute: HashMap<String, String>,
    pub impact: String,
    /// Shifted skills: the sequence drawn on the aimed point (SkillBulletId).
    #[serde(default)]
    pub shifted: String,
    /// UserStatusEffects: on the caster (the shouts' auras), (status id, seconds).
    #[serde(default)]
    pub user_status: Vec<(String, f32)>,
    /// VictimStatusEffects / LocationStatusEffects: (status id, seconds).
    #[serde(default)]
    pub victim_status: Vec<(String, f32)>,
    #[serde(default)]
    pub location_status: Vec<(String, f32)>,
    /// Location effects that start late (DE:): status id -> seconds.
    #[serde(default)]
    pub location_delay: HashMap<String, f32>,
    /// BulletCount bullets, BulletEmitDelay seconds apart (turned into whole ticks
    /// by SecondsToGameTicks, 0x4AED10).
    #[serde(default)]
    pub bullet_count: u32,
    #[serde(default)]
    pub emit_delay: f32,
    /// What it summons (offline play spawns it; online the server does).
    #[serde(default)]
    pub summon: Option<Summon>,
    pub bullet: Option<BulletDef>,
}

fn scale_timing_default() -> bool { true }

impl SkillDef {
    /// The execute sequence for a weapon kind (an attacksequence.xml column).
    fn execute_for(&self, column: &str) -> Option<&String> {
        self.execute
            .get(column)
            .or_else(|| self.execute.get("*"))
            .or_else(|| self.execute.get("1h_weapon"))
            .or_else(|| self.execute.values().next())
    }
}

#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct SkillTable(pub HashMap<u32, SkillDef>);

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Track {
    Anim { name: String, speed: f32, start: i32, end: i32 },
    Fx {
        graphics: String,
        #[serde(default)]
        joint: String,
        at: [f32; 9],
        start: i32,
        end: i32,
        #[serde(default)]
        curves: HashMap<String, Vec<[f32; 8]>>,
    },
    Phase { start: i32, end: i32 },
    Light {
        color: [f32; 3],
        intensity: f32,
        range: f32,
        at: [f32; 9],
        start: i32,
        end: i32,
        #[serde(default)]
        curves: HashMap<String, Vec<[f32; 8]>>,
    },
    Shake { intensity: f32, range: f32, start: i32, end: i32 },
    Hide { start: i32, end: i32 },
    Sound { #[serde(default)] name: String, #[serde(default = "sound_volume")] volume: f32, start: i32, end: i32 },
    /// CharacterTrackBar: a character a level's sequence places (guards, pigeons),
    /// drawn by crate::actors::npc from maps/<map>.deco.json; nothing for the player.
    Character { start: i32, end: i32 },
    /// ColorShaderParameterTrackBar: a shader colour of the actor (HitColor, the
    /// flash of a hit) while it runs, its alpha x the intensity curve; `default`
    /// once it ends.
    Color {
        param: String,
        color: [f32; 4],
        default: [f32; 4],
        intensity: f32,
        start: i32,
        end: i32,
        #[serde(default)]
        curves: HashMap<String, Vec<[f32; 8]>>,
    },
}

fn sound_volume() -> f32 { 100. }

impl Track {
    fn span(&self) -> (i32, i32) {
        match self {
            Track::Anim { start, end, .. }
            | Track::Fx { start, end, .. }
            | Track::Phase { start, end }
            | Track::Light { start, end, .. }
            | Track::Shake { start, end, .. }
            | Track::Hide { start, end }
            | Track::Sound { start, end, .. }
            | Track::Character { start, end }
            | Track::Color { start, end, .. } => (*start, *end),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Sequence {
    pub length: u32,
    /// The sequencer's own "repeat" (GlobalParameter): loops for as long as its
    /// owner lasts (stun_loop, burn_loop...).
    #[serde(default)]
    pub repeat: bool,
    pub tracks: Vec<Track>,
}

#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct SequenceTable(pub HashMap<String, Sequence>);

macro_rules! json_loader {
    ($loader:ident, $asset:ty, $ext:literal) => {
        #[derive(Default, TypePath)]
        pub struct $loader;
        impl AssetLoader for $loader {
            type Asset = $asset;
            type Settings = ();
            type Error = std::io::Error;
            async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<$asset, Self::Error> {
                let mut b = Vec::new();
                r.read_to_end(&mut b).await?;
                serde_json::from_slice(&b).map_err(std::io::Error::other)
            }
            fn extensions(&self) -> &[&str] {
                &[$ext]
            }
        }
    };
}
/// _Template_StatusEffect by wire index (tools/export_skills.py).
#[derive(Deserialize, Debug, Clone)]
pub struct StatusDef {
    pub id: String,
    pub duration: f32,
    /// StartAnimation: the holder's animation state while it lasts ("Resurrect").
    #[serde(default)]
    pub animation: String,
    pub start: HashMap<String, String>,
    pub tick: HashMap<String, String>,
    pub done: HashMap<String, String>,
    pub stop: HashMap<String, String>,
    /// A ride effect's mount (_Template_Mount id; crate::actors::mounts).
    #[serde(default)]
    pub mount: String,
    /// Its MovementBonus: added to the run speed while it lasts.
    #[serde(default)]
    pub movement_bonus: f32,
    /// Its StartModifiers switch the holder's movement off ("ActorFeature:off,
    /// Movement,RegularSkills"; tools/export_skills.py holds_movement): held
    /// in place while it lasts, as the server holds the player's moves.
    /// FAILURE: debuff_cc_petrify has no StartAnimation, so a petrified player
    /// walked on and the server snapped them back every 0.3 s.
    #[serde(default)]
    pub holds: bool,
}

impl StatusDef {
    fn first(m: &HashMap<String, String>) -> Option<&String> {
        m.get("*").or_else(|| m.get("1h_weapon")).or_else(|| m.values().next())
    }
}

#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct StatusTable(pub HashMap<u32, StatusDef>);

json_loader!(SkillTableLoader, SkillTable, "skills.json");
json_loader!(StatusTableLoader, StatusTable, "status_effects.json");
json_loader!(SequenceTableLoader, SequenceTable, "sequences.json");

#[derive(Resource)]
pub struct SkillData {
    skills: Handle<SkillTable>,
    status: Handle<StatusTable>,
    sequences: Handle<SequenceTable>,
    /// Skill id -> wire index, built once the table is in.
    by_id: HashMap<String, u32>,
}

impl SkillData {
    #[cfg(test)]
    pub(crate) fn for_test(sequences: Handle<SequenceTable>) -> Self {
        Self { skills: default(), status: default(), sequences, by_id: default() }
    }

    /// Every sequence of the sequencer's data (sequences.json).
    pub(crate) fn sequences(&self) -> &Handle<SequenceTable> {
        &self.sequences
    }
    /// The status effects by wire index (status_effects.json).
    pub(crate) fn status(&self) -> &Handle<StatusTable> {
        &self.status
    }
}

/// An actor a sequence's hide track is hiding (a dead one for good). Distance
/// culling (crate::actors::npc::cull_actors) leaves its visibility alone.
/// FAILURE (2026-10-16): cull_actors showed the Bony Gnasher again the frame after
///   its death sequence hid it (tick 75); hidden, its animation had been stopped,
///   so its Death clip started over ("2x l'anim de mort").
#[derive(Component)]
pub struct SequenceHidden;

/// One sequence playing: on an actor (its animation, effects that follow it) or on
/// its own anchor entity (a bullet, a point on the ground).
#[derive(Component)]
pub struct SequencePlayer {
    seq: Sequence,
    /// A death's own sequence (monsters::play_death): the only one that may still
    /// change a dead actor's animation.
    on_death: bool,
    /// Seconds since it started.
    t: f32,
    /// The character it animates and hides, if any.
    actor: Option<Entity>,
    /// What its effects and lights are attached to.
    anchor: Entity,
    /// Bullets loop their sequence for as long as they fly.
    looping: bool,
    /// Effects follow the anchor (status effects on an actor, bullets, ground
    /// effects). A skill's own sequence on its caster does not: the 2018
    /// client's DrasaGraphicsObjectTrackBar has no attachment (only
    /// AttachedGraphicsTrackBar::AddAttachment binds to the host), so those
    /// effects stay where they were spawned while the caster walks on.
    follow: bool,
    /// Set to end the sequence now (its effects retire gracefully).
    pub stop: bool,
    /// Movement or a newer skill took over the actor's animation: this sequence
    /// neither starts nor ends an animation any more, but its effects play out.
    superseded: bool,
    /// Per track: started, and what it spawned.
    started: Vec<bool>,
    spawned: Vec<Option<Entity>>,
    /// Per track: what its local placement is multiplied by (the world placement
    /// it was left at, or the character's entity frame), for animated placements.
    bases: Vec<Mat4>,
    /// Frames per frame of time: 1 but for a stretched sequence (a Shifted skill's
    /// falls over its HitFrame ticks, BulletSkillVisualizer 0x5ECA7A / 0x5EC837).
    rate: f32,
}

impl SequencePlayer {
    pub(crate) fn has_pending_animation(&self) -> bool {
        !self.stop && !self.superseded && self.seq.tracks.iter().enumerate().any(|(i, t)| {
            matches!(t, Track::Anim { end, .. } if !self.started[i] && self.t * FPS < *end as f32)
        })
    }

    /// Release even effects detached from the actor's hierarchy by a skill.
    pub(crate) fn despawn(&self, commands: &mut Commands, entity: Entity) {
        for &effect in self.spawned.iter().flatten() {
            commands.entity(effect).try_despawn();
        }
        commands.entity(entity).try_despawn();
    }

    /// Stable actor/anchor roots whose resident rig this sequence may use.
    /// Keep a resident rig until cleanup even when `stop` has just been set.
    pub(crate) fn visual_actors(&self) -> impl Iterator<Item = Entity> {
        self.actor.into_iter().chain(std::iter::once(self.anchor))
    }
}

/// A bullet in flight.
#[derive(Component)]
/// EVIDENCE (2018 client, static): GameBulletSkill::ComputeStraightPosition 0x8EEB6E
///   (start + step x ticks, the stored end point from the end tick on), its end
///   point 0x8F2A6A (start + step x trunc(LifeTime x 25)), RangedTargetPoint
///   ending on the aimed point 0x949601, StaticCollision's wall test 0x8F21B7, the
///   end snapping to a hit actor 0x8ED6A1; the view chasing the tick position
///   ~67 ms behind (0x5E4DB1); its two endings (0x5E4DB1): DeathSequence when the
///   lifetime ran out (0x5E8515), else SkillImpactSequence (0x5E8981, 0x5EC436),
///   both at the end point lowered by the bullet's height, facing its flight.
struct Bullet {
    /// Where it started and its velocity (units / second).
    start: Vec3,
    velocity: Vec3,
    homing: bool,
    /// Seconds since it started, and when it ends.
    t: f32,
    end_t: f32,
    /// Original end time, restored if a server-confirmed target dies in flight.
    lifetime: f32,
    /// StartOffset.y: how far its end effects are lowered (they play on the ground).
    height: f32,
    death: String,
    /// The skill's SkillImpactSequence.
    impact: String,
    /// RangedTargetPoint: it ends on the aimed point, with its impact.
    to_point: bool,
    /// StaticCollision: the ground rising into it stops it (the walls the client
    /// tests are not in this client's data; the navigation mesh stands for them).
    static_collision: bool,
    radius: f32,
    /// Who shot it (monsters' bullets do not hit monsters).
    caster: Entity,
    /// Keep the collision side after the caster's corpse has been disposed: a
    /// hostile monster's (it strikes players), else a player's or an ally's.
    from_monster: bool,
    /// Offline play: the aimed monster, struck when the bullet reaches it.
    target: Option<u32>,
    /// Once it has hit or run out: seconds its particles still have to fade.
    dying: Option<f32>,
    /// Who shot it, as the server names attackers (HitCommand), if known.
    caster_actor: Option<u32>,
    /// Its BulletImpactSequence, on each actor a punch-through bullet passes.
    bullet_impact: String,
    /// Actors it may still strike before it ends (ImpactCount), and those it has
    /// passed through.
    impacts_left: u32,
    passed: Vec<u32>,
    /// The actor the server's blow landed on: it ends there (see fly_bullets).
    snapped: Option<u32>,
}

/// The view's lag behind the bullet's tick position (0x5E4DB1, ~67 ms).
const BULLET_LAG: f32 = 0.067;


/// A caster carried by its skill (Jump, Charge) along the ground from `from` to
/// `to`: a jump between its LoopStartFrame and LoopStartFrame + HitFrame (see
/// perform), a charge on its bullet's flight. A jump's height is its clip's: the
/// leap clips lift the Hips themselves (warrior_1h_skill_08 to 5.6 m, ranger
/// 3.2 m, the dwarf's 2.7-3.2 m) and touch down at frame 18.
/// FAILURE (2026-10-23): an added 1.8 m arc stacked on the clip's, and the flight
/// ended at HitFrame (frame 11), so the character and the camera reached the
/// landing point while the model still hung 5 m up ("the jump is still weird").
#[derive(Component)]
struct SkillMove {
    from: Vec3,
    to: Vec3,
    delay: f32,
    t: f32,
    duration: f32,
    /// A charge: the caster is its bullet (see perform).
    charge: Option<Charge>,
}

/// A ShiftedSkill's (Shifted, Jump) HitFrame after its release, seconds: scaled by
/// the attack speed, except a long falling impact (25 frames or more keeps its
/// authored length; DSOR_Server gameplay/combat_entities.py shifted_skill_phases).
fn shifted_hit(skill: &SkillDef, rate: f32) -> f32 {
    skill.hit_frame as f32 / FPS / if skill.hit_frame < 25 { rate } else { 1.0 }
}

/// What a charging caster draws: its bullet's loop sequence on it while it runs,
/// then the bullet's impact where it stops and the skill's impact on it.
struct Charge {
    looped: String,
    player: Option<Entity>,
    bullet_impact: String,
    impact: String,
}

/// Something to do a little later in a skill (a bullet leaving the hand, an impact).
#[derive(Component)]
struct Pending {
    after: f32,
    what: PendingKind,
}

enum PendingKind {
    /// `aim`: the aimed actor's chest, for the bullet to dip or climb to;
    /// `caster_actor`: the caster as the server names it, whose blows end it.
    Bullet { from: Vec3, dir: Vec3, def: BulletDef, impact: String, to: Option<Vec3>, caster: Entity, caster_actor: Option<u32>, aim: Option<Vec3>, target: Option<u32> },
    /// The caster lands on the aimed point (Teleport skills).
    Teleport { who: Entity, to: Vec3 },
    Sequence { name: String, at: Transform, rate: f32 },
}

/// An effect model: its node animation is started once it is in.
#[derive(Component)]
pub(crate) struct FxModel {
    gltf: Handle<Gltf>,
    started: bool,
    /// Its node animation repeats only in a looping sequence (a bullet in flight);
    /// otherwise it plays once. Repeated, frost nova's ring burst twice within its
    /// track and the teleport's arrival four or five times.
    looping: bool,
    /// Its sequence's rate (attack speed for a ScaleSkillTiming skill): its clip
    /// and emitters run at it, so a faster cast compresses the effect instead of
    /// cutting it off where its track ends.
    /// FAILURE: at 6 attacks a second the sequence ended its track while the
    ///   slash's clip and particles ran at 1x: the slash, the fireball's hand
    ///   burst, barely showed ("le FX du slash apparait plus", 2026-10-21).
    /// UNVERIFIED: that the 2018 client scales graphics-entity time by the
    ///   sequence rate (not traced); the alternative would end FX tracks on
    ///   unscaled time.
    speed: f32,
}

/// An effect whose track ended: its surfaces and lights go, its emitters stop and
/// its particles live out their lives, then it is despawned.
#[derive(Component)]
struct Retiring {
    left: f32,
    started: bool,
}

/// Camera shake left, applied on top of the follow camera.
#[derive(Resource, Default)]
pub struct CameraShake {
    trauma: f32,
}

pub struct SkillsPlugin;

/// The level editor plays authored effects without gameplay, networking or casts.
pub(crate) struct EditorSequencesPlugin;
impl Plugin for EditorSequencesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraShake>()
            .init_resource::<crate::actors::monsters::ShaderColors>()
            .add_systems(Update, (play_sequences, start_fx_animations, fx_particle_speed, retire_effects).chain());
    }
}

impl Plugin for SkillsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<SkillTable>()
            .init_resource::<SkillCooldowns>()
            .init_asset::<SequenceTable>()
            .init_asset::<StatusTable>()
            .register_asset_loader(StatusTableLoader)
            .register_asset_loader(SkillTableLoader)
            .register_asset_loader(SequenceTableLoader)
            .init_resource::<CameraShake>()
            .init_resource::<TestCastClock>()
            .init_resource::<StatusHolds>()
            .init_resource::<preload::FxPreload>()
            .add_systems(Startup, preload::setup)
            .add_systems(Update, preload::update.after(crate::network::net::NetSystems))
            .add_systems(Startup, load)
            .add_systems(
                Update,
                (index_skills, cast_input.run_if(crate::screens::loading::gameplay_ready), remote_skills, status_visuals, trap_visuals, hold_for_status, test_cast, run_pending, carry_casters, fly_bullets, play_sequences, start_fx_animations, fx_particle_speed, retire_effects, shake_camera, test_shot, test_view, debug_fx_positions)
                    .chain()
                    .after(crate::network::net::NetSystems).after(crate::actors::npc::NpcVisualSystems)
                    .before(crate::actors::character::drive_animations),
            );
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Update, vfx_preview.before(play_sequences).after(crate::network::net::NetSystems));
    }
}

pub(crate) mod preload;

#[cfg(test)]
mod regression_tests;

fn load(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(SkillData {
        skills: assets.load("skills/skills.json"),
        status: assets.load("skills/status_effects.json"),
        sequences: assets.load("skills/sequences.json"),
        by_id: HashMap::new(),
    });
}

fn index_skills(mut data: ResMut<SkillData>, tables: Res<Assets<SkillTable>>) {
    if !data.by_id.is_empty() {
        return;
    }
    if let Some(t) = tables.get(&data.skills) {
        data.by_id = t.0.iter().map(|(w, s)| (s.id.clone(), *w)).collect();
    }
}

/// The attacksequence.xml column for a character's weapons: the suffix of its
/// animation set ("mage_female_2h_weapon" -> "2h_weapon"), "empty" unarmed.
fn weapon_column(character: &Character) -> String {
    if character.desc.armament <= 0 && character.desc.look.is_none() {
        return "empty".into();
    }
    let set = character.desc.animation_set();
    for c in ["1h_weapon_shield", "1h_weapon", "2h_weapon", "small_weapon", "large_weapon"] {
        if set.ends_with(c) {
            return c.into();
        }
    }
    "empty".into()
}

/// Our facing (radians, atan2(dx, dz)) as a skill command's aim, and back.
/// EVIDENCE: dsor/combat.py decode_skill_use -- the command's radians are the
///   movement facing byte less 128 of 256.
fn facing_to_aim(facing: f32) -> f32 {
    let a = (facing - std::f32::consts::PI).rem_euclid(std::f32::consts::TAU);
    if a > std::f32::consts::PI { a - std::f32::consts::TAU } else { a }
}
fn aim_to_facing(aim: f32) -> f32 {
    aim + std::f32::consts::PI
}

/// Start everything a skill draws for the actor `actor` at `at`, facing `facing`,
/// aimed at `point` (an actor's feet, or the ground). `caster_actor` is the caster
/// as the server names it (its blows end its bullets); `aim` the aimed actor's
/// chest, for a bullet to dip or climb to (fly_bullets).
#[allow(clippy::too_many_arguments)]
fn perform(
    commands: &mut Commands,
    skill: &SkillDef,
    sequences: &SequenceTable,
    actor: Entity,
    character: Option<&Character>,
    at: Vec3,
    facing: f32,
    point: Vec3,
    caster_actor: Option<u32>,
    aim: Option<Vec3>,
    target: Option<u32>,
    rate: f32,
) {
    let rate = if skill.scale_timing { rate.clamp(0.1, 10.0) } else { 1.0 };
    if character.is_some_and(|c| c.desc.look.is_none()) {
        commands.queue(move |world: &mut World| {
            if let Some(mut anim) = world.get_mut::<CharacterAnim>(actor) { anim.enter_combat(); }
        });
    }
    let column = character.map(weapon_column).unwrap_or_else(|| "empty".into());
    if let Some(seq) = skill.execute_for(&column).and_then(|n| sequences.0.get(n)) {
        // A Sustained skill's execute sequence repeats (dwarf_tower_default_beam:
        // repeat, 101 frames, its beam fx over 0..101) for as long as the skill
        // runs; the server only renews it every 125 ticks (local_combat_state.py
        // SUSTAINED_SKILL_PLAYBACK_INTERVAL_SECONDS). Played once, the Machine
        // Gun Turret's beam went out for a second or more before each renewal
        // ("le FX s'arrete alors que l'animation continue", 2026-10-23). A
        // renewal while its loop still runs keeps that loop; the loop ends with
        // its actor, or when a newer skill or a walk takes the actor over.
        let looping = skill.kind == "Sustained" && seq.repeat;
        let length = seq.length;
        let entity = play(commands, seq.clone(), Some(actor), actor, looping);
        commands.queue(move |world: &mut World| {
            if looping {
                let mut players = world.query::<(Entity, &SequencePlayer)>();
                let running = players.iter(world).any(|(e, p)| {
                    e != entity && p.looping && !p.stop && !p.superseded && p.actor == Some(actor) && p.anchor == actor && p.seq.length == length
                });
                if running {
                    world.entity_mut(entity).despawn();
                    return;
                }
            }
            if let Some(mut sequence) = world.get_mut::<SequencePlayer>(entity) { sequence.rate = rate; }
        });
    }
    let dir = Quat::from_rotation_y(facing) * Vec3::Z;
    match (skill.kind.as_str(), &skill.bullet) {
        ("Shifted", _) if !skill.shifted.is_empty() => shifted_fall(commands, skill, sequences, point, facing, rate),
        ("Charge" | "ChargeThrough", Some(_)) => {}
        (_, Some(b)) => {
            // EVIDENCE: the start (0x9BFFDC): a look-at frame on the aim, StartOffset
            //   turned by it: x to the right of the aim, y up, -z ahead (every z in the
            //   data is negative: magic missile 1.71 ahead, 1.49 up), no clamp; the
            //   flight from start + LoopStartFrame (HitFrame unused for bullets);
            //   BulletCount bullets from the same place, BulletEmitDelay ticks apart
            //   (0x8F0ADA), Angle unused.
            // UNVERIFIED: own casts rescale that delay by a speed attribute (0x8F170D,
            //   attribute not identified).
            let o = Vec3::from(b.start);
            let right = Vec3::new(-dir.z, 0.0, dir.x);
            let from = at + right * o.x + Vec3::Y * o.y - dir * o.z;
            // The point's own ground height, so the grenade lands on it uphill too.
            let to = (skill.kind == "RangedTargetPoint").then(|| Vec3::new(point.x, point.y + o.y, point.z));
            // Not aimed at an actor, a bullet dips or climbs to the aimed point at the
            // hand's height: level on flat ground; shot from a cliff it flies down
            // and its StaticCollision stops it on the ground below.
            // EVIDENCE: the user's release261_192 recording (a mage on a Wild Forest
            //   cliff: the missiles go down to the lower ground and burst there).
            // FAILURE (2026-10-24): shot downhill, ours flew on level at the hand's
            //   height, over everything below.
            let aim = aim.or(Some(Vec3::new(point.x, point.y + o.y, point.z)));
            for k in 0..skill.bullet_count.max(1) {
                commands.spawn(Pending {
                    after: ((skill.loop_start as f32 / FPS).max(skill.emit_delay) + k as f32 * skill.emit_delay) / rate,
                    what: PendingKind::Bullet { from, dir, def: b.clone(), impact: skill.impact.clone(), to, caster: actor, caster_actor, aim, target },
                });
            }
        }
        _ => {}
    }
    if matches!(skill.kind.as_str(), "Jump" | "Charge" | "ChargeThrough") {
        let start = skill.loop_start as f32 / FPS / rate;
        // A jump lands on its hit tick, LoopStartFrame + HitFrame after the cast,
        // as a Shifted skill (the same ShiftedSkill command; 0x946F17): enraging
        // leap at frame 15 (its clip touches down at 18), ranger jump at 17.
        let end = if skill.kind == "Jump" { start + shifted_hit(skill, rate) } else { (skill.hit_frame.max(skill.loop_start + 1)) as f32 / FPS / rate };
        let mut duration = (end - start).max(0.05);
        // A charge's caster rides its bullet: placed on the bullet's straight
        // flight to the target point, its loop sequence on the caster (it holds
        // the run: warrior_stuncharge_bullet animates Skill03_loop), its impacts
        // where it stops (warrior_stuncharge_bimpact, then _impact's Skill03_impact).
        // EVIDENCE: ChargeSkillVisualizer::Trigger (0x5EAB08) asserts
        //   bulletCmd->HasTargetPosition() and places the player by
        //   GameBulletSkill::ComputeStraightPosition (0x8EEB6E): start + velocity x
        //   ticks. A free bullet with LifeTime 0 lived one tick and drew nothing
        //   ("assaut n'a pas de FX").
        // UNVERIFIED: the impacts' order and place; the speed of a bullet-less charge.
        let charge = match (skill.kind.as_str(), &skill.bullet) {
            ("Charge" | "ChargeThrough", Some(b)) => {
                if b.velocity > 0.0 {
                    duration = (Vec2::new(point.x - at.x, point.z - at.z).length() / b.velocity).max(0.05);
                }
                Some(Charge { looped: b.loop_seq.clone(), player: None, bullet_impact: b.impact.clone(), impact: skill.impact.clone() })
            }
            _ => None,
        };
        commands.entity(actor).insert(SkillMove {
            from: at,
            to: point,
            delay: start,
            t: 0.0,
            duration,
            charge,
        });
    }
    if skill.kind == "Teleport" {
        // On its hit tick after LoopStartFrame, as the ShiftedSkill it is sent as
        // (start + LoopStartFrame + HitFrame, 0x946F17): the mage's teleport moves
        // at frame 4, 0.16 s, after its short cast.
        // FAILURE (2026-10-24): it moved at HitFrame (0.04 s), "instant" next to the
        //   original's short cast (the user's release261_192 recording).
        // UNVERIFIED: the Shifted rule for Teleport skills.
        commands.spawn(Pending { after: skill.loop_start as f32 / FPS / rate + shifted_hit(skill, rate), what: PendingKind::Teleport { who: actor, to: point } });
    }
    if !skill.impact.is_empty() && skill.impact != "empty_sequence" {
        let impact_at = match skill.kind.as_str() {
            "Shifted" | "Teleport" | "Jump" => point,
            _ => at,
        };
        // Shifted and Jump: on its hit tick, start + LoopStartFrame + HitFrame
        // (0x946F17; lightning strike 44, meteor 65, enraging leap 15).
        let after = match skill.kind.as_str() {
            "Shifted" | "Jump" | "Teleport" => skill.loop_start as f32 / FPS / rate + shifted_hit(skill, rate),
            _ => skill.hit_frame as f32 / FPS / rate,
        };
        // A bullet's impact is drawn where it ends (fly_bullets), a charge's where
        // its caster stops (carry_casters); the others here.
        if skill.bullet.is_none() || skill.kind == "Shifted" {
            commands.spawn(Pending {
                after,
                what: PendingKind::Sequence {
                    name: skill.impact.clone(),
                    at: Transform::from_translation(impact_at).with_rotation(entity_rotation(facing)),
                    rate: 1.0,
                },
            });
        }
    }
}

/// What falls on one ground point of a Shifted skill (perform; every further
/// point of a multi-bullet one, remote_skills).
fn shifted_fall(commands: &mut Commands, skill: &SkillDef, sequences: &SequenceTable, point: Vec3, facing: f32, rate: f32) {
    // What falls on the aimed point (the lightning bolt, the meteor, the
    // singularity's start): SkillBulletId names a sequence for every Shifted
    // skill. EVIDENCE: BulletSkillVisualizer 0x5ECA7A / 0x5EC837: from
    // start + LoopStartFrame, at the target point turned to the aim heading,
    // stretched to last HitFrame ticks.
    // The stretch is the sequence's phase track, not its whole length: the
    // tail (meteor_bullet 50..100, lightningstrike_bullet 30..50) plays
    // after the impact at that same rate. EVIDENCE (release261):
    // ConnectWithTrackSequencer 0x14034cc84 passes numPhaseFrames (the
    // phase track's end - start, 0x14034d19f) to
    // ShiftedSkillVisualizer::CalculateSequenceFPS 0x14034a714, which
    // returns numPhaseFrames / (HitFrame * 0.04 s). FAILURE: the whole
    // length played meteor 1.2 s and lightning 0.56 s ahead of their damage.
    let impact_rate = if skill.hit_frame < 25 { rate } else { 1.0 };
    let visual_rate = sequences
        .0
        .get(&skill.shifted)
        .map(|q| {
            let frames = q
                .tracks
                .iter()
                .find_map(|t| match t {
                    Track::Phase { start, end } if end > start => Some((end - start) as f32),
                    _ => None,
                })
                .unwrap_or(q.length as f32);
            frames / skill.hit_frame.max(1) as f32 * impact_rate
        })
        .unwrap_or(1.0);
    commands.spawn(Pending {
        after: skill.loop_start as f32 / FPS / rate,
        what: PendingKind::Sequence { name: skill.shifted.clone(), at: Transform::from_translation(point).with_rotation(entity_rotation(facing)), rate: visual_rate },
    });
}

/// Shifted: its impact on its hit tick, start + LoopStartFrame + HitFrame
/// (0x946F17; lightning strike 44, meteor 65).
fn shifted_impact_after(skill: &SkillDef, rate: f32) -> f32 {
    skill.loop_start as f32 / FPS / rate + shifted_hit(skill, rate)
}

/// A multi-bullet Shifted skill's further ground points, after the first that
/// perform played: its fall and its impact on each. A point sent twice (the
/// server repeats them to fill BulletCount when fewer players are in reach)
/// plays once.
/// FAILURE: only the first point played; the Herald's and the Horseman's
/// other meteors hit unseen.
fn shifted_extra_points(commands: &mut Commands, skill: &SkillDef, sequences: &SequenceTable, points: &[[f32; 3]], facing: f32, rate: f32) {
    if skill.kind != "Shifted" || points.len() < 2 {
        return;
    }
    let rate = if skill.scale_timing { rate.clamp(0.1, 10.0) } else { 1.0 };
    let mut played: Vec<Vec3> = vec![Vec3::from(points[0])];
    for p in &points[1..] {
        let point = Vec3::from(*p);
        if played.iter().any(|q| q.distance_squared(point) < 1e-4) {
            continue;
        }
        played.push(point);
        if !skill.shifted.is_empty() {
            shifted_fall(commands, skill, sequences, point, facing, rate);
        }
        if !skill.impact.is_empty() && skill.impact != "empty_sequence" {
            commands.spawn(Pending {
                after: shifted_impact_after(skill, rate),
                what: PendingKind::Sequence {
                    name: skill.impact.clone(),
                    at: Transform::from_translation(point).with_rotation(entity_rotation(facing)),
                    rate: 1.0,
                },
            });
        }
    }
}

pub(crate) fn play(commands: &mut Commands, seq: Sequence, actor: Option<Entity>, anchor: Entity, looping: bool) -> Entity {
    // A caster's own skill sequence leaves its effects in place; anything else
    // (a bullet, a ground effect) carries them.
    let follow = actor != Some(anchor);
    play_with(commands, seq, actor, anchor, looping, follow)
}

/// A sequence on an actor that its movement does not interrupt and whose effects
/// follow it (the blows it takes: crate::monsters::play_blow).
pub(crate) fn play_attached(commands: &mut Commands, seq: Sequence, actor: Entity) -> Entity {
    play_with(commands, seq, Some(actor), actor, false, true)
}

/// A death sequence on its actor: its animation tracks may pose the dead body.
pub(crate) fn play_death(commands: &mut Commands, seq: Sequence, actor: Entity) -> Entity {
    let player = play(commands, seq, Some(actor), actor, false);
    commands.queue(move |w: &mut World| {
        if let Some(mut p) = w.get_mut::<SequencePlayer>(player) {
            p.on_death = true;
        }
    });
    player
}

/// A sequence on `actor` (its animation, hide tracks) whose effects are placed at
/// `anchor`, carried by it when `follow`, again and again when `looping`.
pub(crate) fn play_with(commands: &mut Commands, seq: Sequence, actor: Option<Entity>, anchor: Entity, looping: bool, follow: bool) -> Entity {
    let n = seq.tracks.len();
    commands
        .spawn(SequencePlayer { seq, on_death: false, t: 0.0, actor, anchor, looping, follow, stop: false, superseded: false, rate: 1.0, started: vec![false; n], spawned: vec![None; n], bases: vec![Mat4::IDENTITY; n] })
        .id()
}

#[allow(clippy::too_many_arguments)]
/// What a skill costs in the class's resource: its ResourceCost, a fraction of
/// the maximum, times that maximum (the server's cost_fraction x maximum).
pub fn skill_cost(skill: &SkillDef, resource_max: f32) -> f32 {
    skill.resource_cost * resource_max
}

/// What the skill named `id` costs, once the skill table is in.
/// When each skill, or its cooldown category, is ready again: (start, ready) on
/// the real clock (Time<Real>). cast_input writes it; the quick slots draw it
/// (crate::ui, a dark sweep over the icon while it runs).
#[derive(Resource, Default)]
pub struct SkillCooldowns(HashMap<String, (f64, f64)>);

impl SkillCooldowns {
    /// The share of a skill's cooldown still to run (1 just cast, 0 ready).
    pub fn remaining(&self, data: &SkillData, tables: &Assets<SkillTable>, id: &str, now: f64) -> f32 {
        let Some(skill) = data.by_id.get(id).and_then(|wire| tables.get(&data.skills).and_then(|t| t.0.get(wire))) else { return 0.0 };
        match self.0.get(cooldown_key(skill)) {
            Some(&(start, ready)) if now < ready && ready > start => ((ready - now) / (ready - start)) as f32,
            _ => 0.0,
        }
    }

    /// The seconds a skill's cooldown still has to run (0 ready).
    pub fn seconds_left(&self, data: &SkillData, tables: &Assets<SkillTable>, id: &str, now: f64) -> f64 {
        let Some(skill) = data.by_id.get(id).and_then(|wire| tables.get(&data.skills).and_then(|t| t.0.get(wire))) else { return 0.0 };
        self.0.get(cooldown_key(skill)).map_or(0.0, |&(_, ready)| (ready - now).max(0.0))
    }
}

/// A cooldown's seconds as the original writes them on the slot: whole seconds
/// ("21 s"), tenths under one ("0.4 s"), in tenths of a second (0: none).
/// EVIDENCE: the user's release261_192 shots (a quick slot reading "21 s", a
///   Collector's Bag sticker counting "0.4 s", "0.2 s", "0.1 s").
pub fn cooldown_tenths(seconds: f64) -> u32 {
    if seconds <= 0.0 { 0 } else if seconds >= 1.0 { seconds.ceil() as u32 * 10 } else { (seconds * 10.0).ceil() as u32 }
}

pub fn cooldown_text(tenths: u32) -> String {
    if tenths >= 10 { format!("{} s", tenths / 10) } else { format!("0.{tenths} s") }
}

/// Skills of one cooldown category share their cooldown.
fn cooldown_key(skill: &SkillDef) -> &str {
    if skill.cooldown_category.is_empty() { &skill.id } else { &skill.cooldown_category }
}

pub fn skill_cost_by_id(data: &SkillData, tables: &Assets<SkillTable>, id: &str, resource_max: f32) -> Option<f32> {
    let wire = data.by_id.get(id)?;
    tables.get(&data.skills)?.0.get(wire).map(|s| skill_cost(s, resource_max))
}

/// Whether a targeted cast must walk closer first. Targeted melee as the release261
/// client measures it (Skills::ClientSkillValidator::Validate 0x14032bed0, range
/// check 0x140a1b750, DSOR_Server docs/MELEE_TARGETING.md): the 3D centre distance
/// minus the target's navigation footprint against AttackRange; with the capsule
/// (0.3 for most monsters) it walked up to a medium monster from 2.8 m though the
/// hit reached from there. A shape-less monster keeps its capsule, as the server.
/// Ranged targets keep the flat distance plus the capsule (the server measures
/// those against the capsule; UNVERIFIED against the original).
/// An untargeted melee skill (type Melee: mightyswing, mighty360, stab...)
/// aimed at a monster is measured the same way: 2018 Validate 0x5E3C61 checks
/// every skill type against the clicked actor with one test (0x00A3522D,
/// centre distance minus the target's radius).
fn beyond_reach(skill: &SkillDef, caster: Vec3, target: Vec3, capsule: f32, footprint: Option<f32>) -> bool {
    if matches!(skill.kind.as_str(), "MeleeTarget" | "Melee") {
        caster.distance(target) - footprint.unwrap_or(capsule) > skill.range
    } else {
        Vec2::new(target.x - caster.x, target.z - caster.z).length() > skill.range + capsule
    }
}

/// Whether a cast aimed at a monster out of reach walks to it and strikes
/// there, rather than going off where the player stands.
/// EVIDENCE (dro_client.exe 2018): the slot handler 0x4C1440 calls
///   ClientSkillValidator::Validate 0x5E3C61; with the clicked actor its
///   ValidateTarget 0x00A34FDD keeps the actor's position as the aim for melee
///   types (SkillInfo type flags 0x9BAF5D bit 0: Melee 0, MeleeTarget 1, parsed
///   by 0x9BC159) unless the targeting (0x9BC260: Actor 0, Radius 1, Angle 2,
///   Cone 3, User 4) is User, which aims at the caster. Out of range (17) a
///   melee type is not cast (flags bit 1 clear) and 0x4C0D2D posts
///   Messaging::SkillPerformed instead. UNVERIFIED: that message's consumer
///   (taken to be the walk into range, as for MeleeTarget).
/// FAILURE: a Melee skill on the left button (mighty360) struck the air where
///   the player stood when a monster in front was clicked, instead of walking
///   to it (2026-10-24).
fn approaches(skill: &SkillDef) -> bool {
    matches!(skill.kind.as_str(), "MeleeTarget" | "RangedTarget") || (skill.kind == "Melee" && skill.targeting != "User")
}

fn cast_input(
    mut commands: Commands,
    net: Option<NonSendMut<Net>>,
    data: Res<SkillData>,
    tables: Res<Assets<SkillTable>>,
    seqs: Res<Assets<SequenceTable>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    nav: Option<Res<CurrentNav>>,
    navmeshes: Res<Assets<NavMesh>>,
    time: Res<Time<Real>>,
    mut players: Query<(Entity, &mut Transform, &mut LocalPlayer, &Character)>,
    mut cooldowns: ResMut<SkillCooldowns>,
    (hovered, monsters, mut debug_monsters, ui, mut ui_skills, settings, stats, resource_max): (
        Res<crate::actors::monsters::Hovered>,
        Query<(&GlobalTransform, &crate::actors::monsters::Monster, &CharacterAnim)>,
        ResMut<crate::actors::monsters::DebugMonsters>,
        Res<crate::ui::input::UiInput>,
        ResMut<crate::ui::SkillClicks>,
        Res<crate::ui::Settings>,
        Res<crate::gameplay::stats::CharacterStats>,
        Res<crate::gameplay::stats::ResourceMax>,
    ),
    // queued: a target skill aimed at a monster out of reach, (slot, monster),
    // cast once the walk brings it in reach. next_cast: when the last cast lets
    // the next one go (its SkillUnblockFrame), apart from the movement hold
    // (LocalPlayer::casting, its MotionUnblockFrame).
    (mut queued, mut next_cast): (Local<Option<(usize, Entity)>>, Local<f64>),
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("skills::cast_input");
    let clicked_slot = ui_skills.0.take();
    if ui.free_look { *queued = None; return; }
    if ui.typing { return; }
    let Some(mut net) = net else { return };
    // Dead: no casting until revived (the server refuses it anyway).
    if net.death.dead { return; }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    // A click on the ground (a walk) drops the pending attack, and so does the
    // left button held over the ground: net::click_to_move walks there then.
    // CONTRACT: held on a monster and dragged off it, only one of the pending
    //   approach and the walk may set LocalPlayer::target; the two systems are
    //   unordered, so with both the later one won frame by frame.
    if !ui.blocks(MouseButton::Left) && buttons.pressed(MouseButton::Left) && hovered.0.is_none() && !shift {
        *queued = None;
    }
    // Keys and mouse buttons repeat while held, as in the game, paced by the cast,
    // the cooldown and the resource: Shift or a monster under the cursor with the
    // left button, the right button anywhere ("je peux pas rester appuye sur clique
    // droit pour lancer un sort"). A key pressed (then held) wins over a held mouse
    // button: right button held, a press of 2 casts 2.
    // FAILURE (2026-10-24): keys fired once per press ("I cannot hold 1 and continue
    //   attacking"), and with the right button held a key did nothing ("I cannot
    //   even press right click and press 2 to hit").
    let key = settings.current.slot_pressed(&keys).or_else(|| settings.current.slot_held(&keys));
    let left = key.is_none() && !ui.blocks(MouseButton::Left) && (shift || (settings.current.attack_hovered && hovered.0.is_some())) && buttons.pressed(MouseButton::Left);
    let right = key.is_none() && !ui.blocks(MouseButton::Right) && buttons.pressed(MouseButton::Right);
    let slot = if left {
        Some(0)
    } else if right {
        Some(1)
    } else {
        key
    };
    // DSOR_TEST_SLOT=<slot>: that quick slot pressed all the time (testing online
    // without a keyboard; the cooldowns pace it).
    let slot = slot.or(clicked_slot).or_else(|| std::env::var("DSOR_TEST_SLOT").ok().and_then(|v| v.parse().ok()));
    let (slot, aimed, from_queue) = match slot {
        Some(s) => {
            *queued = None;
            (s, hovered.0.map(|(e, _)| e), false)
        }
        None => match *queued {
            Some((s, e)) => (s, Some(e), true),
            None => return,
        },
    };
    // A dead or vanished monster is no target.
    let aimed = aimed.and_then(|e| monsters.get(e).ok().filter(|(_, m, a)| m.is_alive(a)).map(|(at, m, _)| (e, at.translation(), m.actor, m.radius, m.height, m.footprint)));
    if aimed.is_none() {
        *queued = None;
    }
    let Ok((entity, mut tf, mut player, character)) = players.single_mut() else { return };
    if player.casting > 0.0 || time.elapsed_secs_f64() < *next_cast {
        return;
    }
    let Some(name) = net.bar.get(slot).cloned().flatten() else { return };
    let (Some(table), Some(sequences)) = (tables.get(&data.skills), seqs.get(&data.sequences)) else { return };
    let Some(&wire) = data.by_id.get(&name) else { return };
    let Some(skill) = table.0.get(&wire) else { return };
    // As the real client: no cast while the skill (or its category) cools down, nor
    // without the resource it costs. The server refuses those anyway, and drawing
    // them anyway made skills seem to fire twice or into nothing (server.log:
    // "refused: mage_fireball_default costs 40.0 and only 28.0 is left").
    let now = time.elapsed_secs_f64();
    let key = cooldown_key(skill).to_owned();
    if cooldowns.0.get(&key).is_some_and(|&(_, ready)| now < ready) {
        return;
    }
    // ResourceCost is a fraction of the maximum (rage, mana, focus, steam alike):
    // compared as it was, 0.4 against 10 rage let a 40-rage skill go out, play,
    // and be refused by the server ("rejected skill index 1842: resource=10,
    // cost=40", 2026-10-18): it seemed to hit only sometimes.
    // Its effects' ResourceCost modifiers too: predicted at the base cost, a
    // free cast inside Banner of War drained the shown rage (the server took
    // nothing and, paying nothing, sent no correction).
    let cost = skill_cost(skill, resource_max.0) * stats.skill_cost_factor();
    if net.resource.is_some_and(|r| r + 1e-3 < cost) {
        return;
    }
    let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), cameras.single()) else { return };
    let mesh = nav.as_ref().and_then(|n| navmeshes.get(&n.0));
    let Some(mut point) = aimed.map(|(_, at, ..)| at).or_else(|| cursor_ground(window, camera, cam_tf, mesh, tf.translation))
        // DSOR_TEST_SLOT without a cursor over the window (offline screenshots):
        // 3 m ahead.
        .or_else(|| std::env::var("DSOR_TEST_SLOT").is_ok().then(|| tf.translation + Quat::from_rotation_y(player.facing) * Vec3::Z * 3.0)) else { return };
    // A target skill on a monster beyond its reach walks there first, as the 2018
    // client does (ClientSkillValidator 0x5E3C61 answers 17, out of range; its
    // caller 0x4C1553 casts after approaching, 0x4BDEC4); see beyond_reach. So
    // does a melee skill with no target of its own (approaches).
    let targeted = matches!(skill.kind.as_str(), "MeleeTarget" | "RangedTarget");
    let target_actor = if targeted { aimed.map(|(_, _, a, ..)| a) } else { None };
    // Only the left button walks into reach (a click on a monster: go there and
    // strike). The right button and the keys strike from where the player stands,
    // turned to the monster, never stepping: the server decides whether it lands.
    // FAILURE (2026-10-24): a right click on a monster walked the warrior to it
    //   ("I right click to use my skill, with my mouse hovering over a monster and
    //   I walk to it"; clips 20261024-1143-13, 20261024-1259-25).
    let walks = left || from_queue;
    if let (true, Some((e, at, _, radius, _, footprint))) = (approaches(skill), aimed) {
        if walks && beyond_reach(skill, tf.translation, at, radius, footprint) {
            player.target = Some(at);
            *queued = Some((slot, e));
            return;
        }
        *queued = None;
    }
    let to = Vec2::new(point.x - tf.translation.x, point.z - tf.translation.z);
    // A point skill reaches no farther than its range.
    if skill.range > 0.0 && to.length() > skill.range {
        let p = to.normalize() * skill.range;
        point = Vec3::new(tf.translation.x + p.x, point.y, tf.translation.z + p.y);
    }
    // A point on the ground (an area, a teleport, a jump, a charge) lands only on
    // ground the player could walk to: clicked on a wall, a cliff or the void, the
    // farthest reachable point of the line; none, no cast. The server checks the
    // same (services/movement_validation.py) and would refuse it anyway.
    if !targeted && matches!(skill.kind.as_str(), "Shifted" | "Teleport" | "Jump" | "Charge" | "ChargeThrough" | "RangedTargetPoint") {
        if let Some(m) = mesh {
            match m.reachable_along(tf.translation, point) {
                Some(p) => point = p,
                None => return,
            }
        }
    }
    let to = Vec2::new(point.x - tf.translation.x, point.z - tf.translation.z);
    let facing = if to.length() > 0.01 { to.x.atan2(to.y) } else { player.facing };
    player.facing = facing;
    // Turned now: the cast's effects take the caster's placement this frame.
    tf.rotation = Quat::from_rotation_y(facing);
    let rate = if skill.scale_timing { stats.animation_speed().clamp(0.1, 10.0) } else { 1.0 };
    // Movement waits for MotionUnblockFrame only: a skill with 0 there (Furious
    // Battle Cry, the auras, the buffs) goes off while the player keeps walking.
    // FAILURE: held until SkillUnblockFrame too and its walk dropped, Battle Cry
    //   stopped the warrior ("je devrais pouvoir le faire en me deplacant sans
    //   que ca me stoppe", 2026-10-21).
    player.casting = skill.motion_unblock as f32 / FPS / rate;
    if skill.motion_unblock > 0 {
        player.target = None;
    }
    *next_cast = time.elapsed_secs_f64() + (skill.motion_unblock.max(skill.unblock).max(1) as f32 / FPS / rate) as f64;

    let tick = net.server_tick(now_ms(&time));
    // A fresh cast id per cast: the release261 server keys each cast's projectile
    // timers by it (production services/player_combat.py:397-410); with 0 every
    // cast replaced the previous one's.
    static CAST_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let cast_id = CAST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed).max(1);
    let base = SkillBase { high_water: 0, skill_id: wire as u16, heading: facing_to_aim(facing), start_tick: tick, unknown_u32_0: cast_id, marker: None };
    let dir = Quat::from_rotation_y(facing) * Vec3::Z;
    let speed = skill.bullet.as_ref().map(|b| b.velocity / FPS).unwrap_or(1.0);
    let step = [dir.x * speed, 0.0, dir.z * speed, 0.0];
    let points = vec![point.to_array()];
    let command = match skill.kind.as_str() {
        "MeleeTarget" | "RangedTarget" if target_actor.is_some() => {
            ClientCommand::TargetSkill(combat::TargetSkill { base, target: target_actor.unwrap_or(0), server: None })
        }
        // Without a target (Shift, or the cursor off every monster) it goes out as a
        // plain skill: the original swings into the air and dismounts (the user, on
        // release261_192). TargetSkill with target 0 is dropped by the server before
        // any effect ("target loss is a no-op before mount, resource, cooldown, or
        // hit side effects").
        // FAILURE (2026-10-24): it went out as TargetSkill 0: a swing that struck
        //   nothing and, mounted, did not dismount ("I can still attack while
        //   mounted"; "I seem to miss a lot of my attacks").
        "MeleeTarget" | "RangedTarget" => ClientCommand::Skill(combat::Skill { base, server: None }),
        "Ranged" => ClientCommand::BulletSkill(combat::BulletSkill { base, step, orbit: 0, server: None }),
        "RangedTargetPoint" => {
            ClientCommand::TargetPointBulletSkill(combat::TargetPointBulletSkill { base, step, orbit: 0, points, server: None })
        }
        "Shifted" | "Teleport" | "Jump" | "Charge" | "ChargeThrough" => {
            ClientCommand::ShiftedSkill(combat::ShiftedSkill { base, points, server: None })
        }
        _ => ClientCommand::Skill(combat::Skill { base, server: None }),
    };
    net.send(&command);
    cooldowns.0.insert(key, (now, now + skill.cooldown as f64));
    if let Some(r) = net.resource.as_mut() {
        *r -= cost;
    }
    info!("cast {} (wire {wire}, {}) at {point:?}", skill.id, skill.kind);
    if net.offline {
        offline_landing(&mut net, &mut debug_monsters, skill, tf.translation, facing, point, aimed.map(|(_, _, a, ..)| a));
    }
    // A bullet aimed at a monster dips or climbs to its chest (fly_bullets).
    let aim = aimed.filter(|_| skill.bullet.is_some()).map(|(_, at, _, _, height, _)| at + Vec3::Y * height * 0.5);
    let me = Some(net.local_actor.unwrap_or(crate::network::net::OFFLINE_ACTOR));
    perform(&mut commands, skill, sequences, entity, Some(character), tf.translation, facing, point, me, aim, aimed.map(|(_, _, actor, ..)| actor), rate);
}

/// Offline nobody answers a cast: it lands at its hit frame (and after the
/// bullet's flight), on the aimed monster or around the point -- the caster's,
/// victims' and location's status effects (arcanevortex's 8 s vortex), its summons.
fn offline_landing(net: &mut Net, debug_monsters: &mut crate::actors::monsters::DebugMonsters, skill: &SkillDef, from: Vec3, facing: f32, point: Vec3, target: Option<u32>) {
    if matches!(skill.kind.as_str(), "Ranged" | "RangedTarget") {
        return;
    }
    let mut after = skill.hit_frame.max(skill.loop_start) as f32 / FPS;
    // A bullet's landing, as perform flies it: from LoopStartFrame, from its
    // StartOffset ahead of the caster (the grenade's 1.8 m: counted from the feet,
    // its explosion came 0.2 s after the grenade had gone).
    if let Some(b) = skill.bullet.as_ref().filter(|b| b.velocity > 0.0) {
        let flat = Vec2::new(point.x - from.x, point.z - from.z);
        let ahead = (-b.start[2]).max(0.0);
        // On the aimed monster's surface, as the server times its blow (its
        // CapsuleRadius 0.3 and the bullet's radius); a point skill on its point.
        let surface = if target.is_some() { 0.3 + b.radius.max(0.25) } else { 0.0 };
        after = skill.loop_start as f32 / FPS + (flat.length() - ahead - surface).max(0.0) / b.velocity;
    }
    // Items' and talents' effects (item_..., ..._talent) need what the player
    // does not have offline.
    let own = |v: &Vec<(String, f32)>| v.iter().filter(|(n, _)| !n.starts_with("item_") && !n.contains("talent")).cloned().collect();
    // The caster's own effects (the shouts' auras), at the hit frame.
    let mine: Vec<(String, f32)> = own(&skill.user_status);
    let me = net.local_actor.unwrap_or(crate::network::net::OFFLINE_ACTOR);
    for (status, seconds) in mine {
        net.named_status_events.push((me, status, seconds.max(1.0)));
    }
    let location: Vec<(String, f32)> = own(&skill.location_status);
    let (late, now): (Vec<_>, Vec<_>) = location.into_iter().partition(|(n, _)| skill.location_delay.contains_key(n));
    debug_monsters.strikes.push(crate::actors::monsters::Strike {
        after,
        at: point,
        radius: 2.5,
        target,
        victim: own(&skill.victim_status),
        location: now,
    });
    // DE: the effect waits that long after the hit -- the grenade lies its impact
    // sequence's 30 frames (1.2 s, its fuse blinking) and DE:1.2 blows it.
    for (name, seconds) in late {
        let delay = skill.location_delay[&name];
        debug_monsters.strikes.push(crate::actors::monsters::Strike { after: after + delay, at: point, radius: 0.0, target: None, victim: Vec::new(), location: vec![(name, seconds)] });
    }
    // Its summons, beside the caster (UNVERIFIED: the offset's frame), standing
    // still: no server moves them.
    if let Some(sm) = &skill.summon {
        let rot = Quat::from_rotation_y(facing);
        for k in 0..sm.amount {
            let side = (k as f32 - (sm.amount as f32 - 1.0) / 2.0) * 1.2;
            let at = from + rot * Vec3::new(sm.offset[0] + side, 0.0, sm.offset[2].abs().max(1.0));
            debug_monsters.delayed.push(((skill.hit_frame + sm.delay) as f32 / FPS, sm.id.clone(), at));
        }
    }
}

/// Skills other players used, as the server relays them.
#[allow(clippy::too_many_arguments)]
fn remote_skills(
    mut commands: Commands,
    net: Option<NonSendMut<Net>>,
    data: Res<SkillData>,
    tables: Res<Assets<SkillTable>>,
    seqs: Res<Assets<SequenceTable>>,
    // Monsters too (crate::actors::monsters): they move as remote actors, without a Character.
    mut remotes: Query<(Entity, &mut Transform, &mut RemotePlayer, Option<&Character>, &CharacterAnim, Option<&crate::actors::monsters::Monster>)>,
    locals: Query<(&Transform, &LocalPlayer), Without<RemotePlayer>>,
) {
    let Some(mut net) = net else { return };
    if net.skill_events.is_empty() {
        return;
    }
    let (Some(table), Some(sequences)) = (tables.get(&data.skills), seqs.get(&data.sequences)) else { return };
    // Where each actor's chest is, for a bullet aimed at it.
    let chests: HashMap<u32, Vec3> = remotes.iter()
        .map(|(_, tf, r, _, _, m)| (r.actor, tf.translation + Vec3::Y * m.map_or(PLAYER_HEIGHT, |m| m.height) * 0.5))
        .chain(locals.iter().map(|(tf, l)| (l.actor, tf.translation + Vec3::Y * PLAYER_HEIGHT * 0.5)))
        .collect();
    for ev in std::mem::take(&mut net.skill_events) {
        let Some(skill) = table.0.get(&(ev.wire as u32)) else {
            warn!("actor {:#x} used skill {} that skills.json does not have: nothing played", ev.actor, ev.wire);
            continue;
        };
        let Some((e, mut tf, mut r, character, anim, _)) = remotes.iter_mut().find(|(_, _, r, _, _, _)| r.actor == ev.actor) else {
            debug!("skill {} ({}) of actor {:#x}, which is not here", ev.wire, skill.id, ev.actor);
            continue;
        };
        if anim.dead { continue; }
        let facing = aim_to_facing(ev.heading);
        r.facing = facing;
        // The server locks motion until MotionUnblockFrame; SkillUnblockFrame
        // gates the next cast, and can be longer (or motion can stay unlocked).
        // Keep the preceding stop record's target: freezing at the rendered
        // position stranded attacking monsters behind their actual hit capsule.
        let unblock = skill.motion_unblock;
        r.held = unblock as f32 / FPS;
        r.held_until_tick = Some(ev.start_tick.wrapping_add(unblock));
        // Standing from here: when the hold ends before the animation (a melee
        // swing's unblock frame comes before its last frame), a body that walked up
        // to strike must not read as just stopped -- the run took over and cut the
        // swing ("les ennemis au cac ... jouent pas leur animation entierement").
        // Only a record that moves it again ends the swing (move_remotes).
        r.still_for = crate::net::RUN_GRACE;
        tf.rotation = Quat::from_rotation_y(facing);
        let point = ev
            .points
            .first()
            .map(|p| Vec3::from(*p))
            .unwrap_or(tf.translation + Quat::from_rotation_y(facing) * Vec3::Z * skill.range.min(15.0));
        // Aimed at an actor (TargetSkill, TargetBulletSkill), its bullet dips or
        // climbs to that actor's chest, as the local player's own do: from high
        // ground a monster's bullet flew flat at its hand's height, over or past
        // its victim, while the server's blow still landed.
        // FAILURE (2026-10-18): "a monster on high ground attacked me, and the
        //   attack visually passed through me or missed, but I still took the
        //   damage".
        let aim = ev.target.filter(|_| skill.bullet.is_some()).and_then(|t| chests.get(&t).copied());
        // Its own attack speed, from the frame the server scaled by it: the
        // cast's visuals keep time with the blow the server times the same way.
        // They played at 1.0, so a fast caster's meteor or lightning fell after
        // its damage. UNVERIFIED: the server's speed multiplier and rounding
        // (presence_visuals.py) stand for the original's.
        let base = if skill.kind == "Shifted" || skill.bullet.is_some() { skill.loop_start } else { skill.hit_frame };
        let rate = ev
            .phase_frame
            .filter(|_| base > 0)
            .map(|f| (base as f32 / f as f32).clamp(0.1, 10.0))
            .unwrap_or(1.0);
        perform(&mut commands, skill, sequences, e, character, tf.translation, facing, point, Some(ev.actor), aim, ev.target, rate);
        shifted_extra_points(&mut commands, skill, sequences, &ev.points, facing, rate);
    }
}

fn run_pending(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<SkillData>,
    seqs: Res<Assets<SequenceTable>>,
    mut pending: Query<(Entity, &mut Pending)>,
    mut movers: Query<&mut Transform, With<Character>>,
    anims: Query<&CharacterAnim>,
    monsters: Query<&crate::monsters::Monster>,
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("skills::run_pending");
    let Some(sequences) = seqs.get(&data.sequences) else { return };
    for (e, mut p) in &mut pending {
        let caster = match &p.what {
            PendingKind::Bullet { caster, .. } => Some(*caster),
            PendingKind::Teleport { who, .. } => Some(*who),
            PendingKind::Sequence { .. } => None,
        };
        if caster.is_some_and(|e| !anims.get(e).is_ok_and(|a| !a.dead)) {
            commands.entity(e).despawn();
            continue;
        }
        p.after -= time.delta_secs();
        if p.after > 0.0 {
            continue;
        }
        commands.entity(e).despawn();
        match &p.what {
            PendingKind::Bullet { from, dir, def, impact, to, caster, caster_actor, aim, target } => {
                let speed = def.velocity.max(0.01);
                // Free flight: trunc(LifeTime x 25) ticks along the aim, flat, as
                // the 2018 client flies it and the server reads it (a heading); to
                // the aimed point for RangedTargetPoint. Aimed at an actor it dips
                // or climbs to that actor's chest, to meet it where it stands
                // above or below the hand's height; the flat speed is the same.
                let flat = |p: Vec3| Vec2::new(p.x - from.x, p.z - from.z);
                let (velocity, end_t) = match to {
                    Some(p) => {
                        let d = flat(*p);
                        let t = (d.length() / speed).max(1.0 / FPS);
                        (Vec3::new(d.x / t, (p.y - from.y) / t, d.y / t), t)
                    }
                    None => {
                        let mut v = *dir * speed;
                        if let Some(a) = aim {
                            let d = flat(*a).length();
                            if d > 1.0 {
                                v.y = (a.y - from.y) * speed / d;
                            }
                        }
                        (v, (def.lifetime * FPS).trunc().max(1.0) / FPS)
                    }
                };
                let bullet = commands
                    .spawn((
                        Bullet {
                            start: *from,
                            velocity,
                            homing: matches!(def.motion.as_str(), "Homing" | "LockOnHoming"),
                            t: 0.0,
                            end_t,
                            lifetime: end_t,
                            height: def.start[1],
                            death: def.death.clone(),
                            impact: impact.clone(),
                            to_point: to.is_some(),
                            static_collision: def.static_collision,
                            radius: def.radius.max(0.3),
                            caster: *caster,
                            // A friendly summon's (a Shadow Soul's arrow) flies as
                            // a player's: through players, into monsters.
                            from_monster: monsters.get(*caster).is_ok_and(|m| m.targetable),
                            target: *target,
                            dying: None,
                            caster_actor: *caster_actor,
                            bullet_impact: def.impact.clone(),
                            impacts_left: def.impact_count.max(1),
                            passed: Vec::new(),
                            snapped: None,
                        },
                        Transform::from_translation(*from).with_rotation(entity_rotation(dir.x.atan2(dir.z))),
                        Visibility::default(),
                    ))
                    .id();
                if let Some(seq) = sequences.0.get(&def.loop_seq) {
                    play(&mut commands, seq.clone(), None, bullet, true);
                }
            }
            PendingKind::Teleport { who, to } => {
                if let Ok(mut tf) = movers.get_mut(*who) {
                    tf.translation = *to;
                }
            }
            PendingKind::Sequence { name, at, rate } => {
                if let Some(seq) = sequences.0.get(name) {
                    let anchor = commands.spawn((*at, Visibility::default())).id();
                    let player = play(&mut commands, seq.clone(), None, anchor, false);
                    if *rate != 1.0 {
                        let r = *rate;
                        commands.queue(move |w: &mut World| {
                            if let Some(mut p) = w.get_mut::<SequencePlayer>(player) {
                                p.rate = r;
                            }
                        });
                    }
                }
            }
        }
    }
}

/// A player's collision for monster bullets (see fly_bullets). UNVERIFIED.
const PLAYER_RADIUS: f32 = 0.5;
const PLAYER_HEIGHT: f32 = 2.0;
/// How far under an actor's feet or over its capsule a bullet still strikes it.
/// The server's test is flat (dso_server gameplay/combat_entities.py: the bullet
/// and capsule radii in x/z, no heights), while a bullet flies at the hand's
/// height: with half a unit of slack a monster standing 0.6 under the caster's
/// ground was flown over ("les projectiles n'explosent pas", 2026-10-16). Wide
/// enough for any slope, narrow enough not to strike through a floor.
const BULLET_HEIGHT_SLACK: f32 = 4.0;
/// A server blow whose victim stands farther than this beside a bullet's line
/// (beyond the radii) is not that bullet's: a melee swing, a status effect ticking.
const STRIKE_ASIDE: f32 = 1.5;
/// How long a server blow waits for a bullet that has not left the hand: the
/// server's impact can precede the client's flight (attack speed shortens its
/// release phase, a chasing monster stands nearer on the server).
const STRIKE_MEMORY: f32 = 1.0;

/// Where the flat segment `from` -> `to` first enters the circle of `radius` around
/// `centre`: its fraction along the segment, 0 when it starts inside.
/// EVIDENCE: the 2018 client sweeps the bullet between ticks (0x9BC609); testing
///   the tick position alone stepped a 21 units/s missile over a 0.6-unit capsule
///   below ~25 frames a second.
fn sweep_contact(from: Vec2, to: Vec2, centre: Vec2, radius: f32) -> Option<f32> {
    let f = from - centre;
    if f.length_squared() <= radius * radius {
        return Some(0.0);
    }
    let d = to - from;
    let a = d.length_squared();
    if a <= 1e-9 {
        return None;
    }
    let b = 2.0 * f.dot(d);
    let c = f.length_squared() - radius * radius;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let s = (-b - disc.sqrt()) / (2.0 * a);
    (0.0..=1.0).contains(&s).then_some(s)
}

/// How far beside a bullet's flat line an actor at `p` stands, and how far along it.
fn beside_line(b: &Bullet, p: Vec3) -> (f32, f32) {
    let dir = Vec2::new(b.velocity.x, b.velocity.z).normalize_or_zero();
    let rel = Vec2::new(p.x - b.start.x, p.z - b.start.z);
    let along = rel.dot(dir);
    ((rel - dir * along).length(), along)
}

/// Seconds into a bullet's flight at which it meets the surface of an actor of
/// `radius` at `p`: the actor projected on its line, less both radii (as the
/// server times its blow: combat_entities.py's travel distance).
fn flight_time_to(b: &Bullet, p: Vec3, radius: f32) -> f32 {
    let speed = Vec2::new(b.velocity.x, b.velocity.z).length().max(0.01);
    let (_, along) = beside_line(b, p);
    (along - b.radius - radius).max(0.0) / speed
}

/// Preserve the travelled position when changing course. Lifetime is independent
/// of steering; a missing target leaves the last heading intact.
fn steer_bullet(b: &mut Bullet, target: Vec3) {
    let here = b.start + b.velocity * b.t;
    let delta = target - here;
    let distance = delta.xz().length();
    if distance <= 1e-5 { return; }
    let speed = b.velocity.xz().length();
    b.velocity = delta * (speed / distance);
    b.start = here - b.velocity * b.t;
}

#[allow(clippy::too_many_arguments)]
fn fly_bullets(
    mut commands: Commands,
    time: Res<Time>,
    mut bullets: Query<(Entity, &mut Transform, &mut Bullet)>,
    nav: Option<Res<CurrentNav>>,
    navmeshes: Res<Assets<NavMesh>>,
    monsters: Query<(&GlobalTransform, &crate::actors::monsters::Monster, &CharacterAnim)>,
    // Not the humanoid monsters: they wear a Character too.
    players: Query<(Entity, &GlobalTransform, Option<&LocalPlayer>, Option<&RemotePlayer>), (With<Character>, Without<crate::actors::monsters::Monster>)>,
    net: Option<NonSendMut<Net>>,
    mut debug_monsters: ResMut<crate::actors::monsters::DebugMonsters>,
    // The server's blows not matched to a bullet yet: attacker, victim, when.
    mut strikes: Local<Vec<(u32, u32, f32)>>,
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("skills::fly_bullets");
    let mesh = nav.as_ref().and_then(|n| navmeshes.get(&n.0));
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let offline = net.as_ref().is_some_and(|n| n.offline);
    if let Some(mut net) = net {
        let mut fresh = std::mem::take(&mut net.strikes);
        // A blow and the death it caused name the same pair.
        fresh.dedup();
        strikes.extend(fresh.into_iter().map(|(a, v)| (a, v, now)));
    }
    strikes.retain(|(_, _, at)| now - *at <= STRIKE_MEMORY);
    // Only living actors participate in impacts, including server-confirmed
    // hits: a corpse cannot stop another projectile or trigger another blow.
    let mut actors: HashMap<u32, (Vec3, f32)> = HashMap::new();
    for (at, m, anim) in &monsters {
        if m.is_alive(anim) { actors.insert(m.actor, (at.translation(), m.radius)); }
    }
    for (_, at, local, remote) in &players {
        if let Some(id) = local.map(|l| l.actor).or(remote.map(|r| r.actor)) {
            actors.insert(id, (at.translation(), PLAYER_RADIUS));
        }
    }
    let live_targets: HashMap<u32, Vec3> = monsters.iter()
        .filter(|(_, m, anim)| m.is_alive(anim))
        .map(|(at, m, _)| (m.actor, at.translation() + Vec3::Y * m.height * 0.5))
        .chain(players.iter().filter_map(|(_, at, local, remote)| {
            local.map(|l| l.actor).or(remote.map(|r| r.actor))
                .map(|id| (id, at.translation() + Vec3::Y * PLAYER_HEIGHT * 0.5))
        })).collect();
    for (_, mut tf, mut bullet) in &mut bullets {
        if bullet.homing && bullet.dying.is_none() && bullet.snapped.is_none() {
            if let Some(target) = bullet.target.and_then(|id| live_targets.get(&id)) {
                steer_bullet(&mut bullet, *target);
                tf.rotation = entity_rotation(bullet.velocity.x.atan2(bullet.velocity.z));
            }
        }
    }
    // The server's blow lands on an actor ahead of the attacker's bullet: the
    // bullet ends on that living actor (0x8ED6A1), even if it stands a little
    // off its server position. Each blow takes the bullet that reaches its victim soonest; one
    // that finds none waits for a bullet still in the hand.
    let mut matched = Vec::new();
    for (i, &(attacker, victim, _)) in strikes.iter().enumerate() {
        let Some(&(p, radius)) = actors.get(&victim) else { continue };
        let mut best: Option<(f32, Entity)> = None;
        for (e, _, b) in bullets.iter() {
            if b.caster_actor != Some(attacker) || b.dying.is_some() || b.snapped.is_some() || b.to_point || b.impacts_left > 1 || b.passed.contains(&victim) {
                continue;
            }
            if b.homing && b.target.is_some_and(|id| id != victim) { continue; }
            let (aside, along) = beside_line(b, p);
            if aside > b.radius + radius + STRIKE_ASIDE || along < -radius {
                continue;
            }
            let when = (flight_time_to(b, p, radius) - b.t).max(0.0);
            if best.is_none_or(|(w, _)| when < w) {
                best = Some((when, e));
            }
        }
        if let Some((_, e)) = best {
            if let Ok((_, _, mut b)) = bullets.get_mut(e) {
                // It ends on its victim: turned to the victim's chest, it meets
                // the victim where the victim stands, above or below its flight
                // (a bullet from high ground otherwise ended over the head).
                if let Some(chest) = live_targets.get(&victim) { steer_bullet(&mut b, *chest); }
                b.end_t = flight_time_to(&b, p, radius).max(b.t);
                b.snapped = Some(victim);
            }
            matched.push(i);
        }
    }
    for i in matched.into_iter().rev() {
        strikes.remove(i);
    }
    for (e, mut tf, mut b) in &mut bullets {
        if let Some(left) = b.dying.as_mut() {
            *left -= dt;
            if *left <= 0.0 {
                commands.entity(e).despawn();
            }
            continue;
        }
        if b.snapped.is_some_and(|victim| !actors.contains_key(&victim)) {
            b.snapped = None;
            b.end_t = b.lifetime;
        }
        let before = b.start + b.velocity * b.t;
        b.t = (b.t + dt).min(b.end_t);
        let here = b.start + b.velocity * b.t;
        // Drawn ~67 ms behind its position, as the client's view chases it.
        tf.translation = b.start + b.velocity * (b.t - BULLET_LAG).max(0.0);
        // How it ends: 0 still flying, 1 its lifetime ran out (death), 2 it reached
        // its point or the server's victim, hit an actor or a wall (impact).
        let mut ending = 0;
        let mut struck = None;
        // Where it ends, and the ground there (the struck actor's feet).
        let mut at = here;
        let mut feet = None;
        if b.t >= b.end_t {
            ending = if b.to_point || b.snapped.is_some() { 2 } else { 1 };
            if let Some(v) = b.snapped {
                struck = Some(v);
                feet = actors.get(&v).map(|(p, _)| p.y);
            }
        }
        // Actors in its way, swept over this frame's flight: monsters for a
        // player's bullet (monsters' bullets fly through monsters), players for a
        // monster's ("les projectiles devraient exploser sur moi": they flew
        // through). Flat, as the server tests, within BULLET_HEIGHT_SLACK of the
        // capsule. A punch-through bullet (ImpactCount) strikes each actor it
        // passes with its BulletImpactSequence and flies on; the last one ends
        // it. A bullet the server has landed flies to its victim through all else.
        // UNVERIFIED: the player's radius (PLAYER_RADIUS) and height, not traced.
        if ending == 0 && b.snapped.is_none() {
            let from_monster = b.from_monster;
            let seg = (Vec2::new(before.x, before.z), Vec2::new(here.x, here.z));
            // (fraction of this frame's flight, actor, its feet, its radius)
            let mut contacts: Vec<(f32, u32, Vec3, f32)> = Vec::new();
            let consider = |contacts: &mut Vec<(f32, u32, Vec3, f32)>, actor: u32, p: Vec3, radius: f32, height: f32| {
                let dy = here.y - p.y;
                if dy < -BULLET_HEIGHT_SLACK || dy > height + BULLET_HEIGHT_SLACK || b.passed.contains(&actor) {
                    return;
                }
                if let Some(s) = sweep_contact(seg.0, seg.1, Vec2::new(p.x, p.z), b.radius + radius) {
                    contacts.push((s, actor, p, radius));
                }
            };
            // A homing bullet also ends on the actor it homes on, of either side:
            // the Tactical Turret's support missile (minionskill_coveringfireturret,
            // a TargetBulletSkill at its owner) flies from a friendly summon, so its
            // owner stood in its way of nothing. It reached him, hid inside him and
            // lingered its whole 10 s LifeTime with no impact, one more each cast
            // ("des bombes qui disparaissent sur moi").
            let homed = b.target.filter(|_| b.homing);
            let player_id = |local: Option<&LocalPlayer>, remote: Option<&RemotePlayer>| {
                local.map(|l| l.actor).or(remote.map(|r| r.actor)).unwrap_or(u32::MAX)
            };
            if !from_monster {
                // Our own summons and other allies stand in the way of nothing.
                for (gt, m, a) in &monsters {
                    if m.targetable && m.is_alive(a) {
                        consider(&mut contacts, m.actor, gt.translation(), m.radius, m.height);
                    }
                }
                for (pe, gt, local, remote) in &players {
                    let id = player_id(local, remote);
                    if pe != b.caster && homed == Some(id) {
                        consider(&mut contacts, id, gt.translation(), PLAYER_RADIUS, PLAYER_HEIGHT);
                    }
                }
            } else {
                for (pe, gt, local, remote) in &players {
                    if pe != b.caster {
                        consider(&mut contacts, player_id(local, remote), gt.translation(), PLAYER_RADIUS, PLAYER_HEIGHT);
                    }
                }
                for (gt, m, a) in &monsters {
                    if homed == Some(m.actor) && m.is_alive(a) {
                        consider(&mut contacts, m.actor, gt.translation(), m.radius, m.height);
                    }
                }
            }
            contacts.sort_by(|x, y| x.0.total_cmp(&y.0));
            for (s, actor, p, _) in contacts {
                let contact = before + (here - before) * s;
                if b.impacts_left > 1 {
                    debug!("bullet passes {actor} at {contact:?} ({now:.2} s)");
                    b.impacts_left -= 1;
                    b.passed.push(actor);
                    if offline && !from_monster {
                        debug_monsters.blows.push((actor, 0));
                    }
                    if !b.bullet_impact.is_empty() && b.bullet_impact != "empty_sequence" {
                        let on = Transform::from_translation(Vec3::new(contact.x, p.y, contact.z)).with_rotation(tf.rotation);
                        commands.spawn(Pending { after: 0.0, what: PendingKind::Sequence { name: b.bullet_impact.clone(), at: on, rate: 1.0 } });
                    }
                    continue;
                }
                ending = 2;
                struck = Some(actor);
                at = contact;
                feet = Some(p.y);
                break;
            }
        }
        if ending == 0 && b.static_collision && b.snapped.is_none() {
            if let Some(h) = mesh.and_then(|m| m.heights(here.x, here.z).find(|h| *h > here.y - 0.2 && *h < here.y + 2.5)) {
                ending = 2;
                feet = Some(h);
            }
        }
        if ending == 0 {
            continue;
        }
        let how = match (ending, b.snapped) {
            (1, _) => "death",
            (_, Some(_)) => "strike",
            _ => "impact",
        };
        debug!("bullet ends ({how}) at {at:?}, struck {struck:?} ({now:.2} s)");
        // Offline nobody answers the shot: the monster it reached takes the blow.
        // Not on a side it supports: the turret's missile at its owner.
        let ally = struck.is_some_and(|s| b.from_monster == monsters.iter().any(|(_, m, _)| m.actor == s));
        if let (true, Some(actor)) = (offline && b.snapped.is_none() && !ally, struck.or(b.target.filter(|a| b.to_point && actors.contains_key(a)))) {
            debug_monsters.blows.push((actor, 0));
        }
        let name = if ending == 1 { b.death.clone() } else { b.impact.clone() };
        if !name.is_empty() && name != "empty_sequence" {
            // On the ground: the struck actor's feet, else the ground under the end
            // point nearest the hand's height less the start offset (the navigation
            // mesh), else the end point lowered by that offset.
            let y = feet.or_else(|| mesh.and_then(|m| m.ground(at.x, at.z, at.y - b.height, 3.0))).unwrap_or(at.y - b.height);
            commands.spawn(Pending {
                after: 0.0,
                what: PendingKind::Sequence { name, at: Transform::from_translation(Vec3::new(at.x, y, at.z)).with_rotation(tf.rotation), rate: 1.0 },
            });
        }
        // The bullet and everything its loop sequence drew go at once (the
        // client's track exit removes graphics entities).
        commands.entity(e).despawn();
    }
}

fn curves_of(t: &Track) -> HashMap<String, Vec<[f32; 8]>> {
    match t {
        Track::Fx { curves, .. } | Track::Light { curves, .. } => curves.clone(),
        _ => HashMap::new(),
    }
}

/// A track's bezier curve at a sequence frame: segments [x0,y0,cx0,cy0,cx1,cy1,x1,y1],
/// held constant before the first and after the last (preInfinity / postInfinity
/// "Constant").
fn eval_curve(segs: Option<&Vec<[f32; 8]>>, frame: f32) -> Option<f32> {
    let segs = segs?;
    let first = segs.first()?;
    let last = segs.last()?;
    if frame <= first[0] {
        return Some(first[1]);
    }
    if frame >= last[6] {
        return Some(last[7]);
    }
    let s = segs.iter().find(|s| frame >= s[0] && frame <= s[6])?;
    let bez = |t: f32, a: f32, b: f32, c: f32, d: f32| {
        let u = 1.0 - t;
        u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
    };
    // x(t) = frame, by bisection (x is monotonic along a segment).
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) * 0.5;
        if bez(mid, s[0], s[2], s[4], s[6]) < frame {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let t = (lo + hi) * 0.5;
    Some(bez(t, s[1], s[3], s[5], s[7]))
}

/// A track's 9 placement values (tx..sz) with its animated components applied.
fn curved(at: &[f32; 9], curves: &HashMap<String, Vec<[f32; 8]>>, prefix: &str, frame: f32) -> [f32; 9] {
    let mut out = *at;
    if curves.is_empty() {
        return out;
    }
    for (k, name) in ["tx", "ty", "tz", "rx", "ry", "rz", "sx", "sy", "sz"].iter().enumerate() {
        if let Some(v) = eval_curve(curves.get(&format!("{prefix}.{name}")), frame) {
            out[k] = v;
        }
    }
    out
}

/// A sequence point light's intensity (Nebula 0..10) in bevy lumens.
/// UNVERIFIED: the scale; chosen so the fireball's light does not burn its rock white.


/// An effect transform from a track: translation, rotation in degrees, scale.
fn track_transform(at: &[f32; 9]) -> Transform {
    Transform {
        translation: Vec3::new(at[0], at[1], at[2]),
        rotation: Quat::from_euler(EulerRot::YXZ, at[4].to_radians(), at[3].to_radians(), at[5].to_radians()),
        scale: Vec3::new(at[6], at[7], at[8]),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn play_sequences(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    mut players: Query<(Entity, &mut SequencePlayer)>,
    (exists, pending_visuals): (Query<()>, Query<(), With<crate::actors::npc::NpcVisualPending>>),
    characters: Query<&Character>,
    model_bones: Query<&crate::actors::character::ModelBones>,
    mut anims: Query<&mut CharacterAnim>,
    mut visibility: Query<&mut Visibility>,
    transforms: Query<&GlobalTransform>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    mut shake: ResMut<CameraShake>,
    (locals, remotes, pets): (Query<&LocalPlayer>, Query<&RemotePlayer>, Query<(), With<crate::actors::pets::Pet>>),
    mut point_lights: Query<(&mut PointLight, &mut Transform), Without<Character>>,
    mut colors: ResMut<crate::actors::monsters::ShaderColors>,
    // A character's own Transform: current the frame its skill starts, where its
    // GlobalTransform still holds the previous frame (the facing before it turned
    // to cast).
    bodies: Query<&Transform, (With<Character>, Without<PointLight>)>,
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("skills::play_sequences");
    let eye = cameras.iter().next().map(|c| c.translation());
    let mut lights_to_set: Vec<(Entity, [f32; 3], f32, f32, Transform)> = Vec::new();
    // A new skill sequence on an actor takes its animation over from the older
    // ones (their effects play on): an older one's ending animation track reset
    // the actor to Idle in the middle of the second cast of the same skill.
    let newest: HashMap<Entity, Entity> = players
        .iter()
        .filter(|(_, p)| p.t == 0.0 && p.actor.is_some() && p.actor == Some(p.anchor) && p.seq.tracks.iter().any(|t| matches!(t, Track::Anim { .. })))
        .map(|(e, p)| (p.anchor, e))
        .collect();
    for (e, mut p) in &mut players {
        if let Some(&new) = p.actor.and_then(|a| newest.get(&a)) {
            if new != e && p.t > 0.0 && p.actor == Some(p.anchor) {
                p.superseded = true;
            }
        }
    }
    for (e, mut p) in &mut players {
        // Walking gives the body back to locomotion, not the whole sequence:
        // world effects such as Frostwind must keep their authored lifetime.
        // Attached hit/status sequences and death sequences keep their animation.
        if !p.on_death && !p.follow && p.t > 0.1 && p.actor.is_some_and(|a| {
            locals.get(a).is_ok_and(|l| l.target.is_some() && l.casting <= 0.0)
                || remotes.get(a).is_ok_and(|r| r.still_for <= 0.0)
        }) {
            p.superseded = true;
        }
        // A Sustained skill's loop on its actor (perform) ends once something
        // else takes the actor over; it has no length of its own to run out.
        if p.superseded && p.looping && !p.follow && p.actor == Some(p.anchor) {
            p.stop = true;
        }
        let interrupted = !p.on_death && p.actor.is_some_and(|a| anims.get(a).is_ok_and(|a| a.dead));
        let anchor_alive = exists.contains(p.anchor) && !interrupted;
        // Keep real sequence time while an NPC rig is absent/loading, but defer
        // attachment. One-shot bursts must expire rather than replay much later
        // on re-entry; active looping tracks bind when their rig becomes ready.
        let waiting_for_visual = p.visual_actors().any(|a| pending_visuals.contains(a));
        p.t += time.delta_secs() * p.rate;
        let mut frame = p.t * FPS;
        let length = p.seq.length.max(1) as f32;
        if p.looping && anchor_alive && frame > length {
            p.t %= length / FPS;
            frame = p.t * FPS;
            for i in 0..p.started.len() {
                // A graphics track over the whole sequence stays: restarted, a
                // dropped item's glow grew in again every 20 s (drop_<rarity>,
                // 500 frames, one fx track 0..500).
                let (start, end) = p.seq.tracks[i].span();
                if matches!(p.seq.tracks[i], Track::Fx { .. }) && start <= 0 && end >= p.seq.length as i32 && p.spawned[i].is_some() {
                    continue;
                }
                if let Some(s) = p.spawned[i].take() {
                    commands.entity(s).try_despawn();
                }
                p.started[i] = false;
            }
        }
        let done = p.stop || !anchor_alive || (!p.looping && frame > length);
        if waiting_for_visual && !done { continue; }
        for i in 0..p.seq.tracks.len() {
            let track = p.seq.tracks[i].clone();
            let (start, end) = track.span();
            let ending = done || frame >= end as f32;
            // A shader colour: set every frame it runs, its default once it ends.
            if let (Track::Color { param, color, default, intensity, curves, .. }, Some(actor)) = (&track, p.actor) {
                if param == "HitColor" {
                    if !ending && frame >= start as f32 {
                        p.started[i] = true;
                        let k = eval_curve(curves.get("intensity"), frame).unwrap_or(*intensity);
                        colors.0.push((actor, Vec4::new(color[0], color[1], color[2], color[3] * k)));
                    } else if ending && p.started[i] {
                        colors.0.push((actor, Vec4::from_array(*default)));
                        p.spawned[i] = None;
                    }
                }
                continue;
            }
            if !p.started[i] && frame >= start as f32 && !ending {
                p.started[i] = true;
                match &track {
                    Track::Anim { name, speed, .. } if !p.superseded => {
                        // A dead actor's pose belongs to its death: the killing blow's
                        // "Hit" (started the frame before the Kill) stood a dead
                        // gremlin back up ("les gremlin goon une fois morts restent
                        // en idle").
                        if let Some(Ok(mut a)) = p.actor.map(|a| anims.get_mut(a)).filter(|a| a.as_ref().is_ok_and(|a| !a.dead || p.on_death)) {
                            a.state = AnimState::Named(name.clone());
                            a.speed = *speed * p.rate;
                            // The same skill again restarts its animation.
                            a.replay();
                        }
                    }
                    Track::Fx { graphics, joint, at, .. } => {
                        let parent = if joint.is_empty() {
                            Some(p.anchor)
                        } else {
                            p.actor
                                .and_then(|a| {
                                    characters.get(a).ok().and_then(|c| c.bone(joint)).or_else(|| model_bones.get(a).ok().and_then(|b| b.0.get(joint).copied()))
                                })
                                .or(Some(p.anchor))
                        };
                        let path = format!("{graphics}.glb");
                        // On a character (not a joint): into its entity frame.
                        let mut place = track_transform(&curved(at, &curves_of(&track), "graphicstrans", frame));
                        let mut base = Mat4::IDENTITY;
                        // CONTRACT: every actor, dressed character or whole-model
                        //   monster (RemotePlayer without Character) or pet.
                        // FAILURE: monsters were left out: their effects burst behind
                        //   them ("certains fx sont dans leur dos").
                        if joint.is_empty() && (characters.contains(p.anchor) || remotes.contains(p.anchor) || pets.contains(p.anchor)) {
                            base = Transform::from_rotation(entity_rotation(0.0)).to_matrix();
                            place = Transform::from_matrix(base * place.to_matrix());
                        }
                        let mut fx = commands.spawn((
                            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path.clone()))),
                            FxModel { gltf: assets.load(path), started: false, looping: p.looping, speed: p.rate.max(0.01) },
                            place,
                            // Shown by start_fx_animations once its model, textures
                            // included, is in and its clip plays (see there).
                            Visibility::Hidden,
                        ));
                        let unattached = joint.is_empty() && !p.follow;
                        if unattached {
                            // Left where it was made: the anchor's world placement now.
                            // CONTRACT: a character's own Transform (it has no parent), not
                            //   its GlobalTransform, which is propagated after this system.
                            // FAILURE: every effect left by a cast pointed where the previous
                            //   cast had aimed ("le fx est decale de 1").
                            let now = bodies.get(p.anchor).map(|t| GlobalTransform::from(*t)).ok().or_else(|| transforms.get(p.anchor).ok().copied());
                            if let Some(g) = now.as_ref() {
                                base = g.to_matrix() * base;
                                fx.insert(Transform::from_matrix(g.to_matrix() * place.to_matrix()));
                            }
                        } else if let Some(parent) = parent.filter(|p| exists.contains(*p)) {
                            fx.insert(ChildOf(parent));
                        }
                        p.spawned[i] = Some(fx.id());
                        p.bases[i] = base;
                    }
                    Track::Light { color, intensity, range, at, .. } => {
                        let light = commands
                            .spawn((
                                // As the level's lights: Nebula's colour x intensity, falling
                                // off linearly to its range (crate::rendering::lighting::point_light).
                                crate::rendering::lighting::point_light(*color, *intensity, *range),
                                if characters.contains(p.anchor) || remotes.contains(p.anchor) || pets.contains(p.anchor) {
                                    Transform::from_rotation(entity_rotation(0.0)) * track_transform(at)
                                } else {
                                    track_transform(at)
                                },
                                ChildOf(p.anchor),
                            ))
                            .id();
                        p.spawned[i] = Some(light);
                    }
                    Track::Shake { intensity, range, .. } => {
                        let at = transforms.get(p.anchor).map(|t| t.translation()).ok();
                        let near = match (eye, at) {
                            (Some(e), Some(a)) => e.distance(a) <= range.max(1.0) + 25.0,
                            _ => true,
                        };
                        if near {
                            shake.trauma = (shake.trauma + intensity).min(1.0);
                        }
                    }
                    Track::Hide { .. } => {
                        if let Some(Ok(mut v)) = p.actor.map(|a| visibility.get_mut(a)) {
                            *v = Visibility::Hidden;
                        }
                        if let Some(a) = p.actor {
                            commands.entity(a).try_insert(SequenceHidden);
                        }
                    }
                    Track::Sound { name, volume, start, end } => {
                        let position = bodies.get(p.anchor).map(|t| t.translation)
                            .or_else(|_| transforms.get(p.anchor).map(|t| t.translation())).unwrap_or(Vec3::ZERO);
                        let (name, volume, anchor) = (name.clone(), *volume, p.anchor);
                        let duration = (*end - *start).max(1) as f32 / FPS / p.rate.max(0.01);
                        commands.queue(move |world: &mut World| {
                            if let Some(mut audio) = world.get_resource_mut::<crate::audio::AudioRequests>() {
                                audio.effect(name, position, anchor, e, volume, duration);
                            }
                        });
                    }
                    Track::Phase { .. } | Track::Anim { .. } | Track::Color { .. } | Track::Character { .. } => {}
                }
            }
            // Animated values (bezier curves over the sequence's frames).
            if let (true, Some(spawned)) = (p.started[i] && !ending, p.spawned[i]) {
                match &track {
                    Track::Fx { at, curves, .. } if !curves.is_empty() => {
                        let place = track_transform(&curved(at, curves, "graphicstrans", frame));
                        commands.entity(spawned).try_insert(Transform::from_matrix(p.bases[i] * place.to_matrix()));
                    }
                    Track::Light { color, intensity, range, at, curves, .. } if !curves.is_empty() => {
                        let k = eval_curve(curves.get("intensity"), frame).unwrap_or(*intensity);
                        let r = eval_curve(curves.get("range"), frame).unwrap_or(*range);
                        let place = track_transform(&curved(at, curves, "lighttrans", frame));
                        lights_to_set.push((spawned, *color, k, r, place));
                    }
                    _ => {}
                }
            }
            if p.started[i] && ending {
                // CONTRACT: gone at once, as the client's track exit does
                //   (GraphicsObjectTrackBar OnExit 0x8B2C72 -> 0x8B27C3 removes the
                //   graphics entity): fading out is the track's own business (its
                //   n3 intensity animators, its end frame).
                if let Some(s) = p.spawned[i].take() {
                    commands.entity(s).try_despawn();
                }
                match &track {
                    Track::Anim { name, .. } if !p.superseded => {
                        if let Some(Ok(mut a)) = p.actor.map(|a| anims.get_mut(a)) {
                            // A dead actor keeps its death pose.
                            if a.state == AnimState::Named(name.clone()) && !a.dead {
                                a.state = AnimState::Idle;
                                a.speed = 1.0;
                            }
                        }
                    }
                    Track::Hide { .. } => {
                        // A dead actor hidden by its death sequence stays hidden.
                        let dead = p.actor.and_then(|a| anims.get(a).ok()).is_some_and(|a| a.dead);
                        if let (false, Some(Ok(mut v))) = (dead, p.actor.map(|a| visibility.get_mut(a))) {
                            *v = Visibility::Inherited;
                        }
                        if let (false, Some(a)) = (dead, p.actor) {
                            commands.entity(a).try_remove::<SequenceHidden>();
                        }
                    }
                    _ => {}
                }
                // Mark as finished: started, nothing spawned, past its end.
                p.started[i] = true;
            }
        }
        if done {
            for s in p.spawned.iter_mut().filter_map(|s| s.take()) {
                commands.entity(s).try_despawn();
            }
            // A standalone anchor (a point on the ground) goes with its sequence.
            if p.actor.is_none() && anchor_alive && !p.looping {
                commands.entity(p.anchor).try_despawn();
            }
            commands.entity(e).despawn();
        }
    }
    for (light, color, k, r, place) in lights_to_set {
        if let Ok((mut l, mut tf)) = point_lights.get_mut(light) {
            let pl = crate::rendering::lighting::point_light(color, k, r);
            l.color = pl.color;
            l.intensity = pl.intensity;
            l.range = r;
            *tf = place;
        }
    }
}

/// Effect models: their node animation (the "-loop" clip) plays once they are in.
/// CONTRACT: an effect shows only once its model and every texture of it is in
///   and its clip is started. Shown as soon as its glTF was parsed, a first cast
///   drew it untextured, its textures popping in afterwards ("les sorts n'ont pas
///   de texture", "certains chargent pas en entier tout de suite"), and for a
///   frame at its rest pose before its clip took over ("une frame au debut ou le
///   sort est lance et il se reaffiche apres").
/// A looping clip, played as the game plays it: the curves that end before the
/// clip does (an appearance -- a dropped item's pop out of the ground, its glow
/// growing in) once, holding their last key; the others over and over.
/// CONTRACT: a clip whose curves all span it loops whole, as before.
/// EVIDENCE: dropitems/all_drop_bag_01.glb "all_drop_bag_01-loop" (3.96 s):
///   item_spawn's curves end at 0.44 s (scale 0.001 -> 1, a hop), item_rotate's
///   run the whole 3.96 s; glow_common's white_pPlane2 grows in over 0.6 s. The
///   conversion joined the spawn and loop clips into one animation.
/// FAILURE (2026-10-16): looped whole, every item popped out of the ground again
///   every 4 s ("les items rejouent l'animation de spawn").
pub(crate) fn looping_graph(clip: &Handle<AnimationClip>, clips: &mut Assets<AnimationClip>, anim: &mut AnimationPlayer, speed: f32) -> AnimationGraph {
    let split = clips.get(clip).and_then(|c| {
        let end = c.duration();
        let (mut once, mut looped) = (AnimationClip::default(), AnimationClip::default());
        let mut parts = (0, 0);
        for (target, curves) in c.curves() {
            for curve in curves {
                if curve.0.domain().end() < end - 1.0e-3 {
                    once.add_variable_curve_to_target(*target, curve.clone());
                    parts.0 += 1;
                } else {
                    looped.add_variable_curve_to_target(*target, curve.clone());
                    parts.1 += 1;
                }
            }
        }
        looped.set_duration(end);
        (parts.0 > 0 && parts.1 > 0).then_some((once, looped))
    });
    match split {
        Some((once, looped)) => {
            let mut graph = AnimationGraph::new();
            let a = graph.add_clip(clips.add(once), 1.0, graph.root);
            let b = graph.add_clip(clips.add(looped), 1.0, graph.root);
            anim.play(a).set_speed(speed);
            anim.play(b).set_speed(speed).repeat();
            graph
        }
        None => {
            let (graph, node) = AnimationGraph::from_clip(clip.clone());
            anim.play(node).set_speed(speed).repeat();
            graph
        }
    }
}

fn start_fx_animations(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut fx: Query<(Entity, &mut FxModel)>,
    gltfs: Res<Assets<Gltf>>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut clips: ResMut<Assets<AnimationClip>>,
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("skills::start_fx_animations");
    for (e, mut m) in &mut fx {
        if m.started {
            continue;
        }
        if std::env::var("DSOR_FX_DEBUG").is_ok() {
            info!("fx {:?} pending, loaded: {}", m.gltf.path(), gltfs.get(&m.gltf).is_some());
        }
        let Some(gltf) = gltfs.get(&m.gltf) else { continue };
        let failed = matches!(
            assets.get_recursive_dependency_load_state(&m.gltf),
            Some(bevy::asset::RecursiveDependencyLoadState::Failed(_))
        );
        if !failed && !assets.is_loaded_with_dependencies(&m.gltf) {
            continue;
        }
        if !m.started && std::env::var("DSOR_FX_DEBUG").is_ok() {
            let n = children.iter_descendants(e).count();
            info!("fx {:?}: {} descendants, {} with a player, {} clips", m.gltf.path(), n, children.iter_descendants(e).filter(|c| players.contains(*c)).count(), gltf.animations.len());
        }
        if gltf.animations.is_empty() {
            m.started = true;
            commands.entity(e).try_insert(Visibility::Inherited);
            continue;
        }
        let Some(player) = children.iter_descendants(e).find(|c| players.contains(*c)) else { continue };
        debug!("fx {:?}: playing {} clip(s)", m.gltf.path(), gltf.animations.len());
        let mut anim = AnimationPlayer::default();
        let graph = if m.looping {
            looping_graph(&gltf.animations[0], &mut clips, &mut anim, m.speed)
        } else {
            let (graph, node) = AnimationGraph::from_clip(gltf.animations[0].clone());
            anim.play(node).set_speed(m.speed);
            graph
        };
        commands.entity(player).try_insert((AnimationGraphHandle(graphs.add(graph)), anim));
        commands.entity(e).try_insert(Visibility::Inherited);
        m.started = true;
    }
}

/// Emitters inside an effect run at its sequence's rate (FxModel::speed): set
/// on them as its scene comes in, emitters included.
fn fx_particle_speed(
    mut commands: Commands,
    fx: Query<(Entity, &FxModel)>,
    children: Query<&Children>,
    emitters: Query<(), (With<crate::rendering::particles::Emitter>, Without<crate::rendering::particles::ParticleSpeed>)>,
) {
    for (e, m) in &fx {
        if (m.speed - 1.0).abs() < 1e-3 { continue; }
        for c in children.iter_descendants(e) {
            if emitters.contains(c) { commands.entity(c).try_insert(crate::rendering::particles::ParticleSpeed(m.speed)); }
        }
    }
}

/// Camera shake, decaying, on top of where the follow camera put it this frame.
fn shake_camera(time: Res<Time>, settings: Res<crate::ui::Settings>, mut shake: ResMut<CameraShake>, mut cameras: Query<&mut Transform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>, Without<NotShadowCaster>)>) {
    if !settings.current.camera_shake { shake.trauma = 0.0; return; }
    if shake.trauma <= 0.0 {
        return;
    }
    let t = time.elapsed_secs();
    let amount = shake.trauma * shake.trauma * 0.6;
    for mut cam in &mut cameras {
        cam.translation += Vec3::new((t * 53.0).sin(), (t * 61.0).cos(), (t * 47.0).sin()) * amount;
    }
    shake.trauma = (shake.trauma - time.delta_secs() * 1.5).max(0.0);
}

/// DSOR_CAST=<skill id>[,<every seconds>]: the map viewer's demo character
/// (--character), or the offline player (debug map), casts that skill straight ahead, repeatedly -- to look at a
/// skill's sequences without a server.
#[allow(clippy::too_many_arguments)]
fn test_cast(
    mut commands: Commands,
    data: Res<SkillData>,
    tables: Res<Assets<SkillTable>>,
    seqs: Res<Assets<SequenceTable>>,
    time: Res<Time>,
    // The map viewer's demo character, or the offline player (the game's camera).
    mut demo: Query<(Entity, &mut Transform, &Character, &CharacterAnim, Option<&LocalPlayer>)>,
    mut next: Local<f32>,
    mut clock: ResMut<TestCastClock>,
    mut count: Local<u32>,
    nav: Option<Res<CurrentNav>>,
    navmeshes: Res<Assets<NavMesh>>,
    loading: Res<crate::screens::loading::LoadingScreen>,
    (net, mut debug_monsters): (Option<NonSendMut<Net>>, ResMut<crate::actors::monsters::DebugMonsters>),
) {
    if loading.active() {
        return;
    }
    // DSOR_TEST_RUN=<speed>: the demo character runs in a circle (reproducing a
    // body left behind while moving).
    if let Some(speed) = std::env::var("DSOR_TEST_RUN").ok().and_then(|v| v.parse::<f32>().ok()) {
        if let Some((_, mut tf, _, _, _)) = demo.iter_mut().next() {
            let a = time.elapsed_secs() * speed / 6.0;
            let centre = Vec3::new(121.5, tf.translation.y, 75.4);
            tf.translation = centre + Vec3::new(a.cos() * 6.0, 0.0, a.sin() * 6.0);
            tf.rotation = Quat::from_rotation_y((-a.sin()).atan2(a.cos()) + std::f32::consts::FRAC_PI_2 * 0.0);
        }
    }
    let Ok(spec) = std::env::var("DSOR_CAST") else { return };
    let mut parts = spec.split(',');
    let name = parts.next().unwrap_or_default();
    let every: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(2.0);
    let (Some(table), Some(sequences)) = (tables.get(&data.skills), seqs.get(&data.sequences)) else { return };
    let Some(skill) = data.by_id.get(name).and_then(|w| table.0.get(w)) else { return };
    // The offline player when there is one (the level's decorative characters
    // stand around too); the map viewer's demo character otherwise.
    let Some(chosen) = demo.iter().find(|q| q.4.is_some()).or_else(|| demo.iter().next()).map(|q| q.0) else { return };
    let Ok((e, mut tfm, character, anim, _)) = demo.get_mut(chosen) else { return };
    // Stand on the navigation mesh, as the online player does.
    if let Some(p) = nav.as_ref().and_then(|n| navmeshes.get(&n.0)).and_then(|m| m.nearest(tfm.translation, 3.0)) {
        tfm.translation = p;
    }
    let tf = *tfm;
    if !anim.is_ready() {
        return;
    }
    // DSOR_TEST_FACING=<degrees>: cast once, facing that way (0 = +Z, 90 = +X),
    // and note when, for test_shot.
    let fixed = std::env::var("DSOR_TEST_FACING").ok().and_then(|v| v.parse::<f32>().ok());
    let wanted_casts: u32 = std::env::var("DSOR_SHOT_CAST").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    if fixed.is_some() && *count >= wanted_casts {
        return;
    }
    *next -= time.delta_secs();
    if *next > 0.0 {
        return;
    }
    *next = every;
    let facing = match fixed {
        Some(deg) => {
            let f = deg.to_radians();
            tfm.rotation = Quat::from_rotation_y(f);
            f
        }
        None => tf.rotation.to_euler(EulerRot::YXZ).0,
    };
    // DSOR_SHOT_CAST=<n>: test_shot times from the n-th cast (assets warm).
    *count += 1;
    let wanted: u32 = std::env::var("DSOR_SHOT_CAST").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    if *count == wanted {
        clock.0 = Some(time.elapsed_secs());
    }
    let point = tf.translation + Quat::from_rotation_y(facing) * Vec3::Z * 8.0;
    let me = net.as_ref().map(|n| n.local_actor.unwrap_or(crate::network::net::OFFLINE_ACTOR));
    // Offline (the debug map's player): what lands, as cast_input does.
    if let Some(mut net) = net.filter(|n| n.offline) {
        offline_landing(&mut net, &mut debug_monsters, skill, tf.translation, facing, point, None);
    }
    perform(&mut commands, skill, sequences, e, Some(character), tf.translation, facing, point, me, None, None, 1.0);
}

/// When test_cast cast (DSOR_TEST_FACING), for test_shot.
#[derive(Resource, Default)]
pub struct TestCastClock(Option<f32>);

/// DSOR_SHOT_AFTER_CAST=<seconds>[,<file>]: one screenshot that long after the
/// test cast, then quit -- frame counts depend on the frame rate, this does not.
fn test_shot(
    mut commands: Commands,
    clock: Res<TestCastClock>,
    time: Res<Time>,
    mut taken: Local<Option<f32>>,
) {
    let Ok(spec) = std::env::var("DSOR_SHOT_AFTER_CAST") else { return };
    let mut parts = spec.splitn(2, ',');
    let after: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.5);
    let file = parts.next().unwrap_or("/tmp/dsor-cast.png").to_owned();
    let now = time.elapsed_secs();
    if let Some(at) = *taken {
        if now > at + 0.5 {
            commands.write_message(AppExit::Success);
        }
        return;
    }
    let Some(cast) = clock.0 else { return };
    if now >= cast + after {
        commands
            .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(bevy::render::view::screenshot::save_to_disk(file));
        *taken = Some(now);
    }
}

/// DSOR_SEQ_VIEW=<sequence>[,<x>,<y>,<z>]: that sequence plays in a loop at a fixed
/// point (default: the demo character's spot + 1.5 up), to look at an effect alone.
fn test_view(
    mut commands: Commands,
    data: Res<SkillData>,
    seqs: Res<Assets<SequenceTable>>,
    playing: Query<(), With<SequencePlayer>>,
    current: Option<Res<crate::world::map::CurrentMap>>,
    manifests: Res<Assets<crate::world::map::MapManifest>>,
    mut clock: ResMut<TestCastClock>,
    time: Res<Time>,
    assets: Res<AssetServer>,
) {
    let Ok(spec) = std::env::var("DSOR_SEQ_VIEW") else { return };
    if !playing.is_empty() {
        return;
    }
    // Once the map is in, so the screenshot shows the effect on its ground.
    let Some(map) = current.as_ref() else { return };
    let (done, total) = map.progress(&assets);
    if !map.spawned || done < total {
        return;
    }
    let mut parts = spec.split(',');
    let name = parts.next().unwrap_or_default();
    let xyz: Vec<f32> = parts.filter_map(|v| v.parse().ok()).collect();
    let Some(sequences) = seqs.get(&data.sequences) else { return };
    let Some(seq) = sequences.0.get(name) else { return };
    let at = if xyz.len() == 3 {
        Vec3::new(xyz[0], xyz[1], xyz[2])
    } else {
        let Some(m) = manifests.get(&map.manifest) else { return };
        Vec3::from(m.center) + Vec3::Y * 1.5
    };
    let anchor = commands.spawn((Transform::from_translation(at), Visibility::default())).id();
    play(&mut commands, seq.clone(), None, anchor, true);
    if clock.0.is_none() {
            clock.0 = Some(time.elapsed_secs());
    }
}

fn retire_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut retiring: Query<(Entity, &mut Retiring)>,
    children: Query<&Children>,
    shown_parts: Query<(), Or<(With<Mesh3d>, With<PointLight>)>>,
    emitters: Query<(), With<crate::rendering::particles::Emitter>>,
) {
    for (e, mut r) in &mut retiring {
        if !r.started {
            r.started = true;
            for c in children.iter_descendants(e) {
                if shown_parts.contains(c) {
                    commands.entity(c).try_insert(Visibility::Hidden);
                }
                if emitters.contains(c) {
                    commands.entity(c).try_insert(crate::rendering::particles::StopEmitting);
                }
            }
        }
        r.left -= time.delta_secs();
        if r.left <= 0.0 {
            commands.entity(e).try_despawn();
        }
    }
}

/// DSOR_FX_DEBUG=1: where effects, their actors and the actors' hips really are.
fn debug_fx_positions(
    fx: Query<(Entity, &GlobalTransform, &FxModel, Option<&ChildOf>), Added<FxModel>>,
    chars: Query<(Entity, &GlobalTransform, &Character)>,
    all: Query<&GlobalTransform>,
    mut ray: bevy::picking::mesh_picking::ray_cast::MeshRayCast,
    kids: Query<&Children>,
) {
    if std::env::var("DSOR_FX_DEBUG").is_err() {
        return;
    }
    for (e, at, m, parent) in &fx {
        info!("fx {e:?} {:?} at {:?} parent {:?}", m.gltf.path(), at.translation(), parent.map(|p| p.parent()));
    }
    if !fx.is_empty() {
        for (e, at, c) in &chars {
            let hips = c.bone("Hips").and_then(|h| all.get(h).ok()).map(|g| g.translation());
            info!("character {e:?} at {:?}, hips at {hips:?}", at.translation());
            // The visible ground under the character, against the map's meshes.
            let p = at.translation();
            let r = Ray3d::new(p + Vec3::Y * 5.0, Dir3::NEG_Y);
            let mine: std::collections::HashSet<Entity> = std::iter::once(e).chain(kids.iter_descendants(e)).collect();
            let filter = |x: Entity| !mine.contains(&x);
            let settings = bevy::picking::mesh_picking::ray_cast::MeshRayCastSettings::default()
                .with_visibility(bevy::picking::mesh_picking::ray_cast::RayCastVisibility::VisibleInView)
                .with_filter(&filter);
            let hits: Vec<f32> = ray.cast_ray(r, &settings).iter().take(4).map(|(_, h)| h.point.y).collect();
            info!("visible ground under it (first hits): {hits:?}; navmesh/feet y {}", p.y);
        }
    }
}

fn carry_casters(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<SkillData>,
    seqs: Res<Assets<SequenceTable>>,
    mut movers: Query<(Entity, &mut Transform, &mut SkillMove, Option<&mut LocalPlayer>, &CharacterAnim)>,
    mut players: Query<&mut SequencePlayer>,
) {
    let sequences = seqs.get(&data.sequences);
    for (e, mut tf, mut m, local, anim) in &mut movers {
        if anim.dead {
            if let Some(mut player) = m.charge.as_ref().and_then(|c| c.player).and_then(|p| players.get_mut(p).ok()) {
                player.stop = true;
            }
            commands.entity(e).remove::<SkillMove>();
            continue;
        }
        if let Some(mut l) = local {
            // Held until it lands.
            l.target = None;
            l.casting = l.casting.max(m.delay + m.duration - m.t + 0.05);
        }
        m.t += time.delta_secs();
        let k = ((m.t - m.delay) / m.duration).clamp(0.0, 1.0);
        if m.t < m.delay {
            continue;
        }
        let p = m.from.lerp(m.to, k);
        tf.translation = p;
        if let (Some(c), Some(sequences)) = (m.charge.as_mut(), sequences) {
            if c.player.is_none() && k < 1.0 {
                if let Some(seq) = sequences.0.get(&c.looped) {
                    // Carried with the caster, not left where it started.
                    c.player = Some(play_with(&mut commands, seq.clone(), Some(e), e, true, true));
                }
            }
            if k >= 1.0 {
                if let Some(mut pl) = c.player.and_then(|pl| players.get_mut(pl).ok()) {
                    pl.stop = true;
                }
                if let Some(seq) = sequences.0.get(&c.bullet_impact) {
                    let anchor = commands.spawn((Transform::from_translation(p).with_rotation(tf.rotation), Visibility::default())).id();
                    play(&mut commands, seq.clone(), None, anchor, false);
                }
                if let Some(seq) = sequences.0.get(&c.impact).filter(|_| c.impact != "empty_sequence") {
                    play(&mut commands, seq.clone(), Some(e), e, false);
                }
            }
        }
        if k >= 1.0 {
            commands.entity(e).remove::<SkillMove>();
        }
    }
}

/// What draws one status effect: its looping (start/tick) sequence, and what to
/// play when it ends.
#[derive(Component)]
struct StatusVisual {
    key: (u32, u32),
    player: Entity,
    anchor: Entity,
    holder: Option<Entity>,
    ends: Option<f32>,
    end: Option<String>,
}

/// Status effects on actors (82) and ground effects (64/65): the sequences of
/// _Template_StatusEffect, which the skills' own sequences do not contain --
/// stuns, burns, ice cubes, auras, a banner left on the ground.
#[allow(clippy::too_many_arguments)]
fn status_visuals(
    mut commands: Commands,
    time: Res<Time>,
    net: Option<NonSendMut<Net>>,
    data: Res<SkillData>,
    statuses: Res<Assets<StatusTable>>,
    seqs: Res<Assets<SequenceTable>>,
    actors: Query<(Entity, Option<&LocalPlayer>, Option<&RemotePlayer>, Option<&crate::actors::npc::Npc>)>,
    mut visuals: Query<(Entity, &mut StatusVisual)>,
    mut players: Query<&mut SequencePlayer>,
    mut by_name: Local<HashMap<String, u32>>,
    mut tested: Local<bool>,
    mut pending_test: Local<Vec<crate::network::net::LocationEvent>>,
    locals: Query<&GlobalTransform, With<LocalPlayer>>,
    mut holds: ResMut<StatusHolds>,
    monsters: Query<(&crate::monsters::Monster, &CharacterAnim)>,
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("skills::status_visuals");
    let (Some(table), Some(sequences)) = (statuses.get(&data.status), seqs.get(&data.sequences)) else { return };
    if by_name.is_empty() {
        *by_name = table.0.iter().map(|(i, d)| (d.id.clone(), *i)).collect();
    }
    let now = time.elapsed_secs();
    // Ended: stop the loop, play the end sequence once where it was.
    for (e, v) in &mut visuals {
        if !actors.contains(v.anchor) || v.holder.is_some_and(|e| monsters.get(e).is_ok_and(|(m, a)| !m.is_alive(a))) {
            if let Ok(mut p) = players.get_mut(v.player) { p.stop = true; }
            commands.entity(e).despawn();
            continue;
        }
        if v.ends.is_some_and(|t| now >= t) {
            if let Ok(mut p) = players.get_mut(v.player) {
                p.stop = true;
            }
            if let Some(seq) = v.end.as_ref().and_then(|n| sequences.0.get(n)) {
                play_with(&mut commands, seq.clone(), v.holder, v.anchor, false, true);
            }
            commands.entity(e).despawn();
        }
    }
    // DSOR_TEST_STATUS=<status id>[,x,y,z]: that status effect played once as a
    // ground effect (offline checks; the server sends the real ones).
    if let Ok(spec) = std::env::var("DSOR_TEST_STATUS") {
        // Without a position: at the local player, once it is in.
        let mut it = spec.split(',');
        let name = it.next().unwrap_or_default().to_owned();
        let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
        let at = if v.len() == 3 { Some(Vec3::new(v[0], v[1], v[2])) } else { locals.single().ok().map(|g| g.translation()) };
        if let Some(at) = at.filter(|_| !*tested && time.elapsed_secs() > 3.0) {
            *tested = true;
            pending_test.push(crate::network::net::LocationEvent { id: 999_999, status: name, position: at, heading: 0.0, seconds: Some(20.0) });
        }
    }
    let mut net_events = Vec::new();
    let mut gone = Vec::new();
    let mut status_events = Vec::new();
    let mut named: Vec<(u32, String, f32)> = Vec::new();
    if let Some(mut net) = net {
        net_events = std::mem::take(&mut net.location_events);
        gone = std::mem::take(&mut net.location_gone);
        status_events = std::mem::take(&mut net.status_events);
        named = std::mem::take(&mut net.named_status_events);
    }
    net_events.extend(pending_test.drain(..));
    // A removal ends its instance's visual (its end sequence plays next frame,
    // above). FAILURE (2026-10-16): read as a start, the mount cast's removal
    // (sent twice with the ride) began the summoning effect again.
    for ev in status_events.iter().filter(|e| e.removed) {
        for (_, mut v) in visuals.iter_mut() {
            if v.key == (ev.holder, ev.instance) {
                v.ends = Some(now);
            }
        }
    }
    let actor_entity = |id: u32| {
        actors.iter().find_map(|(e, l, r, n)| {
            (l.is_some_and(|l| l.actor == id) || r.is_some_and(|r| r.actor == id) || n.is_some_and(|n| n.actor == id)).then_some(e)
        })
    };
    let mut start = |commands: &mut Commands, def: &StatusDef, key: (u32, u32), holder: Option<Entity>, anchor: Entity, seconds: Option<f32>| {
        if let Some(h) = holder.filter(|_| !def.animation.is_empty() || def.holds || HOLDING_STATUSES.contains(&def.id.as_str())) {
            holds.0.push((h, def.animation.clone(), seconds.filter(|s| *s > 0.0).unwrap_or(def.duration)));
        }
        // The same instance again: extended, not drawn twice.
        for (e, v) in visuals.iter() {
            if v.key == key {
                if let Ok(mut p) = players.get_mut(v.player) {
                    p.stop = true;
                }
                commands.entity(e).despawn();
            }
        }
        let end = StatusDef::first(&def.done).or_else(|| StatusDef::first(&def.stop)).cloned();
        let Some(seq) = StatusDef::first(&def.start).or_else(|| StatusDef::first(&def.tick)).and_then(|n| sequences.0.get(n)) else {
            // Nothing while it lasts, only its end: skill_grenade_damage_aura (0 s)
            // is the grenade's explosion, dwarf_grenade_explode -- dropped with
            // the effect, the grenade landed without one.
            if end.is_some() {
                let ends = Some(now + seconds.filter(|s| *s > 0.0).unwrap_or(def.duration.max(0.0)));
                commands.spawn(StatusVisual { key, player: anchor, anchor, holder, ends, end });
            }
            return;
        };
        let player = play_with(commands, seq.clone(), holder, anchor, seq.repeat, true);
        let ends = seconds.filter(|s| *s > 0.0).map(|s| now + s).or(if seq.repeat { Some(now + def.duration.max(0.5)) } else { None });
        // A status with both a start and a tick sequence plays both: the start
        // once, the tick for as long as it lasts (warrior_spikedShield_buff:
        // start warrior_spikedShield, tick warrior_spikedshield_decal, its
        // ground circle). Only the start was played, the circle never showed.
        // UNVERIFIED: replayed each TickRate in the original; looped here.
        let tick = StatusDef::first(&def.start)
            .and(StatusDef::first(&def.tick))
            .filter(|t| StatusDef::first(&def.start) != Some(*t))
            .and_then(|n| sequences.0.get(n));
        if let Some(tick) = tick {
            let player = play_with(commands, tick.clone(), holder, anchor, true, true);
            let ends = seconds.filter(|s| *s > 0.0).map(|s| now + s).or(Some(now + def.duration.max(0.5)));
            commands.spawn(StatusVisual { key, player, anchor, holder, ends, end: None });
        }
        commands.spawn(StatusVisual {
            key,
            player,
            anchor,
            holder,
            ends,
            end,
        });
    };
    for (holder, name, seconds) in named {
        let Some(def) = by_name.get(&name).and_then(|i| table.0.get(i)) else { continue };
        let Some(e) = actor_entity(holder).filter(|e| monsters.get(*e).map_or(true, |(m, a)| m.is_alive(a))) else { continue };
        let instance = 0x4000_0000 | by_name.get(&name).copied().unwrap_or(0);
        start(&mut commands, def, (holder, instance), Some(e), e, Some(seconds));
    }
    for ev in status_events {
        if ev.removed { continue; }
        let Some(def) = table.0.get(&(ev.index as u32)) else { continue };
        let Some(holder) = actor_entity(ev.holder).filter(|e| monsters.get(*e).map_or(true, |(m, a)| m.is_alive(a))) else { continue };
        start(&mut commands, def, (ev.holder, ev.instance), Some(holder), holder, Some(ev.seconds));
    }
    for ev in net_events {
        let Some(def) = by_name.get(&ev.status).and_then(|i| table.0.get(i)) else { continue };
        debug!("location effect {} at {:?} ({now:.2} s)", ev.status, ev.position);
        // A ground effect stays where it was placed.
        let anchor = commands
            .spawn((Transform::from_translation(ev.position).with_rotation(entity_rotation(aim_to_facing(ev.heading))), Visibility::default()))
            .id();
        start(&mut commands, def, (u32::MAX, ev.id), None, anchor, ev.seconds);
    }
    for id in gone {
        for (e, v) in visuals.iter() {
            if v.key == (u32::MAX, id) {
                if let Ok(mut p) = players.get_mut(v.player) {
                    p.stop = true;
                }
                if let Some(seq) = v.end.as_ref().and_then(|n| sequences.0.get(n)) {
                    play_with(&mut commands, seq.clone(), None, v.anchor, false, true);
                }
                commands.entity(e).despawn();
            }
        }
    }
}

/// A trap's model by its _Template_Trap id (the server sends the id, never the
/// Graphics column), and the sequence its trip plays.
/// SOURCE: dso-server-production data/db/static.db4 _Template_Trap (dwarf_mine,
///   dwarf_mine1..10: effects_dwarf/dwarf_tripmine; skill_trap_normal;
///   BeastmanFox_trap), status_effects.json skill_mine_explosion_aura's done.
fn trap_graphics(template: &str) -> Option<(&'static str, Option<&'static str>)> {
    if template.starts_with("dwarf_mine") { Some(("effects_dwarf/dwarf_tripmine", Some("dwarf_tripmine_explode"))) }
    else if template == "skill_trap_normal" { Some(("effects_playerskills/ranger_trap_bullet", None)) }
    else if template == "BeastmanFox_trap" { Some(("effects/beastman_fox_skill_02_traploop", None)) }
    else { None }
}

/// A trap drawn on the ground: its looping model, where it lies, what its trip plays.
#[derive(Component)]
struct TrapVisual {
    id: u32,
    player: Entity,
    anchor: Entity,
    explosion: Option<&'static str>,
}

/// The traps the server placed (NewTrap 63), tripped (66) and took away (64):
/// the mine stays visible until its explosion, half a second after the trip.
/// SOURCE: dso-server-production services/player_traps.py (:157-271).
fn trap_visuals(
    mut commands: Commands,
    net: Option<NonSendMut<Net>>,
    data: Res<SkillData>,
    seqs: Res<Assets<SequenceTable>>,
    traps: Query<(Entity, &TrapVisual)>,
    mut players: Query<&mut SequencePlayer>,
) {
    let Some(mut net) = net else { return };
    if net.trap_events.is_empty() && net.traps_tripped.is_empty() && net.traps_gone.is_empty() { return; }
    let placed = std::mem::take(&mut net.trap_events);
    let tripped = std::mem::take(&mut net.traps_tripped);
    let gone = std::mem::take(&mut net.traps_gone);
    for ev in placed {
        let Some((graphics, explosion)) = trap_graphics(&ev.template) else { info!("trap {} has no model here", ev.template); continue };
        debug!("trap {} ({}) at {:?}", ev.id, ev.template, ev.position);
        let anchor = commands
            .spawn((Transform::from_translation(ev.position).with_rotation(entity_rotation(aim_to_facing(ev.heading))), Visibility::default()))
            .id();
        let seq = Sequence { length: 60, repeat: true, tracks: vec![Track::Fx {
            graphics: graphics.to_owned(), joint: String::new(), at: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0], start: 0, end: 60, curves: HashMap::new(),
        }] };
        let player = play_with(&mut commands, seq, None, anchor, true, true);
        commands.spawn(TrapVisual { id: ev.id, player, anchor, explosion });
    }
    for id in tripped {
        for (_, v) in traps.iter().filter(|(_, v)| v.id == id) {
            if let Some(seq) = v.explosion.and_then(|n| seqs.get(&data.sequences).and_then(|t| t.0.get(n))) {
                play_with(&mut commands, seq.clone(), None, v.anchor, false, true);
            }
        }
    }
    for id in gone {
        for (e, v) in traps.iter().filter(|(_, v)| v.id == id) {
            if let Ok(mut p) = players.get_mut(v.player) { p.stop = true; }
            commands.entity(e).despawn();
        }
    }
}

/// DSOR_VFX_PREVIEW=<dir>: the VFX editor's (tools/vfx_editor) live preview. The
/// editor writes `<dir>/preview.json` -- {rev, kind: "skill" | "sequence" |
/// "effect", name, every, reload: [asset paths]} -- after applying an edit to the
/// assets (tools/vfx_apply.py); on a new rev the listed files are reloaded, what
/// was playing stops, and the local player plays it again every `every` seconds:
/// a skill as test_cast does (offline landing included), a sequence on the
/// player, an effect model 3 m ahead. `<dir>/status.json` answers {rev, error}.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource, Default)]
struct VfxPreview {
    dir: Option<std::path::PathBuf>,
    poll: f32,
    rev: u64,
    order: Option<VfxOrder>,
    /// Seconds to the next play.
    next: f32,
    /// Waiting for the reloaded files before playing.
    reloading: Vec<bevy::asset::UntypedHandle>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Deserialize, Clone)]
struct VfxOrder {
    rev: u64,
    kind: String,
    name: String,
    #[serde(default = "vfx_every")]
    every: f32,
    #[serde(default)]
    reload: Vec<String>,
}

#[cfg(not(target_arch = "wasm32"))]
fn vfx_every() -> f32 {
    3.0
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn vfx_preview(
    mut commands: Commands,
    mut state: Local<VfxPreview>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    data: Res<SkillData>,
    tables: Res<Assets<SkillTable>>,
    mut seqs: ResMut<Assets<SequenceTable>>,
    mut players: Query<&mut SequencePlayer>,
    mut caster: Query<(Entity, &mut Transform, &Character, &CharacterAnim), With<LocalPlayer>>,
    loading: Res<crate::screens::loading::LoadingScreen>,
    (net, mut debug_monsters): (Option<NonSendMut<Net>>, ResMut<crate::actors::monsters::DebugMonsters>),
) {
    if state.dir.is_none() {
        let Ok(dir) = std::env::var("DSOR_VFX_PREVIEW") else { return };
        state.dir = Some(dir.into());
    }
    let dir = state.dir.clone().unwrap();
    state.poll -= time.delta_secs();
    if state.poll <= 0.0 {
        state.poll = 0.25;
        let order = std::fs::read(dir.join("preview.json")).ok().and_then(|b| serde_json::from_slice::<VfxOrder>(&b).ok());
        if let Some(order) = order.filter(|o| o.rev != state.rev) {
            state.rev = order.rev;
            state.reloading.clear();
            for p in &order.reload {
                if p == "skills/sequences.json" {
                    // Read again here: an untyped reload cannot pick its loader
                    // (every table is a .json; "Could not find an asset loader"),
                    // and the edit never reached the preview.
                    let file = std::path::Path::new(&crate::asset_root()).join(p);
                    match std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<HashMap<String, Sequence>>(&b).ok()) {
                        Some(table) => {
                            if let Some(mut t) = seqs.get_mut(&data.sequences) {
                                t.0 = table;
                            }
                        }
                        None => warn!("vfx preview: cannot read {}", file.display()),
                    }
                } else {
                    assets.reload(p.clone());
                    state.reloading.push(assets.load::<Gltf>(p.clone()).untyped());
                }
            }
            // What was playing goes: its effects come from the files just edited.
            for mut p in &mut players {
                p.stop = true;
            }
            // Played again once the stopped sequences are gone: started at once,
            // the old cast's end set the caster back to Idle over the new cast's
            // animation (it froze after the first edit).
            state.next = 0.3;
            state.order = Some(order);
        }
    }
    if loading.active() {
        return;
    }
    let Some(order) = state.order.clone() else { return };
    if state.reloading.iter().any(|h| assets.load_state(h.id()).is_loading()) {
        return;
    }
    state.reloading.clear();
    state.next -= time.delta_secs();
    if state.next > 0.0 {
        return;
    }
    state.next = order.every.max(0.5);
    let Ok((e, mut tfm, character, anim)) = caster.single_mut() else { return };
    if !anim.is_ready() {
        return;
    }
    tfm.rotation = Quat::from_rotation_y(0.0);
    let (from, facing) = (tfm.translation, 0.0);
    let point = from + Vec3::Z * 8.0;
    let status = |error: &str| {
        let _ = std::fs::write(dir.join("status.json"), serde_json::json!({"rev": order.rev, "error": error}).to_string());
    };
    let (Some(table), Some(sequences)) = (tables.get(&data.skills), seqs.get(&data.sequences)) else { return };
    match order.kind.as_str() {
        "skill" => {
            let Some(skill) = data.by_id.get(&order.name).and_then(|w| table.0.get(w)) else {
                return status("unknown skill");
            };
            if let Some(mut net) = net.filter(|n| n.offline) {
                offline_landing(&mut net, &mut debug_monsters, skill, from, facing, point, None);
            }
            perform(&mut commands, skill, sequences, e, Some(character), from, facing, point, None, None, None, 1.0);
        }
        "sequence" => {
            let Some(seq) = sequences.0.get(&order.name) else { return status("unknown sequence") };
            play(&mut commands, seq.clone(), Some(e), e, false);
        }
        _ => {
            // One effect model, alone, ahead of the player: a sequence of its own.
            let frames = (order.every.max(0.5) * FPS) as i32;
            let seq = Sequence {
                length: frames as u32,
                repeat: false,
                tracks: vec![Track::Fx { graphics: order.name.clone(), joint: String::new(), at: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0], start: 0, end: frames, curves: HashMap::new() }],
            };
            let anchor = commands.spawn((Transform::from_translation(from + Vec3::Z * 3.0), Visibility::default())).id();
            play(&mut commands, seq, None, anchor, false);
        }
    }
    status("");
}

/// Revivals hold their holder still for their animation: the server locks the
/// player's movement and skills for the effect's duration (its docs/
/// PLAYER_RESPAWN.md: resurrect_self 4.32 s with StartAnimation Resurrect,
/// map_respawn 2 s). Played without it, the player walked away mid-revival.
const HOLDING_STATUSES: [&str; 2] = ["resurrect_self", "map_respawn"];

/// Statuses just started on an actor with an animation or a hold: (holder,
/// animation state or "", seconds), for hold_for_status.
#[derive(Resource, Default)]
pub struct StatusHolds(pub(crate) Vec<(Entity, String, f32)>);

fn hold_for_status(
    time: Res<Time>,
    mut holds: ResMut<StatusHolds>,
    mut playing: Local<Vec<(Entity, String, f32)>>,
    mut actors: Query<(&mut CharacterAnim, Option<&mut LocalPlayer>, Option<&crate::monsters::Monster>)>,
) {
    // Its animation played once: back to Idle when the effect ends (nothing else
    // ends a state set this way, and the actor stood frozen in its last pose).
    let now = time.elapsed_secs();
    playing.retain(|(holder, animation, until)| {
        if now < *until {
            return true;
        }
        if let Ok((mut anim, _, _)) = actors.get_mut(*holder) {
            if !anim.dead && anim.state == AnimState::Named(animation.clone()) {
                anim.state = AnimState::Idle;
            }
        }
        false
    });
    for (holder, animation, seconds) in holds.0.drain(..) {
        let Ok((mut anim, local, monster)) = actors.get_mut(holder) else { continue };
        if monster.is_some_and(|m| !m.is_alive(&anim)) { continue; }
        if !animation.is_empty() {
            anim.dead = false;
            anim.state = AnimState::Named(animation.clone());
            anim.replay();
            playing.push((holder, animation, now + seconds));
        }
        if let Some(mut player) = local {
            player.target = None;
            player.casting = player.casting.max(seconds);
        }
    }
}

#[cfg(test)]
mod reach_tests {
    use super::*;

    fn skill(kind: &str, range: f32) -> SkillDef {
        serde_json::from_value(serde_json::json!({
            "id": "test", "type": kind, "hit_frame": 1, "loop_start": 0, "unblock": 2, "motion_unblock": 2,
            "range": range, "cooldown": 0.0, "execute": {}, "impact": ""
        })).unwrap()
    }

    #[test]
    fn targeted_melee_reaches_to_the_navigation_footprint_in_3d() {
        // Rage Attack (angrystrike) 1.75: a small monster (0.525) from 2.275, a
        // medium one (1.053) from 2.803; the 0.3 capsule stopped it at 2.05.
        let angry = skill("MeleeTarget", 1.75);
        let at = |d: f32| Vec3::new(d, 0.0, 0.0);
        assert!(!beyond_reach(&angry, Vec3::ZERO, at(2.27), 0.3, Some(0.525)));
        assert!(beyond_reach(&angry, Vec3::ZERO, at(2.28), 0.3, Some(0.525)));
        assert!(!beyond_reach(&angry, Vec3::ZERO, at(2.8), 0.3, Some(1.053)));
        // Height counts (the native distance helper is 3D).
        assert!(beyond_reach(&angry, Vec3::ZERO, Vec3::new(2.2, 1.0, 0.0), 0.3, Some(0.525)));
        // No footprint (older assets, a shape the table lacks): the capsule.
        assert!(beyond_reach(&angry, Vec3::ZERO, at(2.1), 0.3, None));
        assert!(!beyond_reach(&angry, Vec3::ZERO, at(2.0), 0.3, None));
        // Ranged targets as before: flat distance plus the capsule.
        let ranged = skill("RangedTarget", 10.0);
        assert!(!beyond_reach(&ranged, Vec3::ZERO, Vec3::new(10.2, 5.0, 0.0), 0.3, Some(4.0)));
        assert!(beyond_reach(&ranged, Vec3::ZERO, at(10.4), 0.3, Some(4.0)));
    }

    #[test]
    fn untargeted_melee_on_a_monster_walks_into_reach_but_self_centred_skills_do_not() {
        let with = |kind: &str, targeting: &str, range: f32| {
            let mut s = skill(kind, range);
            s.targeting = targeting.into();
            s
        };
        // mighty360 (Melee, Radius, 6): measured as targeted melee is.
        let spin = with("Melee", "Radius", 6.0);
        assert!(approaches(&spin));
        let at = |d: f32| Vec3::new(d, 0.0, 0.0);
        assert!(!beyond_reach(&spin, Vec3::ZERO, at(6.5), 0.3, Some(0.525)));
        assert!(beyond_reach(&spin, Vec3::ZERO, at(6.6), 0.3, Some(0.525)));
        assert!(approaches(&with("Melee", "Angle", 3.5)));
        assert!(approaches(&with("Melee", "Cone", 10.0)));
        // A shout or an aura goes off where the player stands.
        assert!(!approaches(&with("Melee", "User", 2.0)));
        assert!(approaches(&with("MeleeTarget", "Actor", 1.75)));
        assert!(!approaches(&with("Ranged", "Radius", 25.0)));
        assert!(!approaches(&with("Shifted", "Radius", 15.0)));
    }
}

#[cfg(test)]
mod jump_tests {
    use super::*;

    #[test]
    fn a_jump_lands_on_its_hit_tick_after_its_release() {
        let skill = |loop_start: u32, hit_frame: u32| -> SkillDef { serde_json::from_value(serde_json::json!({
            "id": "enragingleap", "type": "Jump", "hit_frame": hit_frame, "loop_start": loop_start, "unblock": 9, "motion_unblock": 9,
            "range": 10.0, "cooldown": 5.0, "execute": {}, "impact": ""
        })).unwrap() };
        // Enraging Leap: released at frame 4, down at 4 + 11 = 15 (its clip at 18), not at 11.
        let leap = skill(4, 11);
        assert!((leap.loop_start as f32 / FPS + shifted_hit(&leap, 1.0) - 15.0 / FPS).abs() < 1e-6);
        // Faster with the attack speed; a long fall (25 frames or more) keeps its length.
        assert!((shifted_hit(&leap, 2.0) - 11.0 / FPS / 2.0).abs() < 1e-6);
        assert!((shifted_hit(&skill(21, 33), 2.0) - 33.0 / FPS).abs() < 1e-6);
    }
}

#[cfg(test)]
mod cooldown_tests {
    use super::*;

    #[test]
    fn a_cooldown_is_shared_by_its_category_and_runs_down() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default())).init_asset::<SkillTable>();
        let skill = |id: &str, category: &str| -> SkillDef { serde_json::from_value(serde_json::json!({
            "id": id, "type": "Teleport", "hit_frame": 1, "loop_start": 0, "unblock": 2, "motion_unblock": 2,
            "range": 10.0, "cooldown": 5.0, "cooldown_category": category, "execute": {}, "impact": ""
        })).unwrap() };
        let table = SkillTable(HashMap::from([(1, skill("teleport", "Skill07")), (2, skill("blink", "Skill07")), (3, skill("bolt", ""))]));
        let mut data = SkillData::for_test(default());
        data.skills = app.world_mut().resource_mut::<Assets<SkillTable>>().add(table);
        data.by_id = HashMap::from([("teleport".into(), 1), ("blink".into(), 2), ("bolt".into(), 3)]);
        let tables = app.world().resource::<Assets<SkillTable>>();
        let mut cooldowns = SkillCooldowns::default();
        cooldowns.0.insert("Skill07".into(), (10.0, 15.0));
        assert_eq!(cooldowns.remaining(&data, tables, "teleport", 12.5), 0.5);
        assert_eq!(cooldowns.remaining(&data, tables, "blink", 12.5), 0.5);
        assert_eq!(cooldowns.remaining(&data, tables, "bolt", 12.5), 0.0);
        assert_eq!(cooldowns.remaining(&data, tables, "teleport", 15.0), 0.0);
        assert_eq!(cooldowns.remaining(&data, tables, "unknown", 12.5), 0.0);
        cooldowns.0.insert("bolt".into(), (12.0, 17.0));
        assert_eq!(cooldowns.remaining(&data, tables, "bolt", 13.0), 0.8);
        // The seconds written on the slot: whole seconds, tenths under one.
        assert_eq!(cooldowns.seconds_left(&data, tables, "teleport", 12.5), 2.5);
        assert_eq!(cooldowns.seconds_left(&data, tables, "teleport", 16.0), 0.0);
        let shown = |s: f64| cooldown_tenths(s);
        assert_eq!((shown(20.3), shown(1.0), shown(0.95), shown(0.31), shown(0.0)), (210, 10, 10, 4, 0));
        assert_eq!((cooldown_text(210), cooldown_text(10), cooldown_text(4)), ("21 s".to_owned(), "1 s".to_owned(), "0.4 s".to_owned()));
    }
}

#[cfg(test)]
mod combat_idle_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn a_bullet_shot_down_from_a_cliff_aims_at_the_point_below_at_hand_height() {
        let mut world = World::new();
        let skill: SkillDef = serde_json::from_value(serde_json::json!({
            "id": "test_missile", "type": "Ranged", "hit_frame": 1, "loop_start": 0,
            "unblock": 2, "motion_unblock": 2, "range": 10.0, "cooldown": 0.0, "execute": {}, "impact": "",
            "bullet": {"loop": "", "impact": "", "death": "", "motion": "Straight", "velocity": 20.0,
                "lifetime": 1.0, "radius": 0.3, "start": [0.0, 1.5, -1.0], "static_collision": true}
        })).unwrap();
        let caster = world.spawn_empty().id();
        // From 4 units up, at a point 8 ahead below; and at an actor's chest.
        for (aim, expected) in [(None, Vec3::new(0.0, 1.5, 8.0)), (Some(Vec3::new(0.0, 0.9, 8.0)), Vec3::new(0.0, 0.9, 8.0))] {
            let skill = skill.clone();
            world.run_system_once(move |mut commands: Commands| {
                perform(&mut commands, &skill, &SequenceTable(default()), caster, None,
                    Vec3::Y * 4.0, 0.0, Vec3::new(0.0, 0.0, 8.0), None, aim, None, 1.0);
            }).unwrap();
            let mut pending = world.query::<&Pending>();
            let aims: Vec<_> = pending.iter(&world).filter_map(|p| match &p.what { PendingKind::Bullet { aim, .. } => Some(*aim), _ => None }).collect();
            assert_eq!(aims, vec![Some(expected)]);
            let ids: Vec<Entity> = world.query_filtered::<Entity, With<Pending>>().iter(&world).collect();
            for e in ids { world.despawn(e); }
        }
    }

    #[test]
    fn casting_unarmed_enters_combat_even_without_an_execute_sequence() {
        let mut world = World::new();
        world.init_resource::<Time>();
        let root = world.run_system_once(|mut commands: Commands|
            crate::actors::character::spawn_character(&mut commands, default(), Transform::default())).unwrap();
        world.run_system_once(crate::actors::character::drive_animations).unwrap();
        assert_eq!(world.get::<CharacterAnim>(root).unwrap().state, AnimState::IdleHub);
        world.run_system_once(move |mut commands: Commands, characters: Query<&Character>| {
            let skill: SkillDef = serde_json::from_value(serde_json::json!({
                "id": "test_attack", "type": "Melee", "hit_frame": 1, "loop_start": 0,
                "unblock": 2, "motion_unblock": 2, "range": 2.0, "cooldown": 0.0,
                "execute": {}, "impact": ""
            })).unwrap();
            perform(&mut commands, &skill, &SequenceTable(default()), root, Some(characters.get(root).unwrap()),
                Vec3::ZERO, 0.0, Vec3::Z, None, None, None, 1.0);
        }).unwrap();
        world.run_system_once(crate::actors::character::drive_animations).unwrap();
        assert_eq!(world.get::<CharacterAnim>(root).unwrap().state, AnimState::Idle);
    }
}

#[cfg(test)]
mod npc_residency_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use std::time::Duration;

    #[test]
    fn status_sequence_waits_for_rig_then_attaches_to_joint_and_cleans_up() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Gltf>().init_asset::<bevy::world_serialization::WorldAsset>()
            .init_resource::<CameraShake>().init_resource::<crate::actors::monsters::ShaderColors>()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)))
            .add_systems(Update, play_sequences);
        let root = app.world_mut().spawn((Transform::default(), Visibility::Hidden, crate::actors::npc::NpcVisualPending)).id();
        let sequence = Sequence {
            length: 100, repeat: true,
            tracks: vec![Track::Fx { graphics: "tests/npc_residency_effect".into(), joint: "Hips".into(),
                at: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0], start: 0, end: 100, curves: HashMap::new() }],
        };
        let player = app.world_mut().run_system_once(move |mut commands: Commands|
            play_with(&mut commands, sequence.clone(), Some(root), root, true, true)).unwrap();
        for _ in 0..3 { app.update(); }
        let pending = app.world().get::<SequencePlayer>(player).unwrap();
        assert!(pending.t > 0.0, "sequence deadlines must advance while attachment waits");
        assert!(!pending.started[0] && pending.spawned[0].is_none());

        let joint = app.world_mut().spawn((Transform::default(), ChildOf(root))).id();
        app.world_mut().entity_mut(root).remove::<crate::actors::npc::NpcVisualPending>()
            .insert(crate::actors::character::ModelBones([("Hips".into(), joint)].into_iter().collect()));
        app.update();
        let effect = app.world().get::<SequencePlayer>(player).unwrap().spawned[0].unwrap();
        assert_eq!(app.world().get::<ChildOf>(effect).unwrap().parent(), joint,
            "deferred attachment must use the real joint, not the actor-root fallback");

        app.world_mut().entity_mut(root).insert(crate::actors::npc::NpcVisualPending);
        app.world_mut().get_mut::<SequencePlayer>(player).unwrap().stop = true;
        app.update();
        assert!(app.world().get_entity(player).is_err() && app.world().get_entity(effect).is_err(),
            "status expiry must still clean up while visual readiness is pending");

        // The status's separate end burst has no StatusVisual deadline owner.
        // It must still end entirely while absent, without a deferred late FX.
        let sequence = Sequence { length: 1, repeat: false,
            tracks: vec![Track::Fx { graphics: "tests/npc_residency_status_end".into(), joint: "Hips".into(),
                at: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0], start: 0, end: 1, curves: HashMap::new() }],
        };
        let expired = app.world_mut().run_system_once(move |mut commands: Commands|
            play(&mut commands, sequence.clone(), Some(root), root, false)).unwrap();
        app.update();
        assert!(app.world().get_entity(expired).is_err(), "unloaded one-shot/end bursts must not replay on a later return");
        assert_eq!(app.world_mut().query::<&FxModel>().iter(app.world()).count(), 0);

        let sequence = Sequence { length: 100, repeat: true, tracks: Vec::new() };
        let discarded = app.world_mut().run_system_once(move |mut commands: Commands|
            play(&mut commands, sequence.clone(), Some(root), root, true)).unwrap();
        app.world_mut().despawn(root);
        app.update();
        assert!(app.world().get_entity(discarded).is_err(), "discard must not leave a paused sequence behind");
    }

    #[test]
    fn status_animation_received_without_a_rig_expires_before_reentry() {
        let mut app = App::new();
        app.insert_resource(Time::<()>::default()).init_resource::<StatusHolds>()
            .add_systems(Update, hold_for_status);
        let root = app.world_mut().spawn((CharacterAnim::default(), crate::actors::npc::NpcVisualPending)).id();
        app.world_mut().resource_mut::<StatusHolds>().0.push((root, "Stunned".into(), 1.0));
        app.update();
        assert_eq!(app.world().get::<CharacterAnim>(root).unwrap().state, AnimState::Named("Stunned".into()));
        assert!(app.world().get::<Character>(root).is_none());
        app.world_mut().resource_mut::<Time>().advance_by(Duration::from_secs(2));
        app.update();
        app.world_mut().run_system_once(move |mut commands: Commands|
            crate::actors::character::attach_character(&mut commands, root, crate::actors::character::CharacterDesc::default())).unwrap();
        assert_eq!(app.world().get::<CharacterAnim>(root).unwrap().state, AnimState::Idle,
            "returning visuals must not replay an expired status");
    }
}

#[cfg(test)]
mod bullet_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn remote_cast_preserves_server_stop_and_uses_motion_unlock_frame() {
        for motion_unblock in [0, 10] {
            let mut app = App::new();
            app.init_resource::<Time<Real>>().init_resource::<Assets<SkillTable>>()
                .init_resource::<Assets<SequenceTable>>();
            crate::net::start_offline(app.world_mut(), 1, 0);
            let skill = serde_json::from_value(serde_json::json!({
                "id": "test_attack", "type": "Melee", "hit_frame": 1, "loop_start": 0,
                "unblock": 50, "motion_unblock": motion_unblock, "range": 2.0,
                "cooldown": 0.0, "execute": {}, "impact": ""
            })).unwrap();
            let skills = app.world_mut().resource_mut::<Assets<SkillTable>>().add(SkillTable([(1, skill)].into_iter().collect()));
            let sequences = app.world_mut().resource_mut::<Assets<SequenceTable>>().add(SequenceTable(default()));
            app.insert_resource(SkillData { skills, sequences, status: default(), by_id: default() });
            let stop = Vec3::new(2., 0., 1.);
            let root = app.world_mut().spawn((Transform::default(), CharacterAnim::default(),
                RemotePlayer { actor: 41, target: stop, last_record: Some((stop, 100)), ..default() })).id();
            app.world_mut().non_send_mut::<Net>().skill_events.push(crate::net::SkillEvent {
                actor: 41, wire: 1, heading: 0., points: Vec::new(), target: None, start_tick: 100, phase_frame: None,
            });
            app.world_mut().run_system_once(remote_skills).unwrap();
            let r = app.world().get::<RemotePlayer>(root).unwrap();
            assert_eq!(r.target, stop, "a cast must not replace the server stop with an interpolated position");
            assert_eq!(r.held, motion_unblock as f32 / FPS);
            assert_eq!(r.held_until_tick, Some(100 + motion_unblock));
        }
    }

    fn bullet(start: Vec3, velocity: Vec3, radius: f32) -> Bullet {
        Bullet {
            start,
            velocity,
            homing: false,
            t: 0.0,
            end_t: 10.0,
            lifetime: 10.0,
            height: 1.49,
            death: String::new(),
            impact: String::new(),
            to_point: false,
            static_collision: false,
            radius,
            caster: Entity::PLACEHOLDER,
            from_monster: false,
            target: None,
            dying: None,
            caster_actor: None,
            bullet_impact: String::new(),
            impacts_left: 1,
            passed: Vec::new(),
            snapped: None,
        }
    }

    #[test]
    fn projectiles_pass_zero_hp_dead_and_disposed_targets_without_hit_triggers() {
        for state in 0..3 {
            let mut app = App::new();
            app.init_resource::<Time>().init_resource::<Time<Real>>()
                .init_resource::<Assets<NavMesh>>().init_resource::<crate::monsters::DebugMonsters>();
            crate::net::start_offline(app.world_mut(), 1, 0);
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs(1));
            if state < 2 {
                let mut anim = CharacterAnim::default();
                anim.dead = state == 1;
                app.world_mut().spawn((crate::monsters::tests::monster(41, if state == 0 { 0 } else { 100 }),
                    anim, GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 2.0))));
            }
            app.world_mut().spawn((crate::monsters::tests::monster(42, 100), CharacterAnim::default(),
                GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 6.0))));
            let mut b = bullet(Vec3::new(0.0, 1.0, 0.0), Vec3::Z * 10.0, 0.3);
            b.homing = true;
            b.target = Some(41);
            b.snapped = Some(41);
            b.end_t = 0.2;
            b.caster_actor = Some(10);
            let projectile = app.world_mut().spawn((b, Transform::default())).id();
            app.world_mut().non_send_mut::<Net>().strikes.push((10, 41));
            app.world_mut().run_system_once(fly_bullets).unwrap();
            assert_eq!(app.world().resource::<crate::monsters::DebugMonsters>().blows, [(42, 0)],
                "neither collision, homing nor a late server hit may retrigger the corpse");
            assert!(app.world().get_entity(projectile).is_err(), "the live target behind it still receives the projectile");
        }
    }

    #[test]
    fn a_friendly_summons_homing_missile_ends_on_the_owner_it_homes_on() {
        // The Tactical Turret's support missile at its owner: it lingered, unseen,
        // inside him for its whole LifeTime instead of striking him.
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<Time<Real>>()
            .init_resource::<Assets<NavMesh>>().init_resource::<crate::monsters::DebugMonsters>();
        crate::net::start_offline(app.world_mut(), 1, 0);
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
        app.world_mut().run_system_once(|mut commands: Commands| {
            let e = crate::actors::character::spawn_character(&mut commands, default(), Transform::IDENTITY);
            commands.entity(e).insert((GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 0.3)), { let mut l = LocalPlayer::default(); l.actor = 7; l }));
        }).unwrap();
        let mut b = bullet(Vec3::new(0.0, 1.0, 0.0), Vec3::Z * 4.0, 0.1);
        b.homing = true;
        b.target = Some(7);
        let missile = app.world_mut().spawn((b, Transform::default())).id();
        app.world_mut().run_system_once(fly_bullets).unwrap();
        assert!(app.world().get_entity(missile).is_err(), "the missile ends on its owner");
        assert!(app.world().resource::<crate::monsters::DebugMonsters>().blows.is_empty(), "a support missile is no blow");
    }

    #[test]
    fn pending_actions_are_cancelled_when_the_caster_dies_or_is_disposed() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<Assets<SequenceTable>>();
        let table = app.world_mut().resource_mut::<Assets<SequenceTable>>().add(SequenceTable(HashMap::new()));
        app.insert_resource(SkillData::for_test(table));
        let mut anim = CharacterAnim::default();
        anim.dead = true;
        let caster = app.world_mut().spawn(anim).id();
        for disposed in [false, true] {
            if disposed { app.world_mut().despawn(caster); }
            let action = app.world_mut().spawn(Pending { after: 10.0, what: PendingKind::Teleport { who: caster, to: Vec3::X } }).id();
            app.world_mut().run_system_once(run_pending).unwrap();
            assert!(app.world().get_entity(action).is_err());
        }
    }

    #[test]
    fn monster_projectile_keeps_its_collision_side_after_caster_disposal() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<Assets<NavMesh>>()
            .init_resource::<crate::monsters::DebugMonsters>();
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs(1));
        app.world_mut().spawn((crate::monsters::tests::monster(42, 100), CharacterAnim::default(),
            GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 2.0))));
        let mut b = bullet(Vec3::new(0.0, 1.0, 0.0), Vec3::Z * 10.0, 0.3);
        b.from_monster = true;
        let projectile = app.world_mut().spawn((b, Transform::default())).id();
        app.world_mut().run_system_once(fly_bullets).unwrap();
        assert!(app.world().get_entity(projectile).is_ok(), "a dead caster's projectile must still pass through monsters");
    }

    #[test]
    fn a_bullet_flies_through_our_shadow_soul_into_the_monster_behind_it() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<Time<Real>>()
            .init_resource::<Assets<NavMesh>>().init_resource::<crate::monsters::DebugMonsters>();
        crate::net::start_offline(app.world_mut(), 1, 0);
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs(1));
        let mut soul = crate::monsters::tests::monster(41, 100);
        soul.targetable = false;
        app.world_mut().spawn((soul, CharacterAnim::default(), GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 2.0))));
        app.world_mut().spawn((crate::monsters::tests::monster(42, 100), CharacterAnim::default(),
            GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 6.0))));
        let projectile = app.world_mut().spawn((bullet(Vec3::new(0.0, 1.0, 0.0), Vec3::Z * 10.0, 0.3), Transform::default())).id();
        app.world_mut().run_system_once(fly_bullets).unwrap();
        assert_eq!(app.world().resource::<crate::monsters::DebugMonsters>().blows, [(42, 0)]);
        assert!(app.world().get_entity(projectile).is_err());
    }

    #[test]
    fn a_shadow_souls_arrow_strikes_monsters_and_a_pvp_enemys_strikes_players() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<Assets<SequenceTable>>();
        let table = app.world_mut().resource_mut::<Assets<SequenceTable>>().add(SequenceTable(HashMap::new()));
        app.insert_resource(SkillData::for_test(table));
        let def: BulletDef = serde_json::from_str(r#"{"loop":"","impact":"","death":"","motion":"Straight",
            "velocity":22.0,"lifetime":0.86,"radius":0.2,"start":[0.0,1.5,0.0]}"#).unwrap();
        for (targetable, from_monster) in [(false, false), (true, true)] {
            let mut soul = crate::monsters::tests::monster(0x10600, 100);
            soul.targetable = targetable;
            let caster = app.world_mut().spawn((soul, CharacterAnim::default())).id();
            app.world_mut().spawn(Pending { after: 0.0, what: PendingKind::Bullet { from: Vec3::Y, dir: Vec3::Z, def: def.clone(),
                impact: String::new(), to: None, caster, caster_actor: Some(0x10600), aim: None, target: None } });
            app.world_mut().run_system_once(run_pending).unwrap();
            let mut bullets = app.world_mut().query::<(Entity, &Bullet)>();
            let (e, b) = bullets.single(app.world()).unwrap();
            assert_eq!(b.from_monster, from_monster, "a soul that is {}", if targetable { "hostile" } else { "ours" });
            app.world_mut().despawn(e);
        }
    }

    #[test]
    fn late_status_cannot_revive_a_dead_monster_or_replace_its_death_pose() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<StatusHolds>();
        let mut anim = CharacterAnim::default();
        anim.dead = true;
        anim.state = AnimState::Death;
        let root = app.world_mut().spawn((crate::monsters::tests::monster(41, 0), anim)).id();
        app.world_mut().resource_mut::<StatusHolds>().0.push((root, "Stunned".into(), 5.0));
        app.world_mut().run_system_once(hold_for_status).unwrap();
        let anim = app.world().get::<CharacterAnim>(root).unwrap();
        assert!(anim.dead);
        assert_eq!(anim.state, AnimState::Death);
    }

    #[test]
    fn homing_turns_towards_a_moving_target_without_teleporting_or_extending_lifetime() {
        let mut b = bullet(Vec3::new(0.0, 1.49, 0.0), Vec3::new(0.0, 0.0, 12.0), 0.5);
        b.homing = true;
        b.t = 0.25;
        let before = b.start + b.velocity * b.t;
        steer_bullet(&mut b, Vec3::new(6.0, 2.0, 9.0));
        assert!((b.start + b.velocity * b.t - before).length() < 1e-5);
        assert!((b.velocity.xz().length() - 12.0).abs() < 1e-5);
        assert!(b.velocity.x > 0.0);
        b.t += 0.1;
        let before = b.start + b.velocity * b.t;
        let moved = Vec3::new(-6.0, 3.0, 9.0);
        steer_bullet(&mut b, moved);
        assert!((b.start + b.velocity * b.t - before).length() < 1e-5);
        assert!((b.velocity.normalize() - (moved - before).normalize()).length() < 1e-5);
        assert_eq!(b.end_t, 10.0);
        // Arriving exactly at the target must not produce NaNs.
        steer_bullet(&mut b, before);
        assert!(b.velocity.is_finite() && b.start.is_finite());
    }

    #[test]
    fn sweep_meets_a_capsule_the_tick_positions_step_over() {
        // A 21 units/s missile at 20 frames a second moves 1.05 a frame: neither
        // tick position is within 0.6 of a capsule standing 0.5 beside the line,
        // halfway along the step, which the flight still crosses.
        let s = sweep_contact(Vec2::ZERO, Vec2::new(0.0, 1.05), Vec2::new(0.5, 0.5), 0.6).expect("met");
        assert!(s > 0.0 && s < 1.0, "{s}");
        // Beside the line by more than the radii: missed.
        assert!(sweep_contact(Vec2::ZERO, Vec2::new(0.0, 1.05), Vec2::new(0.7, 0.5), 0.6).is_none());
        // Started inside it (point blank): met at once.
        assert_eq!(sweep_contact(Vec2::ZERO, Vec2::new(0.0, 1.0), Vec2::new(0.2, 0.1), 0.6), Some(0.0));
        // Behind, or beyond this frame's flight: not yet.
        assert!(sweep_contact(Vec2::ZERO, Vec2::new(0.0, 1.0), Vec2::new(0.0, -2.0), 0.6).is_none());
        assert!(sweep_contact(Vec2::ZERO, Vec2::new(0.0, 1.0), Vec2::new(0.0, 2.0), 0.6).is_none());
    }

    #[test]
    fn a_server_blow_ends_the_bullet_on_its_victims_surface() {
        let b = bullet(Vec3::new(0.0, 1.49, 0.0), Vec3::new(0.0, 0.0, 12.0), 0.5);
        // A monster 6 ahead with a 0.3 capsule: met 5.2 along, at 12 units/s.
        let t = flight_time_to(&b, Vec3::new(0.0, 0.0, 6.0), 0.3);
        assert!((t - 5.2 / 12.0).abs() < 1e-5, "{t}");
        let (aside, along) = beside_line(&b, Vec3::new(1.0, 0.0, 6.0));
        assert!((aside - 1.0).abs() < 1e-5 && (along - 6.0).abs() < 1e-5);
        // Already passed: it ends now.
        assert_eq!(flight_time_to(&b, Vec3::new(0.0, 0.0, -1.0), 0.3), 0.0);
    }
}
