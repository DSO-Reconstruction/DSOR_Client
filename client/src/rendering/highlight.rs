//! The hovered actor's outline: a monster, an NPC or another player under the
//! cursor glows along its silhouette, as the 2018 client highlights it.
//!
//! EVIDENCE: export_win32/frame/default.xml: `HighlightSilhouette` renders the
//!   highlighted entities alone (Batch type Highlight) into a buffer cleared to
//!   transparent; `GatherOutline` (pe_outline) keeps their edge; `BlurHighlight0..2`
//!   blur it; pe_compose adds it to the frame (HighlightBuffer).
//!   EntityHighlightManager's colour (0x57E540): a monster is "Hostile"
//!   (_Globals HighlightEnemyColor 0.8, 0, 0); an NPC with a quest takes its quest
//!   type's colour (QuestTypeHighlight*: atoms set at 0x44EB3E..0x44ED12); a
//!   player "Ally" or "Hostile" by its PvP standing; anything else "default".
//! UNVERIFIED: the colours of "default" and "Ally" (their dictionary's filling is
//!   not traced): white here for NPCs and players; the outline's width and blur
//!   (pe_outline and pe_highlight* not disassembled).
//!
//! Here: a second 3D camera, on its own render layer, draws the hovered actor's
//! meshes into an image cleared to pure magenta; every pixel that is not magenta
//! is the silhouette (not its alpha: the monsters' textures keep their specular
//! there, and their AlphaTest surfaces wrote it -- the silhouette came out empty).
//! A full-screen UI material draws the band around it in the highlight colour.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::ui_render::prelude::{MaterialNode, UiMaterial, UiMaterialPlugin};
use bevy::window::PrimaryWindow;

/// The render layer only the highlight camera sees.
const LAYER: usize = 5;
/// _Globals HighlightEnemyColor.
const ENEMY: Vec4 = Vec4::new(0.8, 0.0, 0.0, 1.0);
/// "default" and "Ally" (UNVERIFIED).
const DEFAULT: Vec4 = Vec4::new(1.0, 1.0, 1.0, 1.0);
/// A hovered ground item's outline.
/// MEASURED: the user's clip 20261028-1812-11 (release261_192, crypt): the
///   hovered Silver Coronet's band peaks at (205, 255, 46) over its green glow.
/// UNVERIFIED: its source (an EntityHighlightManager type not traced).
const ITEM: Vec4 = Vec4::new(0.8, 1.0, 0.18, 1.0);
/// The camera that draws the highlighted actor alone.
/// The interface's preview cameras (crate::ui::preview::PreviewCamera) carry it
/// too: every query for the scene camera leaves it out (`Without<HighlightCamera>`).
#[derive(Component)]
pub struct HighlightCamera;

/// What is highlighted now, and in which colour.
#[derive(Resource, Default, PartialEq, Clone, Copy)]
pub(crate) struct Highlighted(pub(crate) Option<(Entity, [u32; 4])>);

#[derive(Resource)]
struct Target {
    image: Handle<Image>,
    material: Handle<OutlineMaterial>,
    size: UVec2,
}

/// A mesh drawn on the highlight layer too (removed again when no longer
/// highlighted).
#[derive(Component)]
struct OnHighlightLayer;

#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct OutlineMaterial {
    #[uniform(0)]
    color: Vec4,
    #[uniform(0)]
    texel: Vec4,
    #[texture(1)]
    #[sampler(2)]
    mask: Handle<Image>,
}

impl UiMaterial for OutlineMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://dsor_client/rendering/shaders/outline.wgsl".into()
    }
}

pub struct HighlightPlugin;

impl Plugin for HighlightPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/outline.wgsl");
        app.add_plugins(UiMaterialPlugin::<OutlineMaterial>::default())
            .init_resource::<Highlighted>()
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, (pick, (relayer, follow_camera, fit_target)).chain().before(TransformSystems::Propagate));
    }
}

