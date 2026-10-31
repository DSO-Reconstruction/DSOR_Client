//! Items on the ground: what the server drops (NewItemCommand) until it takes it
//! back (DiscardItemCommand).
//!
//! An item lies as its template's drop model (`_Template_Item.Graphics`, e.g.
//! `dropitems/dwarf_drop_helmet_01`), playing that model's own looping clip (its
//! slow turn), with its rarity's glow: the sequence `drop_<rarity>` (one looping
//! `dropitems/glow_<rarity>` effect), or the template's DropSequenceOverride.
//! Holding Ctrl names every item, hovering names the one under the cursor, its
//! text in its rarity's colour (white for a common one) with no box behind it, as
//! the original shows it (the user's release261_192 shot; ui/cmdmodeitemlabel.bxml
//! has a BGLabel frame round a TextDescription, not drawn there).
//! EVIDENCE: the drop models' `<name>-loop` clips; skills/sequences.json
//!   drop_common .. drop_mythic; the rarity is ItemInfo field_08 (`kind`), indexing
//!   Common, Uncommon, Magic, Rare, Epic, Unique, Set, Mythic (DSOR_Server
//!   protocol/world_loot_protocol.py EQUIPMENT_RARITIES).
//! FAILURE (2026-10-24): the name sat in a dark box, and only the small model
//!   reacted to the cursor ("you have to press very specific"; the original takes
//!   the whole item).
//! UNVERIFIED: the original's pick area (a sphere round the item here); its glow
//!   reads larger than ours.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::gltf::Gltf;
use bevy::prelude::*;
use serde::Deserialize;

use crate::net::Net;
use crate::nav::{CurrentNav, NavMesh};
use crate::skills::{SequenceTable, SkillData};

/// The rarities in ItemInfo's order.
const RARITIES: [&str; 8] = ["common", "uncommon", "magic", "rare", "epic", "unique", "set", "mythic"];

/// A rarity's text colour (index as RARITIES): the item name's, on the Ctrl label
/// and in tooltips.
/// EVIDENCE: 2018 client FUN_00782b2f(out, rarity), float constants: Common
///   0xE6B450 (0.812, 0.812, 0.812), Uncommon 0xE6B430 (0.157, 0.929, 0), Magic
///   0xE6B470 (0, 0.471, 1), Rare 0xE6B460 (1, 0.09, 1), Epic 0xE6B420 (1, 0.423,
///   0), anything above 0xE6B440 (1, 0.859, 0.067): the 2018 client knows six
///   rarities, so Set and Mythic take the last. UI::CommandModeWidget::UpdateLabel
///   (0x64ADFB) colours TextDescription through FUN_007829E5, which keeps white
///   for some item types (not resolved; the rarity colour is used for all here).
const RARITY_TEXT: [Color; 8] = [
    Color::srgb(0.812, 0.812, 0.812),
    Color::srgb(0.157, 0.929, 0.0),
    Color::srgb(0.0, 0.471, 1.0),
    Color::srgb(1.0, 0.09, 1.0),
    Color::srgb(1.0, 0.423, 0.0),
    Color::srgb(1.0, 0.859, 0.067),
    Color::srgb(1.0, 0.859, 0.067),
    Color::srgb(1.0, 0.859, 0.067),
];

/// What a label floats over the item's origin, world units.
const LABEL_HEIGHT: f32 = 0.9;

/// One item template, as interface/inventory.items.json has it (the fields used).
#[derive(Deserialize, Debug, Clone, Default)]
struct ItemTemplate {
    #[serde(default)]
    name: String,
    #[serde(default)]
    graphics: String,
    #[serde(default)]
    drop_sequence: String,
}

/// A rolled appearance's name (crate::ui::controls::items::Catalog::name).
#[derive(Deserialize, Debug, Clone, Default)]
struct AppearanceName {
    #[serde(default)]
    name: String,
}

#[derive(Asset, TypePath, Deserialize, Debug)]
struct ItemCatalog {
    items: HashMap<String, ItemTemplate>,
    #[serde(default)]
    appearances: HashMap<String, AppearanceName>,
}

