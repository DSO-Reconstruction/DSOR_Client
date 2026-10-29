//! NPCs: what NewNPCCommand (44) names, dressed and placed as the 2018 client does.
//!
//! The command carries the template Id, the level Guid, a position and a visible
//! bit (dsor_proto NewNpc). The rest comes from the client's data
//! (tools/export_npcs.py):
//! - `characters/npc_templates.json`: `_Template_NPC` Graphics, CharacterSet,
//!   AnimSet, StartAnimationState, and the English name (db.npc.xml `<Id>Title`);
//! - `maps/<map>.npcs.json`: `_Instance_NPC` by Guid, with the placement matrix
//!   (its rotation is the facing, which the command does not carry);
//! - `characters/<skeleton>/outfits.json` (DSO_Godot): a CharacterSet's parts.
//! Humans and dwarves (Graphics `characters/uniskel[_dwarf]`) are dressed on the
//! shared skeleton like players, with their outfit's body variation; other
//! Graphics are whole models with their own animations (barrel_dealer, gambler,
//! troll...) and their character set's variation (`characters/<model>.sets.json`,
//! as the monsters'): without it the deco pigeons stood a quarter of their size.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use serde::Deserialize;

use crate::actors::character::{attach_character, spawn_character, AnimState, AnimTable, Character, CharacterAnim, CharacterDesc, CharacterLibrary, CharacterVisual, NpcLook};
use crate::world::map::CurrentMap;

use crate::network::net::{Net, NpcSpawn};

#[derive(Deserialize, Debug, Clone, Default)]
pub struct NpcTemplate {
    pub graphics: String,
    pub outfit: String,
    pub anim_set: String,
    pub state: String,
    #[serde(default)]
    pub map_icon: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub portrait: String,
    #[serde(default)]
    pub no_talk: bool,
    /// _Template_NPC.SwitchPVPMode: the conversation offers the PvP switch.
    #[serde(default)]
    pub switch_pvp: bool,
    #[serde(default)]
    pub shop: String,
    #[serde(default)]
    pub profession: String,
    /// Display label from ProfessionLocaId (not the internal profession id).
    #[serde(default)]
    pub profession_title: String,
    /// Authored adjustment to the status model's built-in overhead height.
    #[serde(default)]
    pub overhead_offset: [f32; 3],
}

impl NpcTemplate {
    /// An instance's own look (`npcs.json` "look") over its template: what the
    /// instance sets wins, the rest is the template's. The level tool's
    /// sidecars (crates/level-data export.rs) carry only the appearance and the
    /// title, so taken alone the NPC lost its portrait (the quest window's
    /// placeholder face), map icon, shop and profession (2026-10-21).
    fn over(&self, base: Option<&NpcTemplate>) -> NpcTemplate {
        let Some(base) = base else { return self.clone() };
        let pick = |own: &String, theirs: &String| if own.is_empty() { theirs.clone() } else { own.clone() };
        NpcTemplate {
            graphics: pick(&self.graphics, &base.graphics),
            outfit: pick(&self.outfit, &base.outfit),
            anim_set: pick(&self.anim_set, &base.anim_set),
            state: pick(&self.state, &base.state),
            map_icon: pick(&self.map_icon, &base.map_icon),
            title: pick(&self.title, &base.title),
            portrait: pick(&self.portrait, &base.portrait),
            no_talk: self.no_talk || base.no_talk,
            switch_pvp: self.switch_pvp || base.switch_pvp,
            shop: pick(&self.shop, &base.shop),
            profession: pick(&self.profession, &base.profession),
            profession_title: pick(&self.profession_title, &base.profession_title),
            overhead_offset: if self.overhead_offset == [0.0; 3] { base.overhead_offset } else { self.overhead_offset },
        }
    }
}

#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct NpcTemplates(pub HashMap<String, NpcTemplate>);

#[derive(Deserialize, Debug)]
pub struct Placement {
    pub id: String,
    /// DirectX row-major 4x4: rows are the axes, the last row the position, which
    /// is glam's column-major layout read as is.
    pub m: [f32; 16],
    pub look: Option<NpcTemplate>,
    /// Authored event-set membership. Older sidecars predate this metadata.
    #[serde(default = "always_present")]
    pub event: i32,
    #[serde(default)]
    pub event_flags: Option<String>,
}

fn always_present() -> i32 { -1 }

impl Placement {
    /// The offline viewer starts with no active events. Negative-only rules
    /// still enable ordinary actors (for example the pre-quest prince).
    /// Online, the server announces every NPC and the running events decide (GatedNpcs).
    fn enabled_offline(&self) -> bool {
        self.event == -1 || self.event_flags.as_ref().is_some_and(|rule| {
            !rule.split(';').filter_map(|d| d.split_once('='))
                .any(|(key, value)| !key.trim().is_empty() && value.trim().eq_ignore_ascii_case("true"))
        })
    }
}

#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct NpcPlacements(pub HashMap<String, Placement>);

