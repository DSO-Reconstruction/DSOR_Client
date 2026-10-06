//! Player characters, drawn the way the 2018 client draws them.
//!
//! A character is one SHARED skeleton -- the `__animations.glb` of its skeleton
//! family (`uniskel` for humans, `uniskel_dwarf` for dwarves), which carries every
//! clip -- and the parts that dress it: the class/gender body parts and the Skin
//! parts of every worn item. Each part is exported with its own copy of the 69
//! joints; once a part is spawned its skin is re-pointed, joint by joint and BY
//! NAME, at the shared skeleton, so one AnimationPlayer drives the whole body.
//!
//! Animations follow the client's own table (data/tables/anims.xml, converted by
//! tools/character_tables.py into characters/anims.json): an animation set
//! ("warrior_1h_weapon", "mage_female_1h_weapon", ...) maps logical states (Idle,
//! Run, Skill01_execute, Hit, Death, Resurrect, ...) to clips.
//!
//! EVIDENCE: the set name rule is GamePlayer::GetAnimSet (sub_993B50) with
//! ArmanentStateToString (0xA2906E): "<class>_" + "female_" + armament suffix; the
//! body composition is the converted char_sel_* outfits (12 class/gender body parts
//! plus item skins).

use std::collections::HashMap;
use std::time::Duration;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::gltf::{Gltf, GltfAssetLabel};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use serde::Deserialize;

/// The blend between two animations.
const BLEND: Duration = Duration::from_millis(150);

const BODY_PARTS: [&str; 12] = [
    "ears", "feet", "hands", "head", "hips", "lowerarms", "lowerlegs", "midbody", "neck",
    "upperarms", "upperbody", "upperlegs",
];

pub const CLASSES: [&str; 4] = ["warrior", "mage", "ranger", "dwarf"];

/// animation set -> state -> clip.
#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct AnimTable(pub HashMap<String, HashMap<String, String>>);

/// The part names one skeleton family has.
#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct PartList(pub Vec<String>);

/// Item template -> its Skin parts (tools/character_tables.py, from _Template_Item).
#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct ItemSkins(pub HashMap<String, Vec<String>>);

#[derive(Default, TypePath)]
pub struct ItemSkinsLoader;
impl AssetLoader for ItemSkinsLoader {
    type Asset = ItemSkins;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<ItemSkins, Self::Error> {
        let mut b = Vec::new();
        r.read_to_end(&mut b).await?;
        serde_json::from_slice(&b).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] {
        &["item_skins.json"]
    }
}

#[derive(Default, TypePath)]
pub struct AnimTableLoader;
impl AssetLoader for AnimTableLoader {
    type Asset = AnimTable;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<AnimTable, Self::Error> {
        let mut b = Vec::new();
        r.read_to_end(&mut b).await?;
        serde_json::from_slice(&b).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] {
        &["anims.json"]
    }
}

#[derive(Default, TypePath)]
pub struct PartListLoader;
impl AssetLoader for PartListLoader {
    type Asset = PartList;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<PartList, Self::Error> {
        let mut b = Vec::new();
        r.read_to_end(&mut b).await?;
        serde_json::from_slice(&b).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] {
        &["parts.json"]
    }
}

/// What the server says a character looks like (NewPlayer / NewRemotePlayer).
#[derive(Clone, Debug, Default)]
pub struct CharacterDesc {
    /// 0 warrior, 1 mage, 2 ranger, 3 dwarf.
    pub class: u8,
    /// 0 male, 1 female.
    pub gender: u8,
    pub hair: u8,
    pub beard: u8,
    pub body: u8,
    pub variation: u8,
    /// (equipment slot, Skin part names) of every worn item.
    pub equipment: Vec<(u8, Vec<String>)>,
    /// ArmamentState: 0 empty, 1/2 one-hand small (+off hand), 3/4 one-hand large
    /// (+off hand), 5 two-hand.
    pub armament: i8,
    /// An NPC: its outfit replaces class, gender and equipment (crate::npc).
    pub look: Option<NpcLook>,
}

/// An NPC's fixed outfit: `_Template_NPC` Graphics, CharacterSet resolved through
/// the skeleton's outfits.json, and AnimSet.
#[derive(Clone, Debug, Default)]
pub struct NpcLook {
    /// "uniskel" or "uniskel_dwarf".
    pub skeleton: String,
    pub parts: Vec<String>,
    pub anim_set: String,
    /// The outfit's body shape (variations.json): (joint, translation, scale) for
    /// every joint, or empty. SEE: apply_variations.
    pub variation: Vec<(String, Vec3, Vec3)>,
}

