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

use crate::character::{AnimState, Character, CharacterAnim};
use crate::nav::{CurrentNav, NavMesh};
use crate::net::{cursor_ground, now_ms, LocalPlayer, Net, RemotePlayer};

/// The sequencer's frame rate.
const FPS: f32 = 25.0;
/// A Nebula entity looks down its -Z; our character models look down +Z (players
/// turn with from_rotation_y(atan2(dx, dz))). Everything a sequence places "on the
/// entity" (effects, lights, bullet start offsets) is in the entity's frame.
/// EVIDENCE: mage_fireball_bullet's StartOffset is (0.043, 1.49, -1.788): the hand,
///   1.8 units AHEAD, on -Z; the skill command's aim is the facing less half a turn
///   (dsor/combat.py); the NPCs' level matrices face -Z (crate::npc).
/// FAILURE (2026-10-06): without it the fireball's trail streamed out in front of
///   it and the cast effects sat behind the caster ("pas affiche au bon endroit").
fn entity_rotation(facing: f32) -> Quat {
    Quat::from_rotation_y(facing + std::f32::consts::PI)
}

#[derive(Deserialize, Debug, Clone)]
pub struct BulletDef {
    #[serde(rename = "loop")]
    pub loop_seq: String,
    /// The impact sequence on whatever it hits, its flight (Straight, Homing,
    /// Orbiting...) and its hit radius: for the HitCommand handling that is not
    /// written yet (docs/fx.md); bullets fly straight until their death sequence.
    #[allow(dead_code)]
    pub impact: String,
    pub death: String,
    #[allow(dead_code)]
    pub motion: String,
    pub velocity: f32,
    pub lifetime: f32,
    #[allow(dead_code)]
    pub radius: f32,
    /// Where it leaves, in the caster's entity frame.
    pub start: [f32; 3],
}

#[derive(Deserialize, Debug, Clone)]
pub struct SkillDef {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub hit_frame: u32,
    pub loop_start: u32,
    pub unblock: u32,
    pub motion_unblock: u32,
    pub range: f32,
    pub cooldown: f32,
    #[serde(default)]
    pub cooldown_category: String,
    #[serde(default)]
    pub resource_cost: f32,
    pub execute: HashMap<String, String>,
    pub impact: String,
    pub bullet: Option<BulletDef>,
}

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
    Sound { start: i32, end: i32 },
}

