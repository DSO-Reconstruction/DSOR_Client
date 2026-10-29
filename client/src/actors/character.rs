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

use bevy::asset::{io::Reader, AssetLoader, LoadContext, VisitAssetDependencies};
use bevy::gltf::{Gltf, GltfAssetLabel};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::animation::AnimatedBy;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use serde::Deserialize;
use crate::actors::appearance::Equipment;

/// The blend between two animations.
const BLEND: Duration = Duration::from_millis(150);
const EQUIPMENT_BLEND: Duration = Duration::from_millis(220);
/// No decoded combat-mode flag: retain the stance briefly after a skill or blow.
const COMBAT_IDLE_SECONDS: f32 = 5.0;

#[cfg(test)]
mod idle_tests;
pub(crate) mod anim_preview;

const BODY_PARTS: [&str; 12] = [
    "ears", "feet", "hands", "head", "hips", "lowerarms", "lowerlegs", "midbody", "neck",
    "upperarms", "upperbody", "upperlegs",
];

pub const CLASSES: [&str; 4] = ["warrior", "mage", "ranger", "dwarf"];

/// animation set -> state -> clip.
#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct AnimTable(pub HashMap<String, HashMap<String, String>>);

/// Available meshes and authored region masks/layers from gfxskintags.bin.
#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(from = "PartCatalog")]
pub struct PartList {
    parts: Vec<String>,
    tags: HashMap<String, SkinTag>,
}

#[derive(Clone, Copy, Deserialize, Debug)]
struct SkinTag { mask: u32, layer: u16 }

#[derive(Deserialize)]
#[serde(untagged)]
enum PartCatalog {
    Current { parts: Vec<String>, tags: HashMap<String, SkinTag> },
    Legacy(Vec<String>),
}

impl From<PartCatalog> for PartList {
    fn from(catalog: PartCatalog) -> Self {
        match catalog {
            PartCatalog::Current { parts, tags } => Self { parts, tags },
            PartCatalog::Legacy(parts) => Self { parts, tags: default() },
        }
    }
}

/// Only visible higher layers occlude lower ones. Parts at the same layer may
/// share regions (a robe and boots); unknown meshes are preserved conservatively.
fn cull_covered_parts(parts: &mut Vec<(String, Option<[f32; 4]>)>, tags: &HashMap<String, SkinTag>) {
    let mut layers: Vec<_> = parts.iter().filter_map(|(name, _)| tags.get(name).map(|tag| tag.layer)).collect();
    layers.sort_unstable();
    layers.dedup();
    let mut covered = 0;
    for layer in layers.into_iter().rev() {
        let mut this_layer = 0;
        parts.retain(|(name, _)| {
            let Some(tag) = tags.get(name).filter(|tag| tag.layer == layer) else { return true };
            if tag.mask & covered != 0 { return false; }
            this_layer |= tag.mask;
            true
        });
        covered |= this_layer;
    }
}

/// Item template -> its Skin parts (tools/character_tables.py, from _Template_Item).
#[derive(Asset, TypePath, Deserialize, Debug, Default)]
#[serde(from = "SkinTable")]
pub struct ItemSkins {
    pub items: HashMap<String, Vec<String>>,
    pub appearances: HashMap<String, Vec<String>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SkinTable {
    Current { items: HashMap<String, Vec<String>>, appearances: HashMap<String, Vec<String>> },
    Legacy(HashMap<String, Vec<String>>),
}

impl From<SkinTable> for ItemSkins {
    fn from(table: SkinTable) -> Self {
        match table {
            SkinTable::Current { items, appearances } => Self { items, appearances },
            SkinTable::Legacy(items) => Self { items, appearances: default() },
        }
    }
}

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
    /// Visible worn slots, their Skin parts and optional RGBA dye.
    pub equipment: Vec<Equipment>,
    /// ArmamentState: 0 empty, 1/2 one-hand small (+off hand), 3/4 one-hand large
    /// (+off hand), 5 two-hand.
    pub armament: i8,
    /// Hair/beard resolved from the roster's customization indices.
    pub cosmetics: Vec<String>,
    /// The body style, its ethnicity (ETHNICITIES): which of the class and
    /// gender's body sets it wears; the body stature, its proportions
    /// (var_<class>_<gender>_<stature, 2 digits>).
    pub body: u8,
    pub stature: u8,
    /// A player of the world's hair and beard styles (NewPlayer's wire indices),
    /// made `cosmetics` when it is built (characters/selection.json's tables).
    pub styles: Option<(u8, u8)>,
    /// An NPC: its outfit replaces class, gender and equipment (crate::actors::npc).
    pub look: Option<NpcLook>,
    /// A transformation a status effect puts on a player (crate::actors::mounts:
    /// StatusDef::transform): its skin, body shape and anim set are worn in place
    /// of the player's own while it stays a player otherwise.
    /// EVIDENCE (data): skill_irondwarf_transform's StartModifiers
    ///   SkinModifier:irondwarf, VariationModifier:var_irondwarf,
    ///   AnimSet:dwarf_transformed.
    pub shape: Option<NpcLook>,
}

/// A body style's body set: <class>_<gender>[_<ethnicity>]_body_00, and its parts
/// <class>_<gender>[_<ethnicity>]_<part> (characters/<skeleton>/outfits.json).
/// The proportions are the class and gender's whatever the ethnicity: the
/// outfits' var_*_asian_00 / var_*_black_00 are never chosen.
/// SOURCE: the 2026 client's UpdateCharacterAppearance 0x140240270 / 0x14023b380
///   and its NewPlayer handling 0x14020f828: "%s_%sbody_00" with "" / "asian_" /
///   "black_" for body style 0 (or any other) / 1 / 2, "var_%s_%02d" with the
///   stature (data/diagnostics/charselection-widget.annotated.txt).
pub const ETHNICITIES: [&str; 3] = ["", "asian", "black"];

/// An NPC's fixed outfit: `_Template_NPC` Graphics, CharacterSet resolved through
/// the skeleton's outfits.json, and AnimSet.
#[derive(Clone, Debug, Default)]
pub struct NpcLook {
    /// Authored CharacterSet, also the skin key in audio/hotspots.xml.
    pub sound_profile: String,
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
    /// var_s x the animation's scale: what its children's translations stretch by.
    scale: Vec3,
}