#[derive(Deserialize, Debug)]
pub struct Outfit {
    pub name: String,
    pub parts: Vec<String>,
    #[serde(default)]
    pub variation: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct JointShape {
    pub bone: String,
    pub t: [f32; 3],
    pub s: [f32; 3],
}

/// `characters/<skeleton>/variations.json` (DSO_Godot): body shapes by name.
#[derive(Asset, TypePath, Deserialize, Debug)]
pub struct Variations {
    pub variations: HashMap<String, Vec<JointShape>>,
}

#[derive(Asset, TypePath, Deserialize, Debug)]
pub struct Outfits {
    pub outfits: Vec<Outfit>,
}

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
json_loader!(NpcTemplatesLoader, NpcTemplates, "npc_templates.json");
json_loader!(NpcPlacementsLoader, NpcPlacements, "npcs.json");
json_loader!(OutfitsLoader, Outfits, "outfits.json");
json_loader!(VariationsLoader, Variations, "variations.json");
json_loader!(DecoLoader, DecoFile, "deco.json");

/// `maps/<map>.deco.json` (tools/export_deco.py): the level's decorative
/// sequences -- the guards at their posts, townsfolk, pigeons, cats, light beams
/// -- which the client plays itself, online too: no server sends them.
#[derive(Asset, TypePath, Deserialize, Debug)]
pub struct DecoFile {
    chars: Vec<DecoChar>,
    seqs: Vec<DecoSeq>,
}

#[derive(Deserialize, Debug)]
struct DecoChar {
    g: String,
    outfit: String,
    /// The anim set playing its clip as "Idle" (characters/anims.json "deco:...").
    set: String,
    m: [f32; 16],
    speed: f32,
}

#[derive(Deserialize, Debug)]
struct DecoSeq {
    seq: String,
    m: [f32; 16],
}

/// The current map's decorations: (map, file, spawned for it, entities).
#[derive(Resource, Default)]
struct Deco {
    file: Option<(String, Handle<DecoFile>)>,
    spawned: HashMap<usize, Entity>,
    pending: bool,
}

#[derive(Resource)]
struct NpcData {
    templates: Handle<NpcTemplates>,
    outfits: HashMap<&'static str, Handle<Outfits>>,
    variations: HashMap<&'static str, Handle<Variations>>,
    /// (map name, its placements).
    placements: Option<(String, Handle<NpcPlacements>)>,
}

/// Map viewer (`--npcs` / `?npcs`): stream the level's nearby NPC placements.
/// Online, the server decides which actors are present.
#[derive(Resource, Default)]
pub struct OfflineNpcs {
    pub queued: Vec<crate::network::net::NpcSpawn>,
    done_for: Option<String>,
    resident: std::collections::HashSet<u32>,
    pending: bool,
}

fn offline_npcs(
    mut commands: Commands,
    offline: Option<ResMut<OfflineNpcs>>,
    data: Res<NpcData>,
    placements: Res<Assets<NpcPlacements>>,
    current: Option<Res<CurrentMap>>,
    manifests: Res<Assets<crate::world::map::MapManifest>>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    players: Query<&GlobalTransform, crate::diagnostics::debug::FollowedPlayer>,
    existing: Query<(Entity, &Npc)>,
) {
    let Some(mut offline) = offline else { return };
    let Some((map, handle)) = &data.placements else { return };
    if offline.done_for.as_ref() != Some(map) {
        offline.resident.clear();
        offline.queued.clear();
        offline.pending = false;
        offline.done_for = Some(map.clone());
    }
    let Some(placed) = placements.get(handle) else { return };
    let Ok(cam) = cameras.single() else { return };
    let ground = current.as_ref().and_then(|c| manifests.get(&c.manifest)).map(|m| m.center[1]).unwrap_or(0.0);
    let focus = crate::world::map::camera_focus(cam, players.iter().next().map(|p| p.translation()), ground);
    let mut added = 0;
    offline.pending = false;
    // Diagnostic comparison with the historical viewer population, which
    // included every event at once. The default obeys authored event rules.
    let all_placements = crate::flag("allnpcplacements");
    for (i, (guid, p)) in placed.0.iter().enumerate() {
        let actor = 0x7000_0000 + i as u32;
        let distance = Vec2::new(p.m[12], p.m[14]).distance(focus.xz());
        let enabled = all_placements || p.enabled_offline();
        if offline.resident.contains(&actor) {
            if !enabled || distance > crate::world::map::UNLOAD_DISTANCE {
                offline.resident.remove(&actor);
                offline.queued.retain(|s| s.actor != actor);
                for (entity, npc) in &existing {
                    if npc.actor == actor { commands.entity(entity).try_despawn(); }
                }
            }
            continue;
        }
        if !enabled || distance > crate::world::map::LOAD_DISTANCE { continue; }
        if added >= 4 { offline.pending = true; continue; }
        offline.queued.push(crate::network::net::NpcSpawn {
            actor,
            template: p.id.clone(),
            guid: guid.clone(),
            position: Vec3::new(p.m[12], p.m[13], p.m[14]),
            visible: true,
        });
        offline.resident.insert(actor);
        added += 1;
    }
}

/// An NPC on the map.
#[derive(Component, Default)]
pub struct Npc {
    pub actor: u32,
    pub template: String,
    pub profile: NpcTemplate,
}

/// A whole-model NPC whose idle has not started yet.
#[derive(Component)]
struct ModelNpc {
    gltf: Handle<Gltf>,
    /// The model's character sets and the one it wears: its body variation
    /// (crate::actors::monsters::vary_model). None for a model without a set.
    sets: Option<(Handle<crate::actors::monsters::ModelSets>, String)>,
    anim_set: String,
    state: String,
    speed: f32,
    /// A sequence's model plays its clip once and holds the last pose
    /// (spawn_sequence_model); level NPCs loop theirs.
    once: bool,
    /// When a sequence placed it (Time's seconds): its clip starts that far in
    /// once the model is in, in step with the sequence that placed it.
    born: Option<f32>,
}

/// A whole model a sequence's character track places (CharacterTrackBar
/// "<skeleton>/<set>/<clip>": the gremlin of the Gremlin Toss emote,
/// meanie_rascal/event_olympia_meanie_rascal_01/event_olympia_throw), playing
/// that clip. crate::gameplay::skills::play_sequences removes it at the
/// track's end.
pub(crate) fn spawn_sequence_model(commands: &mut Commands, assets: &AssetServer, object: &str, speed: f32, transform: Transform, now: f32) -> Option<Entity> {
    let mut parts = object.splitn(3, '/');
    let (skeleton, set, clip) = (parts.next()?, parts.next()?, parts.next()?);
    let graphics = format!("characters/{skeleton}");
    let path = crate::rendering::model_variant::path(&graphics, set);
    let e = commands.spawn((Name::new(format!("sequence model {object}")), transform, Visibility::default(),
        ModelNpc { once: true, born: Some(now), ..ModelNpc::new(assets, &graphics, set, "", clip, speed) })).id();
    commands.spawn((WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))), ChildOf(e), Transform::default(), Visibility::default()));
    Some(e)
}

impl ModelNpc {
    fn new(assets: &AssetServer, graphics: &str, set: &str, anim_set: &str, state: &str, speed: f32) -> Self {
        Self {
            gltf: assets.load(crate::rendering::model_variant::path(graphics, set)),
            sets: (!set.is_empty()).then(|| (assets.load(format!("{graphics}.sets.json")), set.to_owned())),
            anim_set: anim_set.to_owned(),
            state: state.to_owned(),
            speed,
            once: false,
            born: None,
        }
    }
}

