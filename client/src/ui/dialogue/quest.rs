//! The NPC's quest screen (questscreen2) beside the NPC screen's portrait, as
//! release261's UI::QuestWidget2 (questwidget2.cc) fills it: the speech bubble,
//! its buttons, a quest's tasks and its rewards, every part where the layout
//! puts it.
//!
//! EVIDENCE (dro_client64, release261):
//!   - Load (0x1404D219C): the backdrop, the bubble (QuestLabel: HeadlineLabel,
//!     the TextLabel line), three button sets (ButtonSetAccept, ButtonSetBack,
//!     ButtonSetBackAccept), the tasks (TaskLabel: its List, the TaskItemLabel
//!     line, SeparatorTasks) and the rewards (RewardLabel: RewardsLabel,
//!     TemplateXP, TemplateVC/VC1/VC2, TemplateRCVC/RCVC1/RCVC2, ItemSlots). The
//!     buttons carry icons, no caption: Accept's tooltip names what it does
//!     (gui.window.questscreen OpenQuest, CloseQuest, Dialog, Barter,
//!     HoldGround), Back's is Back.
//!   - The mode (0x1404D57DC): a quest in progress has Back alone
//!     (ButtonSetBack); an offer, a completion and a conversation Back and
//!     Accept (ButtonSetBackAccept). ButtonSetAccept is never shown.
//!   - Refresh (0x1404D3844): a quest's title is the headline (a conversation
//!     has none); the text, in the player's gender, is wrapped to the TextLabel's
//!     width into line clones, at least three, stacked by its height
//!     (0x1404D0E00). The frame grows round them; the button set goes down with
//!     the last line. A quest also shows its tasks and rewards.
//!   - The tasks: each shown task's text, its {0:i} the task's amount, wrapped
//!     to the TaskItemLabel's width, a TaskItemLabel clone a line stacked from
//!     the List's top, then a SeparatorTasks. The check mark is on the first
//!     line only: as authored for an offer, else empty or ticked by the quest
//!     log's task bits. With stagedTasks a task waits for its preTasks
//!     (0x14021DEA8).
//!   - The rewards (CreateRewardElement 0x1404CF838), stacked from the panel's
//!     top: the headline; the money (Andermant on TemplateRCVC*, else gold on
//!     TemplateVC*, the variant by the largest coin: 0x1405EAD28, 0x1405EB9BC);
//!     the experience (xpString, or dynamicXpString for dynamicXP: that percent
//!     of the level, 0x1409A359C); the items, four to an ItemSlots row, unused
//!     slots hidden. A reward for another class is left out.
//!   - The headline (DrasaLocale 0x1405AD574): "[INTERNAL] " for a quest not
//!     live, its title, then " (n/m)", its place in its series, for the types
//!     LoadQuestsFromXML numbers 4 to 11 but pvp (tools/quest_catalog.py).
//! SOURCE: a 2026 capture of Thabo's "Full Moon Rising (1/5)" offer
//!   (2026-10-24) shows every part at its authored place, as above.
//! INFERRED: words tagged <QuestHighlightStart> or <cAARRGGBB> are drawn white
//!   (that capture; the tags' style is not in the client's data).
//! DEVIATION: the original fades the bubble in over 0.2 s, the buttons 0.1 s
//!   later, the tasks and rewards 0.3 s later; here they show at once. A task
//!   list longer than the List is cut, not scrolled. The experience is not
//!   multiplied by the player's quest bonus.
use super::*;
use crate::gameplay::quests::{QuestCatalog, QuestEntry};
use controls::template::{icon_layers, Icon};

const ROOT: &str = "Canvas/AlignmentLabel";
const BACKDROP: &str = "Canvas/AlignmentLabel/BackgroundLabel";
const BUBBLE: &str = "Canvas/AlignmentLabel/QuestLabel";
const HEADLINE: &str = "Canvas/AlignmentLabel/QuestLabel/HeadlineLabel";
const LINE: &str = "Canvas/AlignmentLabel/QuestLabel/TextLabel";
const EDGE: &str = "Canvas/AlignmentLabel/QuestLabel/leftedge";
const TASKS: &str = "Canvas/AlignmentLabel/TaskLabel";
const LIST: &str = "Canvas/AlignmentLabel/TaskLabel/List";
const TASKS_HEADLINE: &str = "Canvas/AlignmentLabel/TaskLabel/List/TasksLabel";
const TASK_LINE: &str = "Canvas/AlignmentLabel/TaskLabel/TaskItemLabel";
const SEPARATOR: &str = "Canvas/AlignmentLabel/TaskLabel/List/SeparatorTasks";
const REWARDS: &str = "Canvas/AlignmentLabel/RewardLabel";
const CHECK_EMPTY: &str = "textures/ui/icon_checkmark_empty.png";
const CHECK_DONE: &str = "textures/ui/icon_checkmark.png";
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// A row of reward items (ItemSlot0..3).
const SLOTS: usize = 4;

/// Text in one colour; None, the label's own.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Run {
    pub text: String,
    pub color: Option<[f32; 4]>,
}

