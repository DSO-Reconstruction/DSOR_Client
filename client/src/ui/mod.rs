//! The 2018 client's interface: the windows always on screen (the bottom bar with
//! the health and resource globes, the XP bar and the quick slots; the money in the
//! top left corner; the menu buttons in the top right) and three menus: the
//! inventory (I), the character sheet (C) and the skills (K).
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
//! sharing a texture (the engine's own draw order kept by depth), built once; a
//! resize only moves their parents. The globes and the XP bar are cut to their
//! fill by their material's clip rectangle; texts are Text2d. Nothing is rebuilt
//! per frame: values change materials and texts only when they change.
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

/// The reference screen the 2018 interface was laid out on.
const BASE_W: f32 = 1024.0;
const BASE_H: f32 = 768.0;
/// The interface's render layer: only the UI camera sees it.
const LAYER: usize = 7;
/// The engine's fontSize to reference pixels (DSO_Godot's calibration).
const FONT_SCALE: f32 = 1.5;

/// Windows shown from the start, and their depth (later ones over earlier ones).
const HUD_WINDOWS: [(&str, f32); 3] = [("bottombar", 10.0), ("currencybar", 20.0), ("gamebar", 30.0)];
/// Windows the player opens: (window, key, depth).
const MENU_WINDOWS: [(&str, KeyCode, f32); 3] = [
    ("inventory", KeyCode::KeyI, 100.0),
    ("charactersheet", KeyCode::KeyC, 110.0),
    ("skillwindow2", KeyCode::KeyK, 120.0),
];

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
}

#[derive(Deserialize, Clone)]
struct Widget {
    id: String,
    k: f32,
    rect: [f32; 4],
    hidden: bool,
    art: Option<Art>,
    text: Option<Label>,
}

#[derive(Asset, TypePath, Deserialize)]
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

/// The interface camera takes the scene camera's HDR and MSAA, so both share
/// their target textures (see setup).
#[allow(clippy::type_complexity)]
fn match_scene_camera(
    mut commands: Commands,
    scene: Query<(Has<Hdr>, &Msaa), (With<Camera3d>, Without<UiCamera>)>,
    mut ui: Query<(Entity, Has<Hdr>, &mut Msaa), With<UiCamera>>,
) {
    let (Some((hdr, msaa)), Ok((e, ui_hdr, mut ui_msaa))) = (scene.iter().next(), ui.single_mut()) else { return };
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

/// A window's root: its name, shown or not.
#[derive(Component)]
struct Window2018(String);

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
}

/// A text the game sets: what it shows.
#[derive(Component)]
struct Value(ValueKind);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Gold,
    Silver,
    Copper,
    Andermant,
    Level,
    Name,
    Page,
}

/// A quick slot's icon place: its wire slot and rect.
#[derive(Component)]
struct QuickSlot {
    slot: usize,
    k: f32,
    rect: [f32; 4],
    shown: Option<String>,
}

/// The figures the interface shows (from the server when online).
#[derive(Resource, Default)]
struct Figures {
    health: (f32, f32),
    resource: (f32, f32),
    xp: f32,
    bar: Vec<Option<String>>,
    name: String,
    online: bool,
    level: Option<u32>,
    /// Wallet slot 1, copper; slot 0, Andermant.
    gold: Option<u32>,
    andermant: Option<u32>,
}

#[derive(Resource, Default)]
struct Loaded {
    layouts: Vec<(Handle<Layout>, f32, Option<KeyCode>)>,
    icons: Handle<SkillIcons>,
    spawned: Vec<bool>,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "ui.wgsl");
        app.add_plugins(Material2dPlugin::<UiMaterial>::default())
            .init_asset::<Layout>()
            .init_asset::<SkillIcons>()
            .init_asset_loader::<LayoutLoader>()
            .init_asset_loader::<SkillIconsLoader>()
            .init_resource::<Screen>()
            .init_resource::<Figures>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (match_scene_camera, spawn_windows, follow_screen, read_figures, (update_fills, update_values, update_slots), toggle_windows).chain(),
            );
    }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
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
    let mut loaded = Loaded { icons: assets.load("interface/skills.icons.json"), ..default() };
    for (name, z) in HUD_WINDOWS {
        loaded.layouts.push((assets.load(format!("interface/{name}.ui.json")), z, None));
    }
    for (name, key, z) in MENU_WINDOWS {
        loaded.layouts.push((assets.load(format!("interface/{name}.ui.json")), z, Some(key)));
    }
    loaded.spawned = vec![false; loaded.layouts.len()];
    commands.insert_resource(loaded);
}

