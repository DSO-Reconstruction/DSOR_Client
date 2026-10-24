//! The 2018 client's interface: the windows always on screen (the bottom bar with
//! the health and resource globes, the XP bar and the quick slots; the money in the
//! top left corner; navigation and menu buttons) and the inventory (I),
//! character sheet (C), skills (S) and game menu (Escape).
//!
//! tools/export_ui.py exports each window (`interface/<name>.ui.json`): its
//! widgets in drawing order, each with its artwork as the triangles, atlas UVs and
//! vertex colours of the window's mesh, its text, and `k`, how far it follows the
//! screen's extra width. The interface is laid out on a 1024 x 768 reference
//! screen scaled to the window's height; a wider window moves each widget right by
//! k x (reference width - 1024): the bottom bar stays centred, the money in the
//! top left, the menu in the top right.
//!
//! Drawn by a 2D camera over the 3D one, as meshes: one mesh per run of widgets
//! sharing a texture, anchor and state/page gate (draw order kept by depth), built once; a
//! resize only moves their parents. The globes and the XP bar are cut to their
//! fill by their material's clip rectangle; texts are Text2d. Nothing is rebuilt
//! per frame: values change materials/texts and controls select visual states.
//! controls.rs shares page visibility between drawing and pointer hit testing.
//! SEE: docs/ui.md.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext, RenderAssetUsages};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Hdr, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::sprite::Anchor;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};
use bevy::window::{PrimaryWindow, WindowResized};
use serde::Deserialize;

pub(crate) mod combat_text;
pub(crate) mod input;
pub(crate) mod slider;
mod controls;
mod cursor;
mod dialogue;
/// The item catalog (names by template), for crate::ui::combat_text's loot texts.
pub(crate) use controls::items::Catalog;
mod local_map;
mod npc_hover;
mod preview;
pub(crate) use preview::PreviewCamera;
mod selection;
#[cfg(not(target_arch = "wasm32"))]
mod replay;
pub(crate) use local_map::{MapIcon, MapLabel, MapMarkers, MapSettings};
use controls::{Element, Gate, WindowControls};
pub(crate) use controls::SkillClicks;
pub(crate) use controls::settings::Settings;

/// The reference screen the 2018 interface was laid out on.
const BASE_W: f32 = 1024.0;
const BASE_H: f32 = 768.0;
/// The interface's render layer: only the UI camera sees it.
const LAYER: usize = 7;
/// Shared text reduction for authored labels and runtime UI text.
const TEXT_SCALE: f32 = 0.8;
/// The engine's fontSize to reference pixels (DSO_Godot's calibration).
const FONT_SCALE: f32 = 1.5 * TEXT_SCALE;
/// Each complete window occupies one band, including runtime item contents.
const WINDOW_DEPTH_STEP: f32 = 20.0;
const WINDOW_ART_DEPTH: f32 = 8.0;
const WINDOW_ITEMS_DEPTH: f32 = 12.0;

/// Windows shown from the start, and their depth (later ones over earlier ones).
const HUD_WINDOWS: [(&str, f32); 5] = [("bottombar", 10.0), ("currencybar", 20.0), ("gamebar", 30.0), ("ingamemenu", 35.0), ("interactinfo", 40.0)];

/// The hovered monster's info window (top of the screen): shown while a monster
/// is under the cursor, with only its monster parts -- the frame, the health bar
/// and its name, the level ring.
/// UNVERIFIED: which of interactinfo's parts the 2018 client shows for a monster
///   (its window code is not traced): the object, player, map and drop variants
///   are left out.
const HOVER_WINDOW: &str = "interactinfo";
fn hover_part(id: &str) -> bool {
    // The monster's negative status effects, a row the client adds under the
    // panel (controls::effects::prepare_target).
    controls::effects::target_part(id)
        || id == "Canvas"
        || id == "Canvas/Label_bg"
        || id.starts_with("Canvas/Label_bg/Healthbar")
        || id == "Canvas/Label_bg/ActorInfo"
        || id == "Canvas/Label_bg/ActorInfo/ActorInfo"
        || id.starts_with("Canvas/Label_bg/ActorInfo/ActorInfo/LevelLabel")
        || id.starts_with("Canvas/Label_bg/ActorInfo/ActorInfo/gothic_panel")
        // release261 nests the frame and the level ring one level deeper
        // (ActorInfo1/gothic_panel/LevelLabel/.../Levelcircle4/Level). Its shield
        // icon, the rank circles and the shield-break sparks are other states'.
        // FAILURE (2026-10-16): with the 2018 paths alone, a hovered monster showed
        //   its bar and name, without its level ring or frame ("j'ai juste la barre
        //   et le nom").
        // UNVERIFIED: whether the rank circles (LabelBottom/Circle1..6) show for a
        //   monster of rank > 1.
        || (id == "Canvas/Label_bg/ActorInfo/ActorInfo/ActorInfo1"
            || id.starts_with("Canvas/Label_bg/ActorInfo/ActorInfo/ActorInfo1/gothic_panel"))
            && !["/Circle", "/shieldIcon", "/Spark", "/Boom", "/part", "/Anim_"].iter().any(|x| id.contains(x))
}
/// Windows the player opens: (window, key, depth).
const MENU_WINDOWS: [(&str, KeyCode, f32); 5] = [
    ("inventory", KeyCode::KeyI, 100.0),
    ("charactersheet", KeyCode::KeyC, 120.0),
    ("skilltalentswindow", KeyCode::KeyS, 140.0),
    ("collectorsalbum", KeyCode::KeyW, 160.0),
    ("gamemenu", KeyCode::Escape, 500.0),
];

/// Other top-navigation panels, loaded once and opened by their own shortcut.
const NAV_WINDOWS: [&str; 8] = ["questlog", "travelmap", "locationmap", "shopwindow", "socialscreen", "leaderboards", "matchmakingwindow", "eventwindow"];

/// The death dialog's parts drawn: each price once (the plain label beside its
/// Andermant icon; the centred and membership variants are alternatives the 2018
/// client picks between), without the membership advert, the planar-fissure text
/// or the bag strip at its bottom (other situations' parts).
/// UNVERIFIED: which variant the 2018 dialog code shows (not traced).
fn death_part(id: &str) -> bool {
    !["/CenteredTextLabel", "/PrivilegeTextLabel", "/ReviveHintLabel", "/planarfissuredeathdesc", "/Label_texture/unten", "/TipIconDummy"]
        .iter()
        .any(|p| id.contains(p))
        && !id.ends_with("/Label_texture")
}

/// The local player's death dialog: opened by the server's ShowDeathDialogCommand,
/// its buttons send RespawnCommand (crate::network::net::player_deaths).
const DEATH_WINDOW: &str = "deathdialog";
/// _Globals RespawnCost (at the nearest exit) and InstantRespawnCost (on the spot).
const RESPAWN_COST: u32 = 40;
const INSTANT_RESPAWN_COST: u32 = 120;

fn is_menu(name: &str) -> bool {
    MENU_WINDOWS.iter().any(|(n, _, _)| *n == name) || NAV_WINDOWS.contains(&name) || dialogue::WINDOWS.contains(&name) || name == "unstackwindow" || name == controls::items::DESTROY_DIALOG || name == controls::logout::WINDOW || name == DEATH_WINDOW || name == CHAT_MENU || name == controls::settings::WINDOW
        || SOCIAL_WINDOWS.iter().any(|(n, _)| *n == name) || controls::systems::DESKS.iter().any(|(n, _)| *n == name)
        || name == controls::currency::BAG
}

/// The notification dialogs and the player menu (controls::social): closed until
/// a notification or a middle click opens them; their depth.
const SOCIAL_WINDOWS: [(&str, f32); 5] = [(controls::social::GROUP_DIALOG, 640.0), (controls::social::BOOKING_DIALOG, 630.0), (controls::social::LOGIN_DIALOG, 620.0), (controls::social::CONTEXT_MENU, 660.0), (controls::social::TRADE_WINDOW, 400.0)];

/// The chat window (crate::network::chat): full controls or a compact click-through
/// preview, switched with V; and its
/// channel menu (ButtonChatMenu). Their depth: over the HUD, under the menus.
/// SEE: controls::edit_text (Enter), controls::chat_rows (the log).
const CHAT_WINDOW: &str = "chatwindow2";
const CHAT_MENU: &str = "chatwindow2dropdown";
const CHAT_DEPTH: f32 = 45.0;
/// A log row's width, reference pixels: the List's (39.1 .. 382.0 in
/// chatwindow2's layout) less the 2 the rows are inset by (spawn_windows).
const CHAT_ROW_WIDTH: f32 = 340.9;
/// Its text's size, reference pixels: the TextLine template's 9 x FONT_SCALE.
const CHAT_FONT_SIZE: f32 = 9.0 * FONT_SCALE;

/// How far the chat's ChatLabel ends above the bottom of the screen at first,
/// reference pixels (it can be dragged: controls::DragHandle).
/// EVIDENCE: the 2018 client in the reference video (2026-10-11 23-03-55.mp4, 1280
///   x 720): the compact toggle's frame ends 52 px above the bottom; at that UI's
///   scale (the frame's 77 reference pixels drawn 60 px high) that is ~67 reference
///   pixels, and ChatLabel ends 4 above the frame: ~60. ("un peu plus haut": the
///   first placement, 36 from the window's own Canvas, was too low.)
/// UNVERIFIED: measured on one recording, not from the client's code.
const CHAT_BOTTOM_MARGIN: f32 = 60.0;

const CHAT_PREVIEW_ROWS: usize = 4;

/// One row of the chat log, drawn over the List's TextLine template, top first.
#[derive(Component)]
struct ChatRow {
    index: usize,
    compact: bool,
}

/// Both chat layouts use screen-left coordinates despite the source's centre anchor.
/// The chat windows where the game shows them: in the bottom-left corner of the
/// screen, whatever its width. Their file centres the Canvas (HCenter / VCenter in
/// a CenterX / CenterY window), which put the chat in the middle of a wide screen
/// and 139 reference pixels above the bottom; the 2018 client draws its ChatLabel
/// against the screen's left and bottom edges (reference video: the compact toggle
/// against the left edge, near the bottom). So every widget gets k = 0 (the left
/// edge) and moves down until ChatLabel ends CHAT_BOTTOM_MARGIN above the bottom.
/// UNVERIFIED: the runtime placement code itself was not traced, only its result.
fn prepare_chat(layout: &Layout) -> Layout {
    let mut out = layout.clone();
    let label_bottom = layout.widgets.iter().find(|w| w.id == "Canvas/ChatLabel").map_or(BASE_H - CHAT_BOTTOM_MARGIN, |w| w.rect[3]);
    let dy = BASE_H - CHAT_BOTTOM_MARGIN - label_bottom;
    for w in &mut out.widgets {
        w.k = 0.0;
        w.rect[1] += dy;
        w.rect[3] += dy;
        if let Some(art) = &mut w.art {
            for y in art.pos.iter_mut().skip(1).step_by(2) {
                *y += dy;
            }
        }
    }
    out
}

// -- data ------------------------------------------------------------------------

#[derive(Deserialize, Clone)]
struct Art {
    tex: Option<String>,
    pos: Vec<f32>,
    uv: Vec<f32>,
    color: Vec<f32>,
    idx: Vec<u32>,
}

#[derive(Deserialize, Clone)]
struct Label {
    text: String,
    key: String,
    size: f32,
    color: [f32; 4],
    align: [u8; 2],
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    word_break: bool,
    /// TextEffect 1: an outline of [size, r, g, b, a] (drawn by outline copies, OutlineCopy).
    #[serde(default)]
    outline: Vec<f32>,
}

#[derive(Deserialize, Clone)]
struct Widget {
    id: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    event: String,
    #[serde(default)]
    click_through: bool,
    #[serde(default = "enabled_default")]
    enabled: bool,
    #[serde(default)]
    button: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    spell: Option<controls::spells::Spell>,
    k: f32,
    rect: [f32; 4],
    hidden: bool,
    art: Option<Art>,
    text: Option<Label>,
}

fn enabled_default() -> bool { true }

#[derive(Asset, TypePath, Deserialize, Clone)]
pub struct Layout {
    window: String,
    widgets: Vec<Widget>,
}

/// Skill id -> its icon (`textures/icons/<IconBrush>.png`).
#[derive(Asset, TypePath, Deserialize)]
#[serde(transparent)]
pub struct SkillIcons(HashMap<String, String>);

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
json_loader!(LayoutLoader, Layout, "ui.json");
json_loader!(SkillIconsLoader, SkillIcons, "icons.json");
json_loader!(MapNamesLoader, local_map::MapNames, "names.json");