/// Only server-owned NPCs keep their actor while visual descendants stream.
/// Recipes contain no asset handles: absent rigs do not retain model assets.
#[derive(Component)]
struct OnlineNpcVisual {
    recipe: NpcVisualRecipe,
    resident: bool,
}

enum NpcVisualRecipe {
    Character(CharacterDesc),
    Model(String),
}

/// Prevent interaction/effect attachment until the streamed visual is ready.
#[derive(Component)]
pub(crate) struct NpcVisualPending;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct NpcVisualSystems;

pub struct NpcPlugin;

mod loading;
pub(crate) use loading::NpcLoading;

impl Plugin for NpcPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(super::npc_markers::NpcMarkersPlugin)
            .init_asset::<NpcTemplates>()
            .init_asset::<NpcPlacements>()
            .init_asset::<Outfits>()
            .init_asset::<Variations>()
            .register_asset_loader(VariationsLoader)
            .register_asset_loader(NpcTemplatesLoader)
            .register_asset_loader(NpcPlacementsLoader)
            .register_asset_loader(OutfitsLoader)
            .init_asset::<DecoFile>()
            .register_asset_loader(DecoLoader)
            .init_resource::<Deco>()
            .init_resource::<NpcLoading>()
            .add_systems(PostUpdate, loading::check.after(TransformSystems::Propagate)
                .before(crate::screens::loading::readiness::check))
            .add_systems(Startup, load)
            .add_systems(Update, (follow_map, offline_npcs, spawn_npcs, stream_online_npcs, spawn_deco,
                animate_models, finish_online_visuals, cull_actors, npc_no_shadow)
                .chain().in_set(NpcVisualSystems).after(crate::network::net::NetSystems));
    }
}

fn load(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(NpcData {
        templates: assets.load("characters/npc_templates.json"),
        outfits: ["uniskel", "uniskel_dwarf"]
            .into_iter()
            .map(|s| (s, assets.load(format!("characters/{s}/outfits.json"))))
            .collect(),
        variations: ["uniskel", "uniskel_dwarf"]
            .into_iter()
            .map(|s| (s, assets.load(format!("characters/{s}/variations.json"))))
            .collect(),
        placements: None,
    });
}

fn follow_map(current: Option<Res<CurrentMap>>, mut data: ResMut<NpcData>, assets: Res<AssetServer>) {
    let Some(current) = current else { return };
    if data.placements.as_ref().is_some_and(|(m, _)| *m == current.name) {
        return;
    }
    data.placements = Some((current.name.clone(), assets.load(format!("maps/{}.npcs.json", current.name))));
}

/// Online NPCs that belong to a map event (`_Instance_NPC.EventSetRow`): the
/// server announces every NPC of the map, those of events not running too, and
/// leaves it to the client to show only the ones the running events enable
/// (DSOR_Server services/player_init_world.py, include_inactive_event_sets: "The
/// client owns event visibility"). They follow the same EventFlags rules as the
/// map's props (world/props.rs NetworkProps::rule_enabled): Kingshill's
/// anniversary Crafting Cake or the greed fiends stay hidden outside their event.
#[derive(Default)]
struct GatedNpcs {
    map: String,
    /// The props/events revision last applied.
    revision: Option<u64>,
    /// Every event NPC announced on this map, shown or not, by actor.
    held: HashMap<u32, NpcSpawn>,
    shown: std::collections::HashSet<u32>,
}

/// The EventFlags rule of the map event this NPC belongs to, if any.
fn event_rule<'a>(placed: Option<&'a NpcPlacements>, spawn: &NpcSpawn) -> Option<&'a str> {
    let placement = placed?.0.get(&spawn.guid)?;
    if placement.event < 0 { return None; }
    placement.event_flags.as_deref()
}

