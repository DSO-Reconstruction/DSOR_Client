//! Status effects in the HUD, in three rows of slots:
//!
//! - the local player's, above the action bar (bottombar BuffBar): the positive
//!   ones on its left half (Buffs, filled from Slot1 at the left), the negative
//!   ones on its right half (Debuffs, Slot1 at the right edge, filled towards
//!   the middle);
//! - the hovered monster's negative ones, under its health bar (interactinfo):
//!   a row the client adds under the panel (`prepare_target`), centred on the
//!   bar; Settings > Combat > "Status effects on enemies" turns it off.
//!
//! A slot shows the effect's icon on its configuration's background and frame,
//! its stack count, its time left (the slot's DurN label) and, hovered, a card
//! with its name and modifiers. The rows stay click-through, as the bottom bar
//! is authored (ClickThrough on every part).
//!
//! tools/status_catalog.py exports which effects show and how
//! (interface/status.catalog.json): _Template_StatusEffect Groups BuffUI or
//! DebuffUI, IconBrush, IconConfiguration. The running effects are
//! Net::stat_effects (StatusEffectCommand on our actor) and Net::actor_effects
//! (on the others).
//!
//! Change-driven (docs/ui.md, "Per-frame cost"): the slots are reassigned when
//! the effects shown change (Net::stat_revision, the hovered monster and its
//! effects' stamp, the setting), the time labels rewritten when one of them
//! next reads differently, the icons redrawn when a slot's content does.
use super::*;
use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use crate::actors::monsters::{HoverInfo, Hovered};
use crate::network::net::Net;
use dsor_proto::commands::combat::StatusEffectElement;
use tooltips::card::{Card, WHITE, MUTED, BLUE};

/// Slots per row (Slot1..Slot6).
const SLOTS: usize = 6;
/// The rows: the player's positive and negative effects, the hovered monster's
/// negative ones.
const ROWS: usize = 3;
pub(in crate::ui) const TARGET: usize = 2;
/// Server ticks per second.
const TICKS: u32 = 25;
/// The slot's Icon: Arial 9, white, outlined (TextEffect 1, size 2), bottom right.
const COUNT_SIZE: f32 = 9.0;
const COUNT_OUTLINE: f32 = 2.0;
/// The monster row's parent in interactinfo, its slots' size and spacing
/// (reference pixels; the bottom bar's are 25.6 and 2.5).
const TARGET_ROOT: &str = "Canvas/Label_bg/TargetEffects";
const TARGET_SIZE: f32 = 22.0;
const TARGET_GAP: f32 = 3.0;

#[derive(Asset, TypePath, Deserialize, Default)]
pub(in crate::ui) struct Catalog {
    /// gui.window timeSecondsShort, timeMinutesShort, timeHourShort, timeDaysShort.
    units: [String; 4],
    /// IconConfiguration -> [background, frame] (either may be empty).
    configs: HashMap<String, [String; 2]>,
    /// By wire index (_Template_StatusEffect row - 1): the effects the rows show.
    effects: HashMap<u16, Effect>,
}

#[derive(Deserialize)]
struct Effect {
    id: String,
    debuff: bool,
    icon: String,
    #[serde(default)]
    config: String,
    name: String,
    /// MaxStackSize.
    stacks: u32,
    #[serde(default)]
    lines: Vec<Line>,
}

/// A hover line: its text (the one for a negative value in `minus`), and its
/// arguments by placeholder ({0:s} is args[0]).
#[derive(Deserialize)]
struct Line {
    text: String,
    #[serde(default)]
    minus: String,
    /// The text by class ("warrior", "mage"...), for a class's own resource.
    #[serde(default)]
    classes: HashMap<String, String>,
    #[serde(default)]
    args: Vec<Arg>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Arg {
    Text(String),
    /// The element's parameter `p`, else the known value `v`, else unknown;
    /// times `scale` (100 for a relative modifier, shown as a percentage).
    Number {
        #[serde(default)]
        p: Option<usize>,
        #[serde(default)]
        v: Option<f32>,
        /// A mutable actor variable the client may know (card_lines).
        #[serde(default)]
        var: Option<String>,
        #[serde(default = "one")]
        scale: f32,
    },
}

fn one() -> f32 { 1.0 }

#[derive(Default, TypePath)]
pub(in crate::ui) struct CatalogLoader;
impl AssetLoader for CatalogLoader {
    type Asset = Catalog;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<Catalog, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] { &["status.catalog.json"] }
}

/// What a slot shows: the effect (its wire index), its stacks, and the server
/// tick it ends at (0: it does not end).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Shown {
    pub index: u16,
    pub stacks: u32,
    pub end: u32,
}

/// What the slots are assigned from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Inputs {
    /// Net::stat_revision.
    player: u64,
    catalog: bool,
    /// The hovered monster whose row shows and its effects' stamp (None: no row).
    target: Option<(u32, u64)>,
}

#[derive(Resource, Default)]
pub(in crate::ui) struct StatusBar {
    pub catalog: Handle<Catalog>,
    /// Each row's slots, Slot1 first, and their time labels.
    pub slots: [[Option<Shown>; SLOTS]; ROWS],
    pub times: [[String; SLOTS]; ROWS],
    /// Each row's effects in arrival order: an effect keeps its place while it
    /// runs, refreshed or not (the server sends each refresh as a new instance).
    order: [Vec<u16>; ROWS],
    inputs: Option<Inputs>,
    /// The server tick at which a time label next reads differently.
    next: Option<u32>,
}

/// A slot: its row (0 Buffs, 1 Debuffs, 2 the monster's), its number (Slot1 =
/// 0), where it is.
#[derive(Component)]
pub(in crate::ui) struct EffectSlot {
    pub row: usize,
    pub slot: usize,
    pub k: f32,
    pub rect: [f32; 4],
}

/// What a slot's children draw now, and how far its row was shifted.
#[derive(Component, Default)]
pub(in crate::ui) struct Drawn(Option<(Shown, f32)>);