// -- material ----------------------------------------------------------------------

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct UiMaterial {
    #[uniform(0)]
    clip: Vec4,
    #[uniform(0)]
    flags: Vec4,
    #[texture(1)]
    #[sampler(2)]
    atlas: Option<Handle<Image>>,
}

impl Material2d for UiMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://dsor_client/ui/ui.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

const NO_CLIP: Vec4 = Vec4::new(-1.0e6, -1.0e6, 1.0e6, 1.0e6);

fn material(atlas: Option<Handle<Image>>) -> UiMaterial {
    UiMaterial { clip: NO_CLIP, flags: Vec4::new(atlas.is_some() as u32 as f32, 0.0, 0.0, 0.0), atlas }
}

// -- layout ------------------------------------------------------------------------

/// The reference screen's width for the current window (its height is BASE_H).
#[derive(Resource, Clone, Copy)]
struct Screen {
    width: f32,
    /// Physical pixels per reference pixel (text is rasterised at that size).
    scale: f32,
}

impl Default for Screen {
    fn default() -> Self {
        Self { width: BASE_W, scale: 1.0 }
    }
}

impl Screen {
    /// World position of a reference point of a widget with this k.
    fn world(&self, k: f32, x: f32, y: f32) -> Vec2 {
        Vec2::new(x + k * (self.width - BASE_W) - self.width * 0.5, BASE_H * 0.5 - y)
    }
}

/// Moves with the screen's extra width: x = k x (width - 1024) - width / 2.
#[derive(Component)]
struct Follows {
    k: f32,
    /// Artwork parents flip y (the data is y down); text parents do not.
    flip: bool,
}

/// The interface's camera.
#[derive(Component)]
struct UiCamera;

/// The interface camera takes the scene camera's format, MSAA and texture usages, so both share
/// their target textures (see setup).
#[allow(clippy::type_complexity)]
fn match_scene_camera(
    mut commands: Commands,
    scene: Query<(Has<Hdr>, &Msaa, &bevy::camera::CameraMainTextureUsages), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>, Without<UiCamera>)>,
    mut ui: Query<(Entity, Has<Hdr>, &mut Msaa, &mut bevy::camera::CameraMainTextureUsages), With<UiCamera>>,
) {
    let (Some((hdr, msaa, usages)), Ok((e, ui_hdr, mut ui_msaa, mut ui_usages))) = (scene.iter().next(), ui.single_mut()) else { return };
    if ui_usages.0 != usages.0 { ui_usages.0 = usages.0; }
    if *ui_msaa != *msaa {
        *ui_msaa = *msaa;
    }
    if hdr != ui_hdr {
        if hdr {
            commands.entity(e).insert(Hdr);
        } else {
            commands.entity(e).remove::<Hdr>();
        }
    }
}

/// Every camera on the window takes the window's new size on the frame it
/// changes: the overlay draws into the scene camera's textures with a depth
/// texture of its own size.
/// CONTRACT: bevy's camera_system refreshes a camera's size on a WindowResized
///   message or a change of its projection. A size set in code
///   (`Window::resolution`, the UI replay's `window_size`) reaches winit in
///   `Last` and its message comes a frame later: the scene camera, whose
///   projection is written every frame, took the new size at once, the overlay
///   kept its old depth texture over the shared new colour, and wgpu stopped the
///   client ("Attachments have differing sizes"). Resizes by the player
///   (dragging, maximizing, fullscreen) send the message on the same frame.
fn follow_window_size(
    windows: Query<(Entity, &Window), (With<PrimaryWindow>, Changed<Window>)>,
    mut cameras: Query<(&Camera, &bevy::camera::RenderTarget, &mut Projection)>,
) {
    let Ok((primary, window)) = windows.single() else { return };
    let size = window.physical_size();
    if size.x == 0 || size.y == 0 { return; }
    for (camera, target, mut projection) in &mut cameras {
        let on_window = match target {
            bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Primary) => true,
            bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(e)) => *e == primary,
            _ => false,
        };
        if on_window && camera.physical_target_size().is_some_and(|s| s != size) {
            projection.set_changed();
        }
    }
}

/// A window's root: its name, shown or not.
#[derive(Component)]
pub(crate) struct Window2018(pub(crate) String);

/// A cut fill: the widget rect it fills and which way.
#[derive(Component)]
struct Fill {
    which: Gauge,
    k: f32,
    rect: [f32; 4],
    /// The fraction drawn, last applied.
    shown: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum Gauge {
    Health,
    Resource,
    Xp,
    /// The hovered monster's health (interactinfo Healthbar/progress).
    TargetHealth,
    HealthBar,
    ResourceBar,
    Wisdom,
    /// A party member's health (groupwindow row n; controls::party).
    Group(u8),
}

impl Gauge {
    /// One number per gauge, for the rectangles' map.
    fn key(self) -> u8 {
        match self {
            Self::Health => 0,
            Self::Resource => 1,
            Self::Xp => 2,
            Self::TargetHealth => 3,
            Self::HealthBar => 4,
            Self::ResourceBar => 5,
            Self::Wisdom => 6,
            Self::Group(n) => 16 + n,
        }
    }
}

/// A text the game sets: what it shows.
#[derive(Component)]
struct Value(ValueKind);

/// A label whose text shrinks to stay within its place (the money): `width`
/// and the authored font `size`, reference pixels. The game shortens these
/// amounts (controls::currency::clamped) and lets their font shrink (the
/// layouts' FontScalingFactor 0.6); here it shrinks as far as the place needs.
#[derive(Component)]
struct Fit {
    width: f32,
    size: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Hotkey(usize),
    EssenceKey,
    BarNumber,
    /// A fixed figure: the death dialog's respawn prices ("{0:i}"), the
    /// static database's _Globals RespawnCost / InstantRespawnCost.
    /// UNVERIFIED: the server's own price (a membership makes them free).
    Fixed(u32),
    Gold,
    Silver,
    Copper,
    Andermant,
    /// The currency the player picked in the currency bag, on the bar
    /// (controls::currency::Shown).
    Picked,
    Level,
    XpPercent,
    WisdomLevel,
    WisdomProgress,
    /// The bottom bar's level labels: "Level {0:i}" (levelShort) and "Wisdom
    /// Level {0:i}" (wisdomLevelShort), as the original's bars read.
    BarLevel,
    BarWisdomLevel,
    WisdomPoints,
    LocationName,
    Name,
    Page,
    /// The hovered monster's title and level (interactinfo).
    TargetName,
    TargetLevel,
    /// A character sheet figure (crate::gameplay::stats::Sheet, by widget name).
    Sheet(&'static str),
    /// A status effect's time left: its half of the bar and slot
    /// (controls::effects writes it).
    EffectTime(u8, u8),
}

/// The character sheet's widgets crate::gameplay::stats fills.
const SHEET_WIDGETS: &[&str] = &[
    "CharacterClass", "Guildname", "Level", "NextLevel", "XP", "BaseDamage", "AttackSpeed", "LMKSkill", "RMKSkill",
    "DamageLMK", "DamageRMK", "CurHealth", "MaxHealth", "SkillResourceTitle", "CurSkillResource", "MaxSkillResource",
    "Armor", "Armor_Percentage", "BlockRating", "BlockRating_Percentage", "BlockValue", "BlockValue_Percentage",
    "CriticalHit", "CriticalHit_Percentage", "CriticalDamage", "CriticalDamage_Percentage", "Fire", "Fire_Percentage",
    "Nature", "Nature_Percentage", "Ice", "Ice_Percentage", "Lightning", "Lightning_Percentage", "DarkMagic",
    "DarkMagic_Percentage",
];

/// A quick slot's icon place: its wire slot and rect.
#[derive(Component)]
struct QuickSlot {
    slot: usize,
    k: f32,
    rect: [f32; 4],
    /// The icon and count drawn, whether greyed (its skill costs more than
    /// the resource there is), the cooldown sweep's step (0: ready) and its
    /// seconds left in tenths (skills::cooldown_tenths).
    shown: Option<(String, u32, bool, u8, u32, Option<String>)>,
}

/// The figures the interface shows (from the server when online).
#[derive(Resource, Default)]
struct Figures {
    health: (f32, f32),
    resource: (f32, f32),
    xp: f32,
    /// Total experience and the next level's (net.xp), for the bar's tooltip.
    xp_points: Option<(u32, u32)>,
    /// The server starts wisdom at zero; keep that default until progression is decoded.
    wisdom: (u32, f32),
    wisdom_points: u32,
    bar: Vec<Option<String>>,
    name: String,
    online: bool,
    level: Option<u32>,
    /// The money and the other currencies (0 until the server sends them).
    wallet: controls::currency::Wallet,
}

#[derive(Resource, Default)]
struct Loaded {
    layouts: Vec<(Handle<Layout>, f32, Option<KeyCode>)>,
    icons: Handle<SkillIcons>,
    map_names: Handle<local_map::MapNames>,
    spawned: Vec<bool>,
    /// The class the HUD's resource globe was built for (resource_bar).
    resource_class: Option<u8>,
}

/// The interface's rebuilding work, counted. A frame in which nothing the
/// windows show has changed must leave every counter alone: no layout prepared,
/// no view or quest list rebuilt (docs/ui.md, "Per-frame cost"). The tests
/// check this; `DSOR_FLAGS=frameprofile` logs the counts every five seconds.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
pub(crate) struct UiWork {
    /// Layouts taken through preparation to build their window (spawn_windows).
    pub layouts_prepared: u64,
    /// Systems' window views rebuilt (controls::systems::sync).
    pub system_views: u64,
    /// Quest lists rebuilt (controls::quests::sync).
    pub quest_lists: u64,
    /// The status effect bar's slots assigned (controls::effects::sync).
    pub status_bars: u64,
    /// 3D preview subjects built (preview::choose): a character, a model.
    pub previews: u64,
    /// The account screen's game data strip repainted (screens::login::game_data::paint).
    pub game_data_bars: u64,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "ui.wgsl");
        app.add_plugins(Material2dPlugin::<UiMaterial>::default())
            .add_plugins(selection::SelectionUiPlugin)
            .add_plugins(cursor::CursorPlugin)
            .add_plugins(preview::PreviewPlugin)
            .init_asset::<Layout>()
            .init_asset::<SkillIcons>()
            .init_asset::<local_map::MapNames>()
            .init_asset_loader::<MapNamesLoader>()
            .init_asset_loader::<LayoutLoader>()
            .init_asset_loader::<SkillIconsLoader>()
            .init_asset::<dialogue::Catalog>()
            .init_asset_loader::<dialogue::CatalogLoader>()
            .init_asset::<crate::gameplay::quests::QuestCatalog>()
            .init_asset_loader::<crate::gameplay::quests::QuestCatalogLoader>()
            .init_asset::<crate::gameplay::systems::SystemsCatalog>()
            .init_asset_loader::<crate::gameplay::systems::SystemsCatalogLoader>()
            .init_resource::<controls::systems::State>()
            .init_resource::<controls::systems::Views>()
            .init_resource::<controls::currency::Shown>()
            .init_resource::<controls::message::ItemMessages>()
            .init_resource::<UiWork>()
            .init_resource::<crate::gameplay::dialogue::Dialogue>()
            .init_asset::<controls::items::Catalog>()
            .init_asset_loader::<controls::items::CatalogLoader>()
            .init_asset::<controls::effects::Catalog>()
            .init_asset_loader::<controls::effects::CatalogLoader>()
            .init_resource::<controls::effects::StatusBar>()
            .init_resource::<controls::items::Interaction>()
            .init_resource::<Screen>()
            .init_resource::<Figures>()
            .init_resource::<controls::Pointer>()
            .init_resource::<SkillClicks>()
            .init_resource::<controls::social::SocialUi>()
            .init_resource::<controls::trade::TradeUi>()
            .init_resource::<controls::party::Party>()
            .init_resource::<controls::quests::Quests>()
            .init_resource::<crate::gameplay::quests::QuestUi>().init_resource::<crate::ui::controls::systems::Views>()
            .init_resource::<controls::items::pins::Pins>()
            .init_resource::<Settings>()
            .init_resource::<controls::hotbar::Hotbar>()
            .insert_resource(local_map::MapSettings::load())
            .add_systems(Startup, (setup, controls::items::setup, dialogue::setup, controls::message::setup))
            .add_systems(Update, follow_screen)
            .add_systems(Update, follow_outlines.after(follow_screen))
            .add_systems(PostUpdate, match_scene_camera.after(crate::rendering::graphics::ApplyGraphics))
            .add_systems(PostUpdate, follow_window_size.before(bevy::camera::CameraUpdateSystems))
            .add_systems(PostUpdate, npc_hover::show
                .after(crate::rendering::highlight::pick)
                .before(TransformSystems::Propagate))
            .add_systems(PostUpdate, refresh_mesh_order
                .after(TransformSystems::Propagate)
                .before(bevy::sprite_render::check_entities_needing_specialization::<UiMaterial>))
            .add_systems(
                Update,
                ((dialogue::sync, controls::party::sync, controls::quests::sync, controls::systems::sync, controls::currency::sync_bar, controls::message::sync, spawn_windows, controls::sync_chat).chain(), controls::spells::sync, controls::items::sync, (controls::items::pins::update, controls::keyboard).chain(), controls::interact, (controls::settings::sync, controls::settings::sliders, controls::hotbar::input, controls::spells::apply_drop, controls::stickers::apply_drop).chain(), read_figures, controls::edit_text, controls::items::input, controls::order_windows, (update_fills, (update_values, fit_texts).chain(), update_slots, (local_map::update, local_map::update_markers, local_map::appearance).chain(), (controls::effects::fixture, controls::effects::sync, controls::effects::draw).chain()), controls::chat_rows, controls::sheet::rows, controls::paint, (controls::items::render, controls::items::destroy_dialog), (controls::spells::overlay, controls::items::overlay, controls::tooltips), show_hover_window, death_dialog, (controls::trade::sync, controls::social::column, controls::social::context_menu, controls::social::answer, controls::logout::dialog, controls::systems::send, controls::social::dialogs, controls::trade::render).chain()).chain().after(crate::actors::monsters::MonsterHoverInfo).before(crate::network::net::NetSystems).run_if(crate::screens::loading::gameplay_ready),
            );
        // The 3D previews: once this frame's clicks moved the pages and the
        // views were rebuilt.
        app.add_systems(Update, (preview::resize, preview::choose, preview::reveal, preview::turn, preview::encore).chain()
            .after(controls::interact).after(controls::systems::sync).before(controls::paint)
            .run_if(crate::screens::loading::gameplay_ready));
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(PreUpdate, replay::run.after(bevy::input::InputSystems)
            .before(bevy::ui::UiSystems::Focus).run_if(|login: Option<Res<crate::LoginGate>>,
                loading: Res<crate::screens::loading::LoadingScreen>, selection: Option<Res<crate::screens::selection::Selection>>| {
                (login.is_some() && !loading.active()) || crate::screens::loading::gameplay_ready(login, loading, selection)
            }));
    }
}

