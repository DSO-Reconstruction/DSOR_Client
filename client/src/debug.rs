//! The debug menu (F3): spawn any monster, model or effect, play any sequence or
//! animation, strike what was spawned, play a character of any class -- to look at
//! the 2018 data and try skills without a server.
//!
//! Tabs:
//! - Monsters (`_Template_Monster`), Models and Effects (every converted model,
//!   debug_index.json from tools/export_debug.py), Sequences (skills/sequences.json,
//!   played on the selected monster, else on the player): a row spawns / plays it.
//! - Spawned: what the menu put on the map; a row selects it (Delete removes it).
//! - Anims: the selected monster's animation states (its AnimSet rows that its
//!   model has clips for); a row plays it, looping ones loop.
//! - Heroes: the offline character's class and gender (switches in place).
//! - Skills: the class's skills; a row puts it on the right mouse button.
//! While the menu is open the keyboard types into its filter (the game sees no
//! key): Enter acts on the first row, Up / Down page, Escape clears, F3 closes.
//! Hit / Crit / Kill strike the selected monster as the server's HitCommand /
//! KillCommand would (crate::monsters); Clear removes every spawn.
//! Things land 3 units in front of the player, or at the camera's focus.
//! The debug map: `?map=debug` (natively `debug` as the map): an empty floor and,
//! with no server, a playable character (crate::net::start_offline).

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::gltf::GltfAssetLabel;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use serde::Deserialize;

use crate::character::{AnimState, CharacterAnim, CharacterDesc};
use crate::monsters::{DebugMonsters, Monster, MonsterTemplates};
use crate::net::Net;
use crate::skills::{play, SequenceTable, SkillData};
use crate::stats::StatsData;

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

const TABS: [&str; 8] = ["Monsters", "Models", "Effects", "Sequences", "Spawned", "Anims", "Heroes", "Skills"];
const MONSTERS: usize = 0;
const MODELS: usize = 1;
const EFFECTS: usize = 2;
const SEQUENCES: usize = 3;
const SPAWNED: usize = 4;
const ANIMS: usize = 5;
const HEROES: usize = 6;
const ROWS: usize = 16;
/// Actors of debug monsters (the server's are far below).
const FIRST_ACTOR: u32 = 0x6100_0000;
const CLASSES: [&str; 4] = ["warrior", "mage", "ranger", "dwarf"];

/// Something the menu put on the map.
#[derive(Clone, Copy, PartialEq)]
enum Ref {
    /// A monster, by actor (its entity comes later, crate::monsters).
    Monster(u32),
    Entity(Entity),
}

/// What a row does.
#[derive(Clone)]
enum Act {
    Spawn(String),
    Select(usize),
    Anim(String),
    Hero(u8, u8),
    Skill(String),
}

#[derive(Resource)]
struct Debug {
    open: bool,
    tab: usize,
    filter: String,
    page: usize,
    /// The rows shown: label and what a click does.
    rows: Vec<(String, Act)>,
    total: usize,
    dirty: bool,
    index: Handle<DebugIndex>,
    monsters: Handle<MonsterTemplates>,
    stats: Handle<StatsData>,
    next_actor: u32,
    spawned: Vec<(Ref, String)>,
    selected: Option<usize>,
    status: String,
}

impl Debug {
    /// The monster Hit / Crit / Kill / Anims / Sequences act on: the selected
    /// spawn if it is one, else the last monster spawned.
    fn monster(&self) -> Option<u32> {
        let selected = self.selected.and_then(|i| self.spawned.get(i)).and_then(|(r, _)| match r {
            Ref::Monster(a) => Some(*a),
            Ref::Entity(_) => None,
        });
        selected.or_else(|| {
            self.spawned.iter().rev().find_map(|(r, _)| match r {
                Ref::Monster(a) => Some(*a),
                Ref::Entity(_) => None,
            })
        })
    }
}

/// Something the menu spawned (other than monsters).
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
        tab: MONSTERS,
        filter: String::new(),
        page: 0,
        rows: Vec::new(),
        total: 0,
        dirty: true,
        index: assets.load("debug_index.json"),
        monsters: assets.load("characters/monster_templates.json"),
        stats: assets.load("characters/stats.json"),
        next_actor: FIRST_ACTOR,
        spawned: Vec::new(),
        selected: None,
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
            p.spawn((Text::new("Debug (F3): type to filter, Enter = first row, Up/Down pages"), font.clone(), TextColor(Color::srgb(1.0, 0.85, 0.4))));
            p.spawn(Node { flex_direction: FlexDirection::Row, flex_wrap: FlexWrap::Wrap, column_gap: Val::Px(4.0), row_gap: Val::Px(2.0), ..default() })
                .with_children(|r| {
                    for (i, t) in TABS.iter().enumerate() {
                        r.spawn((TabButton(i), button(102.0))).with_children(|b| {
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
                for (i, t) in ["Hit", "Crit", "Kill", "Delete", "Clear"].iter().enumerate() {
                    r.spawn((Action(i as u8), button(80.0))).with_children(|b| {
                        b.spawn((Text::new(*t), font.clone()));
                    });
                }
            });
            p.spawn((StatusText, Text::new(""), font.clone(), TextColor(Color::srgb(0.7, 0.9, 1.0))));
        });
}