/// A task's check mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Check { Authored, Open, Done }

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Task {
    pub text: Vec<Run>,
    pub check: Check,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub(super) struct Rewards {
    /// Gold in copper.
    pub copper: u32,
    pub andermant: u32,
    /// The experience line; empty for none.
    pub xp: String,
    /// The items: amount, icon.
    pub items: Vec<(u32, Icon)>,
    /// Their templates, in `items`' order (their hover cards).
    pub templates: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Screen {
    /// The headline: a quest's title; empty for a conversation.
    pub title: String,
    pub text: Vec<Run>,
    /// The options the buttons send: Accept (None: Back alone) and Back.
    pub accept: Option<usize>,
    pub back: usize,
    /// A quest's tasks and rewards; None for a conversation.
    pub quest: Option<(Vec<Task>, Rewards)>,
}

/// What a screen is made from besides the NPC's options.
pub(super) struct Facts<'a> {
    pub catalog: &'a Catalog,
    pub quests: Option<&'a QuestCatalog>,
    pub items: Option<&'a controls::items::Catalog>,
    /// Our log's entry for the quest.
    pub entry: Option<&'a QuestEntry>,
    pub female: bool,
    pub name: &'a str,
    /// Our class (crate::actors::character::CLASSES).
    pub class: &'a str,
    pub level: Option<u32>,
    /// (total, the level's floor, the next level's floor).
    pub xp: Option<(u32, u32, u32)>,
}

/// Nebula's inline tags as coloured runs: <QuestHighlightStart> and
/// <cAARRGGBB> open a colour, </> closes it; other tags (<i>) change nothing.
pub(super) fn runs(raw: &str) -> Vec<Run> {
    let mut result: Vec<Run> = Vec::new();
    let mut stack: Vec<Option<[f32; 4]>> = vec![None];
    let mut text = String::new();
    let mut chars = raw.chars();
    let flush = |text: &mut String, color: Option<[f32; 4]>, result: &mut Vec<Run>| {
        if text.is_empty() { return; }
        match result.last_mut() {
            Some(last) if last.color == color => last.text.push_str(text),
            _ => result.push(Run { text: text.clone(), color }),
        }
        text.clear();
    };
    while let Some(c) = chars.next() {
        if c != '<' { text.push(c); continue; }
        let tag: String = chars.by_ref().take_while(|c| *c != '>').collect();
        flush(&mut text, *stack.last().unwrap(), &mut result);
        if tag.starts_with('/') {
            if stack.len() > 1 { stack.pop(); }
            continue;
        }
        let argb = tag.strip_prefix('c').filter(|h| h.len() == 8).and_then(|h| u32::from_str_radix(h, 16).ok());
        let color = match (tag.as_str(), argb) {
            ("QuestHighlightStart", _) => Some(WHITE),
            (_, Some(v)) => Some([(v >> 16 & 0xFF) as f32 / 255.0, (v >> 8 & 0xFF) as f32 / 255.0, (v & 0xFF) as f32 / 255.0, (v >> 24) as f32 / 255.0]),
            _ => *stack.last().unwrap(),
        };
        stack.push(color);
    }
    flush(&mut text, *stack.last().unwrap(), &mut result);
    result
}

/// The text's placeholders: `{n:i...}` the amount (when given), `{n:s...}`
/// the player's name.
pub(super) fn fill(text: &str, amount: Option<u32>, name: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else { break };
        let token = &rest[open + 1..open + close];
        match (token.split(':').nth(1), amount) {
            (Some("i"), Some(amount)) => out.push_str(&amount.to_string()),
            (Some("s"), _) => out.push_str(name),
            _ => out.push_str(&rest[open..=open + close]),
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

/// One word, in the pieces of its colours.
#[derive(Default)]
struct Word {
    pieces: Vec<(String, Option<[f32; 4]>)>,
    /// Line breaks before it.
    breaks: usize,
}

impl Word {
    fn text(&self) -> String { self.pieces.iter().map(|(t, _)| t.as_str()).collect() }
}

/// The runs laid into lines no wider than `width` at the font size `size`
/// (reference pixels, Tahoma's advances: as drawn); a line is its runs, a run
/// followed by another keeping the space between them. Line breaks stay.
pub(super) fn wrap(runs: &[Run], width: f32, size: f32) -> Vec<Vec<Run>> {
    let mut words: Vec<Word> = Vec::new();
    let mut current: Option<Word> = None;
    let mut breaks = 0;
    for run in runs {
        for c in run.text.chars() {
            if c.is_whitespace() {
                words.extend(current.take());
                if c == '\n' { breaks += 1; }
                continue;
            }
            let word = current.get_or_insert_with(|| Word { breaks: std::mem::take(&mut breaks), ..default() });
            match word.pieces.last_mut() {
                Some((text, color)) if *color == run.color => text.push(c),
                _ => word.pieces.push((c.to_string(), run.color)),
            }
        }
    }
    words.extend(current);
    let mut lines: Vec<Vec<Word>> = Vec::new();
    let mut line: Vec<Word> = Vec::new();
    for word in words {
        if word.breaks > 0 && !(line.is_empty() && lines.is_empty()) {
            lines.push(std::mem::take(&mut line));
            for _ in 1..word.breaks { lines.push(Vec::new()); }
        }
        if !line.is_empty() {
            let wider = line.iter().map(Word::text).chain(std::iter::once(word.text())).collect::<Vec<_>>().join(" ");
            if crate::network::chat::text_width(&wider, size) > width { lines.push(std::mem::take(&mut line)); }
        }
        line.push(word);
    }
    if !line.is_empty() { lines.push(line); }
    lines.into_iter().map(|words| {
        let mut runs: Vec<Run> = Vec::new();
        for (n, word) in words.into_iter().enumerate() {
            if n > 0 { if let Some(last) = runs.last_mut() { last.text.push(' '); } }
            for (text, color) in word.pieces {
                match runs.last_mut() {
                    Some(last) if last.color == color => last.text.push_str(&text),
                    _ => runs.push(Run { text, color }),
                }
            }
        }
        runs
    }).collect()
}

/// gui.window.questscreen's text `name`, else the English one.
fn screen_text(catalog: &Catalog, name: &str, english: &str) -> String {
    let text = catalog.text(&format!("gui.window.questscreen_{name}"));
    if text.is_empty() { english.to_owned() } else { text }
}

/// Accept's tooltip for a quest's offer (true) or its completion.
pub(super) fn accept_label(catalog: &Catalog, offer: bool) -> String {
    if offer { screen_text(catalog, "openquest", "Accept quest") } else { screen_text(catalog, "closequest", "Complete quest") }
}
pub(super) fn talk_label(catalog: &Catalog) -> String { screen_text(catalog, "dialog", "End conversation") }
pub(super) fn back_label(catalog: &Catalog) -> String { screen_text(catalog, "back", "Back") }

/// A quest's screen: `field` its offer, progress or complete text.
pub(super) fn quest(id: &str, field: &str, facts: &Facts, accept: Option<usize>, back: usize) -> Screen {
    let catalog = facts.catalog;
    let text = fill(&catalog.spoken(&format!("quest.{id}.s_{field}"), facts.female), None, facts.name);
    let def = facts.quests.and_then(|q| q.quest(id));
    let offer = field == "offer";
    let done = |index: usize| facts.entry.is_some_and(|e| e.task_done(index));
    let tasks = def.map(|d| d.tasks.iter().enumerate().filter(|(index, task)| {
        !d.staged || done(*index) || match facts.entry {
            Some(_) => task.pre.iter().all(|p| done(*p)),
            None => task.pre.is_empty(),
        }
    }).map(|(index, task)| Task {
        text: runs(&fill(&catalog.spoken(&format!("quest.{id}.s_{}", task.title), facts.female), Some(task.target), facts.name)),
        check: if offer { Check::Authored } else if done(index) { Check::Done } else { Check::Open },
    }).collect()).unwrap_or_default();
    let mut rewards = Rewards::default();
    if let Some(d) = def {
        rewards.copper = d.rewards.vc;
        rewards.andermant = d.rewards.rc;
        // CalculateDynamicXPReward: the percent of the level's span, rounded; none at the last level.
        let dynamic = (d.rewards.dynamic_xp > 0).then(|| match (facts.xp, facts.level) {
            (_, Some(level)) if level >= 100 => 0,
            (Some((_, floor, next)), _) => (d.rewards.dynamic_xp as f32 / 100.0 * next.saturating_sub(floor) as f32 + 0.5).floor() as u32,
            _ => 0,
        });
        rewards.xp = match dynamic {
            Some(0) => String::new(),
            Some(xp) => fill(&screen_text(catalog, "dynamicxpstring", "XP (level dependent): {0:i}"), Some(xp), ""),
            None if d.rewards.xp > 0 => fill(&screen_text(catalog, "xpstring", "XP: {0:i}"), Some(d.rewards.xp), ""),
            None => String::new(),
        };
        rewards.templates = d.items.iter().filter(|i| i.class.is_empty() || i.class.eq_ignore_ascii_case(facts.class)).map(|i| i.item.clone()).collect();
        rewards.items = d.items.iter().filter(|i| i.class.is_empty() || i.class.eq_ignore_ascii_case(facts.class)).map(|i| {
            let (image, background, frame) = facts.items.map(|c| c.reward_icon_parts(&i.item, &i.rarity)).unwrap_or_default();
            let part = |p: Option<&str>| p.unwrap_or_default().to_owned();
            (i.amount, Icon { background: part(background), image: part(image), frame: part(frame), ..Default::default() })
        }).collect();
    }
    // The headline (DrasaLocale 0x1405AD574): "[INTERNAL] " for a quest not
    // live, the title, its place in its series.
    let prefix = if def.is_some_and(|d| d.internal) { "[INTERNAL] " } else { "" };
    let part = def.and_then(|d| d.part).filter(|[n, of]| *n > 0 && *of > 0).map_or(String::new(), |[n, of]| format!(" ({n}/{of})"));
    Screen { title: format!("{prefix}{}{part}", catalog.title(id)), text: runs(&text), accept, back, quest: Some((tasks, rewards)) }
}

/// A conversation's screen (quest.<dialog>): no headline, tasks or rewards.
pub(super) fn conversation(dialog: &str, facts: &Facts, accept: usize, back: usize) -> Screen {
    let text = fill(&facts.catalog.spoken(&format!("quest.{dialog}"), facts.female), None, facts.name);
    Screen { title: String::new(), text: runs(&text), accept: Some(accept), back, quest: None }
}

fn subtree<'a>(source: &'a Layout, root: &'a str) -> impl Iterator<Item = &'a Widget> + 'a {
    source.widgets.iter().filter(move |w| w.id == root || w.id.strip_prefix(root).is_some_and(|rest| rest.starts_with('/')))
}

fn find<'a>(source: &'a Layout, id: &str) -> Option<&'a Widget> { source.widgets.iter().find(|w| w.id == id) }

/// Artwork no one sees (the reward panel's frame, the task separators, the
/// lists' containers): left out, or it would take the pointer.
fn unseen(w: &Widget) -> bool {
    w.text.is_none() && w.art.as_ref().is_some_and(|a| a.color.chunks(4).all(|c| c.get(3).is_some_and(|alpha| *alpha <= 0.0)))
}

fn height(w: &Widget) -> f32 { w.rect[3] - w.rect[1] }

/// A clone of `w` renamed (its first `from` to `to`), moved down by `dy`, shown.
fn placed(w: &Widget, from: &str, to: &str, dy: f32) -> Widget {
    let mut w = w.clone();
    w.id = w.id.replacen(from, to, 1);
    w.button = w.button.map(|b| b.replacen(from, to, 1));
    map_widget(&mut w, |x| x, |y| y + dy);
    w.hidden = false;
    w
}

/// A line of runs on clones of the text label `w`: one a run, each starting
/// where the one before ends.
fn line_runs(w: &Widget, line: &[Run], id: &str, layout: &mut Layout) {
    let Some(label) = &w.text else { return };
    let size = label.size * FONT_SCALE;
    let mut x = w.rect[0];
    for (n, run) in line.iter().enumerate() {
        let mut clone = w.clone();
        clone.id = format!("{id}{n}");
        clone.rect[0] = x;
        set_text(&mut clone, &run.text);
        if let Some(text) = &mut clone.text {
            text.word_break = false;
            if let Some(color) = run.color { text.color = color; }
        }
        clone.hidden = false;
        x += crate::network::chat::text_width(&run.text, size);
        layout.widgets.push(clone);
    }
}

pub(super) fn prepare(source: &Layout, screen: &Screen, layout: &mut Layout) {
    // The backdrop shades the world under the conversation, drawn first.
    layout.widgets.extend(find(source, BACKDROP).cloned());
    // The bubble: the headline, the text's lines, the frame grown round them.
    let (Some(line), Some(edge)) = (find(source, LINE), find(source, EDGE)) else { return };
    let size = line.text.as_ref().map_or(9.5, |t| t.size) * FONT_SCALE;
    let lines = wrap(&screen.text, line.rect[2] - line.rect[0], size);
    let step = height(line);
    let extra = step * (lines.len().max(3) - 1) as f32;
    for w in subtree(source, BUBBLE) {
        if w.id == LINE { continue; }
        if w.id == HEADLINE {
            if screen.title.is_empty() { continue; }
            let mut w = w.clone();
            set_text(&mut w, &screen.title);
            layout.widgets.push(w);
            continue;
        }
        let mut w = w.clone();
        map_widget(&mut w, |x| x, |y| stretch(y, edge.rect[1], edge.rect[3], extra));
        layout.widgets.push(w);
    }
    for (n, runs) in lines.iter().enumerate() {
        line_runs(&placed(line, LINE, &format!("{LINE}{n}_"), step * n as f32), runs, &format!("{LINE}{n}_"), layout);
    }
    // The buttons under the last line.
    let set = format!("{ROOT}/{}", if screen.accept.is_some() { "ButtonSetBackAccept" } else { "ButtonSetBack" });
    let buttons = format!("{set}/");
    for w in source.widgets.iter().filter(|w| w.id.starts_with(&buttons)) {
        let mut w = placed(w, &set, &set, extra);
        if w.id == format!("{set}/Back") { w.event = format!("NpcChoice:{}", screen.back); }
        if let (true, Some(accept)) = (w.id == format!("{set}/Accept"), screen.accept) { w.event = format!("NpcChoice:{accept}"); }
        layout.widgets.push(w);
    }
    let Some((tasks, rewards)) = &screen.quest else { return };
    // The tasks, a line a row from the List's top.
    layout.widgets.extend(subtree(source, TASKS_HEADLINE).filter(|w| !unseen(w)).cloned());
    if let (Some(list), Some(row), Some(text)) = (find(source, LIST), find(source, TASK_LINE), find(source, &format!("{TASK_LINE}/TextLabel"))) {
        let width = text.rect[2] - text.rect[0];
        let size = text.text.as_ref().map_or(9.5, |t| t.size) * FONT_SCALE;
        let gap = find(source, SEPARATOR).map_or(0.0, height);
        let mut y = list.rect[1];
        'tasks: for (t, task) in tasks.iter().enumerate() {
            for (n, runs) in wrap(&task.text, width, size).iter().enumerate() {
                if y + height(row) > list.rect[3] + 0.5 { break 'tasks; }
                let name = format!("{TASKS}/Task{t}_{n}");
                let dy = y - row.rect[1];
                for w in subtree(source, TASK_LINE) {
                    let w = placed(w, TASK_LINE, &name, dy);
                    match w.id.rsplit('/').next().unwrap_or("") {
                        "TextLabel" => line_runs(&w, runs, &format!("{name}/TextLabel"), layout),
                        "CheckMarkLabel" if n == 0 => {
                            let mut w = w;
                            let texture = match task.check { Check::Authored => None, Check::Open => Some(CHECK_EMPTY), Check::Done => Some(CHECK_DONE) };
                            if let (Some(texture), Some(art)) = (texture, &mut w.art) { art.tex = Some(texture.to_owned()); }
                            layout.widgets.push(w);
                        }
                        "CheckMarkLabel" => {}
                        _ if !unseen(&w) => layout.widgets.push(w),
                        _ => {}
                    }
                }
                y += height(row);
            }
            y += gap;
        }
    }
    // The rewards, stacked down the panel.
    let mut y = 0.0;
    let mut stack = |template: &str, name: &str, layout: &mut Layout, fill: &dyn Fn(&mut Widget, &mut Layout) -> bool| {
        let root = format!("{REWARDS}/{template}");
        let Some(top) = find(source, &root) else { return };
        let to = format!("{REWARDS}/{name}");
        for w in subtree(source, &root) {
            let mut w = placed(w, &root, &to, y);
            if !unseen(&w) && fill(&mut w, layout) { layout.widgets.push(w); }
        }
        y += height(top);
    };
    stack("RewardsLabel", "Headline", layout, &|_, _| true);
    let (gold, silver) = (rewards.copper / 10_000, rewards.copper / 100 % 100);
    let money = match (rewards.andermant > 0, rewards.copper > 0, gold > 0, silver > 0) {
        (true, false, ..) => Some("TemplateRCVC2"),
        (true, _, true, _) => Some("TemplateRCVC"),
        (true, _, false, true) => Some("TemplateRCVC1"),
        (true, ..) => Some("TemplateRCVC2"),
        (false, true, true, _) => Some("TemplateVC"),
        (false, true, false, true) => Some("TemplateVC1"),
        (false, true, ..) => Some("TemplateVC2"),
        (false, false, ..) => None,
    };
    if let Some(variant) = money {
        stack(variant, "Money", layout, &|w, _| {
            let value = match w.id.rsplit('/').next().unwrap_or("") {
                "Value" => rewards.andermant, "Gold" => gold, "Silver" => silver, "Copper" => rewards.copper % 100, _ => return true,
            };
            set_text(w, &value.to_string());
            true
        });
    }
    if !rewards.xp.is_empty() {
        stack("TemplateXP", "Experience", layout, &|w, _| {
            if w.id.ends_with("/Value") { set_text(w, &rewards.xp); }
            true
        });
    }
    for (row, items) in rewards.items.chunks(SLOTS).enumerate() {
        stack("ItemSlots", &format!("Items{row}"), layout, &|w, layout| {
            let Some(slot) = w.id.split('/').find_map(|p| p.strip_prefix("ItemSlot")).and_then(|n| n.parse::<usize>().ok()) else { return true };
            let Some((amount, icon)) = items.get(slot) else { return false };
            if w.id.ends_with("/Icon0") {
                layout.widgets.extend(icon_layers(w, icon));
                set_text(w, &if *amount > 1 { amount.to_string() } else { String::new() });
                if let Some(template) = rewards.templates.get(row * SLOTS + slot) { controls::template::make_button(w, &format!("ItemCard:{amount}:{template}")); }
            }
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(line: &[Run]) -> String { line.iter().map(|r| r.text.as_str()).collect() }

    /// Highlights and colour tags make white runs; unknown tags change nothing.
    #[test]
    fn tags_colour_their_words() {
        let parsed = runs("Ever heard of <QuestHighlightStart>Varholm</> before? <i>Quiet</> <cFF00FF00>green</>.");
        assert_eq!(parsed.iter().map(|r| (r.text.as_str(), r.color)).collect::<Vec<_>>(), [
            ("Ever heard of ", None), ("Varholm", Some(WHITE)), (" before? Quiet ", None), ("green", Some([0.0, 1.0, 0.0, 1.0])), (".", None)]);
        assert_eq!(fill("Gather {0:i:amount} realm fragments, {0:s:name}! {1:t}", Some(10), "Rha"), "Gather 10 realm fragments, Rha! {1:t}");
        assert_eq!(fill("Hello {0:s}, {0:i}", None, "Rha"), "Hello Rha, {0:i}");
    }

    /// Lines keep within the width as Tahoma measures them; a coloured word
    /// glued to punctuation stays one word; breaks stay.
    #[test]
    fn lines_wrap_to_the_width_and_keep_colours() {
        let size = 9.5 * FONT_SCALE;
        let text = runs("Ever heard of da isle of <QuestHighlightStart>Varholm</>? It's full of adventure, dat place. Dere are dark secrets, treasure, wild creatures, sticky situations and a beautiful woman.");
        let lines = wrap(&text, 300.0, size);
        assert!(lines.len() > 1);
        for line in &lines { assert!(crate::network::chat::text_width(plain(line).trim_end(), size) <= 300.0, "{}", plain(line)); }
        let joined: Vec<String> = lines.iter().map(|l| plain(l).trim_end().to_owned()).collect();
        assert_eq!(joined.join(" "), "Ever heard of da isle of Varholm? It's full of adventure, dat place. Dere are dark secrets, treasure, wild creatures, sticky situations and a beautiful woman.");
        let highlighted = lines.iter().flatten().find(|r| r.color == Some(WHITE)).unwrap();
        assert_eq!(highlighted.text, "Varholm");
        assert_eq!(wrap(&runs("One\n\nTwo"), 300.0, size).iter().map(|l| plain(l)).collect::<Vec<_>>(), ["One", "", "Two"]);
    }

    fn widget(id: &str, rect: [f32; 4], text: Option<f32>, alpha: f32) -> serde_json::Value {
        let [l, t, r, b] = rect;
        serde_json::json!({ "id": id, "type": if id.ends_with("/Back") || id.ends_with("/Accept") { "Button" } else { "Label" },
            "event": if id.ends_with("/Back") { "Back" } else if id.ends_with("/Accept") { "Accept" } else { "" },
            "k": 0.0, "rect": rect, "hidden": id.contains("ButtonSetBack") || id.contains("VC1") || id.contains("RCVC"),
            "art": if text.is_some() { serde_json::Value::Null } else { serde_json::json!({"tex": "textures/ui/npcscreen.png", "pos": [l, t, r, t, r, b, l, b], "uv": vec![0.0f32; 8], "color": [1.0, 1.0, 1.0, alpha, 1.0, 1.0, 1.0, alpha, 1.0, 1.0, 1.0, alpha, 1.0, 1.0, 1.0, alpha], "idx": [0, 1, 2, 0, 2, 3]}) },
            "text": text.map(|size| serde_json::json!({"text": "", "key": "", "size": size, "color": [1.0, 0.83, 0.52, 1.0], "align": [0, 1]})) })
    }

    /// questscreen2's parts at their release261 places.
    fn layout() -> Layout {
        let widgets = vec![
            widget(BACKDROP, [0.0, -1.0, 2597.0, 1813.0], None, 0.5),
            widget(BUBBLE, [260.06, 65.8, 772.98, 118.99], None, 1.0),
            widget(HEADLINE, [260.06, 65.8, 772.98, 118.99], Some(12.5), 1.0),
            widget(LINE, [260.06, 92.84, 772.98, 113.6], Some(9.5), 1.0),
            widget(&format!("{BUBBLE}/topedge"), [269.0, 53.0, 764.0, 66.0], None, 1.0),
            widget(EDGE, [218.0, 104.0, 260.0, 107.0], None, 1.0),
            widget(&format!("{BUBBLE}/bottomleftcorner"), [218.0, 107.0, 269.0, 158.0], None, 1.0),
            widget(&format!("{ROOT}/ButtonSetAccept/Accept"), [244.0, 134.0, 344.0, 173.0], None, 1.0),
            widget(&format!("{ROOT}/ButtonSetBack/Back"), [244.0, 134.0, 344.0, 173.0], None, 1.0),
            widget(&format!("{ROOT}/ButtonSetBackAccept/Back"), [244.0, 134.0, 344.0, 173.0], None, 1.0),
            widget(&format!("{ROOT}/ButtonSetBackAccept/Back/normal/icon"), [281.0, 141.0, 307.0, 165.0], None, 1.0),
            widget(&format!("{ROOT}/ButtonSetBackAccept/Accept"), [346.0, 134.0, 446.0, 173.0], None, 1.0),
            widget(TASKS, [119.0, 369.0, 510.0, 502.0], None, 0.0),
            widget(TASK_LINE, [30.0, 381.62, 421.0, 400.77], None, 0.0),
            widget(&format!("{TASK_LINE}/TextLabel"), [74.18, 381.62, 417.45, 400.77], Some(9.5), 1.0),
            widget(&format!("{TASK_LINE}/CheckMarkLabel"), [30.0, 370.0, 72.0, 412.0], None, 1.0),
            widget(LIST, [30.0, 370.9, 421.0, 656.0], None, 0.0),
            widget(&format!("{TASKS_HEADLINE}/TaskHeadlineLabel"), [39.36, 329.84, 608.27, 354.18], Some(10.0), 1.0),
            widget(SEPARATOR, [30.0, 370.0, 421.0, 377.0], None, 0.0),
            widget(&format!("{LIST}/ScrollUp"), [8.0, 337.0, 31.0, 361.0], None, 1.0),
            widget(REWARDS, [799.0, 54.0, 1368.0, 278.0], None, 0.0),
            widget(&format!("{REWARDS}/RewardsLabel"), [799.0, 54.16, 1368.0, 87.0], None, 0.0),
            widget(&format!("{REWARDS}/RewardsLabel/RewardHeadlineLabel"), [799.62, 54.16, 1368.07, 78.51], Some(10.0), 1.0),
            widget(&format!("{REWARDS}/TemplateVC"), [799.0, 54.16, 1368.0, 73.0], None, 1.0),
            widget(&format!("{REWARDS}/TemplateVC/Gold"), [824.94, 54.76, 852.23, 72.53], Some(9.5), 1.0),
            widget(&format!("{REWARDS}/TemplateVC/Copper"), [940.28, 54.76, 967.63, 72.54], Some(9.5), 1.0),
            widget(&format!("{REWARDS}/TemplateRCVC2"), [799.0, 54.16, 1368.0, 73.0], None, 1.0),
            widget(&format!("{REWARDS}/TemplateXP"), [799.0, 54.16, 1368.0, 80.0], None, 0.0),
            widget(&format!("{REWARDS}/TemplateXP/Value"), [799.39, 59.3, 1163.4, 74.48], Some(9.5), 1.0),
            widget(&format!("{REWARDS}/ItemSlots"), [799.0, 58.0, 1368.0, 114.0], None, 0.0),
            widget(&format!("{REWARDS}/ItemSlots/ItemSlot0"), [799.0, 61.0, 848.0, 110.0], None, 1.0),
            widget(&format!("{REWARDS}/ItemSlots/ItemSlot0/Icon0"), [800.45, 62.14, 847.41, 109.09], Some(10.0), 1.0),
            widget(&format!("{REWARDS}/ItemSlots/ItemSlot1"), [848.0, 61.0, 898.0, 110.0], None, 1.0),
            widget(&format!("{REWARDS}/ItemSlots/ItemSlot1/Icon0"), [849.52, 62.14, 896.47, 109.09], Some(10.0), 1.0),
        ];
        serde_json::from_value(serde_json::json!({ "window": "questscreen2", "widgets": widgets })).unwrap()
    }

    fn ids(layout: &Layout) -> Vec<&str> { layout.widgets.iter().map(|w| w.id.as_str()).collect() }
    fn get<'a>(layout: &'a Layout, id: &str) -> &'a Widget { layout.widgets.iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no {id}")) }
    fn text(w: &Widget) -> &str { w.text.as_ref().map_or("", |t| t.text.as_str()) }

    /// An offer as the 2026 capture shows it: the title, three lines, Back and
    /// Accept under them, the tasks from the List's top, the reward's item 33
    /// pixels under the panel's top; nothing moved sideways.
    #[test]
    fn an_offer_fills_the_bubble_the_tasks_and_the_rewards() {
        let screen = Screen {
            title: "Full Moon Rising (1/5)".into(),
            text: runs("Ever heard of da isle of <QuestHighlightStart>Varholm</> before?"),
            accept: Some(0), back: 1,
            quest: Some((vec![
                Task { text: runs("Gather 10 realm fragments by defeating worthy monsters and bring them to Thabo"), check: Check::Authored },
                Task { text: runs("Ask Thabo about the realm path to Varholm"), check: Check::Authored },
            ], Rewards { items: vec![(1, Icon { image: "chest.png".into(), ..default() })], ..default() })),
        };
        let mut out = Layout { window: "questscreen2".into(), widgets: Vec::new() };
        prepare(&layout(), &screen, &mut out);
        assert_eq!(out.widgets[0].id, BACKDROP);
        assert_eq!(text(get(&out, HEADLINE)), "Full Moon Rising (1/5)");
        // One line of text, three lines of bubble: the frame and the buttons 2 lines down.
        let step = 113.6 - 92.84;
        assert_eq!((text(get(&out, &format!("{LINE}0_0"))), text(get(&out, &format!("{LINE}0_1"))), text(get(&out, &format!("{LINE}0_2")))),
            ("Ever heard of da isle of ", "Varholm ", "before?"));
        assert_eq!(get(&out, &format!("{LINE}0_1")).text.as_ref().unwrap().color, WHITE);
        assert!(get(&out, &format!("{LINE}0_1")).rect[0] > 260.06);
        assert!((get(&out, &format!("{BUBBLE}/bottomleftcorner")).rect[1] - (107.0 + 2.0 * step)).abs() < 1e-3);
        assert!((get(&out, &format!("{BUBBLE}/topedge")).rect[1] - 53.0).abs() < 1e-3);
        let back = get(&out, &format!("{ROOT}/ButtonSetBackAccept/Back"));
        assert_eq!((back.event.as_str(), back.hidden), ("NpcChoice:1", false));
        assert!((back.rect[1] - (134.0 + 2.0 * step)).abs() < 1e-3 && (back.rect[0] - 244.0).abs() < 1e-3);
        assert_eq!(get(&out, &format!("{ROOT}/ButtonSetBackAccept/Accept")).event, "NpcChoice:0");
        assert!(ids(&out).contains(&format!("{ROOT}/ButtonSetBackAccept/Back/normal/icon").as_str()), "the buttons keep their icons");
        assert!(!ids(&out).iter().any(|id| id.contains("ButtonSetAccept/") || id.contains("ButtonSetBack/")));
        // The tasks: the first wraps to two lines, the second starts after them and a separator.
        let first = get(&out, &format!("{TASKS}/Task0_0/TextLabel0"));
        assert!((first.rect[1] - 370.9).abs() < 1e-3);
        assert!(ids(&out).contains(&format!("{TASKS}/Task0_1/TextLabel0").as_str()));
        assert!(ids(&out).contains(&format!("{TASKS}/Task0_0/CheckMarkLabel").as_str()) && !ids(&out).contains(&format!("{TASKS}/Task0_1/CheckMarkLabel").as_str()));
        let second = get(&out, &format!("{TASKS}/Task1_0/TextLabel0"));
        assert!((second.rect[1] - (370.9 + 2.0 * (400.77 - 381.62) + 7.0)).abs() < 1e-3);
        assert_eq!(text(second), "Ask Thabo about the realm path to Varholm");
        assert!(ids(&out).contains(&format!("{TASKS_HEADLINE}/TaskHeadlineLabel").as_str()));
        // Unseen parts (the containers, the separators, the panel's frame) and the scroll buttons stay out.
        assert!(!ids(&out).iter().any(|id| *id == TASKS || *id == LIST || *id == REWARDS || id.ends_with("/SeparatorTasks") || id.contains("/ScrollUp")));
        // The rewards: the headline, then the item row (no money, no experience).
        assert_eq!(text(get(&out, &format!("{REWARDS}/Headline/RewardHeadlineLabel"))), "");
        let slot = get(&out, &format!("{REWARDS}/Items0/ItemSlot0"));
        assert!((slot.rect[1] - (61.0 + (87.0 - 54.16))).abs() < 1e-3);
        assert_eq!(get(&out, &format!("{REWARDS}/Items0/ItemSlot0/Icon0/Image")).art.as_ref().unwrap().tex.as_deref(), Some("chest.png"));
        assert!(!ids(&out).iter().any(|id| id.contains("/ItemSlot1")), "unused slots are hidden");
        assert!(!ids(&out).iter().any(|id| id.contains("Template")));
        assert!(out.widgets.iter().all(|w| w.k == 0.0));
    }

    /// A quest in progress has Back alone; its tasks' marks follow the log; money
    /// and experience stack above the items; a conversation has no headline,
    /// tasks or rewards.
    #[test]
    fn progress_and_conversations_take_their_parts() {
        let screen = Screen {
            title: "Bone Oracle".into(), text: runs("Find me some bones."), accept: None, back: 0,
            quest: Some((vec![Task { text: runs("Bones"), check: Check::Done }, Task { text: runs("More bones"), check: Check::Open }],
                Rewards { copper: 12_345, xp: "XP: 20".into(), items: vec![(3, Icon { image: "bone.png".into(), ..default() })], ..default() })),
        };
        let mut out = Layout { window: "questscreen2".into(), widgets: Vec::new() };
        prepare(&layout(), &screen, &mut out);
        assert_eq!(get(&out, &format!("{ROOT}/ButtonSetBack/Back")).event, "NpcChoice:0");
        assert!(!ids(&out).iter().any(|id| id.contains("ButtonSetBackAccept")));
        assert_eq!(get(&out, &format!("{TASKS}/Task0_0/CheckMarkLabel")).art.as_ref().unwrap().tex.as_deref(), Some(CHECK_DONE));
        assert_eq!(get(&out, &format!("{TASKS}/Task1_0/CheckMarkLabel")).art.as_ref().unwrap().tex.as_deref(), Some(CHECK_EMPTY));
        // Gold present: TemplateVC, 1 gold 23 silver 45 copper, under the headline; the experience, then the items.
        let headline = 87.0 - 54.16;
        assert_eq!((text(get(&out, &format!("{REWARDS}/Money/Gold"))), text(get(&out, &format!("{REWARDS}/Money/Copper")))), ("1", "45"));
        assert!((get(&out, &format!("{REWARDS}/Money")).rect[1] - (54.16 + headline)).abs() < 1e-3);
        assert_eq!(text(get(&out, &format!("{REWARDS}/Experience/Value"))), "XP: 20");
        let money = 73.0 - 54.16;
        assert!((get(&out, &format!("{REWARDS}/Experience/Value")).rect[1] - (59.3 + headline + money)).abs() < 1e-3);
        let xp = 80.0 - 54.16;
        assert!((get(&out, &format!("{REWARDS}/Items0/ItemSlot0")).rect[1] - (61.0 + headline + money + xp)).abs() < 1e-3);
        assert_eq!(text(get(&out, &format!("{REWARDS}/Items0/ItemSlot0/Icon0"))), "3");

        let talk = Screen { title: String::new(), text: runs("Hello."), accept: Some(0), back: 1, quest: None };
        let mut out = Layout { window: "questscreen2".into(), widgets: Vec::new() };
        prepare(&layout(), &talk, &mut out);
        assert!(!ids(&out).iter().any(|id| *id == HEADLINE || id.contains("TaskLabel") || id.contains("RewardLabel")));
        assert_eq!(get(&out, &format!("{ROOT}/ButtonSetBackAccept/Accept")).event, "NpcChoice:0");
    }

    /// The screens from the data: the gendered, highlighted offer; the tasks a
    /// staged quest shows; this class's items; dynamic experience from the level.
    #[test]
    fn screens_come_from_the_quest_data() {
        let catalog: Catalog = serde_json::from_value(serde_json::json!({ "portraits": {}, "icons": {}, "texts": {
            "quest.q.s_title": "Moon", "quest.q.s_offer": "Go, man.", "quest.q.s_offer.f": "Go, woman.",
            "quest.q.s_pay": "Gather {0:i:amount} fragments", "quest.q.s_ask": "Ask {0:s:name}", "quest.q.s_later": "Later",
            "gui.window.questscreen_dynamicxpstring": "XP (level dependent): {0:i}" },
            "marked": { "quest.q.s_offer.f": "Go, <QuestHighlightStart>woman</>." } })).unwrap();
        let quests: QuestCatalog = serde_json::from_value(serde_json::json!({ "quests": { "Q": {
            "staged": true, "part": [1, 5], "rewards": { "dynamic_xp": 5 },
            "items": [{ "item": "a", "class": "mage", "rarity": "Rare" }, { "item": "b", "class": "warrior" }, { "item": "c", "amount": 2 }],
            "tasks": [{ "title": "pay", "target": 10 }, { "title": "ask" }, { "title": "later", "pre": [0] }] } } })).unwrap();
        let facts = Facts { catalog: &catalog, quests: Some(&quests), items: None, entry: None, female: true, name: "Rha",
            class: "mage", level: Some(40), xp: Some((5_000, 4_000, 6_000)) };
        let offer = quest("q", "offer", &facts, Some(0), 1);
        assert_eq!(offer.title, "Moon (1/5)");
        assert_eq!(offer.text, vec![Run { text: "Go, ".into(), color: None }, Run { text: "woman".into(), color: Some(WHITE) }, Run { text: ".".into(), color: None }]);
        let (tasks, rewards) = offer.quest.unwrap();
        assert_eq!(tasks.iter().map(|t| t.text[0].text.as_str()).collect::<Vec<_>>(), ["Gather 10 fragments", "Ask Rha"], "a staged task waits for its prerequisite");
        assert!(tasks.iter().all(|t| t.check == Check::Authored));
        assert_eq!((rewards.items.len(), rewards.items[1].0), (2, 2), "the mage's item and the one for every class");
        assert_eq!(rewards.xp, "XP (level dependent): 100");
        // In progress, the first task done: the staged one shows, the marks follow.
        let entry = QuestEntry { state: 0, finished: 0b1, ..default() };
        let progress = quest("q", "progress", &Facts { entry: Some(&entry), female: false, ..facts }, None, 0);
        let (tasks, _) = progress.quest.unwrap();
        assert_eq!(tasks.iter().map(|t| t.check).collect::<Vec<_>>(), [Check::Done, Check::Open, Check::Open]);
        assert_eq!(quest("q", "offer", &Facts { level: Some(100), ..facts }, Some(0), 1).quest.unwrap().1.xp, "", "no dynamic experience at the last level");
        let talk = conversation("q.s_offer", &facts, 0, 1);
        assert!(talk.title.is_empty() && talk.quest.is_none());
    }
}
