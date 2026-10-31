//! Monsters: what NewMonsterCommand (48) names, drawn as the 2018 client does, and
//! the blows and deaths the server sends them (HitCommand 115, KillCommand 116).
//!
//! The look is `_Template_Monster` (tools/export_monsters.py):
//! - Graphics `characters/uniskel[_dwarf]`: a human dressed on the shared skeleton,
//!   its CharacterSet an outfit (like the NPCs, crate::actors::npc);
//! - any other Graphics: a whole model holding every look of its rig (troll.glb:
//!   troll, cyclops, gnob, golems...). Its CharacterSet is one of the model's skin
//!   lists (`characters/<model>.sets.json`): only those skins are loaded, with the
//!   set's body variation;
//! - AnimSet: the rows of data/tables/anims.xml (`characters/monster_anims.json`),
//!   looked up in the model's own clips.
//! Monsters move as remote actors (crate::network::net::RemotePlayer: MoveCommand, culling,
//! the relayed skills of crate::gameplay::skills).
//!
//! Hits: `Properties::ActorSequenceProperty`, 2018 client.
//! EVIDENCE: HitCommand's handler 0x516453 sends the victim Messaging::Hit with
//!   SetCriticalHit (+0x46, the command's critical flag), SetHeavyHit (+0x45, set
//!   while the game tick is before the command's heavy-until tick), SetDamageTypes,
//!   SetHitTick (setters 0x51D7B3, 0x51DAFF, 0x51D87F, 0x51DB65).
//! EVIDENCE: the property's handler 0x4BAF2A: a critical hit plays the row 6
//!   sequence (CriticalHitSequence) and nothing else; otherwise one sequence per
//!   damage type of the blow (rows 0-5: Physical, Fire, Ice, Lightning, DarkMagic,
//!   Poison). Then a heavy hit adds HeavyHitSequence unless it is already playing.
//! EVIDENCE: FindUnoccupiedSequenceForDamageType 0x4B9FA0: each row holds 4
//!   instances; the hit takes the first whose sequence is not playing, and none if
//!   an instance already carries this hit's tick (the same blow twice).
//! UNVERIFIED: the heavy sequence also needs a flag of the property (+0x39) whose
//!   source is not traced; it is taken as set. Players' own hit sequences: not
//!   traced (their table has no such columns), so blows on players draw nothing.
//! Deaths: zero health or KillCommand disables interaction immediately. The
//!   actor and its effects are removed when its death animation finishes.

use std::collections::HashMap;

use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::gltf::{Gltf, GltfAssetLabel};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use serde::Deserialize;

use crate::actors::character::{spawn_character, vary_joints, AnimState, CharacterAnim, CharacterDesc, ModelBones, NpcLook};
use crate::network::net::{now_ms, MonsterSpawn, Net, RemotePlayer};
use crate::actors::npc::{JointShape, Outfits, Variations};
use crate::gameplay::skills::{play, SequencePlayer, SequenceTable, SkillData};

#[cfg(test)]
pub(crate) mod tests;

#[derive(Deserialize, Debug, Clone)]
pub struct MonsterTemplate {
    pub graphics: String,
    pub set: String,
    pub anim_set: String,
    pub state: String,
    #[serde(rename = "loop")]
    pub loop_start: bool,
    #[serde(default)]
    pub title: String,
    /// Damage type name -> hit sequence.
    #[serde(default)]
    pub hit: HashMap<String, String>,
    #[serde(default)]
    pub critical: String,
    #[serde(default)]
    pub heavy: String,
    #[serde(default)]
    pub death: String,
    /// SummonSequence: played on it as it appears, when summoned.
    #[serde(default)]
    pub summon: String,
    /// MinionType is set (tools/export_monsters.py). A player's minion has one
    /// (minion_player_mercenary...), but so do most monsters: their scaling
    /// progression (Main_Monster_Progression). Summons are told by NewMonster's
    /// category; see spawn_monsters for the map's monsters.
    #[serde(default)]
    pub minion: bool,
    /// DespawnSequence: played on a summon that leaves alive (DiscardMonster),
    /// for DespawnDuration seconds before it is gone.
    #[serde(default)]
    pub despawn: String,
    #[serde(default)]
    pub despawn_s: f32,
    /// Seconds of its anim set's "Spawn" clip (the guardian rising), 0 if none.
    #[serde(default)]
    pub spawn_s: f32,
    /// PrimaryColor, SecondaryColor (rgba): the dyes MatDiffuse and MatSpecular.
    #[serde(default)]
    pub dye: [[f32; 4]; 2],
    #[serde(default)]
    pub radius: f32,
    #[serde(default)]
    pub height: f32,
    /// Its navigation footprint's radius (ActorShapeId, tools/nav_shapes.py):
    /// what a targeted melee cast measures its reach to. None in assets exported
    /// before it, or for a shape the table lacks: the capsule then, as the server.
    #[serde(default)]
    pub footprint: Option<f32>,
    /// _Template_Monster.RunSpeed: what its MoveCommands' speed byte scales.
    /// None in assets exported before it: the player's speed then.
    #[serde(default)]
    pub run_speed: Option<f32>,
}

#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct MonsterTemplates(pub HashMap<String, MonsterTemplate>);

/// Animation set -> state row -> clip base name.
#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct MonsterAnims(pub HashMap<String, HashMap<String, String>>);

#[derive(Deserialize, Debug)]
pub struct ModelSet {
    // Skin selection is applied by model_variant before glTF loading.
    #[serde(default)]
    pub variation: Vec<JointShape>,
}

/// A whole model's character sets (`characters/<model>.sets.json`).
#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct ModelSets(pub HashMap<String, ModelSet>);

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
pub(crate) use json_loader;
json_loader!(MonsterTemplatesLoader, MonsterTemplates, "monster_templates.json");
json_loader!(MonsterAnimsLoader, MonsterAnims, "monster_anims.json");
json_loader!(ModelSetsLoader, ModelSets, "sets.json");

/// A class's own hit sequences (_Template_Player, tools/export_monsters.py).
#[derive(Deserialize, Debug, Clone)]
pub struct PlayerHit {
    pub hit: HashMap<String, String>,
    pub critical: String,
    pub heavy: String,
}

/// Class name -> its hit sequences (`characters/player_hits.json`).
#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct PlayerHits(pub HashMap<String, PlayerHit>);

json_loader!(PlayerHitsLoader, PlayerHits, "player_hits.json");

/// A player's hit sequence slots, as a monster's (Monster::hits, ::heavy).
#[derive(Component, Default)]
pub struct HitSlots {
    rows: [[(Option<u32>, Option<Entity>); INSTANCES]; 7],
    heavy: Option<Entity>,
}

/// A player's mesh whose material is its own copy (own_player_materials).
#[derive(Component)]
struct OwnMaterial;

#[derive(Resource)]
struct MonsterData {
    templates: Handle<MonsterTemplates>,
    anims: Handle<MonsterAnims>,
    player_hits: Handle<PlayerHits>,
    outfits: HashMap<&'static str, Handle<Outfits>>,
    variations: HashMap<&'static str, Handle<Variations>>,
}

/// The local player's actor in the offline test (the demo character has none).
const LOCAL_TEST: u32 = 0x5FFF_FFFF;

/// Rows of the hit table: the six damage types, then the critical hit.
const DAMAGE_TYPES: [&str; 6] = ["Physical", "Fire", "Ice", "Lightning", "DarkMagic", "Poison"];
const CRITICAL_ROW: usize = 6;
/// Instances of each row's sequence (FindUnoccupiedSequenceForDamageType).
const INSTANCES: usize = 4;

/// A monster on the map.
#[derive(Component)]
pub struct Monster {
    pub actor: u32,
    pub template: String,
    pub health: u32,
    pub max_health: u32,
    pub level: u32,
    /// Its collision capsule (CapsuleRadius, CapsuleHeight), times the body
    /// variation's root scale once the model is in: what the cursor picks.
    pub radius: f32,
    pub height: f32,
    /// Its navigation footprint (MonsterTemplate::footprint), not scaled.
    pub footprint: Option<f32>,
    /// The player whose summon it is (NewMonster summoner; 1, or our offline
    /// actor, for our own), 0 for the map's monsters.
    pub summoner: u32,
    /// Whether we may attack it: our faction's stance toward its is Hostile
    /// (Faction::stance): the cursor picks it, bullets stop on it. Not our own
    /// summons, nor a peer's outside PvP, nor an escort.
    pub targetable: bool,
    /// Its KillCommand said it leaves (Kill::despawn): its DespawnSequence plays
    /// instead of its death.
    despawn: bool,
    /// Per row and instance: the hit tick it last played and its sequence player.
    hits: [[(Option<u32>, Option<Entity>); INSTANCES]; 7],
    heavy: Option<Entity>,
}