impl CharacterDesc {
    pub(crate) fn class_name(&self) -> &'static str {
        CLASSES.get(self.class as usize).copied().unwrap_or("warrior")
    }
    fn gender_name(&self) -> &'static str {
        if self.gender == 1 { "female" } else { "male" }
    }
    /// "<class>_<gender>" or "<class>_<gender>_<ethnicity>": the body set's stem.
    fn body_stem(&self) -> String {
        match ETHNICITIES.get(self.body as usize).copied().unwrap_or("") {
            "" => format!("{}_{}", self.class_name(), self.gender_name()),
            ethnicity => format!("{}_{}_{ethnicity}", self.class_name(), self.gender_name()),
        }
    }
    pub(crate) fn skeleton(&self) -> &str {
        match self.look.as_ref().or(self.shape.as_ref()) {
            Some(look) => &look.skeleton,
            None if self.class == 3 => "uniskel_dwarf",
            None => "uniskel",
        }
    }

    /// The animation set name, GamePlayer::GetAnimSet's rule (an NPC's own AnimSet).
    pub fn animation_set(&self) -> String {
        if let Some(look) = self.look.as_ref().or(self.shape.as_ref().filter(|s| !s.anim_set.is_empty())) {
            return look.anim_set.clone();
        }
        let class = self.class_name();
        let suffix = if class == "ranger" {
            if self.armament == 5 { "large_weapon" } else { "small_weapon" }
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
    fn parts(&self, exists: &dyn Fn(&str) -> bool, tags: &HashMap<String, SkinTag>) -> Vec<(String, Option<[f32; 4]>)> {
        // A whole transformation (its own anim set: the Iron Dwarf) wears its
        // skin alone; a costume (a SkinModifier only) goes over the outfit, its
        // tags covering what it hides (cull_covered_parts).
        if let Some(look) = self.look.as_ref().or(self.shape.as_ref().filter(|s| !s.anim_set.is_empty())) {
            return look.parts.iter().filter(|p| exists(p)).map(|p| (p.clone(), None)).collect();
        }
        let (class, gender) = (self.class_name(), self.gender_name());
        let mut out = Vec::new();
        let resolve = |skin: &str| {
            let female = format!("{skin}_female");
            if self.gender == 1 && exists(&female) { Some(female) }
            else if exists(skin) { Some(skin.to_owned()) }
            else { None }
        };
        let worn: Vec<_> = self.equipment.iter().flat_map(|e| e.skin_parts.iter()
            .filter_map(|skin| resolve(skin).map(|part| (part, e.colour)))).collect();
        let stem = self.body_stem();
        for p in BODY_PARTS {
            for candidate in [format!("{stem}_{p}"), format!("{class}_{gender}_{p}"), format!("{gender}_{p}")] {
                if exists(&candidate) {
                    out.push((candidate, None));
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
        for (slot, kind) in [(2, "torso"), (4, "boots"), (3, "gloves")] {
            // A missing converted mesh must not also remove the base clothing.
            let covered = self.equipment.iter().any(|e| e.slot == slot && e.skin_parts.iter().any(|p| resolve(p).is_some()));
            if covered {
                continue;
            }
            let base = format!("{class}_{kind}_00");
            let female = format!("{base}_female");
            if self.gender == 1 && exists(&female) {
                out.push((female, None));
            } else if exists(&base) {
                out.push((base, None));
            }
        }
        for (part, colour) in worn {
            // A two-handed item appears in both remote hand slots, but its mesh
            // belongs to the character only once.
            if !out.iter().any(|(name, _)| name == &part) { out.push((part, colour)); }
        }
        for part in &self.cosmetics {
            if exists(part) && !out.iter().any(|(name, _)| name == part) {
                out.push((part.clone(), None));
            }
        }
        for skin in self.shape.iter().flat_map(|s| s.parts.iter()) {
            if let Some(part) = resolve(skin).filter(|p| !out.iter().any(|(name, _)| name == p)) {
                out.push((part, None));
            }
        }
        cull_covered_parts(&mut out, tags);
        out
    }
}

/// An actor that stands as a player does (drive_animations): its anim set's
/// Idle_Hub out of combat, Idle while it fights (track_player_combat). A
/// player's summon (crate::actors::monsters): a Shadow Soul's Idle is its
/// battle stance (retinue_ancient_watcher_01_idle_bat), its Idle_Hub at rest.
/// INFERRED: from those rows, as the players' own; the original's choice for
///   monsters was not traced. A set without Idle_Hub plays Idle throughout.
#[derive(Component)]
pub(crate) struct RelaxedIdle;

/// Logical animation states, the rows of anims.xml.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum AnimState {
    /// Combat stance for players; default standing state for other actors.
    Idle,
    /// Relaxed player idle, including outside hubs when out of combat.
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

/// A character animated at a lower rate (NPCs): its pose is evaluated one frame in
/// `stride`; between, its animation is stopped (not evaluated at all) and resumed
/// at the right time. bevy evaluates paused animations every frame, stopped ones
/// not at all.
#[derive(Component)]
pub struct AnimLod {
    pub stride: u32,
    pub phase: u32,
    /// The current clip's time when it was last stopped, and the time since.
    held: Option<(f32, f32)>,
}

impl AnimLod {
    pub fn new(stride: u32, phase: u32) -> Self {
        Self { stride: stride.max(1), phase, held: None }
    }
}

/// Set `state` (and `speed`) to change what the character plays.
#[derive(Component, Debug)]
pub struct CharacterAnim {
    audio_epoch: u64,
    pub state: AnimState,
    pub speed: f32,
    applied: Option<(AnimState, f32)>,
    player: Option<Entity>,
    /// The animation playing at full weight, and those fading out.
    current: Option<AnimationNodeIndex>,
    /// The skeleton's animation targets detached from their player while the
    /// character is not animated: bevy's animate_targets visits every target of
    /// every character each frame otherwise (16 % of a browser frame with
    /// Kingshill's NPCs, most of them off screen).
    parked: Vec<(Entity, AnimatedBy)>,
    /// Asked to park this frame (done after the loop).
    parking: bool,
    fading: Vec<(AnimationNodeIndex, f32)>,
    fade_elapsed: f32,
    fade_duration: f32,
    nodes: HashMap<String, (AnimationNodeIndex, bool)>,
    clip_nodes: HashMap<String, AnimationNodeIndex>,
    durations: HashMap<AnimationNodeIndex, f32>,
    set: Option<String>,
    set_changed: bool,
    combat_remaining: f32,
    /// Killed (KillCommand): the death animation keeps its last pose; no sequence
    /// or movement brings the actor back to Idle.
    pub dead: bool,
}

impl Default for CharacterAnim {
    fn default() -> Self {
        Self { audio_epoch: 0, state: AnimState::Idle, speed: 1.0, applied: None, player: None, nodes: HashMap::new(), current: None,
            fading: Vec::new(), fade_elapsed: 0.0, fade_duration: BLEND.as_secs_f32(), clip_nodes: HashMap::new(),
            durations: HashMap::new(), set: None, set_changed: false, combat_remaining: 0.0,
            parked: Vec::new(), parking: false, dead: false }
    }
}

impl CharacterAnim {
    /// The animation playing at full weight: its player, node and length.
    pub(crate) fn playing(&self) -> Option<(Entity, AnimationNodeIndex, f32)> {
        let node = self.current?;
        Some((self.player?, node, self.durations.get(&node).copied().unwrap_or(0.0)))
    }

    /// Audio follows the logical clip even on frames where pose LOD stops Bevy's
    /// player. This keeps nearby offscreen NPCs audible without evaluating bones.
    pub(crate) fn audio_clock(&self, lod: Option<&AnimLod>) -> Option<(AnimationNodeIndex, Option<f32>, u64)> {
        Some((self.current?, lod.and_then(|l| l.held).map(|(at, since)| at + since * self.speed), self.audio_epoch))
    }
    /// Release a streamed-out rig without losing authoritative status state.
    pub(crate) fn release_rig(&mut self) {
        *self = Self { state: self.state.clone(), speed: self.speed, dead: self.dead,
            combat_remaining: self.combat_remaining, ..default() };
    }

    pub(crate) fn enter_combat(&mut self) {
        if !self.dead { self.combat_remaining = COMBAT_IDLE_SECONDS; }
    }

    /// Whether bevy evaluates this character's animation (playing, bones hooked).
    pub fn is_animating(&self) -> bool {
        self.current.is_some() && self.parked.is_empty()
    }

    /// Play the current state again from its start, even if it is already playing
    /// (a skill used twice in a row).
    pub fn replay(&mut self) {
        self.applied = None;
    }

    /// Whether the character's skeleton and animations are ready.
    pub fn is_ready(&self) -> bool {
        self.player.is_some()
    }

    /// Completion of the requested clip, excluding a previous state or the
    /// Idle fallback. None means the rig/clip is unavailable or culled.
    pub(crate) fn playback_finished(&self, players: &Query<&AnimationPlayer>) -> Option<bool> {
        let &(node, _) = self.nodes.get(self.state.key())?;
        if self.current != Some(node) || self.applied.as_ref()?.0 != self.state { return None; }
        Some(players.get(self.player?).ok()?.animation(node)?.is_finished())
    }

    /// The animation states this character has clips for (crate::diagnostics::debug).
    pub fn states(&self) -> Vec<String> {
        let mut v: Vec<String> = self.nodes.keys().cloned().collect();
        v.sort();
        v
    }

    /// Drive a whole model's own animation player (monsters, crate::actors::monsters):
    /// `nodes` are its graph's clips by state row, with whether each loops.
    pub fn bind_model(&mut self, player: Entity, nodes: HashMap<String, (AnimationNodeIndex, bool)>) {
        self.player = Some(player);
        self.nodes = nodes;
        self.current = None;
        self.fading.clear();
        self.applied = None;
    }

    fn advance_blend(&mut self, player: &mut AnimationPlayer, dt: f32) {
        if self.fading.is_empty() { return; }
        self.fade_elapsed += dt;
        let remaining = 1.0 - (self.fade_elapsed / self.fade_duration).clamp(0.0, 1.0);
        let mut outgoing = 0.0;
        self.fading.retain(|&(node, weight)| {
            if remaining == 0.0 { player.stop(node); return false; }
            if let Some(a) = player.animation_mut(node) {
                a.set_weight(weight * remaining);
                outgoing += weight * remaining;
            }
            true
        });
        if let Some(a) = self.current.and_then(|node| player.animation_mut(node)) {
            a.set_weight((1.0 - outgoing).clamp(0.0, 1.0));
        }
    }

    fn transition(&mut self, player: &mut AnimationPlayer, node: AnimationNodeIndex, looping: bool) {
        let same_state = self.applied.as_ref().is_some_and(|(state, _)| *state == self.state);
        if !same_state || self.current != Some(node) { self.audio_epoch = self.audio_epoch.wrapping_add(1); }
        let preserve_phase = self.set_changed && same_state;
        let seek = if preserve_phase && self.current != Some(node) {
            self.current.and_then(|old| {
                let duration = *self.durations.get(&old)?;
                let next_duration = *self.durations.get(&node)?;
                let at = player.animation(old)?.seek_time();
                (duration > 0.0).then(|| (at / duration).clamp(0.0, 1.0) * next_duration)
            })
        } else { None };
        if self.current != Some(node) {
            // Snapshot the actual mixed pose, including interrupted transitions.
            // Starting the new clip at weight 1 would jump halfway to its pose.
            let total: f32 = player.playing_animations().map(|(_, a)| a.weight()).sum();
            self.fading = player.playing_animations().filter(|(n, a)| **n != node && a.weight() > 0.0)
                .map(|(n, a)| (*n, a.weight() / total.max(f32::EPSILON))).collect();
            let outgoing: f32 = self.fading.iter().map(|(_, w)| *w).sum();
            self.fade_elapsed = 0.0;
            self.fade_duration = if self.set_changed { EQUIPMENT_BLEND } else { BLEND }.as_secs_f32();
            player.play(node).set_weight((1.0 - outgoing).clamp(0.0, 1.0));
        }
        let active = player.play(node);
        active.set_speed(self.speed);
        if looping && !self.dead { active.repeat(); }
        else {
            active.set_repeat(bevy::animation::RepeatAnimation::Never);
            if !preserve_phase { active.replay(); }
        }
        if let Some(at) = seek { active.set_seek_time(at); }
        self.current = Some(node);
        self.applied = Some((self.state.clone(), self.speed));
        self.set_changed = false;
    }
}

/// The joints of a whole model (not on the shared skeleton), by name, for effects
/// attached to a joint (crate::gameplay::skills).
#[derive(Component)]
pub struct ModelBones(pub HashMap<String, Entity>);

#[derive(Component)]
pub struct Character {
    pub desc: CharacterDesc,
    built: bool,
    bones: Option<HashMap<String, Entity>>,
    pending_parts: Vec<Entity>,
    skeleton: Option<Entity>,
    displayed_set: String,
    drawn_parts: Vec<OutfitPart>,
    pending_outfit: Option<PendingOutfit>,
    /// Identity of the shared skeleton; equipment swaps retain this generation.
    generation: u32,
    /// Riding (crate::actors::mounts): the mount's anim set, played in place of
    /// the character's own -- its Idle / Run clips have rider twins of the same
    /// name in the uniskel skeletons (mount_idle_polarbear_01-loop...).
    pub(crate) riding: Option<String>,
    /// An animation set to wear for the preview's sequences (anim_preview):
    /// over the riding and the own one.
    preview_set: Option<String>,
}

#[derive(Clone)]
struct OutfitPart { entity: Entity, name: String, colour: Option<[f32; 4]> }

struct PendingOutfit { parts: Vec<OutfitPart>, introduced: Vec<Entity>, animation_set: String }

impl Character {
    pub fn outfit_ready(&self) -> bool { self.outfit_wait().is_none() }
    /// What the outfit still waits for (loading diagnostics); None once it is ready.
    pub(crate) fn outfit_wait(&self) -> Option<&'static str> {
        if !self.built { Some("outfit not built") }
        else if self.pending_outfit.is_some() { Some("outfit change loading") }
        else if self.drawn_parts.is_empty() { Some("no drawable parts") }
        else { None }
    }
    /// A joint of this character's shared skeleton, by name (once it is in).
    pub fn bone(&self, name: &str) -> Option<Entity> {
        self.bones.as_ref()?.get(name).copied()
    }
    /// The shared skeleton's scene root (a child of the character): moving it
    /// moves the whole body (crate::actors::mounts seats it on a saddle).
    pub fn skeleton_root(&self) -> Option<Entity> {
        self.skeleton
    }
}

#[derive(Component)]
struct SkeletonOf(Entity, u32);
#[derive(Component)]
struct PartOf(Entity, u32);

/// Only these children belong to the outfit; attachments survive a redress.
#[derive(Component)]
pub struct CharacterVisual;

#[derive(Component)]
struct EquipmentDye([f32; 4]);

#[derive(Component)]
struct EquipmentDyed;

/// A skeleton's body shape by name (variations.json), or empty while it loads.
pub(crate) fn named_variation(library: &CharacterLibrary, variations: &Assets<crate::actors::npc::Variations>, skeleton: &str, name: &str) -> Vec<(String, Vec3, Vec3)> {
    library.variations.get(skeleton).and_then(|h| variations.get(h)).and_then(|all| all.variations.get(name))
        .map(|shape| shape.iter().map(|j| (j.bone.clone(), Vec3::from(j.t), Vec3::from(j.s))).collect())
        .unwrap_or_default()
}

#[derive(Resource)]
pub struct CharacterLibrary {
    pub anims: Handle<AnimTable>,
    pub item_skins: Handle<ItemSkins>,
    parts: HashMap<&'static str, Handle<PartList>>,
    skeletons: HashMap<&'static str, Handle<Gltf>>,
    /// The skeletons' body shapes, for the players' own (see body_variation).
    variations: HashMap<&'static str, Handle<crate::actors::npc::Variations>>,
    /// The hair and beard tables (CharacterDesc::styles).
    selection: Handle<crate::screens::selection::SelectionData>,
}

#[cfg(test)]
impl CharacterLibrary {
    pub(crate) fn for_test(item_skins: Handle<ItemSkins>) -> Self {
        Self { item_skins, anims: default(), parts: default(), skeletons: default(), variations: default(), selection: default() }
    }
}

pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<JointNames>();
        let handlers = app.world().resource::<bevy::gltf::extensions::GltfExtensionHandlers>().0.clone();
        #[cfg(target_family = "wasm")]
        bevy::tasks::block_on(async { handlers.write().await.push(Box::new(SharedPartSkeleton)) });
        #[cfg(not(target_family = "wasm"))]
        handlers.write_blocking().push(Box::new(SharedPartSkeleton));
        app.init_asset::<AnimTable>()
            .init_asset::<PartList>()
            .init_asset::<ItemSkins>()
            .register_asset_loader(ItemSkinsLoader)
            .register_asset_loader(AnimTableLoader)
            .register_asset_loader(PartListLoader)
            .add_systems(Startup, load_library)
            .add_systems(PreUpdate, track_player_combat.after(crate::network::net::NetReceive))
            .add_systems(Update, (build_characters, revive_skeletons, keep_parts_bound, apply_equipment_dyes,
                finish_outfits, update_animation_sets, drive_animations, check_skins, prune_idle_targets).chain()
                .after(crate::network::net::NetSystems).after(crate::actors::npc::NpcVisualSystems))
            .add_systems(
                PostUpdate,
                apply_variations.after(bevy::app::AnimationSystems).before(TransformSystems::Propagate),
            )
            .add_observer(on_instance_ready)
            .add_plugins(anim_preview::AnimPreviewPlugin);
    }
}

fn load_library(mut commands: Commands, assets: Res<AssetServer>) {
    let mut parts = HashMap::new();
    let mut skeletons = HashMap::new();
    let mut variations = HashMap::new();
    for skel in ["uniskel", "uniskel_dwarf"] {
        parts.insert(skel, assets.load(format!("characters/{skel}/parts.json")));
        skeletons.insert(skel, assets.load(format!("characters/{skel}/__animations.glb")));
        variations.insert(skel, assets.load(format!("characters/{skel}/variations.json")));
    }
    commands.insert_resource(CharacterLibrary {
        anims: assets.load("characters/anims.json"),
        item_skins: assets.load("characters/item_skins.json"),
        parts,
        skeletons,
        variations,
        selection: assets.load("characters/selection.json"),
    });
}

/// A player's body shape: its class and gender's body set's variation.
/// EVIDENCE: _Template_Player.CharacterSet (warrior_male_body_00, mage_male_body_00,
///   ranger_male_body_00, dwarf_male_body_01) and every <class>_<gender>_body_0N of
///   outfits.json carry var_<class>_<gender>_00 (variations.json).
/// FAILURE: without it the players kept the bare skeleton's proportions ("mon
///   perso semble moins gros que l'officiel").
fn body_variation(desc: &CharacterDesc, library: Option<&CharacterLibrary>, variations: &Assets<crate::actors::npc::Variations>) -> Vec<(String, Vec3, Vec3)> {
    let Some(all) = library.and_then(|l| l.variations.get(desc.skeleton())).and_then(|h| variations.get(h)) else { return Vec::new() };
    let name = format!("var_{}_{}_{:02}", desc.class_name(), desc.gender_name(), desc.stature);
    let fallback = format!("var_{}_{}_00", desc.class_name(), desc.gender_name());
    all.variations
        .get(&name)
        .or_else(|| all.variations.get(&fallback))
        .map(|shape| shape.iter().map(|j| (j.bone.clone(), Vec3::from(j.t), Vec3::from(j.s))).collect())
        .unwrap_or_default()
}

/// Spawn a character; it dresses and animates itself once its assets are in.
pub fn spawn_character(commands: &mut Commands, desc: CharacterDesc, transform: Transform) -> Entity {
    let entity = commands
        .spawn((
            Name::new(format!("character {}", desc.animation_set())),
            CharacterAnim::default(),
            transform,
            Visibility::default(),
        ))
        .id();
    attach_character(commands, entity, desc);
    entity
}

/// Build visuals on an existing actor. Its identity, placement and logical
/// animation state survive online NPC visual eviction/re-entry.
pub(crate) fn attach_character(commands: &mut Commands, entity: Entity, desc: CharacterDesc) {
    let displayed_set = desc.animation_set();
    commands.entity(entity).insert(Character {
        desc, built: false, bones: None, pending_parts: Vec::new(), generation: 0,
        skeleton: None, displayed_set, drawn_parts: Vec::new(), pending_outfit: None, riding: None, preview_set: None,
    });
}

fn build_characters(
    mut commands: Commands,
    mut characters: Query<(Entity, &mut Character)>,
    library: Option<Res<CharacterLibrary>>,
    part_lists: Res<Assets<PartList>>,
    assets: Res<AssetServer>,
    variations: Res<Assets<crate::actors::npc::Variations>>,
    selections: Res<Assets<crate::screens::selection::SelectionData>>,
) {
    let Some(library) = library else { return };
    for (entity, mut character) in &mut characters {
        if character.built {
            continue;
        }
        // A player's hair and beard, with no gender check: the world draws a
        // woman's beard style too (NewPlayer handling 0x14020f828).
        if let Some((hair, beard)) = character.desc.styles {
            match selections.get(&library.selection) {
                Some(data) => {
                    let (class, gender) = (character.desc.class, character.desc.gender);
                    character.desc.cosmetics = crate::screens::selection::cosmetics(data, class, gender, Some(hair), Some(beard));
                    character.desc.styles = None;
                }
                None if !assets.load_state(&library.selection).is_failed() => continue,
                None => character.desc.styles = None,
            }
        }
        let skel = character.desc.skeleton().to_owned();
        let skel = skel.as_str();
        let Some(list) = library.parts.get(skel).and_then(|h| part_lists.get(h)) else { continue };
        // A player's body shape comes from its skeleton's variations (body_variation).
        if character.desc.look.is_none()
            && library.variations.get(skel).is_some_and(|h| variations.get(h).is_none() && !assets.load_state(h).is_failed())
        {
            continue;
        }
        let names: std::collections::HashSet<&str> = list.parts.iter().map(String::as_str).collect();
        let parts = character.desc.parts(&|n| names.contains(n), &list.tags);
        character.built = true;
        // Cancel only unpublished parts when a newer equipment packet arrives.
        if let Some(pending) = character.pending_outfit.take() {
            for part in pending.introduced { commands.entity(part).try_despawn(); }
        }
        let skeleton_scene = assets.load(GltfAssetLabel::Scene(0).from_asset(format!("characters/{skel}/__animations.glb")));
        // CONTRACT: a Transform (and Visibility) on every child, or the hierarchy
        //   does not carry the character's movement down to the skeleton.
        // FAILURE (2026-10-06): without them the body stayed where the player had
        //   arrived while the player walked on ("je suis un fantome qui se deplace").
        if character.skeleton.is_none() {
            character.skeleton = Some(commands.spawn((
            SkeletonOf(entity, character.generation),
            CharacterVisual,
            WorldAssetRoot(skeleton_scene),
            ChildOf(entity),
            Transform::default(),
            Visibility::default(),
            )).id());
        }
        let mut outfit = Vec::new();
        let mut introduced = Vec::new();
        for (part, colour) in parts {
            if let Some(existing) = character.drawn_parts.iter().find(|p| p.name == part && p.colour == colour) {
                outfit.push(existing.clone());
                continue;
            }
            let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(format!("characters/{skel}/parts/{part}.glb")));
            let mut entity = commands.spawn((
                PartOf(entity, character.generation),
                CharacterVisual,
                WorldAssetRoot(scene),
                ChildOf(entity),
                Name::new(part.clone()),
                Transform::default(),
                Visibility::Hidden,
            ));
            if let Some(colour) = colour { entity.insert(EquipmentDye(colour)); }
            introduced.push(entity.id());
            outfit.push(OutfitPart { entity: entity.id(), name: part, colour });
        }
        character.pending_outfit = Some(PendingOutfit { parts: outfit, introduced, animation_set: character.desc.animation_set() });
    }
}