impl Track {
    fn span(&self) -> (i32, i32) {
        match self {
            Track::Anim { start, end, .. }
            | Track::Fx { start, end, .. }
            | Track::Phase { start, end }
            | Track::Light { start, end, .. }
            | Track::Shake { start, end, .. }
            | Track::Hide { start, end }
            | Track::Sound { start, end } => (*start, *end),
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
    pub start: HashMap<String, String>,
    pub tick: HashMap<String, String>,
    pub done: HashMap<String, String>,
    pub stop: HashMap<String, String>,
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

/// One sequence playing: on an actor (its animation, effects that follow it) or on
/// its own anchor entity (a bullet, a point on the ground).
#[derive(Component)]
pub struct SequencePlayer {
    seq: Sequence,
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
    /// A newer skill sequence took over the same actor's animation: this one
    /// neither starts nor ends an animation any more.
    superseded: bool,
    /// Per track: started, and what it spawned.
    started: Vec<bool>,
    spawned: Vec<Option<Entity>>,
    /// Per track: what its local placement is multiplied by (the world placement
    /// it was left at, or the character's entity frame), for animated placements.
    bases: Vec<Mat4>,
}

/// A bullet in flight.
#[derive(Component)]
struct Bullet {
    velocity: Vec3,
    left: f32,
    death: String,
    /// Once it has hit or run out: seconds its particles still have to fade.
    dying: Option<f32>,
}


/// A caster carried by its skill (Jump, Charge): from `from` to `to` between the
/// skill's LoopStartFrame and HitFrame, in an arc for a jump.
/// UNVERIFIED: the arc's height; the frames bounding the flight.
#[derive(Component)]
struct SkillMove {
    from: Vec3,
    to: Vec3,
    delay: f32,
    t: f32,
    duration: f32,
    arc: f32,
}

/// A jump's peak height over the straight line, units.
const JUMP_ARC: f32 = 1.8;

/// Something to do a little later in a skill (a bullet leaving the hand, an impact).
#[derive(Component)]
struct Pending {
    after: f32,
    what: PendingKind,
}

enum PendingKind {
    Bullet { from: Vec3, dir: Vec3, def: BulletDef },
    /// The caster lands on the aimed point (Teleport skills).
    Teleport { who: Entity, to: Vec3 },
    Sequence { name: String, at: Transform },
}

/// An effect model: its node animation is started once it is in.
#[derive(Component)]
struct FxModel {
    gltf: Handle<Gltf>,
    started: bool,
    /// Its node animation repeats only in a looping sequence (a bullet in flight);
    /// otherwise it plays once. Repeated, frost nova's ring burst twice within its
    /// track and the teleport's arrival four or five times.
    looping: bool,
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

impl Plugin for SkillsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<SkillTable>()
            .init_asset::<SequenceTable>()
            .init_asset::<StatusTable>()
            .register_asset_loader(StatusTableLoader)
            .register_asset_loader(SkillTableLoader)
            .register_asset_loader(SequenceTableLoader)
            .init_resource::<CameraShake>()
            .init_resource::<TestCastClock>()
            .add_systems(Startup, load)
            .add_systems(
                Update,
                (index_skills, cast_input, remote_skills, status_visuals, test_cast, run_pending, carry_casters, fly_bullets, play_sequences, start_fx_animations, retire_effects, shake_camera, test_shot, test_view, debug_fx_positions)
                    .chain()
                    .after(crate::net::NetSystems),
            );
    }
}

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
/// aimed at `point`.
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
) {
    let column = character.map(weapon_column).unwrap_or_else(|| "empty".into());
    if let Some(seq) = skill.execute_for(&column).and_then(|n| sequences.0.get(n)) {
        play(commands, seq.clone(), Some(actor), actor, false);
    }
    let dir = Quat::from_rotation_y(facing) * Vec3::Z;
    let fire = skill.loop_start.max(skill.hit_frame) as f32 / FPS;
    match (skill.kind.as_str(), &skill.bullet) {
        ("Ranged" | "RangedTargetPoint" | "RangedTarget", Some(b)) => {
            // The bullet leaves from the casting hand: StartOffset's height, a short
            // way ahead. Its full 1.8 units ahead left a gap between the caster and
            // the trail ("la trainee commence trop loin de moi").
            // UNVERIFIED: how the client applies StartOffset's forward component.
            let mut start = Vec3::from(b.start);
            start.z = start.z.clamp(-0.6, 0.6);
            commands.spawn(Pending {
                after: fire,
                what: PendingKind::Bullet { from: at + entity_rotation(facing) * start, dir, def: b.clone() },
            });
        }
        ("Shifted", Some(b)) => {
            // A bolt falling on the aimed point (lightning strike, meteor).
            commands.spawn(Pending {
                after: fire,
                what: PendingKind::Sequence { name: b.loop_seq.clone(), at: Transform::from_translation(point).with_rotation(entity_rotation(facing)) },
            });
        }
        _ => {}
    }
    if matches!(skill.kind.as_str(), "Jump" | "Charge" | "ChargeThrough") {
        let start = skill.loop_start as f32 / FPS;
        let end = (skill.hit_frame.max(skill.loop_start + 1)) as f32 / FPS;
        commands.entity(actor).insert(SkillMove {
            from: at,
            to: point,
            delay: start,
            t: 0.0,
            duration: (end - start).max(0.05),
            arc: if skill.kind == "Jump" { JUMP_ARC } else { 0.0 },
        });
    }
    if skill.kind == "Teleport" {
        commands.spawn(Pending { after: skill.hit_frame as f32 / FPS, what: PendingKind::Teleport { who: actor, to: point } });
    }
    if !skill.impact.is_empty() && skill.impact != "empty_sequence" {
        let impact_at = match skill.kind.as_str() {
            "Shifted" | "Teleport" | "Jump" | "RangedTargetPoint" => point,
            "Ranged" => point,
            _ => at,
        };
        let after = match skill.kind.as_str() {
            "Ranged" => fire + skill.bullet.as_ref().map(|b| (point - at).length().min(b.velocity * b.lifetime) / b.velocity.max(0.1)).unwrap_or(0.0),
            _ => skill.hit_frame as f32 / FPS,
        };
        // Ranged impacts are drawn when the bullet dies (fly_bullets); the others here.
        if skill.kind != "Ranged" {
            commands.spawn(Pending {
                after,
                what: PendingKind::Sequence {
                    name: skill.impact.clone(),
                    at: Transform::from_translation(impact_at).with_rotation(entity_rotation(facing)),
                },
            });
        }
    }
}

fn play(commands: &mut Commands, seq: Sequence, actor: Option<Entity>, anchor: Entity, looping: bool) -> Entity {
    // A caster's own skill sequence leaves its effects in place; anything else
    // (a bullet, a ground effect) carries them.
    let follow = actor != Some(anchor);
    play_with(commands, seq, actor, anchor, looping, follow)
}

fn play_with(commands: &mut Commands, seq: Sequence, actor: Option<Entity>, anchor: Entity, looping: bool, follow: bool) -> Entity {
    let n = seq.tracks.len();
    commands
        .spawn(SequencePlayer { seq, t: 0.0, actor, anchor, looping, follow, stop: false, superseded: false, started: vec![false; n], spawned: vec![None; n], bases: vec![Mat4::IDENTITY; n] })
        .id()
}

#[allow(clippy::too_many_arguments)]
fn cast_input(
    mut commands: Commands,
    net: Option<NonSendMut<Net>>,
    data: Res<SkillData>,
    tables: Res<Assets<SkillTable>>,
    seqs: Res<Assets<SequenceTable>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    nav: Option<Res<CurrentNav>>,
    navmeshes: Res<Assets<NavMesh>>,
    time: Res<Time<Real>>,
    mut players: Query<(Entity, &Transform, &mut LocalPlayer, &Character)>,
    mut cooldowns: Local<HashMap<String, f64>>,
) {
    let Some(mut net) = net else { return };
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    // One press, one cast (holding a key re-fired the skill as soon as it
    // unblocked: "certains sorts se lancent 2x"); only the basic attack on
    // Shift + left button repeats while held.
    let slot = if shift && buttons.pressed(MouseButton::Left) {
        Some(0)
    } else if buttons.just_pressed(MouseButton::Right) {
        Some(1)
    } else {
        [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5]
            .iter()
            .position(|k| keys.just_pressed(*k))
            .map(|i| i + 2)
    };
    let Some(slot) = slot else { return };
    let Ok((entity, tf, mut player, character)) = players.single_mut() else { return };
    if player.casting > 0.0 {
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
    let key = if skill.cooldown_category.is_empty() { skill.id.clone() } else { skill.cooldown_category.clone() };
    if cooldowns.get(&key).is_some_and(|ready| now < *ready) {
        return;
    }
    if net.resource.is_some_and(|r| r + 1e-3 < skill.resource_cost) {
        return;
    }
    let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), cameras.single()) else { return };
    let mesh = nav.as_ref().and_then(|n| navmeshes.get(&n.0));
    let Some(mut point) = cursor_ground(window, camera, cam_tf, mesh, tf.translation) else { return };
    let to = Vec2::new(point.x - tf.translation.x, point.z - tf.translation.z);
    // A point skill reaches no farther than its range.
    if skill.range > 0.0 && to.length() > skill.range {
        let p = to.normalize() * skill.range;
        point = Vec3::new(tf.translation.x + p.x, point.y, tf.translation.z + p.y);
    }
    let facing = if to.length() > 0.01 { to.x.atan2(to.y) } else { player.facing };
    player.facing = facing;
    player.target = None;
    player.casting = (skill.motion_unblock.max(skill.unblock).max(1) as f32 / FPS).min(2.0);