impl Monster {
    pub(crate) fn is_alive(&self, anim: &CharacterAnim) -> bool {
        self.health > 0 && !anim.dead
    }
}

/// Animation completion owns disposal; a bounded fallback also removes bodies
/// whose rig never loads or whose animation is stopped by visibility culling.
/// A summon's despawn goes at its DespawnDuration instead (`despawn_at`).
#[derive(Component)]
struct MonsterDeath {
    sequence: Option<Entity>,
    fallback_at: f32,
    despawn_at: Option<f32>,
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MonsterSystems;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MonsterHoverInfo;

/// What the debug menu (crate::diagnostics::debug) asks for: monsters to place, and blows on
/// them (actor; 0 a hit of the next damage type, 1 a critical, 2 a kill).
#[derive(Resource, Default)]
pub struct DebugMonsters {
    pub spawns: Vec<MonsterSpawn>,
    pub blows: Vec<(u32, u8)>,
    /// Offline play: the local player's skills landing, as no server answers them
    /// (crate::gameplay::skills).
    pub strikes: Vec<Strike>,
    /// Offline play: summons to place later (seconds, template, where).
    pub delayed: Vec<(f32, String, Vec3)>,
    next_type: u8,
    tick: u32,
    summoned: u32,
}

/// One offline skill landing.
pub struct Strike {
    /// Seconds to go.
    pub after: f32,
    pub at: Vec3,
    pub radius: f32,
    /// The aimed monster, if any (else what stands within `radius` of `at`).
    pub target: Option<u32>,
    /// The skill's VictimStatusEffects and LocationStatusEffects: (status, seconds).
    pub victim: Vec<(String, f32)>,
    pub location: Vec<(String, f32)>,
}

/// HitColor writes of the sequencer (crate::gameplay::skills, ColorShaderParameterTrackBar):
/// actor, colour (alpha: how much of it replaces the shaded colour).
#[derive(Resource, Default)]
pub struct ShaderColors(pub Vec<(Entity, Vec4)>);

/// An actor's own copies of its character materials (crate::rendering::surfaces kind
/// Character): its dyes, and the HitColor its sequences flash.
#[derive(Component, Default)]
pub struct ActorMaterials(pub Vec<Handle<crate::rendering::surfaces::NebulaMaterial>>);

/// A whole model waiting for its scene: skins, variation, animations.
/// A whole model with a character set and an anim set, set up as a monster's is
/// (its set's skins, dyes, variation, its anim set's clips on a CharacterAnim):
/// a mount (crate::actors::mounts). Returns the model's root entity.
pub(crate) fn spawn_model(commands: &mut Commands, assets: &AssetServer, graphics: &str, set: &str, anim_set: &str, dye: [[f32; 4]; 2]) -> Entity {
    let path = crate::rendering::model_variant::path(graphics, set);
    let e = commands
        .spawn((
            Transform::default(),
            Visibility::default(),
            CharacterAnim::default(),
            ModelSetup {
                dye,
                gltf: assets.load(path.clone()),
                sets: assets.load(format!("{graphics}.sets.json")),
                set: set.to_owned(),
                anim_set: anim_set.to_owned(),
                again: false,
            },
        ))
        .id();
    commands.spawn((WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))), ModelScene, ChildOf(e), Transform::default(), Visibility::default()));
    e
}

/// The instance of a whole model's own scene under its actor. Its rig is looked
/// for there only: effects attached to the actor are glTF scenes too, with
/// their own animation players and joints.
/// FAILURE (2026-10-23): the Herald's PermanentStatusEffect (Boss02Idle) puts
///   effects/boss_02_idle under it as it appears. setup_models bound that
///   effect's animation player, gone with the effect half a second later: the
///   Herald stood in its bind pose and no skill animation ever played ("le
///   boss est en animation idle").
#[derive(Component)]
pub(crate) struct ModelScene;

#[derive(Component, Clone)]
struct ModelSetup {
    dye: [[f32; 4]; 2],
    gltf: Handle<Gltf>,
    sets: Handle<ModelSets>,
    set: String,
    anim_set: String,
    /// Set up before, on a scene since rebuilt (rebind_lost_models): the body
    /// is already the set's size.
    again: bool,
}

/// What a model was set up from, kept to set it up again on a rebuilt scene.
#[derive(Component)]
struct ModelSource(ModelSetup);

/// Offline (map viewer): DSOR_TEST_MONSTER=<blueprint>[,x,y,z] places a monster;
/// DSOR_TEST_HIT=<seconds>[,critical] hits it every that many seconds with each
/// damage type in turn, then kills it after six blows.
#[derive(Resource, Default)]
struct OfflineMonster {
    placed: bool,
    /// Where it was placed, and whether it is walking away from there
    /// (patrol_test_monster).
    at: Vec3,
    outward: bool,
    next: f32,
    blows: u32,
}

/// Offline: DSOR_TEST_MONSTER_PATROL=<x>,<y>,<z> walks the test monster between
/// where it was placed and that point, a moving target for bullets. Within 8
/// units: farther, crate::network::net::move_remotes snaps instead of walking.
fn patrol_test_monster(mut test: ResMut<OfflineMonster>, mut monsters: Query<(&Transform, &Monster, &mut RemotePlayer, &CharacterAnim)>) {
    let Ok(spec) = std::env::var("DSOR_TEST_MONSTER_PATROL") else { return };
    let v: Vec<f32> = spec.split(',').filter_map(|x| x.parse().ok()).collect();
    if v.len() != 3 || !test.placed {
        return;
    }
    let Some((tf, m, mut r, anim)) = monsters.iter_mut().find(|(_, m, _, _)| m.actor == 0x6000_0000) else { return };
    if !anim.is_ready() || !m.is_alive(anim) {
        return;
    }
    let goal = if test.outward { Vec3::new(v[0], v[1], v[2]) } else { test.at };
    if Vec2::new(goal.x - tf.translation.x, goal.z - tf.translation.z).length() < 0.2 {
        test.outward = !test.outward;
        return;
    }
    r.target = goal;
}

/// The live monster under the cursor, if any (crate::gameplay::skills targets it, a left
/// click attacks it instead of walking).
/// UNVERIFIED: the 2018 client picks through per-joint picking boxes
/// (export_win32/picking); this is the collision capsule, upright.
#[derive(Resource, Default)]
pub struct Hovered(pub Option<(Entity, u32)>);

/// What the hovered monster's info window shows (crate::ui, interactinfo): its
/// title, level and health.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct HoverInfo(pub Option<(String, u32, u32, u32)>, pub bool);

/// A boss's rank: the ranks whose _Template_MonsterRanks.Rank is "boss" or
/// "worldboss" (static.db4: Rank_sentinel, Rank_treasure_mob[_sentinel],
/// Rank_companion_sentinel; Rank_boss[_endlesstower|_lead|_no_loot],
/// Rank_manifested_fear). Only they wear the info window's ornament.
pub(crate) fn boss_rank(rank: &str) -> bool {
    rank.starts_with("Rank_boss") || matches!(rank, "Rank_sentinel" | "Rank_treasure_mob" | "Rank_treasure_mob_sentinel"
        | "Rank_companion_sentinel" | "Rank_manifested_fear")
}

pub struct MonstersPlugin;

impl Plugin for MonstersPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<MonsterTemplates>()
            .init_asset::<MonsterAnims>()
            .init_asset::<ModelSets>()
            .register_asset_loader(MonsterTemplatesLoader)
            .register_asset_loader(MonsterAnimsLoader)
            .register_asset_loader(ModelSetsLoader)
            .init_asset::<PlayerHits>()
            .register_asset_loader(PlayerHitsLoader)
            .init_resource::<OfflineMonster>()
            .init_resource::<Hovered>()
            .init_resource::<HoverInfo>()
            .add_systems(Update, hover_info.after(MonsterSystems).in_set(MonsterHoverInfo))
            .init_resource::<ShaderColors>()
            .init_resource::<DebugMonsters>()
            .add_systems(PostUpdate, (own_player_materials, apply_shader_colors).chain())
            .add_systems(PreUpdate, hover.after(crate::ui::input::CaptureUiInput))
            .add_systems(Startup, load)
            .add_systems(Update, (spawn_monsters, rebind_lost_models, setup_models, monster_health, hits_and_kills, start_deaths, despawn_summons)
                .chain().in_set(MonsterSystems).before(crate::network::net::NetSystems))
            // Animation completion is known in PostUpdate, but lighting and
            // visibility still queue commands for the corpse's attached effects
            // (update_point_light_bounding_spheres inserting a Sphere on a light
            // despawned with its corpse panicked: a0400_deadlands, 2026-10-18).
            // Dispose only after that schedule has flushed all of its commands.
            .add_systems(Last, remove_corpses)
            .add_systems(Update, patrol_test_monster);
    }
}

