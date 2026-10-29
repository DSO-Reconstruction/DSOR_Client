//! Animation preview (developer aid): F8 opens a list, played on our character.
//!   - Clips: our skeleton's (characters/<skeleton>/__animations.glb), those
//!     no player animation set uses first (bosses, NPCs, mounts, emotes),
//!     looping, bare.
//!   - Sequences (Home cycles Clips, Sequences, Emotes): skills/sequences.json's with an animation,
//!     played whole -- their effects, lights, sounds and shakes -- in an
//!     animation set worn for it (its states are the set's: a boss sequence's
//!     Skill08_execute is gameentry_boss_grimmag's clip, not our class's). The
//!     set is guessed from the sequence's name; Left / Right change it.
//!   - Emotes: an emote played as the game does -- its animation, and its
//!     sequence for our class and gender (emotes.json: the vuvuzela in hand,
//!     its sound).
//! Picking: a click on a row, the wheel over the list, or Up / Down (Shift:
//! ten at a time).
//! FAILURE: "tu peux me faire un menu rapidement pour que je puisse les
//!   preview sur mon charactere?"; then "y'a des FX ou des props qui sont pas
//!   affiches" (a clip alone has none).
use super::*;
use crate::gameplay::skills::{EmoteTable, SequencePlayer, SequenceTable, SkillData, Track};
use bevy::input::mouse::MouseWheel;

/// On our character while a bare clip plays: drive_animations leaves it.
#[derive(Component)]
pub(crate) struct Previewing;

#[derive(Component)]
struct Panel;

#[derive(Component)]
struct Row(usize);

const ROWS: usize = 24;

#[derive(Default, PartialEq, Clone, Copy)]
enum Mode { #[default] Clips, Sequences, Emotes }

#[derive(Resource, Default)]
struct Preview {
    open: bool,
    mode: Mode,
    /// The clips, those no player set plays first, and how many those are.
    clips: Vec<String>,
    unused: usize,
    /// The sequences with an animation, and the animation sets our skeleton
    /// has clips of (the non-player ones first; a state without one is skipped).
    sequences: Vec<String>,
    sets: Vec<String>,
    set: usize,
    /// The emotes (emotes.json's names).
    emotes: Vec<String>,
    selected: [usize; 3],
    /// What the panel shows (first row, selected, open, mode, set).
    shown: Option<(usize, usize, bool, u8, usize)>,
    /// What plays: the mode, its clip or sequence, the set.
    playing: Option<(u8, String, usize)>,
    sequence: Option<Entity>,
    /// A sequence waiting for its set's clips to be bound.
    pending: Option<String>,
    /// DSOR_PREVIEW's pick: its set is guessed once the sets are known.
    pending_guess: bool,
}

impl Preview {
    fn rows(&self) -> &[String] {
        match self.mode { Mode::Clips => &self.clips, Mode::Sequences => &self.sequences, Mode::Emotes => &self.emotes }
    }
    fn index(&self) -> usize { self.selected[self.mode as usize] }
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
    if !preview.open { wheel.clear(); return; }
    if !typing && keys.just_pressed(KeyCode::Home) {
        preview.mode = match preview.mode { Mode::Clips => Mode::Sequences, Mode::Sequences => Mode::Emotes, Mode::Emotes => Mode::Clips };
    }
    let sets = preview.sets.len().max(1) as i32;
    if !typing && keys.just_pressed(KeyCode::ArrowRight) { preview.set = (preview.set as i32 + 1).rem_euclid(sets) as usize; }
    if !typing && keys.just_pressed(KeyCode::ArrowLeft) { preview.set = (preview.set as i32 - 1).rem_euclid(sets) as usize; }
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) { 10 } else { 1 };
    let mut delta = 0i32;
    if !typing && keys.just_pressed(KeyCode::ArrowDown) { delta += step; }
    if !typing && keys.just_pressed(KeyCode::ArrowUp) { delta -= step; }
    let hovered = over.iter().any(|c| c.cursor_over());
    for w in wheel.read() { if hovered { delta -= w.y.signum() as i32; } }
    let mode = preview.mode as usize;
    let mut picked = None;
    for (interaction, row) in &rows {
        if *interaction == Interaction::Pressed { picked = Some(row.0); }
    }
    let last = preview.rows().len() as i32 - 1;
    if last < 0 { return; }
    let before = preview.selected[mode];
    if let Some(row) = picked { preview.selected[mode] = row; }
    if delta != 0 { preview.selected[mode] = (preview.selected[mode] as i32 + delta).clamp(0, last) as usize; }
    // A new sequence wears the set its name points at.
    if preview.mode == Mode::Sequences && preview.selected[mode] != before {
        let name = preview.sequences[preview.selected[mode]].clone();
        if let Some(set) = guess_set(&name, &preview.sets) { preview.set = set; }
    }
}