/// Which dynamic part a widget is, if any.
fn gauge_of(id: &str) -> Option<Gauge> {
    let parent = id.rsplit('/').nth(1).unwrap_or("");
    match parent {
        "ProgressBar_health" => Some(Gauge::Health),
        "ProgressBar_rage" | "ProgressBar_mana" | "ProgressBar_stamina" | "ProgressBar_mechanicResource" => Some(Gauge::Resource),
        "ProgressBar_xp" if !id.ends_with("glow") && !id.contains("glow") => Some(Gauge::Xp),
        _ => None,
    }
}

/// The resource globe of a class (0 warrior, 1 mage, 2 ranger, 3 dwarf).
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
    Some(match leaf {
        "gold_display" if id.contains("VirtualCurrency") => ValueKind::Gold,
        "silver_display" => ValueKind::Silver,
        "copper_display" => ValueKind::Copper,
        "andermant_display" => ValueKind::Andermant,
        "Level" if id.contains("ProgressBar_xp") => ValueKind::Level,
        "CharacterName" => ValueKind::Name,
        "PageNumText" => ValueKind::Page,
        _ => return None,
    })
}

/// The quick slots by wire slot: Shift + left button = 0, right button = 1, keys
/// 1-5 = 2-6 (crate::skills), 6 and 7 the bar's last two.
fn quick_slot_of(id: &str) -> Option<usize> {
    let (parent, leaf) = {
        let mut it = id.rsplit('/');
        let leaf = it.next()?;
        (it.next().unwrap_or(""), leaf)
    };
    if parent != "icon_bg" {
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
    players: Query<&crate::character::Character, With<crate::net::LocalPlayer>>,
) {
    let class = players.iter().next().map(|c| c.desc.class).unwrap_or(1);
    for i in 0..loaded.layouts.len() {
        if loaded.spawned[i] {
            continue;
        }
        let (handle, z, key) = loaded.layouts[i].clone();
        let Some(layout) = layouts.get(&handle) else { continue };
        loaded.spawned[i] = true;
        // DSOR_UI_OPEN=inventory,...: menus open from the start (screenshots).
        let open = std::env::var("DSOR_UI_OPEN").is_ok_and(|v| v.split(',').any(|w| w == layout.window));
        let root = commands
            .spawn((
                Window2018(layout.window.clone()),
                Transform::from_xyz(0.0, 0.0, z),
                if key.is_some() && !open { Visibility::Hidden } else { Visibility::Inherited },
            ))
            .id();
        // One artwork parent (y flipped) and one text parent per k.
        let mut parents: HashMap<(u32, bool), Entity> = HashMap::new();
        let mut parent = |commands: &mut Commands, k: f32, flip: bool| -> Entity {
            *parents.entry((k.to_bits(), flip)).or_insert_with(|| {
                let tf = follow_transform(&screen, k, flip);
                commands.spawn((Follows { k, flip }, tf, Visibility::Inherited, ChildOf(root))).id()
            })
        };
        let atlas = |tex: &Option<String>| tex.as_ref().map(|t| assets.load::<Image>(t.clone()));
        let mut run: Option<Run> = None;
        let mut depth = 0.0f32;
        let flush = |commands: &mut Commands, run: Option<Run>, parent: Entity, meshes: &mut Assets<Mesh>, materials: &mut Assets<UiMaterial>| {
            let Some(run) = run else { return };
            if run.idx.is_empty() {
                return;
            }
            let m = materials.add(material(atlas(&run.tex)));
            let z = run.z;
            commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(m), Transform::from_xyz(0.0, 0.0, z), RenderLayers::layer(LAYER), ChildOf(parent)));
        };
        let wanted_resource = resource_bar(class);
        // Each gauge fills one rectangle: the union of its pieces (a globe's two
        // halves, the XP bar's ends and middle).
        let mut gauge_rects: HashMap<(u8, u32), [f32; 4]> = HashMap::new();
        for w in &layout.widgets {
            if let (Some(g), Some(art)) = (gauge_of(&w.id), &w.art) {
                if g == Gauge::Resource && !w.id.contains(&format!("/{wanted_resource}/")) {
                    continue;
                }
                let b = bounds(&art.pos);
                let e = gauge_rects.entry((g as u8, w.k.to_bits())).or_insert(b);
                *e = [e[0].min(b[0]), e[1].min(b[1]), e[2].max(b[2]), e[3].max(b[3])];
            }
        }
        for w in &layout.widgets {
            if let Some(slot) = quick_slot_of(&w.id) {
                let p = parent(&mut commands, w.k, true);
                commands.spawn((QuickSlot { slot, k: w.k, rect: w.rect, shown: None }, Transform::from_xyz(0.0, 0.0, depth + 0.005), Visibility::Inherited, ChildOf(p)));
            }
            let gauge = gauge_of(&w.id);
            let other_class = gauge == Some(Gauge::Resource) && !w.id.contains(&format!("/{wanted_resource}/"));
            if (w.hidden && gauge.is_none()) || other_class {
                continue;
            }
            if let Some(art) = &w.art {
                depth += 0.01;
                if let Some(which) = gauge {
                    // Its own mesh and material, cut to the fill.
                    let k = w.k;
                    let p = parent(&mut commands, k, true);
                    let mut r = Run { k, tex: art.tex.clone(), ..default() };
                    r.push(art, depth);
                    let m = materials.add(material(atlas(&art.tex)));
                    let rect = gauge_rects.get(&(which as u8, k.to_bits())).copied().unwrap_or_else(|| bounds(&art.pos));
                    commands.spawn((
                        Mesh2d(meshes.add(r.mesh())),
                        MeshMaterial2d(m),
                        Fill { which, k, rect, shown: -1.0 },
                        Transform::from_xyz(0.0, 0.0, depth),
                        RenderLayers::layer(LAYER),
                        ChildOf(p),
                    ));
                } else {
                    let same = run.as_ref().is_some_and(|r| r.k == w.k && r.tex == art.tex);
                    if !same {
                        let prev = run.take();
                        if let Some(prev_k) = prev.as_ref().map(|r| r.k) {
                            let p = parent(&mut commands, prev_k, true);
                            flush(&mut commands, prev, p, &mut meshes, &mut materials);
                        }
                        run = Some(Run { k: w.k, tex: art.tex.clone(), ..default() });
                    }
                    run.as_mut().unwrap().push(art, depth);
                }
            }
            if let Some(label) = &w.text {
                let value = value_of(&w.id);
                if !(static_text(label) || value.is_some()) || label.color[3] <= 0.0 {
                    continue;
                }
                let p = parent(&mut commands, w.k, false);
                let [l, t, r, b] = w.rect;
                let x = match label.align[0] { 0 => l, 2 => r, _ => (l + r) * 0.5 };
                let y = match label.align[1] { 0 => t, 2 => b, _ => (t + b) * 0.5 };
                let anchor = Anchor(Vec2::new(
                    match label.align[0] { 0 => -0.5, 2 => 0.5, _ => 0.0 },
                    match label.align[1] { 0 => 0.5, 2 => -0.5, _ => 0.0 },
                ));
                let mut e = commands.spawn((
                    Text2d::new(if value.is_some() { String::new() } else { label.text.clone() }),
                    TextFont { font_size: bevy::text::FontSize::Px(label.size * FONT_SCALE * screen.scale), ..default() },
                    TextColor(Color::srgba(label.color[0], label.color[1], label.color[2], label.color[3])),
                    anchor,
                    Transform::from_xyz(x, -y, depth + 0.005).with_scale(Vec3::splat(1.0 / screen.scale)),
                    RenderLayers::layer(LAYER),
                    ChildOf(p),
                ));
                if let Some(v) = value {
                    e.insert(Value(v));
                }
            }
        }
        if let Some(prev_k) = run.as_ref().map(|r| r.k) {
            let p = parent(&mut commands, prev_k, true);
            flush(&mut commands, run.take(), p, &mut meshes, &mut materials);
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
    mut texts: Query<(&mut TextFont, &mut Transform, &ChildOf), With<Text2d>>,
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
        for (mut font, mut tf, parent) in &mut texts {
            if labels.contains(parent.parent()) {
                if let bevy::text::FontSize::Px(px) = font.font_size {
                    font.font_size = bevy::text::FontSize::Px(px / old_scale * new.scale);
                }
                tf.scale = Vec3::splat(1.0 / new.scale);
            }
        }
    }
    // Clip rectangles are in world space: re-cut.
    for mut f in &mut fills {
        f.shown = -1.0;
    }
}

