//! The debug menu (F3): spawn any monster, model or effect, play any sequence, hit
//! and kill what was spawned -- to look at the 2018 data without a server.
//!
//! - Tabs: Monsters (`_Template_Monster`, characters/monster_templates.json),
//!   Models and Effects (every converted model, debug_index.json from
//!   tools/export_debug.py; effects are the effects*/ ones), Sequences
//!   (skills/sequences.json, played on the last spawned monster, else on the
//!   player).
//! - While the menu is open the keyboard types into its filter (the game sees no
//!   key): Enter spawns the first result, Up / Down page through them, Escape
//!   clears the filter, F3 closes.
//! - Hit / Crit / Kill: blows on the last spawned monster, drawn as the server's
//!   HitCommand / KillCommand are (crate::monsters). Clear removes every spawn.
//! Things land 3 units in front of the player, or at the camera's focus.
//! The debug map: `?map=debug` (natively `debug` as the map): an empty floor.

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::gltf::GltfAssetLabel;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use serde::Deserialize;

use crate::monsters::{DebugMonsters, Monster, MonsterTemplates};
use crate::skills::{play, SequenceTable, SkillData};

#[derive(Asset, TypePath, Deserialize, Debug)]
pub struct DebugIndex {
    pub models: Vec<String>,
}

#[derive(Default, TypePath)]
pub struct DebugIndexLoader;

impl AssetLoader for DebugIndexLoader {
    type Asset = DebugIndex;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<DebugIndex, Self::Error> {
        let mut b = Vec::new();
        r.read_to_end(&mut b).await?;
        serde_json::from_slice(&b).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] {
        &["debug_index.json"]
    }
}

const TABS: [&str; 4] = ["Monsters", "Models", "Effects", "Sequences"];
const ROWS: usize = 16;
/// Actors of debug monsters (the server's are far below).
const FIRST_ACTOR: u32 = 0x6100_0000;

#[derive(Resource)]
struct Debug {
    open: bool,
    tab: usize,
    filter: String,
    page: usize,
    /// What the rows show (full names), rebuilt when something changes.
    results: Vec<String>,
    total: usize,
    dirty: bool,
    index: Handle<DebugIndex>,
    monsters: Handle<MonsterTemplates>,
    next_actor: u32,
    last_monster: Option<u32>,
    status: String,
}

/// Something the menu spawned (Clear removes it).
#[derive(Component)]
struct Spawned;

#[derive(Component)]
struct Panel;
#[derive(Component)]
struct TabButton(usize);
#[derive(Component)]
struct Row(usize);
#[derive(Component)]
struct Action(u8);
#[derive(Component)]
struct FilterText;
#[derive(Component)]
struct StatusText;

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<DebugIndex>()
            .register_asset_loader(DebugIndexLoader)
            .add_systems(Startup, setup)
            .add_systems(PreUpdate, keyboard.after(bevy::input::InputSystems))
            .add_systems(Update, (buttons, refresh).chain());
    }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Debug {
        open: std::env::var("DSOR_DEBUG_MENU").is_ok(),
        tab: 0,
        filter: String::new(),
        page: 0,
        results: Vec::new(),
        total: 0,
        dirty: true,
        index: assets.load("debug_index.json"),
        monsters: assets.load("characters/monster_templates.json"),
        next_actor: FIRST_ACTOR,
        last_monster: None,
        status: String::new(),
    });
    let font = TextFont { font_size: bevy::text::FontSize::Px(13.0), ..default() };
    let button = |w: f32| (Button, Node { width: Val::Px(w), padding: UiRect::axes(Val::Px(4.0), Val::Px(1.0)), ..default() }, BackgroundColor(Color::srgb(0.22, 0.22, 0.28)));
    commands
        .spawn((
            Panel,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(8.0),
                top: Val::Px(70.0),
                width: Val::Px(440.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn((Text::new("Debug (F3): type to filter, Enter spawns the first, Up/Down pages"), font.clone(), TextColor(Color::srgb(1.0, 0.85, 0.4))));
            p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
                for (i, t) in TABS.iter().enumerate() {
                    r.spawn((TabButton(i), button(100.0))).with_children(|b| {
                        b.spawn((Text::new(*t), font.clone()));
                    });
                }
            });
            p.spawn((FilterText, Text::new("> "), font.clone()));
            for i in 0..ROWS {
                p.spawn((Row(i), button(424.0))).with_children(|b| {
                    b.spawn((Text::new(""), font.clone()));
                });
            }
            p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
                for (i, t) in ["Hit", "Crit", "Kill", "Clear"].iter().enumerate() {
                    r.spawn((Action(i as u8), button(100.0))).with_children(|b| {
                        b.spawn((Text::new(*t), font.clone()));
                    });
                }
            });
            p.spawn((StatusText, Text::new(""), font.clone(), TextColor(Color::srgb(0.7, 0.9, 1.0))));
        });
}