fn load(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(MonsterData {
        templates: assets.load("characters/monster_templates.json"),
        player_hits: assets.load("characters/player_hits.json"),
        anims: assets.load("characters/monster_anims.json"),
        outfits: ["uniskel", "uniskel_dwarf"].into_iter().map(|s| (s, assets.load(format!("characters/{s}/outfits.json")))).collect(),
        variations: ["uniskel", "uniskel_dwarf"].into_iter().map(|s| (s, assets.load(format!("characters/{s}/variations.json")))).collect(),
    });
}

#[allow(clippy::too_many_arguments)]
fn spawn_monsters(
    mut commands: Commands,
    net: Option<NonSendMut<Net>>,
    mut offline: Local<Vec<MonsterSpawn>>,
    mut test: ResMut<OfflineMonster>,
    data: Res<MonsterData>,
    templates: Res<Assets<MonsterTemplates>>,
    outfits: Res<Assets<Outfits>>,
    variations: Res<Assets<Variations>>,
    assets: Res<AssetServer>,
    existing: Query<(Entity, &RemotePlayer)>,
    mut debug: ResMut<DebugMonsters>,
    (time, real): (Res<Time>, Res<Time<Real>>),
    (skill_data, seqs): (Option<Res<SkillData>>, Res<Assets<SequenceTable>>),
    (nav, meshes): (Option<Res<crate::world::nav::CurrentNav>>, Res<Assets<crate::world::nav::NavMesh>>),
) {
    // The map viewer, or offline play (--play): no server places monsters.
    // DSOR_TEST_MONSTER=<blueprint>[,x,y,z][;<blueprint>[,x,y,z]]...: the first
    // is the test monster (DSOR_TEST_HIT, DSOR_TEST_MONSTER_PATROL).
    let mut placed = Vec::new();
    if !test.placed && net.as_ref().is_none_or(|n| n.offline) {
        if let Ok(spec) = std::env::var("DSOR_TEST_MONSTER") {
            for (k, one) in spec.split(';').filter(|s| !s.trim().is_empty()).enumerate() {
                let mut it = one.split(',');
                let blueprint = it.next().unwrap_or_default().trim().to_owned();
                let v: Vec<f32> = it.filter_map(|x| x.trim().parse().ok()).collect();
                let position = if v.len() == 3 { Vec3::new(v[0], v[1], v[2]) } else { Vec3::new(123.5, 12.0, 73.4) };
                if k == 0 {
                    test.placed = true;
                    test.at = position;
                }
                placed.push(MonsterSpawn { actor: 0x6000_0000 + k as u32, blueprint, position, health: 100, level: 1, ..default() });
            }
        }
    }
    let now_tick = net.as_ref().map(|n| n.server_tick(now_ms(&real)));
    let (queue, vitals) = match net {
        Some(net) => {
            let net = net.into_inner();
            (&mut net.monster_spawns, Some(&net.monster_vitals))
        }
        None => (&mut *offline, None),
    };
    queue.extend(placed);
    queue.extend(debug.spawns.drain(..));
    // Offline summons whose time has come.
    let dt = time.delta_secs();
    for (after, template, at) in std::mem::take(&mut debug.delayed) {
        if after - dt > 0.0 {
            debug.delayed.push((after - dt, template, at));
        } else {
            debug.summoned += 1;
            queue.push(MonsterSpawn { actor: OFFLINE_SUMMONS + debug.summoned, blueprint: template, position: at, health: 100, level: 1, ..default() });
        }
    }
    if queue.is_empty() {
        return;
    }
    let Some(templates) = templates.get(&data.templates) else { return };
    let ready = |h: &Handle<Outfits>| outfits.get(h).is_some() || assets.load_state(h).is_failed();
    if !data.outfits.values().all(ready) {
        return;
    }
    for spawn in crate::network::net::last_per_actor(std::mem::take(queue), |s| s.actor) {
        let (health, max_health) = vitals.and_then(|v| v.get(&spawn.actor)).copied()
            .unwrap_or((spawn.health, spawn.health));
        // Kill/zero-HP may have arrived while templates or outfits were loading.
        if health == 0 { continue; }
        for (e, r) in &existing {
            if r.actor == spawn.actor {
                commands.entity(e).despawn();
            }
        }
        let Some(t) = templates.0.get(&spawn.blueprint) else {
            warn!("monster {} has no template", spawn.blueprint);
            continue;
        };
        let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
        let position = crate::network::net::remote_ground_position(mesh, spawn.position);
        // A summon turns as its owner did when it came (a monster, until it moves).
        let facing = if spawn.summoner != 0 { spawn.facing } else { 0.0 };
        let transform = Transform::from_translation(position).with_rotation(Quat::from_rotation_y(facing));
        let skeleton = t.graphics.strip_prefix("characters/").unwrap_or(&t.graphics).to_owned();
        let entity = if skeleton == "uniskel" || skeleton == "uniskel_dwarf" {
            let outfit = data
                .outfits
                .get(skeleton.as_str())
                .and_then(|h| outfits.get(h))
                .and_then(|o| o.outfits.iter().find(|o| o.name == t.set));
            let Some(outfit) = outfit else {
                warn!("monster {}: no outfit {}", spawn.blueprint, t.set);
                continue;
            };
            let variation = outfit
                .variation
                .as_ref()
                .and_then(|v| data.variations.get(skeleton.as_str()).and_then(|h| variations.get(h)).and_then(|all| all.variations.get(v)))
                .map(|shape| shape.iter().map(|j| (j.bone.clone(), Vec3::from(j.t), Vec3::from(j.s))).collect())
                .unwrap_or_default();
            let desc = CharacterDesc {
                look: Some(NpcLook { sound_profile: t.set.clone(), skeleton, parts: outfit.parts.clone(), anim_set: t.anim_set.clone(), variation }),
                ..default()
            };
            spawn_character(&mut commands, desc, transform)
        } else {
            let path = crate::rendering::model_variant::path(&t.graphics, &t.set);
            let e = commands
                .spawn((
                    transform,
                    Visibility::default(),
                    CharacterAnim::default(),
                    ModelSetup {
                        dye: t.dye,
                        gltf: assets.load(path.clone()),
                        sets: assets.load(format!("{}.sets.json", t.graphics)),
                        set: t.set.clone(),
                        anim_set: t.anim_set.clone(),
                        again: false,
                    },
                ))
                .id();
            commands.spawn((WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))), ModelScene, ChildOf(e), Transform::default(), Visibility::default()));
            e
        };
        // A looping start state (asleep, burrowed...) is kept; Idle otherwise.
        let state = standing_state(&t.state);
        if t.loop_start && state != "Idle" {
            let mut anim = CharacterAnim::default();
            anim.state = AnimState::Named(state.to_owned());
            commands.entity(entity).insert(anim);
        }
        commands.entity(entity).insert((
            Name::new(format!("monster {}", spawn.blueprint)),
            crate::actors::picking::PickModel(t.graphics.clone()),
            Monster {
                actor: spawn.actor,
                template: spawn.blueprint.clone(),
                health,
                max_health,
                level: spawn.level,
                radius: t.radius.max(0.3),
                height: t.height.max(1.0),
                footprint: t.footprint,
                summoner: spawn.summoner,
                targetable: !spawn.friendly,
                despawn: false,
                hits: Default::default(),
                heavy: None,
            },
            RemotePlayer { actor: spawn.actor, target: position, facing, still_for: 1.0, held: 0.0, held_until_tick: None,
                run_speed: t.run_speed.filter(|s| *s > 0.0), ..default() },
        ));
        if spawn.summoner != 0 {
            commands.entity(entity).insert(crate::actors::character::RelaxedIdle);
        }
        // A summon appears through its SummonSequence (arcanemanifestation's
        // guardian rising, summontree's tree growing, a Shadow Soul coming out
        // of its smoke): without it they stood there at once ("pas d'animation
        // d'apparition"). Its rise, the anim set's Spawn row, goes with it: the
        // SummonSequence holds only the effects around it.
        // EVIDENCE: dro_client64.exe (July 2026) ActorSequenceProperty::OnActivate
        //   0x14012ea48: categories 1 and 2 play "spawn/" + SummonSequence under 11
        //   ticks after NewMonster's spawn tick; the StartAnimationState list
        //   (0x14014e83c: each entry once, the last looping) plays on every
        //   NewMonster. So a summon met later (a newcomer, walking into view) only
        //   rises, without the smoke.
        // UNVERIFIED: the map's own monsters (category 0, no spawn tick) rise
        //   through their SummonSequence when they have a MinionType, as before;
        //   the original plays their start list instead (most have no Spawn
        //   length exported) and their SpecialSpawnSequence when fresh.
        let sequences = skill_data.as_ref().and_then(|d| seqs.get(d.sequences()));
        let rise = (t.spawn_s > 0.0).then(|| {
            let end = (t.spawn_s * 25.0).ceil() as i32;
            (crate::gameplay::skills::Track::Anim { name: "Spawn".into(), speed: 1.0, start: 0, end, fadein: 0.0 }, end as u32)
        });
        let summon = sequences.and_then(|s| s.0.get(&t.summon));
        let summoned = if matches!(spawn.category, 1 | 2) {
            now_tick.is_none_or(|now| (now.wrapping_sub(spawn.spawn_tick) as i32) < SUMMON_FRESH_TICKS)
        } else {
            t.minion || (OFFLINE_SUMMONS..OFFLINE_SUMMONS + 0x0100_0000).contains(&spawn.actor)
        };
        let sequence = match (summoned, summon) {
            (true, Some(seq)) => Some(seq.clone()),
            _ if matches!(spawn.category, 1 | 2) && t.state.split(';').next().map(str::trim) == Some("Spawn") =>
                Some(crate::gameplay::skills::Sequence { length: 0, repeat: false, tracks: Vec::new() }),
            _ => None,
        };
        if let Some(mut seq) = sequence {
            if let Some((track, end)) = rise.clone() {
                seq.tracks.push(track);
                seq.length = seq.length.max(end);
            }
            if !seq.tracks.is_empty() {
                play(&mut commands, seq, Some(entity), entity, false);
            }
            // It rises where it appeared before it walks: the server sets an
            // event's or a summon's monsters on their way at once (StartupDuration
            // 0: the Herald's broodlings), and the walk took the body over a tenth
            // of a second into its rise, so they came running out of nowhere ("les
            // mobs invoques n'ont pas d'animation d'entree", 2026-10-23). Its
            // records still arrive meanwhile; it catches up with them after.
            // UNVERIFIED: whether the original holds the body for its start list.
            if let Some((_, end)) = rise {
                let hold = end as f32 / 25.0;
                commands.entity(entity).entry::<RemotePlayer>().and_modify(move |mut r| r.held = r.held.max(hold));
            }
        }
    }
}

