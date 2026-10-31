//! The quest log (questlog) and the quest tracker (activequestwindow), filled from
//! the server's log (crate::gameplay::quests) and the quest data.
//!
//! The log lists the active quests grouped by type (TemplateQuesttypes headers
//! with their count; a header folds its group), one row per quest (marker,
//! level, title, its zone or state, a cancel button for the quests the server
//! lets us drop), and the open quest's overview: its text, its objectives and
//! its rewards, stacked down their Lists as UI::QuestLogWidget fills them. The
//! wheel scrolls the quest list and the overview (crate::ui::controls::systems
//! routes it). The tracker (top right, against the screen's right edge) follows
//! one quest: its title, zone and objectives, its frame fitted to them; its
//! toggle folds it to the compact button.
//! SOURCE: dso-server-production services/quests.py _quest_cancel (operation 2,
//!   only quests.xml deletable quests), docs/QUEST_PROTOCOL.md; the client's
//!   QuestLogWidget / ActiveQuestWidget element names (dro_client64.exe strings:
//!   TemplateQuesttypes, QuestCounter, DescriptionList, RewardList1,
//!   TemplateXP, TemplateRCVC..., MatchTaskListHeight).
use bevy::prelude::*;
use std::collections::HashSet;

use crate::gameplay::quests::{QuestCatalog, QuestUi, CANCEL};
use crate::ui::{dialogue, local_map, Layout, Loaded, Widget, Window2018};
use super::template::{clone_at, clone_rows, grey, icon_layers, is_template, leaf, make_button, selected, set_text, shift, without, wrap, Icon};
use dsor_proto::commands::{actors, ClientCommand};

pub(in crate::ui) const WINDOW: &str = "questlog";
pub(in crate::ui) const TRACKER: &str = "activequestwindow";
pub(in crate::ui) const LIST: &str = "Canvas/BackGroundLabel/QuestlogLabel/QuestListBackground/CategoriesList";
pub(in crate::ui) const OVERVIEW: &str = "Canvas/BackGroundLabel/QuestlogLabel/QuestOverview";
const LABEL: &str = "Canvas/ActiveQuestLabel";
const TRACKER_FRAME: &str = "Canvas/ActiveQuestLabel/LabelFull/FrameLabel";
/// The quest list: entries stack from its top (145) to its bottom (627).
const LIST_TOP: f32 = 145.0;
const LIST_BOTTOM: f32 = 627.0;
/// The description list (183..468) and the reward list (517..627).
const TEXT_TOP: f32 = 183.0;
const TEXT_BOTTOM: f32 = 468.0;
const REWARD_TOP: f32 = 517.0;
const TEXT_WIDTH: usize = 64;
/// The tracker's objectives start under the title line (138) at 17 a line; its
/// frame ends at least 16 under the last one.
const TRACKER_TASK_HEIGHT: f32 = 17.0;
const TRACKER_FRAME_BOTTOM: f32 = 325.0;
const TRACKER_MIN_BOTTOM: f32 = 175.0;
const MOST_TRACKER_TASKS: usize = 8;
/// The tracker's lines, an objective's wrapped ones included.
const MOST_TRACKER_LINES: usize = 12;
/// The characters a line of objective holds: the tracker's (213 wide) and the
/// log's (388 wide), at about 6.2 a character.
const TRACKER_TASK_WIDTH: usize = 34;
const TASK_WIDTH: usize = 60;
/// An objective's box: ticked once done (the layouts' TaskIcon shows the tick).
const TASK_DONE: &str = "textures/ui/icon_checkmark.png";
const TASK_OPEN: &str = "textures/ui/icon_checkmark_empty.png";

#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct TaskView {
    pub title: String,
    pub progress: u32,
    pub target: u32,
    pub done: bool,
}

impl TaskView {
    fn line(&self) -> String {
        if self.target > 1 { format!("{} ({}/{})", self.title, self.progress.min(self.target), self.target) }
        else { self.title.clone() }
    }

    /// Its lines at `width` characters.
    fn lines(&self, width: usize) -> Vec<String> {
        let lines = wrap(&self.line(), width, 4);
        if lines.is_empty() { vec![String::new()] } else { lines }
    }
}

/// The objective's box: ticked or empty.
fn task_icon(w: &mut Widget, done: bool) {
    if let Some(art) = &mut w.art { art.tex = Some(if done { TASK_DONE } else { TASK_OPEN }.to_owned()); }
}