/// Publish an outfit atomically after its scene, textures and bindings are ready.
/// Common body/clothing parts stay on the same skeleton throughout the swap.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct OutfitReadiness<'w, 's> {
    parts: Query<'w, 's, (&'static bevy::world_serialization::WorldInstance, &'static WorldAssetRoot)>,
    instances: Res<'w, bevy::world_serialization::WorldInstanceSpawner>,
    children: Query<'w, 's, &'static Children>,
    meshes: Query<'w, 's, (&'static Mesh3d, Option<&'static MeshMaterial3d<StandardMaterial>>,
        Option<&'static MeshMaterial3d<crate::rendering::surfaces::NebulaMaterial>>, Option<&'static MeshMaterial3d<crate::rendering::refraction::RefractionMaterial>>, Option<&'static SkinnedMesh>)>,
    standard: Res<'w, Assets<StandardMaterial>>,
    surfaces: Res<'w, Assets<crate::rendering::surfaces::NebulaMaterial>>,
    refractions: Res<'w, Assets<crate::rendering::refraction::RefractionMaterial>>,
    mesh_assets: Res<'w, Assets<Mesh>>,
    images: Res<'w, Assets<Image>>,
    assets: Res<'w, AssetServer>,
}

fn material_textures_ready(base: &StandardMaterial, extra: &[&Option<Handle<Image>>], loaded: impl Fn(bevy::asset::UntypedAssetId) -> bool) -> bool {
    let mut ready = true;
    base.visit_dependencies(&mut |id| ready &= loaded(id));
    ready && extra.iter().all(|image| image.as_ref().is_none_or(|image| loaded(image.id().untyped())))
}

impl OutfitReadiness<'_, '_> {
    /// A failed render dependency can be reported separately from one still
    /// loading, even when the scene's recursive status has not caught up.
    pub(crate) fn failed(&self, entity: Entity) -> bool {
        let failed = |id| crate::screens::loading::asset_failed(&self.assets, id);
        self.children.iter_descendants(entity).any(|e| {
            let Ok((mesh, standard, surface, refraction, skin)) = self.meshes.get(e) else { return false };
            failed(mesh.id().untyped())
                || skin.is_some_and(|s| failed(s.inverse_bindposes.id().untyped()))
                || standard.is_some_and(|h| failed(h.id().untyped()) || self.standard.get(&h.0)
                    .is_some_and(|m| !material_textures_ready(m, &[], |id| !failed(id))))
                || surface.is_some_and(|h| failed(h.id().untyped()) || self.surfaces.get(&h.0)
                    .is_some_and(|m| !material_textures_ready(&m.base, &[&m.extension.cube, &m.extension.layer, &m.extension.mask], |id| !failed(id))))
                || refraction.is_some_and(|h| failed(h.id().untyped()) || self.refractions.get(&h.0)
                    .is_some_and(|m| !material_textures_ready(&m.base, &[&m.extension.dudv], |id| !failed(id))))
        })
    }

    pub(crate) fn ready(&self, entity: Entity) -> bool {
        let Ok((instance, _)) = self.parts.get(entity) else { return false };
        if !self.instances.instance_is_ready(**instance) { return false; }
        // A glTF scene's recursive dependency status can stay Loading after all
        // its render assets arrive. Check the actual mesh/material textures,
        // including cloned dye materials and extension textures, instead.
        self.children.iter_descendants(entity).all(|e| {
            let Ok((mesh, standard, surface, refraction, skin)) = self.meshes.get(e) else { return true };
            if skin.is_some_and(|s| s.joints.is_empty()) { return false; }
            if !self.mesh_assets.contains(mesh.id()) && !self.assets.is_loaded(mesh.id()) { return false; }
            let loaded = |id: bevy::asset::UntypedAssetId| self.assets.is_loaded(id)
                || id.try_typed::<Image>().is_ok_and(|id| self.images.contains(id));
            standard.is_none_or(|h| self.standard.get(&h.0).is_some_and(|m| material_textures_ready(m, &[], loaded)))
                && surface.is_none_or(|h| self.surfaces.get(&h.0).is_some_and(|m|
                    material_textures_ready(&m.base, &[&m.extension.cube, &m.extension.layer, &m.extension.mask], loaded)))
                && refraction.is_none_or(|h| self.refractions.get(&h.0).is_some_and(|m|
                    material_textures_ready(&m.base, &[&m.extension.dudv], loaded)))
        })
    }
}