#[derive(Default, TypePath)]
struct ItemCatalogLoader;

impl AssetLoader for ItemCatalogLoader {
    type Asset = ItemCatalog;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<ItemCatalog, Self::Error> {
        let mut b = Vec::new();
        r.read_to_end(&mut b).await?;
        serde_json::from_slice(&b).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] {
        &["items.json"]
    }
}

#[derive(Resource)]
struct Catalog(Handle<ItemCatalog>);

/// An item on the ground: its server id, name and rarity.
#[derive(Component)]
pub struct GroundItem {
    pub id: u32,
    name: String,
    rarity: usize,
    /// Its model's scene root (not its glow effects).
    model: Entity,
}

/// The ground item under the cursor: (its entity, its model's scene root). It
/// shows its name, and its model is outlined in lime (crate::rendering::highlight).
/// A white outline was wrong ("y'a pas d'aura blanche"); the original's is lime.
#[derive(Resource, Default)]
pub struct ItemHover(pub Option<(Entity, Entity)>);

/// The item a click asked for: walked to, picked up once within reach.
#[derive(Resource, Default)]
struct PendingPickup(Option<Entity>);

/// Within this flat distance the client sends PickupItem.
/// SOURCE: DSOR_Server services/world.py _handle_pickup_item refuses beyond 4.0.
const PICKUP_REACH: f32 = 2.5;
/// The pickable sphere around an item: its centre this high over the ground.
const PICK_HEIGHT: f32 = 0.4;
const PICK_RADIUS: f32 = 1.0;

/// Where a hovered item's name starts, from the cursor's tip (logical pixels).
const CURSOR_LABEL: Vec2 = Vec2::new(15.0, 34.0);

/// Its model, and the animation player its clip was started on.
#[derive(Component)]
struct ItemModel(Handle<Gltf>, Option<Entity>);

/// The Ctrl label of an item.
#[derive(Component)]
struct ItemLabel(Entity);

pub struct DropsPlugin;

impl Plugin for DropsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<ItemCatalog>()
            .register_asset_loader(ItemCatalogLoader)
            .add_systems(Startup, |mut commands: Commands, assets: Res<AssetServer>| {
                commands.insert_resource(Catalog(assets.load("interface/inventory.items.json")));
            })
            .init_resource::<ItemHover>()
            .init_resource::<PendingPickup>()
            .add_systems(Update, (spawn_drops, animate_drops, hover_items, pick_up, labels).chain());
    }
}

fn rarity_name(rarity: usize) -> &'static str {
    RARITIES.get(rarity).copied().unwrap_or("common")
}

