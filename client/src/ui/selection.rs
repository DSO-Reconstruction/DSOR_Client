//! The character screens' original mesh UI: the roster (charselectionwindow, with
//! a runtime four-row list, and confirmdialog to delete) and the creation screen
//! (chargenwindow, with the premium start panel premadepackagewindow).
//! SOURCE: the 2026 client's UI::CharacterSelectionWidget and
//!   UI::CharacterGenerationWidget, decompiled in the live server tree's
//!   data/diagnostics/charselection-widget.annotated.txt and
//!   chargen-widget.annotated.txt (addresses below are theirs).
use super::*;
use crate::screens::selection::{class_key, gender_key, Choice, Creation, CreationData, Dialog, Notice, Selection, SelectionData};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseWheel;

pub struct SelectionUiPlugin;
impl Plugin for SelectionUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup)
            .add_systems(Update, (spawn, input, paint).chain()
                .after(crate::network::net::NetReceive).after(super::follow_screen));
    }
}

/// Which screen the layouts show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page { Roster, Creation }

/// None while the server is asked for the other screen (the editor, op 1; the
/// roster, op 2) and once the character is made: the original hides one screen
/// and shows the other on the server's answer.
fn page(selection: &Selection) -> Option<Page> {
    match &selection.dialog {
        Dialog::Create(c) => (c.open && !c.done).then_some(Page::Creation),
        _ => (!selection.returning).then_some(Page::Roster),
    }
}

#[derive(Resource)]
struct Data {
    roster: Vec<Handle<Layout>>,
    creation: Vec<Handle<Layout>>,
    maps: Handle<local_map::MapNames>,
    selection: Handle<SelectionData>,
}

impl Data {
    fn layouts(&self, page: Page) -> &[Handle<Layout>] {
        match page { Page::Roster => &self.roster, Page::Creation => &self.creation }
    }
}

#[derive(Component)] struct Root(Page);
#[derive(Component)] struct Suspended(Visibility);
#[derive(Component)]
struct Piece {
    button: Option<usize>,
    state: Option<String>,
    row: Option<usize>,
    value: Option<Value>,
    gate: Gate,
    /// Part of the class description's scroll handle (moved along its bar).
    handle: bool,
}
/// What decides whether a piece shows, beside its button state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gate {
    Always,
    /// The creation screen's class (its emblem and background).
    Class(u8),
    /// The creation screen's Beard row: men only (Refresh 0x1403ab8b0).
    Male,
    /// The creation screen's back button: unless the editor's first bool
    /// forbids it (Load 0x1403a971c).
    Back,
    /// The delete confirmation (confirmdialog).
    Delete,
    /// The roster's slot buttons (RefreshCharacterList 0x1403b054c).
    Add, Buy,
    /// A roster row's mentor bonus icons: honor (flag B), wisdom (flag A), both.
    Mentor(u8),
}
#[derive(Clone)]
enum Value {
    Name, Class, Level, Map, SelectedName, Status, Andermant,
    /// A class button's caption (gui.window.chargen_charClass_<class>).
    ClassButton(u8),
    /// The creation screen's class name (CharClass), the name typed (Name),
    /// its placeholder (NameTextLabel), a line of the class description; the
    /// premium start panel's headline; the delete confirmation's question.
    CharClass, Entry, Placeholder, Line(usize), Headline, Question,
}
#[derive(Clone, Debug, PartialEq)]
enum Action {
    Row(usize), Up, Down, Play, Add, Buy, Delete, Left, Right, Rotate, Fullscreen, Quit,
    Class(u8), Prev(Choice), Next(Choice), Create, Back, Name, Clear, Confirm, Cancel,
}
struct Hit { id: String, rect: [f32; 4], k: f32, action: Action }
#[derive(Resource, Default)]
struct Controls {
    hits: Vec<Hit>, hovered: Option<usize>, pressed: Option<usize>, drag_x: Option<f32>,
    /// The name field has the keyboard.
    focus: bool,
    /// The class description, as lines of the list, for the class it was made for.
    lines: (Option<u8>, Vec<String>),
    /// The description list: its rectangle and k (wheel scrolling), and the
    /// scroll handle's travel (reference pixels).
    list: Option<([f32; 4], f32)>,
    travel: f32,
}

/// The class description's lines: the List's TextLineTemplate rows.
const LINES: usize = 9;
/// Characters a description line holds (the template's 192 reference pixels of
/// 9-point Tahoma).
const LINE_WIDTH: usize = 36;
/// The rotate buttons turn the character this fast while held (radians a second).
const TURN_RATE: f32 = 2.0;

const CLASS_EVENTS: [&str; 5] = ["Warrior", "Mage", "Ranger", "Dwarf", "Niwalk"];

/// The class buttons' row (chargenwindow ClassGroup): the four authored ones
/// span 42..964 reference pixels, 211 wide; five are spread over the same span.
/// The Night Walker's (none authored: the original never finished the class)
/// is the Warrior's button, emblem and background moved and renamed.
/// UNVERIFIED: no original layout for five classes; the buttons now overlap by
/// a few pixels of the next one's icon.
const CLASS_ROW: (f32, f32, f32) = (42.0, 964.0, 211.0);

/// A widget's x moved by `dx` reference pixels (its rectangle and its art).
fn shifted(mut w: Widget, dx: f32) -> Widget {
    w.rect[0] += dx; w.rect[2] += dx;
    if let Some(a) = &mut w.art { for xy in a.pos.chunks_exact_mut(2) { xy[0] += dx; } }
    w
}