fn numbered(id: &str, prefix: &str) -> Option<(usize, usize)> {
    let (parent, leaf) = id.rsplit_once('/')?;
    let row = if parent.ends_with("/BuffBar/Buffs") { 0 } else if parent.ends_with("/BuffBar/Debuffs") { 1 }
        else if parent == TARGET_ROOT { TARGET } else { return None };
    let n: usize = leaf.strip_prefix(prefix)?.parse().ok()?;
    (1..=SLOTS).contains(&n).then_some((row, n - 1))
}

/// The slot a widget is (BuffBar/Buffs/Slot3: (0, 2)).
pub(in crate::ui) fn slot_of(id: &str) -> Option<(usize, usize)> { numbered(id, "Slot") }

/// The slot whose time a label shows (BuffBar/Debuffs/Dur1: (1, 0)).
pub(in crate::ui) fn time_label(id: &str) -> Option<(u8, u8)> {
    numbered(id, "Dur").map(|(row, slot)| (row as u8, slot as u8))
}

/// Part of the monster's row (its slots are laid out by `prepare_target`).
pub(in crate::ui) fn target_part(id: &str) -> bool {
    id.strip_prefix(TARGET_ROOT).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The hovered monster's row under its panel: SLOTS slots and their time labels
/// (styled as the bottom bar's DurN), centred on the health bar, below the
/// panel's bottom plate (gothic_panel/LabelBottom).
pub(in crate::ui) fn prepare_target(layout: &Layout) -> Layout {
    let mut out = layout.clone();
    let Some(bar) = layout.widgets.iter().find(|w| w.id.ends_with("/Healthbar") && !w.hidden) else { return out };
    let plate = layout.widgets.iter().find(|w| w.id.ends_with("/gothic_panel/LabelBottom"))
        .and_then(|w| w.art.as_ref()).map_or(bar.rect[3] + 39.0, |a| bounds(&a.pos)[3]);
    let top = plate + 3.0;
    let width = SLOTS as f32 * TARGET_SIZE + (SLOTS - 1) as f32 * TARGET_GAP;
    let left = (bar.rect[0] + bar.rect[2] - width) * 0.5;
    let widget = |id: String, kind: &str, rect: [f32; 4], text: Option<Label>| Widget {
        id, kind: kind.into(), event: String::new(), click_through: true, enabled: true, button: None, state: None,
        spell: None, k: bar.k, rect, hidden: false, art: None, text,
    };
    for n in 0..SLOTS {
        let l = left + n as f32 * (TARGET_SIZE + TARGET_GAP);
        out.widgets.push(widget(format!("{TARGET_ROOT}/Slot{}", n + 1), "Slot", [l, top, l + TARGET_SIZE, top + TARGET_SIZE], None));
        let label = Label { text: "Text".into(), key: "Text".into(), size: 8.5, color: [0.9, 0.78, 0.54, 1.0], align: [1, 1],
            bold: true, word_break: false, outline: vec![2.0, 0.0, 0.0, 0.0, 1.0] };
        let y = top + TARGET_SIZE + 1.0;
        out.widgets.push(widget(format!("{TARGET_ROOT}/Dur{}", n + 1), "TextLabel", [l - 2.0, y, l + TARGET_SIZE + 2.0, y + 8.0], Some(label)));
    }
    out
}

/// How far a row is shifted: the monster's row centres what it shows; the
/// bottom bar's halves are anchored to their ends.
fn shift(row: usize, slots: &[Option<Shown>; SLOTS]) -> f32 {
    if row != TARGET { return 0.0; }
    (SLOTS - slots.iter().flatten().count()) as f32 * (TARGET_SIZE + TARGET_GAP) * 0.5
}

fn reached(now: u32, tick: u32) -> bool { now.wrapping_sub(tick) as i32 >= 0 }

/// Assign the slots when what they show changes; rewrite the time labels when
/// one of them is due. A frame with neither does nothing else.
#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn sync(net: Option<NonSend<Net>>, time: Res<Time<Real>>, catalogs: Res<Assets<Catalog>>,
    settings: Res<settings::Settings>, (hovered, hover_info): (Res<Hovered>, Res<HoverInfo>),
    mut bar: ResMut<StatusBar>, mut work: ResMut<UiWork>) {
    let catalog = catalogs.get(&bar.catalog);
    // The monster under the cursor while its panel shows (alive: HoverInfo).
    let target = hovered.0.filter(|_| settings.current.monster_effects && hover_info.0.is_some()).map(|(_, actor)| actor);
    let inputs = net.as_ref().map(|n| Inputs { player: n.stat_revision, catalog: catalog.is_some(),
        target: target.map(|actor| (actor, n.actor_effects.get(&actor).map_or(0, |e| e.stamp))) });
    let now = net.as_ref().map(|n| n.server_tick(crate::network::net::now_ms(&time)));
    // Bookkeeping leaves the change tick alone: only new contents mark the bar
    // changed (its icons and labels redraw on that).
    let state = bar.bypass_change_detection();
    let assign = state.inputs != inputs;
    let due = matches!((state.next, now), (Some(next), Some(now)) if reached(now, next));
    if !assign && !due { return; }
    let mut changed = false;
    if assign {
        state.inputs = inputs;
        work.status_bars += 1;
        let slots = match (&net, catalog) {
            (Some(net), Some(catalog)) => {
                let [buffs, debuffs, monster] = &mut state.order;
                let monster_row = match target.and_then(|actor| net.actor_effects.get(&actor)) {
                    Some(effects) => assign_row(monster, &effects.elements, catalog, true),
                    None => { monster.clear(); default() }
                };
                [assign_row(buffs, &net.stat_effects, catalog, false), assign_row(debuffs, &net.stat_effects, catalog, true), monster_row]
            }
            _ => { state.order = default(); default() }
        };
        if slots != state.slots { state.slots = slots; changed = true; }
    }
    let (times, next) = match (now, catalog) {
        (Some(now), Some(catalog)) => labels(&state.slots, now, &catalog.units),
        _ => (default(), None),
    };
    state.next = next;
    if times != state.times { state.times = times; changed = true; }
    if changed { bar.set_changed(); }
}