/// Bevy 0.19 retains Material2d sort keys until the mesh/material changes.
/// Sprites and text use their current transforms every frame, so moving a
/// window in Z otherwise lets its stale background draw over its own contents.
/// Mark moved UI meshes dirty after hierarchy propagation and before extraction.
fn refresh_mesh_order(mut meshes: Query<&mut Mesh2d, (With<MeshMaterial2d<UiMaterial>>, Changed<GlobalTransform>)>) {
    for mut mesh in &mut meshes { mesh.set_changed(); }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>, mut quest_ui: ResMut<crate::gameplay::quests::QuestUi>, mut systems: ResMut<controls::systems::State>,
    mut status_bar: ResMut<controls::effects::StatusBar>) {
    commands.spawn((
        Camera2d,
        // Drawn into the 3D camera's own target textures, not cleared: that needs
        // the same HDR and MSAA settings (match_scene_camera), or the overlay
        // gets textures of its own that nothing clears and changing texts smear.
        Camera { order: 10, clear_color: ClearColorConfig::None, ..default() },
        UiCamera,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical { viewport_height: BASE_H },
            ..OrthographicProjection::default_2d()
        }),
        Tonemapping::None,
        // bevy_ui (the debug HUD, the look panel) renders here only: on both
        // cameras it was drawn twice.
        IsDefaultUiCamera,
        RenderLayers::layer(LAYER),
    ));
    let mut loaded = Loaded { icons: assets.load("interface/skills.icons.json"), map_names: assets.load("interface/maps.names.json"), ..default() };
    loaded.layouts.push((assets.load("interface/unstackwindow.ui.json"), 600.0, None));
    loaded.layouts.push((assets.load(format!("interface/{}.ui.json", controls::items::DESTROY_DIALOG)), 610.0, None));
    loaded.layouts.push((assets.load(format!("interface/{}.ui.json", controls::logout::WINDOW)), 612.0, None));
    loaded.layouts.push((assets.load("interface/settingswindow2.ui.json"), 550.0, None));
    loaded.layouts.push((assets.load(format!("interface/{CHAT_WINDOW}.ui.json")), CHAT_DEPTH, None));
    loaded.layouts.push((assets.load(format!("interface/{CHAT_MENU}.ui.json")), CHAT_DEPTH + 3.0, None));
    for (name, z) in HUD_WINDOWS {
        loaded.layouts.push((assets.load(format!("interface/{name}.ui.json")), z, None));
    }
    // The currency bar's bag, opened under it (controls::currency).
    loaded.layouts.push((assets.load(format!("interface/{}.ui.json", controls::currency::BAG)), 48.0, None));
    for (name, key, z) in MENU_WINDOWS {
        loaded.layouts.push((assets.load(format!("interface/{name}.ui.json")), z, Some(key)));
    }
    for (i, name) in NAV_WINDOWS.iter().enumerate() {
        let depth = if *name == "locationmap" { 80.0 } else { 180.0 + i as f32 * WINDOW_DEPTH_STEP };
        loaded.layouts.push((assets.load(format!("interface/{name}.ui.json")), depth, None));
    }
    loaded.layouts.push((assets.load(format!("interface/{DEATH_WINDOW}.ui.json")), 700.0, None));
    // The quest screen under the NPC screen: its backdrop shades the world, not
    // the NPC's portrait (they open together; controls::order_windows stacks
    // them by this depth).
    // FAILURE (2026-10-24): at the same depth, the order of their entities
    //   decided, and the backdrop darkened the portrait every other time.
    for name in dialogue::WINDOWS {
        let depth = if name == "questscreen2" { 370.0 } else { 380.0 };
        loaded.layouts.push((assets.load(format!("interface/{name}.ui.json")), depth, None));
    }
    for (name, z) in SOCIAL_WINDOWS { loaded.layouts.push((assets.load(format!("interface/{name}.ui.json")), z, None)); }
    // The party frames: a HUD window, drawn only while grouped (controls::party).
    loaded.layouts.push((assets.load(format!("interface/{}.ui.json", controls::party::WINDOW)), 45.0, None));
    // The quest tracker (top right) and the quest data its rows and the log read.
    loaded.layouts.push((assets.load(format!("interface/{}.ui.json", controls::quests::TRACKER)), 46.0, None));
    // The items received, under the top navigation (controls::message): over
    // the HUD (the hovered monster's panel shares the top middle), under the menus.
    loaded.layouts.push((assets.load(format!("interface/{}.ui.json", controls::message::WINDOW)), 62.0, None));
    quest_ui.catalog = assets.load("interface/quests.catalog.json");
    // The NPC desks (controls::systems), opened from an NPC's menu, and the
    // systems' data their windows and the quest log's pages read.
    for (name, z) in controls::systems::DESKS { loaded.layouts.push((assets.load(format!("interface/{name}.ui.json")), z, None)); }
    systems.catalog = assets.load("interface/systems.catalog.json");
    // The status effects the bottom bar shows (controls::effects).
    status_bar.catalog = assets.load("interface/status.catalog.json");
    loaded.spawned = vec![false; loaded.layouts.len()];
    commands.insert_resource(loaded);
}

/// Which dynamic part a widget is, if any.
fn gauge_of(id: &str) -> Option<Gauge> {
    if id.contains("/WisdomLabel/") && id.ends_with("/LevelBar") { return Some(Gauge::Wisdom); }
    if let Some(member) = controls::party::gauge_of(id) { return Some(Gauge::Group(member)); }
    let parent = id.rsplit('/').nth(1).unwrap_or("");
    if parent == "Healthbar" && id.ends_with("/progress") {
        return Some(Gauge::TargetHealth);
    }
    match parent {
        "Health" if id.ends_with("/progress") => Some(Gauge::HealthBar),
        "Resource" if id.ends_with("/progress") => Some(Gauge::ResourceBar),
        "bottombar_xp_bar" if id.ends_with("/progress") => Some(Gauge::Xp),
        "bottombar_wisdom_bar" if id.ends_with("/progress") => Some(Gauge::Wisdom),
        "ProgressBar_health" => Some(Gauge::Health),
        "ProgressBar_rage" | "ProgressBar_mana" | "ProgressBar_stamina" | "ProgressBar_mechanicResource" => Some(Gauge::Resource),
        "ProgressBar_xp" if !id.ends_with("glow") && !id.contains("glow") => Some(Gauge::Xp),
        // The character sheet's XP bar: its begin, middle and end pieces.
        "XPBar" if !id.ends_with("xp_leiste_relief") => Some(Gauge::Xp),
        _ => None,
    }
}

/// The resource globe of a class (0 warrior, 1 mage, 2 ranger, 3 dwarf).
/// The bottom bar's resource gauge in the class's own colour: its layout draws
/// it with the mage's mana; the game sets the texture by class.
/// EVIDENCE: release261 0x1403ACD74: class 0 ui/statusbar_bottombar_rage, 1 the
///   mana one, 2 ui/statusbar_bottombar_adrenaline, 3 ui/statusbar_bottombar_steam.
fn class_resource_gauge(mut layout: Layout, class: u8) -> Layout {
    let texture = match class {
        0 => "textures/ui/statusbar_bottombar_rage.png",
        2 => "textures/ui/statusbar_bottombar_adrenaline.png",
        3 => "textures/ui/statusbar_bottombar_steam.png",
        _ => return layout,
    };
    for w in layout.widgets.iter_mut().filter(|w| w.id.ends_with("/Resource/progress")) {
        if let Some(art) = &mut w.art { art.tex = Some(texture.to_owned()); }
    }
    layout
}

/// The class emblems (textures/ui/emblem_class_*): CharClass 0..3.
fn class_emblem(class: u8) -> &'static str {
    match class {
        1 => "spellweaver",
        2 => "ranger",
        3 => "steammechanic",
        _ => "dragonknight",
    }
}

fn resource_bar(class: u8) -> &'static str {
    match class {
        0 => "ProgressBar_rage",
        2 => "ProgressBar_stamina",
        3 => "ProgressBar_mechanicResource",
        _ => "ProgressBar_mana",
    }
}

fn value_of(id: &str) -> Option<ValueKind> {
    let leaf = id.rsplit('/').next().unwrap_or("");
    if let Some(slot) = controls::hotbar::label_slot(id) { return Some(ValueKind::Hotkey(slot)); }
    if let Some((side, slot)) = controls::effects::time_label(id) { return Some(ValueKind::EffectTime(side, slot)); }
    if leaf == "TextLabel_ammoslot1" { return Some(ValueKind::EssenceKey); }
    if leaf == "CurrentQuickbar" { return Some(ValueKind::BarNumber); }
    if id.contains("/StatsCharacter/") {
        if let Some(w) = SHEET_WIDGETS.iter().find(|w| **w == leaf) {
            return Some(ValueKind::Sheet(w));
        }
    }
    if matches!(leaf, "TextLabel" | "CenteredTextLabel" | "PrivilegeTextLabel") && !id.contains("DontBuy") {
        if id.contains("/Label/Buy1/") {
            return Some(ValueKind::Fixed(INSTANT_RESPAWN_COST));
        }
        if id.contains("/Label/Buy/") {
            return Some(ValueKind::Fixed(RESPAWN_COST));
        }
    }
    Some(match leaf {
        "gold_display" if id.contains("VirtualCurrency") => ValueKind::Gold,
        "silver_display" => ValueKind::Silver,
        "copper_display" => ValueKind::Copper,
        "andermant_display" => ValueKind::Andermant,
        "template_amount" if id == controls::currency::PICKED_AMOUNT => ValueKind::Picked,
        // The bottom bar reads "Level 36" / "Wisdom Level 6" and shows no
        // percentages (the user's release261_192 shots; the numbers are its
        // tooltips').
        // FAILURE (2026-10-24): ours read "55 ... 82%" and "0 ... 0%".
        "Level" if id.contains("/bottombar_xp_bar/") => ValueKind::BarLevel,
        "Level" if id.contains("ProgressBar_xp") => ValueKind::Level,
        "XP" if id.contains("/bottombar_xp_bar/") => ValueKind::XpPercent,
        "Level" if id.contains("/bottombar_wisdom_bar/") => ValueKind::BarWisdomLevel,
        "Wisdom" if id.contains("/bottombar_wisdom_bar/") => ValueKind::WisdomProgress,
        "LevelLabel" if id.contains("/WisdomLabel/") => ValueKind::WisdomLevel,
        "AvailablePoints" if id.contains("/WisdomLabel/") => ValueKind::WisdomPoints,
        "ActorName" if id.contains("/Healthbar/") => ValueKind::TargetName,
        "Level" if id.contains("/Levelcircle4/") => ValueKind::TargetLevel,
        "CharacterName" => ValueKind::Name,
        "LocationName" => ValueKind::LocationName,
        "PageNumText" => ValueKind::Page,
        _ => return None,
    })
}