/// The authored widgets of the creation screen with the class row laid out for
/// every class, the Night Walker's button, emblem and background added.
fn with_every_class(layout: &Layout) -> Vec<Widget> {
    let (left, right, width) = CLASS_ROW;
    let step = (right - width - left) / (CLASS_EVENTS.len() - 1) as f32;
    let group = |w: &Widget| CLASS_EVENTS.iter().position(|c| w.id.contains(&format!("/ClassGroup/{c}")));
    let lefts: Vec<Option<f32>> = CLASS_EVENTS.iter()
        .map(|c| layout.widgets.iter().find(|w| w.id.ends_with(&format!("/ClassGroup/{c}"))).map(|w| w.rect[0])).collect();
    let mut out = Vec::with_capacity(layout.widgets.len() + 40);
    for w in &layout.widgets {
        let moved = match group(w) {
            Some(c) => match lefts[c] { Some(x) => shifted(w.clone(), left + step * c as f32 - x), None => w.clone() },
            None => w.clone(),
        };
        out.push(moved);
    }
    let niwalk = CLASS_EVENTS.len() - 1;
    if lefts[niwalk].is_none() {
        let from = "Warrior";
        for w in &layout.widgets {
            let rename = |w: &Widget, a: &str, b: &str| { let mut w = w.clone(); w.id = w.id.replacen(a, b, 1); w };
            if w.id.contains(&format!("/ClassGroup/{from}")) {
                let mut w = rename(w, &format!("/ClassGroup/{from}"), "/ClassGroup/Niwalk");
                if w.event == from { w.event = "Niwalk".into(); }
                let dx = left + step * niwalk as f32 - lefts[0].unwrap_or(left);
                out.push(shifted(w, dx));
            } else if w.id.contains(&format!("/Emblem{from}")) {
                out.push(rename(w, &format!("/Emblem{from}"), "/EmblemNiwalk"));
            } else if w.id.contains(&format!("/Background{from}")) {
                out.push(rename(w, &format!("/Background{from}"), "/BackgroundNiwalk"));
            }
        }
    }
    out
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    let load = |names: &[&str]| names.iter().map(|n| assets.load(format!("interface/{n}.ui.json"))).collect();
    commands.insert_resource(Data { roster: load(&["charselectionwindow", "currencybar", "gamebar", "confirmdialog"]),
        creation: load(&["chargenwindow", "premadepackagewindow", "currencybar", "gamebar"]),
        maps: assets.load("interface/maps.names.json"), selection: assets.load("characters/selection.json") });
    commands.init_resource::<Controls>();
}

/// The widgets a page draws: (widget, roster row, gate).
fn expanded(layout: &Layout) -> Vec<(Widget, Option<usize>, Gate)> {
    match layout.window.as_str() {
        "chargenwindow" => return expanded_creation(layout),
        "premadepackagewindow" => return expanded_packages(layout),
        "confirmdialog" => return expanded_confirm(layout),
        _ => {}
    }
    let template = layout.widgets.iter().find(|w| w.id.ends_with("/TemplateCharacter"));
    let mut result = Vec::new();
    for w in &layout.widgets {
        // The currency bar contains a dedicated RC-only version for this screen.
        let currency = layout.window == "currencybar" && w.id.starts_with("Canvas/LabelIngamePaymentRCOnly");
        // Hidden at Load and never shown: the class unlock and the instance
        // selection; the edits (EditButton, shown with free character edits) are
        // not offered; the selected character's level and map lie off screen.
        if (w.hidden && !currency) || ["/UnlockRequestButton", "/InstanceSelectionLabel", "/EditButton",
            "/SelectedCharacterLevelClass", "/SelectedCharacterMap"].iter().any(|s| w.id.contains(s)) { continue; }
        if layout.window == "currencybar" && !currency { continue; }
        if layout.window == "gamebar" && !w.id.contains("/gamemenu") && !w.id.contains("/fullscreen") { continue; }
        let gate = if w.id.contains("/CharacterAddButton") { Gate::Add }
            else if w.id.contains("/BuyCharacterButton") { Gate::Buy }
            else if w.id.contains("/LabelTwinkIncentiveHonorWisdom") { Gate::Mentor(3) }
            else if w.id.contains("/LabelTwinkIncentiveHonor") { Gate::Mentor(2) }
            else if w.id.contains("/LabelTwinkIncentiveWisdom") { Gate::Mentor(1) }
            else { Gate::Always };
        if let Some(t) = template.filter(|t| w.id.starts_with(&t.id)) {
            for row in 0..4 {
                let mut w = w.clone();
                let dy = row as f32 * (t.rect[3] - t.rect[1]);
                w.id = w.id.replacen(&t.id, &format!("{}/Row{row}", t.id), 1);
                w.rect[1] += dy; w.rect[3] += dy;
                if let Some(a) = &mut w.art { for xy in a.pos.chunks_exact_mut(2) { xy[1] += dy; } }
                result.push((w, Some(row), gate));
            }
        } else { result.push((w.clone(), None, gate)); }
    }
    result
}

/// The creation screen: every class's emblem and background (gated by the
/// class chosen), the description list's line template cloned for its rows,
/// without what Load hides (the instance selection, the class unlock, the
/// price tag of a premium package) and the character edit's SaveButton.
fn expanded_creation(layout: &Layout) -> Vec<(Widget, Option<usize>, Gate)> {
    let mut result = Vec::new();
    for w in &with_every_class(layout) {
        if ["/InstanceSelectionLabel/InstanceSelectionLabel", "/UnlockRequestButton", "/SaveButton", "/SelectedCharacterLevelClass",
            "/SelectedCharacterMap", "/Titel", "/PriceTag"].iter().any(|s| w.id.contains(s)) { continue; }
        let gate = if let Some(c) = CLASS_EVENTS.iter().position(|c| w.id.contains(&format!("/Emblem{c}")) || w.id.contains(&format!("/Background{c}"))) {
            Gate::Class(c as u8)
        } else if w.id.contains("/InformationLabel/Beard") { Gate::Male }
        else if w.id.contains("/CharacterSelectionShadowLabel") { Gate::Back }
        else { Gate::Always };
        if w.hidden && gate == Gate::Always { continue; }
        let mut w = w.clone();
        w.hidden = false;
        if w.id.ends_with("/TextLineTemplate") {
            let height = w.rect[3] - w.rect[1];
            for line in 0..LINES {
                let mut w = w.clone();
                let dy = line as f32 * height;
                w.id = format!("{}{line}", w.id);
                w.rect[1] += dy; w.rect[3] += dy;
                result.push((w, None, gate));
            }
            continue;
        }
        result.push((w, None, gate));
    }
    result
}