#[allow(clippy::too_many_arguments)]
fn spawn_npcs(
    mut commands: Commands,
    net: Option<NonSendMut<Net>>,
    offline: Option<ResMut<OfflineNpcs>>,
    data: Res<NpcData>,
    templates: Res<Assets<NpcTemplates>>,
    placements: Res<Assets<NpcPlacements>>,
    outfits: Res<Assets<Outfits>>,
    variations: Res<Assets<Variations>>,
    assets: Res<AssetServer>,
    existing: Query<(Entity, &Npc)>,
    (current, mut gated): (Option<Res<CurrentMap>>, Local<GatedNpcs>),
) {
    let (queue, gone, online, props) = match (net, offline) {
        (Some(net), _) if !net.offline => {
            let net = net.into_inner();
            (&mut net.npc_spawns, std::mem::take(&mut net.npc_gone), true, Some(&net.props))
        }
        (_, Some(offline)) => (&mut offline.into_inner().queued, Vec::new(), false, None),
        _ => return,
    };
    let map = current.as_ref().map(|c| c.name.as_str()).unwrap_or_default();
    if gated.map != map {
        *gated = GatedNpcs { map: map.to_owned(), ..default() };
    }
    for actor in gone {
        gated.held.remove(&actor);
        gated.shown.remove(&actor);
        for (e, n) in &existing {
            if n.actor == actor {
                commands.entity(e).despawn();
            }
        }
    }
    let revision = props.map(|p| p.revision());
    let events_changed = props.is_some() && gated.revision != revision;
    if queue.is_empty() && !events_changed {
        return;
    }
    let Some(templates) = templates.get(&data.templates) else { return };
    // The map's placements, once loaded (or known missing: then no facing).
    let placed = data.placements.as_ref().map(|(_, h)| h);
    let placed_ready = placed.is_none_or(|h| placements.get(h).is_some() || assets.load_state(h).is_failed());
    let outfits_ready = data.outfits.values().all(|h| outfits.get(h).is_some() || assets.load_state(h).is_failed())
        && data.variations.values().all(|h| variations.get(h).is_some() || assets.load_state(h).is_failed());
    if !placed_ready || !outfits_ready {
        return;
    }
    let placed = placed.and_then(|h| placements.get(h));
    // A map event's NPCs come and go with its EventUpdate: re-offer the held
    // ones it now enables, take away the shown ones it no longer does.
    if let (Some(props), true) = (props, events_changed) {
        gated.revision = revision;
        let held: Vec<NpcSpawn> = gated.held.values().cloned().collect();
        for spawn in held {
            let Some(rule) = event_rule(placed, &spawn) else { continue };
            if props.rule_enabled(rule) {
                if !gated.shown.contains(&spawn.actor) { queue.push(spawn); }
            } else if gated.shown.remove(&spawn.actor) {
                for (e, n) in &existing {
                    if n.actor == spawn.actor { commands.entity(e).despawn(); }
                }
            }
        }
    }
    for spawn in crate::network::net::last_per_actor(std::mem::take(queue), |s| s.actor) {
        for (e, n) in &existing {
            if n.actor == spawn.actor {
                commands.entity(e).despawn();
            }
        }
        if let (Some(props), Some(rule)) = (props, event_rule(placed, &spawn)) {
            gated.held.insert(spawn.actor, spawn.clone());
            if !props.rule_enabled(rule) {
                gated.shown.remove(&spawn.actor);
                continue;
            }
            gated.shown.insert(spawn.actor);
        }
        if !spawn.visible || std::env::var("DSOR_NPC_ONLY").is_ok_and(|v| !v.split(',').any(|t| t == spawn.template)) {
            continue;
        }
        let placement = placed.and_then(|p| p.0.get(&spawn.guid));
        let base = templates.0.get(&spawn.template);
        let template = match placement.and_then(|p| p.look.as_ref()) {
            Some(look) => Some(look.over(base)),
            None => base.cloned(),
        };
        let Some(template) = template else {
            warn!("npc {} has no template", spawn.template);
            continue;
        };
        // The level faces an NPC down its matrix's -Z; the character models face
        // +Z (players turn with from_rotation_y(atan2(dx, dz))), hence the half turn.
        // EVIDENCE: a0200_smith_armor's anvil (s01_deco_blacksmith_anvil_01) is 0.98
        //   units away along -Z of his _Instance_NPC matrix (dot 0.91); with +Z he
        //   stood with his back to it ("ils ont pas la bonne rotation").
        let rotation = placement
            .map(|p| Quat::from_mat4(&Mat4::from_cols_array(&p.m)).normalize() * Quat::from_rotation_y(std::f32::consts::PI))
            .unwrap_or_default();
        let transform = Transform::from_translation(spawn.position).with_rotation(rotation);
        let skeleton = template.graphics.strip_prefix("characters/").unwrap_or(&template.graphics).to_owned();
        let recipe = if skeleton == "uniskel" || skeleton == "uniskel_dwarf" {
            let outfit = data
                .outfits
                .get(skeleton.as_str())
                .and_then(|h| outfits.get(h))
                .and_then(|o| o.outfits.iter().find(|o| o.name == template.outfit));
            let Some(outfit) = outfit else {
                warn!("npc {}: no outfit {}", spawn.template, template.outfit);
                continue;
            };
            let variation = outfit
                .variation
                .as_ref()
                .and_then(|v| data.variations.get(skeleton.as_str()).and_then(|h| variations.get(h)).and_then(|all| all.variations.get(v)))
                .map(|shape| shape.iter().map(|j| (j.bone.clone(), Vec3::from(j.t), Vec3::from(j.s))).collect())
                .unwrap_or_default();
            NpcVisualRecipe::Character(CharacterDesc {
                look: Some(NpcLook { sound_profile: template.outfit.clone(), skeleton, parts: outfit.parts.clone(), anim_set: template.anim_set.clone(), variation }),
                ..default()
            })
        } else if std::env::var("DSOR_NPC_SKIP").is_ok_and(|v| template.graphics.contains(&v)) {
            continue;
        } else {
            NpcVisualRecipe::Model(crate::rendering::model_variant::path(&template.graphics, &template.outfit))
        };
        let entity = commands.spawn((
            Name::new(format!("npc {}", spawn.template)), transform, GlobalTransform::from(transform),
            Visibility::Hidden,
            Npc { actor: spawn.actor, template: spawn.template.clone(), profile: template.clone() },
            crate::ui::MapIcon(template.map_icon.clone()),
        )).id();
        if matches!(&recipe, NpcVisualRecipe::Character(_)) {
            let mut anim = CharacterAnim::default();
            if template.state != "Idle" { anim.state = AnimState::Named(template.state.clone()); }
            commands.entity(entity).insert(anim);
        }
        if online {
            commands.entity(entity).insert((OnlineNpcVisual { recipe, resident: false }, NpcVisualPending));
        } else {
            attach_npc_visual(&mut commands, &assets, entity, &recipe, &template, spawn.actor);
        }
        if crate::flag("nonpcs-shadow") {
            commands.entity(entity).insert(NpcNoShadow);
        }
    }
}

fn attach_npc_visual(
    commands: &mut Commands, assets: &AssetServer, entity: Entity,
    recipe: &NpcVisualRecipe, profile: &NpcTemplate, actor: u32,
) {
    match recipe {
        NpcVisualRecipe::Character(desc) => attach_character(commands, entity, desc.clone()),
        NpcVisualRecipe::Model(path) => {
            commands.entity(entity).insert(ModelNpc::new(assets, &profile.graphics, &profile.outfit, &profile.anim_set, &profile.state, 1.0));
            commands.spawn((
                CharacterVisual, WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path.clone()))),
                ChildOf(entity), Transform::default(), Visibility::default(),
            ));
        }
    }
    commands.entity(entity).insert((
        crate::actors::picking::PickModel(profile.graphics.clone()),
        crate::actors::character::AnimLod::new(3, actor % 3),
    ));
}