/// A joint of a character with a body variation.
///
/// Nebula's joints do not pass their scale down: a joint's scale stretches its own
/// vertices and where its children sit (their translation is multiplied by it),
/// not the children themselves. Bevy's hierarchy would compound it, so the joint
/// itself is left unscaled and its vertices are skinned to a child, `scaled`,
/// carrying the scale. UNVERIFIED against the 2018 client: this is Nebula2's
/// nCharJoint rule; the variation translation is used as the joint's bind
/// translation (animation offsets from the bind pose are kept).
#[derive(Component)]
struct VariedJoint {
    var_t: Vec3,
    var_s: Vec3,
    bind_t: Vec3,
    parent: Option<Entity>,
    scaled: Entity,
    /// What the animation last wrote, and what this system then wrote.
    raw: (Vec3, Vec3),
    written: Option<(Vec3, Vec3)>,
}

impl CharacterDesc {
    fn class_name(&self) -> &'static str {
        CLASSES.get(self.class as usize).copied().unwrap_or("warrior")
    }
    fn gender_name(&self) -> &'static str {
        if self.gender == 1 { "female" } else { "male" }
    }
    fn skeleton(&self) -> &str {
        match &self.look {
            Some(look) => &look.skeleton,
            None if self.class == 3 => "uniskel_dwarf",
            None => "uniskel",
        }
    }

    /// The animation set name, GamePlayer::GetAnimSet's rule (an NPC's own AnimSet).
    pub fn animation_set(&self) -> String {
        if let Some(look) = &self.look {
            return look.anim_set.clone();
        }
        let class = self.class_name();
        let suffix = if class == "ranger" {
            if self.armament >= 3 { "large_weapon" } else { "small_weapon" }
        } else {
            match self.armament {
                2 | 4 => "1h_weapon_shield",
                5 => "2h_weapon",
                _ => "1h_weapon",
            }
        };
        let female = if self.gender == 1 { "female_" } else { "" };
        format!("{class}_{female}{suffix}")
    }

    /// Every part to dress, resolved against the parts that exist.
    fn parts(&self, exists: &dyn Fn(&str) -> bool) -> Vec<String> {
        if let Some(look) = &self.look {
            return look.parts.iter().filter(|p| exists(p)).cloned().collect();
        }
        let (class, gender) = (self.class_name(), self.gender_name());
        let mut out = Vec::new();
        for p in BODY_PARTS {
            for candidate in [format!("{class}_{gender}_{p}"), format!("{gender}_{p}")] {
                if exists(&candidate) {
                    out.push(candidate);
                    break;
                }
            }
        }
        for (_slot, skins) in &self.equipment {
            for skin in skins {
                let female = format!("{skin}_female");
                if self.gender == 1 && exists(&female) {
                    out.push(female);
                } else if exists(skin) {
                    out.push(skin.clone());
                } else {
                    debug!("no part named {skin}");
                }
            }
        }
        out
    }
}

/// Logical animation states, the rows of anims.xml.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AnimState {
    Idle,
    IdleHub,
    Run,
    Hit,
    Death,
    Resurrect,
    Stunned,
    /// Any other row, e.g. "Skill01_execute".
    Named(String),
}

impl AnimState {
    pub fn key(&self) -> &str {
        match self {
            AnimState::Idle => "Idle",
            AnimState::IdleHub => "Idle_Hub",
            AnimState::Run => "Run",
            AnimState::Hit => "Hit",
            AnimState::Death => "Death",
            AnimState::Resurrect => "Resurrect",
            AnimState::Stunned => "Stunned",
            AnimState::Named(s) => s,
        }
    }
}

/// Set `state` (and `speed`) to change what the character plays.
#[derive(Component, Debug)]
pub struct CharacterAnim {
    pub state: AnimState,
    pub speed: f32,
    applied: Option<(AnimState, f32)>,
    player: Option<Entity>,
    nodes: HashMap<String, (AnimationNodeIndex, bool)>,
}

impl Default for CharacterAnim {
    fn default() -> Self {
        Self { state: AnimState::Idle, speed: 1.0, applied: None, player: None, nodes: HashMap::new() }
    }
}

impl CharacterAnim {
    /// Whether the character's skeleton and animations are ready.
    pub fn is_ready(&self) -> bool {
        self.player.is_some()
    }
}

