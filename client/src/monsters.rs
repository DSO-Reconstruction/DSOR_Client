//! Monsters: what NewMonsterCommand (48) names, drawn as the 2018 client does, and
//! the blows and deaths the server sends them (HitCommand 115, KillCommand 116).
//!
//! The look is `_Template_Monster` (tools/export_monsters.py):
//! - Graphics `characters/uniskel[_dwarf]`: a human dressed on the shared skeleton,
//!   its CharacterSet an outfit (like the NPCs, crate::npc);
//! - any other Graphics: a whole model holding every look of its rig (troll.glb:
//!   troll, cyclops, gnob, golems...). Its CharacterSet is one of the model's skin
//!   lists (`characters/<model>.sets.json`): only those skins are shown, with the
//!   set's body variation;
//! - AnimSet: the rows of data/tables/anims.xml (`characters/monster_anims.json`),
//!   looked up in the model's own clips.
//! Monsters move as remote actors (crate::net::RemotePlayer: MoveCommand, culling,
//! the relayed skills of crate::skills).
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
//! Deaths: KillCommand plays DeathSequence; the actor keeps its last pose
//!   (CharacterAnim::dead) until DiscardMonster removes it.

use std::collections::HashMap;

use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::gltf::{Gltf, GltfAssetLabel, GltfExtras};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use serde::Deserialize;

use crate::character::{spawn_character, vary_joints, AnimState, CharacterAnim, CharacterDesc, ModelBones, NpcLook};
use crate::net::{now_ms, MonsterSpawn, Net, RemotePlayer};
use crate::npc::{JointShape, Outfits, Variations};
use crate::skills::{play, SequencePlayer, SequenceTable, SkillData};

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
    /// PrimaryColor, SecondaryColor (rgba): the dyes MatDiffuse and MatSpecular.
    #[serde(default)]
    pub dye: [[f32; 4]; 2],
    #[serde(default)]
    pub radius: f32,
    #[serde(default)]
    pub height: f32,
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
    pub skins: Vec<String>,
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
json_loader!(MonsterTemplatesLoader, MonsterTemplates, "monster_templates.json");
json_loader!(MonsterAnimsLoader, MonsterAnims, "monster_anims.json");
json_loader!(ModelSetsLoader, ModelSets, "sets.json");

#[derive(Resource)]
struct MonsterData {
    templates: Handle<MonsterTemplates>,
    anims: Handle<MonsterAnims>,
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
    /// Per row and instance: the hit tick it last played and its sequence player.
    hits: [[(Option<u32>, Option<Entity>); INSTANCES]; 7],
    heavy: Option<Entity>,
}

/// What the debug menu (crate::debug) asks for: monsters to place, and blows on
/// them (actor; 0 a hit of the next damage type, 1 a critical, 2 a kill).
#[derive(Resource, Default)]
pub struct DebugMonsters {
    pub spawns: Vec<MonsterSpawn>,
    pub blows: Vec<(u32, u8)>,
    /// Offline play: the local player's skills landing, as no server answers them
    /// (crate::skills): (seconds to go, where, radius, the aimed monster).
    pub strikes: Vec<(f32, Vec3, f32, Option<u32>)>,
    next_type: u8,
    tick: u32,
}

/// HitColor writes of the sequencer (crate::skills, ColorShaderParameterTrackBar):
/// actor, colour (alpha: how much of it replaces the shaded colour).
#[derive(Resource, Default)]
pub struct ShaderColors(pub Vec<(Entity, Vec4)>);

/// An actor's own copies of its character materials (crate::surfaces kind
/// Character): its dyes, and the HitColor its sequences flash.
#[derive(Component, Default)]
pub struct ActorMaterials(pub Vec<Handle<crate::surfaces::NebulaMaterial>>);

/// A whole model waiting for its scene: skins, variation, animations.
#[derive(Component)]
struct ModelSetup {
    dye: [[f32; 4]; 2],
    gltf: Handle<Gltf>,
    sets: Handle<ModelSets>,
    set: String,
    anim_set: String,
}