/// A row's slots from an actor's running effects, its negative or positive ones:
/// one icon an effect, its stacks summed over its instances and capped at
/// MaxStackSize (as the character statistics count them: crate::gameplay::stats),
/// the latest end. Past six, the permanent ones (costumes, pets, mounts) give
/// way first, then the oldest.
fn assign_row(order: &mut Vec<u16>, effects: &HashMap<u32, StatusEffectElement>, catalog: &Catalog, debuff: bool) -> [Option<Shown>; SLOTS] {
    let mut elements: Vec<&StatusEffectElement> = effects.values().filter(|e| e.tail.is_some()).collect();
    elements.sort_by_key(|e| e.instance);
    // Effect -> (what it shows, its first instance).
    let mut running: HashMap<u16, (Shown, u32)> = HashMap::new();
    for e in elements {
        let Some(def) = catalog.effects.get(&e.index).filter(|d| d.debuff == debuff) else { continue };
        let stacks = e.spare.saturating_add(1);
        let entry = running.entry(e.index).or_insert((Shown { index: e.index, stacks: 0, end: e.end }, e.instance));
        entry.0.stacks = entry.0.stacks.saturating_add(stacks).min(def.stacks.max(1));
        entry.0.end = match (entry.0.end, e.end) {
            (0, _) | (_, 0) => 0,
            (a, b) => if reached(b, a) { b } else { a },
        };
    }
    order.retain(|i| running.contains_key(i));
    let mut fresh: Vec<(u32, u16)> = running.iter().filter(|(i, _)| !order.contains(i)).map(|(i, r)| (r.1, *i)).collect();
    fresh.sort_unstable();
    order.extend(fresh.into_iter().map(|(_, i)| i));
    let mut keep = order.clone();
    while keep.len() > SLOTS {
        let gone = keep.iter().position(|i| running[i].0.end == 0).unwrap_or(0);
        keep.remove(gone);
    }
    let mut out: [Option<Shown>; SLOTS] = default();
    for (slot, i) in keep.iter().enumerate() { out[slot] = Some(running[i].0); }
    out
}

/// The slots' time labels at `now`, and the tick at which the first of them
/// changes (None: none will before the effects do).
fn labels(slots: &[[Option<Shown>; SLOTS]; ROWS], now: u32, units: &[String; 4]) -> ([[String; SLOTS]; ROWS], Option<u32>) {
    let mut out: [[String; SLOTS]; ROWS] = default();
    let mut next: Option<u32> = None;
    for (row, slots) in slots.iter().enumerate() {
        for (slot, shown) in slots.iter().enumerate() {
            let Some(shown) = shown.filter(|s| s.end != 0) else { continue };
            let left = shown.end.wrapping_sub(now) as i32;
            // Over: the server's removal follows.
            if left <= 0 { continue; }
            let (text, lasts) = remaining(left as u32, units);
            out[row][slot] = text;
            let change = now.wrapping_add(lasts);
            next = Some(next.filter(|n| !reached(*n, change)).unwrap_or(change));
        }
    }
    (out, next)
}

/// Time left in the game's short units: whole seconds up to a minute (rounded
/// up, so the last second reads "1 s"), then whole minutes, hours, days; and
/// for how many more ticks it reads so.
fn remaining(ticks: u32, units: &[String; 4]) -> (String, u32) {
    const MINUTE: u32 = 60 * TICKS;
    const HOUR: u32 = 60 * MINUTE;
    const DAY: u32 = 24 * HOUR;
    if ticks <= MINUTE {
        let seconds = ticks.div_ceil(TICKS);
        return (fill(&units[0], &[Some(seconds.to_string())]), ticks - (seconds - 1) * TICKS);
    }
    let (unit, size) = if ticks <= HOUR { (1, MINUTE) } else if ticks <= DAY { (2, HOUR) } else { (3, DAY) };
    let n = (ticks - 1) / size;
    (fill(&units[unit], &[Some(n.to_string())]), ticks - n * size)
}