/// The premium start panel beside the editor, as it stands without offers: its
/// list empty (the templates hidden), its own buy button hidden (Load
/// 0x140499ce8), its clear button doing nothing.
fn expanded_packages(layout: &Layout) -> Vec<(Widget, Option<usize>, Gate)> {
    layout.widgets.iter().filter(|w| !w.hidden && !["/BuyButtonLabel", "/TemplatePackage"].iter().any(|s| w.id.contains(s)))
        .map(|w| (w.clone(), None, Gate::Always)).collect()
}

/// The confirmation's centred dialog, its default question (ShowConfirmDialog
/// 0x1403f57b4: DialogCenter, the Default layer).
fn expanded_confirm(layout: &Layout) -> Vec<(Widget, Option<usize>, Gate)> {
    layout.widgets.iter().filter(|w| !w.hidden && w.id.starts_with("Canvas/DialogCenter"))
        .map(|w| (w.clone(), None, Gate::Delete)).collect()
}

fn action(w: &Widget, row: Option<usize>) -> Option<Action> {
    if w.kind == "TextEntry" && w.event == "Name" { return Some(Action::Name); }
    if !matches!(w.kind.as_str(), "Button" | "TextButton") { return None; }
    let leaf = w.id.rsplit('/').next()?;
    if let Some(class) = CLASS_EVENTS.iter().position(|c| *c == w.event) { return Some(Action::Class(class as u8)); }
    Some(match leaf {
        "TemplateCharacterButton" => Action::Row(row?), "ScrollUp" => Action::Up, "ScrollDown" => Action::Down,
        "StartGame" => Action::Play, "CharacterAddButton" => Action::Add, "BuyCharacterButton" => Action::Buy,
        "CharacterDeleteButton" => Action::Delete,
        "RotateCharLeftButton" => Action::Left, "RotateCharRightButton" => Action::Right,
        "DragRotateAreaButton01" => Action::Rotate,
        _ => match w.event.as_str() {
            "Logout" => Action::Quit, "ToggleFullscreen" => Action::Fullscreen,
            "Ok" => Action::Create, "CharacterSelection" => Action::Back, "ClearPackage" => Action::Clear,
            "confirm" => Action::Confirm, "cancel" => Action::Cancel,
            "PrevGender" => Action::Prev(Choice::Gender), "NextGender" => Action::Next(Choice::Gender),
            "PrevSkinColor" => Action::Prev(Choice::Body), "NextSkinColor" => Action::Next(Choice::Body),
            "PrevHair" => Action::Prev(Choice::Hair), "NextHair" => Action::Next(Choice::Hair),
            "PrevBeard" => Action::Prev(Choice::Beard), "NextBeard" => Action::Next(Choice::Beard),
            _ => return None,
        },
    })
}