/// Offline (map viewer): DSOR_TEST_MONSTER=<blueprint>[,x,y,z] places a monster;
/// DSOR_TEST_HIT=<seconds>[,critical] hits it every that many seconds with each
/// damage type in turn, then kills it after six blows.
#[derive(Resource, Default)]
struct OfflineMonster {
    placed: bool,
    next: f32,
    blows: u32,
}

/// The live monster under the cursor, if any (crate::skills targets it, a left
/// click attacks it instead of walking).
/// UNVERIFIED: the 2018 client picks through per-joint picking boxes
/// (export_win32/picking); this is the collision capsule, upright.
#[derive(Resource, Default)]
pub struct Hovered(pub Option<(Entity, u32)>);

pub struct MonstersPlugin;

impl Plugin for MonstersPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<MonsterTemplates>()
            .init_asset::<MonsterAnims>()
            .init_asset::<ModelSets>()
            .register_asset_loader(MonsterTemplatesLoader)
            .register_asset_loader(MonsterAnimsLoader)
            .register_asset_loader(ModelSetsLoader)
            .init_resource::<OfflineMonster>()
            .init_resource::<Hovered>()
            .init_resource::<ShaderColors>()
            .init_resource::<DebugMonsters>()
            .add_systems(PostUpdate, apply_shader_colors)
            .add_systems(PreUpdate, hover)
            .add_systems(Startup, load)
            .add_systems(Update, (spawn_monsters, setup_models, monster_health, hits_and_kills).chain());
    }
}

fn load(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(MonsterData {
        templates: assets.load("characters/monster_templates.json"),
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
) {
    if net.is_none() && !test.placed {
        if let Ok(spec) = std::env::var("DSOR_TEST_MONSTER") {
            let mut it = spec.split(',');
            let blueprint = it.next().unwrap_or_default().to_owned();
            let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
            let position = if v.len() == 3 { Vec3::new(v[0], v[1], v[2]) } else { Vec3::new(123.5, 12.0, 73.4) };
            offline.push(MonsterSpawn { actor: 0x6000_0000, blueprint, position, health: 100, level: 1 });
            test.placed = true;
        }
    }
    let queue = match net {
        Some(net) => &mut net.into_inner().monster_spawns,
        None => &mut *offline,
    };
    queue.extend(debug.spawns.drain(..));
    if queue.is_empty() {
        return;
    }
    let Some(templates) = templates.get(&data.templates) else { return };
    let ready = |h: &Handle<Outfits>| outfits.get(h).is_some() || assets.load_state(h).is_failed();
    if !data.outfits.values().all(ready) {
        return;
    }
    for spawn in std::mem::take(queue) {
        for (e, r) in &existing {
            if r.actor == spawn.actor {
                commands.entity(e).despawn();
            }
        }
        let Some(t) = templates.0.get(&spawn.blueprint) else {
            warn!("monster {} has no template", spawn.blueprint);
            continue;
        };
        let transform = Transform::from_translation(spawn.position);
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
                look: Some(NpcLook { skeleton, parts: outfit.parts.clone(), anim_set: t.anim_set.clone(), variation }),
                ..default()
            };
            spawn_character(&mut commands, desc, transform)
        } else {
            let path = format!("{}.glb", t.graphics);
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
                    },
                ))
                .id();
            commands.spawn((WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))), ChildOf(e), Transform::default(), Visibility::default()));
            e
        };
        // A looping start state (asleep, burrowed...) is kept; Idle otherwise.
        if t.loop_start && t.state != "Idle" {
            let mut anim = CharacterAnim::default();
            anim.state = AnimState::Named(t.state.clone());
            commands.entity(entity).insert(anim);
        }
        commands.entity(entity).insert((
            Name::new(format!("monster {}", spawn.blueprint)),
            Monster {
                actor: spawn.actor,
                template: spawn.blueprint.clone(),
                health: spawn.health,
                max_health: spawn.health,
                level: spawn.level,
                radius: t.radius.max(0.3),
                height: t.height.max(1.0),
                hits: Default::default(),
                heavy: None,
            },
            RemotePlayer { actor: spawn.actor, target: spawn.position, facing: 0.0, still_for: 1.0 },
        ));
    }
}