#[allow(clippy::too_many_arguments)]
fn spawn_drops(
    mut commands: Commands,
    net: Option<NonSendMut<Net>>,
    assets: Res<AssetServer>,
    catalog: Res<Catalog>,
    catalogs: Res<Assets<ItemCatalog>>,
    skills: Res<SkillData>,
    seqs: Res<Assets<SequenceTable>>,
    nav: Option<Res<CurrentNav>>,
    navmeshes: Res<Assets<NavMesh>>,
    items: Query<(Entity, &GroundItem)>,
    current: Option<Res<crate::map::CurrentMap>>,
) {
    let Some(mut net) = net else { return };
    for id in std::mem::take(&mut net.items_gone) {
        for (e, item) in &items {
            if item.id == id {
                commands.entity(e).despawn();
            }
        }
    }
    if net.item_drops.is_empty() {
        return;
    }
    // Both tables first: the drops wait in Net meanwhile.
    let (Some(table), Some(sequences)) = (catalogs.get(&catalog.0), seqs.get(skills.sequences())) else { return };
    let mesh = nav.as_ref().and_then(|n| navmeshes.get(&n.0));
    let root = current.as_ref().and_then(|c| c.root);
    for item in std::mem::take(&mut net.item_drops) {
        // A drop again (the server re-announces on re-entering vicinity): replaced.
        for (e, old) in &items {
            if old.id == item.id {
                commands.entity(e).despawn();
            }
        }
        let template = table.items.get(&item.template).cloned().unwrap_or_default();
        let rarity = (item.kind as usize).min(RARITIES.len() - 1);
        let mut at = Vec3::from(item.position);
        if let Some(h) = mesh.and_then(|m| m.ground(at.x, at.z, at.y, 3.0)) {
            at.y = h;
        }
        // The model: Graphics (every template has one; a bag if unknown).
        let graphics = if template.graphics.is_empty() { "dropitems/all_drop_bag_01".to_owned() } else { template.graphics.clone() };
        let path = crate::map::model_path(&graphics);
        let gltf: Handle<Gltf> = assets.load(path.clone());
        // A rolled piece is named by its appearance, as in the bags.
        let name = table.appearances.get(&item.appearance).map(|a| a.name.clone()).filter(|n| !n.is_empty())
            .or_else(|| (!template.name.is_empty()).then(|| template.name.clone()))
            .unwrap_or_else(|| item.template.clone());
        debug!("item {} {} ({}) at {at:?}", item.id, item.template, rarity_name(rarity));
        let e = commands
            .spawn((
                Name::new(format!("item {}", item.template)),
                ItemModel(gltf, None),
                Transform::from_translation(at),
                Visibility::default(),
            ))
            .id();
        if let Some(root) = root {
            commands.entity(e).insert(ChildOf(root));
        }
        let model = commands.spawn((
            bevy::world_serialization::WorldAssetRoot(assets.load(bevy::gltf::GltfAssetLabel::Scene(0).from_asset(path))),
            ChildOf(e),
            Transform::default(),
            Visibility::default(),
        )).id();
        commands.entity(e).insert(GroundItem { id: item.id, name, rarity, model });
        // Its glow: the template's own sequence, else its rarity's.
        let glow = template.drop_sequence.rsplit('/').next().filter(|s| !s.is_empty()).map(str::to_owned)
            .unwrap_or_else(|| format!("drop_{}", rarity_name(rarity)));
        if let Some(seq) = sequences.0.get(&glow) {
            crate::skills::play(&mut commands, seq.clone(), None, e, true);
        }
    }
}

/// Each model plays its own looping clip once its scene is in (its turn).
fn animate_drops(
    mut commands: Commands,
    mut drops: Query<(Entity, &mut ItemModel, &GroundItem)>,
    children: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    playing: Query<&AnimationPlayer>,
    gltfs: Res<Assets<Gltf>>,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut clips: ResMut<Assets<AnimationClip>>,
    targets: Query<(), With<bevy::animation::AnimationTargetId>>,
    nodes: Query<(&bevy::animation::AnimationTargetId, &bevy::gltf::GltfExtras)>,
) {
    for (entity, mut model, item) in &mut drops {
        // Started, and its player still plays. A scene built again under it (its
        // model loaded again after the last item of it went) has a fresh player
        // with nothing playing: its clip starts again there, as an effect's does
        // (crate::skills::start_fx_animations).
        if model.1.is_some_and(|p| playing.get(p).is_ok_and(|a| a.playing_animations().next().is_some())) {
            continue;
        }
        if assets.load_state(&model.0).is_failed() {
            commands.entity(entity).remove::<ItemModel>();
            continue;
        }
        let Some(gltf) = gltfs.get(&model.0) else { continue };
        let Some(clip) = gltf.animations.first() else {
            commands.entity(entity).remove::<ItemModel>();
            continue;
        };
        // Within its model's scene only: its glow (a sequence's effect, also
        // under the item) has a player of its own.
        // FAILURE (2026-10-28): when the glow's scene came in first, the item's
        //   clip went on the glow's player and the two clips took turns on it:
        //   a Chainmail Shirt lay still and without its glow (the user's clip
        //   20261028-1812-11), the Leather Gauntlets beside it turned and glowed.
        let Some(player) = children.iter_descendants(item.model).find(|e| players.contains(*e)) else { continue };
        model.1 = Some(player);
        let mut anim = AnimationPlayer::default();
        let loops = crate::skills::clip_loops(item.model, &children, &nodes);
        let graph = crate::skills::clip_graph(clip, &mut clips, &mut anim, 1.0, &loops)
            .unwrap_or_else(|| crate::skills::looping_graph(clip, &mut clips, &mut anim, 1.0));
        commands.entity(player).try_insert((AnimationGraphHandle(graphs.add(graph)), anim));
        crate::skills::hook_targets(&mut commands, item.model, player, &children, &targets);
    }
}