fn image(size: UVec2) -> Image {
    Image::new_target_texture(size.x.max(1), size.y.max(1), TextureFormat::Rgba8UnormSrgb, None)
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut materials: ResMut<Assets<OutlineMaterial>>) {
    let size = UVec2::new(64, 64);
    let handle = images.add(image(size));
    commands.spawn((
        HighlightCamera,
        Camera3d::default(),
        Camera { order: -10, is_active: false, clear_color: ClearColorConfig::Custom(Color::srgb(1.0, 0.0, 1.0)), ..default() },
        RenderTarget::Image(ImageRenderTarget::from(handle.clone())),
        Tonemapping::None,
        Msaa::Off,
        RenderLayers::layer(LAYER),
        Transform::default(),
    ));
    let material = materials.add(OutlineMaterial { color: Vec4::ZERO, texel: Vec4::ZERO, mask: handle.clone() });
    commands.spawn((
        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        MaterialNode(material.clone()),
        Pickable::IGNORE,
        GlobalZIndex(-1),
    ));
    commands.insert_resource(Target { image: handle, material, size });
}

/// The actor under the cursor: a monster first (crate::actors::monsters::Hovered), else
/// an NPC or another player, nearest along the ray.
#[allow(clippy::type_complexity)]
pub(crate) fn pick(
    ui: Res<crate::ui::input::UiInput>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<HighlightCamera>)>,
    hovered: Res<crate::actors::monsters::Hovered>,
    item: Option<Res<crate::world::drops::ItemHover>>,
    actors: Query<(Entity, &GlobalTransform, &InheritedVisibility, Option<&crate::actors::picking::PickBoxes>, Option<&crate::actors::npc::Npc>), Or<(With<crate::actors::npc::Npc>, (With<crate::network::net::RemotePlayer>, With<crate::actors::character::Character>, Without<crate::actors::monsters::Monster>))>>,
    joints: Query<&GlobalTransform>,
    exits: Query<(Entity, &crate::world::exits::Exit)>,
    usables: Query<(Entity, &GlobalTransform, &InheritedVisibility, Has<crate::world::props::Interactive>),
        Or<(With<crate::world::props::Interactive>, With<crate::world::props::TravelStone>, With<crate::world::props::LocalUse>)>>,
    mut highlighted: ResMut<Highlighted>,
) {
    let colour = |c: Vec4| [c.x.to_bits(), c.y.to_bits(), c.z.to_bits(), c.w.to_bits()];
    let now = if ui.over_ui {
        None
    } else if let Some((e, _)) = hovered.0 {
        Some((e, colour(ENEMY)))
    } else if let Some((_, model)) = item.as_ref().and_then(|i| i.0) {
        // The item's model alone, not its glow (an effect beside it).
        Some((model, colour(ITEM)))
    } else {
        let ray = windows
            .single()
            .ok()
            .and_then(|w| w.cursor_position())
            .zip(cameras.single().ok())
            .and_then(|(c, (cam, at))| cam.viewport_to_world(at, c).ok());
        ray.and_then(|ray| {
            let mut best: Option<(f32, Entity)> = None;
            for (e, at, shown, boxes, npc) in &actors {
                if !shown.get() { continue; }
                // Only the NPCs one talks to are outlined: not the town's
                // pigeons, chickens and other decoration (NoTalk).
                // FAILURE: "j'ai l'outline sur tous les PNJ, meme les pigeons
                //   et les poules".
                if npc.is_some_and(|n| n.profile.no_talk) { continue; }
                if let Some(t) = crate::actors::picking::friendly_hit(ray, at.translation(), boxes, &joints) {
                    if best.is_none_or(|(bt, _)| t < bt) { best = Some((t, e)); }
                }
            }
            // Usable things (props, travel stones, workbenches), as their click picks them.
            // FAILURE: "j'ai pas l'outline" on the travel stones.
            for (e, at, shown, prop) in &usables {
                if !shown.get() { continue; }
                if let Some(t) = crate::world::props::use_hit(ray, at.translation(), !prop) {
                    if best.is_none_or(|(bt, _)| t < bt) { best = Some((t, e)); }
                }
            }
            // Exits share the default silhouette highlight. Pick the same
            // ground disc used for travel, excluding overhead particle batches.
            for (e, exit) in &exits {
                if ray.direction.y.abs() < 1e-5 { continue; }
                let t = (exit.at.y - ray.origin.y) / ray.direction.y;
                if t < 0.0 { continue; }
                let point = ray.get_point(t);
                if point.xz().distance(exit.at.xz()) <= 2.0 && best.is_none_or(|(bt, _)| t < bt) {
                    best = Some((t, e));
                }
            }
            best.map(|(_, e)| (e, colour(if exits.contains(e) { Vec4::new(1.0, 1.0, 1.0, 0.35) } else { DEFAULT })))
        })
    };
    if highlighted.0 != now {
        highlighted.0 = now;
    }
}

