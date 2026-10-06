//! NPCs: what NewNPCCommand (44) names, dressed and placed as the 2018 client does.
//!
//! The command carries the template Id, the level Guid, a position and a visible
//! bit (dsor_proto NewNpc). The rest comes from the client's data
//! (tools/export_npcs.py):
//! - `characters/npc_templates.json`: `_Template_NPC` Graphics, CharacterSet,
//!   AnimSet, StartAnimationState, and the French name (db.npc.xml `<Id>Title`);
//! - `maps/<map>.npcs.json`: `_Instance_NPC` by Guid, with the placement matrix
//!   (its rotation is the facing, which the command does not carry);
//! - `characters/<skeleton>/outfits.json` (DSO_Godot): a CharacterSet's parts.
//! Humans and dwarves (Graphics `characters/uniskel[_dwarf]`) are dressed on the
//! shared skeleton like players; other Graphics are whole models with their own
//! animations (barrel_dealer, gambler, troll...).
//! UNVERIFIED: outfits' `variation` (per-bone body shape) is not applied yet.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use serde::Deserialize;

use crate::character::{spawn_character, AnimState, AnimTable, CharacterAnim, CharacterDesc, CharacterLibrary, NpcLook};
use crate::map::CurrentMap;
use crate::nameplate::Nameplate;
use crate::net::Net;