/// Actor ids of the monsters offline play summons.
const OFFLINE_SUMMONS: u32 = 0x6200_0000;

/// How many ticks after its spawn tick a summon still plays its SummonSequence
/// (ActorSequenceProperty::OnActivate 0x14012ea48: under 11, 0.44 s). Counted
/// signed: a clock of ours behind the server's still finds it fresh.
const SUMMON_FRESH_TICKS: i32 = 11;

/// The state a monster stands in from a StartAnimationState: the last of a list
/// ("Spawn;Idle": the Spawn played once as it appears, see spawn_monsters).
fn standing_state(start: &str) -> &str {
    start.rsplit(';').map(str::trim).find(|s| !s.is_empty()).unwrap_or("Idle")
}

/// A whole model's body: its character set's variation on the joints under `root`
/// (vary_joints), each skinned mesh rebound to the varied joints' scaled children.
/// Monsters and whole-model NPCs (crate::actors::npc): the deco pigeon's set scales
/// every joint by 4, its model being authored a quarter of its size.
pub(crate) fn vary_model(
    commands: &mut Commands,
    joints: &Query<(&Transform, Option<&ChildOf>)>,
    skinned: &Query<&SkinnedMesh>,
    children: &Query<&Children>,
    root: Entity,
    bones: &HashMap<String, Entity>,
    set: &ModelSet,
) {
    if set.variation.is_empty() {
        return;
    }
    let shape: Vec<(String, Vec3, Vec3)> = set.variation.iter().map(|j| (j.bone.clone(), Vec3::from(j.t), Vec3::from(j.s))).collect();
    let scaled = vary_joints(commands, joints, bones, &shape);
    // Each varied joint's vertices follow its scaled child.
    let by_joint: HashMap<Entity, Entity> = scaled.iter().filter_map(|(n, s)| bones.get(n).map(|j| (*j, *s))).collect();
    for d in children.iter_descendants(root) {
        if let Ok(skin) = skinned.get(d) {
            let joints = skin.joints.iter().map(|j| by_joint.get(j).copied().unwrap_or(*j)).collect();
            commands.entity(d).try_insert(SkinnedMesh { inverse_bindposes: skin.inverse_bindposes.clone(), joints });
        }
    }
}

/// A whole model whose scene is in: show the set's skins only, give it the set's
/// body, and drive its own animation player through the AnimSet's rows.
#[allow(clippy::too_many_arguments)]
fn setup_models(
    mut commands: Commands,
    mut pending: Query<(Entity, &ModelSetup, &mut CharacterAnim, Option<&mut Monster>)>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    model_scenes: Query<(), With<ModelScene>>,
    names: Query<&Name>,
    targets: Query<(), (With<AnimationTargetId>, Without<AnimatedBy>)>,
    skinned: Query<&SkinnedMesh>,
    joints: Query<(&Transform, Option<&ChildOf>)>,
    gltfs: Res<Assets<Gltf>>,
    sets: Res<Assets<ModelSets>>,
    anims: Res<Assets<MonsterAnims>>,
    data: Res<MonsterData>,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    (surfaces, mut surface_assets): (
        Query<&MeshMaterial3d<crate::rendering::surfaces::NebulaMaterial>>,
        ResMut<Assets<crate::rendering::surfaces::NebulaMaterial>>,
    ),
) {
    let Some(table) = anims.get(&data.anims) else { return };
    for (e, setup, mut anim, mut monster) in &mut pending {
        let Some(gltf) = gltfs.get(&setup.gltf) else { continue };
        // Its own scene's rig, not an effect's (ModelScene). A model spawned
        // without the marker (tests) is searched whole.
        let scene = children.get(e).ok()
            .and_then(|c| c.iter().find(|c| model_scenes.contains(*c)))
            .unwrap_or(e);
        let Some(player) = children.iter_descendants(scene).find(|c| players.contains(*c)) else { continue };
        let set_file = sets.get(&setup.sets);
        if set_file.is_none() && !assets.load_state(&setup.sets).is_failed() {
            continue;
        }
        let set = set_file.and_then(|s| s.0.get(&setup.set));
        if set_file.is_some() && set.is_none() {
            warn!("model {:?}: no character set {}", setup.gltf.path(), setup.set);
        }
        let mut bones = HashMap::new();
        for d in children.iter_descendants(scene) {
            if let Ok(n) = names.get(d) {
                bones.entry(n.as_str().to_owned()).or_insert(d);
            }
            // Hooked again: bones unhooked while the scene had no driver
            // (crate::rendering::anim_cull, crate::actors::character's idle pruning).
            if targets.contains(d) {
                commands.entity(d).try_insert(AnimatedBy(player));
            }
        }
        if let Some(root) = set.and_then(|s| s.variation.iter().find(|j| j.bone == "Skeleton_Root")) {
            if let Some(monster) = monster.as_mut().filter(|_| !setup.again) {
                let scale = root.s[0].max(root.s[2]);
                monster.radius *= scale;
                monster.height *= root.s[1];
            }
        }
        if let Some(set) = set {
            vary_model(&mut commands, &joints, &skinned, &children, scene, &bones, set);
        }
        // Its own copies of its character materials: its template's dyes, and a
        // HitColor of its own.
        let mut own = Vec::new();
        for d in children.iter_descendants(scene) {
            let Ok(MeshMaterial3d(h)) = surfaces.get(d) else { continue };
            let Some(mut m) = surface_assets.get(h).cloned() else { continue };
            if m.extension.params.p0.x as u32 != crate::rendering::surfaces::Kind::Character as u32 {
                continue;
            }
            m.extension.params.p1 = Vec4::from_array(setup.dye[0]);
            m.extension.params.p2 = Vec4::from_array(setup.dye[1]);
            let copy = surface_assets.add(m);
            commands.entity(d).try_insert(MeshMaterial3d(copy.clone()));
            own.push(copy);
        }
        commands.entity(e).try_insert(ActorMaterials(own));
        let mut graph = AnimationGraph::new();
        let mut nodes = HashMap::new();
        let mut sound_clips = HashMap::new();
        let mut twins = Vec::new();
        if let Some(rows) = table.0.get(&setup.anim_set) {
            for (state, clip) in rows {
                let looped = format!("{clip}-loop");
                let found = gltf.named_animations.get(looped.as_str()).map(|h| (h, true)).or_else(|| gltf.named_animations.get(clip.as_str()).map(|h| (h, false)));
                if let Some((handle, looping)) = found {
                    let node = graph.add_clip(handle.clone(), 1.0, graph.root);
                    sound_clips.insert(node, clip.trim_end_matches("-loop").to_owned());
                    if !looping {
                        let twin = graph.add_clip(handle.clone(), 1.0, graph.root);
                        sound_clips.insert(twin, clip.trim_end_matches("-loop").to_owned());
                        twins.push((node, twin));
                    }
                    nodes.insert(state.clone(), (node, looping));
                }
            }
        } else {
            warn!("model {:?}: no animation set {}", setup.gltf.path(), setup.anim_set);
        }
        commands
            .entity(player)
            .insert((AnimationGraphHandle(graphs.add(graph)), crate::rendering::anim_cull::ManagedAnimation,
                crate::audio::AnimationAudio::new(e, &setup.gltf.path().unwrap().path().to_string_lossy(), &setup.set, sound_clips)));
        anim.bind_model(player, nodes);
        anim.set_twins(twins);
        // The file stays loaded while the monster stands: dropped with ModelSetup,
        // the next monster of its kind loaded it again, the reload replaced its
        // scene (AssetEvent::Modified), and bevy spawned every instance of it
        // anew, without its hidden skins.
        commands.entity(e).try_insert((ModelBones(bones), crate::rendering::model_variant::ResidentModel(setup.gltf.clone()),
            ModelSource(ModelSetup { again: true, ..setup.clone() }))).remove::<ModelSetup>();
    }
}

