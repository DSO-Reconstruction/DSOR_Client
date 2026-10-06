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
        // The class's base outfit fills what no worn item covers: a character with
        // no chest piece wears <class>_torso_00, not its underwear.
        // EVIDENCE: _Template_PlayerCharacter.CharacterSet (mage_male_body_00 ...)
        //   lists the body parts plus mage_torso_00 and mage_boots_00 (rangers also
        //   ranger_gloves_00) -- characters/uniskel/outfits.json.
        // FAILURE (2026-10-06): "mon perso est en slibard".
        for kind in ["torso", "boots", "gloves"] {
            let covered = self.equipment.iter().flat_map(|(_, skins)| skins).any(|s| s.contains(kind));
            if covered {
                continue;
            }
            let base = format!("{class}_{kind}_00");
            let female = format!("{base}_female");
            if self.gender == 1 && exists(&female) {
                out.push(female);
            } else if exists(&base) {
                out.push(base);
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
    /// The animation playing at full weight, and those fading out.
    current: Option<AnimationNodeIndex>,
    fading: Vec<(AnimationNodeIndex, f32)>,
    nodes: HashMap<String, (AnimationNodeIndex, bool)>,
}

impl Default for CharacterAnim {
    fn default() -> Self {
        Self { state: AnimState::Idle, speed: 1.0, applied: None, player: None, nodes: HashMap::new(), current: None, fading: Vec::new() }
    }
}

impl CharacterAnim {
    /// Play the current state again from its start, even if it is already playing
    /// (a skill used twice in a row).
    pub fn replay(&mut self) {
        self.applied = None;
    }

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
            .add_systems(Update, (build_characters, revive_skeletons, keep_parts_bound, drive_animations, check_skins).chain())
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

/// The queries a skeleton is adopted with (on_instance_ready, revive_skeletons).
#[derive(bevy::ecs::system::SystemParam)]
struct SkeletonCtx<'w, 's> {
    commands: Commands<'w, 's>,
    parts: Query<'w, 's, &'static PartOf>,
    children: Query<'w, 's, &'static Children>,
    names: Query<'w, 's, &'static Name>,
    players: Query<'w, 's, (), With<AnimationPlayer>>,
    skinned: Query<'w, 's, (&'static SkinnedMesh, Option<&'static JointNames>)>,
    characters: Query<'w, 's, (&'static mut Character, &'static mut CharacterAnim)>,
    library: Option<Res<'w, CharacterLibrary>>,
    tables: Res<'w, Assets<AnimTable>>,
    gltfs: Res<'w, Assets<Gltf>>,
    graphs: ResMut<'w, Assets<AnimationGraph>>,
    joints: Query<'w, 's, (&'static Transform, Option<&'static ChildOf>)>,
    parents: Query<'w, 's, &'static ChildOf>,
}

/// Take the skeleton instance `entity` as its character's: bones, animation
/// player and graph, body variation, and every part of the dressing bound to it.
fn adopt_skeleton(ctx: &mut SkeletonCtx, entity: Entity, owner: Entity, generation: u32) {
    let commands = &mut ctx.commands;
    let mut bones = HashMap::new();
    let mut player = None;
    for e in ctx.children.iter_descendants(entity) {
        if let Ok(n) = ctx.names.get(e) {
            bones.insert(n.as_str().to_owned(), e);
        }
        if player.is_none() && ctx.players.contains(e) {
            player = Some(e);
        }
    }
    let Ok((mut character, mut anim)) = ctx.characters.get_mut(owner) else { return };
    if character.generation != generation {
        return;
    }
    // Animations: one graph per character, every state of its set.
    if let (Some(player), Some(library)) = (player, ctx.library.as_ref()) {
        let set = character.desc.animation_set();
        let gltf = library.skeletons.get(character.desc.skeleton()).and_then(|h| ctx.gltfs.get(h));
        let table = ctx.tables.get(&library.anims).and_then(|t| t.0.get(&set));
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
                .insert(AnimationGraphHandle(ctx.graphs.add(graph)));
            anim.player = Some(player);
            anim.current = None;
            anim.fading.clear();
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
            let Ok((tf, parent)) = ctx.joints.get(joint) else { continue };
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
    if std::env::var("DSOR_CHECK_SKIN").is_ok() {
        let n = ctx.children.get(owner).map(|c| c.iter().filter(|p| ctx.parts.get(*p).is_ok_and(|p| p.1 == generation)).count()).unwrap_or(0);
        info!("adopt skeleton {entity:?} for {owner:?} gen {generation}: {} bones, {n} parts", bones.len());
    }
    for part in ctx.children.get(owner).into_iter().flatten() {
        if ctx.parts.get(*part).is_ok_and(|p| p.1 == generation) {
            rebind(commands, *part, &bones, &ctx.children, &ctx.names, &ctx.skinned, &ctx.parents);
        }
    }
    character.bones = Some(bones);
}

#[allow(clippy::too_many_arguments)]
fn on_instance_ready(ready: On<WorldInstanceReady>, skeletons: Query<&SkeletonOf>, mut ctx: SkeletonCtx) {
    let entity = ready.entity;
    if let Ok(SkeletonOf(owner, generation)) = skeletons.get(entity) {
        adopt_skeleton(&mut ctx, entity, *owner, *generation);
        return;
    }
    if let Ok(PartOf(owner, generation)) = ctx.parts.get(entity) {
        let (owner, generation) = (*owner, *generation);
        let Ok((mut character, _)) = ctx.characters.get_mut(owner) else { return };
        if character.generation != generation {
            return;
        }
        match &character.bones {
            Some(bones) => {
                let bones = bones.clone();
                rebind(&mut ctx.commands, entity, &bones, &ctx.children, &ctx.names, &ctx.skinned, &ctx.parents)
            }
            None => character.pending_parts.push(entity),
        }
    }
}

/// A skeleton whose instance was replaced is adopted again.
/// CONTRACT: the skeleton scene is instanced again when its assets finish loading
///   (and WorldInstanceReady does not fire again): its old bones and animation
///   player are despawned, and parts bound to them stayed where they were.
/// FAILURE (2026-10-06): "si ca n'a pas fini de charger et que je me deplace tout
///   reste a sa place"; after quick casts "l'anim ne marche plus" (the player was
///   gone). Reproduced with DSOR_CHECK_SKIN: parts bound to a dead joint.
fn revive_skeletons(
    alive: Query<()>,
    skeletons: Query<(Entity, &SkeletonOf, &Children)>,
    mut ctx: SkeletonCtx,
) {
    let mut stale = Vec::new();
    if std::env::var("DSOR_CHECK_SKIN").is_ok() {
        for (owner, (c, a)) in ctx.characters.iter().enumerate() {
            let dead = c.bones.as_ref().map(|b| b.values().filter(|b| !alive.contains(**b)).count());
            let _ = (owner, a);
            if dead.is_some_and(|d| d > 0) {
                let sks: Vec<_> = skeletons.iter().map(|(e, s, k)| (e, s.0, s.1, k.len(), c.generation)).collect();
                info!("revive check: {dead:?} dead bones; skeletons (entity, owner, gen, kids, char gen): {sks:?}");
            }
        }
    }
    for (sk, SkeletonOf(owner, generation), kids) in &skeletons {
        let Ok((character, anim)) = ctx.characters.get(*owner) else { continue };
        if character.generation != *generation || kids.is_empty() {
            continue;
        }
        let bone_dead = character.bones.as_ref().is_some_and(|b| b.values().any(|b| !alive.contains(*b)));
        let player_dead = anim.player.is_some_and(|p| !alive.contains(p));
        if bone_dead || player_dead {
            stale.push((sk, *owner, *generation));
        }
    }
    for (sk, owner, generation) in stale {
        info!("character {owner:?}: skeleton instance replaced, adopting it again");
        adopt_skeleton(&mut ctx, sk, owner, generation);
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
    skinned: &Query<(&SkinnedMesh, Option<&JointNames>)>,
    parents: &Query<&ChildOf>,
) {
    for e in children.iter_descendants(part) {
        let Ok((skin, known)) = skinned.get(e) else { continue };
        // CONTRACT: the names remembered at the first binding. A part already bound
        //   to a skeleton instance that has since been replaced points at dead
        //   joints, whose names can no longer be read.
        // FAILURE (2026-10-06): such parts were skipped and stayed bound to the dead
        //   skeleton -- the body froze in place while the player moved ("si ca n'a
        //   pas fini de charger et que je me deplace tout reste a sa place").
        let joint_names: Option<Vec<String>> = match known {
            Some(JointNames(n)) => Some(n.clone()),
            None => skin.joints.iter().map(|j| names.get(*j).ok().map(|n| n.as_str().to_owned())).collect(),
        };
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
    if std::env::var("DSOR_CHECK_SKIN").is_ok() {
        info!("bind skin {e:?}: first joint {:?} -> {:?}", skin.joints.first(), joint_names.first().and_then(|n| bones.get(n.as_str())));
    }
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
        if std::env::var("DSOR_CHECK_SKIN").is_ok() {
            let under_shared = parents.iter_ancestors(j).any(|a| shared.contains(&a));
            info!("drop own skeleton of skin {mesh:?}: despawn joint tree {j:?} (under shared bone: {under_shared}, {} own joints, {} shared)", own.len(), shared.len());
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
    time: Res<Time>,
    mut characters: Query<(&mut CharacterAnim, &GlobalTransform, &InheritedVisibility)>,
    mut players: Query<&mut AnimationPlayer>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
) {
    let eye = cameras.iter().next().map(|c| c.translation());
    let fade_step = time.delta_secs() / BLEND.as_secs_f32();
    for (mut anim, at, shown) in &mut characters {
        let Some(player) = anim.player else { continue };
        let Ok(mut p) = players.get_mut(player) else { continue };
        // Hidden (culled) or far characters: stopped, so their joints are not
        // evaluated at all -- a paused animation is still applied every frame
        // (7 ms a frame for Kingshill's NPCs, bevy trace_chrome). Played again
        // from their state once back in view.
        let far = !shown.get() || eye.is_some_and(|e| e.distance(at.translation()) > ANIMATION_DISTANCE);
        if far {
            if anim.applied.is_some() {
                p.stop_all();
                anim.current = None;
                anim.fading.clear();
                anim.applied = None;
            }
            continue;
        }
        // Our own cross-fade. bevy's AnimationTransitions stopped an animation
        // started again while its previous fade-out was still running: after two
        // quick casts the character played nothing at all ("je le lance 2x l'anim
        // marche apres ca marche plus").
        let current = anim.current;
        anim.fading.retain_mut(|(node, w)| {
            if Some(*node) == current {
                return false;
            }
            *w -= fade_step;
            if *w <= 0.0 {
                p.stop(*node);
                return false;
            }
            if let Some(a) = p.animation_mut(*node) {
                a.set_weight(*w);
            }
            true
        });
        let wanted = (anim.state.clone(), anim.speed);
        if anim.applied.as_ref() == Some(&wanted) {
            continue;
        }
        let key = anim.state.key().to_owned();
        let Some(&(node, looping)) = anim.nodes.get(&key).or_else(|| anim.nodes.get("Idle")) else {
            continue;
        };
        if let Some(old) = anim.current.filter(|old| *old != node) {
            anim.fading.push((old, 1.0));
        }
        anim.fading.retain(|(n, _)| *n != node);
        let active = p.play(node);
        active.set_weight(1.0).set_speed(anim.speed);
        if looping {
            active.repeat();
        } else {
            active.replay();
        }
        anim.current = Some(node);
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
    anim.current = None;
    anim.fading.clear();
}

/// DSOR_CHECK_SKIN=1: every frame, every skinned mesh under a character must point
/// at living joints of that same character; anything else is logged (a body left
/// behind is a mesh skinned to joints that no longer move with it).
fn check_skins(
    characters: Query<Entity, With<Character>>,
    children: Query<&Children>,
    skins: Query<&SkinnedMesh>,
    names: Query<&Name>,
    exists: Query<()>,
    mut reported: Local<std::collections::HashSet<(Entity, Entity)>>,
) {
    if std::env::var("DSOR_CHECK_SKIN").is_err() {
        return;
    }
    for root in &characters {
        let mine: std::collections::HashSet<Entity> = children.iter_descendants(root).collect();
        for e in children.iter_descendants(root) {
            let Ok(skin) = skins.get(e) else { continue };
            for &j in &skin.joints {
                let problem = if !exists.contains(j) {
                    "dead"
                } else if !mine.contains(&j) {
                    "foreign"
                } else {
                    continue;
                };
                if reported.insert((e, j)) {
                    warn!(
                        "skin {:?} ({}) of character {root:?}: {problem} joint {j:?} ({})",
                        e,
                        names.get(e).map(|n| n.as_str()).unwrap_or("?"),
                        names.get(j).map(|n| n.as_str()).unwrap_or("?")
                    );
                }
            }
        }
    }
}