/// The quick slots by wire slot: Shift + left button = 0, right button = 1, keys
/// 1-5 = 2-6 (crate::gameplay::skills), 6 and 7 the bar's last two.
/// Where a quick slot draws its item. The potion slots' buttons (41.5 px) take
/// in their hover glow around the round slot (33.1 px, PotionSlot*'s
/// bottombar_slot_round_empty_potion); the potion sits inside the circle,
/// centred, about 70 % of its width (the user's release261_192 bar).
/// FAILURE (2026-10-23): drawn over the whole button, the potion spilled over the
///   slot's ring ("wtf is with that potion, it looks so wrong").
fn slot_icon_rect(slot: usize, rect: [f32; 4]) -> [f32; 4] {
    const POTION: f32 = 33.1 * 0.7;
    if !(13..=16).contains(&slot) { return rect; }
    let [l, t, r, b] = rect;
    let (cx, cy, h) = ((l + r) * 0.5, (t + b) * 0.5, POTION * 0.5);
    [cx - h, cy - h, cx + h, cy + h]
}

fn quick_slot_of(id: &str) -> Option<usize> {
    if id.contains("/Frame5/") || id.contains("/icon_bg/") {
        return controls::hotbar::slot_leaf(id.rsplit('/').next()?);
    }
    let (parent, leaf) = {
        let mut it = id.rsplit('/');
        let leaf = it.next()?;
        (it.next().unwrap_or(""), leaf)
    };
    if !matches!(parent, "icon_bg" | "Iconslots") {
        return None;
    }
    Some(match leaf {
        "LMB" => 0,
        "RMB" => 1,
        s if s.starts_with("QuickSlot") => s["QuickSlot".len()..].parse::<usize>().ok()? + 1,
        _ => return None,
    })
}

/// Texts the window carries as they are: localised ones and short literals
/// (slot numbers, "Tab"). Placeholders the game fills ("Text", "T", "XP",
/// "clientVersion") are left out unless they are a Value.
fn static_text(l: &Label) -> bool {
    l.key.starts_with("gui.") || (l.text.chars().count() <= 3 && !l.text.is_empty() && l.text != "T" && l.text != "XP")
}