fn finish_outfits(
    mut commands: Commands,
    mut characters: Query<(&mut Character, &CharacterAnim)>,
    readiness: OutfitReadiness,
    time: Res<Time>,
    mut diagnostic: Local<f32>,
) {
    *diagnostic += time.delta_secs();
    let diagnose = *diagnostic > 2.0 && std::env::var("DSOR_CHECK_SKIN").is_ok();
    if *diagnostic > 2.0 { *diagnostic = 0.0; }
    for (mut character, anim) in &mut characters {
        if character.bones.is_none() || (character.desc.look.is_none() && anim.current.is_none()) { continue; }
        let Some(pending) = &character.pending_outfit else { continue };
        // Ask the spawner directly: scene replacement during asynchronous loading
        // does not reliably deliver another WorldInstanceReady event.
        if !pending.introduced.iter().all(|e| readiness.ready(*e)) {
            if diagnose {
                for e in &pending.introduced {
                    match readiness.parts.get(*e) {
                        Ok((_, scene)) if !readiness.ready(*e) =>
                            info!("outfit waiting {e:?} {:?}: scene or render assets not ready", readiness.assets.get_path(scene.id())),
                        Err(_) => info!("outfit waiting {e:?}: no scene instance"),
                        _ => {}
                    }
                }
            }
            continue;
        }
        publish_outfit(&mut commands, &mut character);
    }
}

fn publish_outfit(commands: &mut Commands, character: &mut Character) {
    let Some(pending) = character.pending_outfit.take() else { return };
    for part in &character.drawn_parts {
        if !pending.parts.iter().any(|p| p.entity == part.entity) { commands.entity(part.entity).try_despawn(); }
    }
    for part in pending.introduced { commands.entity(part).insert(Visibility::Inherited); }
    character.drawn_parts = pending.parts;
    character.displayed_set = pending.animation_set;
}