/// The text with its placeholders ({0:s}, {1:i:value}) replaced by `args`; an
/// unknown one reads as an em dash (as the skill cards show unresolved values).
fn fill(text: &str, args: &[Option<String>]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        out.push_str(&rest[..open]);
        let index = rest[open + 1..open + close].split(':').next().and_then(|n| n.parse::<usize>().ok());
        out.push_str(index.and_then(|i| args.get(i)).and_then(|a| a.as_deref()).unwrap_or("—"));
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

/// A status effect's lines on an item's card (a pet's Effect section,
/// items::preview::sticker): (text, a header). `params` are the item's values
/// ($0..), `vars` the actor variables known (the pets' Collector's Bag bonuses,
/// systems::album). A line whose value is zero is left out (the original's
/// card had no Andermant line, a bonus its owner had not earned).
/// EVIDENCE: the original's Cute Doggy card (the user's recording
///   20261030-1714-37): "Pet Effect:" and "Collector's Bag Effect:" white, the
///   lines blue, figures with two decimals ("20.00%", "+ 33.00% Health
///   Points"), the resistances' three ("+ 15.000% all resistance values").
pub(in crate::ui) fn card_lines(catalog: &Catalog, status: &str, params: &[f32], vars: &HashMap<String, f32>, class: &str) -> Vec<(String, bool)> {
    // Markup off ("<GemBlueStart>Rage</>"): the card colours the line itself.
    let plain = |text: &str| {
        let mut out = String::new();
        let mut rest = text;
        while let Some(open) = rest.find('<') {
            out.push_str(&rest[..open]);
            match rest[open..].find('>') { Some(close) => rest = &rest[open + close + 1..], None => { rest = ""; } }
        }
        out.push_str(rest);
        out
    };
    let Some(effect) = catalog.effects.values().find(|e| e.id == status) else { return Vec::new() };
    let mut out = Vec::new();
    for line in &effect.lines {
        // A header is written white ("<WhiteStart>Pet Effect:</>"); a text of
        // its own ("Enables auto-pickup for Mystic Cubes") is an effect line.
        if line.args.is_empty() {
            let header = line.text.contains("<WhiteStart>");
            out.push((plain(&line.text), header));
            continue;
        }
        let decimals = if line.args.iter().any(|a| matches!(a, Arg::Text(t) if t.contains("resistance"))) { 3 } else { 2 };
        let mut first = None;
        let args: Vec<Option<String>> = line.args.iter().map(|arg| match arg {
            Arg::Text(text) => Some(text.clone()),
            Arg::Number { p, v, var, scale } => {
                let value = p.map(|i| params.get(i).copied().unwrap_or(0.0)).or(*v)
                    .or_else(|| var.as_ref().map(|name| vars.get(name).copied().unwrap_or(0.0)))
                    .map(|x| x * scale);
                first.get_or_insert(value);
                value.map(|x| format!("{:.*}", decimals, x.abs()))
            }
        }).collect();
        let negative = match first {
            Some(Some(x)) if x.abs() < 1e-6 => continue,
            Some(Some(x)) => x < 0.0,
            _ => false,
        };
        // A value only the server knows (the Christmas Elf's resistances, by
        // the wearer's level) is left out rather than written as a dash.
        if line.args.iter().zip(&args).any(|(a, v)| matches!(a, Arg::Number { .. }) && v.is_none()) { continue; }
        let text = if negative && !line.minus.is_empty() { &line.minus } else { line.classes.get(class).unwrap_or(&line.text) };
        out.push((plain(&fill(text, &args)), false));
    }
    out
}

/// A figure as the modifier texts print it: at most two decimals.
fn number(value: f32) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A hover line for this many stacks: its values times the stacks, in the text
/// of their sign (a negative first value picks `minus` and shows its size). A
/// line whose value is zero says nothing and is left out; an unknown value
/// takes the sign of its half.
fn line(line: &Line, params: &[f32], stacks: u32, debuff: bool) -> Option<String> {
    let mut first: Option<Option<f32>> = None;
    let args: Vec<Option<String>> = line.args.iter().map(|arg| match arg {
        Arg::Text(text) => Some(text.clone()),
        Arg::Number { p, v, scale, .. } => {
            let value = p.map_or(*v, |i| params.get(i).copied()).map(|x| x * scale * stacks as f32);
            first.get_or_insert(value);
            value.map(|x| number(x.abs()))
        }
    }).collect();
    let negative = match first {
        Some(Some(x)) if x.abs() < 1e-6 => return None,
        Some(Some(x)) => x < 0.0,
        Some(None) => debuff,
        None => false,
    };
    let text = if negative && !line.minus.is_empty() { &line.minus } else { &line.text };
    Some(fill(text, &args))
}

/// Redraw the slots whose content changed and rewrite the time labels.
#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn draw(
    mut commands: Commands,
    bar: Res<StatusBar>,
    catalogs: Res<Assets<Catalog>>,
    screen: Res<Screen>,
    assets: Res<AssetServer>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<UiMaterial>>),
    (mut slots, new_slots): (Query<(Entity, &EffectSlot, &mut Drawn)>, Query<(), Added<EffectSlot>>),
    (mut labels, new_labels): (Query<(&Value, &mut Text2d, &mut Transform)>, Query<(), Added<Value>>),
) {
    // A rebuilt window brings new labels and slots: they take the current state.
    if bar.is_changed() || !new_labels.is_empty() {
        // The slots' centres: a centred row's labels move with its icons.
        let mut centres = [[None; SLOTS]; ROWS];
        for (_, slot, _) in &slots { centres[slot.row][slot.slot] = Some((slot.rect[0] + slot.rect[2]) * 0.5); }
        for (value, mut text, mut transform) in &mut labels {
            let ValueKind::EffectTime(row, slot) = value.0 else { continue };
            let (row, slot) = (row as usize, slot as usize);
            let want = &bar.times[row][slot];
            if text.0 != *want { text.0.clone_from(want); }
            if let Some(centre) = centres[row][slot] {
                let x = centre + shift(row, &bar.slots[row]);
                if transform.translation.x != x { transform.translation.x = x; }
            }
        }
    }
    if !bar.is_changed() && !screen.is_changed() && new_slots.is_empty() { return; }
    let catalog = catalogs.get(&bar.catalog);
    for (entity, slot, mut drawn) in &mut slots {
        let want = bar.slots[slot.row][slot.slot].map(|shown| (shown, shift(slot.row, &bar.slots[slot.row])));
        if want == drawn.0 && !screen.is_changed() { continue; }
        commands.entity(entity).despawn_related::<Children>();
        drawn.0 = want;
        let (Some((shown, dx)), Some(catalog)) = (want, catalog) else { continue };
        let Some(def) = catalog.effects.get(&shown.index) else { continue };
        let [l, t, r, b] = slot.rect;
        let rect = [l + dx, t, r + dx, b];
        let [background, frame] = catalog.configs.get(&def.config).cloned().unwrap_or_default();
        // The inventory's order: background, image, frame (template::icon_layers).
        for (z, texture) in [(0.0, background), (0.1, def.icon.clone()), (0.2, frame)] {
            if texture.is_empty() { continue; }
            let mut run = Run { k: slot.k, ..default() };
            run.push(&template::image_art(rect, texture.clone()), 0.0);
            commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(materials.add(material(Some(assets.load(texture))))),
                Transform::from_xyz(0.0, 0.0, z), RenderLayers::layer(LAYER), ChildOf(entity)));
        }
        if shown.stacks > 1 { count(&mut commands, &assets, &screen, entity, rect, shown.stacks); }
    }
}