#[derive(Default)]
struct Run {
    k: f32,
    gate: Gate,
    /// Drawing order: transparent 2D meshes are sorted by their entity's z.
    z: f32,
    tex: Option<String>,
    pos: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    color: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl Run {
    fn push(&mut self, art: &Art, z: f32) {
        if self.pos.is_empty() {
            self.z = z;
        }
        let base = self.pos.len() as u32;
        let n = art.pos.len() / 2;
        for i in 0..n {
            self.pos.push([art.pos[2 * i], art.pos[2 * i + 1], 0.0]);
            self.uv.push([art.uv.get(2 * i).copied().unwrap_or(0.0), art.uv.get(2 * i + 1).copied().unwrap_or(0.0)]);
            let c = |j: usize| art.color.get(4 * i + j).copied().unwrap_or(1.0);
            self.color.push([c(0), c(1), c(2), c(3)]);
        }
        self.idx.extend(art.idx.iter().map(|i| base + i));
    }

    fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.color)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_windows(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    layouts: Res<Assets<Layout>>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<UiMaterial>>,
    screen: Res<Screen>,
    players: Query<&crate::actors::character::Character, With<crate::network::net::LocalPlayer>>,
    dialogue: Res<dialogue::Presentation>,
    dialogue_catalogs: Res<Assets<dialogue::Catalog>>,
    party: Res<controls::party::Party>,
    quests: Res<controls::quests::Quests>,
    (views, shown, item_messages): (Res<controls::systems::Views>, Res<controls::currency::Shown>, Res<controls::message::ItemMessages>),
    mut windows: Query<(Entity, &Window2018, &Visibility, &mut WindowControls)>,
    mut work: ResMut<UiWork>,
    previews: Option<Res<preview::Previews>>,
) {
    let known = players.iter().next().map(|c| c.desc.class);
    let class = known.or(loaded.resource_class).unwrap_or(1);
    // The resource globe shows the class's own resource (rage, mana, focus,
    // steam). The HUD is built before the character arrives online: built with
    // the default class it stayed the mage's blue mana for every class ("la rage
    // pour le combattant c'est pas bleu comme du mana", 2026-10-18). Once the
    // class is known, or changes, the windows holding a resource gauge rebuild.
    if known.is_some() && known != loaded.resource_class {
        let built = loaded.resource_class.is_some() || loaded.spawned.iter().any(|s| *s);
        loaded.resource_class = known;
        if built {
            for i in 0..loaded.layouts.len() {
                let Some(layout) = layouts.get(&loaded.layouts[i].0) else { continue };
                if !loaded.spawned[i] || !layout.widgets.iter().any(|w| matches!(gauge_of(&w.id), Some(Gauge::Resource | Gauge::ResourceBar))) { continue; }
                if let Some((root, _, _, _)) = windows.iter().find(|(_, w, _, _)| w.0 == layout.window) {
                    commands.entity(root).despawn_related::<Children>();
                }
                loaded.spawned[i] = false;
            }
        }
    }
    for i in 0..loaded.layouts.len() {
        if loaded.spawned[i] {
            continue;
        }
        let z = loaded.layouts[i].1;
        let Some(layout) = layouts.get(&loaded.layouts[i].0) else { continue };
        // Keep a light root/control model so every existing shortcut, network
        // event and navigation button can open the window. Prepare and build its
        // thousands of artwork/text entities only when it is shown.
        // FAILURE (2026-10-21): the layouts of closed, never-opened menus were
        //   prepared here every frame, then dropped below: 6 ms a frame once the
        //   systems' windows filled theirs (180 -> 95 FPS). A hidden window costs
        //   nothing per frame (docs/ui.md, "Per-frame cost").
        let existing = windows.iter_mut().find(|(_, w, _, _)| w.0 == layout.window);
        if existing.as_ref().is_some_and(|(_, _, visibility, _)| **visibility == Visibility::Hidden) { continue; }
        work.layouts_prepared += 1;
        let prepared;
        let npc_window = dialogue::WINDOWS.contains(&layout.window.as_str());
        let layout = if npc_window {
            prepared = dialogue::prepare(layout, &dialogue, &dialogue_catalogs);
            if prepared.widgets.is_empty() { loaded.spawned[i] = true; continue; }
            &prepared
        } else if matches!(layout.window.as_str(), CHAT_WINDOW | CHAT_MENU) {
            prepared = prepare_chat(layout);
            &prepared
        } else if let Some(filled) = controls::systems::prepare(layout, &views, &quests) {
            prepared = filled; &prepared
        } else if layout.window == controls::quests::TRACKER {
            prepared = controls::quests::prepare_tracker(layout, &quests);
            if prepared.widgets.is_empty() { loaded.spawned[i] = true; continue; }
            &prepared
        } else if layout.window == controls::party::WINDOW {
            prepared = controls::party::prepare(layout, &party);
            if prepared.widgets.is_empty() { loaded.spawned[i] = true; continue; }
            &prepared
        } else if layout.window == controls::social::CONTEXT_MENU {
            prepared = controls::social::prepare_menu(layout);
            &prepared
        } else if layout.window == controls::settings::WINDOW {
            prepared = controls::settings::prepare(layout); &prepared
        } else if layout.window == "bottombar" {
            prepared = class_resource_gauge(controls::hotbar::prepare(layout), class); &prepared
        } else if layout.window == HOVER_WINDOW {
            prepared = controls::effects::prepare_target(layout); &prepared
        } else if layout.window == controls::items::DESTROY_DIALOG {
            prepared = controls::items::prepare_destroy(layout); &prepared
        } else if layout.window == controls::logout::WINDOW {
            prepared = controls::logout::prepare(layout); &prepared
        } else if layout.window == "inventory" {
            prepared = preview::prepare_inventory(layout); &prepared
        } else if layout.window == controls::currency::BAR {
            prepared = controls::currency::prepare_bar(layout, &shown); &prepared
        } else if layout.window == controls::currency::BAG {
            // Opened under the bar's reference label.
            let bar = loaded.layouts.iter().filter_map(|(h, _, _)| layouts.get(h)).find(|l| l.window == controls::currency::BAR);
            prepared = controls::currency::prepare_bag(layout, &views.currencies, bar); &prepared
        } else if layout.window == controls::message::WINDOW {
            prepared = controls::message::prepare(layout, &item_messages);
            if prepared.widgets.is_empty() { loaded.spawned[i] = true; continue; }
            &prepared
        } else { layout };
        // DSOR_UI_OPEN=inventory,...: menus open from the start (screenshots).
        let open = npc_window || std::env::var("DSOR_UI_OPEN").is_ok_and(|v| v.split(',').any(|w| w == layout.window));
        let hidden = (is_menu(&layout.window) || layout.window == HOVER_WINDOW) && !open;
        let (root, mut controls) = if let Some((root, _, _, mut controls)) = existing {
            // Rebuilt (its rows changed): the new buttons, the player's settings.
            let mut fresh = WindowControls::new(layout);
            fresh.carry(std::mem::replace(&mut *controls, WindowControls::new(layout)));
            (root, fresh)
        } else {
        let root = commands
            .spawn((
                Window2018(layout.window.clone()),
                Transform::from_xyz(0.0, 0.0, z),
                if hidden { Visibility::Hidden } else { Visibility::Inherited },
                WindowControls::new(layout), crate::ui::input::UiRegions::default(),
            ))
            .id();
            if hidden { continue; }
            (root, WindowControls::new(layout))
        };
        loaded.spawned[i] = true;
        // One artwork parent (y flipped) and one text parent per k.
        let mut parents: HashMap<(u32, bool), Entity> = HashMap::new();
        let mut parent = |commands: &mut Commands, k: f32, flip: bool| -> Entity {
            *parents.entry((k.to_bits(), flip)).or_insert_with(|| {
                let tf = follow_transform(&screen, k, flip);
                commands.spawn((Follows { k, flip }, tf, Visibility::Inherited, ChildOf(root))).id()
            })
        };
        let atlas = |tex: &Option<String>| tex.as_ref().and_then(|t| {
            // A 3D preview's image (preview::Stage::texture).
            if let Some(stage) = preview::Stage::of_texture(t) { return previews.as_ref().and_then(|p| p.image(stage)); }
            Some(if t == "textures/ui/npcscreen.png" || t == "textures/ui/struktur_hell.png" {
                // The authored speech tails use UVs above 1 to select wrapped
                // atlas regions. Clamping cuts off every tail after the first.
                // The lists' light rows tile their texture (u -1 to 3.5 across
                // the social window's rows): clamped, its edges smeared into
                // streaks.
                assets.load_builder().with_settings(|s: &mut bevy::image::ImageLoaderSettings| {
                    s.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
                        address_mode_u: bevy::image::ImageAddressMode::Repeat,
                        address_mode_v: bevy::image::ImageAddressMode::Repeat,
                        ..bevy::image::ImageSamplerDescriptor::linear()
                    });
                }).load(t.clone())
            } else { assets.load::<Image>(t.clone()) })
        });
        let mut run: Option<Run> = None;
        let mut depth = 0.0f32;
        // Large XML trees must not let their last widget enter another window's
        // band. Keep all authored art/text below the runtime inventory contents.
        let depth_step = 0.01_f32.min(WINDOW_ART_DEPTH / (layout.widgets.len() + 1) as f32);
        let flush = |commands: &mut Commands, run: Option<Run>, parent: Entity, meshes: &mut Assets<Mesh>, materials: &mut Assets<UiMaterial>| {
            let Some(run) = run else { return };
            if run.idx.is_empty() {
                return;
            }
            let mut appearance = material(atlas(&run.tex));
            // The character page shades its own textured backing. Preserve its
            // authored opacity so the dark-panel adjustment cannot erase it.
            appearance.flags.z = (layout.window == "charactersheet") as u32 as f32;
            // A 3D preview's image: its multisampled edges are premultiplied
            // by their coverage (ui.wgsl).
            let stage = run.tex.as_deref().and_then(preview::Stage::of_texture);
            if stage.is_some() { appearance.flags.w = 1.0; }
            let m = materials.add(appearance);
            let z = run.z;
            let gate = run.gate.clone();
            let mut mesh = commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(m), Transform::from_xyz(0.0, 0.0, z), RenderLayers::layer(LAYER), ChildOf(parent), Element { root, gate }, Visibility::Hidden));
            if let Some(stage) = stage { mesh.insert(preview::Quad(stage)); }
        };
        let wanted_resource = resource_bar(class);
        // The map is a runtime overlay with no legacy frame or hit regions,
        // including when using an older exported locationmap layout.
        if layout.window == "locationmap" {
            commands.entity(root).insert((controls, crate::ui::input::UiRegions::default()));
            continue;
        }
        // Each gauge fills one rectangle: the union of its pieces (a globe's two
        // halves, the XP bar's ends and middle).
        let mut gauge_rects: HashMap<(u8, u32), [f32; 4]> = HashMap::new();
        for w in layout.widgets.iter().filter(|w| !w.hidden) {
            if let (Some(g), Some(art)) = (gauge_of(&w.id), &w.art) {
                if g == Gauge::Resource && !w.id.contains(&format!("/{wanted_resource}/")) {
                    continue;
                }
                let b = bounds(&art.pos);
                let e = gauge_rects.entry((g.key(), w.k.to_bits())).or_insert(b);
                *e = [e[0].min(b[0]), e[1].min(b[1]), e[2].max(b[2]), e[3].max(b[3])];
            }
        }
        for w in draw_order(&layout.widgets).into_iter().map(|i| &layout.widgets[i]).filter(|w| !w.hidden) {
            if layout.window == HOVER_WINDOW && !hover_part(&w.id) {
                continue;
            }
            if layout.window == DEATH_WINDOW && !death_part(&w.id) {
                continue;
            }
            if let Some(slot) = quick_slot_of(&w.id) {
                controls.region(w.k, w.rect, Gate::default());
                let p = parent(&mut commands, w.k, true);
                commands.spawn((QuickSlot { slot, k: w.k, rect: slot_icon_rect(slot, w.rect), shown: None }, Transform::from_xyz(0.0, 0.0, WINDOW_ITEMS_DEPTH), Visibility::Inherited, ChildOf(p)));
            }
            // The status effects' slots: no artwork and no hit region (click-through).
            if let Some((row, slot)) = controls::effects::slot_of(&w.id) {
                let p = parent(&mut commands, w.k, true);
                commands.spawn((controls::effects::EffectSlot { row, slot, k: w.k, rect: w.rect }, controls::effects::Drawn::default(),
                    Transform::from_xyz(0.0, 0.0, WINDOW_ITEMS_DEPTH), Visibility::Inherited, ChildOf(p)));
            }
            let gauge = gauge_of(&w.id);
            let other_class = gauge == Some(Gauge::Resource) && !w.id.contains(&format!("/{wanted_resource}/"));
            if other_class {
                continue;
            }
            // The character sheet's class emblem is authored as the warrior's
            // (emblem_class_dragonknight): the player's own class's instead.
            // FAILURE: a mage's sheet showed the warrior logo (2026-10-21).
            let emblem;
            let w = if w.id.ends_with("/CharacterInfoArea/ClassIcon") && w.art.is_some() {
                let mut own = w.clone();
                if let Some(art) = own.art.as_mut() { art.tex = Some(format!("textures/ui/emblem_class_{}.png", class_emblem(class))); }
                emblem = own;
                &emblem
            } else { w };
            let gate = controls.gate(&layout.window, w);
            if let Some(art) = &w.art {
                if !passes_pointer(&layout.window, w, art) {
                    controls.region(w.k, bounds(&art.pos), gate.clone());
                }
                depth += depth_step;
                if w.kind == "Slot" { controls.set_depth(&w.id, depth); }
                if let Some(which) = gauge {
                    // Its own mesh and material, cut to the fill.
                    let k = w.k;
                    let p = parent(&mut commands, k, true);
                    let mut r = Run { k, tex: art.tex.clone(), ..default() };
                    r.push(art, depth);
                    let m = materials.add(material(atlas(&art.tex)));
                    let rect = gauge_rects.get(&(which.key(), k.to_bits())).copied().unwrap_or_else(|| bounds(&art.pos));
                    commands.spawn((
                        Mesh2d(meshes.add(r.mesh())),
                        MeshMaterial2d(m),
                        Fill { which, k, rect, shown: -1.0 },
                        Transform::from_xyz(0.0, 0.0, depth),
                        RenderLayers::layer(LAYER),
                        ChildOf(p),
                        Element { root, gate: gate.clone() },
                        Visibility::Hidden,
                    ));
                } else if let Some(row) = controls::settings::row_index(w).filter(|_| layout.window == controls::settings::WINDOW) {
                    let p = parent(&mut commands, w.k, true);
                    let mut r = Run::default();
                    r.push(art, depth);
                    let mut mesh = r.mesh();
                    mesh.asset_usage |= RenderAssetUsages::MAIN_WORLD;
                    commands.spawn((Mesh2d(meshes.add(mesh)), MeshMaterial2d(materials.add(material(None))),
                        controls::settings::RowArt { row, rect: w.rect, original: art.clone(), depth, shown: None },
                        Transform::from_xyz(0.0, 0.0, depth), RenderLayers::layer(LAYER), ChildOf(p),
                        Element { root, gate: gate.clone() }, Visibility::Hidden));
                } else {
                    let same = run.as_ref().is_some_and(|r| r.k == w.k && r.tex == art.tex && r.gate == gate);
                    if !same {
                        let prev = run.take();
                        if let Some(prev_k) = prev.as_ref().map(|r| r.k) {
                            let p = parent(&mut commands, prev_k, true);
                            flush(&mut commands, prev, p, &mut meshes, &mut materials);
                        }
                        run = Some(Run { k: w.k, tex: art.tex.clone(), gate: gate.clone(), ..default() });
                    }
                    run.as_mut().unwrap().push(art, depth);
                }
            }
            if let (true, Some(label)) = (layout.window == CHAT_WINDOW && w.id.ends_with("/List/TextLine"), &w.text) {
                // The log: the template row repeated down the List.
                let list = w.id.trim_end_matches("/TextLine");
                let bottom = layout.widgets.iter().find(|l| l.id == list).map_or(w.rect[3], |l| l.rect[3]);
                let [l, t, _, b] = w.rect;
                let height = (b - t).max(1.0);
                let p = parent(&mut commands, w.k, false);
                let preview_bottom = layout.widgets.iter().find(|w| w.event == "SwitchToFull").map_or(bottom, |w| w.rect[3]);
                for compact in [false, true] {
                    let count = if compact { CHAT_PREVIEW_ROWS } else { ((bottom - t) / height).floor().max(1.0) as usize };
                    // Beside the expand button, ending just above the full entry bar.
                    let top = if compact { preview_bottom - height * count as f32 } else { t };
                    for row in 0..count {
                        commands.spawn((
                            ChatRow { index: row, compact },
                            Text2d::new(""),
                            TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(label.size * FONT_SCALE * screen.scale), ..default() },
                            TextColor(Color::WHITE),
                            Anchor(Vec2::new(-0.5, 0.0)),
                            Transform::from_xyz(l + 2.0, -(top + height * (row as f32 + 0.5)), depth + depth_step * 0.5).with_scale(Vec3::splat(1.0 / screen.scale)),
                            RenderLayers::layer(LAYER),
                            ChildOf(p),
                            Element { root, gate: Gate::chat(compact) },
                            Visibility::Hidden,
                        ));
                    }
                }
            }
            if let Some(label) = &w.text {
                // Dialogue owns these labels, including the trading page
                // counter. Do not bind them to inventory/global HUD values.
                let value = if label.key.starts_with("gui.runtime") { None } else { value_of(&w.id) };
                // The stickerbook's authored empty-state labels are literal
                // English text, unlike the usual gui.* localization keys.
                let stickerbook_text = layout.window == "collectorsalbum"
                    && matches!(w.id.rsplit('/').next(), Some("TextLabel_fav" | "TextLabel_info"));
                if !(static_text(label) || stickerbook_text || value.is_some() || w.kind == "TextEntry") || label.color[3] <= 0.0 {
                    continue;
                }
                // The status effects' time labels stay click-through, as authored,
                // and so does the item received notice.
                if !matches!(value, Some(ValueKind::EffectTime(..))) && layout.window != controls::message::WINDOW {
                    controls.region(w.k, w.rect, gate.clone());
                }
                let p = parent(&mut commands, w.k, false);
                let [l, t, r, b] = w.rect;
                let x = match label.align[0] { 0 => l, 2 => r, _ => (l + r) * 0.5 };
                let y = match label.align[1] { 0 => t, 2 => b, _ => (t + b) * 0.5 };
                let anchor = Anchor(Vec2::new(
                    match label.align[0] { 0 => -0.5, 2 => 0.5, _ => 0.0 },
                    match label.align[1] { 0 => 0.5, 2 => -0.5, _ => 0.0 },
                ));
                let bound = value.is_some();
                let mut e = commands.spawn((
                    Text2d::new(if value.is_some() { String::new() } else { label.text.clone() }),
                    // Every label of the 2018 layouts is font "tahoma", drawn thin
                    // whatever its "bold" flag (recorded): Wine's Tahoma
                    // (assets/fonts/tahoma.ttf, copied by hand).
                    TextFont {
                        font: assets.load("fonts/tahoma.ttf").into(),
                        font_size: bevy::text::FontSize::Px(label.size * FONT_SCALE * screen.scale),
                        ..default()
                    },
                    TextColor(Color::srgba(label.color[0], label.color[1], label.color[2], label.color[3])),
                    anchor,
                    Transform::from_xyz(x, -y, depth + depth_step * 0.5).with_scale(Vec3::splat(1.0 / screen.scale)),
                    RenderLayers::layer(LAYER),
                    ChildOf(p),
                    Element { root, gate: gate.clone() },
                    Visibility::Hidden,
                ));
                if let Some(v) = value {
                    e.insert(Value(v));
                }
                // The money shrinks into its place (fit_texts).
                if let Some(width) = controls::currency::fit_width(&layout.window, w) {
                    e.insert(Fit { width, size: label.size * FONT_SCALE });
                }
                let wrap = label.word_break.then(|| (bevy::text::TextBounds::new_horizontal((r - l) * screen.scale),
                    TextLayout::justify(match label.align[0] { 0 => Justify::Left, 2 => Justify::Right, _ => Justify::Center })));
                if let Some(wrap) = wrap.clone() { e.insert(wrap); }
                if w.kind == "TextEntry" { e.insert(controls::EntryLabel(w.id.clone())); }
                if layout.window == controls::settings::WINDOW && w.id.contains("/Runtime/") {
                    e.insert(controls::settings::Caption(w.id.rsplit('/').next().unwrap_or("").to_owned()));
                }
                // The outline: copies of the text behind it in the outline's colour
                // (follow_outlines keeps them on the label's text, size and colour).
                if let Some((radius, raw)) = outline_of(label) {
                    let main = e.id();
                    let color = raw.with_alpha(raw.alpha() * label.color[3]);
                    for (offset, shadow) in outline_copies(radius) {
                        let copy = OutlineCopy { offset, shadow, dz: -depth_step * 0.25, color: raw };
                        let mut c = commands.spawn((
                            Text2d::new(if bound { String::new() } else { label.text.clone() }),
                            TextFont {
                                font: assets.load("fonts/tahoma.ttf").into(),
                                font_size: bevy::text::FontSize::Px(label.size * FONT_SCALE * screen.scale),
                                ..default()
                            },
                            TextColor(color),
                            anchor,
                            copy.transform(screen.scale),
                            bevy::sprite::Text2dShadow { offset: shadow * screen.scale, color },
                            RenderLayers::layer(LAYER),
                            ChildOf(main),
                            Visibility::Inherited,
                            copy,
                        ));
                        if let Some(wrap) = wrap.clone() { c.insert(wrap); }
                    }
                }
            }
        }
        if let Some(prev_k) = run.as_ref().map(|r| r.k) {
            let p = parent(&mut commands, prev_k, true);
            flush(&mut commands, run.take(), p, &mut meshes, &mut materials);
        }
        commands.entity(root).insert((controls, crate::ui::input::UiRegions::default()));
    }
}