/// The set sharing most words with a sequence's name (boss_grimmag_fireball:
/// gameentry_boss_grimmag), generic words aside.
fn guess_set(sequence: &str, sets: &[String]) -> Option<usize> {
    const GENERIC: [&str; 14] = ["boss", "skill", "death", "execute", "bullet", "impact", "loop", "spawn", "hit", "start", "end", "e", "01", "02"];
    let words: Vec<&str> = sequence.split('_').filter(|w| !GENERIC.contains(w) && w.len() > 1).collect();
    sets.iter().enumerate()
        .map(|(i, set)| (set.split('_').filter(|w| words.contains(w)).count(), std::cmp::Reverse(set.len()), i))
        .filter(|(n, ..)| *n > 0).max().map(|(.., i)| i)
}

/// The skeleton's clips, sets and the sequences, once loaded.
fn list(
    mut preview: ResMut<Preview>,
    library: Option<Res<CharacterLibrary>>,
    gltfs: Res<Assets<Gltf>>,
    tables: Res<Assets<AnimTable>>,
    skills: Option<Res<SkillData>>,
    (sequences, emotes): (Res<Assets<SequenceTable>>, Res<Assets<EmoteTable>>),
    local: Query<&Character, With<crate::network::net::LocalPlayer>>,
) {
    if !preview.open { return; }
    if preview.emotes.is_empty() {
        if let Some(table) = skills.as_ref().and_then(|s| emotes.get(s.emotes())) {
            let mut names: Vec<String> = table.0.keys().cloned().collect();
            names.sort();
            if let Some(i) = std::env::var("DSOR_PREVIEW").ok().and_then(|n| names.iter().position(|s| *s == n)) {
                preview.mode = Mode::Emotes;
                preview.selected[Mode::Emotes as usize] = i;
            }
            preview.emotes = names;
        }
    }
    if preview.sequences.is_empty() {
        if let Some(table) = skills.as_ref().and_then(|s| sequences.get(s.sequences())) {
            let mut names: Vec<String> = table.0.iter()
                .filter(|(_, s)| s.tracks.iter().any(|t| matches!(t, Track::Anim { .. }))).map(|(n, _)| n.clone()).collect();
            names.sort();
            // DSOR_PREVIEW=<sequence>: opened on it (screenshots).
            if let Some(i) = std::env::var("DSOR_PREVIEW").ok().and_then(|n| names.iter().position(|s| *s == n)) {
                preview.mode = Mode::Sequences;
                preview.selected[Mode::Sequences as usize] = i;
                preview.pending_guess = true;
            }
            preview.sequences = names;
        }
    }
    if !preview.clips.is_empty() { return; }
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
    let mut sets: Vec<String> = table.0.iter()
        .filter(|(_, rows)| rows.values().any(|clip| gltf.named_animations.contains_key(clip.as_str())))
        .map(|(set, _)| set.clone()).collect();
    sets.sort_by_key(|s| (player_set(s), s.clone()));
    preview.set = sets.iter().position(|s| *s == character.displayed_set).unwrap_or(0);
    if std::mem::take(&mut preview.pending_guess) {
        let name = preview.sequences[preview.selected[Mode::Sequences as usize]].clone();
        if let Some(set) = guess_set(&name, &sets) { preview.set = set; }
    }
    preview.sets = sets;
}

fn panel(mut commands: Commands, mut preview: ResMut<Preview>, panels: Query<Entity, With<Panel>>) {
    let rows = preview.rows().len();
    let selected = preview.index();
    let first = selected.saturating_sub(ROWS / 2).min(rows.saturating_sub(ROWS));
    let key = Some((first, selected, preview.open, preview.mode as u8, preview.set));
    if preview.shown == key { return; }
    preview.shown = key;
    for e in &panels { commands.entity(e).despawn(); }
    if !preview.open { return; }
    let font = |size: f32| TextFont { font_size: bevy::text::FontSize::Px(size), ..default() };
    let heading = match preview.mode {
        Mode::Clips => format!("Clips {}/{} ({} unused by the classes)\nHome: sequences   F8: close", selected + 1, rows, preview.unused),
        Mode::Sequences => format!("Sequences {}/{} (animation, effects, sounds)\nSet: {}  (Left / Right)\nHome: emotes   F8: close",
            selected + 1, rows, preview.sets.get(preview.set).map_or("-", String::as_str)),
        Mode::Emotes => format!("Emotes {}/{} (animation, prop, sound)\nHome: clips   F8: close", selected + 1, rows),
    };
    commands.spawn((
        Panel, crate::ui::input::BlocksWorldInput,
        Node { position_type: PositionType::Absolute, right: Val::Px(16.0), top: Val::Px(90.0), width: Val::Px(560.0),
            flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(8.0)), row_gap: Val::Px(1.0), ..default() },
        BackgroundColor(Color::srgba(0.05, 0.04, 0.03, 0.88)), GlobalZIndex(1000),
    )).with_children(|p| {
        p.spawn((Text::new(heading), font(20.0), TextColor(Color::srgb(1.0, 0.85, 0.55))));
        if rows == 0 {
            p.spawn((Text::new("Loading..."), font(20.0), TextColor(Color::WHITE)));
        }
        for i in first..(first + ROWS).min(rows) {
            let picked = i == selected;
            let grey = preview.mode == Mode::Clips && i >= preview.unused;
            let colour = if picked { Color::srgb(1.0, 0.8, 0.3) } else if grey { Color::srgb(0.6, 0.6, 0.6) } else { Color::WHITE };
            p.spawn((Button, Row(i), Node { padding: UiRect::horizontal(Val::Px(4.0)), ..default() },
                BackgroundColor(if picked { Color::srgba(0.4, 0.3, 0.1, 0.6) } else { Color::NONE })))
                .with_children(|row| { row.spawn((Text::new(preview.rows()[i].clone()), font(20.0), TextColor(colour))); });
        }
    });
}