impl QuestView {
    /// The overview's rows: the text's lines, the objectives' heading, the
    /// objectives' lines.
    pub(in crate::ui) fn overview_rows(&self) -> usize {
        wrap(&self.description, TEXT_WIDTH, 40).len() + usize::from(!self.tasks.is_empty()) + self.tasks.iter().map(|t| t.lines(TASK_WIDTH).len()).sum::<usize>()
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct QuestView {
    pub id: String,
    pub title: String,
    pub level: u32,
    pub state: u8,
    /// The quest's group (quests.xml type, repeatable variants folded in).
    pub kind: String,
    /// The zone it is done in.
    pub zone: String,
    pub description: String,
    pub deletable: bool,
    pub tasks: Vec<TaskView>,
    /// Experience, gold in copper, Andermant.
    pub rewards: (u32, u32, u32),
    pub items: Vec<(u32, Icon)>,
    /// The reward items' templates, in `items`' order (their hover cards).
    pub item_templates: Vec<String>,
}

impl QuestView {
    fn zone_line(&self) -> String {
        if self.state == 1 { "Completed: return to the quest giver".to_owned() } else { self.zone.clone() }
    }
}

/// The quest groups in the log's order, and their headers' texts
/// (gui.window.questlog_*QuestCategoryTitle).
const GROUPS: [(&str, &str); 7] = [("world", "Main Quests"), ("local", "Side Quests"), ("event", "Event Quests"), ("heroic", "Parallel World Quests"),
    ("crafting", "Crafting"), ("pvp", "Battle Quests"), ("wanted", "Fortune Hunters")];

fn group(kind: &str) -> &'static str {
    let base = kind.trim_end_matches("repeatable");
    GROUPS.iter().map(|(k, _)| *k).find(|k| *k == base).unwrap_or(if base == "row" || base == "treasuredigging" { "local" } else { "world" })
}

/// The quests as shown: the active ones, which one the log opens, which one the
/// tracker follows, the folded groups, the lists' scroll.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct Quests {
    pub list: Vec<QuestView>,
    pub selected: usize,
    pub tracked: usize,
    pub folded: Vec<String>,
    /// Rows scrolled off the quest list's top; lines off the overview's.
    pub scroll: (usize, usize),
    pub compact: bool,
}

/// What the quest list is built from, besides QuestUi (by its change ticks).
#[derive(Clone, Copy, PartialEq)]
pub(in crate::ui) struct QuestInputs {
    /// Net::ui_revision: the server's quest log, among the rest.
    revision: u64,
    /// The quest data, texts (and which), map names and item catalog loaded.
    loaded: (bool, Option<AssetId<dialogue::Catalog>>, bool, bool),
    class: u8,
    /// The list's and the overview's scroll (systems::State).
    scroll: (usize, usize),
}

/// The log from the network state; the windows are rebuilt when it changes.
/// CONTRACT: nothing per frame while nothing changed: the list (which the
///   always-shown tracker needs) is rebuilt only when QuestInputs or QuestUi
///   change (docs/ui.md, "Per-frame cost").
#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn sync(
    mut commands: Commands,
    net: Option<NonSendMut<crate::network::net::Net>>,
    mut ui: ResMut<QuestUi>,
    catalogs: Res<Assets<QuestCatalog>>,
    presentation: Res<dialogue::Presentation>,
    texts: Res<Assets<dialogue::Catalog>>,
    lookups: QuestLookups,
    mut quests: ResMut<Quests>,
    mut built: Local<(Option<QuestInputs>, u32)>,
    mut work: ResMut<crate::ui::UiWork>,
    mut loaded: ResMut<Loaded>,
    layouts: Res<Assets<Layout>>,
    windows: Query<(Entity, &Window2018)>,
) {
    let Some(mut net) = net else { return };
    if ui.cancel.is_some() {
        // The server answers with QuestUpdate state 3 (and the items taken back).
        let quest_id = ui.cancel.take().unwrap_or_default();
        if !net.offline { net.send(&ClientCommand::Quest(actors::Quest { quest_id, state: CANCEL, parameter: 0 })); }
    }
    let catalog_id = texts.get(presentation.catalog()).is_some().then(|| presentation.catalog().id());
    let data = catalogs.get(&ui.catalog);
    let items = lookups.item_data.as_ref().and_then(|d| lookups.item_catalogs.get(&d.catalog));
    let names = lookups.names.get(&loaded.map_names);
    let class = lookups.players.iter().next().map_or(0, |c| c.desc.class);
    let scroll = (lookups.state.scroll.get(LIST).copied().unwrap_or(0), lookups.state.scroll.get(OVERVIEW).copied().unwrap_or(0));
    let inputs = QuestInputs { revision: net.ui_revision, loaded: (data.is_some(), catalog_id, names.is_some(), items.is_some()), class, scroll };
    // Development builds (and DSOR_FLAGS=uiaudit) rebuild now and then anyway:
    // a list that changed means an input above was missed.
    built.1 = built.1.wrapping_add(1);
    let audit = (cfg!(debug_assertions) || crate::flag("uiaudit")) && built.1 % 60 == 0;
    let current = built.0 == Some(inputs) && !ui.is_changed();
    if current && !audit { return; }
    built.0 = Some(inputs);
    if !current { work.quest_lists += 1; }
    let fallback = dialogue::Catalog::default();
    let catalog = texts.get(presentation.catalog()).unwrap_or(&fallback);
    let class_name = crate::actors::character::CLASSES.get(class as usize).copied().unwrap_or("warrior");
    let mut list: Vec<_> = net.npc_dialogue.log.iter().filter(|(_, e)| e.active()).map(|(id, entry)| {
        let def = data.and_then(|d| d.quest(id));
        let tasks = def.map(|d| d.tasks.iter().enumerate().map(|(index, task)| {
            let text = catalog.text(&format!("quest.{id}.s_{}", task.title));
            TaskView {
                title: if text.is_empty() { dialogue::readable(&task.title) } else { text },
                progress: entry.task_progress(index),
                target: entry.targets.get(&index).copied().unwrap_or(task.target),
                done: entry.task_done(index),
            }
        }).collect()).unwrap_or_default();
        let description = [if entry.state == 1 { "complete" } else { "progress" }, "offer"].iter()
            .map(|field| catalog.quest(id, field)).find(|t| !t.is_empty()).unwrap_or_default();
        let map = def.map_or(String::new(), |d| d.map.clone());
        let zone = if map.is_empty() { String::new() } else { names.map_or_else(|| dialogue::readable(&map), |n| n.caption(&map)) };
        let item_templates: Vec<String> = def.map(|d| d.items.iter().filter(|i| i.class.is_empty() || i.class.eq_ignore_ascii_case(class_name)).take(4).map(|i| i.item.clone()).collect()).unwrap_or_default();
        let reward_items = def.map(|d| d.items.iter().filter(|i| i.class.is_empty() || i.class.eq_ignore_ascii_case(class_name)).take(4).map(|i| {
            let (image, background, frame) = items.map(|c| c.icon_parts(&i.item, None)).unwrap_or_default();
            (i.amount, Icon { background: background.unwrap_or_default().into(), image: image.unwrap_or_default().into(), frame: frame.unwrap_or_default().into(), ..Default::default() })
        }).collect()).unwrap_or_default();
        QuestView { id: id.clone(), title: catalog.title(id), level: def.map_or(0, |d| d.level), state: entry.state,
            kind: group(def.map_or("world", |d| d.kind.as_str())).to_owned(), zone, description,
            deletable: def.is_some_and(|d| d.deletable), tasks,
            rewards: def.map_or((0, 0, 0), |d| (d.rewards.xp, d.rewards.vc, d.rewards.rc)), items: reward_items, item_templates }
    }).collect();
    let order = |kind: &str| GROUPS.iter().position(|(k, _)| *k == kind).unwrap_or(GROUPS.len());
    list.sort_by(|a, b| order(&a.kind).cmp(&order(&b.kind)).then(a.level.cmp(&b.level)).then(a.title.cmp(&b.title)).then(a.id.cmp(&b.id)));
    let selected = ui.selected.as_ref().and_then(|s| list.iter().position(|q| &q.id == s)).unwrap_or(0);
    let tracked = ui.tracked.min(list.len().saturating_sub(1));
    if ui.tracked != tracked { ui.tracked = tracked; }
    let next = Quests { list, selected, tracked, folded: ui.folded.iter().cloned().collect(), scroll, compact: ui.compact };
    if *quests == next { return; }
    if current { error!("the quest list changed without an input change: a Net field it shows was edited without Net::touch_ui"); }
    // The tracker only depends on the followed quest and its mode.
    let tracker_changed = quests.list.get(quests.tracked) != next.list.get(next.tracked) || quests.compact != next.compact;
    let log_changed = quests.list != next.list || quests.selected != next.selected || quests.folded != next.folded || quests.scroll != next.scroll;
    *quests = next;
    // The log keeps its root (and whether it is open); the tracker is rebuilt whole.
    for (entity, window) in &windows {
        if window.0 == WINDOW && log_changed { commands.entity(entity).despawn_related::<Children>(); }
        if window.0 == TRACKER && tracker_changed { commands.entity(entity).despawn(); }
    }
    for i in 0..loaded.layouts.len() {
        if layouts.get(&loaded.layouts[i].0).is_some_and(|l| (l.window == WINDOW && log_changed) || (l.window == TRACKER && tracker_changed)) { loaded.spawned[i] = false; }
    }
}