/// Clone the material for this mesh: modifying a loaded asset would dye every
/// player wearing it. The existing character shader applies colour through its
/// authored dye mask, leaving skin, metal trim and glow materials alone.
fn apply_equipment_dyes(
    mut commands: Commands,
    parts: Query<(Entity, &EquipmentDye)>,
    children: Query<&Children>,
    meshes: Query<&MeshMaterial3d<crate::rendering::surfaces::NebulaMaterial>, Without<EquipmentDyed>>,
    mut materials: ResMut<Assets<crate::rendering::surfaces::NebulaMaterial>>,
) {
    for (part, dye) in &parts {
        for entity in children.iter_descendants(part) {
            let Ok(handle) = meshes.get(entity) else { continue };
            let Some(original) = materials.get(&handle.0) else { continue };
            if original.extension.params.p0.x != crate::rendering::surfaces::Kind::Character as u32 as f32 { continue; }
            let mut material = original.clone();
            // surface.wgsl mixes character dyes in gamma space, like Nebula.
            material.extension.params.p1 = Vec4::from_array(dye.0);
            commands.entity(entity).insert((MeshMaterial3d(materials.add(material)), EquipmentDyed));
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
    graphs: ResMut<'w, Assets<AnimationGraph>>,
    joints: Query<'w, 's, (&'static Transform, Option<&'static ChildOf>)>,
    parents: Query<'w, 's, &'static ChildOf>,
    variations: Res<'w, Assets<crate::actors::npc::Variations>>,
}

/// Give the joints a body variation (VariedJoint): (joint, translation, scale) per
/// joint. Returns each varied joint's scaled child by the joint's name: what the
/// skinned vertices of that joint must be bound to.
pub fn vary_joints(
    commands: &mut Commands,
    joints: &Query<(&Transform, Option<&ChildOf>)>,
    bones: &HashMap<String, Entity>,
    variation: &[(String, Vec3, Vec3)],
) -> HashMap<String, Entity> {
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
            scale: var_s * tf.scale,
        });
        scaled_bones.insert(name.to_owned(), scaled);
    }
    scaled_bones
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
    // Equipment swaps keep this player and graph. A genuinely replaced scene
    // instance needs a new binding; animation tables may finish loading later.
    if let Some(player) = player {
        if anim.player != Some(player) {
            commands
                .entity(player)
                .insert((AnimationGraphHandle(ctx.graphs.add(AnimationGraph::new())), crate::rendering::anim_cull::ManagedAnimation));
            anim.player = Some(player);
            anim.current = None;
            anim.fading.clear();
            anim.applied = None;
            anim.nodes.clear();
            anim.clip_nodes.clear();
            anim.durations.clear();
            anim.set = None;
            anim.set_changed = false;
            anim.parked.clear();
            anim.parking = false;
        }
    }
    let variation = match character.desc.look.as_ref().or(character.desc.shape.as_ref().filter(|s| !s.variation.is_empty())) {
        Some(l) => l.variation.clone(),
        None => body_variation(&character.desc, ctx.library.as_deref(), &ctx.variations),
    };
    if !variation.is_empty() {
        let scaled_bones = vary_joints(commands, &ctx.joints, &bones, &variation);
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

/// Append newly needed clips to the existing graph, retaining old clips while
/// they fade out. Reusing nodes also keeps a common clip's playback uninterrupted.
fn update_animation_sets(
    mut commands: Commands,
    mut characters: Query<(Entity, &Character, &mut CharacterAnim)>,
    players: Query<&AnimationGraphHandle>,
    library: Option<Res<CharacterLibrary>>,
    tables: Res<Assets<AnimTable>>,
    gltfs: Res<Assets<Gltf>>,
    clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let Some(library) = library else { return };
    for (entity, character, mut anim) in &mut characters {
        let set = character.preview_set.as_ref().or(character.riding.as_ref()).unwrap_or(&character.displayed_set);
        if anim.set.as_ref() == Some(set) { continue; }
        let Some(player) = anim.player else { continue };
        let Ok(handle) = players.get(player) else { continue };
        let Some(mut graph) = graphs.get_mut(&handle.0) else { continue };
        let Some(gltf) = library.skeletons.get(character.desc.skeleton()).and_then(|h| gltfs.get(h)) else { continue };
        let Some(table) = tables.get(&library.anims).and_then(|t| t.0.get(set)) else { continue };
        if table.values().filter_map(|name| gltf.named_animations.get(name.as_str())).any(|clip| clips.get(clip).is_none()) { continue; }
        let mut nodes = HashMap::new();
        let root = graph.root;
        for (state, name) in table {
            let Some(clip) = gltf.named_animations.get(name.as_str()) else { continue };
            let Some(data) = clips.get(clip) else { continue };
            let node = *anim.clip_nodes.entry(name.clone()).or_insert_with(|| graph.add_clip(clip.clone(), 1.0, root));
            anim.durations.insert(node, data.duration());
            nodes.insert(state.clone(), (node, name.ends_with("-loop")));
        }
        if nodes.is_empty() { continue; }
        anim.nodes = nodes;
        anim.set = Some(set.clone());
        anim.set_changed = anim.current.is_some();
        let profile = character.desc.look.as_ref().map(|l| l.sound_profile.clone()).unwrap_or_else(||
            format!("{}_{}_body_00", character.desc.class_name(), character.desc.gender_name()));
        commands.entity(player).insert(crate::audio::AnimationAudio::new(entity,
            &format!("characters/{}", character.desc.skeleton()), &profile,
            anim.clip_nodes.iter().map(|(name, node)| (*node, name.clone())).collect()));
    }
}

/// Recover bindings when an individual shared bone or animation player dies.
/// Bevy 0.19 emits WorldInstanceReady for complete scene replacements, handled
/// by on_instance_ready. This fallback also covers partial removals and delayed
/// reconstruction. Every cached bone had a Name when adopted, so removals wake
/// the sweep instead of checking every retained bone on every steady frame.
/// FAILURE (2026-10-06): "si ca n'a pas fini de charger et que je me deplace tout
///   reste a sa place"; after quick casts "l'anim ne marche plus" (the player was
///   gone). Reproduced with DSOR_CHECK_SKIN: parts bound to a dead joint.
fn revive_skeletons(
    alive: Query<()>,
    skeletons: Query<(Entity, &SkeletonOf, &Children)>,
    mut removed_names: RemovedComponents<Name>,
    mut removed_players: RemovedComponents<AnimationPlayer>,
    mut retry: Local<bool>,
    mut ctx: SkeletonCtx,
) {
    // Drain both readers even when diagnostics force a sweep. A partial rebuild
    // can be empty this frame and acquire its children without another removal.
    let names_removed = removed_names.read().count() != 0;
    let players_removed = removed_players.read().count() != 0;
    let check_skins = std::env::var("DSOR_CHECK_SKIN").is_ok();
    if !names_removed && !players_removed && !*retry && !check_skins { return; }
    let mut stale = Vec::new();
    if check_skins {
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
    // Also keep watching a still-live bone whose Name was removed: its later
    // despawn cannot emit another Name removal. Newly reserved scale helpers
    // become alive when commands apply, so they need at most one extra sweep.
    *retry = ctx.characters.iter().any(|(character, anim)| character.skeleton.is_some() && (
        character.bones.as_ref().is_some_and(|bones| bones.values().any(|bone|
            !alive.contains(*bone) || !ctx.names.contains(*bone)))
        || anim.player.is_some_and(|player| !alive.contains(player))
    ));
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
    let Some(joints) = joint_names.iter().enumerate()
        .map(|(i, n)| bones.get(n.as_str()).copied().or_else(|| skin.joints.get(i).copied()))
        .collect::<Option<Vec<Entity>>>() else { return };
    // CONTRACT: a re-bound part's bounds are a box around the whole character, not
    //   the box computed for its own unanimated skeleton copy. The part's mesh sits
    //   at the character's origin, so a character-sized box in its space covers
    //   wherever the shared skeleton draws it.
    // FAILURE (2026-10-06): with NoFrustumCulling every part of every NPC was drawn
    //   off screen too -- ~11 skinned meshes per NPC, 20 FPS in the browser.
    commands
        .entity(e)
        .insert((
            SkinnedMesh { inverse_bindposes: skin.inverse_bindposes.clone(), joints },
            bevy::camera::primitives::Aabb::from_min_max(CHARACTER_BOUNDS.0, CHARACTER_BOUNDS.1),
        ))
        // Our fixed character box replaces bevy's per-frame skinned bounds
        // (0.6 ms a frame for Kingshill's NPCs, single-threaded).
        .remove::<bevy::camera::visibility::DynamicSkinnedMeshBounds>();
}

/// A box around any posed character (weapons and capes included), in its own space.
const CHARACTER_BOUNDS: (Vec3, Vec3) = (Vec3::new(-2.0, -0.5, -2.0), Vec3::new(2.0, 3.5, 2.0));


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
#[derive(Component, Clone, Reflect)]
#[reflect(Component)]
struct JointNames(Vec<String>);

/// Equipment uses the actor's skeleton. Strip its private copy from the scene
/// asset before instancing, rather than spawning ~69 joints and immediately
/// despawning them for every piece and every scene reload.
#[derive(Clone)]
struct SharedPartSkeleton;

impl bevy::gltf::extensions::GltfExtensionHandler for SharedPartSkeleton {
    fn dyn_clone(&self) -> Box<dyn bevy::gltf::extensions::ErasedGltfExtensionHandler> { Box::new(self.clone()) }

    fn on_scene_completed(&mut self, context: &mut bevy::asset::LoadContext<'_>, _: &gltf::Scene,
        root: Entity, world: &mut World) {
        if !context.path().path().components().any(|c| c.as_os_str() == "parts") { return; }
        strip_part_skeleton(root, world);
    }
}

fn strip_part_skeleton(root: Entity, world: &mut World) {
    let skins: Vec<_> = world.query::<(Entity, &SkinnedMesh)>().iter(world).map(|(e, s)| (e, s.clone())).collect();
    let mut skeleton_roots = std::collections::HashSet::new();
    for (entity, skin) in skins {
        let Some(names) = skin.joints.iter().map(|&j| world.get::<Name>(j).map(|n| n.as_str().to_owned()))
            .collect::<Option<Vec<_>>>() else { continue };
        let mut roots = std::collections::HashSet::new();
        for &joint in &skin.joints {
            let mut top = joint;
            while let Some(parent) = world.get::<ChildOf>(top).map(ChildOf::parent).filter(|&p| p != root) { top = parent; }
            roots.insert(top);
        }
        // Only the separate skeleton trees produced by the equipment converter.
        // Retain any unusual asset with renderable content under a joint tree.
        let meshes: Vec<_> = world.query_filtered::<Entity, With<Mesh3d>>().iter(world).collect();
        if meshes.iter().any(|&mesh| {
            let mut next = Some(mesh);
            while let Some(e) = next {
                if roots.contains(&e) { return true; }
                next = world.get::<ChildOf>(e).map(ChildOf::parent);
            }
            false
        }) { continue; }
        skeleton_roots.extend(roots);
        world.entity_mut(entity).insert((JointNames(names), SkinnedMesh { joints: Vec::new(), ..skin }));
    }
    for root in skeleton_roots { world.despawn(root); }
}

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
        if !skin.joints.is_empty() && skin.joints.iter().all(|j| shared.contains(j)) {
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
    mut set: ParamSet<(
        Query<(Entity, Ref<Transform>, &mut VariedJoint), Without<VariedChild>>,
        Query<&mut Transform, Without<VariedChild>>,
    )>,
    mut scaled_tf: Query<&mut Transform, With<VariedChild>>,
    mut dirty: Local<Vec<(Entity, Vec3, Entity, Vec3)>>,
) {
    // Only joints the animation moved since the last run (a frozen or off-screen
    // character costs nothing): raw pose and scale first, so a child reads its
    // parent's scale of this frame.
    dirty.clear();
    {
        let mut joints = set.p0();
        let mut moved = Vec::new();
        for (e, tf, mut j) in &mut joints {
            // A joint varied after its scene came in, and no clip moves it (a
            // monster whose Idle clip the model lacks: minion_teslaturret's
            // all-0.4 variation), has an unchanged Transform: applied once anyway,
            // or its vertices shrink about joints left at full-size offsets.
            if (!tf.is_changed() && j.written.is_some()) || j.written == Some((tf.translation, tf.scale)) {
                continue;
            }
            j.raw = (tf.translation, tf.scale);
            j.scale = j.var_s * j.raw.1;
            moved.push(e);
        }
        for e in moved {
            let Ok((_, _, j)) = joints.get(e) else { continue };
            let parent_scale = j.parent.and_then(|p| joints.get(p).ok()).map(|(_, _, p)| p.scale).unwrap_or(Vec3::ONE);
            let t = (j.raw.0 + j.var_t - j.bind_t) * parent_scale;
            let (scaled, scale) = (j.scaled, j.scale);
            if let Ok((_, _, mut j)) = joints.get_mut(e) {
                j.written = Some((t, Vec3::ONE));
            }
            dirty.push((e, t, scaled, scale));
        }
    }
    // CONTRACT: write only what changes: every write marks the joint changed and
    //   makes bevy re-propagate the skeleton.
    let mut tfs = set.p1();
    for &(e, t, scaled, scale) in dirty.iter() {
        if let Ok(mut tf) = tfs.get_mut(e) {
            if tf.translation != t || tf.scale != Vec3::ONE {
                tf.translation = t;
                tf.scale = Vec3::ONE;
            }
        }
        if let Ok(mut sc) = scaled_tf.get_mut(scaled) {
            if sc.scale != scale {
                sc.scale = scale;
            }
        }
    }
}

/// The scaled child of a varied joint.
#[derive(Component)]
struct VariedChild;

/// Read blows before the monster systems consume them. Healing and regeneration
/// do not put either participant in combat; blocked/immune attacks still do.
#[allow(clippy::type_complexity)]
fn track_player_combat(
    net: Option<NonSend<crate::network::net::Net>>,
    mut players: Query<(Option<&crate::network::net::LocalPlayer>, Option<&crate::network::net::RemotePlayer>, &mut CharacterAnim),
        (Or<(With<crate::network::net::LocalPlayer>, With<crate::network::net::RemotePlayer>)>,
            Or<(Without<crate::actors::monsters::Monster>, With<RelaxedIdle>)>)>,
) {
    let Some(net) = net else { return };
    if net.hits.is_empty() && net.kills.is_empty() { return; }
    for (local, remote, mut anim) in &mut players {
        let Some(actor) = local.map(|p| p.actor).or_else(|| remote.map(|p| p.actor)) else { continue };
        let hit = net.hits.iter().any(|(victim, hit)| {
            (actor == *victim || actor == hit.attacker) && hit.damage >= 0
                && !hit.combat_text.starts_with("Healing") && *victim != hit.attacker
        });
        let kill = net.kills.iter().any(|(victim, kill)| actor == kill.killer && *victim != kill.killer);
        if hit || kill { anim.enter_combat(); }
    }
}

pub(crate) fn drive_animations(
    time: Res<Time>,
    mut frame: Local<u32>,
    mut characters: Query<(Entity, &mut CharacterAnim, &GlobalTransform, &InheritedVisibility, Option<&mut AnimLod>, Option<&crate::screens::selection::Preview>, Option<&crate::actors::npc::NpcVisualPending>, Option<&Character>, Has<RelaxedIdle>), Without<anim_preview::Previewing>>,
    visibility: Option<Res<crate::rendering::anim_cull::AnimationVisibility>>,
    targets: Query<(Entity, &AnimatedBy)>,
    mut commands: Commands,
    mut players: Query<&mut AnimationPlayer>,
    mut to_park: Local<Vec<Entity>>,
    cameras: Query<(&Camera, &GlobalTransform, &bevy::camera::RenderTarget), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    me: Query<&GlobalTransform, crate::diagnostics::debug::FollowedPlayer>,
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("character::drive_animations");
    // Distance is a coarse gate; completed camera/shadow visibility additionally
    // gates NPC pose sampling without stopping their animation clocks.
    let focus = cameras.iter().find(|(_, _, target)| matches!(target, bevy::camera::RenderTarget::Window(_)))
        .map(|(_, g, _)| crate::world::map::camera_focus(g, me.iter().next().map(|p| p.translation()), 0.0));
    let near = |p: Vec3| focus.is_none_or(|f| Vec2::new(p.x - f.x, p.z - f.z).length() <= crate::world::map::CULL_DISTANCE);
    *frame = frame.wrapping_add(1);
    for (entity, mut anim, at, shown, mut lod, preview, pending, character, relaxed) in &mut characters {
        let as_player = character.filter(|c| c.desc.look.is_none());
        if as_player.is_some() || relaxed {
            // Advance even while the rig is loading or culled. Resolve standing
            // here, after movement and sequences have chosen their logical state.
            anim.combat_remaining = if anim.dead { 0.0 } else { (anim.combat_remaining - time.delta_secs()).max(0.0) };
            if !anim.dead && matches!(anim.state, AnimState::Idle | AnimState::IdleHub) {
                let riding = as_player.is_some_and(|c| c.riding.is_some());
                anim.state = if anim.combat_remaining > 0.0 || riding { AnimState::Idle } else { AnimState::IdleHub };
                anim.speed = 1.0;
            }
        }
        let Some(player) = anim.player else { continue };
        let Ok(mut p) = players.get_mut(player) else { continue };
        // A pending portrait must pose before it is revealed. Check distance
        // before the LOD early return so a hidden actor parks immediately.
        // The diagnostic freezes world actors, but selection still needs its
        // first posed preview before it can enter the world.
        // Streamed NPCs must sample their first pose while still hidden. Their
        // readiness gate is cleared on a later Update, after this frame's pose.
        let preparing = pending.is_some();
        let far = preview.is_none() && (crate::flag("noanim") || (!shown.get() && !preparing) || !near(at.translation()));
        if far {
            if anim.current.is_some() || anim.applied.is_some() {
                p.stop_all();
                anim.current = None;
                anim.fading.clear();
                anim.applied = None;
                if let Some(lod) = lod.as_mut() { lod.held = None; }
            }
            if anim.parked.is_empty() && !anim.parking {
                anim.parking = true;
                to_park.push(player);
            }
            continue;
        }
        let unseen = !preparing && visibility.as_ref().is_some_and(|v| !v.needs_animation(entity));
        let waking = !unseen && !anim.parked.is_empty();
        if unseen {
            if anim.parked.is_empty() && !anim.parking {
                anim.parking = true;
                to_park.push(player);
            }
        } else {
            anim.parking = false;
            for (bone, by) in std::mem::take(&mut anim.parked) {
                commands.entity(bone).try_insert(by);
            }
        }
        // A lower-rate character between its frames: held still, unevaluated.
        if let Some(lod) = lod.as_mut() {
            let mine = (*frame + lod.phase) % lod.stride == 0;
            let steady = !preparing && !unseen && !waking && !anim.set_changed && anim.fading.is_empty() && anim.applied.as_ref() == Some(&(anim.state.clone(), anim.speed));
            if let (true, Some(node)) = (steady, anim.current) {
                if !mine {
                    match lod.held.as_mut() {
                        Some((_, since)) => *since += time.delta_secs(),
                        None => {
                            let at = p.animation(node).map(|a| a.seek_time()).unwrap_or(0.0);
                            p.stop(node);
                            lod.held = Some((at, time.delta_secs()));
                        }
                    }
                    continue;
                }
                if let Some((at, since)) = lod.held.take() {
                    let speed = anim.speed;
                    let a = p.play(node);
                    a.set_weight(1.0).set_speed(speed).repeat();
                    a.seek_to(at + since * speed);
                }
            } else if let Some((at, since)) = lod.held.take() {
                if let Some(node) = anim.current {
                    let speed = anim.speed;
                    let a = p.play(node);
                    a.set_weight(1.0).set_speed(speed).repeat();
                    a.seek_to(at + since * speed);
                }
            }
        }
        // Offscreen NPCs still advance clips, state changes and fades. Only the
        // expensive bone targets are detached; waking does not restart a clip.
        // Our own cross-fade. bevy's AnimationTransitions stopped an animation
        // started again while its previous fade-out was still running: after two
        // quick casts the character played nothing at all ("je le lance 2x l'anim
        // marche apres ca marche plus").
        anim.advance_blend(&mut p, time.delta_secs());
        let wanted = (anim.state.clone(), anim.speed);
        if !anim.set_changed && anim.applied.as_ref() == Some(&wanted) {
            continue;
        }
        let key = anim.state.key().to_owned();
        let Some(&(node, looping)) = anim.nodes.get(&key).or_else(|| anim.nodes.get("Idle")) else {
            continue;
        };
        if preparing && anim.current.is_none() {
            commands.entity(player).insert(crate::rendering::anim_cull::PendingFirstPose::default());
        }
        anim.transition(&mut p, node, looping);
    }
    if !to_park.is_empty() {
        let wanted: std::collections::HashSet<Entity> = to_park.drain(..).collect();
        let mut found: HashMap<Entity, Vec<(Entity, AnimatedBy)>> = HashMap::new();
        for (e, by) in &targets {
            if wanted.contains(&by.0) {
                found.entry(by.0).or_default().push((e, *by));
                commands.entity(e).remove::<AnimatedBy>();
            }
        }
        for (_, mut anim, ..) in &mut characters {
            if let Some(p) = anim.player.filter(|p| wanted.contains(p)) {
                anim.parked = found.remove(&p).unwrap_or_default();
            }
        }
    }
}

/// Wear a status effect's transformation (CharacterDesc::shape), or drop it.
/// CONTRACT: a fresh skeleton instance (new generation): a body variation is
///   applied only when a skeleton is adopted, and an equipment change keeps the
///   live one.
/// FAILURE: rebuilt as an equipment change, the Iron Dwarf kept the dwarf's
///   size (var_irondwarf scales the rig about 2x: "il a pas la bonne taille").
pub fn reshape(commands: &mut Commands, character: &mut Character, shape: Option<NpcLook>) {
    let same = match (&character.desc.shape, &shape) {
        (None, None) => true,
        (Some(a), Some(b)) => a.parts == b.parts && a.anim_set == b.anim_set,
        _ => false,
    };
    if same { return; }
    // Only a new body shape needs a fresh skeleton (the Iron Dwarf); a costume
    // is dressed as an equipment change, on the live one.
    let varied = |s: &Option<NpcLook>| s.as_ref().is_some_and(|s| !s.variation.is_empty());
    let new_skeleton = varied(&character.desc.shape) || varied(&shape);
    character.desc.shape = shape;
    if new_skeleton {
        if let Some(skeleton) = character.skeleton.take() {
            commands.entity(skeleton).try_despawn();
        }
        // The old parts were skinned to its joints: left behind, they stood
        // frozen where the change happened ("ma cape ou mon arme reste a la
        // place ou je me suis transforme").
        for part in character.drawn_parts.drain(..) {
            commands.entity(part.entity).try_despawn();
        }
        for part in character.pending_parts.drain(..) {
            commands.entity(part).try_despawn();
        }
        character.generation = character.generation.wrapping_add(1);
    }
    character.built = false;
}

/// Request a new outfit without touching the live skeleton, animation or parts.
/// build_characters and finish_outfits prepare and publish the replacement.
pub fn redress(
    character: &mut Character,
    equipment: Vec<Equipment>,
    armament: i8,
) {
    if character.desc.equipment == equipment && character.desc.armament == armament { return; }
    character.desc.equipment = equipment;
    character.desc.armament = armament;
    character.built = false;
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

/// Every second: animation targets whose player plays nothing (map decor whose
/// animation is never started, finished effects) are detached, so bevy's
/// animate_targets stops visiting them every frame. Characters park and restore
/// their own (drive_animations).
fn prune_idle_targets(
    mut commands: Commands,
    time: Res<Time>,
    mut next: Local<f32>,
    targets: Query<(Entity, &AnimatedBy)>,
    players: Query<Ref<AnimationPlayer>>,
    characters: Query<&CharacterAnim>,
    parents: Query<&ChildOf>,
    names: Query<&Name>,
) {
    *next -= time.delta_secs();
    if *next > 0.0 {
        return;
    }
    *next = 1.0;
    if std::env::var("DSOR_ANIM_DEBUG").is_ok() {
        let total = targets.iter().count();
        let mut by_player: std::collections::HashMap<Entity, usize> = Default::default();
        for (_, AnimatedBy(p)) in &targets {
            *by_player.entry(*p).or_default() += 1;
        }
        let playing = by_player.keys().filter(|p| players.get(**p).is_ok_and(|p| p.playing_animations().next().is_some())).count();
        let parked: usize = characters.iter().map(|a| a.parked.len()).sum();
        let chars = characters.iter().count();
        let active = characters.iter().filter(|a| a.parked.is_empty() && a.player.is_some()).count();
        info!("anim: {total} targets on {} players ({playing} playing); {chars} characters, {active} not parked, {parked} targets parked", by_player.len());
        if let Some((e, AnimatedBy(p))) = targets.iter().next() {
            let chain: Vec<String> = std::iter::once(e).chain(parents.iter_ancestors(e)).take(6).map(|a| format!("{a:?}:{}", names.get(a).map(|n| n.as_str()).unwrap_or("-"))).collect();
            info!("sample target {chain:?}, player {p:?} name {:?}", names.get(*p).map(|n| n.as_str()));
        }
    }
    let character_players: std::collections::HashSet<Entity> = characters.iter().filter_map(|a| a.player).collect();
    for (e, AnimatedBy(player)) in &targets {
        if character_players.contains(player) {
            continue;
        }
        // A player added since the last sweep may not have started yet (an
        // effect's starts once its model is in): left for the next one.
        let idle = players
            .get(*player)
            .map(|p| !p.is_added() && p.playing_animations().next().is_none())
            .unwrap_or(true);
        if idle {
            commands.entity(e).remove::<AnimatedBy>();
        }
    }
}

#[cfg(test)]
mod skeleton_liveness_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    struct Rig { owner: Entity, skeleton: Entity, player: Entity, bone: Entity, mesh: Entity }

    fn fixture() -> (App, Rig) {
        let mut app = App::new();
        app.init_resource::<Assets<AnimationGraph>>()
            .init_resource::<Assets<crate::actors::npc::Variations>>()
            .add_systems(Update, revive_skeletons);
        let world = app.world_mut();
        let owner = world.run_system_once(|mut commands: Commands|
            spawn_character(&mut commands, CharacterDesc::default(), Transform::default())).unwrap();
        let skeleton = world.spawn((SkeletonOf(owner, 0), ChildOf(owner))).id();
        // Keep these siblings so deleting the unnamed player tests its own
        // removal reader rather than incidentally deleting a named bone.
        let player = world.spawn((AnimationPlayer::default(), ChildOf(skeleton))).id();
        let bone = world.spawn((Name::new("hand"), Transform::default(), ChildOf(skeleton))).id();
        let part = world.spawn((PartOf(owner, 0), ChildOf(owner))).id();
        let mesh = world.spawn((SkinnedMesh { joints: vec![bone], ..default() },
            JointNames(vec!["hand".into()]), ChildOf(part))).id();
        {
            let mut character = world.get_mut::<Character>(owner).unwrap();
            character.skeleton = Some(skeleton);
            character.bones = Some(HashMap::from([("hand".into(), bone)]));
        }
        world.get_mut::<CharacterAnim>(owner).unwrap().player = Some(player);
        app.update();
        (app, Rig { owner, skeleton, player, bone, mesh })
    }

    fn new_bone(world: &mut World, skeleton: Entity) -> Entity {
        world.spawn((Name::new("hand"), Transform::default(), ChildOf(skeleton))).id()
    }

    fn assert_binding(app: &App, rig: &Rig, bone: Entity) {
        assert_eq!(app.world().get::<Character>(rig.owner).unwrap().bone("hand"), Some(bone));
        assert_eq!(app.world().get::<SkinnedMesh>(rig.mesh).unwrap().joints, vec![bone]);
    }

    #[test]
    fn removed_shared_bone_rebinds_existing_clothing() {
        let (mut app, rig) = fixture();
        app.world_mut().despawn(rig.bone);
        let replacement = new_bone(app.world_mut(), rig.skeleton);
        app.update();
        assert_binding(&app, &rig, replacement);
        app.update();
        assert_binding(&app, &rig, replacement);
    }

    #[test]
    fn removed_unnamed_animation_player_is_adopted_again() {
        let (mut app, rig) = fixture();
        app.world_mut().despawn(rig.player);
        let replacement = app.world_mut().spawn((AnimationPlayer::default(), ChildOf(rig.skeleton))).id();
        app.update();
        assert_eq!(app.world().get::<CharacterAnim>(rig.owner).unwrap().player, Some(replacement));
        assert!(app.world().get::<AnimationGraphHandle>(replacement).is_some());
        assert_binding(&app, &rig, rig.bone);
    }

    #[test]
    fn an_empty_skeleton_retries_when_replacement_arrives_without_another_removal() {
        let (mut app, rig) = fixture();
        app.world_mut().despawn(rig.player);
        app.world_mut().despawn(rig.bone);
        app.update();
        app.update();
        let replacement = app.world_mut().spawn((AnimationPlayer::default(), ChildOf(rig.skeleton))).id();
        let bone = new_bone(app.world_mut(), rig.skeleton);
        app.update();
        assert_eq!(app.world().get::<CharacterAnim>(rig.owner).unwrap().player, Some(replacement));
        assert_binding(&app, &rig, bone);
    }

    #[test]
    fn a_bone_despawn_after_its_name_was_removed_still_repairs_bindings() {
        let (mut app, rig) = fixture();
        app.world_mut().entity_mut(rig.bone).remove::<Name>();
        app.update();
        app.world_mut().despawn(rig.bone);
        let replacement = new_bone(app.world_mut(), rig.skeleton);
        app.update();
        assert_binding(&app, &rig, replacement);
    }

    #[test]
    fn unrelated_removals_and_owner_despawn_do_not_rebuild_valid_rigs() {
        let (mut app, rig) = fixture();
        let unrelated = app.world_mut().spawn(Name::new("temporary UI node")).id();
        app.world_mut().despawn(unrelated);
        app.update();
        assert_binding(&app, &rig, rig.bone);
        assert_eq!(app.world().get::<CharacterAnim>(rig.owner).unwrap().player, Some(rig.player));
        assert!(app.world().get::<AnimationGraphHandle>(rig.player).is_none(), "no adoption for a healthy rig");
        app.world_mut().despawn(rig.owner);
        app.update();
        app.update();
        assert!(app.world().get_entity(rig.skeleton).is_err());
    }
}

#[cfg(test)]
mod appearance_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn streamed_rig_release_preserves_status_but_drops_old_bindings() {
        let mut world = World::new();
        let player = world.spawn_empty().id();
        let mut anim = CharacterAnim::default();
        anim.state = AnimState::Named("Stunned".into());
        anim.speed = 0.5;
        anim.dead = true;
        anim.bind_model(player, HashMap::new());
        anim.parked.push((player, AnimatedBy(player)));
        anim.release_rig();
        assert!(!anim.is_ready());
        assert!(anim.parked.is_empty() && anim.nodes.is_empty() && anim.current.is_none());
        assert_eq!(anim.state, AnimState::Named("Stunned".into()));
        assert_eq!(anim.speed, 0.5);
        assert!(anim.dead);
        let root = world.spawn((anim, Transform::from_xyz(3.0, 0.0, 7.0))).id();
        world.run_system_once(move |mut commands: Commands| attach_character(&mut commands, root, CharacterDesc::default())).unwrap();
        assert!(world.get::<Character>(root).is_some());
        assert_eq!(world.get::<CharacterAnim>(root).unwrap().state, AnimState::Named("Stunned".into()));
        assert_eq!(world.get::<Transform>(root).unwrap().translation, Vec3::new(3.0, 0.0, 7.0));
    }

    #[test]
    fn compact_equipment_scene_binds_to_shared_skeleton_without_spawning_private_bones() {
        let mut world = World::new();
        let scene = world.spawn_empty().id();
        let joint_root = world.spawn((Name::new("Skeleton_Root"), Transform::default(), ChildOf(scene))).id();
        let hand = world.spawn((Name::new("hand"), Transform::default(), ChildOf(joint_root))).id();
        let unused = world.spawn((Name::new("unused"), ChildOf(joint_root))).id();
        let mesh = world.spawn((Mesh3d::default(), ChildOf(scene), SkinnedMesh { joints: vec![hand], ..default() })).id();
        strip_part_skeleton(scene, &mut world);
        assert!(world.get_entity(joint_root).is_err());
        assert!(world.get_entity(hand).is_err());
        assert!(world.get_entity(unused).is_err());
        assert_eq!(world.get::<JointNames>(mesh).unwrap().0, vec!["hand"]);
        assert!(world.get::<SkinnedMesh>(mesh).unwrap().joints.is_empty());
        let shared_hand = world.spawn(Name::new("hand")).id();
        world.run_system_once(move |mut commands: Commands, skins: Query<&SkinnedMesh>, parents: Query<&ChildOf>| {
            bind(&mut commands, mesh, skins.get(mesh).unwrap(), &["hand".into()],
                &HashMap::from([("hand".into(), shared_hand)]), &parents);
        }).unwrap();
        assert_eq!(world.get::<SkinnedMesh>(mesh).unwrap().joints, vec![shared_hand]);
    }

    #[test]
    fn equipment_with_geometry_under_its_rig_keeps_that_rig() {
        let mut world = World::new();
        let scene = world.spawn_empty().id();
        let joint = world.spawn((Name::new("bone"), ChildOf(scene))).id();
        let mesh = world.spawn((Mesh3d::default(), ChildOf(joint), SkinnedMesh { joints: vec![joint], ..default() })).id();
        strip_part_skeleton(scene, &mut world);
        assert_eq!(world.get::<SkinnedMesh>(mesh).unwrap().joints, vec![joint]);
        assert!(world.get_entity(joint).is_ok());
    }

    #[test]
    fn clothing_coverage_keeps_exposed_skin_and_restores_defaults() {
        // Actual gfxskintags.bin masks: the female torso leaves the upper chest
        // exposed; starter boots leave feet visible, armored boots cover them.
        let mut tags = HashMap::new();
        for gender in ["male", "female"] {
            for (part, mask) in [("head", 1), ("ears", 2), ("neck", 4), ("upperarms", 8),
                ("lowerarms", 0x10), ("hands", 0x20), ("upperbody", 0x40), ("midbody", 0x80),
                ("hips", 0x100), ("upperlegs", 0x200), ("lowerlegs", 0x400), ("feet", 0x800)] {
                tags.insert(format!("warrior_{gender}_{part}"), SkinTag { mask, layer: 0 });
            }
        }
        for (part, mask) in [("warrior_torso_00", 0x100180), ("warrior_torso_00_female", 0x100780),
            ("warrior_torso_01", 0x1003c0), ("warrior_torso_01_female", 0x100780),
            ("warrior_boots_00", 0x20000), ("warrior_boots_01", 0x20800), ("warrior_gloves_01", 0x10020)] {
            tags.insert(part.into(), SkinTag { mask, layer: 1 });
        }
        let exists = |name: &str| tags.contains_key(name);
        for gender in [0, 1] {
            let mut desc = CharacterDesc { gender, ..default() };
            let base = desc.parts(&exists, &tags);
            desc.equipment = vec![Equipment { slot: 2, skin_parts: vec!["warrior_torso_01".into()], colour: Some([1.0, 0.0, 0.0, 1.0]) },
                Equipment { slot: 3, skin_parts: vec!["warrior_gloves_01".into()], ..default() },
                Equipment { slot: 4, skin_parts: vec!["warrior_boots_01".into()], ..default() }];
            let parts = desc.parts(&exists, &tags);
            let has = |name: &str| parts.iter().any(|(p, _)| p == name);
            let prefix = if gender == 0 { "warrior_male" } else { "warrior_female" };
            assert_eq!(has(&format!("{prefix}_upperbody")), gender == 1);
            for part in ["hands", "feet", "midbody", "hips", "upperlegs"] { assert!(!has(&format!("{prefix}_{part}"))); }
            for part in ["head", "neck", "upperarms"] { assert!(has(&format!("{prefix}_{part}"))); }
            assert!(!has("warrior_torso_00") && !has("warrior_torso_00_female"));
            assert!(parts.iter().any(|(_, colour)| *colour == Some([1.0, 0.0, 0.0, 1.0])));
            desc.equipment.clear();
            assert_eq!(desc.parts(&exists, &tags), base);
            desc.equipment = vec![Equipment { slot: 2, skin_parts: vec!["missing_armor".into()], ..default() }];
            assert_eq!(desc.parts(&exists, &tags), base, "unavailable gear must not hide the fallback outfit");
        }
    }

    #[test]
    fn only_visible_higher_layers_occlude_and_equal_layers_coexist() {
        let tags = [("chest", 0x40, 0), ("armor", 0x4040, 1), ("hood", 0x4000, 3),
            ("gloves", 0x20, 1), ("sleeves", 0x20, 1), ("hands", 0x20, 0)]
            .into_iter().map(|(name, mask, layer)| (name.into(), SkinTag { mask, layer })).collect();
        let mut parts = ["chest", "armor", "hood", "gloves", "sleeves", "hands", "unknown_fx"]
            .into_iter().map(|name| (name.to_owned(), None)).collect();
        cull_covered_parts(&mut parts, &tags);
        assert_eq!(parts.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(), ["chest", "hood", "gloves", "sleeves", "unknown_fx"]);
    }

    #[test]
    fn part_catalog_reads_coverage_and_accepts_legacy_assets() {
        let catalog: PartList = serde_json::from_str(r#"{"parts":["body","robe"],"tags":{"body":{"mask":64,"layer":0},"robe":{"mask":1048640,"layer":1}}}"#).unwrap();
        let mut parts = catalog.parts.iter().map(|name| (name.clone(), None)).collect();
        cull_covered_parts(&mut parts, &catalog.tags);
        assert_eq!(parts, vec![("robe".into(), None)]);
        let legacy: PartList = serde_json::from_str(r#"["body","robe"]"#).unwrap();
        assert_eq!(legacy.parts, catalog.parts);
        assert!(legacy.tags.is_empty());
    }

    #[test]
    fn outfit_readiness_waits_for_base_and_extension_textures() {
        let mut images = Assets::<Image>::default();
        let diffuse = images.add(Image::default());
        let normal = images.add(Image::default());
        let dye = Some(images.add(Image::default()));
        let material = StandardMaterial { base_color_texture: Some(diffuse.clone()), normal_map_texture: Some(normal.clone()), ..default() };
        assert!(!material_textures_ready(&material, &[&dye], |id| id != diffuse.id().untyped()));
        assert!(!material_textures_ready(&material, &[&dye], |id| id != normal.id().untyped()));
        assert!(!material_textures_ready(&material, &[&dye], |id| id != dye.as_ref().unwrap().id().untyped()));
        assert!(material_textures_ready(&material.clone(), &[&dye, &None], |id| images.contains(id.typed::<Image>())));
    }

    #[test]
    fn weapon_blends_start_at_the_current_pose_and_survive_rapid_reversal() {
        use bevy::animation::{AnimationPlugin, AnimationTargetId, animated_field};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), AnimationPlugin));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(1.0 / 60.0)));
        let target = AnimationTargetId::from_name(&Name::new("arm"));
        let handles: Vec<_> = [0.0, 1.2].into_iter().map(|angle| {
            let mut clip = AnimationClip::default();
            clip.add_curve_to_target(target, AnimatableCurve::new(animated_field!(Transform::rotation),
                UnevenSampleAutoCurve::new([(0.0, Quat::from_rotation_x(angle)), (2.0, Quat::from_rotation_x(angle))]).unwrap()));
            app.world_mut().resource_mut::<Assets<AnimationClip>>().add(clip)
        }).collect();
        let (graph, nodes) = AnimationGraph::from_clips(handles);
        let graph = app.world_mut().resource_mut::<Assets<AnimationGraph>>().add(graph);
        let player = app.world_mut().spawn((AnimationPlayer::default(), AnimationGraphHandle(graph))).id();
        let bone = app.world_mut().spawn((target, AnimatedBy(player), Transform::default())).id();
        let mut anim = CharacterAnim::default();
        anim.transition(&mut app.world_mut().get_mut::<AnimationPlayer>(player).unwrap(), nodes[0], true);
        app.update(); app.update();
        anim.set_changed = true;
        anim.transition(&mut app.world_mut().get_mut::<AnimationPlayer>(player).unwrap(), nodes[1], true);
        app.update();
        assert!(app.world().get::<Transform>(bone).unwrap().rotation.angle_between(Quat::IDENTITY) < 1e-5, "no first-frame posture jump");
        for frame in 0..20 {
            if frame == 4 {
                let before = app.world().get::<Transform>(bone).unwrap().rotation;
                anim.set_changed = true;
                anim.transition(&mut app.world_mut().get_mut::<AnimationPlayer>(player).unwrap(), nodes[0], true);
                app.update();
                assert!(before.angle_between(app.world().get::<Transform>(bone).unwrap().rotation) < 0.001,
                    "rapidly switching back must start from the mixed pose");
            }
            let before = app.world().get::<Transform>(bone).unwrap().rotation;
            anim.advance_blend(&mut app.world_mut().get_mut::<AnimationPlayer>(player).unwrap(), 1.0 / 60.0);
            app.update();
            let after = app.world().get::<Transform>(bone).unwrap().rotation;
            assert!(before.angle_between(after) < 0.15, "pose must change gradually");
            let total: f32 = app.world().get::<AnimationPlayer>(player).unwrap().playing_animations().map(|(_, a)| a.weight()).sum();
            assert!((total - 1.0).abs() < 1e-5);
        }
        assert!(anim.fading.is_empty());
        assert!(app.world().get::<Transform>(bone).unwrap().rotation.angle_between(Quat::IDENTITY) < 0.001);
        assert!(app.world().get::<AnimationPlayer>(player).unwrap().is_playing_animation(nodes[0]));
        assert!(!app.world().get::<AnimationPlayer>(player).unwrap().is_playing_animation(nodes[1]));
    }

    #[test]
    fn changing_weapon_keeps_gait_phase_and_does_not_restart_a_shared_clip() {
        let (_, nodes) = AnimationGraph::from_clips([Handle::default(), Handle::default()]);
        let mut player = AnimationPlayer::default();
        let mut anim = CharacterAnim { state: AnimState::Run, ..default() };
        anim.durations = [(nodes[0], 2.0), (nodes[1], 4.0)].into_iter().collect();
        anim.transition(&mut player, nodes[0], true);
        player.animation_mut(nodes[0]).unwrap().set_seek_time(1.5);
        anim.set_changed = true;
        anim.transition(&mut player, nodes[1], true);
        assert_eq!(player.animation(nodes[1]).unwrap().seek_time(), 3.0);
        assert_eq!(player.animation(nodes[1]).unwrap().weight(), 0.0);
        anim.advance_blend(&mut player, EQUIPMENT_BLEND.as_secs_f32());
        anim.set_changed = true;
        anim.transition(&mut player, nodes[1], true);
        assert_eq!(player.animation(nodes[1]).unwrap().seek_time(), 3.0);
        // One-shot progress also survives a set change; no old events are replayed.
        anim.state = AnimState::Death;
        anim.applied = Some((AnimState::Death, 1.0));
        anim.dead = true;
        anim.set_changed = true;
        anim.transition(&mut player, nodes[0], false);
        assert_eq!(player.animation(nodes[0]).unwrap().seek_time(), 1.5);
        assert!(anim.dead);
    }

    #[test]
    fn outfit_swap_keeps_skeleton_and_common_parts_until_replacements_are_ready() {
        let mut world = World::new();
        let root = world.run_system_once(|mut commands: Commands| spawn_character(&mut commands, CharacterDesc::default(), Transform::default())).unwrap();
        let skeleton = world.spawn((AnimationPlayer::default(), ChildOf(root))).id();
        let body = world.spawn((ChildOf(root), Visibility::Inherited)).id();
        let old = world.spawn((ChildOf(root), Visibility::Inherited)).id();
        let new = world.spawn((ChildOf(root), Visibility::Hidden)).id();
        let body_part = OutfitPart { entity: body, name: "body".into(), colour: None };
        let mut character = world.get_mut::<Character>(root).unwrap();
        character.skeleton = Some(skeleton);
        character.bones = Some([("bone".into(), skeleton)].into_iter().collect());
        character.drawn_parts = vec![body_part.clone(), OutfitPart { entity: old, name: "shield".into(), colour: None }];
        redress(&mut character, vec![], 5);
        assert_eq!(character.skeleton, Some(skeleton));
        assert_eq!(character.bone("bone"), Some(skeleton));
        character.pending_outfit = Some(PendingOutfit { parts: vec![body_part, OutfitPart { entity: new, name: "two_hand".into(), colour: None }],
            introduced: vec![new], animation_set: "warrior_2h_weapon".into() });
        assert_eq!(*world.get::<Visibility>(old).unwrap(), Visibility::Inherited);
        assert_eq!(*world.get::<Visibility>(new).unwrap(), Visibility::Hidden);
        world.run_system_once(|mut commands: Commands, mut characters: Query<&mut Character>| {
            publish_outfit(&mut commands, &mut characters.single_mut().unwrap());
        }).unwrap();
        assert!(world.get_entity(skeleton).is_ok());
        assert!(world.get_entity(body).is_ok());
        assert!(world.get_entity(old).is_err());
        assert_eq!(*world.get::<Visibility>(new).unwrap(), Visibility::Inherited);
        assert_eq!(world.get::<Character>(root).unwrap().displayed_set, "warrior_2h_weapon");
    }

    #[test]
    fn hidden_selection_preview_poses_before_reveal_while_hidden_world_actors_stay_culled() {
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        let (_, nodes) = AnimationGraph::from_clips([Handle::default()]);
        let mut roots = Vec::new();
        for preview in [false, true] {
            let player = world.spawn(AnimationPlayer::default()).id();
            let anim = CharacterAnim { player: Some(player), state: AnimState::Named("CharSelectIdle".into()),
                nodes: [("CharSelectIdle".into(), (nodes[0], true))].into_iter().collect(), ..default() };
            let root = world.spawn((anim, GlobalTransform::default(), InheritedVisibility::HIDDEN)).id();
            if preview { world.entity_mut(root).insert(crate::screens::selection::Preview); }
            roots.push(root);
        }
        world.run_system_once(drive_animations).unwrap();
        assert!(!world.get::<CharacterAnim>(roots[0]).unwrap().is_animating());
        assert!(world.get::<CharacterAnim>(roots[1]).unwrap().is_animating(),
            "outfit readiness waits for a pose, so hidden previews must animate before publication");
    }

    #[test]
    fn streamed_npc_is_posed_on_first_visible_frame_and_after_rig_reentry() {
        use bevy::animation::{AnimationPlugin, AnimationTargetId, animated_field};
        use crate::actors::npc::placement_tests::{finish_pose_test, mark_resident_for_pose_test};
        // Neither relative Update order may publish just because drive called
        // play(): the graph's threaded cache and a sampled pose must exist first.
        for finish_first in [true, false] {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, AssetPlugin::default(), AnimationPlugin, crate::rendering::anim_cull::AnimCullPlugin))
                .init_resource::<Assets<crate::world::map::MapManifest>>();
            app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(1.0 / 60.0)));
            if finish_first { app.add_systems(Update, (finish_pose_test, drive_animations).chain()); }
            else { app.add_systems(Update, (drive_animations, finish_pose_test).chain()); }
            let camera = Transform::from_xyz(0.0, 10.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z);
            app.world_mut().spawn((Camera3d::default(), GlobalTransform::from(camera)));
            let target = AnimationTargetId::from_name(&Name::new("npc bone"));
            let pose = Vec3::new(0.0, 2.0, 0.0);
            let mut clip = AnimationClip::default();
            clip.add_curve_to_target(target, AnimatableCurve::new(animated_field!(Transform::translation),
                UnevenSampleAutoCurve::new([(0.0, pose), (2.0, pose)]).unwrap()));
            let clip = app.world_mut().resource_mut::<Assets<AnimationClip>>().add(clip);
            let (graph, node) = AnimationGraph::from_clip(clip);
            let graph = app.world_mut().resource_mut::<Assets<AnimationGraph>>().add(graph);
            let root = app.world_mut().run_system_once(|mut commands: Commands|
                spawn_character(&mut commands, CharacterDesc::default(), Transform::default())).unwrap();
            let mut old_player = None;
            for _ in 0..2 {
                if let Some(old) = old_player { app.world_mut().despawn(old); }
                let player = app.world_mut().spawn((AnimationPlayer::default(), AnimationGraphHandle(graph.clone()), ChildOf(root))).id();
                let bone = app.world_mut().spawn((target, AnimatedBy(player), Transform::default(), ChildOf(player))).id();
                old_player = Some(player);
                let mut anim = app.world_mut().get_mut::<CharacterAnim>(root).unwrap();
                anim.release_rig();
                anim.bind_model(player, [("Idle".into(), (node, true))].into_iter().collect());
                let mut character = app.world_mut().get_mut::<Character>(root).unwrap();
                character.built = true;
                character.bones = Some([("npc bone".into(), bone)].into_iter().collect());
                character.drawn_parts = vec![OutfitPart { entity: bone, name: "body".into(), colour: None }];
                app.world_mut().entity_mut(root).insert((Visibility::Hidden, InheritedVisibility::HIDDEN));
                mark_resident_for_pose_test(app.world_mut(), root);
                finish_pose_test(app.world_mut());
                assert!(app.world().get::<crate::actors::npc::NpcVisualPending>(root).is_some(),
                    "a loaded skeleton without a sampled pose must stay hidden");
                for frame in 0..3 {
                    app.update();
                    if app.world().get::<crate::actors::npc::NpcVisualPending>(root).is_none() {
                        assert_eq!(app.world().get::<Transform>(bone).unwrap().translation, pose,
                            "publication must follow actual curve evaluation, not just play()");
                        break;
                    }
                    assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden);
                    if frame >= 1 {
                        assert_eq!(app.world().get::<Transform>(bone).unwrap().translation, pose,
                            "the hidden warm-up must evaluate once the graph cache is ready");
                    }
                }
                assert!(app.world().get::<crate::actors::npc::NpcVisualPending>(root).is_none());
                assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Inherited);
                assert_eq!(app.world().get::<Transform>(bone).unwrap().translation, pose);
            }
        }
    }

    #[test]
    fn view_culling_parks_bones_without_restarting_clips_or_delaying_lod_reentry() {
        let mut app = App::new();
        app.init_resource::<Time>().init_resource::<Assets<AnimationGraph>>()
            .init_resource::<Assets<AnimationClip>>().add_plugins(crate::rendering::anim_cull::AnimCullPlugin)
            .add_systems(Update, drive_animations);
        let (_, nodes) = AnimationGraph::from_clips([Handle::default()]);
        let player = app.world_mut().spawn(AnimationPlayer::default()).id();
        let mut anim = CharacterAnim { player: Some(player),
            nodes: [("Idle".into(), (nodes[0], true))].into_iter().collect(), ..default() };
        anim.transition(&mut app.world_mut().get_mut::<AnimationPlayer>(player).unwrap(), nodes[0], true);
        let root = app.world_mut().spawn((anim, AnimLod::new(3, 0), GlobalTransform::default(), InheritedVisibility::VISIBLE)).id();
        let bone = app.world_mut().spawn(AnimatedBy(player)).id();
        let mesh = app.world_mut().spawn((Mesh3d::default(), ViewVisibility::HIDDEN, ChildOf(root))).id();
        for _ in 0..5 {
            app.world_mut().resource_mut::<Time>().advance_by(Duration::from_millis(100));
            app.update();
        }
        assert!(app.world().get::<AnimatedBy>(bone).is_none());
        assert!(app.world().get::<AnimationPlayer>(player).unwrap().is_playing_animation(nodes[0]),
            "offscreen clip time must keep advancing while bone sampling is parked");
        app.world_mut().get_mut::<AnimationPlayer>(player).unwrap().animation_mut(nodes[0]).unwrap().set_seek_time(3.5);
        app.world_mut().entity_mut(mesh).insert(ViewVisibility::VISIBLE);
        app.update(); // Complete the next visibility pass.
        app.update(); // Consume visibility, even between the NPC's LOD frames.
        assert_eq!(app.world().get::<AnimatedBy>(bone).unwrap().0, player);
        assert_eq!(app.world().get::<AnimationPlayer>(player).unwrap().animation(nodes[0]).unwrap().seek_time(), 3.5);
        assert!(app.world().get::<CharacterAnim>(root).unwrap().is_animating());
    }

    #[test]
    fn parts_use_gender_variants_deduplicate_two_hand_meshes_and_restore_base_clothes() {
        let available = ["mage_female_head", "mage_torso_00_female", "mage_boots_00", "robe_female", "staff"];
        let exists = |s: &str| available.contains(&s);
        let mut desc = CharacterDesc { class: 1, gender: 1, ..default() };
        desc.equipment = vec![Equipment { slot: 2, skin_parts: vec!["missing_torso".into()], ..default() }];
        assert!(desc.parts(&exists, &default()).iter().any(|(p, _)| p == "mage_torso_00_female"));
        desc.equipment = vec![Equipment { slot: 2, skin_parts: vec!["robe".into()], colour: Some([1.0, 0.0, 0.0, 1.0]) },
            Equipment { slot: 5, skin_parts: vec!["staff".into()], ..default() },
            Equipment { slot: 6, skin_parts: vec!["staff".into()], ..default() }];
        let parts = desc.parts(&exists, &default());
        assert!(!parts.iter().any(|(p, _)| p == "mage_torso_00_female"));
        assert_eq!(parts.iter().filter(|(p, _)| p == "staff").count(), 1);
        assert!(parts.contains(&("robe_female".into(), Some([1.0, 0.0, 0.0, 1.0]))));
        assert!(parts.contains(&("mage_female_head".into(), None)));
        desc.class = 2;
        desc.armament = 3;
        assert_eq!(desc.animation_set(), "ranger_female_small_weapon");
        desc.armament = 5;
        assert_eq!(desc.animation_set(), "ranger_female_large_weapon");
    }

    #[test]
    fn dyes_are_per_instance_mask_parameters_and_retry_delayed_materials() {
        use crate::rendering::surfaces::{NebulaMaterial, NebulaSurface, Kind, SurfaceParams};
        let mut app = App::new();
        app.init_resource::<Assets<NebulaMaterial>>().add_systems(Update, apply_equipment_dyes);
        let original = app.world_mut().resource_mut::<Assets<NebulaMaterial>>().add(NebulaMaterial {
            base: default(), extension: NebulaSurface { params: SurfaceParams { p0: Vec4::new(Kind::Character as u32 as f32, 0.0, 0.0, 0.0), ..default() }, cube: None, layer: None, mask: None },
        });
        let plain = app.world_mut().spawn(MeshMaterial3d(original.clone())).id();
        let root = app.world_mut().spawn(EquipmentDye([0.5, 0.0, 0.0, 0.75])).id();
        let dyed = app.world_mut().spawn((MeshMaterial3d(original.clone()), ChildOf(root))).id();
        app.update();
        let handle = app.world().get::<MeshMaterial3d<NebulaMaterial>>(dyed).unwrap().0.clone();
        assert_ne!(handle, original);
        assert_eq!(app.world().get::<MeshMaterial3d<NebulaMaterial>>(plain).unwrap().0, original);
        let materials = app.world().resource::<Assets<NebulaMaterial>>();
        assert_eq!(materials.get(&original).unwrap().extension.params.p1, Vec4::ZERO);
        assert_eq!(materials.get(&handle).unwrap().extension.params.p1, Vec4::new(0.5, 0.0, 0.0, 0.75));
        app.update();
        assert_eq!(app.world().get::<MeshMaterial3d<NebulaMaterial>>(dyed).unwrap().0, handle, "no fresh material each frame");
        // A later scene instance gets its own material when the child arrives.
        let late = app.world_mut().spawn((MeshMaterial3d(original.clone()), ChildOf(root))).id();
        app.update();
        assert_ne!(app.world().get::<MeshMaterial3d<NebulaMaterial>>(late).unwrap().0, original);
    }
}
