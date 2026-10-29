//! Animation preview (developer aid): F8 lists the clips of our skeleton
//! (characters/<skeleton>/__animations.glb), those no player animation set
//! uses first (bosses, NPCs, mounts, emotes), and plays the one picked on our
//! character, looping, until F8 closes the list.
//! Picking: a click on a row, the wheel over the list, or Up / Down (Shift:
//! ten at a time). The character's own animation driving pauses meanwhile.
//! FAILURE: "tu peux me faire un menu rapidement pour que je puisse les
//!   preview sur mon charactere?" (boss and monster animations to reuse).
use super::*;
use bevy::input::mouse::MouseWheel;

/// On our character while a previewed clip plays: drive_animations leaves it.
#[derive(Component)]
pub(crate) struct Previewing;

#[derive(Component)]
struct Panel;

#[derive(Component)]
struct Row(usize);

const ROWS: usize = 24;

#[derive(Resource, Default)]
struct Preview {
    open: bool,
    /// The clips, those no player set plays first, and how many those are.
    clips: Vec<String>,
    unused: usize,
    selected: usize,
    /// What the panel and the character show: (first row, selected, open).
    shown: Option<(usize, usize, bool)>,
    playing: Option<String>,
}

pub(crate) struct AnimPreviewPlugin;

impl Plugin for AnimPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Preview>()
            .add_systems(Update, (input, list, panel, play).chain().before(drive_animations));
    }
}

fn input(
    keys: Res<ButtonInput<KeyCode>>,
    ui: Option<Res<crate::ui::input::UiInput>>,
    mut wheel: MessageReader<MouseWheel>,
    rows: Query<(&Interaction, &Row), Changed<Interaction>>,
    over: Query<&bevy::ui::RelativeCursorPosition, With<Panel>>,
    mut preview: ResMut<Preview>,
) {
    let typing = ui.as_ref().is_some_and(|u| u.typing);
    if !typing && keys.just_pressed(KeyCode::F8) { preview.open = !preview.open; }
    if !preview.open || preview.clips.is_empty() { wheel.clear(); return; }
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) { 10 } else { 1 };
    let mut delta = 0i32;
    if !typing && keys.just_pressed(KeyCode::ArrowDown) { delta += step; }
    if !typing && keys.just_pressed(KeyCode::ArrowUp) { delta -= step; }
    let hovered = over.iter().any(|c| c.cursor_over());
    for w in wheel.read() { if hovered { delta -= w.y.signum() as i32; } }
    for (interaction, row) in &rows {
        if *interaction == Interaction::Pressed { preview.selected = row.0; }
    }
    if delta != 0 {
        let last = preview.clips.len() as i32 - 1;
        preview.selected = (preview.selected as i32 + delta).clamp(0, last) as usize;
    }
}

/// The skeleton's clips, once loaded: the ones no player set names first.
fn list(
    mut preview: ResMut<Preview>,
    library: Option<Res<CharacterLibrary>>,
    gltfs: Res<Assets<Gltf>>,
    tables: Res<Assets<AnimTable>>,
    local: Query<&Character, With<crate::network::net::LocalPlayer>>,
) {
    if !preview.open || !preview.clips.is_empty() { return; }
    let (Some(library), Ok(character)) = (library, local.single()) else { return };
    let (Some(gltf), Some(table)) = (library.skeletons.get(character.desc.skeleton()).and_then(|h| gltfs.get(h)), tables.get(&library.anims)) else { return };
    let player_set = |set: &str| ["warrior", "mage", "ranger", "dwarf", "male", "female"].iter().any(|c| set.starts_with(c));
    let used: std::collections::HashSet<&String> = table.0.iter().filter(|(set, _)| player_set(set)).flat_map(|(_, rows)| rows.values()).collect();
    let mut unused: Vec<String> = gltf.named_animations.keys().map(|k| k.to_string()).filter(|k| !used.contains(k)).collect();
    let mut rest: Vec<String> = gltf.named_animations.keys().map(|k| k.to_string()).filter(|k| used.contains(k)).collect();
    unused.sort();
    rest.sort();
    preview.unused = unused.len();
    preview.clips = unused.into_iter().chain(rest).collect();
}