/// Keep the authoritative root, dialogue identity and map marker everywhere;
/// only nearby actors own rigs. Existing effects pin a resident rig until their
/// sequence retires. A new effect on an absent rig waits rather than loading it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn stream_online_npcs(
    mut commands: Commands,
    assets: Res<AssetServer>,
    current: Option<Res<CurrentMap>>,
    manifests: Res<Assets<crate::world::map::MapManifest>>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    players: Query<&GlobalTransform, crate::diagnostics::debug::FollowedPlayer>,
    mut npcs: Query<(Entity, &Transform, &Npc, &mut OnlineNpcVisual, Option<&mut CharacterAnim>)>,
    children: Query<&Children>,
    visuals: Query<(), With<CharacterVisual>>,
    sequences: Query<&crate::gameplay::skills::SequencePlayer>,
) {
    let Ok(camera) = cameras.single() else { return };
    let ground = current.as_ref().and_then(|c| manifests.get(&c.manifest)).map(|m| m.center[1]).unwrap_or(0.0);
    let focus = crate::world::map::camera_focus(camera, players.iter().next().map(|p| p.translation()), ground);
    let pinned: std::collections::HashSet<Entity> = sequences.iter().flat_map(|s| s.visual_actors()).collect();
    let legacy = crate::flag("legacyonlinenpcs");
    let mut pending = Vec::new();
    for (entity, at, _, mut visual, anim) in &mut npcs {
        // These are unparented network roots; Transform is current even on the
        // spawn frame before GlobalTransform propagation.
        let distance = at.translation.xz().distance(focus.xz());
        if visual.resident {
            if !legacy && distance > crate::world::map::UNLOAD_DISTANCE && !pinned.contains(&entity) {
                for child in children.get(entity).into_iter().flatten() {
                    if visuals.contains(*child) { commands.entity(*child).try_despawn(); }
                }
                commands.entity(entity)
                    .remove::<(Character, ModelNpc, crate::actors::character::ModelBones,
                        crate::rendering::model_variant::ResidentModel, crate::actors::character::AnimLod,
                        crate::actors::picking::PickModel, crate::actors::picking::PickBoxes)>()
                    .insert(NpcVisualPending);
                if let Some(mut anim) = anim { anim.release_rig(); }
                visual.resident = false;
            }
        } else if legacy || distance <= crate::world::map::LOAD_DISTANCE {
            pending.push((distance, entity));
        }
    }
    pending.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, entity) in pending.into_iter().take(4) {
        let Ok((_, _, npc, mut visual, _)) = npcs.get_mut(entity) else { continue };
        attach_npc_visual(&mut commands, &assets, entity, &visual.recipe, &npc.profile, npc.actor);
        visual.resident = true;
    }
}

#[allow(clippy::type_complexity)]
fn finish_online_visuals(
    mut commands: Commands,
    npcs: Query<(Entity, &OnlineNpcVisual, Option<&Character>, Option<&CharacterAnim>,
        Option<&crate::actors::character::ModelBones>), With<NpcVisualPending>>,
    children: Query<&Children>,
    first_poses: Query<(), With<crate::rendering::anim_cull::PendingFirstPose>>,
) {
    for (entity, visual, character, anim, bones) in &npcs {
        if visual.resident && (bones.is_some()
            || character.is_some_and(|c| c.outfit_ready())
                && anim.is_some_and(|a| if crate::flag("noanim") { a.is_ready() } else { a.is_animating() }))
        {
            if !children.iter_descendants(entity).any(|child| first_poses.contains(child)) {
                commands.entity(entity).remove::<NpcVisualPending>();
            }
        }
    }
}

/// Whole-model NPCs: play their idle once the model and its player are in.
#[allow(clippy::too_many_arguments)]
fn animate_models(
    mut commands: Commands,
    npcs: Query<(Entity, &ModelNpc)>,
    children: Query<&Children>,
    names: Query<&Name>,
    instances: Query<&bevy::world_serialization::WorldInstance>,
    spawner: Res<bevy::world_serialization::WorldInstanceSpawner>,
    players: Query<(), With<AnimationPlayer>>,
    gltfs: Res<Assets<Gltf>>,
    library: Option<Res<CharacterLibrary>>,
    tables: Res<Assets<AnimTable>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    (assets, sets, joints, skinned, time): (
        Res<AssetServer>,
        Res<Assets<crate::actors::monsters::ModelSets>>,
        Query<(&Transform, Option<&ChildOf>)>,
        Query<&bevy::mesh::skinning::SkinnedMesh>,
        Res<Time>,
    ),
) {
    for (entity, npc) in &npcs {
        let Some(gltf) = gltfs.get(&npc.gltf) else { continue };
        if !children.get(entity).into_iter().flatten().any(|e|
            instances.get(*e).is_ok_and(|i| spawner.instance_is_ready(**i))) { continue; }
        // Its body: the set's variation (a model without a sets file has none).
        let set = match &npc.sets {
            Some((handle, name)) => match sets.get(handle) {
                Some(file) => file.0.get(name),
                None if assets.load_state(handle).is_failed() => None,
                None => continue,
            },
            None => None,
        };
        let bones = children.iter_descendants(entity).filter_map(|e|
            names.get(e).ok().map(|n| (n.as_str().to_owned(), e))).collect();
        if let Some(set) = set {
            crate::actors::monsters::vary_model(&mut commands, &joints, &skinned, &children, entity, &bones, set);
        }
        commands.entity(entity).insert((crate::actors::character::ModelBones(bones),
            crate::rendering::model_variant::ResidentModel(npc.gltf.clone()))).remove::<ModelNpc>();
        let Some(player) = children.iter_descendants(entity).find(|e| players.contains(*e)) else { continue };
        let from_table = library
            .as_ref()
            .and_then(|l| tables.get(&l.anims))
            .and_then(|t| t.0.get(&npc.anim_set))
            .and_then(|set| set.get(&npc.state))
            .and_then(|clip| gltf.named_animations.get(clip.as_str()));
        // A sequence's model names its clip itself (spawn_sequence_model).
        let from_table = from_table.or_else(|| npc.anim_set.is_empty().then(|| gltf.named_animations.get(npc.state.as_str())).flatten());
        let clip = from_table.or_else(|| {
            let mut names: Vec<&str> = gltf.named_animations.keys().map(|k| k.as_ref()).collect();
            names.sort();
            names.iter().find(|n| n.contains("idle")).and_then(|n| gltf.named_animations.get(*n))
        });
        let Some(clip) = clip else {
            warn!("npc model {:?}: no idle among {} clips", npc.gltf.path(), gltf.named_animations.len());
            continue;
        };
        info!("npc model {:?}: playing {:?}", npc.gltf.path(), clip.path());
        let (graph, node) = AnimationGraph::from_clip(clip.clone());
        let sound_clips = gltf.named_animations.iter().find(|(_, h)| *h == clip)
            .map(|(name, _)| (node, name.to_string())).into_iter().collect();
        let mut anim = AnimationPlayer::default();
        // FAILURE: looped, an emote's thrown gremlin came back to the thrower's
        //   hands for the rest of the track ("il devrait pas reapparaitre a
        //   cote de moi").
        // FAILURE: started once its model had loaded, the gremlin ran behind
        //   the thrower's own animation.
        if npc.once {
            let late = npc.born.map_or(0.0, |born| (time.elapsed_secs() - born).max(0.0) * npc.speed);
            anim.play(node).set_speed(npc.speed).seek_to(late);
        } else { anim.play(node).repeat().set_speed(npc.speed); }
        commands.entity(player).insert((AnimationGraphHandle(graphs.add(graph)), anim,
            crate::audio::AnimationAudio::new(entity, &npc.gltf.path().unwrap().path().to_string_lossy(), "", sound_clips),
            crate::rendering::anim_cull::PendingFirstPose::default()));
    }
}