/// A whole model whose scene is in: show the set's skins only, give it the set's
/// body, and drive its own animation player through the AnimSet's rows.
#[allow(clippy::too_many_arguments)]
fn setup_models(
    mut commands: Commands,
    mut pending: Query<(Entity, &ModelSetup, &mut CharacterAnim, &mut Monster)>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    names: Query<&Name>,
    extras: Query<&GltfExtras>,
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
        Query<&MeshMaterial3d<crate::surfaces::NebulaMaterial>>,
        ResMut<Assets<crate::surfaces::NebulaMaterial>>,
    ),
) {
    let Some(table) = anims.get(&data.anims) else { return };
    for (e, setup, mut anim, mut monster) in &mut pending {
        let Some(gltf) = gltfs.get(&setup.gltf) else { continue };
        let Some(player) = children.iter_descendants(e).find(|c| players.contains(*c)) else { continue };
        let set_file = sets.get(&setup.sets);
        if set_file.is_none() && !assets.load_state(&setup.sets).is_failed() {
            continue;
        }
        let set = set_file.and_then(|s| s.0.get(&setup.set));
        if set_file.is_some() && set.is_none() {
            warn!("model {:?}: no character set {}", setup.gltf.path(), setup.set);
        }
        let mut bones = HashMap::new();
        for d in children.iter_descendants(e) {
            if let Some(set) = set {
                let skin = extras
                    .get(d)
                    .ok()
                    .and_then(|x| serde_json::from_str::<serde_json::Value>(&x.value).ok())
                    .and_then(|v| v.get("dsor_skin").and_then(|s| s.as_str()).map(str::to_owned));
                if skin.is_some_and(|s| !set.skins.contains(&s)) {
                    commands.entity(d).insert(Visibility::Hidden);
                }
            }
            if let Ok(n) = names.get(d) {
                bones.entry(n.as_str().to_owned()).or_insert(d);
            }
            // Hooked again: bones unhooked while the scene had no driver
            // (crate::anim_cull, crate::character's idle pruning).
            if targets.contains(d) {
                commands.entity(d).insert(AnimatedBy(player));
            }
        }
        if let Some(root) = set.and_then(|s| s.variation.iter().find(|j| j.bone == "Skeleton_Root")) {
            let scale = root.s[0].max(root.s[2]);
            monster.radius *= scale;
            monster.height *= root.s[1];
        }
        if let Some(set) = set.filter(|s| !s.variation.is_empty()) {
            let shape: Vec<(String, Vec3, Vec3)> = set.variation.iter().map(|j| (j.bone.clone(), Vec3::from(j.t), Vec3::from(j.s))).collect();
            let scaled = vary_joints(&mut commands, &joints, &bones, &shape);
            // Each varied joint's vertices follow its scaled child.
            let by_joint: HashMap<Entity, Entity> = scaled.iter().filter_map(|(n, s)| bones.get(n).map(|j| (*j, *s))).collect();
            for d in children.iter_descendants(e) {
                if let Ok(skin) = skinned.get(d) {
                    let joints = skin.joints.iter().map(|j| by_joint.get(j).copied().unwrap_or(*j)).collect();
                    commands.entity(d).insert(SkinnedMesh { inverse_bindposes: skin.inverse_bindposes.clone(), joints });
                }
            }
        }
        // Its own copies of its character materials: its template's dyes, and a
        // HitColor of its own.
        let mut own = Vec::new();
        for d in children.iter_descendants(e) {
            let Ok(MeshMaterial3d(h)) = surfaces.get(d) else { continue };
            let Some(mut m) = surface_assets.get(h).cloned() else { continue };
            if m.extension.params.p0.x as u32 != crate::surfaces::Kind::Character as u32 {
                continue;
            }
            m.extension.params.p1 = Vec4::from_array(setup.dye[0]);
            m.extension.params.p2 = Vec4::from_array(setup.dye[1]);
            let copy = surface_assets.add(m);
            commands.entity(d).insert(MeshMaterial3d(copy.clone()));
            own.push(copy);
        }
        commands.entity(e).insert(ActorMaterials(own));
        let mut graph = AnimationGraph::new();
        let mut nodes = HashMap::new();
        if let Some(rows) = table.0.get(&setup.anim_set) {
            for (state, clip) in rows {
                let looped = format!("{clip}-loop");
                let found = gltf.named_animations.get(looped.as_str()).map(|h| (h, true)).or_else(|| gltf.named_animations.get(clip.as_str()).map(|h| (h, false)));
                if let Some((handle, looping)) = found {
                    nodes.insert(state.clone(), (graph.add_clip(handle.clone(), 1.0, graph.root), looping));
                }
            }
        } else {
            warn!("model {:?}: no animation set {}", setup.gltf.path(), setup.anim_set);
        }
        commands
            .entity(player)
            .insert((AnimationGraphHandle(graphs.add(graph)), crate::anim_cull::ManagedAnimation));
        anim.bind_model(player, nodes);
        commands.entity(e).insert(ModelBones(bones)).remove::<ModelSetup>();
    }
}