/// Whether a widget's artwork leaves the pointer to what lies under it.
/// The chat windows' and the quest tracker's invisible containers (no texture,
/// fully transparent) span far beyond what they show: the chat's above its log,
/// the tracker's LabelFull / LabelCompact from the top of the screen down over
/// the game bar's buttons. The NPC quest backdrop shades the world but is
/// explicitly click-through; walking outside the conversation still works.
/// FAILURE (2026-10-21): with the tracker's containers taking the pointer (the
///   tracker is drawn over the game bar), the Premium, autoplay, fullscreen and
///   game menu buttons above it could not be hovered or clicked.
/// The item received notice (controls::message) takes no clicks at all.
fn passes_pointer(window: &str, w: &Widget, art: &Art) -> bool {
    let invisible = art.tex.is_none() && art.color.chunks(4).all(|c| c.get(3).is_some_and(|a| *a <= 0.0));
    (dialogue::WINDOWS.contains(&window) && w.click_through)
        || (invisible && matches!(window, CHAT_WINDOW | CHAT_MENU | controls::quests::TRACKER))
        || window == controls::message::WINDOW
}

/// The widgets' drawing order: as authored, except a button state's captions,
/// which the game draws over the state's artwork (its relief shading and
/// frame follow the caption in the layouts and would darken it).
fn draw_order(widgets: &[Widget]) -> Vec<usize> {
    let mut keys: Vec<(f64, usize)> = Vec::with_capacity(widgets.len());
    for (i, w) in widgets.iter().enumerate() {
        let mut key = i as f64;
        if w.state.is_some() && w.text.is_some() && w.art.is_none() {
            if let Some((parent, _)) = w.id.rsplit_once('/') {
                let prefix = format!("{parent}/");
                let end = widgets[i + 1..].iter().take_while(|n| n.id.starts_with(&prefix)).count();
                key = (i + end) as f64 + 0.5 + i as f64 * 1e-7;
            }
        }
        keys.push((key, i));
    }
    keys.sort_by(|a, b| a.0.total_cmp(&b.0));
    keys.into_iter().map(|(_, i)| i).collect()
}

/// A label's outline (TextEffect 1): its radius in reference pixels (following
/// the outline's size) and its colour.
fn outline_of(label: &Label) -> Option<(f32, Color)> {
    let [size, r, g, b, a] = label.outline[..] else { return None };
    if size <= 0.0 || a <= 0.0 { return None; }
    Some(((0.5 + 0.35 * size).min(1.5), Color::srgba(r, g, b, a)))
}

/// The outline's copies: each drawn at an offset and, by its shadow, at the
/// opposite one -- eight directions around the glyphs from four copies
/// (reference pixels).
fn outline_copies(radius: f32) -> [(Vec2, Vec2); 4] {
    let d = radius * std::f32::consts::FRAC_1_SQRT_2;
    [(Vec2::new(-radius, 0.0), Vec2::new(2.0 * radius, 0.0)), (Vec2::new(0.0, -radius), Vec2::new(0.0, 2.0 * radius)),
        (Vec2::new(-d, -d), Vec2::new(2.0 * d, 2.0 * d)), (Vec2::new(-d, d), Vec2::new(2.0 * d, -2.0 * d))]
}

/// A copy of an outlined label (its child): where it sits and its shadow falls
/// (reference pixels), how far behind the label (world), the outline's colour.
#[derive(Component, Clone, Copy)]
struct OutlineCopy {
    offset: Vec2,
    shadow: Vec2,
    dz: f32,
    color: Color,
}

impl OutlineCopy {
    /// Its transform under the label (scaled by 1 / scale).
    fn transform(&self, scale: f32) -> Transform {
        Transform::from_translation((self.offset * scale).extend(self.dz * scale))
    }
}

/// The outline copies follow their label: its text, size, wrap width and
/// colour's alpha (a resize, a bound value, a faded label).
#[allow(clippy::type_complexity)]
fn follow_outlines(
    screen: Res<Screen>,
    labels: Query<(&Text2d, &TextFont, &TextColor, &bevy::text::TextBounds, &Children),
        (Without<OutlineCopy>, Or<(Changed<Text2d>, Changed<TextFont>, Changed<TextColor>, Changed<bevy::text::TextBounds>)>)>,
    mut copies: Query<(&OutlineCopy, &mut Text2d, &mut TextFont, &mut TextColor, &mut bevy::text::TextBounds, &mut Transform, &mut bevy::sprite::Text2dShadow)>,
) {
    for (text, font, color, bounds, children) in &labels {
        for child in children.iter() {
            let Ok((copy, mut t, mut f, mut c, mut b, mut tf, mut shadow)) = copies.get_mut(child) else { continue };
            if t.0 != text.0 { t.0.clone_from(&text.0); }
            f.font_size = font.font_size;
            *b = *bounds;
            let faded = copy.color.with_alpha(copy.color.alpha() * color.0.alpha());
            if c.0 != faded { c.0 = faded; shadow.color = faded; }
            let placed = copy.transform(screen.scale);
            if tf.translation != placed.translation { *tf = placed; }
            let offset = copy.shadow * screen.scale;
            if shadow.offset != offset { shadow.offset = offset; }
        }
    }
}

fn bounds(pos: &[f32]) -> [f32; 4] {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in pos.chunks_exact(2) {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    }
    b
}

fn follow_transform(screen: &Screen, k: f32, flip: bool) -> Transform {
    let o = screen.world(k, 0.0, 0.0);
    Transform { translation: o.extend(0.0), scale: Vec3::new(1.0, if flip { -1.0 } else { 1.0 }, 1.0), ..default() }
}

/// The window's size: the reference width and the text resolution.
#[allow(clippy::type_complexity)]
fn follow_screen(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut resized: MessageReader<WindowResized>,
    mut screen: ResMut<Screen>,
    mut follows: Query<(&Follows, &mut Transform), Without<Text2d>>,
    mut texts: Query<(&mut TextFont, &mut Transform, &ChildOf, &mut bevy::text::TextBounds, Option<&mut bevy::sprite::Text2dShadow>), With<Text2d>>,
    labels: Query<(), With<Follows>>,
    mut fills: Query<&mut Fill>,
    mut first: Local<bool>,
) {
    let changed = resized.read().count() > 0 || !*first;
    let Ok(window) = windows.single() else { return };
    if !changed || window.height() <= 0.0 {
        return;
    }
    *first = true;
    let new = Screen { width: BASE_H * window.width() / window.height(), scale: window.physical_height() as f32 / BASE_H };
    let old_scale = screen.scale;
    *screen = new;
    for (f, mut tf) in &mut follows {
        *tf = follow_transform(&new, f.k, f.flip);
    }
    if (old_scale - new.scale).abs() > 1e-3 {
        for (mut font, mut tf, parent, mut bounds, shadow) in &mut texts {
            if labels.contains(parent.parent()) {
                if let bevy::text::FontSize::Px(px) = font.font_size {
                    font.font_size = bevy::text::FontSize::Px(px / old_scale * new.scale);
                }
                tf.scale = Vec3::splat(1.0 / new.scale);
                if let Some(width) = &mut bounds.width { *width *= new.scale / old_scale; }
                if let Some(mut shadow) = shadow { shadow.offset *= new.scale / old_scale; }
            }
        }
    }
    // Clip rectangles are in world space: re-cut.
    for mut f in &mut fills {
        f.shown = -1.0;
    }
}

/// The server's figures, or a resting HUD offline.
fn read_figures(net: Option<NonSend<crate::network::net::Net>>, mut figures: ResMut<Figures>, resource_max: Res<crate::gameplay::stats::ResourceMax>,
    mut wallet_seen: Local<Option<u64>>) {
    let Some(net) = net else {
        if !figures.online && figures.health.1 == 0.0 {
            // Offline (the map viewer): a full-looking HUD, so it can be seen.
            figures.health = (80.0, 100.0);
            figures.resource = (60.0, 100.0);
            figures.xp = 0.35;
            figures.level = Some(1);
            figures.wisdom = (0, 0.0);
            figures.bar = ["magicmissile", "fireball", "frostnova", "iceball", "meteor", "teleport", "lightningstrike"]
                .iter()
                .map(|s| Some((*s).to_owned()))
                .collect();
            figures.name = "DSOR".to_owned();
        }
        return;
    };
    if !figures.online {
        figures.online = true;
        figures.health = (0.0, 0.0);
        figures.resource = (0.0, 0.0);
        figures.xp = 0.0;
        figures.wisdom = (0, 0.0);
        figures.wisdom_points = 0;
        figures.bar.clear();
    }
    // The server gives the current values; the maxima are the largest seen
    // (UNVERIFIED: ActorStatsUpdate's max fields are not decoded yet).
    if let Some(h) = net.health {
        let max = net.max_health.unwrap_or(0.0).max(figures.health.1).max(h);
        if figures.health != (h, max) {
            figures.health = (h, max);
        }
    }
    // DSOR_TEST_RESOURCE=<n>: offline, the resource gauge at n (screenshots).
    let test = net.offline.then(|| std::env::var("DSOR_TEST_RESOURCE").ok().and_then(|v| v.parse::<f32>().ok())).flatten();
    if let Some(r) = net.resource.or(test) {
        // Against the class's maximum (rage starts empty: the largest value seen
        // made 10 rage look like a full bar).
        let max = resource_max.0.max(r);
        if figures.resource != (r, max) {
            figures.resource = (r, max);
        }
    }
    if figures.level != net.level {
        figures.level = net.level;
    }
    if let Some((total, floor, ceiling)) = net.xp {
        let f = if ceiling > floor { (total.saturating_sub(floor)) as f32 / (ceiling - floor) as f32 } else { 0.0 };
        if figures.xp != f {
            figures.xp = f;
        }
        if figures.xp_points != Some((total, ceiling)) {
            figures.xp_points = Some((total, ceiling));
        }
    }
    // The currencies come with server commands: read again when one came.
    if *wallet_seen != Some(net.ui_revision) {
        *wallet_seen = Some(net.ui_revision);
        let wallet = controls::currency::Wallet::of(&net);
        if figures.wallet != wallet {
            figures.wallet = wallet;
        }
    }
    if figures.bar != net.bar {
        figures.bar = net.bar.clone();
    }
    if let Some(n) = &net.local_name {
        if &figures.name != n {
            figures.name = n.clone();
        }
    }
}

fn fraction(v: (f32, f32)) -> f32 {
    if v.1 > 0.0 { (v.0 / v.1).clamp(0.0, 1.0) } else { 0.0 }
}

fn update_fills(
    figures: Res<Figures>,
    party: Res<controls::party::Party>,
    hover: Res<crate::actors::monsters::HoverInfo>,
    screen: Res<Screen>,
    mut fills: Query<(&mut Fill, &MeshMaterial2d<UiMaterial>, &Element)>,
    roots: Query<Ref<Transform>, With<Window2018>>,
    mut materials: ResMut<Assets<UiMaterial>>,
) {
    for (mut f, m, element) in &mut fills {
        let Ok(root) = roots.get(element.root) else { continue };
        let v = match f.which {
            Gauge::Health | Gauge::HealthBar => fraction(figures.health),
            Gauge::Resource | Gauge::ResourceBar => fraction(figures.resource),
            Gauge::Xp => figures.xp.clamp(0.0, 1.0),
            Gauge::Wisdom => figures.wisdom.1.clamp(0.0, 1.0),
            Gauge::TargetHealth => hover.0.as_ref().map(|h| fraction((h.2 as f32, h.3 as f32))).unwrap_or(0.0),
            Gauge::Group(n) => party.health(n as usize),
        };
        if (v - f.shown).abs() < 1e-3 && !root.is_changed() {
            continue;
        }
        f.shown = v;
        let [l, t, r, b] = f.rect;
        // Globes fill from the bottom; the XP bar from the left.
        let (min, max) = match f.which {
            Gauge::Health | Gauge::Resource => (screen.world(f.k, l, b), screen.world(f.k, r, b - (b - t) * v)),
            _ => (screen.world(f.k, l, b), screen.world(f.k, l + (r - l) * v, t)),
        };
        let offset = root.translation.truncate();
        let (min, max) = (min + offset, max + offset);
        if let Some(mut mat) = materials.get_mut(&m.0) {
            mat.clip = Vec4::new(min.x, min.y, max.x, max.y);
        }
    }
}