/// Other actors away from the camera's focus are hidden, as map cells are
/// (crate::world::map::cull_cells): not drawn, not in the shadow pass, no name.
#[allow(clippy::type_complexity)]
fn cull_actors(
    current: Option<Res<CurrentMap>>,
    manifests: Res<Assets<crate::world::map::MapManifest>>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    players: Query<&GlobalTransform, crate::diagnostics::debug::FollowedPlayer>,
    mut actors: Query<(&GlobalTransform, &mut Visibility, Option<&NpcVisualPending>), (Or<(With<Npc>, With<crate::network::net::RemotePlayer>)>, Without<crate::gameplay::skills::SequenceHidden>)>,
) {
    let Ok(cam) = cameras.single() else { return };
    let ground = current.as_ref().and_then(|c| manifests.get(&c.manifest)).map(|m| m.center[1]).unwrap_or(0.0);
    let focus = crate::world::map::camera_focus(cam, players.iter().next().map(|p| p.translation()), ground);
    for (at, mut v, pending) in &mut actors {
        let p = at.translation();
        let near = Vec2::new(p.x - focus.x, p.z - focus.z).length() <= crate::world::map::CULL_DISTANCE;
        let wanted = if near && pending.is_none() { Visibility::Inherited } else { Visibility::Hidden };
        if *v != wanted {
            *v = wanted;
        }
    }
}

/// Diagnostics (flag nonpcs-shadow): NPC surfaces cast no shadow.
#[derive(Component)]
struct NpcNoShadow;

fn npc_no_shadow(
    mut commands: Commands,
    npcs: Query<Entity, With<NpcNoShadow>>,
    parents: Query<&ChildOf>,
    meshes: Query<Entity, (Added<Mesh3d>, Without<bevy::light::NotShadowCaster>)>,
) {
    if npcs.is_empty() { return; }
    // Scenes and re-streamed rigs arrive asynchronously. Inspect each new mesh
    // once rather than traversing the entire crowd on every diagnostic frame.
    for mesh in &meshes {
        if parents.iter_ancestors(mesh).any(|root| npcs.contains(root)) {
            commands.entity(mesh).insert(bevy::light::NotShadowCaster);
        }
    }
}