fn panel(mut commands: Commands, mut preview: ResMut<Preview>, panels: Query<Entity, With<Panel>>) {
    let first = preview.selected.saturating_sub(ROWS / 2).min(preview.clips.len().saturating_sub(ROWS));
    let key = Some((first, preview.selected, preview.open));
    if preview.shown == key { return; }
    preview.shown = key;
    for e in &panels { commands.entity(e).despawn(); }
    if !preview.open { return; }
    let font = |size: f32| TextFont { font_size: bevy::text::FontSize::Px(size), ..default() };
    commands.spawn((
        Panel, crate::ui::input::BlocksWorldInput,
        Node { position_type: PositionType::Absolute, right: Val::Px(16.0), top: Val::Px(90.0), width: Val::Px(560.0),
            flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(8.0)), row_gap: Val::Px(1.0), ..default() },
        BackgroundColor(Color::srgba(0.05, 0.04, 0.03, 0.88)), GlobalZIndex(1000),
    )).with_children(|p| {
        p.spawn((Text::new(format!("Animations {}/{} ({} unused by the classes)  F8 closes",
            preview.selected + 1, preview.clips.len(), preview.unused)), font(20.0), TextColor(Color::srgb(1.0, 0.85, 0.55))));
        if preview.clips.is_empty() {
            p.spawn((Text::new("Loading the skeleton's clips..."), font(20.0), TextColor(Color::WHITE)));
        }
        for i in first..(first + ROWS).min(preview.clips.len()) {
            let picked = i == preview.selected;
            let colour = if picked { Color::srgb(1.0, 0.8, 0.3) } else if i < preview.unused { Color::WHITE } else { Color::srgb(0.6, 0.6, 0.6) };
            p.spawn((Button, Row(i), Node { padding: UiRect::horizontal(Val::Px(4.0)), ..default() },
                BackgroundColor(if picked { Color::srgba(0.4, 0.3, 0.1, 0.6) } else { Color::NONE })))
                .with_children(|row| { row.spawn((Text::new(preview.clips[i].clone()), font(20.0), TextColor(colour))); });
        }
    });
}

/// Play the picked clip on our character, looping; give it back when closed.
fn play(
    mut commands: Commands,
    mut preview: ResMut<Preview>,
    library: Option<Res<CharacterLibrary>>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut local: Query<(Entity, &Character, &mut CharacterAnim), With<crate::network::net::LocalPlayer>>,
    mut players: Query<(&mut AnimationPlayer, &AnimationGraphHandle)>,
) {
    let wanted = preview.open.then(|| preview.clips.get(preview.selected).cloned()).flatten();
    if preview.playing == wanted { return; }
    let Ok((entity, character, mut anim)) = local.single_mut() else { return };
    let Some((mut player, handle)) = anim.player.and_then(|p| players.get_mut(p).ok()) else { return };
    player.stop_all();
    anim.current = None;
    anim.fading.clear();
    anim.applied = None;
    let Some(name) = wanted.clone() else {
        commands.entity(entity).remove::<Previewing>();
        preview.playing = None;
        return;
    };
    let Some(clip) = library.as_ref().and_then(|l| l.skeletons.get(character.desc.skeleton())).and_then(|h| gltfs.get(h))
        .and_then(|g| g.named_animations.get(name.as_str())).cloned() else { return };
    let Some(mut graph) = graphs.get_mut(&handle.0) else { return };
    let root = graph.root;
    let node = *anim.clip_nodes.entry(name.clone()).or_insert_with(|| graph.add_clip(clip, 1.0, root));
    player.play(node).repeat();
    commands.entity(entity).insert(Previewing);
    preview.playing = wanted;
    info!("previewing animation {name}");
}