fn update_values(
    figures: Res<Figures>,
    settings: Res<Settings>,
    hotbar: Res<controls::hotbar::Hotbar>,
    sheet: Res<crate::gameplay::stats::Sheet>,
    hover: Res<crate::actors::monsters::HoverInfo>,
    current_map: Option<Res<crate::world::map::CurrentMap>>,
    shown: Res<controls::currency::Shown>,
    mut texts: Query<(&Value, &mut Text2d)>,
) {
    let changed = figures.is_changed() || settings.is_changed() || hotbar.is_changed() || sheet.is_changed() || hover.is_changed() || current_map.as_ref().is_some_and(|m| m.is_changed())
        || shown.is_changed();
    for (v, mut t) in &mut texts {
        // Layouts load asynchronously, even when the current figures are stable.
        if !changed && !t.is_added() { continue; }
        // Gold is counted in copper: 100 copper a silver, 100 silver a gold;
        // each shortened to its place on the currency bar.
        // EVIDENCE: release261 UI::CurrencyBarWidget 0x1403DAA44 (copper / 10000
        //   in 5 digits, the rest's silver and copper), 0x1403DA7F0 (Andermant,
        //   9), 0x1403DAE14 (the picked currency, 10).
        let wallet = &figures.wallet;
        let s = match v.0 {
            ValueKind::Hotkey(slot) => settings.current.slot_label(slot),
            ValueKind::EssenceKey => settings.current.label("essence"),
            ValueKind::BarNumber => (hotbar.active + 1).to_string(),
            ValueKind::Fixed(n) => n.to_string(),
            ValueKind::Gold => controls::currency::clamped(wallet.copper / 10_000, 5),
            ValueKind::Silver => (wallet.copper % 10_000 / 100).to_string(),
            ValueKind::Copper => (wallet.copper % 100).to_string(),
            ValueKind::Andermant => controls::currency::clamped(wallet.andermant, 9),
            ValueKind::Picked => controls::currency::clamped(wallet.balance(&shown.id), 10),
            ValueKind::Level => figures.level.map(|l| l.to_string()).unwrap_or_else(|| "-".to_owned()),
            ValueKind::XpPercent | ValueKind::WisdomProgress => String::new(),
            ValueKind::WisdomLevel => figures.wisdom.0.to_string(),
            ValueKind::BarLevel => figures.level.map(|l| format!("Level {l}")).unwrap_or_default(),
            ValueKind::BarWisdomLevel => format!("Wisdom Level {}", figures.wisdom.0),
            ValueKind::WisdomPoints => format!("Available Points: {}", figures.wisdom_points),
            ValueKind::LocationName => current_map.as_ref().map(|m| m.name.split_once('_').map_or(m.name.as_str(), |(_, name)| name).replace('_', " ")).unwrap_or_else(|| "Map".to_owned()),
            ValueKind::Name => figures.name.clone(),
            // Each window owns its current page (controls::paint).
            ValueKind::Page => continue,
            // Written as they tick down (controls::effects::draw).
            ValueKind::EffectTime(..) => continue,
            ValueKind::TargetName => hover.0.as_ref().map(|h| h.0.clone()).unwrap_or_default(),
            ValueKind::TargetLevel => hover.0.as_ref().map(|h| h.1.to_string()).unwrap_or_default(),
            ValueKind::Sheet(w) => match sheet.0.get(w) {
                Some(v) => v.clone(),
                None => continue,
            },
        };
        if t.0 != s {
            t.0 = s;
        }
    }
}

/// The money labels' font, shrunk until their text fits their place; only when
/// a text changes (a resize scales the font with the rest: follow_screen).
fn fit_texts(screen: Res<Screen>, mut texts: Query<(&Fit, &Text2d, &mut TextFont), Changed<Text2d>>) {
    for (fit, text, mut font) in &mut texts {
        let px = controls::currency::fitted(&text.0, fit.size, fit.width) * screen.scale;
        if !matches!(font.font_size, bevy::text::FontSize::Px(old) if (old - px).abs() < 1e-3) {
            font.font_size = bevy::text::FontSize::Px(px);
        }
    }
}

/// Runtime slot contents use the full authored socket, with counts above the art.
#[allow(clippy::too_many_arguments)]
fn update_slots(
    mut commands: Commands,
    figures: Res<Figures>,
    loaded: Res<Loaded>,
    icon_table: Res<Assets<SkillIcons>>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<UiMaterial>>,
    mut slots: Query<(Entity, &mut QuickSlot)>,
    net: Option<NonSend<crate::network::net::Net>>,
    hotbar: Res<controls::hotbar::Hotbar>,
    data: Res<controls::items::Data>,
    catalogs: Res<Assets<Catalog>>,
    screen: Res<Screen>,
    (skill_data, skill_tables, resource_max, char_stats): (Res<crate::gameplay::skills::SkillData>, Res<Assets<crate::gameplay::skills::SkillTable>>, Res<crate::gameplay::stats::ResourceMax>, Res<crate::gameplay::stats::CharacterStats>),
    (cooldowns, time, pointer): (Res<crate::gameplay::skills::SkillCooldowns>, Res<Time<Real>>, Res<controls::Pointer>),
) {
    let Some(table) = icon_table.get(&loaded.icons) else { return };
    let catalog = catalogs.get(&data.catalog);
    let resource = net.as_ref().and_then(|n| n.resource);
    let now = time.elapsed_secs_f64();
    for (e, mut s) in &mut slots {
        let skill = controls::hotbar::skill_id(s.slot, hotbar.active, net.as_deref(), &figures);
        // A skill the resource does not cover is greyed, as it cannot be cast
        // (crate::gameplay::skills cast_input refuses it), for every class.
        let dim = resource.is_some_and(|r| skill.as_deref()
            .and_then(|id| crate::gameplay::skills::skill_cost_by_id(&skill_data, &skill_tables, id, resource_max.0))
            .map(|cost| cost * char_stats.skill_cost_factor())
            .is_some_and(|cost| r + 1e-3 < cost));
        // A skill cooling down (cast_input refuses it until then) gets a dark
        // sweep over its icon, redrawn in COOLDOWN_STEPS steps as it turns.
        let cool = skill.as_deref().map_or(0.0, |id| cooldowns.remaining(&skill_data, &skill_tables, id, now));
        let step = (cool * COOLDOWN_STEPS as f32).ceil().min(COOLDOWN_STEPS as f32) as u8;
        // And the seconds left on it, as the original's "21 s".
        // FAILURE (2026-10-24): "the skills still don't have a cooldown": only the
        //   sweep showed, hard to see on dark icons.
        let tenths = skill.as_deref().map_or(0, |id| crate::gameplay::skills::cooldown_tenths(cooldowns.seconds_left(&skill_data, &skill_tables, id, now)));
        // A spell being dragged off this slot leaves it empty meanwhile, as in the game.
        let lifted = pointer.lifted_slot() == Some((hotbar.active, s.slot));
        let want = controls::hotbar::appearance(s.slot, hotbar.active, net.as_deref(), &figures, catalog, table)
            .filter(|_| !lifted)
            .map(|(path, count)| (path, count, dim, step, tenths, controls::hotbar::backdrop(s.slot, hotbar.active, net.as_deref(), catalog)));
        if want == s.shown && !screen.is_changed() { continue; }
        commands.entity(e).despawn_related::<Children>();
        if let Some((path, count, dim, step, tenths, backdrop)) = &want {
            let [l, t, r, b] = s.rect;
            let shade = if *dim { [0.35, 0.35, 0.35, 1.0] } else { [1.0; 4] };
            for (layer, z) in backdrop.iter().map(|b| (b, -0.05)).chain([(path, 0.0)]) {
                let mut run = Run { k:s.k, ..default() }; run.push(&slot_art(s.slot, s.rect, shade), 0.0);
                let m = materials.add(material(Some(assets.load(layer.clone()))));
                commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(m), Transform::from_xyz(0.0, 0.0, z), RenderLayers::layer(LAYER), ChildOf(e)));
            }
            if *step > 0 {
                let mut run = Run { k: s.k, ..default() };
                run.push(&cooldown_sweep(s.rect, *step as f32 / COOLDOWN_STEPS as f32), 0.0);
                commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(materials.add(material(None))),
                    Transform::from_xyz(0.0, 0.0, 0.5), RenderLayers::layer(LAYER), ChildOf(e)));
            }
            // This parent is y-flipped for the artwork; text flips back.
            if *tenths > 0 {
                let text = crate::gameplay::skills::cooldown_text(*tenths);
                let font = TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(11.0 * screen.scale), ..default() };
                let at = Vec3::new((l + r) * 0.5, (t + b) * 0.5, 1.2);
                for (offset, color) in [(Vec3::new(0.8, 0.8, -0.1), Color::BLACK), (Vec3::ZERO, Color::WHITE)] {
                    commands.spawn((Text2d::new(text.clone()), font.clone(), TextColor(color), Anchor::CENTER,
                        Transform::from_translation(at + offset).with_scale(Vec3::new(1.0, -1.0, 1.0) / screen.scale),
                        RenderLayers::layer(LAYER), ChildOf(e)));
                }
            }
            if *count > 0 {
                commands.spawn((Text2d::new(count.to_string()), TextFont { font: assets.load("fonts/tahoma.ttf").into(),
                    font_size: bevy::text::FontSize::Px(10.0 * screen.scale), ..default() }, TextColor(Color::WHITE),
                    Anchor::BOTTOM_RIGHT, Transform::from_xyz(r-1.0,b-1.0,1.0).with_scale(Vec3::new(1.0,-1.0,1.0)/screen.scale),
                    RenderLayers::layer(LAYER), ChildOf(e)));
            }
        }
        s.shown = want;
    }
}

/// What of a slot its picture covers: the round mount and companion sockets (8,
/// 9) a disc inside their ring, every other slot its whole square.
/// FAILURE (2026-10-24): the Manticore's square picture spilled over the mount
///   socket's ring (the user's shot beside the original's round one).
/// UNVERIFIED: the disc's size, matched by eye to the ring's opening.
const ROUND_SLOT: f32 = 0.88;

fn slot_art(slot: usize, rect: [f32; 4], shade: [f32; 4]) -> Art {
    let [l, t, r, b] = rect;
    if !matches!(slot, 8 | 9) {
        return Art { tex: None, pos: vec![l,t,r,t,r,b,l,b], uv: vec![0.0,0.0,1.0,0.0,1.0,1.0,0.0,1.0], color: shade.repeat(4), idx: vec![0,1,2,0,2,3] };
    }
    const SIDES: u32 = 40;
    let (cx, cy, radius) = ((l + r) * 0.5, (t + b) * 0.5, (r - l).min(b - t) * 0.5 * ROUND_SLOT);
    let (mut pos, mut uv, mut idx) = (vec![cx, cy], vec![0.5, 0.5], Vec::new());
    for i in 0..SIDES {
        let (sin, cos) = (i as f32 / SIDES as f32 * std::f32::consts::TAU).sin_cos();
        pos.extend([cx + cos * radius, cy + sin * radius]);
        uv.extend([0.5 + cos * 0.5, 0.5 + sin * 0.5]);
        idx.extend([0, 1 + i, 1 + (i + 1) % SIDES]);
    }
    Art { tex: None, pos, uv, color: shade.repeat(SIDES as usize + 1), idx }
}

/// Steps of the cooldown sweep: a redraw every 1/64 of the cooldown.
const COOLDOWN_STEPS: u32 = 64;

/// The part of a slot still cooling down: a dark sector from 12 o'clock,
/// clockwise, over `remaining` of the turn (it shrinks clockwise as the cooldown
/// runs), cut to the slot's square. `rect` is l, t, r, b with t at the top.
/// Alpha 0.75 (blended as in gamma, ui.wgsl): a quarter of the icon shows, so
/// dark icons (teleport) still read as cooling down; 0.6 was hard to see there.
/// UNVERIFIED: the original's own look (its Slot widget draws it in code).
fn cooldown_sweep(rect: [f32; 4], remaining: f32) -> Art {
    use std::f32::consts::{FRAC_PI_4, TAU};
    let [l, t, r, b] = rect;
    let (cx, cy) = ((l + r) * 0.5, (t + b) * 0.5);
    // Signed half sizes: + towards 3 o'clock and towards 12 o'clock.
    let (hw, hh) = ((r - l) * 0.5, (t - b) * 0.5);
    let start = (1.0 - remaining.clamp(0.0, 1.0)) * TAU;
    // The fan's rim: where the sweep starts, the corners after it, 12 o'clock.
    // Between them the rim is one straight edge, so the fan is exact.
    let corners = [1.0, 3.0, 5.0, 7.0].map(|i| i * FRAC_PI_4).into_iter().filter(|&c| c > start);
    let mut pos = vec![cx, cy];
    for a in std::iter::once(start).chain(corners).chain([TAU]) {
        let (u, v) = (a.sin(), a.cos());
        let s = 1.0 / u.abs().max(v.abs());
        pos.extend([cx + u * s * hw, cy + v * s * hh]);
    }
    let n = (pos.len() / 2) as u32;
    let idx = (1..n - 1).flat_map(|i| [0, i, i + 1]).collect();
    Art { tex: None, uv: vec![0.0; pos.len()], color: [0.0, 0.0, 0.0, 0.75].repeat(n as usize), pos, idx }
}