fn value(w: &Widget, row: Option<usize>) -> Option<Value> {
    let leaf = w.id.rsplit('/').next()?;
    if let Some(line) = w.id.rsplit_once("/TextLineTemplate").and_then(|(_, n)| n.parse().ok()) { return Some(Value::Line(line)); }
    if leaf == "TextLabel" && w.id.contains("/ClassGroup/") {
        return CLASS_EVENTS.iter().position(|c| w.id.contains(&format!("/ClassGroup/{c}/"))).map(|c| Value::ClassButton(c as u8));
    }
    Some(match leaf {
        "CharacterName" if row.is_some() => Value::Name,
        "CharacterClass" if row.is_some() => Value::Class,
        "CharacterLevel" if row.is_some() => Value::Level,
        "Map" if row.is_some() => Value::Map,
        "SelectedCharacterName" => Value::SelectedName,
        "DenyReason" => Value::Status,
        "andermant_display" => Value::Andermant,
        "CharClass" => Value::CharClass,
        "Name" if w.kind == "TextEntry" => Value::Entry,
        "NameTextLabel" => Value::Placeholder,
        "PackageInfo" => Value::Headline,
        "Question" if w.id.starts_with("Canvas/DialogCenter") => Value::Question,
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
fn spawn(mut commands: Commands, selection: Res<Selection>, data: Res<Data>, layouts: Res<Assets<Layout>>,
    assets: Res<AssetServer>, screen: Res<Screen>, mut controls: ResMut<Controls>, roots: Query<(Entity, &Root)>,
    mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<UiMaterial>>,
    mut windows: Query<(Entity, &mut Visibility, Option<&Suspended>), With<Window2018>>) {
    for (entity, mut visibility, suspended) in &mut windows {
        if selection.active && suspended.is_none() {
            commands.entity(entity).insert(Suspended(*visibility)); *visibility = Visibility::Hidden;
        } else if !selection.active {
            if let Some(saved) = suspended { *visibility = saved.0; commands.entity(entity).remove::<Suspended>(); }
        }
    }
    let page = page(&selection).filter(|_| selection.active);
    let current = roots.iter().next().map(|(_, r)| r.0);
    if current == page && (page.is_none() || !roots.is_empty()) { return; }
    if current.is_some() {
        for (root, _) in &roots { commands.entity(root).despawn(); }
        controls.hits.clear(); controls.pressed = None; controls.hovered = None; controls.drag_x = None; controls.list = None;
    }
    let Some(page) = page else { return };
    if data.layouts(page).iter().any(|h| layouts.get(h).is_none()) { return; }
    // The name field waits for a click (no focus at Load, OnShow or Refresh).
    controls.focus = false;
    let root = commands.spawn((Root(page), Transform::from_xyz(0.0, 0.0, 400.0), Visibility::Inherited)).id();
    let windows: Vec<_> = data.layouts(page).iter().map(|h| expanded(layouts.get(h).unwrap())).collect();
    let window_of: Vec<usize> = windows.iter().enumerate().flat_map(|(n, w)| std::iter::repeat_n(n, w.len())).collect();
    let widgets: Vec<_> = windows.into_iter().flatten().collect();
    controls.hits = widgets.iter().filter_map(|(w, row, _)| action(w, *row).map(|action| Hit { id: w.id.clone(), rect: w.rect, k: w.k, action })).collect();
    let list = widgets.iter().find(|(w, _, _)| w.id.ends_with("/FrameLabel7/TextList")).map(|(w, _, _)| (w.rect, w.k));
    let bar = widgets.iter().find(|(w, _, _)| w.id.ends_with("/TextList/ScrollBarBackground/ScrollBarBoundaries")).map(|(w, _, _)| w.rect);
    let knob = widgets.iter().find(|(w, _, _)| w.id.ends_with("/TextList/ScrollBarBackground/ScrollBarBoundaries/ScrollHandle")).map(|(w, _, _)| w.rect);
    controls.list = list;
    controls.travel = match (bar, knob) { (Some(b), Some(h)) => ((b[3] - b[1]) - (h[3] - h[1])).max(0.0), _ => 0.0 };
    let mut parents = HashMap::new();
    for (index, (w, row, gate)) in widgets.iter().enumerate() {
        let button = controls.hits.iter().position(|h| w.id == h.id || w.id.starts_with(&(h.id.clone() + "/")));
        let state = button.and_then(|b| w.id.strip_prefix(&(controls.hits[b].id.clone() + "/")))
            .and_then(|s| s.split('/').next()).filter(|s| matches!(*s, "normal" | "pressed" | "mouseover" | "disabled" | "mouseoverpressed")).map(str::to_owned);
        let handle = w.id.contains("/TextList/ScrollBarBackground/ScrollBarBoundaries/ScrollHandle");
        // Each window draws its still artwork, then its buttons, then its texts,
        // each in authored order: the crest of the creation screen's frame goes
        // under its Create button and caption, the class's tinted background
        // under the class name and description.
        // INFERRED: from those two layouts (authored order hides the caption
        //   under the crest and tints the text red); docs/ui.md's exceptions to
        //   authored order (captions over relief art, backdrops under texts)
        //   follow the same rule. The renderer itself was not traced.
        let z = |layer: usize| (window_of[index] * 3 + layer) as f32 + index as f32 / widgets.len() as f32;
        let mut parent = |flip: bool, commands: &mut Commands| {
            *parents.entry((w.k.to_bits(), flip)).or_insert_with(|| commands.spawn((Follows { k: w.k, flip },
                follow_transform(&screen, w.k, flip), Visibility::Inherited, ChildOf(root))).id())
        };
        if let Some(art) = &w.art {
            // Payment is decorative here; do not paint all of its authored states.
            if w.id.contains("/PaymentButton/") && !w.id.contains("/PaymentButton/normal") { continue; }
            let mut run = Run::default(); run.push(art, 0.0);
            if !run.idx.is_empty() {
                let p = parent(true, &mut commands);
                commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(materials.add(material(art.tex.as_ref().map(|t| assets.load(t.clone()))))),
                    Transform::from_xyz(0.0, 0.0, z(button.is_some() as usize)), RenderLayers::layer(LAYER), ChildOf(p), Visibility::Hidden,
                    Piece { button, state: state.clone(), row: *row, value: None, gate: *gate, handle }));
            }
        }
        if let Some(label) = &w.text {
            let dynamic = value(w, *row);
            if dynamic.is_none() && !static_text(label) { continue; }
            let [l, t, r, b] = w.rect;
            let x = match label.align[0] { 0 => l, 2 => r, _ => (l + r) * 0.5 };
            let y = match label.align[1] { 0 => t, 2 => b, _ => (t + b) * 0.5 };
            let p = parent(false, &mut commands);
            commands.spawn((Text2d::new(if dynamic.is_some() { String::new() } else { label.text.clone() }),
                TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(label.size.max(1.0) * FONT_SCALE * screen.scale), ..default() },
                TextColor(Color::srgba(label.color[0], label.color[1], label.color[2], label.color[3])),
                Anchor(Vec2::new(match label.align[0] { 0 => -0.5, 2 => 0.5, _ => 0.0 }, match label.align[1] { 0 => 0.5, 2 => -0.5, _ => 0.0 })),
                Transform::from_xyz(x, -y, z(2)).with_scale(Vec3::splat(1.0 / screen.scale)),
                // The source's text rectangles stop before the unused incentive icons.
                // Allow long names/map captions across the rest of their roster row.
                bevy::text::TextBounds::new_horizontal((if row.is_some() { 220.0 } else { r - l }) * screen.scale),
                TextLayout::justify(match label.align[0] { 0 => Justify::Left, 2 => Justify::Right, _ => Justify::Center }),
                RenderLayers::layer(LAYER), ChildOf(p), Visibility::Hidden, Piece { button, state, row: *row, value: dynamic, gate: *gate, handle: false }));
        }
    }
}

fn creation(selection: &Selection) -> Option<&Creation> {
    match &selection.dialog { Dialog::Create(c) => Some(c), _ => None }
}

/// The roster's slot buttons: (Add shown, Add enabled, Buy shown). Add while
/// a slot is free; a full account buys one until MaxCharactersPerAccount, then
/// Add shows again, disabled.
fn slots(selection: &Selection, data: Option<&CreationData>) -> (bool, bool, bool) {
    let count = selection.characters.len();
    let most = data.map_or(5, |d| d.most.max(1));
    if count < selection.capacity as usize { (true, true, false) }
    else if count < most { (false, false, true) }
    else { (true, false, false) }
}