/// Play the pick on our character; give it back when closed.
#[allow(clippy::too_many_arguments)]
fn play(
    mut commands: Commands,
    mut preview: ResMut<Preview>,
    library: Option<Res<CharacterLibrary>>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut local: Query<(Entity, &mut Character, &mut CharacterAnim), With<crate::network::net::LocalPlayer>>,
    mut players: Query<(&mut AnimationPlayer, &AnimationGraphHandle)>,
    (skills, sequences, emotes, mut sequence_players): (Option<Res<SkillData>>, Res<Assets<SequenceTable>>, Res<Assets<EmoteTable>>, Query<&mut SequencePlayer>),
) {
    let Ok((entity, mut character, mut anim)) = local.single_mut() else { return };
    // A sequence waits for its set's clips (update_animation_sets binds them).
    if let Some(name) = preview.pending.clone() {
        if anim.set.is_some() && anim.set == character.preview_set {
            if let Some(seq) = skills.as_ref().and_then(|s| sequences.get(s.sequences())).and_then(|t| t.0.get(&name)) {
                preview.sequence = Some(crate::gameplay::skills::play(&mut commands, seq.clone(), Some(entity), entity, true));
                info!("previewing sequence {name} in {:?}", character.preview_set);
            }
            preview.pending = None;
        }
    }
    let wanted = preview.open.then(|| preview.rows().get(preview.index()).map(|n| (preview.mode as u8, n.clone(), preview.set))).flatten();
    if preview.playing == wanted { return; }
    // Whatever played before ends.
    if let Some(old) = preview.sequence.take() {
        if let Ok(mut p) = sequence_players.get_mut(old) { p.stop = true; }
    }
    preview.pending = None;
    character.preview_set = None;
    commands.entity(entity).remove::<Previewing>();
    if let Some((mut player, _)) = anim.player.and_then(|p| players.get_mut(p).ok()) { player.stop_all(); }
    anim.current = None;
    anim.fading.clear();
    anim.applied = None;
    anim.state = AnimState::Idle;
    preview.playing = wanted.clone();
    let Some((mode, name, set)) = wanted else { return };
    if mode == Mode::Sequences as u8 {
        character.preview_set = preview.sets.get(set).cloned();
        preview.pending = Some(name);
        return;
    }
    // An emote: its animation state in our own set, and our class's sequence.
    if mode == Mode::Emotes as u8 {
        anim.state = AnimState::Named(name.clone());
        anim.replay();
        let sequence = skills.as_ref().and_then(|s| emotes.get(s.emotes())).and_then(|t| t.sequence(&name, &character)).cloned();
        if let Some(seq) = sequence.as_ref().and_then(|n| skills.as_ref().and_then(|s| sequences.get(s.sequences())).and_then(|t| t.0.get(n))) {
            preview.sequence = Some(crate::gameplay::skills::play(&mut commands, seq.clone(), Some(entity), entity, true));
        }
        info!("previewing emote {name} ({sequence:?})");
        return;
    }
    let Some((mut player, handle)) = anim.player.and_then(|p| players.get_mut(p).ok()) else { return };
    let Some(clip) = library.as_ref().and_then(|l| l.skeletons.get(character.desc.skeleton())).and_then(|h| gltfs.get(h))
        .and_then(|g| g.named_animations.get(name.as_str())).cloned() else { return };
    let Some(mut graph) = graphs.get_mut(&handle.0) else { return };
    let root = graph.root;
    let node = *anim.clip_nodes.entry(name.clone()).or_insert_with(|| graph.add_clip(clip, 1.0, root));
    player.play(node).repeat();
    commands.entity(entity).insert(Previewing);
    info!("previewing animation {name}");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_boss_sequence_wears_its_bosses_set() {
        let sets: Vec<String> = ["warrior_2h_weapon", "gameentry_boss_grimmag", "gameentry_boss_grimmag_spawner", "e_dra_02_black_knight"]
            .iter().map(|s| s.to_string()).collect();
        assert_eq!(guess_set("boss_grimmag_fireball", &sets), Some(1));
        assert_eq!(guess_set("e_dra_02_black_knight_skill01", &sets), Some(3));
        assert_eq!(guess_set("nothing_alike", &sets), None);
    }
}
