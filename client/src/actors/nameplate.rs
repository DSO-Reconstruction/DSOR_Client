//! Names over heads: players (own and others) and NPCs.
//!
//! The 2018 client's `NameLabel` (ui/overheadtext.bxml): Tahoma 10.5 at a 1040-pixel
//! reference height, white, with a black text effect. An admin's name -- the third
//! leading bool of NewPlayer / NewRemotePlayer -- is drawn in `_Globals`
//! OverheadAdminColor, (1.0, 0.5, 0.2): orange.
//! UNVERIFIED: the head offset (2.25 units over the feet).

use bevy::prelude::*;

/// `_Globals.OverheadAdminColor` (static.db4: 0000803F 0000003F CDCC4C3E 0000803F).
const ADMIN: Color = Color::srgb(1.0, 0.5, 0.2);
/// NameLabel fontSize 10.5 at the 1040-pixel layout height.
const FONT_PT: f32 = 10.5;
const LAYOUT_HEIGHT: f32 = 1040.0;
/// Name height over the actor's origin (its feet).
const HEAD: f32 = 2.25;
/// On a mount: the name over the seated rider's head, this far over the saddle,
/// when that is higher than HEAD.
/// FAILURE (2026-10-24): on the Manticore the name sat on the rider's body (the
///   user's shot); the original raises it well above them there, and hardly at
///   all on the white tiger (its shots).
/// UNVERIFIED: the seated height, matched by eye to those two shots.
const SEATED_HEAD: f32 = 1.7;
/// The name's height over the hips: a body lifted off its feet (a leap's clip
/// raises the Hips up to 5.6 m) takes its name up with it, when that is higher
/// than HEAD.
/// FAILURE (2026-10-29): the name stayed over the ground during Rage Jump while
///   the warrior flew high above it ("if I jump, it follows me, it feels
///   quite wrong"); the original's rises with him.
/// UNVERIFIED: the distance (HEAD over hips standing about 1 m high).
const OVER_HIPS: f32 = 1.25;
/// Beyond this from the camera, names are not drawn.
const MAX_DISTANCE: f32 = 45.0;
/// Width of the box a name is centred in, pixels.
const BOX: f32 = 400.0;
/// NameLabel's top over the overhead window's centre, as a share of the screen
/// height (41.26 / 768: ui/overheadtext.bxml).
const NAME_OVER_CENTRE: f32 = 41.26 / 768.0;

/// A player in open battle: `_Globals.HighlightEnemyColor` (0.8, 0, 0).
/// UNVERIFIED: the original's overhead colour for it; the user asked for red
///   with "(PvP)" after the name, an admin keeping orange.
const PVP: Color = Color::srgb(0.8, 0.0, 0.0);

#[derive(Component, Clone)]
pub struct Nameplate {
    pub text: String,
    pub color: Color,
    /// A player's own name and admin flag (`set_pvp` rebuilds text and colour).
    pub player: Option<(String, bool)>,
    pub pvp: bool,
}

impl Nameplate {
    pub fn player(name: String, admin: bool) -> Self {
        Self { text: name.clone(), color: if admin { ADMIN } else { Color::WHITE }, player: Some((name, admin)), pvp: false }
    }

    /// In open battle: "Name (PvP)", red unless an admin.
    pub fn set_pvp(&mut self, pvp: bool) {
        let Some((name, admin)) = &self.player else { return };
        self.pvp = pvp;
        self.text = if pvp { format!("{name} (PvP)") } else { name.clone() };
        self.color = if *admin { ADMIN } else if pvp { PVP } else { Color::WHITE };
    }
}

/// The UI node drawing an actor's name.
#[derive(Component)]
struct Label(Entity);

pub struct NameplatePlugin;

impl Plugin for NameplatePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, place.after(TransformSystems::Propagate));
    }
}