fn enabled(action: &Action, selection: &Selection, data: Option<&CreationData>) -> bool {
    match action {
        Action::Fullscreen | Action::Quit => return true,
        Action::Confirm | Action::Cancel => return matches!(selection.dialog, Dialog::Delete { .. }),
        Action::Class(_) | Action::Prev(_) | Action::Next(_) | Action::Create | Action::Name | Action::Clear => return creation(selection).is_some(),
        Action::Back => return creation(selection).is_some_and(|c| !c.editor.flag),
        _ => {}
    }
    if creation(selection).is_some() { return matches!(action, Action::Rotate); }
    if selection.entering || selection.dialog != Dialog::None { return false; }
    match action {
        Action::Row(row) => selection.first_row + row < selection.characters.len(),
        Action::Up => selection.first_row > 0,
        Action::Down => selection.first_row + 4 < selection.characters.len(),
        Action::Play | Action::Left | Action::Right | Action::Rotate => selection.current().is_some(),
        Action::Delete => !selection.characters.is_empty(),
        Action::Add => !selection.offline && slots(selection, data).1,
        // DEVIATION: buying a slot (the misc_character_extension offer) is not
        // offered: the button shows, disabled as when it is too dear.
        Action::Buy => false,
        _ => true,
    }
}

#[allow(clippy::too_many_arguments)]
fn input(mut selection: ResMut<Selection>, mut controls: ResMut<Controls>, mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>, mut scroll: MessageReader<MouseWheel>, mut typed: MessageReader<KeyboardInput>, time: Res<Time>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>, loading: Res<crate::screens::loading::LoadingScreen>, mut exit: MessageWriter<AppExit>,
    data: Res<Data>, selection_data: Res<Assets<SelectionData>>, mut audio: Option<ResMut<crate::audio::AudioRequests>>) {
    let mut sound = |cue: &str| if let Some(audio) = audio.as_mut() { audio.ui(cue) };
    if !selection.cues.is_empty() { for cue in std::mem::take(&mut selection.cues) { sound(&cue); } }
    if !selection.active || loading.active() { scroll.clear(); typed.clear(); return; }
    let Ok(mut window) = windows.single_mut() else { return };
    let creating = selection_data.get(&data.selection).map(|d| &d.creation);
    let cursor = window.cursor_position().map(|p| p * (BASE_H / window.height()));
    let width = BASE_H * window.width() / window.height();
    let inside = |rect: [f32; 4], k: f32, p: Vec2| { let [l,t,r,b] = rect; let x = p.x - k * (width - BASE_W); x >= l && x <= r && p.y >= t && p.y <= b };
    let hovered = cursor.and_then(|p| controls.hits.iter().enumerate().rev()
        .filter(|(_, h)| enabled(&h.action, &selection, creating) && inside(h.rect, h.k, p))
    // The authored portrait drag rectangle overlaps the rotation arrows.
    // Explicit buttons take priority over that broad background hit area.
        .min_by_key(|(_, h)| matches!(h.action, Action::Rotate)).map(|(i, _)| i));
    if controls.hovered != hovered { controls.hovered = hovered; }
    let wheel: f32 = scroll.read().map(|w| w.y).sum();
    if let (Some(c), Some(creating)) = (creation(&selection), creating) {
        // The name field types while it has the keyboard; Enter (which commits)
        // and Escape let go of it. The screen itself has no keys.
        let keys_typed: Vec<_> = typed.read().filter(|e| controls.focus && e.state.is_pressed()
            && !matches!(e.key_code, KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Escape | KeyCode::Tab)).cloned().collect();
        let scrolled = wheel != 0.0 && cursor.is_some_and(|p| controls.list.is_some_and(|(rect, k)| inside(rect, k, p)));
        if controls.focus && keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Escape]) { controls.focus = false; }
        if !keys_typed.is_empty() || scrolled {
            let mut c = c.clone();
            let mut changed = false;
            for event in keys_typed { changed |= c.edit(event.key_code, event.text.as_deref(), creating); }
            if scrolled {
                let max = controls.lines.1.len().saturating_sub(LINES);
                let next = (c.scroll as i32 - wheel.signum() as i32).clamp(0, max as i32) as usize;
                changed |= next != c.scroll;
                c.scroll = next;
            }
            if changed { selection.dialog = Dialog::Create(c); selection.creation_changed(false, false); }
        }
    } else if matches!(selection.dialog, Dialog::Delete { .. }) {
        typed.clear();
        if keys.just_pressed(KeyCode::Escape) { selection.answer_delete(false); sound("ui/general/ui_cancel"); }
    } else if selection.dialog == Dialog::None && !selection.entering {
        // ADDITION: the original's roster has no keys; the arrows choose, Enter plays.
        typed.clear();
        if keys.just_pressed(KeyCode::ArrowUp) { selection.step(-1); }
        if keys.just_pressed(KeyCode::ArrowDown) { selection.step(1); }
        if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) { selection.play(); }
        if wheel != 0.0 && controls.hovered.is_some_and(|i| matches!(controls.hits[i].action, Action::Row(_) | Action::Up | Action::Down)) {
            let max = selection.characters.len().saturating_sub(4);
            selection.first_row = (selection.first_row as i32 - wheel.signum() as i32).clamp(0, max as i32) as usize;
        }
    } else { typed.clear(); }
    if mouse.just_pressed(MouseButton::Left) {
        controls.pressed = controls.hovered;
        controls.drag_x = cursor.map(|p| p.x);
        // A click anywhere but on the name field takes the keyboard from it.
        let on_name = controls.hovered.is_some_and(|i| controls.hits[i].action == Action::Name);
        if controls.focus && !on_name { controls.focus = false; }
    }
    if mouse.pressed(MouseButton::Left) {
        if let Some(i) = controls.pressed {
            match controls.hits[i].action {
                Action::Left => selection.angle += time.delta_secs() * TURN_RATE,
                Action::Right => selection.angle -= time.delta_secs() * TURN_RATE,
                Action::Rotate => if let Some(x) = cursor.map(|p| p.x) {
                    if let Some(old) = controls.drag_x { selection.angle += (x - old) * 0.012; }
                    controls.drag_x = Some(x);
                },
                _ => {}
            }
        }
    }
    if mouse.just_released(MouseButton::Left) {
        if let Some(i) = controls.pressed.filter(|i| Some(*i) == controls.hovered) {
            let action = controls.hits[i].action.clone();
            if enabled(&action, &selection, creating) {
                click(action, &mut selection, &mut controls, creating, &mut window, &mut exit, &mut sound);
            }
        }
        controls.pressed = None; controls.drag_x = None;
    }
}