/// The server's figures, or a resting HUD offline.
fn read_figures(net: Option<NonSend<crate::net::Net>>, mut figures: ResMut<Figures>) {
    let Some(net) = net else {
        if !figures.online && figures.health.1 == 0.0 {
            // Offline (the map viewer): a full-looking HUD, so it can be seen.
            figures.health = (80.0, 100.0);
            figures.resource = (60.0, 100.0);
            figures.xp = 0.35;
            figures.bar = ["mage_magicmissile_default", "mage_fireball_default", "mage_frostnova_default", "mage_iceball_default", "mage_meteor_default", "mage_teleport_default", "mage_lightningstrike_default"]
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
    if let Some(r) = net.resource {
        let max = figures.resource.1.max(r);
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
    }
    if let Some(w) = net.wallet {
        if (figures.andermant, figures.gold) != (Some(w[0]), Some(w[1])) {
            figures.andermant = Some(w[0]);
            figures.gold = Some(w[1]);
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
    screen: Res<Screen>,
    mut fills: Query<(&mut Fill, &MeshMaterial2d<UiMaterial>)>,
    mut materials: ResMut<Assets<UiMaterial>>,
) {
    for (mut f, m) in &mut fills {
        let v = match f.which {
            Gauge::Health => fraction(figures.health),
            Gauge::Resource => fraction(figures.resource),
            Gauge::Xp => figures.xp.clamp(0.0, 1.0),
        };
        if (v - f.shown).abs() < 1e-3 {
            continue;
        }
        f.shown = v;
        let [l, t, r, b] = f.rect;
        // Globes fill from the bottom; the XP bar from the left.
        let (min, max) = match f.which {
            Gauge::Xp => (screen.world(f.k, l, b), screen.world(f.k, l + (r - l) * v, t)),
            _ => (screen.world(f.k, l, b), screen.world(f.k, r, b - (b - t) * v)),
        };
        if let Some(mut mat) = materials.get_mut(&m.0) {
            mat.clip = Vec4::new(min.x, min.y, max.x, max.y);
        }
    }
}

fn update_values(figures: Res<Figures>, mut texts: Query<(&Value, &mut Text2d)>) {
    if !figures.is_changed() {
        return;
    }
    for (v, mut t) in &mut texts {
        // Gold is counted in copper: 100 copper a silver, 100 silver a gold.
        // UNVERIFIED: the split (the currency bar's own code is not traced).
        let copper = figures.gold.unwrap_or(0);
        let s = match v.0 {
            ValueKind::Gold => (copper / 10_000).to_string(),
            ValueKind::Silver => (copper / 100 % 100).to_string(),
            ValueKind::Copper => (copper % 100).to_string(),
            ValueKind::Andermant => figures.andermant.unwrap_or(0).to_string(),
            ValueKind::Level => figures.level.map(|l| l.to_string()).unwrap_or_else(|| "-".to_owned()),
            ValueKind::Name => figures.name.clone(),
            ValueKind::Page => "1/1".to_owned(),
        };
        if t.0 != s {
            t.0 = s;
        }
    }
}

/// Quick slot icons: the skill's IconBrush, inset in its slot.
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
) {
    let Some(table) = icon_table.get(&loaded.icons) else { return };
    for (e, mut s) in &mut slots {
        let want = figures.bar.get(s.slot).cloned().flatten().filter(|id| table.0.contains_key(id));
        if want == s.shown {
            continue;
        }
        commands.entity(e).despawn_related::<Children>();
        if let Some(id) = &want {
            let [l, t, r, b] = s.rect;
            let inset = 4.0;
            let (l, t, r, b) = (l + inset, t + inset, r - inset, b - inset);
            let art = Art {
                tex: None,
                pos: vec![l, t, r, t, r, b, l, b],
                uv: vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
                color: vec![1.0; 16],
                idx: vec![0, 1, 2, 0, 2, 3],
            };
            let mut run = Run { k: s.k, ..default() };
            run.push(&art, 0.0);
            let m = materials.add(material(Some(assets.load(table.0[id].clone()))));
            commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(m), Transform::IDENTITY, RenderLayers::layer(LAYER), ChildOf(e)));
        }
        s.shown = want;
    }
}

fn toggle_windows(keys: Res<ButtonInput<KeyCode>>, loaded: Res<Loaded>, layouts: Res<Assets<Layout>>, mut windows: Query<(&Window2018, &mut Visibility)>) {
    for (handle, _, key) in &loaded.layouts {
        let (Some(key), Some(layout)) = (key, layouts.get(handle)) else { continue };
        if !keys.just_pressed(*key) {
            continue;
        }
        for (w, mut v) in &mut windows {
            if w.0 == layout.window {
                *v = if *v == Visibility::Hidden { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
    }
}