/// The stack count at the icon's bottom right, outlined as the slot's Icon text
/// is (crate::ui::outline_copies). The slot's parent is y-flipped for the
/// artwork; the text flips back.
fn count(commands: &mut Commands, assets: &AssetServer, screen: &Screen, slot: Entity, rect: [f32; 4], stacks: u32) {
    let [_, _, r, b] = rect;
    let font = TextFont { font: assets.load("fonts/tahoma.ttf").into(),
        font_size: bevy::text::FontSize::Px(COUNT_SIZE * FONT_SCALE * screen.scale), ..default() };
    let text = commands.spawn((Text2d::new(stacks.to_string()), font.clone(), TextColor(Color::WHITE), Anchor::BOTTOM_RIGHT,
        Transform::from_xyz(r - 1.0, b - 1.0, 0.5).with_scale(Vec3::new(1.0, -1.0, 1.0) / screen.scale),
        RenderLayers::layer(LAYER), ChildOf(slot))).id();
    let radius = (0.5 + 0.35 * COUNT_OUTLINE).min(1.5);
    for (offset, shadow) in outline_copies(radius) {
        let copy = OutlineCopy { offset, shadow, dz: -0.01, color: Color::BLACK };
        commands.spawn((Text2d::new(stacks.to_string()), font.clone(), TextColor(Color::BLACK), Anchor::BOTTOM_RIGHT,
            copy.transform(screen.scale), bevy::sprite::Text2dShadow { offset: shadow * screen.scale, color: Color::BLACK },
            RenderLayers::layer(LAYER), ChildOf(text), Visibility::Inherited, copy));
    }
}

/// The hover card of the effect under `point` (reference pixels; `offset_of`
/// gives a row's window offset, None while it is hidden), anchored to its slot.
#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn card(point: Vec2, offset_of: impl Fn(usize) -> Option<Vec2>, width: f32, bar: &StatusBar,
    catalog: Option<&Catalog>, slots: &Query<&EffectSlot>, net: Option<&Net>, template: Option<&Layout>,
    bag: &HashMap<String, f32>, class: &str) -> Option<(Card, [f32; 4])> {
    let catalog = catalog?;
    let (slot, shown, rect) = slots.iter().find_map(|s| {
        let shown = bar.slots[s.row][s.slot]?;
        let offset = offset_of(s.row)?;
        let dx = offset.x + shift(s.row, &bar.slots[s.row]);
        let rect = [s.rect[0] + dx, s.rect[1] + offset.y, s.rect[2] + dx, s.rect[3] + offset.y];
        contains(s.k, rect, point, width).then_some((s, shown, rect))
    })?;
    let def = catalog.effects.get(&shown.index)?;
    // The values its modifiers take ($0, $1...): its latest instance's parameters.
    let effects = if slot.row == TARGET {
        bar.inputs.and_then(|i| i.target).and_then(|(actor, _)| net?.actor_effects.get(&actor)).map(|e| &e.elements)
    } else { net.map(|n| &n.stat_effects) };
    let params = effects.and_then(|e| e.values().filter(|e| e.index == shown.index).max_by_key(|e| e.instance))
        .and_then(|e| e.tail.as_ref()).map_or(&[][..], |t| t.parameters.as_slice());
    // The original's buff card: the small hand of the sticker cards, its name
    // in italics, "Effect", the headers white and the lines blue; a pet's
    // Collector's Bag lines with the bonuses earned, the class's own resource.
    // EVIDENCE: the original's Loving Kraken and Mortis Doll buff cards (the
    //   user's recording 20261031-1536-17 and screenshot): lines 22 px apart at
    //   3838 px wide, ours 45; no icon; "+ 30.00% Mana" all blue.
    // FAILURE (2026-10-31): twice the original's size, "+ —% Health Points"
    //   lines and "—: value unavailable" ("ours looks so big").
    let mut card = super::items::small_card(template);
    let headed = def.lines.iter().any(|l| l.text.contains("<WhiteStart>"));
    let lines: Vec<(String, bool)> = if headed { card_lines(catalog, &def.id, params, bag, class) }
        else { def.lines.iter().filter_map(|l| line(l, params, shown.stacks, def.debuff)).map(|t| (t, false)).collect() };
    let time = &bar.times[slot.row][slot.slot];
    let mut hints = Vec::new();
    if shown.stacks > 1 { hints.push(format!("Stacks: {} / {}", shown.stacks, def.stacks)); }
    if !time.is_empty() { hints.push(format!("Remaining: {time}")); }
    // As the skill cards say it (tooltips::overlay).
    if lines.iter().any(|(l, _)| l.contains('—')) { hints.push("—: value unavailable".into()); }
    let title = format!("<i>{}</>", def.name);
    super::items::fit_small(&mut card, std::iter::once(title.as_str()).chain(lines.iter().map(|(t, _)| t.as_str())).chain(hints.iter().map(String::as_str)));
    card.text(&title, WHITE);
    if !lines.is_empty() {
        card.rule();
        card.centered("Effect", MUTED, false);
        for (text, header) in &lines { card.text(text, if *header { WHITE } else { BLUE }); }
    }
    if !hints.is_empty() { card.gap(4.0); }
    for hint in &hints { card.hint(hint); }
    let x = slot.k * (width - BASE_W);
    Some((card, [rect[0] + x, rect[1], rect[2] + x, rect[3]]))
}

/// Parse a fixture: `<id>[:seconds[:stacks[:p0/p1...]]],...` (seconds 0:
/// permanent; the parameters are the modifiers' $0, $1...).
fn fixture_effects(spec: &str, catalog: &Catalog, now: u32, first: u32) -> Vec<StatusEffectElement> {
    let mut out = Vec::new();
    for (n, entry) in spec.split(',').enumerate() {
        let mut parts = entry.split(':');
        let id = parts.next().unwrap_or("").trim();
        // `#<wire index>`: an effect the HUD does not show (a Shadow Soul's
        // minion_mercenary_*, which crate::ui::preview reads).
        let index = match id.strip_prefix('#') {
            Some(index) => index.parse().ok(),
            None => catalog.effects.iter().find(|(_, e)| e.id == id).map(|(&index, _)| index),
        };
        let Some(index) = index else {
            warn!("status effect fixture: {id} is not an effect the HUD shows");
            continue;
        };
        let ticks = (parts.next().and_then(|s| s.parse::<f32>().ok()).unwrap_or(60.0).max(0.0) * TICKS as f32) as u32;
        let stacks = parts.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(1).max(1);
        let parameters = parts.next().map(|s| s.split('/').filter_map(|v| v.parse().ok()).collect()).unwrap_or_default();
        let instance = first + n as u32;
        out.push(StatusEffectElement {
            index, instance, start: now, end: if ticks > 0 { now.wrapping_add(ticks) } else { 0 }, duration: ticks,
            spare: stacks - 1, flag_26: stacks > 1,
            tail: Some(dsor_proto::commands::combat::StatusEffectTail { parameters, ..default() }), ..default()
        });
    }
    out
}