/// Exclude a visual subtree (such as an overhead icon) from actor outlines.
#[derive(Component)]
pub(crate) struct NoHighlight;

/// The highlighted actor's meshes on the highlight layer too; the camera on only
/// while something is highlighted.
#[allow(clippy::type_complexity)]
fn relayer(
    mut commands: Commands,
    highlighted: Res<Highlighted>,
    mut last: Local<Option<Entity>>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    // Decorative billboards are not part of an object's outline. In particular,
    // outlining an exit's light column exposes the flat quad around its halo.
    // Particle emitters neither: a travel stone's sparks made its outline
    // move all the time ("il devrait pas en compte le FX just le mesh").
    excluded: Query<(), Or<(With<NoHighlight>, With<crate::rendering::materials::FacesViewer>, With<crate::rendering::particles::Emitter>)>>,
    meshes: Query<(), With<Mesh3d>>,
    on_layer: Query<Entity, With<OnHighlightLayer>>,
    mut camera: Query<&mut Camera, (With<HighlightCamera>, Without<crate::ui::PreviewCamera>)>,
    target: Option<Res<Target>>,
    mut materials: ResMut<Assets<OutlineMaterial>>,
) {
    let want = highlighted.0.map(|(e, _)| e);
    // Meshes spawned later (parts still loading) join on the next frames too.
    if want == *last && want.is_none() {
        return;
    }
    if want != *last {
        for e in &on_layer {
            commands.entity(e).try_remove::<(RenderLayers, OnHighlightLayer)>();
        }
        *last = want;
        if let Ok(mut cam) = camera.single_mut() {
            cam.is_active = want.is_some();
        }
        if let (Some(t), Some((_, c))) = (target.as_ref(), highlighted.0) {
            if let Some(mut m) = materials.get_mut(&t.material) {
                m.color = Vec4::new(f32::from_bits(c[0]), f32::from_bits(c[1]), f32::from_bits(c[2]), f32::from_bits(c[3]));
            }
        }
    }
    if let Some(root) = want {
        for d in children.iter_descendants(root) {
            if meshes.contains(d) && !on_layer.contains(d)
                && !excluded.contains(d) && !parents.iter_ancestors(d).any(|p| excluded.contains(p)) {
                commands.entity(d).try_insert((RenderLayers::from_layers(&[0, LAYER]), OnHighlightLayer));
            }
        }
    }
    if want.is_none() {
        if let Some(t) = target.as_ref() {
            if let Some(mut m) = materials.get_mut(&t.material) {
                m.color = Vec4::ZERO;
            }
        }
    }
}

/// The highlight camera sees exactly what the game camera sees.
fn follow_camera(
    main: Query<(&Transform, &Projection), (With<Camera3d>, Without<HighlightCamera>)>,
    mut highlight: Query<(&mut Transform, &mut Projection), (With<HighlightCamera>, Without<crate::ui::PreviewCamera>)>,
) {
    let (Ok((tf, proj)), Ok((mut htf, mut hproj))) = (main.single(), highlight.single_mut()) else { return };
    *htf = *tf;
    *hproj = proj.clone();
}

/// The silhouette image keeps the window's size.
/// CONTRACT: a new image on each resize, given to the camera and the material
///   both. Resized in place, the material kept sampling the first (64 x 64)
///   texture and the camera kept framing for it: the silhouette came out small in
///   a corner, away from the actor.
fn fit_target(
    windows: Query<&Window, With<PrimaryWindow>>,
    target: Option<ResMut<Target>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<OutlineMaterial>>,
    mut camera: Query<&mut RenderTarget, (With<HighlightCamera>, Without<crate::ui::PreviewCamera>)>,
) {
    let (Ok(w), Some(mut t)) = (windows.single(), target) else { return };
    let size = UVec2::new(w.physical_width(), w.physical_height());
    if size == t.size || size.x == 0 || size.y == 0 {
        return;
    }
    t.size = size;
    let old = std::mem::replace(&mut t.image, images.add(image(size)));
    images.remove(&old);
    if let Ok(mut rt) = camera.single_mut() {
        *rt = RenderTarget::Image(ImageRenderTarget::from(t.image.clone()));
    }
    if let Some(mut m) = materials.get_mut(&t.material) {
        m.mask = t.image.clone();
        m.texel = Vec4::new(1.0 / size.x as f32, 1.0 / size.y as f32, 0.0, 0.0);
    }
}