/// While Ctrl is held: each item's name over it, in its rarity's colour.
/// The item under the cursor, unless the UI or a monster has it.
fn hover_items(
    ui: Res<crate::ui_input::UiInput>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::highlight::HighlightCamera>)>,
    monster: Option<Res<crate::monsters::Hovered>>,
    items: Query<(Entity, &GroundItem, &GlobalTransform, &InheritedVisibility)>,
    labels: Query<(&ItemLabel, &Node, &ComputedNode, &Visibility)>,
    mut hover: ResMut<ItemHover>,
) {
    let cursor = (!ui.over_ui && monster.is_none_or(|m| m.0.is_none()))
        .then(|| windows.single().ok().and_then(|w| w.cursor_position()))
        .flatten();
    // A name shown (Ctrl held, or the item hovered) is its item too: Ctrl and a
    // click on the name picks it up, and the cursor moving onto it keeps it.
    let named = cursor.and_then(|c| {
        labels.iter().find_map(|(of, node, size, vis)| {
            let (Val::Px(x), Val::Px(y)) = (node.left, node.top) else { return None };
            let s = size.size() * size.inverse_scale_factor();
            (*vis != Visibility::Hidden && c.x >= x && c.x <= x + s.x && c.y >= y && c.y <= y + s.y).then_some(of.0)
        })
    });
    if let Some(e) = named {
        let now = items.get(e).ok().map(|(e, item, _, _)| (e, item.model));
        if hover.0 != now {
            hover.0 = now;
        }
        return;
    }
    let ray = cursor.zip(cameras.single().ok()).and_then(|(c, (cam, at))| cam.viewport_to_world(at, c).ok());
    let now = ray.and_then(|ray| {
        let mut best: Option<(f32, Entity, Entity)> = None;
        for (e, item, at, shown) in &items {
            if !shown.get() {
                continue;
            }
            let centre = at.translation() + Vec3::Y * PICK_HEIGHT;
            let t = (centre - ray.origin).dot(*ray.direction);
            let off = ray.get_point(t).distance(centre);
            if t <= 0.0 || off > PICK_RADIUS {
                continue;
            }
            // The item nearest the cursor, not the camera: in a pile their
            // pick spheres overlap.
            // FAILURE (2026-10-28): after a boss, the cursor on a Sledgehammer
            //   outlined and named a healing sphere beside it (clip
            //   20261028-1832-42).
            if best.is_none_or(|(bo, _, _)| off < bo) {
                best = Some((off, e, item.model));
            }
        }
        best.map(|(_, e, m)| (e, m))
    });
    if hover.0 != now {
        hover.0 = now;
    }
}

/// A click on an item walks to it; within reach, PickupItem (the server answers
/// with the item's DiscardItem and the inventory or currency it went to).
#[allow(clippy::too_many_arguments)]
fn pick_up(
    ui: Res<crate::ui_input::UiInput>,
    buttons: Res<ButtonInput<MouseButton>>,
    hover: Res<ItemHover>,
    mut pending: ResMut<PendingPickup>,
    items: Query<(&GroundItem, &GlobalTransform)>,
    mut players: Query<(&Transform, &mut crate::net::LocalPlayer)>,
    net: Option<NonSendMut<Net>>,
) {
    if !ui.blocks(MouseButton::Left) && buttons.just_pressed(MouseButton::Left) {
        pending.0 = hover.0.map(|(e, _)| e);
    }
    let Some(e) = pending.0 else { return };
    let (Ok((item, at)), Some(mut net)) = (items.get(e), net) else {
        pending.0 = None;
        return;
    };
    let Ok((tf, mut player)) = players.single_mut() else { return };
    let goal = at.translation();
    if Vec2::new(goal.x - tf.translation.x, goal.z - tf.translation.z).length() > PICKUP_REACH {
        player.target = Some(goal);
        return;
    }
    pending.0 = None;
    player.target = None;
    net.send(&dsor_proto::commands::ClientCommand::PickupItem(dsor_proto::commands::actors::PickupItem { item: item.id }));
}