#[derive(Component)]
pub struct Character {
    pub desc: CharacterDesc,
    built: bool,
    bones: Option<HashMap<String, Entity>>,
    pending_parts: Vec<Entity>,
    /// Which dressing this is: bumped by `redress`, so the skeleton and parts of a
    /// previous dressing, ready only after it was replaced, are ignored.
    /// FAILURE (2026-10-06): the first skeleton reported ready after the inventory
    /// re-dressed the player; the new parts bound to its despawned bones and the body
    /// stayed behind as the player walked ("j'ai quitte le corps du perso").
    generation: u32,
}

impl Character {
    /// A joint of this character's shared skeleton, by name (once it is in).
    pub fn bone(&self, name: &str) -> Option<Entity> {
        self.bones.as_ref()?.get(name).copied()
    }
}

#[derive(Component)]
struct SkeletonOf(Entity, u32);
#[derive(Component)]
struct PartOf(Entity, u32);

#[derive(Resource)]
pub struct CharacterLibrary {
    pub anims: Handle<AnimTable>,
    pub item_skins: Handle<ItemSkins>,
    parts: HashMap<&'static str, Handle<PartList>>,
    skeletons: HashMap<&'static str, Handle<Gltf>>,
}

pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<AnimTable>()
            .init_asset::<PartList>()
            .init_asset::<ItemSkins>()
            .register_asset_loader(ItemSkinsLoader)
            .register_asset_loader(AnimTableLoader)
            .register_asset_loader(PartListLoader)
            .add_systems(Startup, load_library)
            .add_systems(Update, (build_characters, keep_parts_bound, drive_animations))
            .add_systems(
                PostUpdate,
                apply_variations.after(bevy::app::AnimationSystems).before(TransformSystems::Propagate),
            )
            .add_observer(on_instance_ready);
    }
}

fn load_library(mut commands: Commands, assets: Res<AssetServer>) {
    let mut parts = HashMap::new();
    let mut skeletons = HashMap::new();
    for skel in ["uniskel", "uniskel_dwarf"] {
        parts.insert(skel, assets.load(format!("characters/{skel}/parts.json")));
        skeletons.insert(skel, assets.load(format!("characters/{skel}/__animations.glb")));
    }
    commands.insert_resource(CharacterLibrary {
        anims: assets.load("characters/anims.json"),
        item_skins: assets.load("characters/item_skins.json"),
        parts,
        skeletons,
    });
}

/// Spawn a character; it dresses and animates itself once its assets are in.
pub fn spawn_character(commands: &mut Commands, desc: CharacterDesc, transform: Transform) -> Entity {
    commands
        .spawn((
            Name::new(format!("character {}", desc.animation_set())),
            Character { desc, built: false, bones: None, pending_parts: Vec::new(), generation: 0 },
            CharacterAnim::default(),
            transform,
            Visibility::default(),
        ))
        .id()
}