/// F3, and the filter's typing; the game sees no key while the menu is open.
#[allow(clippy::too_many_arguments)]
fn keyboard(
    mut commands: Commands,
    mut events: MessageReader<KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut debug: ResMut<Debug>,
    mut panel: Query<&mut Visibility, With<Panel>>,
    mut spawn: SpawnCtx,
) {
    let mut enter = false;
    for ev in events.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        if ev.key_code == KeyCode::F3 {
            debug.open = !debug.open;
            continue;
        }
        if !debug.open {
            continue;
        }
        match &ev.logical_key {
            Key::Backspace => {
                debug.filter.pop();
            }
            Key::Escape => debug.filter.clear(),
            Key::Enter => enter = true,
            Key::ArrowDown => debug.page += 1,
            Key::ArrowUp => debug.page = debug.page.saturating_sub(1),
            Key::Character(c) => debug.filter.push_str(c),
            Key::Space => debug.filter.push(' '),
            _ => continue,
        }
        if !matches!(&ev.logical_key, Key::ArrowDown | Key::ArrowUp) {
            debug.page = 0;
        }
        debug.dirty = true;
    }
    for mut v in &mut panel {
        let want = if debug.open { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if debug.open {
        keys.reset_all();
        if enter {
            if let Some(name) = debug.results.first().cloned() {
                spawn.spawn(&mut commands, &mut debug, &name);
            }
        }
    }
}

/// What spawning needs.
#[derive(bevy::ecs::system::SystemParam)]
struct SpawnCtx<'w, 's> {
    assets: Res<'w, AssetServer>,
    cameras: Query<'w, 's, &'static GlobalTransform, With<Camera3d>>,
    players: Query<'w, 's, (Entity, &'static GlobalTransform), Or<(With<crate::net::LocalPlayer>, With<crate::character::Character>)>>,
    locals: Query<'w, 's, Entity, With<crate::net::LocalPlayer>>,
    monsters: Query<'w, 's, (Entity, &'static Monster)>,
    skills: Res<'w, SkillData>,
    seqs: Res<'w, Assets<SequenceTable>>,
    debug_monsters: ResMut<'w, DebugMonsters>,
}

impl SpawnCtx<'_, '_> {
    /// Where things land: in front of the player (or the demo character), else the
    /// camera's focus on the ground.
    fn place(&self) -> Vec3 {
        let player = self.locals.iter().next().and_then(|e| self.players.get(e).ok()).or_else(|| self.players.iter().next());
        if let Some((_, at)) = player {
            // Characters face +Z (crate::net turns them with atan2(dx, dz)).
            return at.translation() + at.back().as_vec3() * 3.0;
        }
        self.cameras.iter().next().map(|c| crate::map::camera_focus(c, None, 0.0)).unwrap_or_default()
    }

    fn spawn(&mut self, commands: &mut Commands, debug: &mut Debug, name: &str) {
        let at = self.place();
        let tab = TABS[debug.tab.min(3)];
        info!("debug: {tab} {name} at {at:?}");
        match debug.tab {
            0 => {
                let actor = debug.next_actor;
                debug.next_actor += 1;
                debug.last_monster = Some(actor);
                self.debug_monsters.spawns.push(crate::net::MonsterSpawn { actor, blueprint: name.to_owned(), position: at, health: 100, level: 1 });
                debug.status = format!("monster {name} (actor {actor:#x})");
            }
            1 | 2 => {
                let path = format!("{name}.glb");
                commands.spawn((
                    Spawned,
                    Name::new(format!("debug {name}")),
                    WorldAssetRoot(self.assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
                    Transform::from_translation(at),
                    Visibility::default(),
                ));
                debug.status = format!("model {name} at {:.1} {:.1} {:.1}", at.x, at.y, at.z);
            }
            _ => {
                let Some(seq) = self.seqs.get(self.skills.sequences()).and_then(|t| t.0.get(name)).cloned() else { return };
                // On the last spawned monster, else the player, else on the ground.
                let subject = debug
                    .last_monster
                    .and_then(|a| self.monsters.iter().find(|(_, m)| m.actor == a).map(|(e, _)| e))
                    .or_else(|| self.locals.iter().next())
                    .or_else(|| self.players.iter().next().map(|(e, _)| e));
                match subject {
                    Some(e) => {
                        play(commands, seq, Some(e), e, false);
                    }
                    None => {
                        let anchor = commands.spawn((Spawned, Transform::from_translation(at), Visibility::default())).id();
                        play(commands, seq, None, anchor, false);
                    }
                }
                debug.status = format!("sequence {name}");
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn buttons(
    mut commands: Commands,
    mut debug: ResMut<Debug>,
    tabs: Query<(&Interaction, &TabButton), Changed<Interaction>>,
    rows: Query<(&Interaction, &Row), Changed<Interaction>>,
    actions: Query<(&Interaction, &Action), Changed<Interaction>>,
    spawned: Query<Entity, With<Spawned>>,
    mut spawn: SpawnCtx,
    mut scripted: Local<bool>,
    time: Res<Time>,
) {
    // DSOR_DEBUG_SPAWN=<tab>:<name>;... (tab 0 monsters, 1 models, 2 effects,
    // 3 sequences): spawned once, two seconds in (testing without a mouse).
    if !*scripted && time.elapsed_secs() > 2.0 {
        *scripted = true;
        if let Ok(list) = std::env::var("DSOR_DEBUG_SPAWN") {
            for item in list.split(';') {
                let Some((tab, name)) = item.split_once(':') else { continue };
                let keep = debug.tab;
                debug.tab = tab.parse().unwrap_or(0);
                spawn.spawn(&mut commands, &mut debug, name);
                debug.tab = keep;
            }
        }
    }
    for (i, TabButton(t)) in &tabs {
        if *i == Interaction::Pressed && debug.tab != *t {
            debug.tab = *t;
            debug.page = 0;
            debug.dirty = true;
        }
    }
    for (i, Row(r)) in &rows {
        if *i == Interaction::Pressed {
            if let Some(name) = debug.results.get(*r).cloned() {
                spawn.spawn(&mut commands, &mut debug, &name);
            }
        }
    }
    for (i, Action(a)) in &actions {
        if *i != Interaction::Pressed {
            continue;
        }
        match a {
            0..=2 => {
                if let Some(actor) = debug.last_monster {
                    spawn.debug_monsters.blows.push((actor, *a));
                }
            }
            _ => {
                for e in &spawned {
                    commands.entity(e).despawn();
                }
                for (e, m) in &spawn.monsters {
                    if m.actor >= FIRST_ACTOR {
                        commands.entity(e).despawn();
                    }
                }
                debug.last_monster = None;
                debug.status = "cleared".into();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn refresh(
    mut debug: ResMut<Debug>,
    index: Res<Assets<DebugIndex>>,
    monsters: Res<Assets<MonsterTemplates>>,
    skills: Res<SkillData>,
    seqs: Res<Assets<SequenceTable>>,
    rows: Query<(&Row, &Children)>,
    mut texts: Query<&mut Text>,
    filter_text: Query<Entity, With<FilterText>>,
    status_text: Query<Entity, With<StatusText>>,
    tabs: Query<(&TabButton, &mut BackgroundColor)>,
) {
    if !debug.open {
        return;
    }
    // The data comes in after the menu: rebuild once it is there.
    let loaded = (index.get(&debug.index).is_some(), monsters.get(&debug.monsters).is_some(), seqs.get(skills.sequences()).is_some());
    if !debug.dirty && !debug.is_changed() && loaded == (true, true, true) {
        return;
    }
    let words: Vec<String> = debug.filter.to_lowercase().split_whitespace().map(str::to_owned).collect();
    let keep = |s: &str| {
        let l = s.to_lowercase();
        words.iter().all(|w| l.contains(w.as_str()))
    };
    let mut all: Vec<String> = match debug.tab {
        0 => monsters.get(&debug.monsters).map(|m| m.0.keys().filter(|k| keep(k)).cloned().collect()).unwrap_or_default(),
        1 => index.get(&debug.index).map(|i| i.models.iter().filter(|m| !m.starts_with("effects") && keep(m)).cloned().collect()).unwrap_or_default(),
        2 => index.get(&debug.index).map(|i| i.models.iter().filter(|m| m.starts_with("effects") && keep(m)).cloned().collect()).unwrap_or_default(),
        _ => seqs.get(skills.sequences()).map(|s| s.0.keys().filter(|k| keep(k)).cloned().collect()).unwrap_or_default(),
    };
    all.sort();
    let pages = all.len().div_ceil(ROWS).max(1);
    let page = debug.page.min(pages - 1);
    debug.total = all.len();
    debug.results = all.into_iter().skip(page * ROWS).take(ROWS).collect();
    debug.dirty = false;
    for (Row(i), kids) in &rows {
        if let Some(mut t) = kids.first().and_then(|k| texts.get_mut(*k).ok()) {
            t.0 = debug.results.get(*i).cloned().unwrap_or_default();
        }
    }
    if let Some(mut t) = filter_text.iter().next().and_then(|e| texts.get_mut(e).ok()) {
        t.0 = format!("> {}_   ({} found, page {}/{})", debug.filter, debug.total, page + 1, pages);
    }
    if let Some(mut t) = status_text.iter().next().and_then(|e| texts.get_mut(e).ok()) {
        t.0 = debug.status.clone();
    }
    for (TabButton(t), mut bg) in tabs {
        bg.0 = if *t == debug.tab { Color::srgb(0.45, 0.35, 0.15) } else { Color::srgb(0.22, 0.22, 0.28) };
    }
}