/// A model whose animation player is gone is set up again on its new scene.
/// FAILURE (2026-10-29): leaving the crypt for Heredur's room and back within
///   6 s, the crypt's monster models were freed and loaded again; bevy rebuilt
///   the scenes of the monsters already placed after setup_models had bound
///   their players, and the new copies stood in their bind pose, sliding as
///   they walked (as the effects did: crate::gameplay::skills FxModel).
fn rebind_lost_models(
    mut commands: Commands,
    models: Query<(Entity, &ModelSource, &CharacterAnim), Without<ModelSetup>>,
    players: Query<(), With<AnimationPlayer>>,
) {
    for (e, source, anim) in &models {
        if anim.bound_player().is_some_and(|p| !players.contains(p)) {
            commands.entity(e).try_insert(source.0.clone());
        }
    }
}

fn monster_health(net: Option<NonSendMut<Net>>, mut monsters: Query<(&mut Monster, &CharacterAnim)>) {
    let Some(mut net) = net else { return };
    for (actor, health, max) in std::mem::take(&mut net.monster_health) {
        if let Some((mut m, _)) = monsters.iter_mut().find(|(m, a)| m.actor == actor && m.is_alive(a)) {
            m.health = health;
            m.max_health = max;
        }
    }
}

/// A blow or a death, as the 2018 client's ActorSequenceProperty draws it.
struct Blow {
    victim: u32,
    attacker: u32,
    damage: i32,
    blocked: bool,
    immune: bool,
    kind: i32,
    /// Whose blow the combat value is, and its factor.
    owner: u32,
    value: f32,
    tick: u32,
    damage_types: Vec<u8>,
    critical: bool,
    heavy: bool,
    health: Option<(u32, u32)>,
    /// The server's combat text type, when it names one ("HealingSelf",
    /// "DamageDonePoison", "DamageImmunityOther").
    text: String,
}