/// Stream nearby decorative characters and looping sequences, using the same
/// prefetch/unload buffer as map geometry.
#[allow(clippy::too_many_arguments)]
fn spawn_deco(
    mut commands: Commands,
    current: Option<Res<CurrentMap>>,
    mut deco: ResMut<Deco>,
    files: Res<Assets<DecoFile>>,
    data: Res<NpcData>,
    outfits: Res<Assets<Outfits>>,
    variations: Res<Assets<Variations>>,
    assets: Res<AssetServer>,
    (skill_data, seqs): (Option<Res<crate::gameplay::skills::SkillData>>, Res<Assets<crate::gameplay::skills::SequenceTable>>),
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    players: Query<&GlobalTransform, crate::diagnostics::debug::FollowedPlayer>,
    manifests: Res<Assets<crate::world::map::MapManifest>>,
) {
    let Some(current) = current else { return };
    if deco.file.as_ref().is_none_or(|(m, _)| *m != current.name) {
        for e in std::mem::take(&mut deco.spawned).into_values() {
            commands.entity(e).try_despawn();
        }
        deco.file = Some((current.name.clone(), assets.load(format!("maps/{}.deco.json", current.name))));
        deco.pending = true;
    }
    if std::env::var("DSOR_NO_DECO").is_ok() {
        return;
    }
    let Some((_, handle)) = &deco.file else { return };
    let Some(file) = files.get(handle) else { return };
    let Ok(cam) = cameras.single() else { return };
    let ground = manifests.get(&current.manifest).map(|m| m.center[1]).unwrap_or(0.0);
    let focus = crate::world::map::camera_focus(cam, players.iter().next().map(|p| p.translation()), ground);
    let near = |m: &[f32; 16], radius: f32| Vec2::new(m[12], m[14]).distance(focus.xz()) <= radius;
    deco.spawned.retain(|&i, entity| {
        let m = if i < file.chars.len() { &file.chars[i].m } else { &file.seqs[i - file.chars.len()].m };
        if near(m, crate::world::map::UNLOAD_DISTANCE) { return true; }
        // SequencePlayer detects a missing anchor and disposes its effects.
        commands.entity(*entity).try_despawn();
        false
    });
    let ready = data.outfits.values().all(|h| outfits.get(h).is_some() || assets.load_state(h).is_failed())
        && data.variations.values().all(|h| variations.get(h).is_some() || assets.load_state(h).is_failed());
    let table = skill_data.as_ref().and_then(|d| seqs.get(d.sequences()));
    if table.is_none() && !skill_data.as_ref().is_some_and(|d| assets.load_state(d.sequences()).is_failed()) { return; }
    if !ready {
        return;
    }
    // Bound actor hierarchy creation when entering a populated area.
    let mut spawned = 0;
    deco.pending = false;
    for (i, c) in file.chars.iter().enumerate() {
        if deco.spawned.contains_key(&i) || !near(&c.m, crate::world::map::LOAD_DISTANCE) { continue; }
        if spawned >= 4 { deco.pending = true; break; }
        // As the NPCs: the level faces them down -Z, the models face +Z.
        let m = Mat4::from_cols_array(&c.m);
        let (scale, rotation, at) = m.to_scale_rotation_translation();
        let transform = Transform { translation: at, rotation: rotation * Quat::from_rotation_y(std::f32::consts::PI), scale };
        let skeleton = c.g.strip_prefix("characters/").unwrap_or(&c.g).to_owned();
        let entity = if skeleton == "uniskel" || skeleton == "uniskel_dwarf" {
            let outfit = data.outfits.get(skeleton.as_str()).and_then(|h| outfits.get(h)).and_then(|o| o.outfits.iter().find(|o| o.name == c.outfit));
            let Some(outfit) = outfit else {
                debug!("deco: no outfit {}", c.outfit);
                continue;
            };
            let variation = outfit
                .variation
                .as_ref()
                .and_then(|v| data.variations.get(skeleton.as_str()).and_then(|h| variations.get(h)).and_then(|all| all.variations.get(v)))
                .map(|shape| shape.iter().map(|j| (j.bone.clone(), Vec3::from(j.t), Vec3::from(j.s))).collect())
                .unwrap_or_default();
            let desc = CharacterDesc { look: Some(NpcLook { sound_profile: c.outfit.clone(), skeleton, parts: outfit.parts.clone(), anim_set: c.set.clone(), variation }), ..default() };
            let e = spawn_character(&mut commands, desc, transform);
            if c.speed != 1.0 {
                let mut anim = CharacterAnim::default();
                anim.speed = c.speed;
                commands.entity(e).insert(anim);
            }
            e
        } else {
            let path = crate::rendering::model_variant::path(&c.g, &c.outfit);
            let e = commands
                .spawn((
                    Name::new(format!("deco {}", c.outfit)),
                    transform,
                    Visibility::default(),
                    ModelNpc::new(&assets, &c.g, &c.outfit, &c.set, "Idle", c.speed),
                ))
                .id();
            commands.spawn((WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))), ChildOf(e), Transform::default(), Visibility::default()));
            e
        };
        // An actor id of its own, for the distance culling (cull_actors); no
        // picking: nobody answers.
        commands.entity(entity).insert((Npc { actor: DECO_ACTORS + i as u32, profile: NpcTemplate { no_talk: true, ..default() }, ..default() }, crate::actors::character::AnimLod::new(3, i as u32 % 3)));
        if crate::flag("nonpcs-shadow") {
            commands.entity(entity).insert(NpcNoShadow);
        }
        deco.spawned.insert(i, entity);
        spawned += 1;
    }
    for (i, s) in file.seqs.iter().enumerate() {
        let key = file.chars.len() + i;
        if deco.spawned.contains_key(&key) || !near(&s.m, crate::world::map::LOAD_DISTANCE) { continue; }
        let Some(seq) = table.and_then(|t| t.0.get(&s.seq)) else { continue };
        let anchor = commands.spawn((Transform::from_matrix(Mat4::from_cols_array(&s.m)), Visibility::default())).id();
        crate::gameplay::skills::play(&mut commands, seq.clone(), None, anchor, true);
        deco.spawned.insert(key, anchor);
    }
}

/// Actor ids of the decorations' characters (no server actor has them).
const DECO_ACTORS: u32 = 0x7800_0000;

#[cfg(test)]
pub(crate) mod placement_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn an_instance_look_keeps_its_templates_portrait_and_shop() {
        let template = NpcTemplate { graphics: "characters/uniskel".into(), outfit: "smith".into(), title: "Smith".into(),
            portrait: "npc_portraits/smith".into(), shop: "smith_shop".into(), map_icon: "ui/icon_map_smith".into(), ..default() };
        let look: NpcTemplate = serde_json::from_str(r#"{"graphics":"characters/uniskel","outfit":"smith_winter","anim_set":"npc","state":"Idle","loop":true,"title":""}"#).unwrap();
        let merged = look.over(Some(&template));
        assert_eq!((merged.outfit.as_str(), merged.title.as_str()), ("smith_winter", "Smith"));
        assert_eq!((merged.portrait.as_str(), merged.shop.as_str(), merged.map_icon.as_str()), ("npc_portraits/smith", "smith_shop", "ui/icon_map_smith"));
    }

    pub(crate) fn mark_resident_for_pose_test(world: &mut World, entity: Entity) {
        world.entity_mut(entity).insert((Npc::default(), NpcVisualPending,
            OnlineNpcVisual { recipe: NpcVisualRecipe::Character(CharacterDesc::default()), resident: true }));
    }

    pub(crate) fn finish_pose_test(world: &mut World) {
        world.run_system_once(finish_online_visuals).unwrap();
        world.run_system_once(cull_actors).unwrap();
    }