fn build_characters(
    mut commands: Commands,
    mut characters: Query<(Entity, &mut Character)>,
    library: Option<Res<CharacterLibrary>>,
    part_lists: Res<Assets<PartList>>,
    assets: Res<AssetServer>,
) {
    let Some(library) = library else { return };
    for (entity, mut character) in &mut characters {
        if character.built {
            continue;
        }
        let skel = character.desc.skeleton().to_owned();
        let skel = skel.as_str();
        let Some(list) = library.parts.get(skel).and_then(|h| part_lists.get(h)) else { continue };
        let names: std::collections::HashSet<&str> = list.0.iter().map(String::as_str).collect();
        let parts = character.desc.parts(&|n| names.contains(n));
        character.built = true;
        let skeleton_scene = assets.load(GltfAssetLabel::Scene(0).from_asset(format!("characters/{skel}/__animations.glb")));
        // CONTRACT: a Transform (and Visibility) on every child, or the hierarchy
        //   does not carry the character's movement down to the skeleton.
        // FAILURE (2026-10-06): without them the body stayed where the player had
        //   arrived while the player walked on ("je suis un fantome qui se deplace").
        commands.spawn((
            SkeletonOf(entity, character.generation),
            WorldAssetRoot(skeleton_scene),
            ChildOf(entity),
            Transform::default(),
            Visibility::default(),
        ));
        for part in parts {
            let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(format!("characters/{skel}/parts/{part}.glb")));
            commands.spawn((
                PartOf(entity, character.generation),
                WorldAssetRoot(scene),
                ChildOf(entity),
                Name::new(part),
                Transform::default(),
                Visibility::default(),
            ));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn on_instance_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    skeletons: Query<&SkeletonOf>,
    parts: Query<&PartOf>,
    children: Query<&Children>,
    names: Query<&Name>,
    players: Query<(), With<AnimationPlayer>>,
    skinned: Query<&SkinnedMesh>,
    mut characters: Query<(&mut Character, &mut CharacterAnim)>,
    library: Option<Res<CharacterLibrary>>,
    tables: Res<Assets<AnimTable>>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    joints: Query<(&Transform, Option<&ChildOf>)>,
    parents: Query<&ChildOf>,
) {
    let entity = ready.entity;
    if let Ok(SkeletonOf(owner, generation)) = skeletons.get(entity) {
        let (owner, generation) = (*owner, *generation);
        let mut bones = HashMap::new();
        let mut player = None;
        for e in children.iter_descendants(entity) {
            if let Ok(n) = names.get(e) {
                bones.insert(n.as_str().to_owned(), e);
            }
            if player.is_none() && players.contains(e) {
                player = Some(e);
            }
        }
        let Ok((mut character, mut anim)) = characters.get_mut(owner) else { return };
        if character.generation != generation {
            return;
        }
        // Animations: one graph per character, every state of its set.
        if let (Some(player), Some(library)) = (player, library.as_ref()) {
            let set = character.desc.animation_set();
            let gltf = library.skeletons.get(character.desc.skeleton()).and_then(|h| gltfs.get(h));
            let table = tables.get(&library.anims).and_then(|t| t.0.get(&set));
            if let (Some(gltf), Some(table)) = (gltf, table) {
                let mut graph = AnimationGraph::new();
                for (state, clip) in table {
                    if let Some(handle) = gltf.named_animations.get(clip.as_str()) {
                        let node = graph.add_clip(handle.clone(), 1.0, graph.root);
                        anim.nodes.insert(state.clone(), (node, clip.ends_with("-loop")));
                    }
                }
                commands
                    .entity(player)
                    .insert((AnimationGraphHandle(graphs.add(graph)), AnimationTransitions::new()));
                anim.player = Some(player);
                anim.applied = None;
            } else {
                warn!("no animation set {set} (or its skeleton is not loaded yet)");
            }
        }
        let variation = character.desc.look.as_ref().map(|l| l.variation.clone()).unwrap_or_default();
        if !variation.is_empty() {
            let shape: HashMap<&str, (Vec3, Vec3)> = variation.iter().map(|(n, t, s)| (n.as_str(), (*t, *s))).collect();
            let varied: HashMap<Entity, &str> =
                bones.iter().filter(|(n, _)| shape.contains_key(n.as_str())).map(|(n, e)| (*e, n.as_str())).collect();
            let mut scaled_bones = HashMap::new();
            for (&joint, &name) in &varied {
                let Ok((tf, parent)) = joints.get(joint) else { continue };
                let (var_t, var_s) = shape[name];
                let scaled = commands
                    .spawn((Name::new(format!("{name}#scaled")), VariedChild, Transform::from_scale(var_s), ChildOf(joint)))
                    .id();
                commands.entity(joint).insert(VariedJoint {
                    var_t,
                    var_s,
                    bind_t: tf.translation,
                    parent: parent.map(|p| p.parent()).filter(|p| varied.contains_key(p)),
                    scaled,
                    raw: (tf.translation, tf.scale),
                    written: None,
                });
                scaled_bones.insert(name.to_owned(), scaled);
            }
            bones.extend(scaled_bones);
        }
        // Every part of this dressing, pending or already bound to a skeleton
        // instance this one replaces.
        character.pending_parts.clear();
        for part in children.get(owner).into_iter().flatten() {
            if parts.get(*part).is_ok_and(|p| p.1 == generation) {
                rebind(&mut commands, *part, &bones, &children, &names, &skinned, &parents);
            }
        }
        character.bones = Some(bones);
        return;
    }
    if let Ok(PartOf(owner, generation)) = parts.get(entity) {
        let Ok((mut character, _)) = characters.get_mut(*owner) else { return };
        if character.generation != *generation {
            return;
        }
        match &character.bones {
            Some(bones) => rebind(&mut commands, entity, bones, &children, &names, &skinned, &parents),
            None => character.pending_parts.push(entity),
        }
    }
}

/// Point every skinned mesh of `part` at the shared skeleton, joint by joint by name.
/// SEE: keep_parts_bound, which does the same every frame.
fn rebind(
    commands: &mut Commands,
    part: Entity,
    bones: &HashMap<String, Entity>,
    children: &Query<&Children>,
    names: &Query<&Name>,
    skinned: &Query<&SkinnedMesh>,
    parents: &Query<&ChildOf>,
) {
    for e in children.iter_descendants(part) {
        let Ok(skin) = skinned.get(e) else { continue };
        let joint_names: Option<Vec<String>> =
            skin.joints.iter().map(|j| names.get(*j).ok().map(|n| n.as_str().to_owned())).collect();
        let Some(joint_names) = joint_names else { continue };
        bind(commands, e, skin, &joint_names, bones, parents);
        commands.entity(e).insert(JointNames(joint_names));
    }
}

fn bind(
    commands: &mut Commands,
    e: Entity,
    skin: &SkinnedMesh,
    joint_names: &[String],
    bones: &HashMap<String, Entity>,
    parents: &Query<&ChildOf>,
) {
    drop_own_skeleton(commands, e, skin, bones, parents);
    let joints: Vec<Entity> = joint_names
        .iter()
        .zip(skin.joints.iter())
        .map(|(n, own)| bones.get(n.as_str()).copied().unwrap_or(*own))
        .collect();
    // CONTRACT: a re-bound part's bounds are a box around the whole character, not
    //   the box computed for its own unanimated skeleton copy. The part's mesh sits
    //   at the character's origin, so a character-sized box in its space covers
    //   wherever the shared skeleton draws it.
    // FAILURE (2026-10-06): with NoFrustumCulling every part of every NPC was drawn
    //   off screen too -- ~11 skinned meshes per NPC, 20 FPS in the browser.
    commands.entity(e).insert((
        SkinnedMesh { inverse_bindposes: skin.inverse_bindposes.clone(), joints },
        bevy::camera::primitives::Aabb::from_min_max(CHARACTER_BOUNDS.0, CHARACTER_BOUNDS.1),
    ));
}

/// A box around any posed character (weapons and capes included), in its own space.
const CHARACTER_BOUNDS: (Vec3, Vec3) = (Vec3::new(-2.0, -0.5, -2.0), Vec3::new(2.0, 3.5, 2.0));

/// Characters farther than this from the camera stop animating (their pose is
/// kept); the game camera sees about 40 units around the player.
const ANIMATION_DISTANCE: f32 = 60.0;

/// Despawn the part's own copy of the skeleton once its skin points at the shared
/// one: ~70 unused joints per part, ~800 entities per dressed character, which
/// the browser paid for every frame.
/// CONTRACT: only joint trees that do not hold the skinned mesh itself.
fn drop_own_skeleton(
    commands: &mut Commands,
    mesh: Entity,
    skin: &SkinnedMesh,
    bones: &HashMap<String, Entity>,
    parents: &Query<&ChildOf>,
) {
    let shared: std::collections::HashSet<Entity> = bones.values().copied().collect();
    let own: std::collections::HashSet<Entity> = skin.joints.iter().copied().filter(|j| !shared.contains(j)).collect();
    let mesh_ancestors: std::collections::HashSet<Entity> = parents.iter_ancestors(mesh).collect();
    for &j in &own {
        let top = parents.get(j).map(|p| p.parent()).ok();
        if top.is_some_and(|p| own.contains(&p)) || mesh_ancestors.contains(&j) || j == mesh {
            continue;
        }
        commands.entity(j).try_despawn();
    }
}

/// The joint names of a part's skin, as its own skeleton copy named them.
#[derive(Component)]
struct JointNames(Vec<String>);

/// A skinned mesh that appears under a character after its skeleton is ready is
/// bound to that skeleton.
/// CONTRACT: a glTF scene can be instanced again (its textures finishing loading
///   re-spawns it), which recreates the part's entities with their own joints; a
///   one-time binding is then lost and the body stops following the character.
/// FAILURE (2026-10-06): the player walked away from their own body.
/// FAILURE (2026-10-06): this ran over every part of every character each frame,
///   copying 69 joint names per part: 5.5 ms a frame with Kingshill's NPCs
///   (bevy trace_chrome). It now only looks at skinned meshes just added.
fn keep_parts_bound(
    mut commands: Commands,
    added: Query<(Entity, &SkinnedMesh, Option<&JointNames>), Added<SkinnedMesh>>,
    characters: Query<&Character>,
    parents: Query<&ChildOf>,
    names: Query<&Name>,
) {
    for (e, skin, known) in &added {
        let Some(owner) = parents.iter_ancestors(e).find(|a| characters.contains(*a)) else { continue };
        let Ok(character) = characters.get(owner) else { continue };
        // Not ready yet: on_instance_ready binds the parts once the skeleton is.
        let Some(bones) = &character.bones else { continue };
        let shared: std::collections::HashSet<Entity> = bones.values().copied().collect();
        if skin.joints.iter().all(|j| shared.contains(j)) {
            continue; // our own binding, re-inserted
        }
        let joint_names: Vec<String> = match known {
            Some(JointNames(n)) => n.clone(),
            None => {
                let Some(n) = skin
                    .joints
                    .iter()
                    .map(|j| names.get(*j).ok().map(|n| n.as_str().to_owned()))
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                commands.entity(e).insert(JointNames(n.clone()));
                n
            }
        };
        bind(&mut commands, e, skin, &joint_names, bones, &parents);
    }
}

/// After the animation pose, before transforms propagate: every varied joint
/// unscaled, its translation stretched by its parent's scale, its scale on its
/// `scaled` child. SEE: VariedJoint.
fn apply_variations(
    mut joints: Query<(Entity, &mut Transform, &mut VariedJoint), Without<VariedChild>>,
    mut scaled: Query<&mut Transform, With<VariedChild>>,
) {
    let mut scale: HashMap<Entity, Vec3> = HashMap::new();
    for (e, tf, mut j) in &mut joints {
        // A joint the animation did not write this frame still holds our output.
        if j.written != Some((tf.translation, tf.scale)) {
            j.raw = (tf.translation, tf.scale);
        }
        scale.insert(e, j.var_s * j.raw.1);
    }
    for (e, mut tf, mut j) in &mut joints {
        let parent_scale = j.parent.and_then(|p| scale.get(&p)).copied().unwrap_or(Vec3::ONE);
        let t = (j.raw.0 + j.var_t - j.bind_t) * parent_scale;
        tf.translation = t;
        tf.scale = Vec3::ONE;
        j.written = Some((t, Vec3::ONE));
        if let Ok(mut s) = scaled.get_mut(j.scaled) {
            s.scale = scale[&e];
        }
    }
}

/// The scaled child of a varied joint.
#[derive(Component)]
struct VariedChild;

fn drive_animations(
    mut characters: Query<(&mut CharacterAnim, &GlobalTransform, &InheritedVisibility)>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
) {
    let eye = cameras.iter().next().map(|c| c.translation());
    for (mut anim, at, shown) in &mut characters {
        let Some(player) = anim.player else { continue };
        // Hidden (culled) or far characters: stopped, so their joints are not
        // evaluated at all -- a paused animation is still applied every frame
        // (7 ms a frame for Kingshill's NPCs, bevy trace_chrome). Played again
        // from their state once back in view.
        let far = !shown.get() || eye.is_some_and(|e| e.distance(at.translation()) > ANIMATION_DISTANCE);
        if far {
            if anim.applied.is_some() {
                if let Ok((mut p, _)) = players.get_mut(player) {
                    p.stop_all();
                }
                anim.applied = None;
            }
            continue;
        }
        let wanted = (anim.state.clone(), anim.speed);
        if anim.applied.as_ref() == Some(&wanted) {
            continue;
        }
        let Ok((mut p, mut transitions)) = players.get_mut(player) else { continue };
        let key = anim.state.key().to_owned();
        let Some(&(node, looping)) = anim.nodes.get(&key).or_else(|| anim.nodes.get("Idle")) else {
            continue;
        };
        let active = transitions.play(&mut p, node, BLEND);
        active.set_speed(anim.speed);
        if looping {
            active.repeat();
        } else {
            active.replay();
        }
        anim.applied = Some(wanted);
    }
}

/// Dress a character again with new equipment: its skeleton and parts are rebuilt,
/// its animation state kept.
pub fn redress(
    commands: &mut Commands,
    entity: Entity,
    character: &mut Character,
    anim: &mut CharacterAnim,
    children: Option<&Children>,
    equipment: Vec<(u8, Vec<String>)>,
    armament: i8,
) {
    if let Some(children) = children {
        for c in children.iter() {
            commands.entity(c).despawn();
        }
    }
    character.desc.equipment = equipment;
    character.desc.armament = armament;
    character.built = false;
    character.generation = character.generation.wrapping_add(1);
    character.bones = None;
    character.pending_parts.clear();
    anim.player = None;
    anim.nodes.clear();
    anim.applied = None;
}