#[allow(clippy::too_many_arguments)]
fn labels(
    hover: Res<ItemHover>,
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    ui: Res<crate::ui_input::UiInput>,
    assets: Res<AssetServer>,
    items: Query<(Entity, &GroundItem, &GlobalTransform, &InheritedVisibility)>,
    mut labels: Query<(Entity, &ItemLabel, &mut Node, &mut Visibility, &ComputedNode)>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::highlight::HighlightCamera>)>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let held = !ui.typing && (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight));
    let mut labelled: HashMap<Entity, Entity> = HashMap::new();
    for (label, of, _, _, _) in &labels {
        if items.contains(of.0) {
            labelled.insert(of.0, label);
        } else {
            commands.entity(label).despawn();
        }
    }
    let hovered = hover.0.map(|(e, _)| e);
    for (e, item, _, _) in &items {
        if held || hovered == Some(e) {
            if !labelled.contains_key(&e) {
                commands.spawn((
                    ItemLabel(e),
                    Node {
                        position_type: PositionType::Absolute,
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                        ..default()
                    },
                    Text::new(item.name.clone()),
                    TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK },
                    TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(22.0), ..default() },
                    TextColor(RARITY_TEXT[item.rarity.min(RARITY_TEXT.len() - 1)]),
                    Pickable::IGNORE,
                    Visibility::Hidden,
                ));
            }
        }
    }
    let cam = cameras.single().ok();
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());
    for (_, of, mut node, mut visibility, size) in &mut labels {
        // The hovered item's name (Ctrl not held) beside the cursor, below and to
        // its right, as a tooltip: it no longer covers the item and its glow.
        // EVIDENCE: the user's original-game clip (release261_192, crypt,
        //   2026-10-23 22:16 UTC): "Scut cavaleresc" and "Sabie de bastard" start
        //   just past the arrow's tip, a line below it.
        // MEASURED: the user's clip 20261028-1812-11 (original, crypt, 3830 px
        //   wide at 150 %): "Bronze Ring" starts 23 px right of the arrow's tip
        //   and 52 px below it, capitals 22 px tall (ours 14 at 14 px).
        if !held && hovered == Some(of.0) {
            if let Some(c) = cursor.filter(|_| items.get(of.0).is_ok_and(|(_, _, _, shown)| shown.get())) {
                node.left = Val::Px(c.x + CURSOR_LABEL.x);
                node.top = Val::Px(c.y + CURSOR_LABEL.y);
                if *visibility != Visibility::Inherited {
                    *visibility = Visibility::Inherited;
                }
                continue;
            }
        }
        let screen = items.get(of.0).ok().filter(|(e, _, _, shown)| (held || hovered == Some(*e)) && shown.get()).and_then(|(_, _, at, _)| {
            cam.and_then(|(c, ct)| c.world_to_viewport(ct, at.translation() + Vec3::Y * LABEL_HEIGHT).ok())
        });
        match screen {
            Some(p) => {
                // Centred over the item (its laid-out size, logical pixels).
                let half = size.size() * size.inverse_scale_factor() * 0.5;
                node.left = Val::Px(p.x - half.x);
                node.top = Val::Px(p.y - half.y * 2.0);
                if *visibility != Visibility::Inherited {
                    *visibility = Visibility::Inherited;
                }
            }
            None => {
                if *visibility != Visibility::Hidden {
                    *visibility = Visibility::Hidden;
                }
            }
        }
    }
}