/// A released click on an enabled control, with the original's sound.
fn click(action: Action, selection: &mut Selection, controls: &mut Controls, creating: Option<&CreationData>,
    window: &mut Window, exit: &mut MessageWriter<AppExit>, sound: &mut impl FnMut(&str)) {
    match action {
        Action::Row(row) => {
            let c = &selection.characters[selection.first_row + row];
            let (id, class, gender) = (c.character, c.player_class as u8, c.gender as u8);
            if selection.selected != Some(id) {
                sound(&format!("ui/char_gen/ui_char_gen_choose_{}", class_key(class)));
                sound(&format!("ui/char_gen/ui_char_gen_vo_{}_{}", class_key(class), gender_key(gender)));
            }
            selection.choose(id);
        }
        Action::Up => selection.first_row = selection.first_row.saturating_sub(1),
        Action::Down => selection.first_row += 1,
        Action::Play => selection.play(),
        Action::Add => { selection.open_editor(); sound("ui/general/ui_switch_screen"); }
        Action::Delete => { selection.ask_delete(); sound("ui/general/ui_cancel"); sound("ui/general/ui_open_screen"); }
        Action::Confirm => { selection.answer_delete(true); sound("ui/general/ui_confirm"); }
        Action::Cancel => { selection.answer_delete(false); sound("ui/general/ui_cancel"); }
        Action::Fullscreen => { window.mode = if matches!(window.mode, bevy::window::WindowMode::Windowed) {
            bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current)
        } else { bevy::window::WindowMode::Windowed }; }
        Action::Quit => { exit.write(AppExit::Success); }
        Action::Back => { selection.leave_editor(); sound("ui/general/ui_switch_screen"); }
        Action::Create => selection.create(),
        Action::Name => {
            sound("ui/general/ui_click");
            if !controls.focus {
                controls.focus = true;
                // Taking the keyboard hides the last refusal, but for a figure's
                // (8: `lastDenyReason & 7`, HandleEvent 0x1403a9676).
                if matches!(selection.notice, Notice::Refused(r) if r & 7 != 0) { selection.notice = Notice::None; selection.revision += 1; }
            }
        }
        Action::Class(_) | Action::Prev(_) | Action::Next(_) => if let (Some(c), Some(creating)) = (creation(selection), creating) {
            let mut c = c.clone();
            let turn = matches!(action, Action::Class(_) | Action::Prev(Choice::Gender) | Action::Next(Choice::Gender));
            match action {
                Action::Class(class) => {
                    c.set_look(class, c.gender);
                    sound(&format!("ui/char_gen/ui_char_gen_choose_{}", class_key(class)));
                    sound(&format!("ui/char_gen/ui_char_gen_vo_{}_{}", class_key(class), gender_key(c.gender)));
                }
                Action::Prev(Choice::Gender) | Action::Next(Choice::Gender) => {
                    c.step(Choice::Gender, 1, creating);
                    sound("ui/char_gen/ui_char_gen_switch_gender");
                    sound(&format!("ui/char_gen/ui_char_gen_vo_{}_{}", class_key(c.class), gender_key(c.gender)));
                }
                Action::Prev(choice) => { c.step(choice, -1, creating); sound("ui/char_gen/ui_char_gen_switch"); }
                Action::Next(choice) => { c.step(choice, 1, creating); sound("ui/char_gen/ui_char_gen_switch"); }
                _ => {}
            }
            let look = creation(selection).map(|old| (old.class, old.gender, old.body, old.hair, old.beard, old.showcase))
                != Some((c.class, c.gender, c.body, c.hair, c.beard, c.showcase));
            if look {
                selection.dialog = Dialog::Create(c);
                selection.creation_changed(true, turn);
            }
        },
        _ => {}
    }
}

