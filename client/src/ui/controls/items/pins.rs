//! Pinned item cards: Alt over an item in the bags pins its tooltip card in a
//! small window of its own, which stays where it is dragged, so several can sit
//! side by side to compare items ("une petite fenetre de l'item que je peux
//! deplacer pour comparer les items entre eux", 2026-10-20). A pinned card is
//! the item as it was when pinned. Its x (top right) or a right click on it
//! closes it.
//! UNVERIFIED: not the original's (its compare view is the equipped item's
//! tooltip shown beside the hovered one); a DSOR convenience.

use super::*;

/// At most this many cards at once; the oldest goes for a new one.
const MOST: usize = 8;
/// Over the windows and the hover tooltip's 800.
const PIN_Z: f32 = 820.0;
/// The close mark, a square at the card's top right (reference pixels).
const CLOSE: f32 = 22.0;

pub(in crate::ui) struct Pin {
    root: Entity,
    at: Vec2,
    size: Vec2,
}

#[derive(Resource, Default)]
pub(in crate::ui) struct Pins {
    cards: Vec<Pin>,
    /// The card being dragged and where on it it was grabbed.
    drag: Option<(usize, Vec2)>,
}

#[derive(Component)]
pub(in crate::ui) struct PinnedCard;

impl Pins {
    fn hit(&self, point: Vec2) -> Option<usize> {
        // The newest on top.
        self.cards.iter().rposition(|p| {
            point.x >= p.at.x && point.y >= p.at.y && point.x <= p.at.x + p.size.x && point.y <= p.at.y + p.size.y
        })
    }
}