#[derive(Default)]
pub(in crate::ui) struct Fixture {
    read: bool,
    player: Option<String>,
    target: Option<String>,
    /// The monsters that got the target fixture.
    targets: Vec<u32>,
}

/// Offline fixtures, to look at the rows without a server (both also count in
/// the offline game: the player's in its statistics):
/// DSOR_TEST_EFFECTS gives the player these effects; DSOR_TEST_TARGET_EFFECTS
/// gives them to each monster when it is first hovered (DSOR_TEST_TARGET=1
/// hovers the nearest one). Same format: `<id>[:seconds[:stacks[:p0/p1...]]],...`,
/// `#<wire index>` in place of the id for an effect the bar does not show.
pub(in crate::ui) fn fixture(net: Option<NonSendMut<Net>>, bar: Res<StatusBar>, catalogs: Res<Assets<Catalog>>,
    hovered: Res<Hovered>, time: Res<Time<Real>>, mut state: Local<Fixture>) {
    if !state.read {
        state.read = true;
        state.player = std::env::var("DSOR_TEST_EFFECTS").ok();
        state.target = std::env::var("DSOR_TEST_TARGET_EFFECTS").ok();
    }
    if state.player.is_none() && state.target.is_none() { return; }
    let Some(mut net) = net else { return };
    if !net.offline { state.player = None; state.target = None; return; }
    let Some(catalog) = catalogs.get(&bar.catalog) else { return };
    let now = net.server_tick(crate::network::net::now_ms(&time));
    if let Some(spec) = state.player.take() {
        for e in fixture_effects(&spec, catalog, now, 0x7000_0000) { net.stat_effects.insert(e.instance, e); }
        net.stat_revision = net.stat_revision.wrapping_add(1);
    }
    let Some((_, actor)) = hovered.0.filter(|(_, actor)| !state.targets.contains(actor)) else { return };
    if let Some(effects) = state.target.as_deref().map(|spec| fixture_effects(spec, catalog, now, 0x7100_0000)) {
        state.targets.push(actor);
        net.apply_actor_effects(actor, true, &effects);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pet_card_shows_its_own_bonus_and_the_collectors_bag_bonuses_earned() {
        let catalog: Catalog = serde_json::from_value(serde_json::json!({
            "units": ["{0:s} s", "{0:i} m", "{0:i} hr", "{0:i} d"], "configs": {},
            "effects": {"494": {"id": "pet_dog_01", "debuff": false, "icon": "i.png", "name": "Cute Doggy (Pet)", "stacks": 1, "lines": [
                {"text": "<WhiteStart>Pet Effect:</>", "args": []},
                {"text": "Selling price of items increased by {0:s}%", "minus": "", "args": [{"p": 0, "scale": 100.0}]},
                {"text": "<WhiteStart>Collector's Bag Effect:</>", "args": []},
                {"text": "+ {0:s}% Health Points", "args": [{"var": "pet_hitpoint_01", "scale": 100.0}]},
                {"text": "+ {1:s}% {0:s}", "args": ["all resistance values", {"var": "pet_resistance_01", "scale": 100.0}]},
                {"text": "+ {1:s}% Andermant drop stack size while accompanied by a pet", "args": ["Andermant", {"var": "pet_andermant_01", "scale": 100.0}]}]}}
        })).unwrap();
        let bag = HashMap::from([("pet_hitpoint_01".to_owned(), 0.33), ("pet_resistance_01".to_owned(), 0.15)]);
        let lines = card_lines(&catalog, "pet_dog_01", &[0.2], &bag, "warrior");
        assert_eq!(lines, vec![
            ("Pet Effect:".to_owned(), true),
            ("Selling price of items increased by 20.00%".to_owned(), false),
            ("Collector's Bag Effect:".to_owned(), true),
            ("+ 33.00% Health Points".to_owned(), false),
            ("+ 15.000% all resistance values".to_owned(), false),
        ], "no Andermant earned: no Andermant line");
        assert!(card_lines(&catalog, "pet_cat_01", &[], &bag, "warrior").is_empty());
    }

    fn catalog() -> Catalog {
        serde_json::from_value(serde_json::json!({
            "units": ["{0:s} s", "{0:i} m", "{0:i} hr", "{0:i} d"],
            "configs": {"statuseffect_buff": ["textures/icons_toolkit/bg_status_buff.png", "textures/icons_toolkit/frame_status.png"]},
            "effects": {
                "10": {"id": "DamageBuff", "debuff": false, "icon": "textures/icons/a.png", "config": "statuseffect_buff", "name": "Blessing of might", "stacks": 1,
                       "lines": [{"text": "+ {0:s}% damage", "minus": "- {0:s}% damage", "args": [{"p": 0, "scale": 100.0}]}]},
                "11": {"id": "debuff_cc_frost", "debuff": true, "icon": "textures/icons/b.png", "name": "Frost", "stacks": 50,
                       "lines": [{"text": "+ {0:s}% movement speed", "minus": "- {0:s}% movement speed", "args": [{"v": -0.02, "scale": 100.0}]}]},
                "12": {"id": "pet_wisp", "debuff": false, "icon": "textures/icons/c.png", "name": "Firefly", "stacks": 1,
                       "lines": [{"text": "<WhiteStart>Pet Effect:</>"}, {"text": "+ {1:s}% {0:s}", "minus": "- {1:s}% {0:s}", "args": ["all resistance values", {"scale": 100.0}]}]}
            }
        })).unwrap()
    }

    fn element(index: u16, instance: u32, end: u32, spare: u32) -> StatusEffectElement {
        StatusEffectElement { index, instance, end, spare, tail: Some(default()), ..default() }
    }

    #[test]
    fn widgets_name_their_row_and_slot() {
        assert_eq!(slot_of("Canvas/Label_new/BuffBar/Buffs/Slot1"), Some((0, 0)));
        assert_eq!(slot_of("Canvas/Label_new/BuffBar/Debuffs/Slot6"), Some((1, 5)));
        assert_eq!(slot_of("Canvas/Label_new/BuffBar/Debuffs/Slot1/Icon"), None);
        assert_eq!(slot_of("Canvas/Label_new/Frame1/Iconslots/QuickSlot1"), None);
        assert_eq!(slot_of("Canvas/Label_bg/TargetEffects/Slot2"), Some((TARGET, 1)));
        assert_eq!(time_label("Canvas/Label_new/BuffBar/Debuffs/Dur2"), Some((1, 1)));
        assert_eq!(time_label("Canvas/Label_bg/TargetEffects/Dur6"), Some((TARGET as u8, 5)));
        assert_eq!(time_label("Canvas/Label_new/BuffBar/Buffs/Dur7"), None);
        assert!(target_part("Canvas/Label_bg/TargetEffects/Dur1") && !target_part("Canvas/Label_bg/TargetEffectsX"));
    }

    #[test]
    fn effects_take_their_row_keep_their_place_and_sum_their_stacks() {
        let catalog = catalog();
        let (mut buffs, mut debuffs) = (Vec::new(), Vec::new());
        let mut effects = HashMap::new();
        // Not shown (no BuffUI / DebuffUI): never assigned.
        effects.insert(1, element(99, 1, 0, 0));
        effects.insert(2, element(11, 2, 500, 2));
        effects.insert(3, element(10, 3, 900, 0));
        effects.insert(4, element(11, 4, 700, 4));
        let positive = assign_row(&mut buffs, &effects, &catalog, false);
        let negative = assign_row(&mut debuffs, &effects, &catalog, true);
        assert_eq!(positive[0], Some(Shown { index: 10, stacks: 1, end: 900 }));
        assert_eq!(negative[0], Some(Shown { index: 11, stacks: 8, end: 700 }), "both frost instances: 3 + 5 stacks, the later end");
        assert!(positive[1].is_none() && negative[1].is_none());
        // A refresh is a new instance: the effect keeps its slot.
        effects.insert(5, element(12, 5, 0, 0));
        effects.remove(&3);
        effects.insert(6, element(10, 6, 1900, 7));
        let positive = assign_row(&mut buffs, &effects, &catalog, false);
        assert_eq!(positive[0].map(|s| (s.index, s.stacks)), Some((10, 1)), "capped at its MaxStackSize 1");
        assert_eq!(positive[1].map(|s| s.index), Some(12));
    }

    #[test]
    fn permanent_effects_give_way_past_six() {
        let mut catalog = catalog();
        for i in 20..28u16 {
            let effect = serde_json::json!({"id": format!("e{i}"), "debuff": false, "icon": "x.png", "name": "x", "stacks": 1});
            catalog.effects.insert(i, serde_json::from_value(effect).unwrap());
        }
        let mut effects = HashMap::new();
        // Four permanent ones first, then four with a timer.
        for (n, i) in (20..28u16).enumerate() { effects.insert(n as u32, element(i, n as u32, if i < 24 { 0 } else { 100 }, 0)); }
        let row = assign_row(&mut Vec::new(), &effects, &catalog, false);
        let shown: Vec<u16> = row.iter().flatten().map(|s| s.index).collect();
        assert_eq!(shown, [22, 23, 24, 25, 26, 27], "the two oldest permanent effects give way, in arrival order");
    }

    #[test]
    fn time_labels_use_the_short_units_and_say_when_they_change() {
        let units = catalog().units;
        assert_eq!(remaining(1, &units), ("1 s".into(), 1));
        assert_eq!(remaining(25 * 10, &units), ("10 s".into(), 25));
        assert_eq!(remaining(25 * 10 + 3, &units), ("11 s".into(), 3));
        assert_eq!(remaining(25 * 60, &units), ("60 s".into(), 25));
        assert_eq!(remaining(25 * 60 + 1, &units), ("1 m".into(), 1));
        assert_eq!(remaining(25 * 150, &units), ("2 m".into(), 25 * 30));
        assert_eq!(remaining(25 * 3600, &units), ("59 m".into(), 25 * 60));
        assert_eq!(remaining(25 * 3600 + 1, &units), ("1 hr".into(), 1));
        assert_eq!(remaining(25 * 86400 * 3, &units), ("2 d".into(), 25 * 86400));
        let mut slots: [[Option<Shown>; SLOTS]; ROWS] = default();
        slots[0][0] = Some(Shown { index: 10, stacks: 1, end: 1000 + 25 * 90 });
        slots[0][1] = Some(Shown { index: 12, stacks: 1, end: 0 });
        slots[1][2] = Some(Shown { index: 11, stacks: 3, end: 1000 + 25 * 5 + 10 });
        slots[TARGET][0] = Some(Shown { index: 11, stacks: 2, end: 1000 + 25 * 3 + 4 });
        let (times, next) = labels(&slots, 1000, &units);
        assert_eq!((times[0][0].as_str(), times[0][1].as_str(), times[1][2].as_str(), times[TARGET][0].as_str()), ("1 m", "", "6 s", "4 s"));
        assert_eq!(next, Some(1004), "the monster's frost label changes first, 4 ticks on");
        assert_eq!(labels(&slots, 1000 + 25 * 200, &units).1, None, "all over: nothing until the server removes them");
    }

    #[test]
    fn hover_lines_take_the_values_their_sign_and_stacks() {
        let catalog = catalog();
        let damage = &catalog.effects[&10].lines[0];
        assert_eq!(line(damage, &[0.25], 1, false).as_deref(), Some("+ 25% damage"));
        assert_eq!(line(damage, &[-0.125], 1, true).as_deref(), Some("- 12.5% damage"));
        assert_eq!(line(damage, &[0.0], 1, false), None, "a zero modifier says nothing");
        assert_eq!(line(damage, &[], 1, false).as_deref(), Some("+ —% damage"), "a missing parameter is unknown");
        let frost = &catalog.effects[&11].lines[0];
        assert_eq!(line(frost, &[], 3, true).as_deref(), Some("- 6% movement speed"));
        let pet = &catalog.effects[&12].lines;
        assert_eq!(line(&pet[0], &[], 1, false).as_deref(), Some("<WhiteStart>Pet Effect:</>"));
        assert_eq!(line(&pet[1], &[], 1, false).as_deref(), Some("+ —% all resistance values"));
    }

    #[test]
    fn the_monster_row_hangs_centred_under_the_health_bar_and_shifts_to_centre_its_icons() {
        let source = template::fixture::layout("interactinfo", &[("Canvas/Label_bg/Healthbar", "UVProgressBar", false)]);
        let layout = prepare_target(&source);
        let slots: Vec<&Widget> = layout.widgets.iter().filter(|w| slot_of(&w.id).is_some_and(|(row, _)| row == TARGET)).collect();
        let labels = layout.widgets.iter().filter(|w| time_label(&w.id).is_some()).count();
        assert_eq!((slots.len(), labels), (SLOTS, SLOTS));
        let [bar_left, _, bar_right, bar_bottom] = source.widgets[0].rect;
        let (left, right) = (slots[0].rect[0], slots[SLOTS - 1].rect[2]);
        assert!(((left + right) - (bar_left + bar_right)).abs() < 1e-3, "centred on the bar");
        assert!(slots.iter().all(|w| w.rect[1] > bar_bottom && w.click_through && w.art.is_none()));
        let mut row: [Option<Shown>; SLOTS] = default();
        assert_eq!(shift(0, &row), 0.0);
        row[0] = Some(Shown::default());
        assert_eq!(shift(TARGET, &row), 2.5 * (TARGET_SIZE + TARGET_GAP), "one icon: in the middle of the row");
        assert_eq!(shift(1, &row), 0.0, "the bottom bar's halves stay anchored");
    }

    /// docs/ui.md, "Per-frame cost": the slots are assigned when what they show
    /// changes, never on a frame where it did not.
    #[test]
    fn the_status_rows_are_reassigned_only_when_what_they_show_changes() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Catalog>().init_resource::<StatusBar>().init_resource::<UiWork>()
            .init_resource::<settings::Settings>().init_resource::<Hovered>().init_resource::<HoverInfo>()
            .add_systems(Update, sync);
        let handle = app.world_mut().resource_mut::<Assets<Catalog>>().add(catalog());
        app.world_mut().resource_mut::<StatusBar>().catalog = handle;
        app.world_mut().resource_mut::<settings::Settings>().current.monster_effects = true;
        crate::network::net::start_offline(app.world_mut(), 3, 0);
        let assigned = |app: &App| app.world().resource::<UiWork>().status_bars;
        let slots = |app: &App| app.world().resource::<StatusBar>().slots;
        app.update();
        assert_eq!(assigned(&app), 1);
        for _ in 0..10 { app.update(); }
        assert_eq!(assigned(&app), 1, "nothing changed: nothing assigned");
        {
            let mut net = app.world_mut().non_send_mut::<Net>();
            net.stat_effects.insert(7, element(12, 7, 0, 0));
            net.stat_revision += 1;
        }
        app.update();
        assert_eq!(assigned(&app), 2);
        assert_eq!(slots(&app)[0][0].map(|s| s.index), Some(12));
        for _ in 0..10 { app.update(); }
        assert_eq!(assigned(&app), 2, "a permanent effect has no timer to follow");
        // A monster's effects: its negative ones under its panel while it is hovered.
        const MONSTER: u32 = 0x6000_0000;
        app.world_mut().non_send_mut::<Net>().apply_actor_effects(MONSTER, true, &[element(11, 1, 0, 2), element(10, 2, 0, 0)]);
        app.update();
        assert_eq!(assigned(&app), 2, "no monster hovered: its effects change nothing");
        app.world_mut().resource_mut::<Hovered>().0 = Some((Entity::PLACEHOLDER, MONSTER));
        app.world_mut().resource_mut::<HoverInfo>().0 = Some(("Skeleton".into(), 3, 50, 100));
        app.update();
        assert_eq!(assigned(&app), 3);
        assert_eq!(slots(&app)[TARGET][0], Some(Shown { index: 11, stacks: 3, end: 0 }));
        assert!(slots(&app)[TARGET][1].is_none(), "its positive effects are not shown");
        for _ in 0..10 { app.update(); }
        assert_eq!(assigned(&app), 3);
        app.world_mut().non_send_mut::<Net>().apply_actor_effects(MONSTER + 1, true, &[element(11, 3, 0, 0)]);
        app.update();
        assert_eq!(assigned(&app), 3, "another monster's effects change nothing");
        app.world_mut().non_send_mut::<Net>().apply_actor_effects(MONSTER, false, &[StatusEffectElement { instance: 1, ..default() }]);
        app.update();
        assert_eq!(assigned(&app), 4);
        assert!(slots(&app)[TARGET][0].is_none(), "removed");
        app.world_mut().non_send_mut::<Net>().apply_actor_effects(MONSTER, false, &[element(11, 4, 0, 0)]);
        app.world_mut().resource_mut::<settings::Settings>().current.monster_effects = false;
        app.update();
        assert!(slots(&app)[TARGET][0].is_none(), "turned off in the settings");
        app.world_mut().resource_mut::<settings::Settings>().current.monster_effects = true;
        app.update();
        assert!(slots(&app)[TARGET][0].is_some());
        app.world_mut().resource_mut::<HoverInfo>().0 = None;
        app.update();
        assert!(slots(&app)[TARGET][0].is_none(), "no panel, no row");
    }
}