    let tick = net.server_tick(now_ms(&time));
    let base = SkillBase { high_water: 0, skill_id: wire as u16, heading: facing_to_aim(facing), start_tick: tick, unknown_u32_0: 0 };
    let dir = Quat::from_rotation_y(facing) * Vec3::Z;
    let speed = skill.bullet.as_ref().map(|b| b.velocity / FPS).unwrap_or(1.0);
    let step = [dir.x * speed, 0.0, dir.z * speed, 0.0];
    let points = vec![point.to_array()];
    let command = match skill.kind.as_str() {
        "MeleeTarget" | "RangedTarget" => ClientCommand::TargetSkill(combat::TargetSkill { base, target: u32::MAX, server: None }),
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
    cooldowns.insert(key, now + skill.cooldown as f64);
    if let Some(r) = net.resource.as_mut() {
        *r -= skill.resource_cost;
    }
    info!("cast {} (wire {wire}, {}) at {point:?}", skill.id, skill.kind);
    perform(&mut commands, skill, sequences, entity, Some(character), tf.translation, facing, point);
}

/// Skills other players used, as the server relays them.
#[allow(clippy::too_many_arguments)]
fn remote_skills(
    mut commands: Commands,
    net: Option<NonSendMut<Net>>,
    data: Res<SkillData>,
    tables: Res<Assets<SkillTable>>,
    seqs: Res<Assets<SequenceTable>>,
    mut remotes: Query<(Entity, &mut Transform, &mut RemotePlayer, &Character)>,
) {
    let Some(mut net) = net else { return };
    if net.skill_events.is_empty() {
        return;
    }
    let (Some(table), Some(sequences)) = (tables.get(&data.skills), seqs.get(&data.sequences)) else { return };
    for ev in std::mem::take(&mut net.skill_events) {
        let Some(skill) = table.0.get(&(ev.wire as u32)) else { continue };
        let Some((e, mut tf, mut r, character)) = remotes.iter_mut().find(|(_, _, r, _)| r.actor == ev.actor) else { continue };
        let facing = aim_to_facing(ev.heading);
        r.facing = facing;
        tf.rotation = Quat::from_rotation_y(facing);
        let point = ev
            .points
            .first()
            .map(|p| Vec3::from(*p))
            .unwrap_or(tf.translation + Quat::from_rotation_y(facing) * Vec3::Z * skill.range.min(15.0));
        perform(&mut commands, skill, sequences, e, Some(character), tf.translation, facing, point);
    }
}

fn run_pending(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<SkillData>,
    seqs: Res<Assets<SequenceTable>>,
    mut pending: Query<(Entity, &mut Pending)>,
    mut movers: Query<&mut Transform, With<Character>>,
) {
    let Some(sequences) = seqs.get(&data.sequences) else { return };
    for (e, mut p) in &mut pending {
        p.after -= time.delta_secs();
        if p.after > 0.0 {
            continue;
        }
        commands.entity(e).despawn();
        match &p.what {
            PendingKind::Bullet { from, dir, def } => {
                let bullet = commands
                    .spawn((
                        Bullet { velocity: *dir * def.velocity, left: def.lifetime.max(0.05), death: def.death.clone(), dying: None },
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
            PendingKind::Sequence { name, at } => {
                if let Some(seq) = sequences.0.get(name) {
                    let anchor = commands.spawn((*at, Visibility::default())).id();
                    play(&mut commands, seq.clone(), None, anchor, false);
                }
            }
        }
    }
}

fn fly_bullets(
    mut commands: Commands,
    time: Res<Time>,
    mut bullets: Query<(Entity, &mut Transform, &mut Bullet)>,
    children: Query<&Children>,
    shown_parts: Query<(), Or<(With<Mesh3d>, With<PointLight>)>>,
    emitters: Query<(), With<crate::particles::Emitter>>,
    nav: Option<Res<CurrentNav>>,
    navmeshes: Res<Assets<NavMesh>>,
) {
    let mesh = nav.as_ref().and_then(|n| navmeshes.get(&n.0));
    for (e, mut tf, mut b) in &mut bullets {
        let dt = time.delta_secs();
        if let Some(left) = b.dying.as_mut() {
            *left -= dt;
            if *left <= 0.0 {
                commands.entity(e).despawn();
            }
            continue;
        }
        tf.translation += b.velocity * dt;
        b.left -= dt;
        // The ground rising into its path (stairs, a slope) stops it.
        // UNVERIFIED: the server's own collision; walls off the navigation mesh
        // are not detected.
        let p = tf.translation;
        let hit_ground = mesh.is_some_and(|m| m.heights(p.x, p.z).any(|h| h > p.y - 0.2 && h < p.y + 2.5));
        if hit_ground {
            b.left = 0.0;
        }
        if b.left <= 0.0 {
            if !b.death.is_empty() {
                commands.spawn(Pending {
                    after: 0.0,
                    what: PendingKind::Sequence { name: b.death.clone(), at: Transform::from_translation(tf.translation).with_rotation(tf.rotation) },
                });
            }
            // The bullet and everything its loop sequence drew go at once (the
            // client's track exit removes graphics entities); its death sequence
            // takes over where it ended.
            let _ = (&children, &shown_parts, &emitters);
            commands.entity(e).despawn();
        }
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
const LIGHT_LUMENS: f32 = 40_000.0;

/// An effect transform from a track: translation, rotation in degrees, scale.
fn track_transform(at: &[f32; 9]) -> Transform {
    Transform {
        translation: Vec3::new(at[0], at[1], at[2]),
        rotation: Quat::from_euler(EulerRot::YXZ, at[4].to_radians(), at[3].to_radians(), at[5].to_radians()),
        scale: Vec3::new(at[6], at[7], at[8]),
    }
}

#[allow(clippy::too_many_arguments)]
fn play_sequences(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    mut players: Query<(Entity, &mut SequencePlayer)>,
    exists: Query<()>,
    characters: Query<&Character>,
    mut anims: Query<&mut CharacterAnim>,
    mut visibility: Query<&mut Visibility>,
    transforms: Query<&GlobalTransform>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut shake: ResMut<CameraShake>,
    locals: Query<&LocalPlayer>,
    remotes: Query<&RemotePlayer>,
    mut point_lights: Query<(&mut PointLight, &mut Transform), Without<Character>>,
) {
    let eye = cameras.iter().next().map(|c| c.translation());
    let mut lights_to_set: Vec<(Entity, f32, f32, Transform)> = Vec::new();
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
        // Walking away interrupts the skill, as in the game: its effects go with it
        // (they stayed on the running player for seconds otherwise).
        let interrupted = p.actor.is_some_and(|a| {
            locals.get(a).is_ok_and(|l| l.target.is_some() && l.casting <= 0.0)
                || remotes.get(a).is_ok_and(|r| r.still_for <= 0.0)
        }) && p.t > 0.1;
        let anchor_alive = exists.contains(p.anchor) && !interrupted;
        p.t += time.delta_secs();
        let mut frame = p.t * FPS;
        let length = p.seq.length.max(1) as f32;
        if p.looping && anchor_alive && frame > length {
            p.t %= length / FPS;
            frame = p.t * FPS;
            for i in 0..p.started.len() {
                if let Some(s) = p.spawned[i].take() {
                    commands.entity(s).try_despawn();
                }
                p.started[i] = false;
            }
        }
        let done = p.stop || !anchor_alive || (!p.looping && frame > length);
        for i in 0..p.seq.tracks.len() {
            let track = p.seq.tracks[i].clone();
            let (start, end) = track.span();
            let ending = done || frame >= end as f32;
            if !p.started[i] && frame >= start as f32 && !ending {
                p.started[i] = true;
                match &track {
                    Track::Anim { name, speed, .. } if !p.superseded => {
                        if let Some(Ok(mut a)) = p.actor.map(|a| anims.get_mut(a)) {
                            a.state = AnimState::Named(name.clone());
                            a.speed = *speed;
                            // The same skill again restarts its animation.
                            a.replay();
                        }
                    }
                    Track::Fx { graphics, joint, at, .. } => {
                        let parent = if joint.is_empty() {
                            Some(p.anchor)
                        } else {
                            p.actor.and_then(|a| characters.get(a).ok()).and_then(|c| c.bone(joint)).or(Some(p.anchor))
                        };
                        let path = format!("{graphics}.glb");
                        // On a character (not a joint): into its entity frame.
                        let mut place = track_transform(&curved(at, &curves_of(&track), "graphicstrans", frame));
                        let mut base = Mat4::IDENTITY;
                        if joint.is_empty() && characters.contains(p.anchor) {
                            base = Transform::from_rotation(entity_rotation(0.0)).to_matrix();
                            place = Transform::from_matrix(base * place.to_matrix());
                        }
                        let mut fx = commands.spawn((
                            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path.clone()))),
                            FxModel { gltf: assets.load(path), started: false, looping: p.looping },
                            place,
                            Visibility::default(),
                        ));
                        let unattached = joint.is_empty() && !p.follow;
                        if unattached {
                            // Left where it was made: the anchor's world placement now.
                            if let Ok(g) = transforms.get(p.anchor) {
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
                                PointLight {
                                    color: Color::srgb(color[0], color[1], color[2]),
                                    // UNVERIFIED: Nebula's intensity (0..10) to lumens.
                                    intensity: intensity * LIGHT_LUMENS,
                                    range: *range,
                                    shadow_maps_enabled: false,
                                    ..default()
                                },
                                if characters.contains(p.anchor) {
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
                    }
                    Track::Phase { .. } | Track::Sound { .. } | Track::Anim { .. } => {}
                }
            }
            // Animated values (bezier curves over the sequence's frames).
            if let (true, Some(spawned)) = (p.started[i] && !ending, p.spawned[i]) {
                match &track {
                    Track::Fx { at, curves, .. } if !curves.is_empty() => {
                        let place = track_transform(&curved(at, curves, "graphicstrans", frame));
                        commands.entity(spawned).try_insert(Transform::from_matrix(p.bases[i] * place.to_matrix()));
                    }
                    Track::Light { intensity, range, at, curves, .. } if !curves.is_empty() => {
                        let k = eval_curve(curves.get("intensity"), frame).unwrap_or(*intensity);
                        let r = eval_curve(curves.get("range"), frame).unwrap_or(*range);
                        let place = track_transform(&curved(at, curves, "lighttrans", frame));
                        lights_to_set.push((spawned, k, r, place));
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
                            if a.state == AnimState::Named(name.clone()) {
                                a.state = AnimState::Idle;
                                a.speed = 1.0;
                            }
                        }
                    }
                    Track::Hide { .. } => {
                        if let Some(Ok(mut v)) = p.actor.map(|a| visibility.get_mut(a)) {
                            *v = Visibility::Inherited;
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
    for (light, k, r, place) in lights_to_set {
        if let Ok((mut l, mut tf)) = point_lights.get_mut(light) {
            l.intensity = k * LIGHT_LUMENS;
            l.range = r;
            *tf = place;
        }
    }
}

/// Effect models: their node animation (the "-loop" clip) plays once they are in.
fn start_fx_animations(
    mut commands: Commands,
    mut fx: Query<(Entity, &mut FxModel)>,
    gltfs: Res<Assets<Gltf>>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    for (e, mut m) in &mut fx {
        if m.started {
            continue;
        }
        if std::env::var("DSOR_FX_DEBUG").is_ok() {
            info!("fx {:?} pending, loaded: {}", m.gltf.path(), gltfs.get(&m.gltf).is_some());
        }
        let Some(gltf) = gltfs.get(&m.gltf) else { continue };
        if !m.started && std::env::var("DSOR_FX_DEBUG").is_ok() {
            let n = children.iter_descendants(e).count();
            info!("fx {:?}: {} descendants, {} with a player, {} clips", m.gltf.path(), n, children.iter_descendants(e).filter(|c| players.contains(*c)).count(), gltf.animations.len());
        }
        if gltf.animations.is_empty() {
            m.started = true;
            continue;
        }
        let Some(player) = children.iter_descendants(e).find(|c| players.contains(*c)) else { continue };
        debug!("fx {:?}: playing {} clip(s)", m.gltf.path(), gltf.animations.len());
        let (graph, node) = AnimationGraph::from_clip(gltf.animations[0].clone());
        let mut anim = AnimationPlayer::default();
        if m.looping {
            anim.play(node).repeat();
        } else {
            anim.play(node);
        }
        commands.entity(player).insert((AnimationGraphHandle(graphs.add(graph)), anim));
        m.started = true;
    }
}

/// Camera shake, decaying, on top of where the follow camera put it this frame.
fn shake_camera(time: Res<Time>, mut shake: ResMut<CameraShake>, mut cameras: Query<&mut Transform, (With<Camera3d>, Without<NotShadowCaster>)>) {
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
/// (--character) casts that skill straight ahead, repeatedly -- to look at a
/// skill's sequences without a server.
#[allow(clippy::too_many_arguments)]
fn test_cast(
    mut commands: Commands,
    data: Res<SkillData>,
    tables: Res<Assets<SkillTable>>,
    seqs: Res<Assets<SequenceTable>>,
    time: Res<Time>,
    mut demo: Query<(Entity, &mut Transform, &Character, &CharacterAnim), Without<LocalPlayer>>,
    mut next: Local<f32>,
    mut clock: ResMut<TestCastClock>,
    mut count: Local<u32>,
    nav: Option<Res<CurrentNav>>,
    navmeshes: Res<Assets<NavMesh>>,
) {
    // DSOR_TEST_RUN=<speed>: the demo character runs in a circle (reproducing a
    // body left behind while moving).
    if let Some(speed) = std::env::var("DSOR_TEST_RUN").ok().and_then(|v| v.parse::<f32>().ok()) {
        if let Some((_, mut tf, _, _)) = demo.iter_mut().next() {
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
    let Some((e, mut tfm, character, anim)) = demo.iter_mut().next() else { return };
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
    perform(&mut commands, skill, sequences, e, Some(character), tf.translation, facing, point);
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
    current: Option<Res<crate::map::CurrentMap>>,
    manifests: Res<Assets<crate::map::MapManifest>>,
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
    emitters: Query<(), With<crate::particles::Emitter>>,
) {
    for (e, mut r) in &mut retiring {
        if !r.started {
            r.started = true;
            for c in children.iter_descendants(e) {
                if shown_parts.contains(c) {
                    commands.entity(c).insert(Visibility::Hidden);
                }
                if emitters.contains(c) {
                    commands.entity(c).insert(crate::particles::StopEmitting);
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
    mut movers: Query<(Entity, &mut Transform, &mut SkillMove, Option<&mut LocalPlayer>)>,
) {
    for (e, mut tf, mut m, local) in &mut movers {
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
        let mut p = m.from.lerp(m.to, k);
        p.y += m.arc * 4.0 * k * (1.0 - k);
        tf.translation = p;
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
    actors: Query<(Entity, Option<&LocalPlayer>, Option<&RemotePlayer>, Option<&crate::npc::Npc>)>,
    mut visuals: Query<(Entity, &StatusVisual)>,
    mut players: Query<&mut SequencePlayer>,
    mut by_name: Local<HashMap<String, u32>>,
    mut tested: Local<bool>,
    mut pending_test: Local<Vec<crate::net::LocationEvent>>,
) {
    let (Some(table), Some(sequences)) = (statuses.get(&data.status), seqs.get(&data.sequences)) else { return };
    if by_name.is_empty() {
        *by_name = table.0.iter().map(|(i, d)| (d.id.clone(), *i)).collect();
    }
    let now = time.elapsed_secs();
    // Ended: stop the loop, play the end sequence once where it was.
    for (e, v) in &mut visuals {
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
        if !*tested && seqs.get(&data.sequences).is_some() {
            *tested = true;
            let mut it = spec.split(',');
            let name = it.next().unwrap_or_default().to_owned();
            let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
            let at = if v.len() == 3 { Vec3::new(v[0], v[1], v[2]) } else { Vec3::new(121.5, 12.0, 71.4) };
            pending_test.push(crate::net::LocationEvent { id: 999_999, status: name, position: at, heading: 0.0, seconds: Some(6.0) });
        }
    }
    let mut net_events = Vec::new();
    let mut gone = Vec::new();
    let mut status_events = Vec::new();
    if let Some(mut net) = net {
        net_events = std::mem::take(&mut net.location_events);
        gone = std::mem::take(&mut net.location_gone);
        status_events = std::mem::take(&mut net.status_events);
    }
    net_events.extend(pending_test.drain(..));
    let actor_entity = |id: u32| {
        actors.iter().find_map(|(e, l, r, n)| {
            (l.is_some_and(|l| l.actor == id) || r.is_some_and(|r| r.actor == id) || n.is_some_and(|n| n.actor == id)).then_some(e)
        })
    };
    let mut start = |commands: &mut Commands, def: &StatusDef, key: (u32, u32), holder: Option<Entity>, anchor: Entity, seconds: Option<f32>| {
        // The same instance again: extended, not drawn twice.
        for (e, v) in visuals.iter() {
            if v.key == key {
                if let Ok(mut p) = players.get_mut(v.player) {
                    p.stop = true;
                }
                commands.entity(e).despawn();
            }
        }
        let Some(seq) = StatusDef::first(&def.start).or_else(|| StatusDef::first(&def.tick)).and_then(|n| sequences.0.get(n)) else { return };
        let player = play_with(commands, seq.clone(), holder, anchor, seq.repeat, true);
        let ends = seconds.filter(|s| *s > 0.0).map(|s| now + s).or(if seq.repeat { Some(now + def.duration.max(0.5)) } else { None });
        commands.spawn(StatusVisual {
            key,
            player,
            anchor,
            holder,
            ends,
            end: StatusDef::first(&def.done).or_else(|| StatusDef::first(&def.stop)).cloned(),
        });
    };
    for ev in status_events {
        let Some(def) = table.0.get(&(ev.index as u32)) else { continue };
        let Some(holder) = actor_entity(ev.holder) else { continue };
        start(&mut commands, def, (ev.holder, ev.instance), Some(holder), holder, Some(ev.seconds));
    }
    for ev in net_events {
        let Some(def) = by_name.get(&ev.status).and_then(|i| table.0.get(i)) else { continue };
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