/// The class description's lines: the description then the slogan, each
/// paragraph wrapped to the list's width, the empty ones kept
/// (UpdateCharacterDescriptions 0x1403ac790; a literal "\n" is a new line).
fn description_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.replace("\\n", "\n").split('\n') {
        let wrapped = super::controls::template::wrap(paragraph, LINE_WIDTH, usize::MAX);
        if wrapped.is_empty() { lines.push(String::new()); } else { lines.extend(wrapped); }
    }
    lines
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn paint(selection: Res<Selection>, mut controls: ResMut<Controls>, data: Res<Data>, maps: Res<Assets<local_map::MapNames>>,
    selection_data: Res<Assets<SelectionData>>, roots: Query<(), With<Root>>,
    mut pieces: Query<(&Piece, &mut Visibility, Option<&mut Text2d>, &mut Transform)>,
    mut last: Local<Option<(u64, Option<usize>, Option<usize>, bool, bool, bool, usize)>>) {
    if !selection.active { *last = None; return; }
    let creating = selection_data.get(&data.selection).map(|d| &d.creation);
    let c = creation(&selection);
    // The class description, made again only for another class.
    if let (Some(c), Some(creating)) = (c, creating) {
        if controls.lines.0 != Some(c.class) {
            let key = |k: &str| creating.text(&format!("chargen_{k}_{}", class_key(c.class))).to_owned();
            controls.lines = (Some(c.class), description_lines(&format!("{}\n{}", key("chardesc"), key("charslogan"))));
        }
    }
    // Nothing it shows changed: no work (docs/ui.md, "Per-frame cost").
    let key = (selection.revision, controls.hovered, controls.pressed, controls.focus, creating.is_some(),
        maps.get(&data.maps).is_some(), pieces.iter().len() + roots.iter().len());
    if *last == Some(key) { return; }
    *last = Some(key);
    let lines = &controls.lines.1;
    let scroll = c.map_or(0, |c| c.scroll);
    let handle_dy = if lines.len() > LINES { controls.travel * scroll as f32 / (lines.len() - LINES) as f32 } else { 0.0 };
    let (add, _, buy) = slots(&selection, creating);
    let text = |key: &str| creating.map(|d| d.text(key).to_owned()).unwrap_or_default();
    let class_name = |class: u8, gender: u8| text(&format!("chargen_charClass_{}{}", class_key(class), if gender == 1 { ".f" } else { "" }));
    for (piece, mut visibility, label, mut tf) in &mut pieces {
        let row = piece.row.and_then(|r| selection.characters.get(selection.first_row + r));
        let gated = match piece.gate {
            Gate::Always => true,
            Gate::Class(class) => c.is_some_and(|c| c.class == class),
            Gate::Male => c.is_some_and(|c| c.gender == 0),
            Gate::Back => c.is_some_and(|c| !c.editor.flag),
            Gate::Delete => matches!(selection.dialog, Dialog::Delete { .. }),
            Gate::Add => add,
            Gate::Buy => buy,
            Gate::Mentor(kind) => row.is_some_and(|r| (r.unknown_flag_0 as u8) | ((r.unknown_flag_1 as u8) << 1) == kind),
        };
        let shown = gated && if let (Some(button), Some(state)) = (piece.button, &piece.state) {
            let action = &controls.hits[button].action;
            let selected = (matches!(action, Action::Row(_)) && row.is_some_and(|c| Some(c.character) == selection.selected))
                || matches!(action, Action::Class(class) if c.is_some_and(|c| c.class == *class));
            let expected = if !enabled(action, &selection, creating) { "disabled" }
                else if controls.pressed == Some(button) || selected { "pressed" }
                else if controls.hovered == Some(button) { "mouseover" } else { "normal" };
            state == expected
        } else { true };
        let want = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != want { *visibility = want; }
        if piece.handle && tf.translation.y != handle_dy { tf.translation.y = handle_dy; }
        if let (Some(value), Some(mut label)) = (&piece.value, label) {
            let value = match value {
                Value::Name => row.map(|c| c.name.clone()).unwrap_or_default(),
                Value::Class => row.map(|c| class_name(c.player_class as u8, c.gender as u8)).unwrap_or_default(),
                Value::Level => row.map(|c| crate::screens::selection::fill(&text("charselection_characterLevel"), &[&c.level.to_string()])).unwrap_or_default(),
                Value::Map => row.map(|c| maps.get(&data.maps).map(|n| n.caption(&c.map_name)).unwrap_or_else(|| c.map_name.clone())).unwrap_or_default(),
                Value::SelectedName => selection.current().map(|c| c.name.clone()).unwrap_or_default(),
                Value::Status => selection.notice.text(creating),
                // The currency bar's Andermant, shortened past 9 digits: the
                // roster's last character's, the editor's own (SetupCharacter
                // 0x14023a5b4 sets it from the command's +0x90).
                Value::Andermant => match c {
                    Some(c) => super::controls::currency::clamped(c.editor.words[3] as u64, 9),
                    None => selection.characters.last().map(|c| super::controls::currency::clamped(c.andermant as u64, 9)).unwrap_or_default(),
                },
                Value::ClassButton(class) => class_name(*class, c.map_or(0, |c| c.gender)),
                Value::CharClass => c.map(|c| class_name(c.class, c.gender)).unwrap_or_default(),
                Value::Entry => c.map(|c| {
                    let mut name = c.name.clone();
                    if controls.focus { name.insert(c.caret, '|'); }
                    name
                }).unwrap_or_default(),
                Value::Placeholder => c.filter(|c| c.name.is_empty() && !controls.focus).map(|_| text("chargen_charName")).unwrap_or_default(),
                Value::Line(i) => lines.get(scroll + i).cloned().unwrap_or_default(),
                Value::Headline => text("gui.payment.packages_premadePackageHeadline"),
                Value::Question => match &selection.dialog {
                    Dialog::Delete { name, .. } => crate::screens::selection::fill(&text("charselection_deleteCharacter"), &[name]),
                    _ => String::new(),
                },
            };
            if **label != value { **label = value; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::selection::Request;
    use dsor_proto::commands::player::{CharacterSelection, SelectionCharacter};

    fn layout(name: &str) -> Option<Layout> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../assets/interface/{name}.ui.json"));
        path.exists().then(|| serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap())
    }

    #[test]
    #[ignore = "requires locally converted character selection XML assets"]
    fn roster_pointer_keyboard_resize_rotation_and_delete_confirmation() {
        let hits = ["charselectionwindow", "confirmdialog"].iter().flat_map(|n| expanded(&layout(n).unwrap())).filter_map(|(w, row, _)| action(&w, row).map(|action|
            Hit { id: w.id.clone(), rect: w.rect, k: w.k, action })).collect();
        let mut state = Selection::default();
        state.receive(CharacterSelection { operation: 1, character: 1, free: 6,
            characters: (1..=5).map(|id| SelectionCharacter { character: id, name: format!("Hero{id}"), ..default() }).collect(), ..default() }, None);
        state.requests.clear();
        let mut app = App::new();
        app.insert_resource(state).insert_resource(Controls { hits, ..default() })
            .insert_resource(Data { roster: Vec::new(), creation: Vec::new(), maps: default(), selection: default() })
            .init_resource::<Assets<SelectionData>>()
            .init_resource::<ButtonInput<MouseButton>>().init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Time>().init_resource::<crate::screens::loading::LoadingScreen>()
            .add_message::<MouseWheel>().add_message::<AppExit>().add_message::<KeyboardInput>()
            .add_systems(Update, input);
        let primary = app.world_mut().spawn((Window { focused: true, resolution: (1280, 720).into(), ..default() }, PrimaryWindow)).id();
        let click = |app: &mut App, action: Action| {
            let (rect, k) = { let c = app.world().resource::<Controls>(); let h = c.hits.iter().find(|h| h.action == action).unwrap(); (h.rect, h.k) };
            let mut window = app.world_mut().get_mut::<Window>(primary).unwrap();
            let width = BASE_H * window.width() / window.height(); let scale = window.height() / BASE_H;
            let [l,t,r,b] = rect;
            window.set_cursor_position(Some(Vec2::new((l+r)*0.5+k*(width-BASE_W),(t+b)*0.5)*scale));
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left); app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left); app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        };
        click(&mut app, Action::Row(1)); assert_eq!(app.world().resource::<Selection>().selected, Some(2));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown); app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        assert_eq!(app.world().resource::<Selection>().selected, Some(3));
        click(&mut app, Action::Down); assert_eq!(app.world().resource::<Selection>().first_row, 1);
        app.world_mut().get_mut::<Window>(primary).unwrap().resolution.set(1920.0, 1080.0);
        click(&mut app, Action::Row(3)); assert_eq!(app.world().resource::<Selection>().selected, Some(5));
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
        click(&mut app, Action::Left); assert!(app.world().resource::<Selection>().angle > 0.1);
        assert!(app.world().resource::<Selection>().requests.iter().all(|r| matches!(r, Request::Preview(_))));
        app.world_mut().resource_mut::<Selection>().requests.clear();
        click(&mut app, Action::Delete);
        assert!(matches!(app.world().resource::<Selection>().dialog, Dialog::Delete { id: 5, .. }));
        assert!(app.world().resource::<Selection>().requests.is_empty());
        click(&mut app, Action::Cancel);
        assert_eq!(app.world().resource::<Selection>().dialog, Dialog::None);
        assert!(app.world().resource::<Selection>().requests.is_empty());
        click(&mut app, Action::Delete);
        click(&mut app, Action::Confirm);
        let state = app.world().resource::<Selection>();
        assert!(matches!(state.requests.as_slice(), [Request::Delete(5), Request::Preview(1)]));
        assert_eq!((state.characters.len(), state.selected), (4, Some(1)), "the character leaves at once, the first is chosen");
    }

    #[test]
    fn four_rows_keep_authored_geometry_and_hide_alternative_buttons() {
        let Some(layout) = layout("charselectionwindow") else { return };
        let widgets = expanded(&layout);
        let rows: Vec<_> = widgets.iter().filter(|(w, r, _)| matches!(action(w, *r), Some(Action::Row(_)))).collect();
        assert_eq!(rows.len(), 4);
        for pair in rows.windows(2) { assert!((pair[0].0.rect[3] - pair[1].0.rect[1]).abs() < 0.1); }
        assert!(!widgets.iter().any(|(w, _, _)| w.id.contains("UnlockRequestButton") || w.id.contains("EditButton")));
        assert!(widgets.iter().any(|(w, _, g)| *g == Gate::Buy && w.id.ends_with("/BuyCharacterButton")));
        assert!(widgets.iter().any(|(w, _, g)| *g == Gate::Mentor(3) && w.id.contains("/Row3/")));
    }

    #[test]
    fn the_slot_buttons_follow_the_account() {
        let data = CreationData { most: 5, ..default() };
        let mut state = Selection::default();
        state.capacity = 3;
        state.characters = vec![SelectionCharacter::default(); 2];
        assert_eq!(slots(&state, Some(&data)), (true, true, false));
        state.characters.push(SelectionCharacter::default());
        assert_eq!(slots(&state, Some(&data)), (false, false, true));
        state.characters.resize(5, SelectionCharacter::default());
        state.capacity = 5;
        assert_eq!(slots(&state, Some(&data)), (true, false, false));
    }

    #[test]
    fn the_creation_layout_has_every_control_and_one_emblem_a_class() {
        let Some(source) = layout("chargenwindow") else { return };
        let widgets = expanded(&source);
        let actions: Vec<_> = widgets.iter().filter_map(|(w, r, _)| action(w, *r)).collect();
        for want in [Action::Class(0), Action::Class(1), Action::Class(2), Action::Class(3), Action::Create, Action::Back, Action::Name, Action::Rotate,
            Action::Prev(Choice::Gender), Action::Next(Choice::Gender), Action::Prev(Choice::Body), Action::Next(Choice::Body),
            Action::Prev(Choice::Hair), Action::Next(Choice::Hair), Action::Prev(Choice::Beard), Action::Next(Choice::Beard)] {
            assert!(actions.contains(&want), "{want:?}");
        }
        assert!(!widgets.iter().any(|(w, _, _)| w.id.contains("SaveButton") || w.id.contains("InstanceSelectionButton") || w.id.contains("PriceTag")));
        for class in 0..4 {
            assert!(widgets.iter().any(|(w, _, g)| *g == Gate::Class(class) && w.id.contains("/Emblem")), "emblem {class}");
            assert!(widgets.iter().any(|(w, _, g)| *g == Gate::Class(class) && w.id.contains("/Background")), "background {class}");
        }
        assert!(widgets.iter().filter(|(w, _, _)| w.id.contains("/InformationLabel/Beard")).all(|(_, _, g)| *g == Gate::Male));
        assert!(widgets.iter().any(|(w, _, g)| *g == Gate::Back && w.id.ends_with("/CharacterSelection")));
        let lines: Vec<_> = widgets.iter().filter_map(|(w, r, _)| match value(w, *r) { Some(Value::Line(i)) => Some((i, w.rect)), _ => None }).collect();
        assert_eq!(lines.len(), LINES);
        assert!(lines.windows(2).all(|p| p[1].1[1] > p[0].1[1]));
        if let Some(packages) = layout("premadepackagewindow") {
            let widgets = expanded(&packages);
            assert!(!widgets.iter().any(|(w, _, _)| w.id.contains("/BuyButtonLabel") || w.id.contains("/TemplatePackage")));
            assert!(widgets.iter().any(|(w, r, _)| matches!(value(w, *r), Some(Value::Headline))));
        }
    }

    #[test]
    fn descriptions_keep_their_paragraphs() {
        let lines = description_lines("Protector of Power \\n\nStrengths: Melee combat \nThe blood of the Dragon runs through the veins of the proud Dragonknight.");
        assert_eq!(lines[0], "Protector of Power");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "Strengths: Melee combat");
        assert!(lines.len() > 4 && lines.iter().all(|l| l.chars().count() <= LINE_WIDTH));
    }
}