fn monster_health(net: Option<NonSendMut<Net>>, mut monsters: Query<&mut Monster>) {
    let Some(mut net) = net else { return };
    for (actor, health, max) in std::mem::take(&mut net.monster_health) {
        if let Some(mut m) = monsters.iter_mut().find(|m| m.actor == actor) {
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
}

#[allow(clippy::too_many_arguments)]
fn hits_and_kills(
    mut commands: Commands,
    time: Res<Time>,
    real: Res<Time<Real>>,
    net: Option<NonSendMut<Net>>,
    mut test: ResMut<OfflineMonster>,
    skills: Res<SkillData>,
    seqs: Res<Assets<SequenceTable>>,
    data: Res<MonsterData>,
    templates: Res<Assets<MonsterTemplates>>,
    mut monsters: Query<(Entity, &mut Monster, &mut CharacterAnim)>,
    playing: Query<(), With<SequencePlayer>>,
    (locals, remotes, mut texts, mut debug, transforms): (
        Query<(Entity, &crate::net::LocalPlayer)>,
        Query<(Entity, &RemotePlayer), Without<Monster>>,
        ResMut<crate::combat_text::CombatTexts>,
        ResMut<DebugMonsters>,
        Query<&GlobalTransform>,
    ),
) {
    let (Some(sequences), Some(templates)) = (seqs.get(skills.sequences()), templates.get(&data.templates)) else { return };
    let mut blows = Vec::new();
    let mut deaths = Vec::new();
    // Offline the test blows are the local player's own.
    let mut local = LOCAL_TEST;
    match net {
        Some(mut net) => {
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
                });
            }
            deaths.extend(std::mem::take(&mut net.kills).into_iter().map(|(victim, _)| victim));
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
    debug.strikes.retain_mut(|(after, at, radius, target)| {
        *after -= dt;
        if *after > 0.0 {
            return true;
        }
        landed.push((*at, *radius, *target));
        false
    });
    for (at, radius, target) in landed {
        let near = |p: Vec3| Vec2::new(p.x - at.x, p.z - at.z).length() <= radius;
        let hit: Vec<u32> = monsters
            .iter()
            .filter(|(e, m, a)| !a.dead && (target == Some(m.actor) || (target.is_none() && near(transforms.get(*e).map(|t| t.translation()).unwrap_or(Vec3::INFINITY)))))
            .map(|(_, m, _)| m.actor)
            .collect();
        for actor in hit {
            debug.blows.push((actor, 0));
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
        // Debug monsters have 100 health: the blow that empties it kills.
        if let Some((_, mut m, _)) = monsters.iter_mut().find(|(_, m, _)| m.actor == victim) {
            if victim >= 0x6100_0000 {
                m.health = m.health.saturating_sub(damage as u32);
                if m.health == 0 {
                    deaths.push(victim);
                }
            }
        }
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
        });
    }
    for blow in blows {
        // Floating texts (crate::combat_text), for blows given or taken by the
        // local player (HitCommand's handler 0x516453).
        let victim_entity = monsters
            .iter()
            .find(|(_, m, _)| m.actor == blow.victim)
            .map(|(e, ..)| e)
            .or_else(|| locals.iter().find(|(_, l)| l.actor == blow.victim).map(|(e, _)| e))
            .or_else(|| remotes.iter().find(|(_, r)| r.actor == blow.victim).map(|(e, _)| e));
        let mine = blow.victim == local;
        if let (Some(v), true) = (victim_entity, (mine || blow.attacker == local) && blow.victim != blow.attacker) {
            use crate::combat_text::*;
            let pick = |own: u8, other: u8| if mine { own } else { other };
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
                texts.0.push((v, pick(LOW_HEALTH_HIT_RECEIVED, LOW_HEALTH_HIT), Say::LowHealth));
            }
        }
        // The combat value (life drained...) over its owner, when it is the local
        // player and the value is positive.
        if blow.owner == local && blow.value > 0.0 {
            if let Some((e, _)) = locals.iter().next() {
                texts.0.push((e, crate::combat_text::HEALING_SELF, crate::combat_text::Say::Healing((blow.damage as f32 * blow.value) as i32)));
            }
        }
        let Some((e, mut m, anim)) = monsters.iter_mut().find(|(_, m, _)| m.actor == blow.victim) else { continue };
        if let Some((health, max)) = blow.health {
            m.health = health;
            m.max_health = max;
        }
        if anim.dead {
            continue;
        }
        let Some(t) = templates.0.get(&m.template) else { continue };
        let rows: Vec<(usize, &str)> = if blow.critical {
            vec![(CRITICAL_ROW, t.critical.as_str())]
        } else {
            blow.damage_types
                .iter()
                .filter_map(|&d| DAMAGE_TYPES.get(d as usize).map(|name| (d as usize, t.hit.get(*name).map(String::as_str).unwrap_or(""))))
                .collect()
        };
        for (row, name) in rows {
            let Some(seq) = sequences.0.get(name) else { continue };
            let slots = &mut m.hits[row];
            // The same blow twice: nothing new.
            if slots.iter().any(|(tick, _)| *tick == Some(blow.tick)) {
                continue;
            }
            let Some(free) = slots.iter().position(|(_, p)| p.is_none_or(|p| !playing.contains(p))) else { continue };
            let player = play(&mut commands, seq.clone(), Some(e), e, false);
            slots[free] = (Some(blow.tick), Some(player));
        }
        if blow.heavy && !m.heavy.is_some_and(|p| playing.contains(p)) {
            if let Some(seq) = sequences.0.get(&t.heavy) {
                m.heavy = Some(play(&mut commands, seq.clone(), Some(e), e, false));
            }
        }
    }
    for victim in deaths {
        let Some((e, m, mut anim)) = monsters.iter_mut().find(|(_, m, _)| m.actor == victim) else { continue };
        if anim.dead {
            continue;
        }
        anim.dead = true;
        anim.state = AnimState::Named("Death".into());
        anim.replay();
        if let Some(seq) = templates.0.get(&m.template).and_then(|t| sequences.0.get(&t.death)) {
            play(&mut commands, seq.clone(), Some(e), e, false);
        }
    }
}