    fn stream_test() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Gltf>().init_asset::<bevy::world_serialization::WorldAsset>()
            .init_resource::<Assets<crate::world::map::MapManifest>>()
            .add_systems(Update, stream_online_npcs);
        let camera = Transform::from_xyz(0.0, 10.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z);
        app.world_mut().spawn((Camera3d::default(), GlobalTransform::from(camera)));
        app
    }

    fn test_actor(world: &mut World, x: f32, actor: u32) -> Entity {
        world.spawn((
            Npc { actor, template: "smith".into(), profile: NpcTemplate { graphics: "characters/uniskel".into(), title: "Smith".into(), ..default() } },
            Transform::from_xyz(x, 0.0, 0.0), Visibility::Hidden,
            crate::ui::MapIcon("smith_icon".into()), CharacterAnim::default(), NpcVisualPending,
            OnlineNpcVisual { recipe: NpcVisualRecipe::Character(CharacterDesc::default()), resident: false },
        )).id()
    }

    #[test]
    fn online_rigs_stream_with_budget_and_hysteresis_without_replacing_actor_roots() {
        let mut app = stream_test();
        let roots: Vec<_> = (0..5).map(|i| test_actor(app.world_mut(), i as f32, i)).collect();
        let far = test_actor(app.world_mut(), 95.0, 99);
        app.update();
        assert_eq!(roots.iter().filter(|e| app.world().get::<Character>(**e).is_some()).count(), 4);
        assert!(app.world().get::<Character>(far).is_none());
        app.update();
        assert!(roots.iter().all(|e| app.world().get::<Character>(*e).is_some()));

        let root = roots[0];
        let rig = app.world_mut().spawn((CharacterVisual, ChildOf(root))).id();
        let joint = app.world_mut().spawn(ChildOf(rig)).id();
        let attachment = app.world_mut().spawn(ChildOf(root)).id();
        app.world_mut().entity_mut(root).insert(crate::actors::picking::PickBoxes(vec![(joint, Transform::default(), Vec3::ONE)]));
        app.world_mut().get_mut::<CharacterAnim>(root).unwrap().state = AnimState::Named("Stunned".into());
        app.world_mut().get_mut::<Transform>(root).unwrap().translation.x = 80.0;
        app.update();
        assert!(app.world().get::<Character>(root).is_some(), "prefetch/unload buffer must retain the rig");
        app.world_mut().get_mut::<Transform>(root).unwrap().translation.x = 91.0;
        app.update();
        assert!(app.world().get::<Character>(root).is_none());
        assert!(app.world().get_entity(rig).is_err() && app.world().get_entity(joint).is_err());
        assert!(app.world().get_entity(attachment).is_ok(), "unrelated root attachments survive");
        assert!(app.world().get::<crate::actors::picking::PickBoxes>(root).is_none());
        assert!(app.world().get::<crate::actors::picking::PickModel>(root).is_none());
        assert_eq!(app.world().get::<Npc>(root).unwrap().actor, 0);
        assert_eq!(app.world().get::<Npc>(root).unwrap().profile.title, "Smith");
        assert_eq!(app.world().get::<crate::ui::MapIcon>(root).unwrap().0, "smith_icon");
        assert_eq!(app.world().get::<CharacterAnim>(root).unwrap().state, AnimState::Named("Stunned".into()));

        app.world_mut().get_mut::<Transform>(root).unwrap().translation.x = 80.0;
        app.update();
        assert!(app.world().get::<Character>(root).is_none(), "returning inside 90 alone must not reload");
        app.world_mut().get_mut::<Transform>(root).unwrap().translation.x = 69.0;
        app.update();
        assert!(app.world().get::<Character>(root).is_some());
        assert_eq!(app.world().get::<CharacterAnim>(root).unwrap().state, AnimState::Named("Stunned".into()));
        assert!(app.world().get::<crate::actors::picking::PickModel>(root).is_some());
        assert!(app.world().get::<crate::actors::picking::PickBoxes>(root).is_none(), "old joint bindings must not return");
    }

    #[test]
    fn sequences_pin_resident_rigs_but_do_not_load_absent_distant_actors() {
        let mut app = stream_test();
        let root = test_actor(app.world_mut(), 1.0, 1);
        let far = test_actor(app.world_mut(), 100.0, 2);
        app.update();
        let rig = app.world_mut().spawn((CharacterVisual, ChildOf(root))).id();
        let sequence = crate::gameplay::skills::Sequence { length: 100, repeat: true, tracks: Vec::new() };
        let player = app.world_mut().run_system_once(move |mut commands: Commands| {
            crate::gameplay::skills::play(&mut commands, sequence.clone(), Some(far), far, true);
            crate::gameplay::skills::play(&mut commands, sequence.clone(), Some(root), root, true)
        }).unwrap();
        app.world_mut().get_mut::<Transform>(root).unwrap().translation.x = 100.0;
        app.update();
        assert!(app.world().get_entity(rig).is_ok());
        assert!(app.world().get::<Character>(far).is_none(), "pending remote statuses must not defeat streaming");
        app.world_mut().despawn(player);
        app.update();
        assert!(app.world().get_entity(rig).is_err());
        assert!(app.world().get::<Npc>(root).is_some());
        // Authoritative discard/replacement still removes the entire actor tree.
        app.world_mut().despawn(root);
        assert!(app.world().get_entity(root).is_err());
    }

    #[test]
    fn npc_shadow_diagnostic_tags_late_and_restreamed_meshes_only() {
        let mut app = App::new();
        app.add_systems(Update, npc_no_shadow);
        let root = app.world_mut().spawn(NpcNoShadow).id();
        let scene = app.world_mut().spawn(ChildOf(root)).id();
        let unrelated = app.world_mut().spawn(Mesh3d::default()).id();
        app.update();
        let first = app.world_mut().spawn((Mesh3d::default(), ChildOf(scene))).id();
        app.update();
        assert!(app.world().get::<bevy::light::NotShadowCaster>(first).is_some());
        assert!(app.world().get::<bevy::light::NotShadowCaster>(unrelated).is_none());
        app.world_mut().despawn(scene);
        let replacement = app.world_mut().spawn((Mesh3d::default(), ChildOf(root))).id();
        app.update();
        assert!(app.world().get::<bevy::light::NotShadowCaster>(replacement).is_some());
    }

    #[test]
    fn offline_population_obeys_event_conditions_without_hiding_base_quest_actors() {
        let legacy = serde_json::json!({"id":"guard","m":[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.]});
        assert!(serde_json::from_value::<Placement>(legacy.clone()).unwrap().enabled_offline());
        for (rule, expected) in [
            (None, false),
            (Some("christmas=True;"), false),
            (Some("easter=False;"), true),
            (Some("prince_has_left=False;"), true),
            (Some("christmas=False;anniversary=True;"), false),
            (Some(" summer = TRUE ;"), false),
            (Some(""), true),
            (Some("malformed;=true;flag=unexpected;"), true),
        ] {
            let mut entry = legacy.clone();
            entry["event"] = 92.into();
            entry["event_flags"] = serde_json::json!(rule);
            let placement: Placement = serde_json::from_value(entry).unwrap();
            assert_eq!(placement.enabled_offline(), expected, "{rule:?}");
        }
    }
}