/// What the quest views look up besides the quest data.
#[derive(bevy::ecs::system::SystemParam)]
pub(in crate::ui) struct QuestLookups<'w, 's> {
    item_data: Option<Res<'w, super::items::Data>>,
    item_catalogs: Res<'w, Assets<super::items::Catalog>>,
    names: Res<'w, Assets<local_map::MapNames>>,
    state: Res<'w, super::systems::State>,
    players: Query<'w, 's, &'static crate::actors::character::Character, With<crate::network::net::LocalPlayer>>,
}

/// Entries of a List, stacked from `top`: each a template's subtree placed at
/// the running y, until `bottom`. Returns the y after the last one placed.
struct Stack<'a> {
    source: &'a Layout,
    y: f32,
    bottom: f32,
    skip: usize,
    full: bool,
}

impl<'a> Stack<'a> {
    fn new(source: &'a Layout, top: f32, bottom: f32, skip: usize) -> Self { Self { source, y: top, bottom, skip, full: false } }

    /// The template's clone at the running y, if it still fits.
    fn push(&mut self, template: &str, replacement: &str) -> Option<Vec<Widget>> {
        let root = self.source.widgets.iter().find(|w| w.id == template)?;
        let height = root.rect[3] - root.rect[1];
        if self.skip > 0 { self.skip -= 1; return None; }
        if self.full || self.y + height > self.bottom + 0.5 { self.full = true; return None; }
        let rows: Vec<Widget> = clone_rows(self.source, template, replacement, self.y - root.rect[1]).map(|mut w| { w.hidden = w.hidden && is_alternative(&w); w }).collect();
        self.y += height;
        Some(rows)
    }
}

/// The parts a template keeps hidden whatever its data (its animations and
/// unused alternatives).
fn is_alternative(w: &Widget) -> bool {
    w.id.split('/').any(|p| p.starts_with("Frame") && p.len() > 5 && p[5..].chars().all(|c| c.is_ascii_digit()) && p != "Frame1")
}

/// The quest log: the grouped quest list and the open quest's overview.
pub(in crate::ui) fn prepare_log(source: &Layout, quests: &Quests) -> Layout {
    let list_prefix = format!("{LIST}/");
    let overview_prefix = format!("{OVERVIEW}/");
    let open = quests.list.get(quests.selected);
    let mut layout = without(source, |id| {
        (id.starts_with(&list_prefix) && is_template(&id[list_prefix.len()..]))
            || (id.starts_with(&overview_prefix) && (id.contains("/DescriptionList/Template") || id.contains("/RewardList1/")))
            || (id.ends_with("/RewardsHeading") && open.is_none_or(|q| q.rewards == (0, 0, 0) && q.items.is_empty()))
    });
    if let Some(w) = layout.widgets.iter_mut().find(|w| w.id.ends_with("/QuestOverview/QuestTitleLabel/QuestTitle")) {
        set_text(w, open.map_or("", |q| q.title.as_str()));
    }
    // The overview's half-transparent backdrop comes after its title, rewards
    // heading and scroll bar in the source: drawn there, it dimmed the title's
    // authored gold to grey. It goes under the whole overview.
    // FAILURE (2026-10-21): "the quest title ... is a bit dark".
    let backdrop = format!("{OVERVIEW}/DescriptionListBackground");
    let from = layout.widgets.iter().position(|w| w.id == backdrop);
    let to = layout.widgets.iter().position(|w| w.id.starts_with(&overview_prefix));
    if let (Some(from), Some(to)) = (from, to) {
        if to < from { let w = layout.widgets.remove(from); layout.widgets.insert(to, w); }
    }
    // The list: a header per group (its count; it folds the group), its rows.
    let mut stack = Stack::new(source, LIST_TOP, LIST_BOTTOM, quests.scroll.0);
    for (kind, title) in GROUPS {
        let members: Vec<(usize, &QuestView)> = quests.list.iter().enumerate().filter(|(_, q)| q.kind == kind).collect();
        if members.is_empty() { continue; }
        let folded = quests.folded.iter().any(|f| f == kind);
        let header = format!("{LIST}/Group_{kind}");
        if let Some(rows) = stack.push(&format!("{LIST}/TemplateQuesttypes"), &header) {
            for mut w in rows {
                if w.id == header { w.event = format!("QuestGroup:{kind}"); }
                if leaf(&w.id) == "TextLabel" { set_text(&mut w, title); }
                if leaf(&w.id) == "QuestCounter" { set_text(&mut w, &members.len().to_string()); }
                if !folded { selected(&mut w); }
                layout.widgets.push(w);
            }
        }
        if folded { continue; }
        for (index, quest) in members {
            let replacement = format!("{LIST}/Quest{index}");
            let Some(rows) = stack.push(&format!("{LIST}/TemplateQuest"), &replacement) else { continue };
            for mut w in rows {
                let name = leaf(&w.id).to_owned();
                if w.id.contains("/cancelBtn") {
                    if !quest.deletable { continue; }
                    if name == "cancelBtn" { w.event = format!("QuestCancel:{}", quest.id); }
                }
                if w.id == replacement { w.event = format!("QuestSelect:{}", quest.id); }
                super::template::rename(&mut w, "TemplateQuestmarkerIcon", "QuestmarkerIcon");
                match name.as_str() {
                    "QuestTitle" => set_text(&mut w, &quest.title),
                    "QuestZone" => set_text(&mut w, &quest.zone_line()),
                    "QuestLevel" => set_text(&mut w, &if quest.level > 0 { quest.level.to_string() } else { String::new() }),
                    _ => {}
                }
                if index == quests.selected { selected(&mut w); }
                layout.widgets.push(w);
            }
        }
    }
    place_handle(&mut layout, &format!("{LIST}/ScrollBarBackground/ScrollBarBoundaries"), quests.scroll.0, quests.entries());
    let Some(quest) = open else { return layout };
    // The overview: the text's lines, the objectives' heading, the objectives.
    let lines = wrap(&quest.description, TEXT_WIDTH, 40);
    let mut stack = Stack::new(source, TEXT_TOP, TEXT_BOTTOM, quests.scroll.1);
    let list = format!("{OVERVIEW}/DescriptionList");
    for (n, line) in lines.iter().enumerate() {
        for mut w in stack.push(&format!("{list}/TemplateText"), &format!("{list}/Line{n}")).unwrap_or_default() {
            if leaf(&w.id) == "Value" { set_text(&mut w, line); }
            layout.widgets.push(w);
        }
    }
    if !quest.tasks.is_empty() {
        for w in stack.push(&format!("{list}/TemplateTasksHeading"), &format!("{list}/TasksHeading")).unwrap_or_default() {
            layout.widgets.push(w);
        }
    }
    // An objective's box, then its lines (the further ones without the box).
    for (n, task) in quest.tasks.iter().enumerate() {
        for (k, line) in task.lines(TASK_WIDTH).iter().enumerate() {
            let name = if k == 0 { format!("{list}/Task{n}") } else { format!("{list}/Task{n}_{k}") };
            for mut w in stack.push(&format!("{list}/TemplateTask"), &name).unwrap_or_default() {
                if w.id.contains("/AnimatedManLabel") { continue; }
                match leaf(&w.id) {
                    "TaskIcon" if k > 0 => continue,
                    "TaskIcon" => task_icon(&mut w, task.done),
                    "TaskTitle" => {
                        set_text(&mut w, line);
                        if task.done { grey(&mut w); }
                    }
                    _ => {}
                }
                layout.widgets.push(w);
            }
        }
    }
    let entries = lines.len() + usize::from(!quest.tasks.is_empty()) + quest.tasks.len();
    place_handle(&mut layout, &format!("{list}/ScrollBarBackground1/ScrollBarBoundaries"), quests.scroll.1, entries);
    // The rewards: experience, money, items.
    let rewards = format!("{OVERVIEW}/RewardList1");
    let mut stack = Stack::new(source, REWARD_TOP, LIST_BOTTOM, 0);
    let (xp, vc, rc) = quest.rewards;
    if xp > 0 {
        for mut w in stack.push(&format!("{rewards}/TemplateXP"), &format!("{rewards}/Experience")).unwrap_or_default() {
            if leaf(&w.id) == "Value" { set_text(&mut w, &format!("{xp} Experience")); }
            w.hidden = false;
            layout.widgets.push(w);
        }
    }
    if vc > 0 || rc > 0 {
        let (gold, silver, copper) = (vc / 10_000, vc / 100 % 100, vc % 100);
        // The variant that shows exactly the coins there are.
        let variant = match (rc > 0, gold > 0, silver > 0) {
            (true, true, _) => "TemplateRCVC", (true, false, true) => "TemplateRCVC1", (true, false, false) => "TemplateRCVC2",
            (false, true, _) => "TemplateVC", (false, false, true) => "TemplateVC1", (false, false, false) => "TemplateVC2",
        };
        for mut w in stack.push(&format!("{rewards}/{variant}"), &format!("{rewards}/Money")).unwrap_or_default() {
            w.hidden = false;
            match leaf(&w.id) {
                "Value" => set_text(&mut w, &rc.to_string()),
                "Gold" => set_text(&mut w, &gold.to_string()),
                "Silver" => set_text(&mut w, &silver.to_string()),
                "Copper" => set_text(&mut w, &copper.to_string()),
                _ => {}
            }
            layout.widgets.push(w);
        }
    }
    if !quest.items.is_empty() {
        for mut w in stack.push(&format!("{rewards}/TemplateItemSlots"), &format!("{rewards}/Items")).unwrap_or_default() {
            let slot = w.id.split('/').find_map(|p| p.strip_prefix("ItemSlot")).and_then(|n| n.parse::<usize>().ok());
            if let Some(n) = slot {
                let Some((amount, icon)) = quest.items.get(n) else { continue };
                if leaf(&w.id) == "Icon0" {
                    layout.widgets.extend(icon_layers(&mut w, icon));
                    set_text(&mut w, &if *amount > 1 { amount.to_string() } else { String::new() });
                    // Hovered, the reward's card (tooltips, Action::ItemCard).
                    if let Some(template) = quest.item_templates.get(n) { super::template::make_button(&mut w, &format!("ItemCard:{amount}:{template}")); }
                }
            }
            w.hidden = false;
            layout.widgets.push(w);
        }
    }
    layout
}

/// A List's scroll handle placed along its bar for `scroll` of `total` entries.
pub(in crate::ui) fn place_handle(layout: &mut Layout, boundaries: &str, scroll: usize, total: usize) {
    let Some(bar) = layout.widgets.iter().find(|w| w.id == boundaries).map(|w| w.rect) else { return };
    let handle = format!("{boundaries}/ScrollHandle");
    let Some(rect) = layout.widgets.iter().find(|w| w.id == handle).map(|w| w.rect) else { return };
    let travel = (bar[3] - bar[1]) - (rect[3] - rect[1]);
    let fraction = if total <= 1 { 0.0 } else { (scroll as f32 / (total - 1) as f32).clamp(0.0, 1.0) };
    let dy = bar[1] + travel * fraction - rect[1];
    let prefix = format!("{handle}/");
    for w in layout.widgets.iter_mut().filter(|w| w.id == handle || w.id.starts_with(&prefix)) { shift(w, 0.0, dy); }
}

/// The y-stretch of a panel between `top` and `bottom` by `extra` (its edges and
/// background follow; what lies above `top` stays).
fn stretch(w: &mut Widget, top: f32, bottom: f32, extra: f32) {
    super::template::map_y(w, |y| y + if y <= top { 0.0 } else if y >= bottom { extra } else { extra * (y - top) / (bottom - top) });
}

/// The tracker: the followed quest's title and objectives in a frame fitted to
/// them, or the compact button; nothing without a quest.
pub(in crate::ui) fn prepare_tracker(source: &Layout, quests: &Quests) -> Layout {
    let mut layout = Layout { window: source.window.clone(), widgets: Vec::new() };
    let Some(quest) = quests.list.get(quests.tracked) else { return layout };
    let compact = quests.compact;
    // Each objective's lines, as far as the tracker shows.
    let mut rows: Vec<(usize, usize, String)> = Vec::new();
    for (n, task) in quest.tasks.iter().take(MOST_TRACKER_TASKS).enumerate() {
        for (k, line) in task.lines(TRACKER_TASK_WIDTH).into_iter().enumerate() { rows.push((n, k, line)); }
    }
    rows.truncate(MOST_TRACKER_LINES);
    let last = 138.0 + rows.len() as f32 * TRACKER_TASK_HEIGHT;
    let bottom = (last + 16.0).max(TRACKER_MIN_BOTTOM);
    let extra = bottom - TRACKER_FRAME_BOTTOM;
    for widget in &source.widgets {
        let id = &widget.id;
        if is_template(id) || id.contains("/AnchorQuestGuidance") { continue; }
        if (compact && id.contains("/LabelFull")) || (!compact && id.contains("/LabelCompact")) { continue; }
        // The objectives never outgrow the fitted frame: no scrolling.
        if leaf(id).starts_with("ScrollUp") || leaf(id).starts_with("ScrollDown") || id.contains("/ScrollUp/") || id.contains("/ScrollDown/") {
            if !(id.contains("Disabled")) { continue; }
        }
        let mut w = widget.clone();
        if id.starts_with(TRACKER_FRAME) { stretch(&mut w, 130.0, TRACKER_FRAME_BOTTOM, extra); }
        if id == &format!("{LABEL}/LabelFull") || id == LABEL { stretch(&mut w, 130.0, TRACKER_FRAME_BOTTOM, extra); }
        layout.widgets.push(w);
    }
    if compact { return layout; }
    for mut w in clone_rows(source, &format!("{TRACKER_FRAME}/QuestTitle/TemplateQuestTitleLine"), &format!("{TRACKER_FRAME}/QuestTitle/Line"), 0.0) {
        match leaf(&w.id) {
            "Title" => set_text(&mut w, &quest.title),
            "Location" => set_text(&mut w, &if quest.state == 1 { quest.zone_line() } else if quest.level > 0 { format!("Level {} - {}", quest.level, quest.zone) } else { quest.zone.clone() }),
            _ => {}
        }
        layout.widgets.push(w);
    }
    for (row, (n, k, line)) in rows.iter().enumerate() {
        let task = &quest.tasks[*n];
        let name = if *k == 0 { format!("{TRACKER_FRAME}/List/Task{n}") } else { format!("{TRACKER_FRAME}/List/Task{n}_{k}") };
        for mut w in clone_at(source, &format!("{TRACKER_FRAME}/List/TemplateTask"), &name, 0.0, row as f32 * TRACKER_TASK_HEIGHT) {
            super::template::rename(&mut w, "TemplateTaskLine", "TaskLine");
            if w.id.contains("/AnimatedManLabel") || w.id.contains("/TaskIconAnimatedParent") { continue; }
            match leaf(&w.id) {
                "TaskIcon" if *k > 0 => continue,
                "TaskIcon" => task_icon(&mut w, task.done),
                "TaskTitle" => {
                    set_text(&mut w, line);
                    if task.done { grey(&mut w); }
                }
                _ => {}
            }
            layout.widgets.push(w);
        }
    }
    let _ = make_button;
    layout
}

impl Quests {
    /// The quest list's entries: a header per group, the rows of the unfolded.
    pub(in crate::ui) fn entries(&self) -> usize {
        GROUPS.iter().map(|(kind, _)| {
            let n = self.list.iter().filter(|q| q.kind == *kind).count();
            if n == 0 { 0 } else if self.folded.iter().any(|f| f == kind) { 1 } else { n + 1 }
        }).sum()
    }
}

/// The folded groups changed by a header's click.
pub(in crate::ui) fn fold(folded: &mut HashSet<String>, kind: &str) {
    if !folded.remove(kind) { folded.insert(kind.to_owned()); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::template::fixture::{find, layout, text_of};

    /// docs/ui.md, "Per-frame cost": the list the tracker needs is rebuilt
    /// when the log, its selection or its scroll change, not every frame.
    #[test]
    fn the_quest_list_is_rebuilt_only_when_something_changed() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<QuestCatalog>().init_asset::<dialogue::Catalog>().init_asset::<super::super::items::Catalog>()
            .init_asset::<local_map::MapNames>().init_asset::<Layout>()
            .init_resource::<QuestUi>().init_resource::<dialogue::Presentation>().init_resource::<super::super::systems::State>()
            .init_resource::<Quests>().init_resource::<Loaded>().init_resource::<crate::ui::UiWork>()
            .add_systems(Update, sync);
        crate::network::net::start_offline(app.world_mut(), 1, 0);
        let built = |app: &App| app.world().resource::<crate::ui::UiWork>().quest_lists;
        app.update();
        assert_eq!(built(&app), 1);
        for _ in 0..10 { app.update(); }
        assert_eq!(built(&app), 1, "idle frames rebuild nothing");
        app.world_mut().non_send_mut::<crate::network::net::Net>().touch_ui();
        app.update();
        assert_eq!(built(&app), 2, "the server said something");
        app.world_mut().resource_mut::<QuestUi>().compact = true;
        app.update();
        assert_eq!(built(&app), 3, "the tracker was folded");
        app.world_mut().resource_mut::<super::super::systems::State>().scroll.insert(LIST.into(), 1);
        app.update();
        assert_eq!(built(&app), 4, "the list was scrolled");
        for _ in 0..10 { app.update(); }
        assert_eq!(built(&app), 4);
    }

    fn quests() -> Quests {
        Quests { list: vec![
            QuestView { id: "a".into(), title: "Alpha".into(), level: 3, state: 0, kind: "world".into(), zone: "Kingshill".into(), deletable: true, description: "one two three four".into(),
                tasks: vec![TaskView { title: "Kill rats".into(), progress: 2, target: 10, done: false }, TaskView { title: "Talk".into(), progress: 1, target: 1, done: true }],
                rewards: (193, 79, 3), items: vec![(2, Icon { image: "i.png".into(), ..default() })], ..default() },
            QuestView { id: "b".into(), title: "Beta".into(), level: 5, state: 1, kind: "local".into(), deletable: false, ..default() },
        ], selected: 1, tracked: 0, ..default() }
    }

    #[test]
    fn the_log_groups_quests_and_stacks_the_overview() {
        let l = format!("{LIST}/TemplateQuest");
        let o = format!("{OVERVIEW}/DescriptionList");
        let r = format!("{OVERVIEW}/RewardList1");
        let ids: Vec<(String, &str, bool)> = vec![
            (l.clone(), "Button", false), (format!("{l}/normal"), "Label", false), (format!("{l}/pressed"), "Label", false),
            (format!("{l}/Label_empty/Label_empty1/cancelBtn"), "TextButton", false), (format!("{l}/Label_empty/Label_empty1/QuestTitle"), "TextLabel", true),
            (format!("{l}/Label_empty/Label_empty1/QuestZone"), "TextLabel", true), (format!("{l}/Label_empty/Label_empty1/TemplateQuestmarkerIcon"), "Label", false),
            (format!("{LIST}/TemplateQuesttypes"), "Button", false), (format!("{LIST}/TemplateQuesttypes/normal/TextLabel"), "TextLabel", true),
            (format!("{LIST}/TemplateQuesttypes/Label_empty/Label_empty1/QuestCounter"), "TextLabel", true),
            (format!("{OVERVIEW}/QuestTitleLabel/QuestTitle"), "TextLabel", true), (format!("{OVERVIEW}/RewardsHeading"), "TextLabel", true),
            (format!("{OVERVIEW}/DescriptionListBackground"), "Label", false),
            (format!("{o}/TemplateText"), "Label", false), (format!("{o}/TemplateText/Value"), "TextLabel", true),
            (format!("{o}/TemplateTasksHeading"), "TextLabel", true), (format!("{o}/TemplateTask"), "Label", false), (format!("{o}/TemplateTask/TaskTitle"), "TextLabel", true),
            (format!("{r}/TemplateXP"), "Label", false), (format!("{r}/TemplateXP/Value"), "TextLabel", true),
            (format!("{r}/TemplateRCVC2"), "Label", false), (format!("{r}/TemplateRCVC2/Value"), "TextLabel", true), (format!("{r}/TemplateRCVC2/Copper"), "TextLabel", true),
            (format!("{r}/TemplateItemSlots"), "Label", false), (format!("{r}/TemplateItemSlots/ItemSlot0/Icon0"), "Icon", true), (format!("{r}/TemplateItemSlots/ItemSlot1/Icon0"), "Icon", true),
        ];
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let mut source = layout(WINDOW, &refs);
        // Templates 46 high (the fixture's rects are 192..238); the rewards' as authored.
        for w in &mut source.widgets {
            let (top, bottom) = if w.id.contains("/TemplateXP") || w.id.contains("/TemplateRCVC2") { (535.0, 554.0) } else if w.id.contains("/TemplateItemSlots") { (517.0, 572.0) } else { (192.0, 238.0) };
            w.rect[1] = top; w.rect[3] = bottom;
        }
        let mut q = quests();
        let prepared = prepare_log(&source, &q);
        let ids: Vec<_> = prepared.widgets.iter().map(|w| w.id.clone()).collect();
        assert!(!ids.iter().any(|id| is_template(id)));
        // Main quests' header, Alpha, then Side quests' header, Beta.
        let header = find(&prepared, &format!("{LIST}/Group_world"));
        assert_eq!((header.event.as_str(), header.rect[1]), ("QuestGroup:world", LIST_TOP));
        assert_eq!(text_of(&prepared, &format!("{LIST}/Group_world/normal/TextLabel")), "Main Quests");
        assert_eq!(text_of(&prepared, &format!("{LIST}/Group_local/Label_empty/Label_empty1/QuestCounter")), "1");
        let row = |n: usize| find(&prepared, &format!("{LIST}/Quest{n}"));
        assert_eq!((row(0).event.as_str(), row(0).rect[1]), ("QuestSelect:a", LIST_TOP + 46.0));
        assert_eq!(row(1).rect[1], LIST_TOP + 46.0 * 3.0);
        assert_eq!(text_of(&prepared, &format!("{LIST}/Quest0/Label_empty/Label_empty1/QuestZone")), "Kingshill");
        assert!(ids.contains(&format!("{LIST}/Quest0/Label_empty/Label_empty1/cancelBtn")) && !ids.contains(&format!("{LIST}/Quest1/Label_empty/Label_empty1/cancelBtn")));
        assert!(ids.contains(&format!("{LIST}/Quest0/Label_empty/Label_empty1/QuestmarkerIcon")));
        assert!(find(&prepared, &format!("{LIST}/Quest1/normal")).hidden);
        assert_eq!(text_of(&prepared, &format!("{OVERVIEW}/QuestTitleLabel/QuestTitle")), "Beta");
        assert!(!ids.iter().any(|id| id.ends_with("/RewardsHeading")));
        // The overview's backdrop is drawn under its title, not over it.
        let at = |id: &str| ids.iter().position(|i| *i == format!("{OVERVIEW}/{id}")).unwrap();
        assert!(at("DescriptionListBackground") < at("QuestTitleLabel/QuestTitle"));
        // Alpha open: its line, the heading, its tasks stacked; its rewards.
        q.selected = 0;
        let prepared = prepare_log(&source, &q);
        assert_eq!(find(&prepared, &format!("{o}/Line0")).rect[1], TEXT_TOP);
        assert_eq!(find(&prepared, &format!("{o}/TasksHeading")).rect[1], TEXT_TOP + 46.0);
        assert_eq!((text_of(&prepared, &format!("{o}/Task0/TaskTitle")), find(&prepared, &format!("{o}/Task0")).rect[1]), ("Kill rats (2/10)".to_owned(), TEXT_TOP + 92.0));
        assert_eq!(find(&prepared, &format!("{o}/Task1/TaskTitle")).text.as_ref().unwrap().color, [0.55, 0.55, 0.55, 1.0]);
        assert_eq!(text_of(&prepared, &format!("{r}/Experience/Value")), "193 Experience");
        assert_eq!((text_of(&prepared, &format!("{r}/Money/Value")), text_of(&prepared, &format!("{r}/Money/Copper"))), ("3".to_owned(), "79".to_owned()));
        assert!(prepared.widgets.iter().any(|w| w.id == format!("{r}/Items/ItemSlot0/Icon0/Image")) && !prepared.widgets.iter().any(|w| w.id.contains("/Items/ItemSlot1")));
        assert!(prepared.widgets.iter().any(|w| w.id.ends_with("/RewardsHeading")));
        // A folded group keeps its header only.
        q.folded = vec!["world".into()];
        let prepared = prepare_log(&source, &q);
        assert!(!prepared.widgets.iter().any(|w| w.id == format!("{LIST}/Quest0")));
        assert_eq!(find(&prepared, &format!("{LIST}/Group_local")).rect[1], LIST_TOP + 46.0);
        assert_eq!(wrap("aa bb cc dd", 5, 9), ["aa bb", "cc dd"]);
        assert_eq!(group("eventrepeatable"), "event");
        assert_eq!(group("rowrepeatable"), "local");
    }

    #[test]
    fn the_tracker_fits_its_frame_and_folds() {
        let t = format!("{TRACKER_FRAME}/QuestTitle/TemplateQuestTitleLine");
        let k = format!("{TRACKER_FRAME}/List/TemplateTask");
        let ids: Vec<(String, &str, bool)> = vec![
            (format!("{LABEL}/LabelFull"), "Label", false), (format!("{LABEL}/LabelCompact/MenuBackgroundLabel"), "Label", false), (format!("{TRACKER_FRAME}/bottomedge"), "Label", false),
            (t.clone(), "Label", false), (format!("{t}/Title"), "TextLabel", true), (format!("{t}/Location"), "TextLabel", true),
            (k.clone(), "Label", false), (format!("{k}/TemplateTaskLine/TaskTitle"), "TextLabel", true), (format!("{k}/TemplateTaskLine/TaskIcon"), "Label", false), (format!("{TRACKER_FRAME}/List/TemplateSeperator"), "Label", false),
            (format!("{LABEL}/LabelFull/MenuBackgroundLabel/ScrollUp"), "Button", false), (format!("{LABEL}/LabelFull/MenuBackgroundLabel/ScrollUpDisabled"), "Label", false),
        ];
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let mut source = layout(TRACKER, &refs);
        if let Some(w) = source.widgets.iter_mut().find(|w| w.id.ends_with("/bottomedge")) { w.rect = [734.0, 325.0, 1027.0, 327.0]; }
        let prepared = prepare_tracker(&source, &quests());
        let ids: Vec<_> = prepared.widgets.iter().map(|w| w.id.clone()).collect();
        assert!(!ids.iter().any(|id| id.contains("LabelCompact") || is_template(id) || id.ends_with("/ScrollUp")));
        assert!(ids.iter().any(|id| id.ends_with("/ScrollUpDisabled")));
        assert_eq!(text_of(&prepared, &format!("{TRACKER_FRAME}/QuestTitle/Line/Title")), "Alpha");
        assert_eq!(text_of(&prepared, &format!("{TRACKER_FRAME}/QuestTitle/Line/Location")), "Level 3 - Kingshill");
        let task = |n: usize| find(&prepared, &format!("{TRACKER_FRAME}/List/Task{n}/TaskLine/TaskTitle"));
        assert_eq!((task(0).text.as_ref().unwrap().text.as_str(), task(1).rect[1] - task(0).rect[1]), ("Kill rats (2/10)", TRACKER_TASK_HEIGHT));
        // Two objectives: the frame ends 16 under the second.
        assert_eq!(find(&prepared, &format!("{TRACKER_FRAME}/bottomedge")).rect[1], 138.0 + 2.0 * TRACKER_TASK_HEIGHT + 16.0);
        let icon = |n: usize| find(&prepared, &format!("{TRACKER_FRAME}/List/Task{n}/TaskLine/TaskIcon")).art.as_ref().unwrap().tex.clone();
        assert_eq!((icon(0).as_deref(), icon(1).as_deref()), (Some(TASK_OPEN), Some(TASK_DONE)));
        // A long objective takes further lines, without a box.
        let mut long = quests();
        long.list[0].tasks[0].title = "Find Georgie's son Gus near the farm and bring him home".into();
        let wrapped = prepare_tracker(&source, &long);
        assert_eq!(text_of(&wrapped, &format!("{TRACKER_FRAME}/List/Task0/TaskLine/TaskTitle")), "Find Georgie's son Gus near the");
        assert_eq!(text_of(&wrapped, &format!("{TRACKER_FRAME}/List/Task0_1/TaskLine/TaskTitle")), "farm and bring him home (2/10)");
        assert!(!wrapped.widgets.iter().any(|w| w.id == format!("{TRACKER_FRAME}/List/Task0_1/TaskLine/TaskIcon")));
        assert_eq!(find(&wrapped, &format!("{TRACKER_FRAME}/List/Task1/TaskLine/TaskTitle")).rect[1] - find(&wrapped, &format!("{TRACKER_FRAME}/List/Task0/TaskLine/TaskTitle")).rect[1], 2.0 * TRACKER_TASK_HEIGHT);
        let compact = prepare_tracker(&source, &Quests { compact: true, ..quests() });
        assert!(compact.widgets.iter().all(|w| !w.id.contains("LabelFull")) && compact.widgets.iter().any(|w| w.id.contains("LabelCompact")));
        assert!(prepare_tracker(&source, &Quests::default()).widgets.is_empty());
    }
}