#[allow(clippy::too_many_arguments)]
fn hits_and_kills(
    mut commands: Commands,
    time: Res<Time>,
    real: Res<Time<Real>>,
    mut net: Option<NonSendMut<Net>>,
    mut test: ResMut<OfflineMonster>,
    skills: Option<Res<SkillData>>,
    seqs: Res<Assets<SequenceTable>>,
    data: Res<MonsterData>,
    templates: Res<Assets<MonsterTemplates>>,
    mut monsters: Query<(Entity, &mut Monster, &mut CharacterAnim)>,
    playing: Query<(), With<SequencePlayer>>,
    (locals, remotes, mut texts, mut debug, transforms): (
        Query<(Entity, &crate::network::net::LocalPlayer)>,
        Query<(Entity, &RemotePlayer), Without<Monster>>,
        ResMut<crate::ui::combat_text::CombatTexts>,
        ResMut<DebugMonsters>,
        Query<&GlobalTransform>,
    ),
    (mut players, player_hits): (
        Query<(&crate::actors::character::Character, Option<&mut HitSlots>), Without<Monster>>,
        Res<Assets<PlayerHits>>,
    ),
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("monsters::hits_and_kills");
    // Combat state must still advance while visual assets are loading/missing.
    let sequences = skills.as_ref().and_then(|s| seqs.get(s.sequences()));
    let templates = templates.get(&data.templates);
    let mut blows = Vec::new();
    let mut deaths = Vec::new();
    // Killed with the despawn bit: they leave rather than die (Kill::despawn).
    let mut leaving = Vec::new();
    // Offline the test blows are the local player's own.
    let mut local = LOCAL_TEST;
    match net.as_deref_mut() {
        Some(net) => {
            local = net.local_actor.unwrap_or(u32::MAX);
            let now = net.server_tick(now_ms(&real));
            for (victim, h) in std::mem::take(&mut net.hits) {
                blows.push(Blow {
                    victim,
                    attacker: h.attacker,
                    damage: h.damage,
                    blocked: h.blocked,
                    immune: h.immune,
                    kind: h.kind,
                    owner: h.combat_value_owner,
                    value: h.combat_value,
                    tick: h.tick,
                    damage_types: h.damage_types,
                    critical: h.critical,
                    heavy: now < h.heavy_until_tick,
                    health: Some((h.victim_health.max(0) as u32, h.victim_max_health.max(0) as u32)),
                    text: h.combat_text,
                });
            }
            for (victim, kill) in std::mem::take(&mut net.kills) {
                if kill.despawn { leaving.push(victim); }
                deaths.push(victim);
            }
        }
        None => {
            // DSOR_TEST_HIT=<seconds>[,critical]: offline blows on the test monster.
            if let Some(spec) = std::env::var("DSOR_TEST_HIT").ok().filter(|_| test.placed) {
                let mut it = spec.split(',');
                let every: f32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(1.5);
                let critical = it.next() == Some("critical");
                test.next -= time.delta_secs();
                if test.next <= 0.0 && monsters.iter().any(|(_, m, a)| m.actor == 0x6000_0000 && a.is_ready() && !a.dead) {
                    test.next = every;
                    test.blows += 1;
                    if test.blows > DAMAGE_TYPES.len() as u32 {
                        deaths.push(0x6000_0000);
                    } else {
                        blows.push(Blow {
                            victim: 0x6000_0000,
                            attacker: LOCAL_TEST,
                            damage: 37 * test.blows as i32,
                            blocked: test.blows == 3,
                            immune: false,
                            kind: 3,
                            owner: LOCAL_TEST,
                            value: 0.0,
                            tick: test.blows,
                            damage_types: vec![(test.blows - 1) as u8],
                            critical,
                            heavy: false,
                            health: None,
                            text: String::new(),
                        });
                    }
                }
            }
        }
    }
    // Offline play: the player's skills strike what they aimed at, or what stands
    // where they land.
    let dt = time.delta_secs();
    let mut landed = Vec::new();
    for mut s in std::mem::take(&mut debug.strikes) {
        s.after -= dt;
        if s.after > 0.0 {
            debug.strikes.push(s);
        } else {
            landed.push(s);
        }
    }
    for Strike { at, radius, target, victim, location, .. } in landed {
        // Its ground effects, as the server's NewLocationEffect would place them.
        if let Some(net) = net.as_deref_mut() {
            for (status, seconds) in location {
                debug.tick += 1;
                net.location_events.push(crate::network::net::LocationEvent {
                    id: 0x4000_0000 + debug.tick,
                    status,
                    position: at,
                    heading: 0.0,
                    seconds: (seconds > 0.0).then_some(seconds),
                });
            }
        }
        let near = |p: Vec3| Vec2::new(p.x - at.x, p.z - at.z).length() <= radius;
        let hit: Vec<u32> = monsters
            .iter()
            .filter(|(e, m, a)| m.is_alive(a) && (target == Some(m.actor) || (target.is_none() && near(transforms.get(*e).map(|t| t.translation()).unwrap_or(Vec3::INFINITY)))))
            .map(|(_, m, _)| m.actor)
            .collect();
        for actor in hit {
            debug.blows.push((actor, 0));
            // Its victim effects, as the server's StatusEffect would start them.
            if let Some(net) = net.as_deref_mut() {
                for (status, seconds) in &victim {
                    net.named_status_events.push((actor, status.clone(), *seconds));
                }
            }
        }
    }
    // The debug menu's blows, as the local player's.
    for (victim, kind) in std::mem::take(&mut debug.blows) {
        if kind == 2 {
            deaths.push(victim);
            continue;
        }
        debug.tick += 1;
        let t = debug.next_type;
        let damage = 10 + (debug.tick * 37 % 40) as i32;
        debug.next_type = (t + 1) % DAMAGE_TYPES.len() as u8;
        blows.push(Blow {
            victim,
            attacker: local,
            damage,
            blocked: false,
            immune: false,
            kind: 3,
            owner: local,
            value: 0.0,
            tick: 0x4000_0000 + debug.tick,
            damage_types: vec![t],
            critical: kind == 1,
            heavy: false,
            health: None,
            text: String::new(),
        });
    }
    for blow in blows {
        // Ignore late hits/heals entirely; a new spawn is required to revive a
        // monster. Apply each blow in order so a lethal one blocks later blows.
        if let Some((_, mut m, anim)) = monsters.iter_mut().find(|(_, m, _)| m.actor == blow.victim) {
            if !m.is_alive(&anim) { continue; }
            if let Some((health, max)) = blow.health {
                m.health = health;
                m.max_health = max;
            } else if blow.victim >= 0x6100_0000 {
                m.health = m.health.saturating_sub(blow.damage.max(0) as u32);
            }
        }
        // Floating texts (crate::ui::combat_text), for blows given or taken by the
        // local player (HitCommand's handler 0x516453).
        let victim_entity = monsters
            .iter()
            .find(|(_, m, _)| m.actor == blow.victim)
            .map(|(e, ..)| e)
            .or_else(|| locals.iter().find(|(_, l)| l.actor == blow.victim).map(|(e, _)| e))
            .or_else(|| remotes.iter().find(|(_, r)| r.actor == blow.victim).map(|(e, _)| e));
        let mine = blow.victim == local;
        // Release 261 shows every blow the local player takes, its own heals
        // included (attacker = victim = the player: potions, regeneration); the
        // others only when it gave them. EVIDENCE: HitCommand's handler
        // 0x1401cbd13 (dro_client64): the victim's test 0x1401cc0d0 jumps
        // straight to the texts. Its summons' blows (a Shadow Soul's) are its own
        // too: the attacker is a monster whose summoner is the local player.
        // EVIDENCE: the July 2026 build's handler 0x1401c4050 (0x1401ce6a4: a
        //   monster with actor +0x164, NewMonster's summoner, local); a second
        //   test of actor +0x168 (0x1401ce5c8) is not known and not done.
        let given = blow.attacker == local
            || monsters.iter().any(|(_, m, _)| m.actor == blow.attacker && m.summoner == local);
        if let (Some(v), true) = (victim_entity, mine || (given && blow.victim != blow.attacker)) {
            use crate::ui::combat_text::*;
            // The server's type, when it names one, in place of each branch's own
            // (0x1401cbdbe: the string at +0x88 read as a CombatTextType, -1 when
            // empty; every branch tests it before picking its default).
            // UNVERIFIED: "DamageImmunityOther" without the immune flag takes the
            // damage branch, so it shows the (zero) damage in the immunity colour.
            let pick = |own: u8, other: u8| {
                if blow.text.is_empty() {
                    type_name(if mine { own } else { other }).to_owned()
                } else {
                    blow.text.clone()
                }
            };
            if blow.blocked {
                texts.0.push((v, pick(BLOCK_SELF, BLOCK_ENEMY), Say::Block));
            }
            if blow.immune {
                texts.0.push((v, pick(DAMAGE_IMMUNITY_SELF, DAMAGE_IMMUNITY_OTHER), Say::Immunity));
            } else if blow.critical {
                texts.0.push((v, pick(CRITICAL_RECEIVED, CRITICAL_DONE), Say::Critical(blow.damage)));
            } else {
                texts.0.push((v, pick(DAMAGE_RECEIVED, DAMAGE_DONE), Say::Damage(blow.damage)));
            }
            if blow.kind == 10 {
                texts.0.push((v, type_name(if mine { LOW_HEALTH_HIT_RECEIVED } else { LOW_HEALTH_HIT }).to_owned(), Say::LowHealth));
            }
        }
        // The combat value (life drained...) over its owner, when it is the local
        // player and the value is positive.
        if blow.owner == local && blow.value > 0.0 {
            if let Some((e, _)) = locals.iter().next() {
                texts.0.push((e, crate::ui::combat_text::type_name(crate::ui::combat_text::HEALING_SELF).to_owned(), crate::ui::combat_text::Say::Healing((blow.damage as f32 * blow.value) as i32)));
            }
        }
        // A player struck: its class's own hit sequences (default_hit_player: the
        // body flashes red), as a monster's (ActorSequenceProperty 0x4BAF2A).
        if let (Some(v), None) = (victim_entity, monsters.iter().find(|(_, m, _)| m.actor == blow.victim)) {
            let row = player_hits.get(&data.player_hits).zip(players.get_mut(v).ok());
            if let (Some(sequences), Some((table, (c, slots)))) = (sequences, row) {
                if let Some(h) = table.0.get(c.desc.class_name()) {
                    let mut fresh = HitSlots::default();
                    let has = slots.is_some();
                    let slots = match slots {
                        Some(s) => s.into_inner(),
                        None => &mut fresh,
                    };
                    play_blow(&mut commands, sequences, v, &mut slots.rows, &mut slots.heavy, &blow, &h.hit, &h.critical, &h.heavy, &playing);
                    if !has {
                        commands.entity(v).try_insert(fresh);
                    }
                }
            }
            continue;
        }
        let Some((e, mut m, anim)) = monsters.iter_mut().find(|(_, m, _)| m.actor == blow.victim) else { continue };
        if !m.is_alive(&anim) {
            continue;
        }
        let (Some(sequences), Some(t)) = (sequences, templates.and_then(|t| t.0.get(&m.template))) else { continue };
        let Monster { hits, heavy, .. } = &mut *m;
        play_blow(&mut commands, sequences, e, hits, heavy, &blow, &t.hit, &t.critical, &t.heavy, &playing);
    }
    for victim in deaths {
        let Some((_, mut m, _)) = monsters.iter_mut().find(|(_, m, _)| m.actor == victim) else {
            // A player's death is crate::network::net's; anything else is a monster the
            // client does not have (yet, or any more): it would stand there alive.
            let local = locals.iter().next().map(|(_, l)| l.actor);
            if local != Some(victim) && !remotes.iter().any(|(_, r)| r.actor == victim) {
                warn!("KillCommand for actor {victim}: no such monster here");
            }
            continue;
        };
        m.health = 0;
        m.despawn |= leaving.contains(&victim);
    }
}