fn place(
    mut commands: Commands,
    windows: Query<&Window>,
    cameras: Query<(Entity, &Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    actors: Query<(Entity, &Nameplate, &GlobalTransform, &InheritedVisibility, Option<&Label>, Option<&crate::actors::mounts::Riding>)>,
    mut labels: Query<(Entity, &mut Node, &mut Visibility, &mut TextColor, &mut Text), With<LabelOf>>,
    owners: Query<&LabelOf>,
    assets: Res<AssetServer>,
    mut font: Local<Option<Handle<Font>>>,
    settings: Res<crate::ui::Settings>,
    local: Query<(), With<crate::network::net::LocalPlayer>>,
    (characters, bones): (Query<&crate::actors::character::Character>, Query<&GlobalTransform>),
) {
    // EVIDENCE: ui/overheadtext.bxml NameLabel: font "tahoma"; drawn thin by the
    // 2018 client (recorded), so Wine's regular Tahoma (assets/fonts/tahoma.ttf,
    // copied by hand: not shipped with the game data).
    let font = font.get_or_insert_with(|| assets.load("fonts/tahoma.ttf")).clone();
    if crate::flag("nonames") {
        return;
    }
    let Ok((camera_entity, camera, cam_at)) = cameras.single() else { return };
    let height = windows.iter().next().map(|w| w.height()).unwrap_or(LAYOUT_HEIGHT);
    // Points to pixels at the layout height, then as a share of the window
    // height, so the name scales with the window as the game's layout does.
    let vh = FONT_PT * 96.0 / 72.0 / LAYOUT_HEIGHT * 100.0;
    for (entity, plate, at, visible, label, riding) in &actors {
        let Some(Label(node)) = label else {
            let node = commands
                .spawn((
                    LabelOf(entity),
                    // Render with the scene, before the mesh HUD and menus on
                    // the later UI camera. ZIndex cannot order across cameras.
                    UiTargetCamera(camera_entity),
                    Node { position_type: PositionType::Absolute, width: Val::Px(BOX), justify_content: JustifyContent::Center, ..default() },
                    Text::new(plate.text.clone()),
                    TextFont { font: font.clone().into(), font_size: FontSize::Vh(vh), ..default() },
                    TextColor(plate.color),
                    TextLayout::justify(Justify::Center),
                    TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK },
                    Visibility::Hidden,
                ))
                .id();
            commands.entity(entity).insert(Label(node));
            continue;
        };
        let Ok((_, mut style, mut vis, mut color, mut text)) = labels.get_mut(*node) else { continue };
        let hips = characters.get(entity).ok().and_then(|c| c.bone("Hips")).and_then(|h| bones.get(h).ok())
            .map_or(0.0, |h| h.translation().y - at.translation().y);
        let head = at.translation() + Vec3::Y * riding.map_or(HEAD.max(hips + OVER_HIPS), |r| HEAD.max(r.lift + SEATED_HEAD));
        let shown = (if local.contains(entity) { settings.current.own_name } else { settings.current.other_names })
            && visible.get()
            && head.distance(cam_at.translation()) < MAX_DISTANCE
            && !plate.text.is_empty();
        let screen = shown.then(|| camera.world_to_viewport(cam_at, head).ok()).flatten();
        let Some(p) = screen else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Inherited;
        style.left = Val::Px(p.x - BOX / 2.0);
        // EVIDENCE: ui/overheadtext.bxml: NameLabel's top (354.74) is 41.26 of the
        //   768 reference pixels over its canvas's centre (396.56), the point the
        //   window is placed on. UNVERIFIED: that point as HEAD over the feet.
        // FAILURE: 1.2 font heights over the head drew the name too low ("le
        //   pseudo devrait etre plus haut").
        style.top = Val::Px(p.y - NAME_OVER_CENTRE * height);
        if color.0 != plate.color {
            color.0 = plate.color;
        }
        if text.0 != plate.text {
            text.0 = plate.text.clone();
        }
    }
    // Labels whose actor is gone.
    for (node, ..) in &labels {
        if let Ok(LabelOf(owner)) = owners.get(node) {
            if actors.get(*owner).is_err() {
                commands.entity(node).despawn();
            }
        }
    }
}

#[derive(Component)]
struct LabelOf(Entity);