/// Pin the hovered item on Alt; drag, close and keep the cards. Runs before the
/// windows' own pointer handling, which never sees a press meant for a card.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::ui) fn update(
    mut commands: Commands,
    mut pins: ResMut<Pins>,
    windows: Query<(&Window2018, &WindowControls, &Visibility, &Transform), Without<PinnedCard>>,
    primary: Query<&Window, With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    net: Option<NonSend<crate::network::net::Net>>,
    (data, catalogs, layouts): (Res<Data>, Res<Assets<Catalog>>, Res<Assets<Layout>>),
    assets: Res<AssetServer>,
    screen: Res<Screen>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<UiMaterial>>),
    mut transforms: Query<&mut Transform, With<PinnedCard>>,
    (settings, pointer): (Res<settings::Settings>, Res<Pointer>),
) {
    let point = primary.single().ok().filter(|w| w.focused)
        .and_then(|w| w.cursor_position().map(|p| p * (BASE_H / w.height().max(1.0))));
    // Pin: Alt over an item of the bags.
    if keys.any_just_pressed([KeyCode::AltLeft, KeyCode::AltRight]) && pointer.focus.is_none() {
        let (Some(net), Some(catalog), Some(p)) = (net.as_ref(), catalogs.get(&data.catalog), point) else { return };
        let card = windows.iter().filter(|(n, _, v, _)| n.0 == "inventory" && **v != Visibility::Hidden)
            .find_map(|(_, c, _, _)| c.hovered.and_then(|n| match c.buttons[n].action {
                Action::Item(cell) => displayed(cell, c, &net.inventory, catalog),
                _ => None,
            }))
            .map(|d| preview::card(&d, catalog, layouts.get(&data.tooltip), settings.current.advanced_tooltips(&keys, false)));
        if let Some(card) = card {
            let size = Vec2::new(card.width, card.height());
            // Beside the cursor, kept on the screen, a step off the last card.
            let step = pins.cards.len() as f32 * 14.0;
            let at = Vec2::new((p.x + 24.0 + step).min(screen.width - size.x).max(0.0), (p.y - 20.0 + step).min(BASE_H - size.y).max(0.0));
            let root = commands.spawn((
                PinnedCard,
                Transform::from_translation(screen.world(0.0, at.x, at.y).extend(PIN_Z + pins.cards.len() as f32)),
                Visibility::Inherited,
                crate::ui::input::UiRegions(vec![(0.0, [at.x, at.y, at.x + size.x, at.y + size.y])]),
            )).id();
            card.draw(&mut commands, &assets, &screen, &mut meshes, &mut materials, root, layouts.get(&data.tooltip));
            // The windows' own close button (a Close button's normal art,
            // deco_elemente_5), in the card's top right corner.
            // FAILURE: "mettre la bonne texture de croix aussi" (a typed "x").
            // The inventory's (a square red one; some windows have a round one).
            let mut ordered: Vec<&Layout> = layouts.iter().map(|(_, l)| l).collect();
            ordered.sort_by_key(|l| l.window != "inventory");
            let close_art = ordered.into_iter().find_map(|l| {
                let button = l.widgets.iter().find(|w| w.event == "Close")?;
                l.widgets.iter().find(|w| w.id == format!("{}/normal", button.id)).and_then(|w| w.art.clone().map(|a| (a, w.rect)))
            });
            if let Some((mut art, from)) = close_art {
                let to = [size.x - CLOSE - 2.0, 2.0, size.x - 2.0, CLOSE + 2.0];
                for p in art.pos.chunks_exact_mut(2) {
                    p[0] = to[0] + (p[0] - from[0]) / (from[2] - from[0]) * (to[2] - to[0]);
                    p[1] = -(to[1] + (p[1] - from[1]) / (from[3] - from[1]) * (to[3] - to[1]));
                }
                let atlas = art.tex.as_ref().map(|t| assets.load(t.clone()));
                let mut run = crate::ui::Run::default();
                run.push(&art, 0.0);
                commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(materials.add(crate::ui::material(atlas))),
                    Transform::from_xyz(0.0, 0.0, 2.0), RenderLayers::layer(LAYER), ChildOf(root)));
            }
            pins.cards.push(Pin { root, at, size });
            if pins.cards.len() > MOST {
                let old = pins.cards.remove(0);
                commands.entity(old.root).despawn();
            }
        }
    }
    let Some(p) = point else { pins.drag = None; return };
    // Close: its x, or a right click on it.
    let close = |pins: &mut Pins, commands: &mut Commands, index: usize| {
        let pin = pins.cards.remove(index);
        commands.entity(pin.root).despawn();
        pins.drag = None;
    };
    if mouse.just_pressed(MouseButton::Right) {
        if let Some(index) = pins.hit(p) {
            close(&mut pins, &mut commands, index);
            mouse.clear_just_pressed(MouseButton::Right);
        }
    }
    if mouse.just_pressed(MouseButton::Left) {
        if let Some(index) = pins.hit(p) {
            let pin = &pins.cards[index];
            if p.x >= pin.at.x + pin.size.x - CLOSE && p.y <= pin.at.y + CLOSE {
                close(&mut pins, &mut commands, index);
            } else {
                // Grabbed: on top of the others.
                let pin = pins.cards.remove(index);
                let grab = p - pin.at;
                pins.cards.push(pin);
                let top = pins.cards.len() - 1;
                pins.drag = Some((top, grab));
            }
            // The press is the card's, not the window under it.
            mouse.clear_just_pressed(MouseButton::Left);
        }
    }
    if let Some((index, grab)) = pins.drag {
        if mouse.pressed(MouseButton::Left) {
            let size = pins.cards[index].size;
            let at = (p - grab).clamp(Vec2::ZERO, Vec2::new((screen.width - size.x).max(0.0), (BASE_H - size.y).max(0.0)));
            pins.cards[index].at = at;
        } else {
            pins.drag = None;
            mouse.clear_just_released(MouseButton::Left);
        }
    }
    // Every card where it stands, the newest on top.
    for (i, pin) in pins.cards.iter().enumerate() {
        if let Ok(mut tf) = transforms.get_mut(pin.root) {
            let want = screen.world(0.0, pin.at.x, pin.at.y).extend(PIN_Z + i as f32);
            if tf.translation != want { tf.translation = want; }
        }
        commands.entity(pin.root).try_insert(crate::ui::input::UiRegions(vec![(0.0, [pin.at.x, pin.at.y, pin.at.x + pin.size.x, pin.at.y + pin.size.y])]));
    }
}