fn hover(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    monsters: Query<(Entity, &GlobalTransform, &Monster, &CharacterAnim, &InheritedVisibility)>,
    mut hovered: ResMut<Hovered>,
) {
    let ray = windows
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .zip(cameras.single().ok())
        .and_then(|(c, (cam, at))| cam.viewport_to_world(at, c).ok());
    let mut best: Option<(f32, Entity, u32)> = None;
    if let Some(ray) = ray {
        let (o, d) = (ray.origin, *ray.direction);
        for (e, at, m, anim, shown) in &monsters {
            if anim.dead || !shown.get() {
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
    // DSOR_TEST_TARGET=1: the nearest live monster counts as under the cursor
    // (testing attacks without a mouse).
    if std::env::var("DSOR_TEST_TARGET").is_ok() {
        if let Some((_, at)) = cameras.single().ok() {
            let eye = at.translation();
            best = monsters
                .iter()
                .filter(|(_, _, _, a, _)| !a.dead)
                .map(|(e, g, m, _, _)| (g.translation().distance(eye), e, m.actor))
                .min_by(|a, b| a.0.total_cmp(&b.0));
        }
    }
    let now = best.map(|(_, e, a)| (e, a));
    if hovered.0 != now {
        hovered.0 = now;
    }
}

fn apply_shader_colors(
    mut colors: ResMut<ShaderColors>,
    actors: Query<&ActorMaterials>,
    mut materials: ResMut<Assets<crate::surfaces::NebulaMaterial>>,
) {
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