#[derive(Deserialize, Debug, Clone)]
pub struct NpcTemplate {
    pub graphics: String,
    pub outfit: String,
    pub anim_set: String,
    pub state: String,
    #[serde(default)]
    pub title: String,
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

#[derive(Resource)]
struct NpcData {
    templates: Handle<NpcTemplates>,
    outfits: HashMap<&'static str, Handle<Outfits>>,
    variations: HashMap<&'static str, Handle<Variations>>,
    /// (map name, its placements).
    placements: Option<(String, Handle<NpcPlacements>)>,
}

/// Map viewer (`--npcs` / `?npcs`): every NPC the level places, as if the server
/// had sent each one. Online, the server decides who is there.
#[derive(Resource, Default)]
pub struct OfflineNpcs {
    pub queued: Vec<crate::net::NpcSpawn>,
    done_for: Option<String>,
}

fn offline_npcs(
    offline: Option<ResMut<OfflineNpcs>>,
    data: Res<NpcData>,
    placements: Res<Assets<NpcPlacements>>,
) {
    let Some(mut offline) = offline else { return };
    let Some((map, handle)) = &data.placements else { return };
    if offline.done_for.as_ref() == Some(map) {
        return;
    }
    let Some(placed) = placements.get(handle) else { return };
    offline.done_for = Some(map.clone());
    for (i, (guid, p)) in placed.0.iter().enumerate() {
        offline.queued.push(crate::net::NpcSpawn {
            actor: 0x7000_0000 + i as u32,
            template: p.id.clone(),
            guid: guid.clone(),
            position: Vec3::new(p.m[12], p.m[13], p.m[14]),
            visible: true,
        });
    }
    info!("{}: {} NPCs placed by the level", map, placed.0.len());
}

/// An NPC on the map.
#[derive(Component)]
pub struct Npc {
    pub actor: u32,
    pub template: String,
}

/// A whole-model NPC whose idle has not started yet.
#[derive(Component)]
struct ModelNpc {
    gltf: Handle<Gltf>,
    anim_set: String,
    state: String,
}

pub struct NpcPlugin;

impl Plugin for NpcPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<NpcTemplates>()
            .init_asset::<NpcPlacements>()
            .init_asset::<Outfits>()
            .init_asset::<Variations>()
            .register_asset_loader(VariationsLoader)
            .register_asset_loader(NpcTemplatesLoader)
            .register_asset_loader(NpcPlacementsLoader)
            .register_asset_loader(OutfitsLoader)
            .add_systems(Startup, load)
            .add_systems(Update, (follow_map, offline_npcs, spawn_npcs, animate_models).chain());
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
) {
    let (queue, gone) = match (net, offline) {
        (Some(net), _) => {
            let net = net.into_inner();
            (&mut net.npc_spawns, std::mem::take(&mut net.npc_gone))
        }
        (None, Some(offline)) => (&mut offline.into_inner().queued, Vec::new()),
        _ => return,
    };
    for actor in gone {
        for (e, n) in &existing {
            if n.actor == actor {
                commands.entity(e).despawn();
            }
        }
    }
    if queue.is_empty() {
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
    for spawn in std::mem::take(queue) {
        for (e, n) in &existing {
            if n.actor == spawn.actor {
                commands.entity(e).despawn();
            }
        }
        if !spawn.visible || std::env::var("DSOR_NPC_ONLY").is_ok_and(|v| !v.split(',').any(|t| t == spawn.template)) {
            continue;
        }
        let placement = placed.and_then(|p| p.0.get(&spawn.guid));
        let template = placement
            .and_then(|p| p.look.clone())
            .or_else(|| templates.0.get(&spawn.template).cloned());
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
        let entity = if skeleton == "uniskel" || skeleton == "uniskel_dwarf" {
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
            let desc = CharacterDesc {
                look: Some(NpcLook { skeleton, parts: outfit.parts.clone(), anim_set: template.anim_set.clone(), variation }),
                ..default()
            };
            let e = spawn_character(&mut commands, desc, transform);
            if template.state != "Idle" {
                let mut anim = CharacterAnim::default();
                anim.state = AnimState::Named(template.state.clone());
                commands.entity(e).insert(anim);
            }
            e
        } else if std::env::var("DSOR_NPC_SKIP").is_ok_and(|v| template.graphics.contains(&v)) {
            continue;
        } else {
            let path = format!("{}.glb", template.graphics);
            let gltf: Handle<Gltf> = assets.load(path.clone());
            let e = commands
                .spawn((
                    Name::new(format!("npc {}", spawn.template)),
                    transform,
                    Visibility::default(),
                    ModelNpc { gltf, anim_set: template.anim_set.clone(), state: template.state.clone() },
                ))
                .id();
            commands.spawn((
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
                ChildOf(e),
                Transform::default(),
                Visibility::default(),
            ));
            e
        };
        commands
            .entity(entity)
            .insert((Npc { actor: spawn.actor, template: spawn.template.clone() }, Nameplate::npc(template.title.clone())));
    }
}

/// Whole-model NPCs: play their idle once the model and its player are in.
#[allow(clippy::too_many_arguments)]
fn animate_models(
    mut commands: Commands,
    npcs: Query<(Entity, &ModelNpc)>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    gltfs: Res<Assets<Gltf>>,
    library: Option<Res<CharacterLibrary>>,
    tables: Res<Assets<AnimTable>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    for (entity, npc) in &npcs {
        let Some(gltf) = gltfs.get(&npc.gltf) else { continue };
        let Some(player) = children.iter_descendants(entity).find(|e| players.contains(*e)) else { continue };
        let from_table = library
            .as_ref()
            .and_then(|l| tables.get(&l.anims))
            .and_then(|t| t.0.get(&npc.anim_set))
            .and_then(|set| set.get(&npc.state))
            .and_then(|clip| gltf.named_animations.get(clip.as_str()));
        let clip = from_table.or_else(|| {
            let mut names: Vec<&str> = gltf.named_animations.keys().map(|k| k.as_ref()).collect();
            names.sort();
            names.iter().find(|n| n.contains("idle")).and_then(|n| gltf.named_animations.get(*n))
        });
        commands.entity(entity).remove::<ModelNpc>();
        let Some(clip) = clip else {
            warn!("npc model {:?}: no idle among {} clips", npc.gltf.path(), gltf.named_animations.len());
            continue;
        };
        info!("npc model {:?}: playing {:?}", npc.gltf.path(), clip.path());
        let (graph, node) = AnimationGraph::from_clip(clip.clone());
        let mut anim = AnimationPlayer::default();
        anim.play(node).repeat();
        commands.entity(player).insert((AnimationGraphHandle(graphs.add(graph)), anim));
    }
}