fn start_deaths(
    mut commands: Commands,
    time: Res<Time>,
    skills: Option<Res<SkillData>>,
    seqs: Res<Assets<SequenceTable>>,
    data: Res<MonsterData>,
    templates: Res<Assets<MonsterTemplates>>,
    mut monsters: Query<(Entity, &Monster, &mut CharacterAnim), Without<MonsterDeath>>,
    mut hovered: ResMut<Hovered>,
    mut info: ResMut<HoverInfo>,
) {
    let sequences = skills.as_ref().and_then(|s| seqs.get(s.sequences()));
    for (e, m, mut anim) in &mut monsters {
        if m.health > 0 && !anim.dead { continue; }
        let t = templates.get(&data.templates).and_then(|t| t.0.get(&m.template));
        let death = if m.despawn {
            despawn(&mut commands, e, &mut anim, t, sequences, time.elapsed_secs())
        } else {
            anim.dead = true;
            anim.state = AnimState::Named("Death".into());
            anim.speed = 1.0;
            anim.replay();
            let seq = t.and_then(|t| sequences.and_then(|s| s.0.get(&t.death)));
            let fallback = seq.map_or(5.0, |s| s.length.max(1) as f32 / 25.0);
            let sequence = seq.map(|s| crate::skills::play_death(&mut commands, s.clone(), e));
            MonsterDeath { sequence, fallback_at: time.elapsed_secs() + fallback, despawn_at: None }
        };
        commands.entity(e).insert(death).remove::<(crate::actors::picking::PickBoxes, crate::actors::picking::PickModel)>();
        if hovered.0.is_some_and(|(target, _)| target == e) {
            hovered.0 = None;
            info.0 = None;
        }
    }
}

/// A player's summon the server discarded while it stood (a Shadow Soul replaced
/// by another, its owner gone): it leaves as one killed with the despawn bit
/// does (`despawn`), not at once as other discarded actors (apply_remotes). One
/// already dying finishes its death.
/// DEVIATION: the original removes any discarded actor at once (DiscardMonster
///   handler 0x1401ff3c8, dro_client64.exe July 2026: EntityManager::RemoveEntity)
///   and plays a DespawnSequence only on a KillCommand with the despawn bit,
///   leaving the discard to the server after DespawnDuration. The release261
///   server never sends that kill (services/combat_resolution.py field_78=False)
///   and discards a replaced soul at once (services/player_summons.py), so without
///   this a soul would only ever vanish.
#[allow(clippy::too_many_arguments)]
fn despawn_summons(
    mut commands: Commands,
    time: Res<Time>,
    net: Option<NonSendMut<Net>>,
    skills: Option<Res<SkillData>>,
    seqs: Res<Assets<SequenceTable>>,
    data: Res<MonsterData>,
    templates: Res<Assets<MonsterTemplates>>,
    mut monsters: Query<(Entity, &Monster, &mut CharacterAnim, Has<MonsterDeath>)>,
    (mut hovered, mut info): (ResMut<Hovered>, ResMut<HoverInfo>),
) {
    let Some(mut net) = net else { return };
    if net.summons_gone.is_empty() { return; }
    let gone = std::mem::take(&mut net.summons_gone);
    let now = time.elapsed_secs();
    let sequences = skills.as_ref().and_then(|s| seqs.get(s.sequences()));
    for (e, m, mut anim, dying) in &mut monsters {
        if dying || !gone.contains(&m.actor) { continue; }
        let t = templates.get(&data.templates).and_then(|t| t.0.get(&m.template));
        let death = despawn(&mut commands, e, &mut anim, t, sequences, now);
        commands.entity(e).insert(death).remove::<(crate::actors::picking::PickBoxes, crate::actors::picking::PickModel)>();
        if hovered.0.is_some_and(|(target, _)| target == e) {
            hovered.0 = None;
            info.0 = None;
        }
    }
}

/// A monster leaving rather than dying (Kill::despawn, a summon's discard): its
/// DespawnSequence (a Shadow Soul's smoke, its body hidden from frame 6), the
/// anim set's Despawn row beneath it, and its removal after DespawnDuration
/// (else the sequence's length; at once with neither).
/// EVIDENCE: ActorSequenceProperty::OnKillReceived 0x140130294 (dro_client64.exe
///   July 2026) plays "despawn/" + DespawnSequence for a despawn kill, no Death
///   clip. DespawnDuration has no reader in the client (only the template
///   loader 0x1408754d8): it is the server's wait before the discard, used here
///   as ours.
fn despawn(commands: &mut Commands, e: Entity, anim: &mut CharacterAnim, t: Option<&MonsterTemplate>,
    sequences: Option<&SequenceTable>, now: f32) -> MonsterDeath {
    let seq = t.and_then(|t| sequences.and_then(|s| s.0.get(&t.despawn)));
    let seconds = t.map_or(0.0, |t| t.despawn_s).max(0.0);
    let seconds = if seconds > 0.0 { seconds } else { seq.map_or(0.0, |s| s.length as f32 / 25.0) };
    anim.dead = true;
    anim.state = AnimState::Named("Despawn".into());
    anim.speed = 1.0;
    anim.replay();
    let sequence = seq.filter(|_| seconds > 0.0).map(|s| crate::skills::play_death(commands, s.clone(), e));
    MonsterDeath { sequence, fallback_at: now + seconds, despawn_at: Some(now + seconds) }
}

fn remove_corpses(
    time: Res<Time>,
    mut net: Option<NonSendMut<Net>>,
    monsters: Query<(Entity, &Monster, &CharacterAnim, &MonsterDeath)>,
    players: Query<&AnimationPlayer>,
    sequences: Query<(Entity, &SequencePlayer)>,
    mut commands: Commands,
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("monsters::remove_corpses");
    for (e, m, anim, death) in &monsters {
        if let Some(at) = death.despawn_at {
            if time.elapsed_secs() < at { continue; }
        } else {
            let pending = death.sequence.and_then(|s| sequences.get(s).ok())
                .is_some_and(|(_, s)| s.has_pending_animation());
            let finished = anim.playback_finished(&players);
            if pending || !finished.unwrap_or(time.elapsed_secs() >= death.fallback_at) { continue; }
        }
        for (s, player) in &sequences {
            if player.visual_actors().any(|actor| actor == e) {
                player.despawn(&mut commands, s);
            }
        }
        if let Some(net) = net.as_mut() { net.forget_actor(m.actor); }
        // Despawn now in both modes, including the rig, picking geometry, and
        // attached effects through ChildOf's recursive relationship cleanup.
        commands.entity(e).try_despawn();
    }
}

