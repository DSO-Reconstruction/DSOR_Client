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
/// Beyond this from the camera, names are not drawn.
const MAX_DISTANCE: f32 = 45.0;
/// Width of the box a name is centred in, pixels.
const BOX: f32 = 400.0;

#[derive(Component, Clone)]
pub struct Nameplate {
    pub text: String,
    pub color: Color,
}

impl Nameplate {
    pub fn player(name: String, admin: bool) -> Self {
        Self { text: name, color: if admin { ADMIN } else { Color::WHITE } }
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
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    actors: Query<(Entity, &Nameplate, &GlobalTransform, &InheritedVisibility, Option<&Label>)>,
    mut labels: Query<(Entity, &mut Node, &mut Visibility, &mut TextColor, &mut Text), With<LabelOf>>,
    owners: Query<&LabelOf>,
    assets: Res<AssetServer>,
    mut font: Local<Option<Handle<Font>>>,
) {
    // Bevy's built-in font is ASCII only ("F?lix"); Noto Sans Bold stands in for
    // the game's Tahoma bold (assets/fonts, not shipped with the repository).
    let font = font.get_or_insert_with(|| assets.load("fonts/NotoSans-Bold.ttf")).clone();
    if crate::flag("nonames") {
        return;
    }
    let Ok((camera, cam_at)) = cameras.single() else { return };
    let height = windows.iter().next().map(|w| w.height()).unwrap_or(LAYOUT_HEIGHT);
    // Points to pixels at the layout height, then as a share of the window
    // height, so the name scales with the window as the game's layout does.
    let vh = FONT_PT * 96.0 / 72.0 / LAYOUT_HEIGHT * 100.0;
    let size = vh * height / 100.0;
    for (entity, plate, at, visible, label) in &actors {
        let Some(Label(node)) = label else {
            let node = commands
                .spawn((
                    LabelOf(entity),
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
        let head = at.translation() + Vec3::Y * HEAD;
        let shown = visible.get()
            && head.distance(cam_at.translation()) < MAX_DISTANCE
            && !plate.text.is_empty();
        let screen = shown.then(|| camera.world_to_viewport(cam_at, head).ok()).flatten();
        let Some(p) = screen else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Inherited;
        style.left = Val::Px(p.x - BOX / 2.0);
        style.top = Val::Px(p.y - size * 1.2);
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