/// The hovered monster's info window, shown while one is under the cursor.
fn show_hover_window(hover: Res<crate::actors::monsters::HoverInfo>, mut windows: Query<(&Window2018, &mut Visibility, &mut WindowControls)>) {
    if !hover.is_changed() {
        return;
    }
    for (w, mut v, mut controls) in &mut windows {
        if w.0 == HOVER_WINDOW {
            // Its gothic ornament (controls::section, page "hover_boss") on bosses only.
            controls.set_page("hover_boss", u8::from(hover.1));
            let want = if hover.0.is_some() { Visibility::Inherited } else { Visibility::Hidden };
            if *v != want {
                *v = want;
            }
        }
    }
}

/// The death dialog follows the server: shown from ShowDeathDialogCommand until a
/// respawn is chosen or the player is revived (crate::network::net::Net::death).
fn death_dialog(net: Option<NonSend<crate::network::net::Net>>, mut windows: Query<(&Window2018, &mut Visibility)>) {
    // DSOR_UI_OPEN=deathdialog: shown regardless (to look at it offline).
    let wanted = net.as_ref().is_some_and(|n| n.death.dialog && !n.death.requested)
        || std::env::var("DSOR_UI_OPEN").is_ok_and(|v| v.split(',').any(|w| w == DEATH_WINDOW));
    for (w, mut v) in &mut windows {
        if w.0 != DEATH_WINDOW {
            continue;
        }
        let want = if wanted { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

#[cfg(test)]
mod outline_tests {
    use super::*;

    #[test]
    fn outline_copies_cover_eight_directions_at_the_radius() {
        let mut points: Vec<Vec2> = outline_copies(1.0).iter().flat_map(|(offset, shadow)| [*offset, *offset + *shadow]).collect();
        assert!(points.iter().all(|p| (p.length() - 1.0).abs() < 1e-5));
        points.sort_by(|a, b| a.to_angle().total_cmp(&b.to_angle()));
        for pair in points.windows(2) { assert!((pair[1].to_angle() - pair[0].to_angle() - std::f32::consts::FRAC_PI_4).abs() < 1e-4); }
        assert_eq!(points.len(), 8);
    }
}

#[cfg(test)]
mod residency_tests {
    use super::*;

    /// The area a fan covers, and whether every rim point is in the box.
    fn fan(art: &Art) -> (f32, Vec<[f32; 2]>) {
        let p = |i: u32| [art.pos[2 * i as usize], art.pos[2 * i as usize + 1]];
        let area = art.idx.chunks(3).map(|t| {
            let ([ax, ay], [bx, by], [cx, cy]) = (p(t[0]), p(t[1]), p(t[2]));
            ((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)).abs() * 0.5
        }).sum();
        (area, (0..(art.pos.len() / 2) as u32).map(p).collect())
    }

    #[test]
    fn cooldown_sweep_covers_what_is_left_clockwise_from_twelve() {
        // Art coordinates: y down, so the slot's top is t = 0.
        let rect = [0.0, 0.0, 20.0, 20.0];
        let (full, _) = fan(&cooldown_sweep(rect, 1.0));
        assert!((full - 400.0).abs() < 1e-3, "{full}");
        // A quarter left: the part from 9 to 12 o'clock, the top-left quarter.
        let (quarter, points) = fan(&cooldown_sweep(rect, 0.25));
        assert!((quarter - 100.0).abs() < 1e-3, "{quarter}");
        assert!(points.iter().all(|&[x, y]| x <= 10.0 + 1e-4 && y <= 10.0 + 1e-4), "{points:?}");
        // An eighth: the triangle from the top-left corner to 12 o'clock.
        let (eighth, _) = fan(&cooldown_sweep(rect, 0.125));
        assert!((eighth - 50.0).abs() < 1e-3, "{eighth}");
        // Half: 6 to 12 o'clock, the left half.
        let (half, points) = fan(&cooldown_sweep(rect, 0.5));
        assert!((half - 200.0).abs() < 1e-3, "{half}");
        assert!(points.iter().all(|&[x, _]| x <= 10.0 + 1e-4), "{points:?}");
    }

    #[test]
    fn overlay_tracks_scene_texture_usages_and_format() {
        use bevy::camera::CameraMainTextureUsages;
        use bevy::render::render_resource::TextureUsages;
        let mut app = App::new();
        app.add_systems(Update, match_scene_camera);
        let usages = CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING);
        let scene = app.world_mut().spawn((Camera3d::default(), Hdr, Msaa::Off, usages)).id();
        let overlay = app.world_mut().spawn((Camera2d, UiCamera, Msaa::Sample4)).id();
        app.update();
        assert!(app.world().get::<Hdr>(overlay).is_some());
        assert_eq!(app.world().get::<Msaa>(overlay), Some(&Msaa::Off));
        assert_eq!(app.world().get::<CameraMainTextureUsages>(overlay).unwrap().0, usages.0);
        app.world_mut().entity_mut(scene).remove::<Hdr>().insert(CameraMainTextureUsages::default());
        app.update();
        assert!(app.world().get::<Hdr>(overlay).is_none());
        assert_eq!(app.world().get::<CameraMainTextureUsages>(overlay).unwrap().0, CameraMainTextureUsages::default().0);
    }

    #[test]
    fn overlay_takes_a_window_size_set_in_code_with_the_scene_camera() {
        use bevy::window::{WindowCreated, WindowScaleFactorChanged};
        let mut app = App::new();
        app.add_message::<WindowResized>().add_message::<WindowCreated>().add_message::<WindowScaleFactorChanged>()
            .add_message::<bevy::asset::AssetEvent<Image>>()
            .init_resource::<Assets<Image>>().init_resource::<bevy::render::texture::ManualTextureViews>()
            .add_systems(PostUpdate, (follow_window_size.before(bevy::camera::CameraUpdateSystems),
                bevy::render::camera::camera_system.in_set(bevy::camera::CameraUpdateSystems)));
        let window = app.world_mut().spawn((Window::default(), PrimaryWindow)).id();
        app.world_mut().get_mut::<Window>(window).unwrap().resolution.set(1920.0, 1080.0);
        let scene = app.world_mut().spawn(Camera3d::default()).id();
        let overlay = app.world_mut().spawn((Camera2d, UiCamera)).id();
        app.update();
        let sizes = |app: &App| [scene, overlay].map(|e| app.world().get::<Camera>(e).unwrap().physical_target_size());
        assert_eq!(sizes(&app), [Some(UVec2::new(1920, 1080)); 2]);
        // A window change that keeps the size (the cursor moving) touches no camera.
        let before = app.world().entity(overlay).get_ref::<Projection>().unwrap().last_changed();
        app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(Vec2::new(10.0, 10.0)));
        app.update();
        assert_eq!(app.world().entity(overlay).get_ref::<Projection>().unwrap().last_changed(), before);
        // Smaller, then larger: the size is set in code and no WindowResized
        // arrives yet; the game writes the scene camera's projection every frame.
        for (w, h) in [(1024, 768), (1600, 900)] {
            app.world_mut().get_mut::<Window>(window).unwrap().resolution.set(w as f32, h as f32);
            app.world_mut().get_mut::<Projection>(scene).unwrap().set_changed();
            app.update();
            assert_eq!(sizes(&app), [Some(UVec2::new(w, h)); 2]);
        }
    }

    #[test]
    fn unopened_menu_has_no_art_and_first_open_builds_it_once() {
        let mut app=App::new();
        app.add_plugins((MinimalPlugins,bevy::asset::AssetPlugin::default()))
            .init_asset::<Layout>().init_asset::<Mesh>().init_asset::<UiMaterial>()
            .init_asset::<Image>().init_asset::<Font>().init_asset::<dialogue::Catalog>()
            .init_resource::<Loaded>().init_resource::<Screen>().init_resource::<dialogue::Presentation>().init_resource::<controls::party::Party>().init_resource::<controls::quests::Quests>().init_resource::<crate::gameplay::quests::QuestUi>().init_resource::<controls::systems::Views>().init_resource::<controls::currency::Shown>().init_resource::<controls::message::ItemMessages>().init_resource::<UiWork>()
            .add_systems(Update,spawn_windows);
        let layout: Layout=serde_json::from_value(serde_json::json!({"window":"inventory","widgets":[{
            "id":"Canvas/background","k":0.0,"rect":[0,0,20,20],"hidden":false,
            "art":{"tex":null,"pos":[0,0,20,0,0,20],"uv":[0,0,1,0,0,1],"color":[1,1,1,1,1,1,1,1,1,1,1,1],"idx":[0,1,2]}
        }]})).unwrap();
        let handle=app.world_mut().resource_mut::<Assets<Layout>>().add(layout);
        app.world_mut().resource_mut::<Loaded>().layouts.push((handle,100.,None));
        app.world_mut().resource_mut::<Loaded>().spawned.push(false);
        app.update(); app.update();
        let root=app.world_mut().query_filtered::<Entity,With<Window2018>>().single(app.world()).unwrap();
        assert!(app.world().get::<WindowControls>(root).is_some());
        assert_eq!(app.world_mut().query::<&Mesh2d>().iter(app.world()).count(),0);
        assert!(app.world().get::<Children>(root).is_none());
        *app.world_mut().get_mut::<Visibility>(root).unwrap()=Visibility::Inherited;
        app.update();
        assert_eq!(app.world_mut().query::<&Mesh2d>().iter(app.world()).count(),1);
        for visibility in [Visibility::Hidden,Visibility::Inherited,Visibility::Hidden,Visibility::Inherited] {
            *app.world_mut().get_mut::<Visibility>(root).unwrap()=visibility;
            app.update();
        }
        assert_eq!(app.world_mut().query::<&Mesh2d>().iter(app.world()).count(),1);
        assert_eq!(app.world_mut().query::<&Window2018>().iter(app.world()).count(),1);
    }

    /// docs/ui.md, "Per-frame cost". FAILURE (2026-10-21): every closed,
    /// never-opened menu's layout was prepared each frame, then dropped (6 ms a
    /// frame with the systems' windows: 180 -> 95 FPS).
    #[test]
    fn closed_windows_are_not_prepared_every_frame() {
        let mut app=App::new();
        app.add_plugins((MinimalPlugins,bevy::asset::AssetPlugin::default()))
            .init_asset::<Layout>().init_asset::<Mesh>().init_asset::<UiMaterial>()
            .init_asset::<Image>().init_asset::<Font>().init_asset::<dialogue::Catalog>()
            .init_resource::<Loaded>().init_resource::<Screen>().init_resource::<dialogue::Presentation>().init_resource::<controls::party::Party>().init_resource::<controls::quests::Quests>().init_resource::<crate::gameplay::quests::QuestUi>().init_resource::<controls::systems::Views>().init_resource::<controls::currency::Shown>().init_resource::<controls::message::ItemMessages>().init_resource::<UiWork>()
            .add_systems(Update,spawn_windows);
        let layout: Layout=serde_json::from_value(serde_json::json!({"window":controls::systems::ALBUM,"widgets":[{
            "id":"Canvas/background","k":0.0,"rect":[0,0,20,20],"hidden":false,
            "art":{"tex":null,"pos":[0,0,20,0,0,20],"uv":[0,0,1,0,0,1],"color":[1,1,1,1,1,1,1,1,1,1,1,1],"idx":[0,1,2]}
        }]})).unwrap();
        let handle=app.world_mut().resource_mut::<Assets<Layout>>().add(layout);
        app.world_mut().resource_mut::<Loaded>().layouts.push((handle,100.,None));
        app.world_mut().resource_mut::<Loaded>().spawned.push(false);
        let prepared=|app: &App| app.world().resource::<UiWork>().layouts_prepared;
        app.update();
        let root=app.world_mut().query_filtered::<Entity,With<Window2018>>().single(app.world()).unwrap();
        assert_eq!(app.world().get::<Visibility>(root),Some(&Visibility::Hidden));
        assert_eq!(prepared(&app),1,"once, for the closed window's control model");
        for _ in 0..10 { app.update(); }
        assert_eq!(prepared(&app),1,"a closed window costs nothing a frame");
        *app.world_mut().get_mut::<Visibility>(root).unwrap()=Visibility::Inherited;
        app.update();
        assert_eq!(prepared(&app),2,"opened: built once");
        for _ in 0..10 { app.update(); }
        assert_eq!(prepared(&app),2,"open and unchanged: nothing more");
    }
}