/// F3, and the filter's typing; the game sees no key while the menu is open.
fn keyboard(
    mut events: MessageReader<KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut debug: ResMut<Debug>,
    mut panel: Query<&mut Visibility, With<Panel>>,
    mut ctx: Ctx,
) {
    let mut enter = false;
    for ev in events.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        if ev.key_code == KeyCode::F3 {
            debug.open = !debug.open;
            debug.dirty = true;
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
            if let Some((_, act)) = debug.rows.first().cloned() {
                ctx.act(&mut debug, act);
            }
        }
    }
}

/// What acting needs.
#[derive(bevy::ecs::system::SystemParam)]
struct Ctx<'w, 's> {
    commands: Commands<'w, 's>,
    assets: Res<'w, AssetServer>,
    cameras: Query<'w, 's, &'static GlobalTransform, With<Camera3d>>,
    players: Query<'w, 's, (Entity, &'static GlobalTransform), Or<(With<crate::net::LocalPlayer>, With<crate::character::Character>)>>,
    locals: Query<'w, 's, (Entity, &'static GlobalTransform, &'static crate::net::LocalPlayer)>,
    monsters: Query<'w, 's, (Entity, &'static Monster)>,
    anims: Query<'w, 's, &'static mut CharacterAnim>,
    skills: Res<'w, SkillData>,
    seqs: Res<'w, Assets<SequenceTable>>,
    debug_monsters: ResMut<'w, DebugMonsters>,
    net: Option<NonSendMut<'w, Net>>,
}

impl Ctx<'_, '_> {
    /// Where things land: in front of the player (or the demo character), else the
    /// camera's focus on the ground.
    fn place(&self) -> Vec3 {
        let player = self.locals.iter().next().map(|(e, g, _)| (e, g)).or_else(|| self.players.iter().next());
        if let Some((_, at)) = player {
            // Characters face +Z (crate::net turns them with atan2(dx, dz)).
            return at.translation() + at.back().as_vec3() * 3.0;
        }
        self.cameras.iter().next().map(|c| crate::map::camera_focus(c, None, 0.0)).unwrap_or_default()
    }

    fn monster_entity(&self, actor: u32) -> Option<Entity> {
        self.monsters.iter().find(|(_, m)| m.actor == actor).map(|(e, _)| e)
    }

    fn act(&mut self, debug: &mut Debug, act: Act) {
        let at = self.place();
        match act {
            Act::Spawn(name) => self.spawn(debug, &name, at),
            Act::Select(i) => {
                debug.selected = Some(i);
                debug.status = format!("selected {}", debug.spawned.get(i).map(|(_, n)| n.as_str()).unwrap_or(""));
            }
            Act::Anim(state) => {
                let Some(e) = debug.monster().and_then(|a| self.monster_entity(a)) else { return };
                if let Ok(mut a) = self.anims.get_mut(e) {
                    a.dead = false;
                    a.state = if state == "Idle" { AnimState::Idle } else { AnimState::Named(state.clone()) };
                    a.replay();
                    debug.status = format!("animation {state}");
                }
            }
            Act::Hero(class, gender) => {
                let Some(net) = self.net.as_mut().filter(|n| n.offline) else {
                    debug.status = "only offline (no server)".into();
                    return;
                };
                // Re-made where the current one stands; its own sequences end with it.
                let (pos, facing) = self.locals.iter().next().map(|(_, g, l)| (g.translation(), l.facing)).unwrap_or((at, 0.0));
                net.bar = crate::net::default_bar(class);
                net.queue_local(CharacterDesc { class, gender, ..default() }, pos, facing);
                debug.status = format!("playing {} ({})", CLASSES[class as usize], if gender == 1 { "female" } else { "male" });
            }
            Act::Skill(id) => {
                let Some(net) = self.net.as_mut() else { return };
                if net.bar.len() < 2 {
                    net.bar.resize(2, None);
                }
                net.bar[1] = Some(id.clone());
                debug.status = format!("right button: {id}");
            }
        }
        debug.dirty = true;
    }

    fn spawn(&mut self, debug: &mut Debug, name: &str, at: Vec3) {
        info!("debug: spawn {name} at {at:?}");
        match debug.tab {
            MONSTERS => {
                let actor = debug.next_actor;
                debug.next_actor += 1;
                self.debug_monsters.spawns.push(crate::net::MonsterSpawn { actor, blueprint: name.to_owned(), position: at, health: 100, level: 1 });
                debug.spawned.push((Ref::Monster(actor), format!("monster {name}")));
                debug.selected = Some(debug.spawned.len() - 1);
                debug.status = format!("monster {name}");
            }
            MODELS | EFFECTS => {
                let e = self
                    .commands
                    .spawn((
                        Spawned,
                        Name::new(format!("debug {name}")),
                        WorldAssetRoot(self.assets.load(GltfAssetLabel::Scene(0).from_asset(format!("{name}.glb")))),
                        Transform::from_translation(at),
                        Visibility::default(),
                    ))
                    .id();
                debug.spawned.push((Ref::Entity(e), name.to_owned()));
                debug.status = format!("{name} at {:.1} {:.1} {:.1}", at.x, at.y, at.z);
            }
            _ => {
                let Some(seq) = self.seqs.get(self.skills.sequences()).and_then(|t| t.0.get(name)).cloned() else { return };
                // On the selected monster, else the player, else on the ground.
                let subject = debug
                    .monster()
                    .and_then(|a| self.monster_entity(a))
                    .or_else(|| self.locals.iter().next().map(|(e, _, _)| e))
                    .or_else(|| self.players.iter().next().map(|(e, _)| e));
                match subject {
                    Some(e) => {
                        play(&mut self.commands, seq, Some(e), e, false);
                    }
                    None => {
                        let anchor = self.commands.spawn((Spawned, Transform::from_translation(at), Visibility::default())).id();
                        debug.spawned.push((Ref::Entity(anchor), format!("sequence {name}")));
                        play(&mut self.commands, seq, None, anchor, false);
                    }
                }
                debug.status = format!("sequence {name}");
            }
        }
        debug.dirty = true;
    }

    fn remove(&mut self, r: Ref) {
        match r {
            Ref::Entity(e) => {
                self.commands.entity(e).try_despawn();
            }
            Ref::Monster(a) => {
                if let Some(e) = self.monster_entity(a) {
                    self.commands.entity(e).try_despawn();
                }
            }
        }
    }
}

fn buttons(
    mut debug: ResMut<Debug>,
    tabs: Query<(&Interaction, &TabButton), Changed<Interaction>>,
    rows: Query<(&Interaction, &Row), Changed<Interaction>>,
    actions: Query<(&Interaction, &Action), Changed<Interaction>>,
    mut ctx: Ctx,
    mut scripted: Local<bool>,
    time: Res<Time>,
) {
    // DSOR_DEBUG_SPAWN=<tab>:<name>;... (tab 0 monsters, 1 models, 2 effects,
    // 3 sequences): spawned once, two seconds in (testing without a mouse).
    if !*scripted && time.elapsed_secs() > 2.0 {
        *scripted = true;
        if let Ok(list) = std::env::var("DSOR_DEBUG_SPAWN") {
            for (i, item) in list.split(';').enumerate() {
                let Some((tab, name)) = item.split_once(':') else { continue };
                let keep = debug.tab;
                debug.tab = tab.parse().unwrap_or(MONSTERS);
                // In a row, 3 units apart, so one look shows them all.
                let at = ctx.place() + Vec3::X * (i as f32 * 3.0 - 6.0);
                ctx.spawn(&mut debug, name, at);
                debug.tab = keep;
            }
        }
    }
    for (i, TabButton(t)) in &tabs {
        if *i == Interaction::Pressed && debug.tab != *t {
            debug.tab = *t;
            debug.page = 0;
            debug.filter.clear();
            debug.dirty = true;
        }
    }
    for (i, Row(r)) in &rows {
        if *i == Interaction::Pressed {
            if let Some((_, act)) = debug.rows.get(*r).cloned() {
                ctx.act(&mut debug, act);
            }
        }
    }
    for (i, Action(a)) in &actions {
        if *i != Interaction::Pressed {
            continue;
        }
        match a {
            0..=2 => match debug.monster() {
                Some(actor) => ctx.debug_monsters.blows.push((actor, *a)),
                None => debug.status = "no monster spawned".into(),
            },
            3 => {
                // Delete the selected spawn (else the last one).
                let i = debug.selected.filter(|i| *i < debug.spawned.len()).or(debug.spawned.len().checked_sub(1));
                if let Some(i) = i {
                    let (r, name) = debug.spawned.remove(i);
                    ctx.remove(r);
                    debug.status = format!("deleted {name}");
                }
                debug.selected = None;
            }
            _ => {
                for (r, _) in std::mem::take(&mut debug.spawned) {
                    ctx.remove(r);
                }
                debug.selected = None;
                debug.status = "cleared".into();
            }
        }
        debug.dirty = true;
    }
}

#[allow(clippy::too_many_arguments)]
fn refresh(
    mut debug: ResMut<Debug>,
    index: Res<Assets<DebugIndex>>,
    monsters: Res<Assets<MonsterTemplates>>,
    stats: Res<Assets<StatsData>>,
    skills: Res<SkillData>,
    seqs: Res<Assets<SequenceTable>>,
    live: Query<(&Monster, &CharacterAnim)>,
    players: Query<&crate::character::Character, With<crate::net::LocalPlayer>>,
    rows: Query<(&Row, &Children)>,
    mut texts: Query<&mut Text>,
    filter_text: Query<Entity, With<FilterText>>,
    status_text: Query<Entity, With<StatusText>>,
    tabs: Query<(&TabButton, &mut BackgroundColor)>,
    mut every: Local<f32>,
    time: Res<Time>,
) {
    if !debug.open {
        return;
    }
    // The data comes in after the menu, and the Anims / Spawned tabs follow the
    // world: rebuilt when something changed, or twice a second.
    *every -= time.delta_secs();
    if !debug.dirty && *every > 0.0 {
        return;
    }
    *every = 0.5;
    let words: Vec<String> = debug.filter.to_lowercase().split_whitespace().map(str::to_owned).collect();
    let keep = |s: &str| {
        let l = s.to_lowercase();
        words.iter().all(|w| l.contains(w.as_str()))
    };
    let spawn = |v: Vec<String>| v.into_iter().map(|n| (n.clone(), Act::Spawn(n))).collect::<Vec<_>>();
    let mut all: Vec<(String, Act)> = match debug.tab {
        MONSTERS => spawn(monsters.get(&debug.monsters).map(|m| m.0.keys().filter(|k| keep(k)).cloned().collect()).unwrap_or_default()),
        MODELS => spawn(index.get(&debug.index).map(|i| i.models.iter().filter(|m| !m.starts_with("effects") && keep(m)).cloned().collect()).unwrap_or_default()),
        EFFECTS => spawn(index.get(&debug.index).map(|i| i.models.iter().filter(|m| m.starts_with("effects") && keep(m)).cloned().collect()).unwrap_or_default()),
        SEQUENCES => spawn(seqs.get(skills.sequences()).map(|s| s.0.keys().filter(|k| keep(k)).cloned().collect()).unwrap_or_default()),
        SPAWNED => debug
            .spawned
            .iter()
            .enumerate()
            .filter(|(_, (_, n))| keep(n))
            .map(|(i, (_, n))| (if debug.selected == Some(i) { format!("> {n}") } else { n.clone() }, Act::Select(i)))
            .collect(),
        ANIMS => debug
            .monster()
            .and_then(|a| live.iter().find(|(m, _)| m.actor == a))
            .map(|(_, anim)| anim.states().into_iter().filter(|s| keep(s)).map(|s| (s.clone(), Act::Anim(s))).collect())
            .unwrap_or_default(),
        HEROES => (0..4u8)
            .flat_map(|c| (0..2u8).map(move |g| (c, g)))
            .map(|(c, g)| (format!("{} ({})", CLASSES[c as usize], if g == 1 { "female" } else { "male" }), Act::Hero(c, g)))
            .filter(|(n, _)| keep(n))
            .collect(),
        _ => {
            let class = players.iter().next().map(|c| c.desc.class as usize).unwrap_or(1).min(3);
            let prefix = format!("{}_", CLASSES[class]);
            stats
                .get(&debug.stats)
                .map(|s| {
                    s.skills
                        .iter()
                        .filter(|(id, _)| id.starts_with(&prefix) && keep(id))
                        .map(|(id, (_, _, title))| (format!("{title} ({id})"), Act::Skill(id.clone())))
                        .collect()
                })
                .unwrap_or_default()
        }
    };
    // Spawned keeps its order (newest last); the rest sorted.
    if debug.tab != SPAWNED {
        all.sort_by(|a, b| a.0.cmp(&b.0));
    }
    let pages = all.len().div_ceil(ROWS).max(1);
    let page = debug.page.min(pages - 1);
    debug.total = all.len();
    debug.rows = all.into_iter().skip(page * ROWS).take(ROWS).collect();
    debug.dirty = false;
    for (Row(i), kids) in &rows {
        if let Some(mut t) = kids.first().and_then(|k| texts.get_mut(*k).ok()) {
            let s = debug.rows.get(*i).map(|(l, _)| l.clone()).unwrap_or_default();
            if t.0 != s {
                t.0 = s;
            }
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
