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
    pub id: String,
    #[serde(rename = "loop")]
    pub loop_seq: String,
    pub impact: String,
    pub death: String,
    pub motion: String,
    pub velocity: f32,
    pub lifetime: f32,
    pub radius: f32,
    /// Where it leaves, in the caster's entity frame.
    pub start: [f32; 3],
}

#[derive(Deserialize, Debug, Clone)]
pub struct SkillDef {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub targeting: String,
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
    pub pre: HashMap<String, String>,
    pub execute: HashMap<String, String>,
    pub post: HashMap<String, String>,
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
    Fx { graphics: String, #[serde(default)] joint: String, at: [f32; 9], start: i32, end: i32 },
    Phase { start: i32, end: i32 },
    Light { color: [f32; 3], intensity: f32, range: f32, at: [f32; 9], start: i32, end: i32 },
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
json_loader!(SkillTableLoader, SkillTable, "skills.json");
json_loader!(SequenceTableLoader, SequenceTable, "sequences.json");

#[derive(Resource)]
pub struct SkillData {
    skills: Handle<SkillTable>,
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
    /// Per track: started, and what it spawned.
    started: Vec<bool>,
    spawned: Vec<Option<Entity>>,
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

/// How long a dead bullet's trail particles live on (the longest particle
/// lifetime of the player skills' bullets is under a second).
const TRAIL_FADE: f32 = 1.0;

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
            .register_asset_loader(SkillTableLoader)
            .register_asset_loader(SequenceTableLoader)
            .init_resource::<CameraShake>()
            .init_resource::<TestCastClock>()
            .add_systems(Startup, load)
            .add_systems(
                Update,
                (index_skills, cast_input, remote_skills, test_cast, run_pending, fly_bullets, play_sequences, start_fx_animations, retire_effects, shake_camera, test_shot, test_view)
                    .chain()
                    .after(crate::net::NetSystems),
            );
    }
}

fn load(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(SkillData {
        skills: assets.load("skills/skills.json"),
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
    let n = seq.tracks.len();
    commands
        .spawn(SequencePlayer { seq, t: 0.0, actor, anchor, looping, started: vec![false; n], spawned: vec![None; n] })
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
            // The ball and its light go; its trail stops growing and fades out
            // instead of vanishing with it ("y'a plus la trainee").
            for c in children.iter_descendants(e) {
                if shown_parts.contains(c) {
                    commands.entity(c).insert(Visibility::Hidden);
                }
                if emitters.contains(c) {
                    commands.entity(c).insert(crate::particles::StopEmitting);
                }
            }
            b.dying = Some(TRAIL_FADE);
        }
    }
}

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
) {
    let eye = cameras.iter().next().map(|c| c.translation());
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
        let done = !anchor_alive || (!p.looping && frame > length);
        for i in 0..p.seq.tracks.len() {
            let track = p.seq.tracks[i].clone();
            let (start, end) = track.span();
            let ending = done || frame >= end as f32;
            if !p.started[i] && frame >= start as f32 && !ending {
                p.started[i] = true;
                match &track {
                    Track::Anim { name, speed, .. } => {
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
                        let mut place = track_transform(at);
                        if joint.is_empty() && characters.contains(p.anchor) {
                            place = Transform::from_rotation(entity_rotation(0.0)) * place;
                        }
                        let mut fx = commands.spawn((
                            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path.clone()))),
                            FxModel { gltf: assets.load(path), started: false, looping: p.looping },
                            place,
                            Visibility::default(),
                        ));
                        if let Some(parent) = parent.filter(|p| exists.contains(*p)) {
                            fx.insert(ChildOf(parent));
                        }
                        p.spawned[i] = Some(fx.id());
                    }
                    Track::Light { color, intensity, range, at, .. } => {
                        let light = commands
                            .spawn((
                                PointLight {
                                    color: Color::srgb(color[0], color[1], color[2]),
                                    // UNVERIFIED: Nebula's intensity (0..10) to lumens.
                                    intensity: intensity * 40_000.0,
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
                    Track::Phase { .. } | Track::Sound { .. } => {}
                }
            }
            if p.started[i] && ending {
                if let Some(s) = p.spawned[i].take() {
                    if matches!(track, Track::Fx { .. }) {
                        commands.entity(s).try_insert(Retiring { left: TRAIL_FADE, started: false });
                    } else {
                        commands.entity(s).try_despawn();
                    }
                }
                match &track {
                    Track::Anim { name, .. } => {
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
                commands.entity(s).try_insert(Retiring { left: TRAIL_FADE, started: false });
            }
            // A standalone anchor (a point on the ground) goes with its sequence.
            if p.actor.is_none() && anchor_alive && !p.looping {
                commands.entity(p.anchor).try_despawn();
            }
            commands.entity(e).despawn();
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
) {
    let Ok(spec) = std::env::var("DSOR_SEQ_VIEW") else { return };
    if !playing.is_empty() {
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
        let Some(m) = current.and_then(|c| manifests.get(&c.manifest)) else { return };
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