fn hover(
    ui: Res<crate::ui::input::UiInput>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    monsters: Query<(Entity, &GlobalTransform, &Monster, &CharacterAnim, &InheritedVisibility, Option<&crate::actors::picking::PickBoxes>)>,
    joints: Query<&GlobalTransform>,
    mut hovered: ResMut<Hovered>,
    players: Query<(Entity, &GlobalTransform, &crate::network::net::RemotePlayer, &CharacterAnim, &InheritedVisibility, Option<&crate::actors::picking::PickBoxes>),
        (With<crate::actors::character::Character>, Without<Monster>)>,
    net: Option<NonSend<Net>>,
) {
    if ui.over_ui {
        if hovered.0.is_some() { hovered.0 = None; }
        return;
    }
    let ray = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .zip(cameras.single().ok())
        .and_then(|(c, (cam, at))| cam.viewport_to_world(at, c).ok());
    let mut best: Option<(f32, Entity, u32)> = None;
    if let Some(ray) = ray {
        let (o, d) = (ray.origin, *ray.direction);
        for (e, at, m, anim, shown, boxes) in &monsters {
            // Ours, a peer's summon outside PvP, an escort: never a target.
            if !m.targetable || !m.is_alive(anim) || !shown.get() {
                continue;
            }
            // The client's picking boxes on the joints (crate::actors::picking); the
            // collision capsule only for a model without them.
            if let Some(boxes) = boxes.filter(|b| !b.0.is_empty()) {
                if let Some(t) = crate::actors::picking::hit(ray, boxes, &joints) {
                    if best.is_none_or(|(bt, _, _)| t < bt) {
                        best = Some((t, e, m.actor));
                    }
                }
                continue;
            }
            let base = at.translation();
            // Ray against the upright cylinder: nearest approach in the ground
            // plane, then the height there.
            let (ox, oz) = (o.x - base.x, o.z - base.z);
            let (a, b) = (d.x * d.x + d.z * d.z, ox * d.x + oz * d.z);
            if a < 1e-6 {
                continue;
            }
            let t = (-b / a).max(0.0);
            let (px, pz) = (ox + d.x * t, oz + d.z * t);
            if px * px + pz * pz > m.radius * m.radius {
                continue;
            }
            let y = o.y + d.y * t - base.y;
            if !(-0.2..=m.height + 0.2).contains(&y) {
                continue;
            }
            if best.is_none_or(|(bt, _, _)| t < bt) {
                best = Some((t, e, m.actor));
            }
        }
    }
    // A player in open battle while we are too: a target as a monster is
    // (DSOR_Server services/pvp_combat.py _pvp_target_is_hostile: both flagged).
    // UNVERIFIED: the picking capsule without boxes (0.5 m, 2 m tall).
    if let (Some(ray), Some(net)) = (ray, net.as_ref().filter(|n| n.pvp_allowed())) {
        let (o, d) = (ray.origin, *ray.direction);
        for (e, at, r, anim, shown, boxes) in &players {
            if anim.dead || !shown.get() || !net.in_pvp(Some(r.actor)) { continue; }
            let t = if let Some(boxes) = boxes.filter(|b| !b.0.is_empty()) {
                crate::actors::picking::hit(ray, boxes, &joints)
            } else {
                let base = at.translation();
                let (ox, oz) = (o.x - base.x, o.z - base.z);
                let (a, b) = (d.x * d.x + d.z * d.z, ox * d.x + oz * d.z);
                if a < 1e-6 { continue; }
                let t = (-b / a).max(0.0);
                let (px, pz) = (ox + d.x * t, oz + d.z * t);
                let y = o.y + d.y * t - base.y;
                (px * px + pz * pz <= 0.25 && (-0.2..=2.2).contains(&y)).then_some(t)
            };
            if let Some(t) = t.filter(|t| best.is_none_or(|(bt, _, _)| *t < bt)) { best = Some((t, e, r.actor)); }
        }
    }
    // DSOR_TEST_TARGET=1: the nearest live monster counts as under the cursor
    // (testing attacks without a mouse).
    if std::env::var("DSOR_TEST_TARGET").is_ok() {
        if let Some((_, at)) = cameras.single().ok() {
            let eye = at.translation();
            best = monsters
                .iter()
                .filter(|(_, _, m, a, shown, _)| m.targetable && m.is_alive(a) && shown.get())
                .map(|(e, g, m, _, _, _)| (g.translation().distance(eye), e, m.actor))
                .min_by(|a, b| a.0.total_cmp(&b.0));
        }
    }
    let now = best.map(|(_, e, a)| (e, a));
    if hovered.0 != now {
        hovered.0 = now;
    }
}

/// A blow's hit sequences on its victim `e`: one per damage type it carries (the
/// critical row instead on a critical), each in a free instance of its row, the
/// same blow never twice; the heavy hit's own when none is playing.
/// EVIDENCE: ActorSequenceProperty's hit handler 0x4BAF2A and
///   FindUnoccupiedSequenceForDamageType 0x4B9FA0 (4 instances per row).
#[allow(clippy::too_many_arguments)]
fn play_blow(
    commands: &mut Commands,
    sequences: &SequenceTable,
    e: Entity,
    hits: &mut [[(Option<u32>, Option<Entity>); INSTANCES]; 7],
    heavy: &mut Option<Entity>,
    blow: &Blow,
    hit: &HashMap<String, String>,
    critical: &str,
    heavy_seq: &str,
    playing: &Query<(), With<SequencePlayer>>,
) {
    let rows: Vec<(usize, &str)> = if blow.critical {
        vec![(CRITICAL_ROW, critical)]
    } else {
        blow.damage_types
            .iter()
            .filter_map(|&d| DAMAGE_TYPES.get(d as usize).map(|name| (d as usize, hit.get(*name).map(String::as_str).unwrap_or(""))))
            .collect()
    };
    for (row, name) in rows {
        let Some(seq) = sequences.0.get(name) else { continue };
        let slots = &mut hits[row];
        // The same blow twice: nothing new.
        if slots.iter().any(|(tick, _)| *tick == Some(blow.tick)) {
            continue;
        }
        let Some(free) = slots.iter().position(|(_, p)| p.is_none_or(|p| !playing.contains(p))) else { continue };
        let player = crate::skills::play_attached(commands, seq.clone(), e);
        slots[free] = (Some(blow.tick), Some(player));
    }
    if blow.heavy && !heavy.is_some_and(|p| playing.contains(p)) {
        if let Some(seq) = sequences.0.get(heavy_seq) {
            *heavy = Some(crate::skills::play_attached(commands, seq.clone(), e));
        }
    }
}

/// Each player's character surfaces get their own material copy, gathered in its
/// ActorMaterials, so its HitColor (the hit sequences' red flash) colours that
/// player alone; monsters get theirs in setup_models.
#[allow(clippy::type_complexity)]
fn own_player_materials(
    mut commands: Commands,
    meshes: Query<(Entity, &MeshMaterial3d<crate::rendering::surfaces::NebulaMaterial>), (Added<MeshMaterial3d<crate::rendering::surfaces::NebulaMaterial>>, Without<OwnMaterial>)>,
    parents: Query<&ChildOf>,
    players: Query<(), (With<crate::actors::character::Character>, Or<(With<crate::network::net::LocalPlayer>, With<RemotePlayer>)>, Without<Monster>)>,
    mut owned: Query<&mut ActorMaterials>,
    mut materials: ResMut<Assets<crate::rendering::surfaces::NebulaMaterial>>,
) {
    let mut fresh: HashMap<Entity, Vec<Handle<crate::rendering::surfaces::NebulaMaterial>>> = HashMap::new();
    for (e, h) in &meshes {
        let Some(player) = parents.iter_ancestors(e).find(|a| players.contains(*a)) else { continue };
        let Some(m) = materials.get(&h.0).cloned() else { continue };
        commands.entity(e).try_insert(OwnMaterial);
        if m.extension.params.p0.x as u32 != crate::rendering::surfaces::Kind::Character as u32 {
            continue;
        }
        let copy = materials.add(m);
        commands.entity(e).try_insert(MeshMaterial3d(copy.clone()));
        match owned.get_mut(player) {
            Ok(mut o) => o.0.push(copy),
            Err(_) => fresh.entry(player).or_default().push(copy),
        }
    }
    for (player, list) in fresh {
        commands.entity(player).try_insert(ActorMaterials(list));
    }
}

fn apply_shader_colors(
    mut colors: ResMut<ShaderColors>,
    actors: Query<&ActorMaterials>,
    mut materials: ResMut<Assets<crate::rendering::surfaces::NebulaMaterial>>,
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("monsters::apply_shader_colors");
    for (actor, c) in colors.0.drain(..) {
        let Ok(own) = actors.get(actor) else { continue };
        for h in &own.0 {
            if let Some(mut m) = materials.get_mut(h) {
                if m.extension.params.p3 != c {
                    m.extension.params.p3 = c;
                }
            }
        }
    }
}

/// The hovered monster's title (_Template_Monster title, language/en), level and
/// health, for its info window.
fn hover_info(
    hovered: Res<Hovered>,
    monsters: Query<(&Monster, &CharacterAnim)>,
    data: Res<MonsterData>,
    templates: Res<Assets<MonsterTemplates>>,
    mut info: ResMut<HoverInfo>,
    net: Option<NonSend<Net>>,
) {
    // DSOR_TEST_HOVER=boss|monster: the info window shown as for one (screenshots).
    if let Ok(kind) = std::env::var("DSOR_TEST_HOVER") {
        let test = HoverInfo(Some(("Bog Beast".into(), 19, 260, 391)), kind == "boss");
        if *info != test { *info = test; }
        return;
    }
    let boss = hovered.0.and_then(|(e, _)| monsters.get(e).ok())
        .and_then(|(m, _)| net.as_ref()?.monster_ranks.get(&m.actor).map(|r| boss_rank(r))).unwrap_or(false);
    if info.1 != boss {
        info.1 = boss;
    }
    let now = hovered.0.and_then(|(e, _)| monsters.get(e).ok()).filter(|(m, a)| m.is_alive(a)).map(|(m, _)| {
        let title = templates.get(&data.templates).and_then(|t| t.0.get(&m.template)).map(|t| t.title.clone()).unwrap_or_default();
        (title, m.level, m.health, m.max_health)
    // A player in open battle (hover): their name, level and health.
    // FAILURE: "j'ai pas le tooltip en haut avec son nom, son niveau sa vie".
    }).or_else(|| hovered.0.filter(|(e, _)| !monsters.contains(*e))
        .and_then(|(_, actor)| net.as_ref()?.remote_info.get(&actor).cloned()));
    if info.0 != now {
        info.0 = now;
    }
}
