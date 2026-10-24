//! Behaviour for the original mesh UI. Artwork keeps its authored geometry;
//! visibility, hit testing and actions share the same page/state gates.
use super::*;
use bevy::ui::RelativeCursorPosition;
use bevy::window::{MonitorSelection, WindowMode};
use bevy::input::keyboard::KeyboardInput;
mod panels;
pub(super) mod spells;
pub(super) mod stickers;
pub(super) mod items;
pub(super) mod sheet;
pub(super) mod social;
pub(super) mod trade;
pub(super) mod party;
pub(super) mod quests;
pub(super) mod systems;
pub(super) mod template;
pub(crate) mod settings;
pub(super) mod hotbar;
pub(super) mod effects;
pub(super) mod currency;
pub(super) mod message;
pub(super) mod logout;
mod tooltips;
pub(super) use tooltips::overlay as tooltips;

/// Reference pixels, scaled by the same factor as the rest of the interface.
const DRAG_HANDLE_HEIGHT: f32 = 32.0;
/// Chat log rows per click of its scroll buttons.
const CHAT_SCROLL: i32 = 3;

#[derive(Clone, Copy)]
struct DragHandle {
    k: f32,
    rect: [f32; 4],
    /// When it can be grabbed (the chat: in its full mode).
    section: Section,
}

impl DragHandle {
    fn from_layout(layout: &Layout) -> Option<Self> {
        // The chat moves by its log's background, as a whole (its rows take no
        // clicks), while the full window shows.
        if layout.window == CHAT_WINDOW {
            let frame = layout.widgets.iter().find(|w| !w.hidden && w.id.ends_with("/LabelFull/ChatBackgroundLabel"))?;
            let rect = frame.art.as_ref().map(|a| bounds(&a.pos)).unwrap_or(frame.rect);
            return Some(Self { k: frame.k, rect, section: Section::ChatFull });
        }
        // Use the painted frame, not the source container bounds: inventory's
        // container includes offscreen templates thousands of pixels above it.
        let suffix = match layout.window.as_str() {
            "inventory" | "charactersheet" => "/FrameLabel/BorderLabel",
            "skilltalentswindow" => "/BackGroundLabel/Frame/Frame2",
            "gamemenu" => "Canvas/CenterLabel",
            other => panels::frame_suffix(other)?,
        };
        let frame = layout.widgets.iter().find(|w| !w.hidden && w.id.ends_with(suffix))?;
        let mut rect = frame.art.as_ref().map(|a| bounds(&a.pos)).unwrap_or(frame.rect);
        for title in layout.widgets.iter().filter(|w| !w.hidden &&
            ["/WindowTitleLabel", "/Headline", "/TitlaLabel", "/WindowTitleBar"].iter().any(|s| w.id.ends_with(s))) {
            if title.rect[1] >= 0.0 { rect[1] = rect[1].min(title.rect[1]); }
        }
        rect[3] = rect[1] + DRAG_HANDLE_HEIGHT;
        Some(Self { k: frame.k, rect, section: Section::Always })
    }

    fn clamp_offset(self, offset: Vec2, width: f32) -> Vec2 {
        let [l, t, r, b] = self.rect;
        let anchor = self.k * (width - BASE_W);
        // Keep enough of the title reachable to recover a partly offscreen
        // window, including after resizing the client.
        let visible_width = 64.0_f32.min(r - l).min(width);
        Vec2::new(offset.x.clamp(visible_width - r - anchor, width - visible_width - l - anchor),
            offset.y.clamp(-t, BASE_H - b))
    }
}

fn window_offset(transform: &Transform) -> Vec2 {
    Vec2::new(transform.translation.x, -transform.translation.y)
}

fn translated([l, t, r, b]: [f32; 4], offset: Vec2) -> [f32; 4] {
    [l + offset.x, t + offset.y, r + offset.x, b + offset.y]
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Section {
    SettingsRow(usize),
    #[default]
    Always,
    Inventory(u8),
    /// The inventory's mercenary page: its Shadow Soul's figure (true), or the
    /// empty placeholder (false) while there is none (crate::ui::preview sets
    /// the "shadow_soul" page).
    Mercenary(bool),
    InventoryReturn,
    OverflowFooter,
    BagFourthRow,
    Character(u8),
    /// Allowed pages as a bitmask; some shared headers serve several tabs.
    Page(&'static str, u32),
    SkillClass(u8),
    Ranking(u8, bool),
    ChatFull,
    ChatCompact,
    ChatChannel(u8),
    ChatTyping,
    ChatScrolled,
    Hidden,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Normal,
    Hover,
    Pressed,
    HoverPressed,
    Disabled,
}

impl State {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "normal" => Self::Normal,
            "mouseover" => Self::Hover,
            "pressed" => Self::Pressed,
            "mouseoverpressed" => Self::HoverPressed,
            "disabled" => Self::Disabled,
            _ => return None,
        })
    }

    fn bit(self) -> u8 { 1 << self as u8 }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Gate {
    section: Section,
    button: Option<(usize, State)>,
}

impl Gate {
    pub(super) fn chat(compact: bool) -> Self {
        Self { section: if compact { Section::ChatCompact } else { Section::ChatFull }, button: None }
    }
}

fn section(window: &str, id: &str) -> Section {
    if window == settings::WINDOW {
        let leaf = id.rsplit('/').next().unwrap_or("");
        if let Some(row) = leaf.strip_prefix("row").or_else(|| leaf.strip_prefix("label")).and_then(|s| s.parse().ok()) { return Section::SettingsRow(row); }
    }
    let parts: Vec<_> = id.split('/').collect();
    // The hovered monster's ornament -- its top and bottom flourishes -- only
    // on a boss (ui show_hover_window); the frame (gothic_panel) and the level
    // ring stay for every monster.
    if window == "interactinfo" && ["/gothic_panel/LabelTop", "/gothic_panel/LabelTop2", "/gothic_panel/LabelBottom"].iter().any(|end| id.ends_with(end)) {
        return Section::Page("hover_boss", 0b10);
    }
    // The character list's scroll handle moves with the list: crate::ui::controls::sheet draws it.
    if window == "charactersheet" && id.contains("/OverviewList/ScrollBarBackground/ScrollBarBoundaries/ScrollHandle") { return Section::Hidden; }
    // Its row templates too: drawn as they are, the first headline, its icon and
    // a sub-headline bar stood still under the scrolling rows ("ca ecrit sur les
    // sous menus", "les epees restent").
    if window == "charactersheet" && id.contains("/CharacterStatsArea/Template") { return Section::Hidden; }
    // The album's preview box: its sticker turning (crate::ui::preview), or
    // the sticker's icon.
    if window == systems::ALBUM {
        if id.contains("/background_image/DragRotateAreaLabel") || id.ends_with(systems::album::TURNING) { return Section::Page(super::preview::ALBUM_MODEL, 0b10); }
        if id.contains("/background_preview/Sticker/") { return Section::Page(super::preview::ALBUM_MODEL, 0b01); }
    }
    if let Some(section) = panels::section(window, &parts) { return section; }
    if let Some(section) = social::section(window, &parts) { return section; }
    if window == "inventory" {
        if id.contains("/unten/Label_bg/Label_empty") { return Section::OverflowFooter; }
        if id.contains("/unten/Label_bg/icon_area/TemplateSlot#3_") { return Section::BagFourthRow; }
        if id.contains("/mercenary/label_empty/") && id.ends_with("/figur") { return Section::Mercenary(true); }
        if id.contains("/mercenary/label_empty/") && id.ends_with("/empty") { return Section::Mercenary(false); }
        for (name, page) in [("oben", 0), ("IngredientBag", 1), ("TrinketBag", 2), ("mercenary", 3)] {
            if parts.contains(&name) { return Section::Inventory(page); }
        }
        if parts.contains(&"InventoryReturn") { return Section::InventoryReturn; }
        // These are the unavailable-access overlays, mutually exclusive with
        // the real controls. Keep the controls and their disabled states.
        if parts.iter().any(|p| p.ends_with("NotVisible")) { return Section::Hidden; }
    }
    if window == "charactersheet" {
        for (name, page) in [("StatsCharacter", 0), ("StatsPVP", 1), ("AchievementTitles", 2), ("MembershipStatus", 3), ("VeteranStatus", 4)] {
            if parts.contains(&name) { return Section::Character(page); }
        }
    }
    if window == CHAT_WINDOW {
        // Compact mode keeps only the expand button; runtime rows are click-through.
        if parts.contains(&"LabelCompact") {
            return if parts.contains(&"ButtonToggleMode") { Section::ChatCompact } else { Section::Hidden };
        }
        // The List's row templates are drawn by chat_rows.
        if parts.iter().any(|p| matches!(*p, "TextEntryChannelLabel" | "ScrollHandle" | "ButtonTextLine"))
            || parts.last() == Some(&"TextLine") { return Section::Hidden; }
        // "Press enter to open the chat" while not typing; ScrollEnd's glow while
        // scrolled back (both set by edit_text / chat_rows).
        if parts.contains(&"TextLabelHowTo") { return Section::ChatTyping; }
        if parts.contains(&"HighlightLabel") { return Section::ChatScrolled; }
        if parts.contains(&"ButtonChatMenu") {
            if let Some(channel) = ["IconLabelMap", "IconLabelRegion", "IconLabelParty", "IconLabelGuild", "IconLabelWhisper"].iter().position(|name| parts.contains(name)) {
                return Section::ChatChannel(channel as u8);
            }
        }
        return Section::ChatFull;
    }
    if window == "gamebar" && parts.iter().any(|p| matches!(*p, "Premium" | "Deluxe" | "bgAutoplay" | "autoplay" | "SigninBtn" | "CameBackSignInBtn" | "SeasonPassBtn")) {
        return Section::Hidden;
    }
    // The comeback event's Return Link and Return Check-in buttons: shown unless
    // the player hid them (Settings > General; settings::sync sets the pages).
    if window == "gamebar" {
        for group in settings::GAMEBAR_OPTIONAL { if parts.contains(&group) { return Section::Page(group, 1); } }
    }
    Section::Always
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Settings(String),
    Bar(i32),
    Essence,
    NpcChoice(usize),
    Close,
    Window(&'static str),
    Inventory(u8),
    Bag(usize),
    Ingredient(usize),
    Rarity(usize),
    Character(u8),
    Tab(&'static str, u8),
    Toggle(String),
    Edit(String),
    Membership,
    Page(i32),
    Fullscreen,
    Lock,
    Skill(usize),
    Spell,
    Item(items::Cell),
    Split(i32),
    SplitSlider,
    /// items::DESTROY_DIALOG's answer (items::input): destroy the item or not.
    Destroy(bool),
    /// Scroll the chat log back (rows; 0: to the newest).
    ChatScroll(i32),
    /// Write on this channel (crate::network::chat::Kind::channel), from the channel menu.
    ChatChannel(u8),
    ChatCompact(bool),
    Unavailable,
    /// The death dialog's choice: (at the nearest exit, on the spot).
    Respawn(bool, bool),
    /// A notification dialog's or the player menu's button (social::answer).
    Social(social::Social),
    /// The quest log's rows (quests::prepare_log): open this quest, drop it.
    QuestSelect(String),
    QuestCancel(String),
    /// The tracker's previous/next quest.
    QuestTrack(i32),
    /// Fold or unfold a quest group of the log (its type).
    QuestGroup(String),
    /// The tracker's toggle: compact or full.
    QuestCompact(bool),
    /// The party frames' toggle: compact (the button alone) or full.
    PartyCompact(bool),
    /// The game menu's switch character / logout (logout::dialog asks first).
    Logout,
    /// logout::WINDOW's buttons: confirm (true) or cancel.
    LogoutAnswer(bool),
    /// A talent card of skilltalentswindow: buy one rank (spells::apply_drop sends it).
    Talent(String),
    /// The travel map's destination (social::answer sends it).
    Travel(String),
    /// A systems window's request (systems::send carries it out).
    System(systems::Request),
    /// Held, it turns its window's 3D preview with the cursor (crate::ui::preview).
    DragRotate,
    /// A currency bag row: show this currency on the bar (currency).
    Currency(String),
}

fn action(window: &str, w: &Widget) -> Action {
    if window == settings::WINDOW { return Action::Settings(w.event.clone()); }
    if window == "gamemenu" && w.event == "Options" { return Action::Settings("open".into()); }
    // The server has one logout: back to the character screen (after a countdown
    // outside towns); its two buttons do the same.
    if window == "gamemenu" && matches!(w.event.as_str(), "SwitchCharacter" | "Logout") { return Action::Logout; }
    if dialogue::WINDOWS.contains(&window) {
        if let Some(index) = w.event.strip_prefix("NpcChoice:").and_then(|s| s.parse().ok()) { return Action::NpcChoice(index); }
    }
    let leaf = w.id.rsplit('/').next().unwrap_or("");
    if let Some(spell) = &w.spell { return if w.event == "InspectTalent" { Action::Talent(spell.id.clone()) } else { Action::Spell }; }
    if w.kind == "TextEntry" { return Action::Edit(w.id.clone()); }
    if w.event == "Close" { return Action::Close; }
    if w.event == "DragRotate" { return Action::DragRotate; }
    if let Some(action) = currency::action(window, w) { return action; }
    if let Some(action) = social::action(window, w) { return action; }
    if let Some(action) = trade::action(window, w) { return action; }
    if let Some(action) = systems::action(window, w) { return action; }
    if window == "deathdialog" {
        // DontBuy: back in the last town, free; Buy: on the same map, at the
        // nearest exit (RespawnCost); Buy1: on the spot (InstantRespawnCost). The
        // server charges both (docs/PLAYER_RESPAWN.md).
        return match leaf {
            "DontBuy" => Action::Respawn(false, false),
            "Buy" => Action::Respawn(true, false),
            "Buy1" => Action::Respawn(false, true),
            _ => Action::Unavailable,
        };
    }
    if window == CHAT_WINDOW {
        return match (w.event.as_str(), leaf) {
            ("SwitchToCompact", _) => Action::ChatCompact(true),
            ("SwitchToFull", _) => Action::ChatCompact(false),
            ("ToggleChatMenu", _) => Action::Window(CHAT_MENU),
            ("ScrollEnd", _) => Action::ChatScroll(0),
            (_, "ScrollUp") => Action::ChatScroll(CHAT_SCROLL),
            (_, "ScrollDown") => Action::ChatScroll(-CHAT_SCROLL),
            (_, "ChannelTabMap") => Action::Tab("chat_channel", 0),
            (_, "ChannelTabRegion") => Action::Tab("chat_channel", 1),
            (_, "ChannelTabParty") => Action::Tab("chat_channel", 2),
            (_, "ChannelTabGuild") => Action::Tab("chat_channel", 3),
            // ToggleLanguageFilter.
            _ => Action::Unavailable,
        };
    }
    if window == CHAT_MENU {
        return ["Map", "Region", "Party", "Guild", "Whisper"].iter()
            .position(|c| w.event.strip_prefix("SelectChannel") == Some(c))
            .map_or(Action::Unavailable, |i| Action::ChatChannel(i as u8));
    }
    if window == logout::WINDOW {
        return match w.event.as_str() { "confirm" => Action::LogoutAnswer(true), "cancel" => Action::LogoutAnswer(false), _ => Action::Unavailable };
    }
    if window == items::DESTROY_DIALOG {
        return match w.event.as_str() { "confirm" => Action::Destroy(true), "cancel" => Action::Destroy(false), _ => Action::Unavailable };
    }
    if window == "unstackwindow" {
        return match w.event.as_str() { "split" => Action::Split(0), "increase" => Action::Split(1), "decrease" => Action::Split(-1),
            _ if w.kind == "Slider" => Action::SplitSlider, _ => Action::Unavailable };
    }
    if window == "inventory" {
        if let Some(cell) = items::cell(w) { return Action::Item(cell); }
    }
    if let Some(action) = panels::action(window, w) { return action; }
    if window == "ingamemenu" {
        return match leaf {
            "char" => Action::Window("charactersheet"),
            "inventory" => Action::Window("inventory"),
            "skills" => Action::Window("skilltalentswindow"),
            "quest" => Action::Window("questlog"),
            // The authored button keeps its old map ID, but is the stickerbook.
            "travelmap" => Action::Window("collectorsalbum"),
            "minimap" => Action::Window("locationmap"),
            "dealer" => Action::Window("shopwindow"),
            "socialscreen" => Action::Window("socialscreen"),
            "pvpleaderboards" => Action::Window("leaderboards"),
            "pvp" => Action::Window("matchmakingwindow"),
            "events" => Action::Window("eventwindow"),
            _ => Action::Unavailable,
        };
    }
    if window == "gamebar" {
        return match w.event.as_str() {
            "Logout" => Action::Window("gamemenu"),
            "ToggleFullscreen" => Action::Fullscreen,
            "Premium" => Action::Membership,
            _ => Action::Unavailable,
        };
    }
    if window == "inventory" {
        match w.event.as_str() {
            "InventoryReturn" => return Action::Inventory(0),
            "IngredientBag" => return Action::Inventory(1),
            "TrinketBag" => return Action::Inventory(2),
            "MercenaryBag" => return Action::Inventory(3),
            "NextPage" => return Action::Page(1),
            "PrevPage" => return Action::Page(-1),
            _ => {}
        }
        if let Some(suffix) = leaf.strip_prefix("BagButton") {
            if let Some(page) = if suffix.is_empty() { Some(0) } else { suffix.parse().ok() } {
                return if w.id.contains("/IngredientBag/") { Action::Ingredient(page) } else { Action::Bag(page) };
            }
        }
        if let Some(rarity) = leaf.strip_prefix("Button_Rarity").and_then(|s| s.parse().ok()) {
            return Action::Rarity(rarity);
        }
        return match leaf {
            "ExtraBagButton" => Action::Bag(items::EXTRA),
            "OverflowBagButton" => Action::Bag(items::OVERFLOW),
            _ => Action::Unavailable,
        };
    }
    if window == "charactersheet" {
        return match leaf {
            "TabButtonCharacter" => Action::Character(0),
            "TabButtonPVP" => Action::Character(1),
            "TabButtonTitles" => Action::Character(2),
            "TabButtonMembership" => Action::Character(3),
            "TabButtonVeteran" => Action::Character(4),
            _ => Action::Unavailable,
        };
    }
    if window == "bottombar" {
        if w.event == "EssenceToggle" { return Action::Essence; }
        if w.event == "PreviousQuickbar" { return Action::Bar(-1); }
        if w.event == "NextQuickbar" { return Action::Bar(1); }
        if w.event == "Lock" { return Action::Lock; }
        if let Some(slot) = hotbar::slot_leaf(leaf) { return Action::Skill(slot); }
        if leaf == "LMB" { return Action::Skill(0); }
        if leaf == "RMB" { return Action::Skill(1); }
        if let Some(slot) = leaf.strip_prefix("QuickSlot").and_then(|s| s.parse::<usize>().ok()) {
            return Action::Skill(slot + 1);
        }
    }
    Action::Unavailable
}

struct Button {
    id: String,
    k: f32,
    rect: [f32; 4],
    section: Section,
    action: Action,
    authored_enabled: bool,
    states: u8,
    spell: Option<spells::Spell>,
    /// A slot's art depth in its window (spawn_windows): items::render draws
    /// its drop highlight there, under the widgets after it.
    depth: f32,
}

#[derive(Component)]
pub(super) struct WindowControls {
    pub(super) map_mode: local_map::MapMode,
    buttons: Vec<Button>,
    regions: Vec<(f32, [f32; 4], Gate)>,
    drag_handle: Option<DragHandle>,
    inventory: u8,
    character: u8,
    bag: usize,
    bag_pages: usize,
    premium_bag: bool,
    overflow_page: usize,
    overflow_pages: usize,
    ingredient: usize,
    rarity: usize,
    locked: bool,
    hovered: Option<usize>,
    pressed: Option<usize>,
    pages: HashMap<&'static str, u8>,
    toggles: std::collections::HashSet<String>,
    open_windows: Vec<String>,
    entries: HashMap<String, TextEntry>,
    spells: spells::Model,
    chat_compact: bool,
    chat_channels: [bool; 5],
    /// logout::WINDOW waiting for its countdown.
    waiting: bool,
}

impl WindowControls {
    /// A slot widget's art depth (spawn_windows), where its drop highlight goes.
    pub(super) fn set_depth(&mut self, id: &str, depth: f32) {
        if let Some(b) = self.buttons.iter_mut().find(|b| b.id == id) { b.depth = depth; }
    }

    pub(super) fn new(layout: &Layout) -> Self {
        let buttons = layout.widgets.iter()
            .filter(|w| layout.window != "locationmap" && !w.hidden
                // The chat's entry is ClickThrough, yet Enter and clicks focus it.
                && (!w.click_through || (layout.window == CHAT_WINDOW && w.kind == "TextEntry")) && (matches!(w.kind.as_str(), "Button" | "TextButton" | "SlotButton" | "TextEntry" | "Slider") || (layout.window == "inventory" && items::cell(w).is_some())))
            .map(|w| Button {
                id: w.id.clone(), k: w.k, rect: w.rect,
                section: section(&layout.window, &w.id), action: action(&layout.window, w), authored_enabled: w.enabled,
                spell: w.spell.clone(), depth: 0.0,
                states: layout.widgets.iter().filter(|s| !s.hidden && s.button.as_ref() == Some(&w.id))
                    .filter_map(|s| s.state.as_deref().and_then(State::parse)).fold(0, |bits, state| bits | state.bit()),
            }).collect();
        let mut controls = Self { map_mode: local_map::MapMode::Compact, buttons, regions: Vec::new(), drag_handle: DragHandle::from_layout(layout), inventory: 0, character: 0, bag: 0, bag_pages: 8, premium_bag: false, overflow_page: 0, overflow_pages: 1, ingredient: 0, rarity: 1, locked: false, hovered: None, pressed: None,
            pages: HashMap::new(), toggles: default(), open_windows: Vec::new(), entries: HashMap::new(), spells: default(), chat_compact: false, chat_channels: [false; 5], waiting: false };
        // The chat's entry starts empty, not with its "Enter text" sample.
        // Start unobtrusively at login; screenshots can opt into expanded mode.
        if layout.window == CHAT_WINDOW {
            for w in layout.widgets.iter().filter(|w| w.kind == "TextEntry") { controls.entries.insert(w.id.clone(), TextEntry::default()); }
            controls.chat_compact = !std::env::var("DSOR_CHAT_MODE").is_ok_and(|m| m == "full");
        }
        // Together with DSOR_UI_OPEN=locationmap, capture either overlay size.
        if layout.window == "locationmap" && std::env::var("DSOR_MAP_VIEW").is_ok_and(|v| v == "expanded") {
            controls.map_mode = local_map::MapMode::Expanded;
        }
        // Native fixture captures without desktop input (only with packet replay).
        if layout.window == "inventory" && std::env::var_os("DSOR_INVENTORY_PREVIEW").is_some() {
            if let Ok(view) = std::env::var("DSOR_INVENTORY_VIEW") {
                let mut parts = view.split(',');
                match parts.next() {
                    Some("ingredients") => controls.inventory = 1,
                    Some("premium") => controls.bag = items::EXTRA,
                    Some("overflow") => controls.bag = items::OVERFLOW,
                    _ => {}
                }
                controls.rarity = parts.next().and_then(|n| n.parse::<usize>().ok()).unwrap_or(1).clamp(1, 5);
            }
        }
        controls
    }

    /// A rebuilt window keeps what its player set: its pages and toggles, its
    /// texts, its lock; the buttons and regions are the new layout's.
    pub(super) fn carry(&mut self, old: Self) {
        self.map_mode = old.map_mode;
        self.inventory = old.inventory;
        self.character = old.character;
        self.bag = old.bag;
        self.bag_pages = old.bag_pages;
        self.premium_bag = old.premium_bag;
        self.overflow_page = old.overflow_page;
        self.overflow_pages = old.overflow_pages;
        self.ingredient = old.ingredient;
        self.rarity = old.rarity;
        self.locked = old.locked;
        self.pages = old.pages;
        self.toggles = old.toggles;
        self.entries = old.entries;
        self.spells = old.spells;
        self.chat_compact = old.chat_compact;
        self.chat_channels = old.chat_channels;
        self.open_windows = old.open_windows;
    }

    pub(super) fn gate(&self, window: &str, widget: &Widget) -> Gate {
        let button = widget.button.as_ref().and_then(|id| self.buttons.iter().position(|b| &b.id == id))
            .zip(widget.state.as_deref().and_then(State::parse));
        // A visual state whose owner is noninteractive (e.g. a ClickThrough
        // template) still has only one default state.
        let orphan_state = widget.state.as_deref().is_some_and(|s| s != "normal") && button.is_none();
        Gate { section: if orphan_state { Section::Hidden } else { section(window, &widget.id) }, button }
    }

    pub(super) fn region(&mut self, k: f32, rect: [f32; 4], gate: Gate) {
        self.regions.push((k, rect, gate));
    }

    /// Show a group's page (Section::Page); unchanged when already shown.
    pub(super) fn set_page(&mut self, group: &'static str, page: u8) {
        if self.pages.get(group).copied().unwrap_or(0) != page { self.pages.insert(group, page); }
    }

    /// A group's page shown (Section::Page), 0 by default.
    pub(super) fn page(&self, group: &str) -> u8 { self.pages.get(group).copied().unwrap_or(0) }

    /// The inventory's page: 0 equipment, 1 ingredients, 2 trinkets, 3 mercenary.
    pub(super) fn inventory_page(&self) -> u8 { self.inventory }

    fn section_visible(&self, section: Section) -> bool {
        match section {
            Section::Always => true,
            Section::SettingsRow(row) => row < self.pages.get("settings_rows").copied().unwrap_or(7) as usize,
            Section::Inventory(page) => self.inventory == page,
            Section::Mercenary(soul) => self.inventory == 3 && (self.page("shadow_soul") != 0) == soul,
            Section::InventoryReturn => self.inventory != 0,
            Section::OverflowFooter => self.bag == items::OVERFLOW,
            Section::BagFourthRow => self.bag != items::OVERFLOW,
            Section::Character(page) => self.character == page,
            Section::Page(group, mask) => mask & (1 << self.pages.get(group).copied().unwrap_or(0)) != 0,
            Section::SkillClass(class) => self.spells.class == class && self.pages.get("talents").copied().unwrap_or(0) == 0,
            Section::Ranking(group, overall) => self.pages.get("rank_group").copied().unwrap_or(0) == group
                && (self.pages.get("rank_category").copied().unwrap_or(0) == 0) == overall,
            Section::ChatFull => !self.chat_compact,
            Section::ChatCompact => self.chat_compact,
            Section::ChatChannel(channel) => !self.chat_compact && self.pages.get("chat_channel").copied().unwrap_or(0) == channel,
            Section::ChatTyping => !self.chat_compact && self.pages.get("chat_typing").copied().unwrap_or(0) == 0,
            Section::ChatScrolled => !self.chat_compact && self.pages.get("chat_scrolled").copied().unwrap_or(0) != 0,
            Section::Hidden => false,
        }
    }

    fn enabled(&self, button: &Button) -> bool {
        button.authored_enabled && match button.action {
            Action::Unavailable => false,
            Action::Tab("chat_channel", channel) | Action::ChatChannel(channel) => self.chat_channels.get(channel as usize).copied().unwrap_or(false),
            Action::Spell => button.spell.as_ref().is_some_and(|s| self.spells.available(s)),
            Action::Bag(items::EXTRA) => self.premium_bag,
            Action::Bag(items::OVERFLOW) => true,
            Action::Bag(page) => page < self.bag_pages,
            Action::Page(-1) => self.bag == items::OVERFLOW && self.overflow_page > 0,
            Action::Page(1) => self.bag == items::OVERFLOW && self.overflow_page + 1 < self.overflow_pages,
            // Counting down, the dialog's confirm is greyed (its cancel stays).
            Action::LogoutAnswer(true) => !self.waiting,
            _ => true,
        }
    }

    fn selected(&self, action: &Action) -> bool {
        match action {
            Action::Settings(event) => ["general", "graphics", "audio", "controls", "cursor", "combatSettings"].iter().position(|t| *t == event)
                .is_some_and(|i| self.pages.get("settings").copied().unwrap_or(0) == i as u8),
            Action::Window(name) => self.open_windows.iter().any(|n| n == name),
            Action::Tab(group, page) => self.pages.get(group).copied().unwrap_or(0) == *page,
            Action::Toggle(id) => self.toggles.contains(id),
            Action::Inventory(page) => self.inventory == *page,
            Action::Bag(page) => self.bag == *page,
            Action::Character(page) => self.character == *page,
            Action::Ingredient(page) => self.ingredient == *page,
            Action::Rarity(rarity) => self.rarity == *rarity,
            Action::Lock => self.locked,
            _ => false,
        }
    }

    fn state(&self, index: usize) -> State {
        let b = &self.buttons[index];
        let hovered = self.hovered == Some(index);
        let down = (self.pressed == Some(index) && hovered) || self.selected(&b.action);
        let candidates = if !self.enabled(b) {
            [State::Disabled, State::Normal, State::Normal]
        } else if down && hovered {
            [State::HoverPressed, State::Pressed, State::Hover]
        } else if down {
            [State::Pressed, State::Normal, State::Normal]
        } else if hovered {
            [State::Hover, State::Normal, State::Normal]
        } else {
            [State::Normal; 3]
        };
        candidates.into_iter().find(|s| b.states & s.bit() != 0).unwrap_or(State::Normal)
    }

    fn visible(&self, gate: &Gate) -> bool {
        self.section_visible(gate.section) && gate.button.is_none_or(|(index, state)| self.state(index) == state)
    }

    fn hit(&self, point: Vec2, width: f32) -> Option<usize> {
        self.buttons.iter().enumerate().rev().find(|(_, b)| self.section_visible(b.section) && contains(b.k, b.rect, point, width)).map(|(i, _)| i)
    }

    fn covers(&self, point: Vec2, width: f32) -> bool {
        self.regions.iter().any(|(k, rect, gate)| self.visible(gate) && contains(*k, *rect, point, width))
            || self.hit(point, width).is_some()
            || self.title_hit(point, width)
    }

    fn title_hit(&self, point: Vec2, width: f32) -> bool {
        self.drag_handle.is_some_and(|h| self.section_visible(h.section) && contains(h.k, h.rect, point, width))
    }
}

fn contains(k: f32, [l, t, r, b]: [f32; 4], point: Vec2, width: f32) -> bool {
    let x = point.x - k * (width - BASE_W);
    x >= l && x <= r && point.y >= t && point.y <= b
}

#[derive(Component)]
pub(super) struct Element {
    pub root: Entity,
    pub gate: Gate,
}

/// One mouse press belongs to exactly one control until release. Releasing
/// over another control, or pressing outside then dragging in, never clicks it.
#[derive(Resource, Default)]
pub(super) struct Pointer {
    down: Option<(Entity, usize)>,
    drag: Option<Drag>,
    focus: Option<(Entity, String)>,
    spell_drag: Option<spells::DragSpell>,
    spell_drop: Option<spells::DropSpell>,
    /// A Collector's Bag sticker being dragged, and the shortcut dropped on a quick
    /// slot (kind 2 + sticker id, or kind 1 + item template; slot), for
    /// stickers::apply_drop.
    sticker_drag: Option<stickers::StickerDrag>,
    sticker_drop: Option<(i32, String, usize)>,
    /// A sticker or item shortcut dragged from a quick slot: onto another slot, or
    /// off the bar (None), for stickers::apply_drop.
    slot_move: Option<(usize, Option<usize>)>,
    /// A window to toggle (the Universal Map's travel map), for keyboard.
    open_window: Option<&'static str>,
    item_command: Option<i32>,
    /// The destroy dialog's answer, for items::input.
    destroy: Option<bool>,
    slider: Option<settings::SliderDrag>,
    /// The game menu's logout, and its dialog's answer, for logout::dialog.
    logout: bool,
    logout_answer: Option<bool>,
    /// A talent to buy a rank of, and a (preset, category) to switch to, for
    /// spells::apply_drop.
    talent: Option<String>,
    preset: Option<(u8, u8)>,
    /// The travel map's destination, for social::answer.
    travel: Option<String>,
    /// The systems windows' requests, for systems::send.
    system: Vec<systems::Request>,
    /// A DragRotate button held: its window and the cursor's x last frame; the
    /// reference pixels it moved since crate::ui::preview::turn last took them.
    turn: Option<(Entity, f32)>,
    turned: Option<(Entity, f32)>,
}

impl Pointer {
    /// The window whose preview DragRotate turns, and by how far the cursor
    /// moved sideways (reference pixels); taken once.
    pub(super) fn take_turn(&mut self) -> Option<(Entity, f32)> {
        self.turned.take()
    }

    /// The (bar, slot) a spell is being dragged from (spells::DragSpell::lifted).
    pub(super) fn lifted_slot(&self) -> Option<(usize, usize)> {
        self.spell_drag.as_ref().and_then(|d| d.lifted()).or_else(|| self.sticker_drag.as_ref().and_then(|d| d.lifted()))
    }
}

#[derive(Component)]
pub(super) struct EntryLabel(pub String);

#[derive(Default)]
struct TextEntry {
    text: String,
    cursor: usize,
    selected: bool,
}

impl TextEntry {
    fn edit(&mut self, key: KeyCode, text: Option<&str>, control: bool) {
        if control {
            if key == KeyCode::KeyA { self.selected = true; }
            return;
        }
        let left = self.text[..self.cursor].char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
        let right = self.text[self.cursor..].chars().next().map(|c| self.cursor + c.len_utf8()).unwrap_or(self.cursor);
        match key {
            KeyCode::ArrowLeft => { self.cursor = left; self.selected = false; }
            KeyCode::ArrowRight => { self.cursor = right; self.selected = false; }
            KeyCode::Home => { self.cursor = 0; self.selected = false; }
            KeyCode::End => { self.cursor = self.text.len(); self.selected = false; }
            KeyCode::Backspace | KeyCode::Delete => {
                if self.selected { self.text.clear(); self.cursor = 0; }
                else if key == KeyCode::Backspace { self.text.replace_range(left..self.cursor, ""); self.cursor = left; }
                else { self.text.replace_range(self.cursor..right, ""); }
                self.selected = false;
            }
            _ => if let Some(text) = text {
                if self.selected { self.text.clear(); self.cursor = 0; self.selected = false; }
                let text: String = text.chars().filter(|c| !c.is_control()).take(128usize.saturating_sub(self.text.chars().count())).collect();
                self.text.insert_str(self.cursor, &text);
                self.cursor += text.len();
            }
        }
    }
}

struct Drag {
    root: Entity,
    /// Cursor position relative to the root when grabbed (reference pixels).
    grab: Vec2,
    width: f32,
}

impl Pointer {
    fn update(&mut self, hit: Option<(Entity, usize)>, press: bool, release: bool) -> Option<(Entity, usize)> {
        if press { self.down = hit; }
        if release {
            let down = self.down.take();
            return down.filter(|d| Some(*d) == hit);
        }
        None
    }
}

#[derive(Resource, Default)]
pub(crate) struct SkillClicks(pub Option<usize>);

type Windows<'w, 's> = Query<'w, 's, (Entity, &'static Window2018, &'static mut WindowControls, &'static mut Visibility, &'static mut Transform)>;

fn raise(entity: Entity, windows: &mut Windows) {
    let mut order: Vec<_> = windows.iter().filter(|(_, name, _, _, _)| stacked_window(&name.0))
        .map(|(e, _, _, _, tf)| (e, tf.translation.z)).collect();
    if !order.iter().any(|(e, _)| *e == entity) { return; }
    order.sort_by(|a, b| (a.0 == entity).cmp(&(b.0 == entity)).then(a.1.total_cmp(&b.1)));
    // Bounded, separated depth bands keep entire windows together and below
    // the modal game menu, even after repeated changes of focus.
    for (i, (e, _)) in order.into_iter().enumerate() {
        if let Ok((_, _, _, _, mut tf)) = windows.get_mut(e) { tf.translation.z = 100.0 + i as f32 * WINDOW_DEPTH_STEP; }
    }
}

fn stacked_window(name: &str) -> bool {
    is_menu(name) && !matches!(name, "gamemenu" | "unstackwindow" | items::DESTROY_DIALOG | "locationmap" | settings::WINDOW | social::CONTEXT_MENU)
}

/// Also cover windows opened outside toggle(), including asynchronously loaded
/// startup panels. Focus changes still go through raise() in pointer handling.
pub(super) fn order_windows(mut windows: Windows, mut visible: Local<std::collections::HashSet<Entity>>) {
    let mut opened: Vec<_> = windows.iter().filter(|(e, name, _, v, _)|
        stacked_window(&name.0) && *v != Visibility::Hidden && !visible.contains(e))
        .map(|(e, _, _, _, tf)| (e, tf.translation.z)).collect();
    opened.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    for (entity, _) in opened { raise(entity, &mut windows); }
    *visible = windows.iter().filter(|(_, name, _, v, _)| stacked_window(&name.0) && *v != Visibility::Hidden)
        .map(|(e, _, _, _, _)| e).collect();
}

fn toggle(name: &str, windows: &mut Windows) {
    if name == "locationmap" {
        for (_, w, mut controls, mut visibility, _) in windows.iter_mut() {
            if w.0 != name { continue; }
            if *visibility == Visibility::Hidden {
                controls.map_mode = local_map::MapMode::Compact;
                *visibility = Visibility::Inherited;
            } else if controls.map_mode == local_map::MapMode::Compact {
                controls.map_mode = local_map::MapMode::Expanded;
            } else {
                *visibility = Visibility::Hidden;
            }
        }
        return;
    }
    let open = windows.iter().any(|(_, w, _, v, _)| w.0 == name && *v == Visibility::Hidden);
    for (_, w, mut controls, mut v, _) in windows.iter_mut() {
        if w.0 == name {
            *v = if open { Visibility::Inherited } else { Visibility::Hidden };
            controls.hovered = None;
            controls.pressed = None;
        } else if open && matches!(name, "charactersheet" | "skilltalentswindow") && matches!(w.0.as_str(), "charactersheet" | "skilltalentswindow") {
            *v = Visibility::Hidden;
        }
    }
    if open {
        if let Some(e) = windows.iter().find(|(_, w, _, _, _)| w.0 == name).map(|(e, _, _, _, _)| e) { raise(e, windows); }
    }
}

pub(super) fn keyboard(keys: Res<ButtonInput<KeyCode>>, mut windows: Windows, mut pointer: ResMut<Pointer>, mut items: ResMut<items::Interaction>,
    cameras: Query<Has<crate::diagnostics::debug::DetachedCamera>, With<crate::FreeCamera>>, mut players: Query<&mut crate::network::net::LocalPlayer>, mut dialogue: ResMut<crate::gameplay::dialogue::Dialogue>,
    mut settings: ResMut<settings::Settings>) {
    if let Some(name) = pointer.open_window.take() { toggle(name, &mut windows); }
    if settings.keyboard(&keys) { return; }
    if keys.just_pressed(KeyCode::Escape) && items.cancel_drag() { pointer.down = None; return; }
    if keys.just_pressed(KeyCode::Escape) && pointer.spell_drag.take().is_some() {
        pointer.down = None;
        return;
    }
    if let Some((root, id)) = pointer.focus.clone() {
        if keys.just_pressed(KeyCode::Escape) {
            pointer.focus = None;
            // Escape cancels the chat line.
            if let Ok((_, w, mut c, _, _)) = windows.get_mut(root) {
                if w.0 == CHAT_WINDOW { c.entries.insert(id, TextEntry::default()); }
            }
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        if dialogue.target.is_some() {
            dialogue.close();
            for mut player in &mut players { player.target = None; }
            for (_, name, _, mut visibility, _) in &mut windows {
                if super::dialogue::WINDOWS.contains(&name.0.as_str()) { *visibility = Visibility::Hidden; }
            }
            return;
        }
        // The map overlay (Tab) stays: Escape is not its key.
        // FAILURE (2026-10-17): "dans la mini map quand je fais echap ca la ferme
        //   alors que ca devrait pas".
        let top = windows.iter().filter(|(_, w, _, v, _)| *v != Visibility::Hidden && is_menu(&w.0) && w.0 != "locationmap")
            .max_by(|a, b| a.4.translation.z.total_cmp(&b.4.translation.z)).map(|(e, _, _, _, _)| e);
        if let Some(top) = top {
            if let Ok((_, _, _, mut v, _)) = windows.get_mut(top) { *v = Visibility::Hidden; }
        } else { toggle("gamemenu", &mut windows); }
        return;
    }
    // The viewer and the detached debug camera own their movement keys.
    // Keep raw input available to the camera and other UI shortcuts working.
    let free_fly = cameras.iter().any(|detached| detached || players.is_empty());
    for name in MENU_WINDOWS.iter().map(|w| w.0).chain(NAV_WINDOWS) {
        if name == "gamemenu" || name == "locationmap" { continue; }
        if free_fly && settings.current.bindings.get(name).is_some_and(|k| matches!(k.as_str(), "KeyW" | "KeyA" | "KeyS" | "KeyD" | "KeyQ" | "KeyE")) { continue; }
        if settings.current.pressed(name, &keys) { toggle(name, &mut windows); }
    }
    if settings.current.pressed("locationmap", &keys) { toggle("locationmap", &mut windows); }
    // V switches between the expanded controls and a click-through preview.
    if settings.current.pressed("chat", &keys) {
        let compact = windows.iter().any(|(_, w, c, _, _)| w.0 == CHAT_WINDOW && !c.chat_compact);
        set_chat_compact(compact, &mut windows, &mut pointer);
    }
}

fn set_chat_compact(compact: bool, windows: &mut Windows, pointer: &mut Pointer) {
    for (root, w, mut c, mut v, _) in windows.iter_mut() {
        if w.0 == CHAT_WINDOW {
            c.chat_compact = compact;
            c.hovered = None;
            c.pressed = None;
            *v = Visibility::Inherited;
            if compact && pointer.focus.as_ref().is_some_and(|(e, _)| *e == root) { pointer.focus = None; }
        }
        if w.0 == CHAT_MENU { *v = Visibility::Hidden; }
    }
    pointer.down = None;
}

/// Availability follows server joins/leaves, including disconnects and late UI loads.
pub(super) fn sync_chat(log: Res<crate::network::chat::ChatLog>, mut windows: Query<(&Window2018, &mut WindowControls)>) {
    let available = std::array::from_fn(|i| log.available(crate::network::chat::Kind::channel(i as u8)));
    for (w, mut c) in &mut windows {
        if !matches!(w.0.as_str(), CHAT_WINDOW | CHAT_MENU) { continue; }
        if w.0 == CHAT_WINDOW {
            let selected = c.pages.get("chat_channel").copied().unwrap_or(0);
            // Keep the default map selection while its initial join is pending.
            let was_available = c.chat_channels.get(selected as usize).copied().unwrap_or(false);
            if !available.get(selected as usize).copied().unwrap_or(false) && (selected != 0 || was_available) {
                let fallback = available.iter().position(|joined| *joined).unwrap_or(0) as u8;
                if selected != fallback { c.pages.insert("chat_channel", fallback); }
            }
        }
        if c.chat_channels != available { c.chat_channels = available; }
    }
}

/// The chat window's TextEntry, when the window is open.
fn chat_entry(windows: &Windows) -> Option<(Entity, String)> {
    windows.iter().filter(|(_, w, _, v, _)| w.0 == CHAT_WINDOW && *v != Visibility::Hidden).find_map(|(e, _, c, _, _)|
        c.buttons.iter().find(|b| matches!(b.action, Action::Edit(_)) && c.section_visible(b.section)).map(|b| (e, b.id.clone())))
}

/// The chat log in the window's rows: the newest at the bottom, colour by channel.
pub(super) fn chat_rows(
    mut log: ResMut<crate::network::chat::ChatLog>,
    mut rows: Query<(&ChatRow, &mut Text2d, &mut TextColor)>,
    added: Query<(), Added<ChatRow>>,
    mut windows: Query<(&Window2018, &mut WindowControls)>,
) {
    // The compact preview fades its lines out (ChatMessageLogHideTime): it is
    // redrawn every frame while one of them is still shown.
    let fading = log.lines.iter().any(|l| l.at.elapsed().as_secs_f32() < crate::network::chat::HIDE_AFTER + 0.1);
    if !log.is_changed() && added.is_empty() && !fading { return; }
    let count = rows.iter().filter(|(row, _, _)| !row.compact).count();
    let compact_count = rows.iter().filter(|(row, _, _)| row.compact).count();
    let log = log.bypass_change_detection();
    let shown = log.rows(count, CHAT_ROW_WIDTH, CHAT_FONT_SIZE);
    let recent = log.recent_rows(compact_count, CHAT_ROW_WIDTH, CHAT_FONT_SIZE);
    let first = count - shown.len();
    for (row, mut text, mut colour) in &mut rows {
        let (kind, line, alpha) = if row.compact {
            row.index.checked_sub(compact_count - recent.len()).and_then(|i| recent.get(i)).cloned().unwrap_or_default()
        } else {
            row.index.checked_sub(first).and_then(|i| shown.get(i)).cloned().map(|(k, l)| (k, l, 1.0)).unwrap_or_default()
        };
        if text.0 != line { text.0 = line; }
        let c = kind.colour().with_alpha(alpha);
        if colour.0 != c { colour.0 = c; }
    }
    let scrolled = u8::from(log.scroll > 0);
    for (w, mut c) in &mut windows {
        if w.0 == CHAT_WINDOW && c.pages.get("chat_scrolled") != Some(&scrolled) { c.pages.insert("chat_scrolled", scrolled); }
    }
}

pub(super) fn interact(
    mut windows: Windows,
    mut primary: Query<&mut Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut pointer: ResMut<Pointer>,
    mut skill_clicks: ResMut<SkillClicks>,
    mut respawn: ResMut<crate::network::net::RespawnChoice>,
    panels: Query<(&RelativeCursorPosition, &InheritedVisibility), With<crate::ui::input::BlocksWorldInput>>,
    mut chat: ResMut<crate::network::chat::ChatLog>,
    mut dialogue: ResMut<crate::gameplay::dialogue::Dialogue>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut social: ResMut<social::SocialUi>,
    mut settings: ResMut<settings::Settings>,
    mut hotbar: ResMut<hotbar::Hotbar>,
    mut audio: Option<ResMut<crate::audio::AudioRequests>>,
    (mut quest_ui, mut party, views): (ResMut<crate::gameplay::quests::QuestUi>, ResMut<party::Party>, Res<systems::Views>),
) {
    let Ok(mut window) = primary.single_mut() else { return };
    let blocked = panels.iter().any(|(cursor, visible)| visible.get() && cursor.cursor_over());
    let point = window.cursor_position().filter(|_| window.focused && window.height() > 0.0)
        .map(|p| p * (BASE_H / window.height()));
    let width = BASE_H * window.width() / window.height().max(1.0);
    let modal = windows.iter().filter(|(_, w, _, v, _)| matches!(w.0.as_str(), "gamemenu" | "unstackwindow" | items::DESTROY_DIALOG | settings::WINDOW) && *v != Visibility::Hidden)
        .max_by(|a, b| a.4.translation.z.total_cmp(&b.4.translation.z)).map(|(e, _, _, _, _)| e);
    if !window.focused || mouse.just_pressed(MouseButton::Left) { pointer.focus = None; }

    if !window.focused || mouse.just_pressed(MouseButton::Left) {
        pointer.drag = None;
    }
    if pointer.drag.as_ref().is_some_and(|d| !windows.get(d.root).is_ok_and(|(_, _, _, v, _)| *v != Visibility::Hidden) || modal.is_some_and(|m| m != d.root)) {
        pointer.drag = None;
    }
    if mouse.pressed(MouseButton::Left) || mouse.just_released(MouseButton::Left) {
        if let (Some(drag), Some(p)) = (&mut pointer.drag, point) {
            if let Ok((_, _, controls, _, mut tf)) = windows.get_mut(drag.root) {
                if let Some(handle) = controls.drag_handle {
                    drag.grab.x += handle.k * (width - drag.width);
                    drag.width = width;
                    let offset = handle.clamp_offset(p - drag.grab, width);
                    tf.translation.x = offset.x;
                    tf.translation.y = -offset.y;
                }
            }
        }
    }
    if !mouse.pressed(MouseButton::Left) { pointer.drag = None; }
    for (_, _, controls, _, mut tf) in &mut windows {
        if let Some(handle) = controls.drag_handle {
            let offset = handle.clamp_offset(window_offset(&tf), width);
            if offset != window_offset(&tf) {
                tf.translation.x = offset.x;
                tf.translation.y = -offset.y;
            }
        }
    }
    // The wheel over the chat's log scrolls it (up: older lines), a row a notch.
    let over_log = point.is_some_and(|p| windows.iter().any(|(_, w, c, v, tf)| w.0 == CHAT_WINDOW && *v != Visibility::Hidden
        && !blocked && c.title_hit(p - window_offset(tf), width)));
    for w in wheel.read() {
        if !over_log { continue; }
        let notches = if w.unit == bevy::input::mouse::MouseScrollUnit::Line { w.y } else { w.y / 40.0 };
        let rows = notches.round() as i32;
        if rows != 0 { chat.scroll = (chat.scroll as i32 + rows).max(0) as usize; }
    }
    // The chat's channel menu opens by its ButtonChatMenu wherever the chat was moved.
    let chat_at = windows.iter().find(|(_, w, _, _, _)| w.0 == CHAT_WINDOW).map(|(_, _, _, _, tf)| tf.translation.truncate());
    if let Some(at) = chat_at {
        for (_, w, _, _, mut tf) in &mut windows {
            if w.0 == CHAT_MENU && tf.translation.truncate() != at {
                tf.translation.x = at.x;
                tf.translation.y = at.y;
            }
        }
    }

    let top = point.filter(|_| !blocked).and_then(|p| windows.iter()
        .filter(|(e, _, controls, visibility, tf)| *visibility != Visibility::Hidden && modal.is_none_or(|m| m == *e) && controls.covers(p - window_offset(tf), width))
        .max_by(|a, b| a.4.translation.z.total_cmp(&b.4.translation.z))
        .map(|(e, _, c, _, tf)| (e, c.hit(p - window_offset(tf), width), c.title_hit(p - window_offset(tf), width))));
    if mouse.just_pressed(MouseButton::Left) {
        currency::dismiss(&mut windows, top.map(|(e, button, _)| (e, button)));
        if let Some((e, button, title)) = top {
            raise(e, &mut windows);
            if title && button.is_none() {
                let (_, _, _, _, tf) = windows.get(e).unwrap();
                pointer.drag = Some(Drag { root: e, grab: point.unwrap() - window_offset(tf), width });
            }
        }
    }
    let hovered = top.filter(|_| pointer.drag.is_none()).and_then(|(e, b, _)| b.map(|i| (e, i)));
    let hit = hovered.filter(|(e, index)| windows.get(*e).is_ok_and(|(_, _, c, _, _)| c.enabled(&c.buttons[*index])));
    if !window.focused || blocked || pointer.down.is_some_and(|(e, _)| !windows.get(e).is_ok_and(|(_, _, _, v, _)| *v != Visibility::Hidden)) { pointer.down = None; }
    let mut clicked = pointer.update(hit, mouse.just_pressed(MouseButton::Left), mouse.just_released(MouseButton::Left));
    // A right click on an album sticker uses it (systems::right_clicked).
    if mouse.just_pressed(MouseButton::Right) {
        if let Some(request) = hit.and_then(|(e, i)| windows.get(e).ok().and_then(|(_, _, c, _, _)| systems::right_clicked(&c.buttons[i].action))) {
            info!("ui right click: {request:?}");
            pointer.system.push(request);
        }
    }
    if spells::drag(&mut pointer, &windows, &mouse, hit, top.map(|(e, _, _)| e), point, window.focused && !blocked && modal.is_none()) {
        clicked = None;
    }
    if stickers::drag(&mut pointer, &windows, &views, &mouse, hit, top.map(|(e, _, _)| e), point, window.focused && !blocked && modal.is_none()) {
        clicked = None;
    }
    if settings::drag_slider(&mut pointer, &windows, &mouse, point, width, window.focused && !blocked, &mut settings) {
        clicked = None;
    }
    // A DragRotate button held, even once the cursor leaves it: its sideways
    // movement turns the window's preview (crate::ui::preview::turn).
    let turning = pointer.down.filter(|(e, i)| mouse.pressed(MouseButton::Left)
        && windows.get(*e).is_ok_and(|(_, _, c, _, _)| c.buttons.get(*i).is_some_and(|b| b.action == Action::DragRotate)));
    match (turning, point) {
        (Some((root, _)), Some(p)) => {
            if let Some((_, last)) = pointer.turn.filter(|(r, _)| *r == root) {
                let moved = pointer.turned.filter(|(r, _)| *r == root).map_or(0.0, |(_, d)| d) + p.x - last;
                if moved != 0.0 { pointer.turned = Some((root, moved)); }
            }
            pointer.turn = Some((root, p.x));
        }
        _ => pointer.turn = None,
    }
    if !mouse.pressed(MouseButton::Left) { pointer.down = None; }
    for (e, _, mut controls, visibility, _) in &mut windows {
        controls.hovered = hovered.filter(|(root, _)| *root == e).map(|(_, i)| i);
        controls.pressed = pointer.down.filter(|(root, _)| *root == e && *visibility != Visibility::Hidden).map(|(_, i)| i);
    }
    let Some((entity, index)) = clicked else { return };
    let Ok((_, name, mut controls, mut visibility, _)) = windows.get_mut(entity) else { return };
    let action = controls.buttons[index].action.clone();
    // Letting go of a turned preview is no click.
    if action == Action::DragRotate { return; }
    if let Some(audio) = audio.as_mut() {
        audio.ui(if matches!(action, Action::Close) { "ui/general/ui_close_screen" } else { "ui/general/ui_click" });
    }
    info!("ui {}: {} ({action:?})", name.0, controls.buttons[index].id);
    match action {
        Action::Settings(event) => {
            if event == "open" {
                settings.begin();
                *visibility = Visibility::Hidden;
                toggle(settings::WINDOW, &mut windows);
            } else { settings.action(&event); }
        }
        Action::Bar(delta) => hotbar.change = delta,
        Action::Essence => hotbar.swap_essence = true,
        Action::NpcChoice(index) => dialogue.choice = Some(index),
        Action::Item(_) | Action::SplitSlider => {},
        Action::Split(delta) => pointer.item_command = Some(delta),
        Action::Destroy(confirm) => pointer.destroy = Some(confirm),
        Action::LogoutAnswer(answer) => pointer.logout_answer = Some(answer),
        Action::Close => *visibility = Visibility::Hidden,
        Action::Inventory(page) => controls.inventory = page,
        Action::Character(page) => controls.character = page,
        Action::Tab(group, page) => {
            controls.pages.insert(group, page);
            if group == "rank_group" { controls.pages.insert("rank_category", 0); }
            if group == "preset" { pointer.preset = Some((page, controls.pages.get("talents").copied().unwrap_or(0))); }
        }
        Action::Toggle(id) => { if !controls.toggles.remove(&id) { controls.toggles.insert(id); } }
        Action::Edit(id) => { controls.entries.entry(id.clone()).or_default(); pointer.focus = Some((entity, id)); }
        Action::Bag(page) => controls.bag = page,
        Action::Ingredient(page) => controls.ingredient = page,
        Action::Rarity(rarity) => controls.rarity = rarity,
        Action::Page(delta) => controls.overflow_page = (controls.overflow_page as i32 + delta).clamp(0, controls.overflow_pages.saturating_sub(1) as i32) as usize,
        Action::Lock => { controls.locked = !controls.locked; settings.current.lock_bar = controls.locked; }
        Action::Skill(slot) => skill_clicks.0 = Some(slot),
        Action::Fullscreen => {
            settings.current.fullscreen = window.mode == WindowMode::Windowed;
            window.mode = if settings.current.fullscreen { WindowMode::BorderlessFullscreen(MonitorSelection::Current) } else { WindowMode::Windowed };
        },
        Action::Window(target) => toggle(target, &mut windows),
        Action::Membership => {
            for (_, w, mut c, mut v, _) in &mut windows {
                if w.0 == "charactersheet" { c.character = 3; *v = Visibility::Inherited; }
                if w.0 == "skilltalentswindow" { *v = Visibility::Hidden; }
            }
            if let Some(e) = windows.iter().find(|(_, w, _, _, _)| w.0 == "charactersheet").map(|(e, _, _, _, _)| e) { raise(e, &mut windows); }
        }
        Action::ChatScroll(0) => chat.scroll = 0,
        Action::ChatScroll(rows) => chat.scroll = (chat.scroll as i32 + rows).max(0) as usize,
        Action::ChatCompact(compact) => set_chat_compact(compact, &mut windows, &mut pointer),
        Action::ChatChannel(channel) => {
            *visibility = Visibility::Hidden;
            for (_, w, mut c, _, _) in &mut windows {
                if w.0 == CHAT_WINDOW { c.pages.insert("chat_channel", channel); }
            }
        }
        Action::Spell | Action::Unavailable | Action::DragRotate => {}
        Action::Social(clicked) => social.clicked = Some(clicked),
        Action::QuestSelect(id) => quest_ui.selected = Some(id),
        Action::QuestCancel(id) => quest_ui.cancel = Some(id),
        Action::QuestTrack(delta) => quest_ui.tracked = (quest_ui.tracked as i32 + delta).max(0) as usize,
        Action::QuestGroup(kind) => quests::fold(&mut quest_ui.folded, &kind),
        Action::QuestCompact(compact) => quest_ui.compact = compact,
        Action::PartyCompact(compact) => party.compact = compact,
        Action::Logout => { pointer.logout = true; *visibility = Visibility::Hidden; }
        Action::Talent(id) => pointer.talent = Some(id),
        Action::Travel(map) => { pointer.travel = Some(map); *visibility = Visibility::Hidden; }
        // A social window row: the player menu, where the cursor is (social::context_menu).
        Action::System(systems::Request::Player(name)) => { social.target = Some(name); social.menu_at_cursor = true; }
        Action::System(request) => { let request = systems::clicked(request, &mut controls); pointer.system.push(request); }
        Action::Respawn(at_exit, on_spot) => {
            respawn.0 = Some((at_exit, on_spot));
            *visibility = Visibility::Hidden;
        }
        Action::Currency(id) => {
            settings.pick_currency(&id);
            *visibility = Visibility::Hidden;
        }
    }
}

pub(super) fn edit_text(
    mut events: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut pointer: ResMut<Pointer>,
    mut windows: Windows,
    mut input: ResMut<crate::ui::input::UiInput>,
    mut chat: ResMut<crate::network::chat::ChatLog>,
) {
    // A tool window (crate::diagnostics::lighting_menu) has the keyboard: its
    // Enter must not open the chat.
    if input.hold_keys {
        events.clear();
        return;
    }
    if pointer.focus.as_ref().is_some_and(|(e, id)| !windows.get(*e).is_ok_and(|(_, _, c, v, _)| *v != Visibility::Hidden
        && c.buttons.iter().any(|b| b.id == *id && c.section_visible(b.section)))) { pointer.focus = None; }
    for event in events.read() {
        if !event.state.is_pressed() { continue; }
        let enter = matches!(event.key_code, KeyCode::Enter | KeyCode::NumpadEnter);
        let Some((root, id)) = pointer.focus.clone() else {
            // Enter with the chat window open: type a line.
            if enter && !event.repeat {
                set_chat_compact(false, &mut windows, &mut pointer);
                pointer.focus = chat_entry(&windows);
            }
            continue;
        };
        if enter || event.key_code == KeyCode::Escape {
            pointer.focus = None;
            // Enter sends the chat line, on the window's channel; in the social
            // window's search panel, it searches.
            if let (true, Ok((_, w, mut c, _, _))) = (enter, windows.get_mut(root)) {
                if w.0 == CHAT_WINDOW {
                    let typed = c.entries.insert(id, TextEntry::default()).map(|e| e.text).unwrap_or_default();
                    chat.submit(crate::network::chat::Kind::channel(c.pages.get("chat_channel").copied().unwrap_or(0)), &typed);
                } else if w.0 == systems::SOCIAL {
                    pointer.system.push(systems::Request::SocialSearch);
                }
            }
            continue;
        }
        if let Ok((_, _, mut c, _, _)) = windows.get_mut(root) {
            c.entries.entry(id).or_default().edit(event.key_code, event.text.as_deref(),
                keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight));
        }
    }
    for (e, w, mut c, _, _) in &mut windows {
        if w.0 != CHAT_WINDOW { continue; }
        // A started line keeps its text when the chat loses the focus; the hint
        // ("Press enter to open the chat") only covers an empty entry.
        // FAILURE (2026-10-17): clicking away while typing showed the hint over
        // the half-written line.
        let draft = c.entries.values().any(|t| !t.text.is_empty());
        let typing = u8::from(pointer.focus.as_ref().is_some_and(|(root, _)| *root == e) || draft);
        if c.pages.get("chat_typing") != Some(&typing) { c.pages.insert("chat_typing", typing); }
    }
    input.typing = pointer.focus.is_some() || input.hold_keys;
}

pub(super) fn paint(
    mut windows: Query<(&mut WindowControls, &Window2018, &Visibility, &mut crate::ui::input::UiRegions, &Transform), Without<Element>>,
    mut elements: Query<(&Element, &mut Visibility, Option<&Value>, Option<&EntryLabel>, Option<&mut Text2d>), Without<Window2018>>,
    pointer: Res<Pointer>,
) {
    let open: Vec<_> = windows.iter().filter(|(_, _, v, _, _)| **v != Visibility::Hidden).map(|(_, name, _, _, _)| name.0.clone()).collect();
    for (mut controls, _, _, _, _) in &mut windows {
        if controls.open_windows != open { controls.open_windows.clone_from(&open); }
    }
    for (element, mut visibility, value, entry, text) in &mut elements {
        let Ok((controls, name, _, _, _)) = windows.get(element.root) else { continue };
        let visible = controls.visible(&element.gate);
        let want = if visible { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != want { *visibility = want; }
        if let Some(mut text) = text {
            if let Some(EntryLabel(id)) = entry {
                if let Some(entry) = controls.entries.get(id) {
                    let mut display = entry.text.clone();
                    if pointer.focus.as_ref().is_some_and(|(e, focused)| *e == element.root && focused == id) { display.insert(entry.cursor, '|'); }
                    if text.0 != display { text.0 = display; }
                }
            } else if matches!(value, Some(Value(ValueKind::Page))) {
                let page = if name.0 == "inventory" { format!("{}/{}", controls.overflow_page + 1, controls.overflow_pages) } else { "1/1".to_owned() };
                if text.0 != page { text.0 = page; }
            }
        }
    }
    for (controls, name, visibility, mut regions, tf) in &mut windows {
        regions.0.clear();
        if *visibility == Visibility::Hidden { continue; }
        let offset = window_offset(tf);
        regions.0.extend(controls.regions.iter().filter(|(_, _, gate)| controls.visible(gate)).map(|(k, rect, _)| (*k, translated(*rect, offset))));
        regions.0.extend(controls.buttons.iter().filter(|b| controls.section_visible(b.section)).map(|b| (b.k, translated(b.rect, offset))));
        if let Some(h) = controls.drag_handle { regions.0.push((h.k, translated(h.rect, offset))); }
        if matches!(name.0.as_str(), "gamemenu" | "unstackwindow" | items::DESTROY_DIALOG) { regions.0.push((0.0, [-1.0e6, -1.0e6, 1.0e6, 1.0e6])); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(window: &str, widgets: serde_json::Value) -> Layout {
        serde_json::from_value(serde_json::json!({ "window": window, "widgets": widgets })).unwrap()
    }

    fn button(id: &str, event: &str) -> Widget {
        serde_json::from_value(serde_json::json!({"id":id,"type":"Button","event":event,"k":0.5,"rect":[0,0,20,20],"hidden":false})).unwrap()
    }

    /// The 3D previews' places (crate::ui::preview): the mercenary page's
    /// Shadow Soul or its empty placeholder; the album's sticker turning, with
    /// its DragRotate buttons, or its icon.
    #[test]
    fn the_preview_areas_show_their_model_or_its_placeholder() {
        let mut inventory = WindowControls::new(&fixture("inventory", serde_json::json!([])));
        let visible = |c: &WindowControls, window: &str, id: &str| c.section_visible(section(window, id));
        let page = "Canvas/PositioningLabel/Label_texture/mercenary/label_empty/deco_verlauf_character";
        let (figure, empty) = (format!("{page}/figur"), format!("{page}/empty"));
        let (figure, empty) = (figure.as_str(), empty.as_str());
        assert!(!visible(&inventory, "inventory", figure) && !visible(&inventory, "inventory", empty), "another page");
        inventory.inventory = 3;
        assert!(!visible(&inventory, "inventory", figure) && visible(&inventory, "inventory", empty), "no Shadow Soul");
        inventory.set_page("shadow_soul", 1);
        assert!(visible(&inventory, "inventory", figure) && !visible(&inventory, "inventory", empty));
        let mut album = WindowControls::new(&fixture(systems::ALBUM, serde_json::json!([])));
        let image = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/Categorypage/background_preview/previewBackground/background_image";
        let icon = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/Categorypage/background_preview/Sticker/Image";
        let drag = format!("{image}/DragRotateAreaLabel/DragRotateAreaButton01");
        assert!(visible(&album, systems::ALBUM, icon) && !visible(&album, systems::ALBUM, systems::album::TURNING) && !visible(&album, systems::ALBUM, &drag));
        album.set_page(super::super::preview::ALBUM_MODEL, 1);
        assert!(!visible(&album, systems::ALBUM, icon) && visible(&album, systems::ALBUM, systems::album::TURNING) && visible(&album, systems::ALBUM, &drag));
        assert!(visible(&album, systems::ALBUM, image), "the box itself shows either way");
        assert_eq!(action(systems::ALBUM, &button(&drag, "DragRotate")), Action::DragRotate);
    }

    #[test]
    fn click_requires_the_same_press_and_release_target() {
        let mut pointer = Pointer::default();
        let a = Some((Entity::from_bits(1), 0));
        let b = Some((Entity::from_bits(1), 1));
        assert_eq!(pointer.update(a, true, false), None);
        assert_eq!(pointer.update(b, false, true), None);
        pointer.update(None, true, false);
        assert_eq!(pointer.update(a, false, true), None);
        pointer.update(a, true, false);
        assert_eq!(pointer.update(a, false, true), a);
    }

    #[test]
    fn inventory_pages_and_return_are_mutually_exclusive() {
        let mut c = WindowControls::new(&fixture("inventory", serde_json::json!([])));
        for selected in 0..4 {
            c.inventory = selected;
            for (name, page) in [("oben", 0), ("IngredientBag", 1), ("TrinketBag", 2), ("mercenary", 3)] {
                let gate = section("inventory", &format!("Canvas/PositioningLabel/Label_texture/{name}/normal"));
                assert_eq!(c.section_visible(gate), page == selected);
            }
            assert_eq!(c.section_visible(Section::InventoryReturn), selected != 0);
        }
    }

    #[test]
    fn hover_press_disabled_and_release_use_authored_states() {
        let mut c = WindowControls::new(&fixture("inventory", serde_json::json!([])));
        c.buttons.push(Button { id: "Close".into(), k: 0.0, rect: [0.0; 4], section: Section::Always,
            action: Action::Close, authored_enabled: true, states: 31, spell: None, depth: 0.0 });
        assert_eq!(c.state(0), State::Normal);
        c.hovered = Some(0);
        assert_eq!(c.state(0), State::Hover);
        c.pressed = Some(0);
        assert_eq!(c.state(0), State::HoverPressed);
        c.buttons[0].states &= !State::HoverPressed.bit();
        assert_eq!(c.state(0), State::Pressed);
        c.hovered = None;
        assert_eq!(c.state(0), State::Normal);
        c.buttons[0].authored_enabled = false;
        assert_eq!(c.state(0), State::Disabled);
    }

    #[test]
    fn events_and_identical_tab_names_resolve_in_their_own_window() {
        assert_eq!(action("inventory", &button("Canvas/IngredientBag/BagButton2", "")), Action::Ingredient(2));
        assert_eq!(action("inventory", &button("Canvas/unten/BagButton2", "")), Action::Bag(2));
        assert_eq!(action("inventory", &button("Canvas/Frame/Button", "Close")), Action::Close);
        assert_eq!(action("gamebar", &button("Canvas/gamemenu", "Logout")), Action::Window("gamemenu"));
        assert_eq!(action("inventory", &button("Canvas/SortInventoryButton", "SortInventory")), Action::Unavailable);
    }

    #[test]
    fn free_flight_reserves_wasd_and_keeps_other_shortcuts() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Pointer>()
            .init_resource::<settings::Settings>()
            .init_resource::<hotbar::Hotbar>()
            .init_resource::<items::Interaction>()
            .init_resource::<crate::gameplay::dialogue::Dialogue>()
            .add_systems(Update, keyboard);
        let camera = app.world_mut().spawn(crate::FreeCamera::default()).id();
        let mut roots = HashMap::new();
        for (name, z) in MENU_WINDOWS.map(|(name, _, z)| (name, z)).into_iter().chain([("locationmap", 80.0)]) {
            let controls = WindowControls::new(&fixture(name, serde_json::json!([])));
            roots.insert(name, app.world_mut().spawn((Window2018(name.into()), controls,
                Visibility::Hidden, Transform::from_xyz(0.0, 0.0, z))).id());
        }
        let press = |app: &mut App, key| {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
            app.update();
            assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(key), "camera input must remain available");
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset(key);
        };
        // Movement must neither open a closed window nor close one opened by mouse.
        for visible in [Visibility::Hidden, Visibility::Inherited] {
            for name in ["collectorsalbum", "skilltalentswindow"] {
                *app.world_mut().get_mut::<Visibility>(roots[name]).unwrap() = visible;
            }
            for key in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD] {
                press(&mut app, key);
                for (name, root) in &roots {
                    let expected = if matches!(*name, "collectorsalbum" | "skilltalentswindow") { visible } else { Visibility::Hidden };
                    assert_eq!(*app.world().get::<Visibility>(*root).unwrap(), expected, "{key:?}: {name}");
                }
            }
        }
        for root in roots.values() { *app.world_mut().get_mut::<Visibility>(*root).unwrap() = Visibility::Hidden; }
        for (key, name) in [(KeyCode::KeyI, "inventory"), (KeyCode::KeyC, "charactersheet")] {
            press(&mut app, key);
            assert_eq!(*app.world().get::<Visibility>(roots[name]).unwrap(), Visibility::Inherited);
            press(&mut app, KeyCode::Escape);
            assert_eq!(*app.world().get::<Visibility>(roots[name]).unwrap(), Visibility::Hidden);
        }
        // The map overlay is Tab's: Escape leaves it open.
        press(&mut app, KeyCode::Tab);
        assert_eq!(*app.world().get::<Visibility>(roots["locationmap"]).unwrap(), Visibility::Inherited);
        press(&mut app, KeyCode::Escape);
        assert_eq!(*app.world().get::<Visibility>(roots["locationmap"]).unwrap(), Visibility::Inherited);
        for root in roots.values() { *app.world_mut().get_mut::<Visibility>(*root).unwrap() = Visibility::Hidden; }
        app.world_mut().entity_mut(camera).remove::<crate::FreeCamera>();
        for (key, name) in [(KeyCode::KeyW, "collectorsalbum"), (KeyCode::KeyS, "skilltalentswindow")] {
            press(&mut app, key);
            assert_eq!(*app.world().get::<Visibility>(roots[name]).unwrap(), Visibility::Inherited);
            press(&mut app, key);
            assert_eq!(*app.world().get::<Visibility>(roots[name]).unwrap(), Visibility::Hidden);
        }
        app.world_mut().entity_mut(camera).insert(crate::FreeCamera::default());
        press(&mut app, KeyCode::KeyW);
        press(&mut app, KeyCode::KeyS);
        assert_eq!(*app.world().get::<Visibility>(roots["collectorsalbum"]).unwrap(), Visibility::Hidden);
        assert_eq!(*app.world().get::<Visibility>(roots["skilltalentswindow"]).unwrap(), Visibility::Hidden);
        // A spawned character also reserves movement keys while its camera is detached.
        app.world_mut().spawn(crate::network::net::LocalPlayer::default());
        app.world_mut().entity_mut(camera).insert(crate::diagnostics::debug::DetachedCamera);
        press(&mut app, KeyCode::KeyW);
        press(&mut app, KeyCode::KeyS);
        assert_eq!(*app.world().get::<Visibility>(roots["collectorsalbum"]).unwrap(), Visibility::Hidden);
        assert_eq!(*app.world().get::<Visibility>(roots["skilltalentswindow"]).unwrap(), Visibility::Hidden);
        app.world_mut().entity_mut(camera).remove::<crate::diagnostics::debug::DetachedCamera>();
        press(&mut app, KeyCode::KeyW);
        assert_eq!(*app.world().get::<Visibility>(roots["collectorsalbum"]).unwrap(), Visibility::Inherited);
    }

    #[test]
    fn hit_regions_follow_the_same_scale_and_anchor_as_artwork() {
        let rect = [600.0, 100.0, 650.0, 150.0];
        let width = 768.0 * 1920.0 / 1080.0;
        assert!(contains(0.5, rect, Vec2::new(625.0 + (width - 1024.0) / 2.0, 125.0), width));
        assert!(!contains(0.5, rect, Vec2::new(625.0, 125.0), width));
    }

    #[test]
    fn the_quest_tracker_leaves_the_game_bar_above_it_to_the_pointer() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Layout>().init_asset::<Mesh>().init_asset::<UiMaterial>()
            .init_asset::<Image>().init_asset::<Font>().init_asset::<crate::ui::dialogue::Catalog>()
            .init_resource::<Loaded>().init_resource::<Screen>().init_resource::<crate::ui::dialogue::Presentation>()
            .init_resource::<party::Party>().init_resource::<crate::gameplay::quests::QuestUi>().init_resource::<systems::Views>().init_resource::<currency::Shown>().init_resource::<message::ItemMessages>().init_resource::<crate::ui::UiWork>()
            .insert_resource(quests::Quests { list: vec![quests::QuestView::default()], ..default() })
            .add_systems(Update, spawn_windows);
        let art = |[l, t, r, b]: [f32; 4], alpha: f32| {
            let color = [1.0, 1.0, 1.0, alpha].repeat(4);
            serde_json::json!({"tex": null, "pos": [l, t, r, t, r, b, l, b], "uv": vec![0.0; 8], "color": color, "idx": [0, 1, 2, 0, 2, 3]})
        };
        // As exported: LabelFull is invisible and spans from the top of the
        // screen; the frame it holds starts under the game bar's buttons.
        let layout: Layout = serde_json::from_value(serde_json::json!({"window": quests::TRACKER, "widgets": [
            {"id": "Canvas/ActiveQuestLabel/LabelFull", "k": 1.0, "rect": [720, 0, 1024, 325], "hidden": false, "art": art([720.0, 0.0, 1024.0, 325.0], 0.0)},
            {"id": "Canvas/ActiveQuestLabel/LabelFull/FrameLabel", "k": 1.0, "rect": [731, 76, 1027, 325], "hidden": false, "art": art([731.0, 76.0, 1027.0, 325.0], 0.5)},
        ]})).unwrap();
        let handle = app.world_mut().resource_mut::<Assets<Layout>>().add(layout);
        app.world_mut().resource_mut::<Loaded>().layouts.push((handle, 46.0, None));
        app.world_mut().resource_mut::<Loaded>().spawned.push(false);
        app.update();
        let controls = app.world_mut().query_filtered::<&WindowControls, With<Window2018>>().single(app.world()).unwrap();
        // gamebar's Premium button (843, 1 .. 886, 44), anchored right as the tracker.
        assert!(!controls.covers(Vec2::new(864.0, 20.0), BASE_W), "the game bar's buttons stay reachable");
        assert!(controls.covers(Vec2::new(850.0, 120.0), BASE_W), "the tracker's frame still takes the pointer");
    }

    #[test]
    fn dragging_a_window_moves_its_gauge_clip_without_changing_the_fill() {
        let mut app = App::new();
        app.init_resource::<Figures>().init_resource::<party::Party>().init_resource::<quests::Quests>().init_resource::<crate::gameplay::quests::QuestUi>().init_resource::<crate::ui::controls::systems::Views>()
            .init_resource::<crate::actors::monsters::HoverInfo>()
            .init_resource::<Screen>()
            .init_resource::<Assets<UiMaterial>>()
            .add_systems(Update, update_fills);
        app.world_mut().resource_mut::<Figures>().xp = 0.25;
        let root = app.world_mut().spawn((Window2018("charactersheet".into()), Transform::default())).id();
        let mat = app.world_mut().resource_mut::<Assets<UiMaterial>>().add(material(None));
        app.world_mut().spawn((Fill { which: Gauge::Xp, k: 0.5, rect: [10.0, 20.0, 110.0, 30.0], shown: -1.0 },
            MeshMaterial2d(mat.clone()), Element { root, gate: Gate::default() }));
        app.update();
        let clip = app.world().resource::<Assets<UiMaterial>>().get(&mat).unwrap().clip;
        assert_eq!(clip.z - clip.x, 25.0);
        app.world_mut().get_mut::<Transform>(root).unwrap().translation = Vec3::new(150.0, -45.0, 0.0);
        app.update();
        assert_eq!(app.world().resource::<Assets<UiMaterial>>().get(&mat).unwrap().clip, clip + Vec4::new(150.0, -45.0, 150.0, -45.0));
    }

    pub(super) fn exported_app() -> (App, Entity, HashMap<&'static str, Entity>, HashMap<&'static str, Layout>) {
        let mut app = App::new();
        app.add_plugins((TransformPlugin, crate::ui::input::UiInputPlugin))
            .add_message::<KeyboardInput>()
            .add_message::<bevy::input::mouse::MouseWheel>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Pointer>()
            .init_resource::<settings::Settings>()
            .init_resource::<hotbar::Hotbar>()
            .init_resource::<items::Interaction>()
            .init_resource::<social::SocialUi>()
            .init_resource::<trade::TradeUi>()
            .init_resource::<SkillClicks>()
            .init_resource::<crate::network::chat::ChatLog>()
            .init_resource::<crate::gameplay::dialogue::Dialogue>()
            .init_resource::<crate::network::net::RespawnChoice>()
            .init_resource::<local_map::MapSettings>()
            .init_resource::<crate::gameplay::quests::QuestUi>().init_resource::<crate::ui::controls::systems::Views>()
            .init_resource::<party::Party>()
            .add_systems(Update, (sync_chat, keyboard, interact, edit_text, order_windows, paint).chain());
        let primary = app.world_mut().spawn((Window { focused: true, resolution: (1024, 768).into(), ..default() }, PrimaryWindow)).id();
        let mut roots = HashMap::new();
        let mut layouts = HashMap::new();
        let definitions = HUD_WINDOWS.into_iter().chain(MENU_WINDOWS.map(|(name, _, z)| (name, z)))
            .chain(NAV_WINDOWS.into_iter().enumerate().map(|(i, name)| (name, if name == "locationmap" { 80.0 } else { 180.0 + i as f32 * WINDOW_DEPTH_STEP })))
            .chain([("unstackwindow", 600.0)]);
        for (name, z) in definitions {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../assets/interface/{name}.ui.json"));
            let layout: Layout = serde_json::from_slice(&std::fs::read(path).expect("export the original UI first")).unwrap();
            let mut c = WindowControls::new(&layout);
            for w in &layout.widgets {
                if !w.hidden && name != "locationmap" {
                    if let Some(a) = &w.art { c.region(w.k, bounds(&a.pos), c.gate(name, w)); }
                }
            }
            let root = app.world_mut().spawn((Window2018(name.into()), c,
                if name == "ingamemenu" { Visibility::Inherited } else { Visibility::Hidden },
                Transform::from_xyz(0.0, 0.0, z), crate::ui::input::UiRegions::default())).id();
            roots.insert(name, root);
            layouts.insert(name, layout);
        }
        (app, primary, roots, layouts)
    }

    #[test]
    fn chat_compact_rows_stay_recent_while_full_chat_is_scrolled() {
        use crate::network::chat::{ChatLog, Kind};
        let mut app = App::new();
        app.init_resource::<ChatLog>().add_systems(Update, chat_rows);
        for i in 0..8 { app.world_mut().resource_mut::<ChatLog>().push(Kind::Map, format!("line {i}")); }
        app.world_mut().resource_mut::<ChatLog>().scroll = 3;
        let full: Vec<_> = (0..3).map(|index| app.world_mut().spawn((ChatRow { index, compact: false }, Text2d::new(""), TextColor::default())).id()).collect();
        let compact: Vec<_> = (0..CHAT_PREVIEW_ROWS).map(|index| app.world_mut().spawn((ChatRow { index, compact: true }, Text2d::new(""), TextColor::default())).id()).collect();
        app.update();
        let shown = |app: &App, rows: &[Entity]| rows.iter().map(|e| app.world().get::<Text2d>(*e).unwrap().0.clone()).collect::<Vec<_>>();
        assert_eq!(shown(&app, &full), ["line 2", "line 3", "line 4"]);
        assert_eq!(shown(&app, &compact), ["line 4", "line 5", "line 6", "line 7"]);
        assert_eq!(app.world().resource::<ChatLog>().scroll, 3);
        app.world_mut().resource_mut::<ChatLog>().push(Kind::Guild, "new message".into());
        app.update();
        assert_eq!(shown(&app, &compact), ["line 5", "line 6", "line 7", "new message"]);
        assert_eq!(app.world().get::<TextColor>(*compact.last().unwrap()).unwrap().0, Kind::Guild.colour());
    }

    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn chat_window_compacts_anchors_and_tracks_available_channels() {
        let (mut app, primary, others, _) = exported_app();
        let mut roots = HashMap::new();
        for (name, z) in [(CHAT_WINDOW, CHAT_DEPTH), (CHAT_MENU, CHAT_DEPTH + 3.0)] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../assets/interface/{name}.ui.json"));
            let layout: Layout = serde_json::from_slice(&std::fs::read(path).expect("export the original UI first")).unwrap();
            let layout = prepare_chat(&layout);
            assert!(layout.widgets.iter().all(|w| w.k == 0.0));
            let mut controls = WindowControls::new(&layout);
            for w in layout.widgets.iter().filter(|w| !w.hidden) {
                if let Some(art) = &w.art {
                    if !passes_pointer(name, w, art) { controls.region(w.k, bounds(&art.pos), controls.gate(name, w)); }
                }
            }
            let root = app.world_mut().spawn((Window2018(name.into()), controls,
                if name == CHAT_WINDOW { Visibility::Inherited } else { Visibility::Hidden },
                Transform::from_xyz(0.0, 0.0, z), crate::ui::input::UiRegions::default())).id();
            roots.insert(name, root);
        }
        let (chat, menu) = (roots[CHAT_WINDOW], roots[CHAT_MENU]);
        app.world_mut().resource_mut::<crate::network::chat::ChatLog>().event(dsor_proto::chat::ChatEvent::LoggedIn);
        app.update();
        assert_eq!(app.world().get::<WindowControls>(chat).unwrap().pages.get("chat_channel").copied().unwrap_or(0), 0, "wait for the map join without switching to whisper");
        {
            let mut log = app.world_mut().resource_mut::<crate::network::chat::ChatLog>();
            log.event(dsor_proto::chat::parse("e#Map__room"));
            log.event(dsor_proto::chat::parse("e#Region__room"));
        }
        let key = |app: &mut App, key_code: KeyCode, text: Option<&str>| {
            let window = primary;
            app.world_mut().write_message(KeyboardInput { key_code, logical_key: bevy::input::keyboard::Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
                state: bevy::input::ButtonState::Pressed, text: text.map(Into::into), repeat: false, window });
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key_code);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset(key_code);
        };
        let shown = |app: &App, e: Entity| *app.world().get::<Visibility>(e).unwrap() != Visibility::Hidden;
        let typing = |app: &App| app.world().resource::<crate::ui::input::UiInput>().typing;
        let sent = |app: &App| app.world().resource::<crate::network::chat::ChatLog>().outbox().to_vec();
        assert!(app.world().get::<WindowControls>(chat).unwrap().chat_compact,"login starts compact");
        key(&mut app, KeyCode::KeyV, Some("v"));
        assert!(!app.world().get::<WindowControls>(chat).unwrap().chat_compact,"V expands the initial preview");
        key(&mut app, KeyCode::KeyV, Some("v"));
        assert!(shown(&app, chat));
        assert!(app.world().get::<WindowControls>(chat).unwrap().chat_compact);
        let controls = app.world().get::<WindowControls>(chat).unwrap();
        let buttons: Vec<_> = controls.buttons.iter().filter(|b| controls.section_visible(b.section)).collect();
        assert_eq!(buttons.len(), 1);
        assert_eq!(buttons[0].action, Action::ChatCompact(false));
        let expand = buttons[0].rect;
        let expand_center = Vec2::new((expand[0]+expand[2])*0.5, (expand[1]+expand[3])*0.5);
        for width in [800.0, 1024.0, 1920.0, 2560.0] {
            assert!(controls.hit(expand_center, width).is_some(), "expand stays at screen left");
            assert!(!controls.covers(Vec2::new(100.0, 550.0), width), "recent text is click-through");
            assert!(!controls.covers(Vec2::new(100.0, 610.0), width), "compact has no entry bar");
        }
        // A tool window holding the keyboard (the Lighting Menu typing a value)
        // keeps its Enter: the chat stays as it was.
        app.world_mut().resource_mut::<crate::ui::input::UiInput>().hold_keys = true;
        key(&mut app, KeyCode::Enter, None);
        assert!(!typing(&app) && app.world().get::<WindowControls>(chat).unwrap().chat_compact, "a tool window's Enter is not the chat's");
        app.world_mut().resource_mut::<crate::ui::input::UiInput>().hold_keys = false;
        key(&mut app, KeyCode::Enter, None);
        assert!(typing(&app), "Enter expands the compact preview and focuses the entry");
        key(&mut app, KeyCode::Escape, None);
        key(&mut app, KeyCode::KeyV, Some("v"));
        click_control(&mut app, primary, chat, "/ButtonToggleMode");
        assert!(!app.world().get::<WindowControls>(chat).unwrap().chat_compact);
        key(&mut app, KeyCode::Enter, None);
        assert!(typing(&app) && !app.world().get::<WindowControls>(chat).unwrap().chat_compact);
        for (code, text) in [(KeyCode::KeyV, "v"), (KeyCode::Digit1, "!"), (KeyCode::KeyI, "i")] {
            key(&mut app, code, Some(text));
        }
        assert!(shown(&app, chat) && !shown(&app, others["inventory"]), "keys typed, not acted on");
        key(&mut app, KeyCode::Enter, None);
        assert!(!typing(&app));
        assert_eq!(sent(&app), vec!["sm#map#en#ha#v!i"]);
        // The region tab, then the channel menu's whisper entry.
        click_control(&mut app, primary, chat, "/ChannelTabRegion");
        key(&mut app, KeyCode::Enter, None);
        key(&mut app, KeyCode::KeyA, Some("a"));
        key(&mut app, KeyCode::Enter, None);
        assert_eq!(sent(&app)[1], "sm#region#en#ha#a");
        click_control(&mut app, primary, chat, "/ButtonChatMenu");
        assert!(shown(&app, menu));
        click_control(&mut app, primary, menu, "/TextButtonWhisper");
        assert!(!shown(&app, menu));
        assert_eq!(app.world().get::<WindowControls>(chat).unwrap().pages.get("chat_channel"), Some(&4));
        // Escape cancels the line, and does not open the game menu.
        key(&mut app, KeyCode::Enter, None);
        key(&mut app, KeyCode::KeyX, Some("x"));
        key(&mut app, KeyCode::Escape, None);
        assert!(!typing(&app) && sent(&app).len() == 2);
        assert!(!shown(&app, others["gamemenu"]));
        key(&mut app, KeyCode::Enter, None);
        key(&mut app, KeyCode::Enter, None);
        assert_eq!(sent(&app).len(), 2, "the cancelled text is gone");

        // Unjoined channels use disabled artwork and cannot change the selection.
        for (suffix, channel) in [("/ChannelTabParty", 2), ("/ChannelTabGuild", 3)] {
            let c = app.world().get::<WindowControls>(chat).unwrap();
            let index = c.buttons.iter().position(|b| b.id.ends_with(suffix)).unwrap();
            assert_eq!(c.state(index), State::Disabled);
            click_control(&mut app, primary, chat, suffix);
            assert_ne!(app.world().get::<WindowControls>(chat).unwrap().pages.get("chat_channel"), Some(&channel));
        }
        click_control(&mut app, primary, chat, "/ButtonChatMenu");
        click_control(&mut app, primary, menu, "/TextButtonParty");
        assert!(shown(&app, menu), "disabled menu choice does not activate");
        key(&mut app, KeyCode::KeyV, Some("v"));
        assert!(!shown(&app, menu), "compacting closes the channel menu");
        click_control(&mut app, primary, chat, "/ButtonToggleMode");

        app.world_mut().resource_mut::<crate::network::chat::ChatLog>().event(dsor_proto::chat::parse("e#Group__room"));
        app.update();
        click_control(&mut app, primary, chat, "/ChannelTabParty");
        assert_eq!(app.world().get::<WindowControls>(chat).unwrap().pages.get("chat_channel"), Some(&2));
        app.world_mut().resource_mut::<crate::network::chat::ChatLog>().event(dsor_proto::chat::parse("l#group"));
        app.update();
        assert_eq!(app.world().get::<WindowControls>(chat).unwrap().pages.get("chat_channel"), Some(&0));

        // Each button visual state contains five alternatives: exactly one renders.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/interface/chatwindow2.ui.json");
        let layout: Layout = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let mut c = app.world_mut().get_mut::<WindowControls>(chat).unwrap();
        let button = c.buttons.iter().position(|b| b.id.contains("LabelFull/") && b.id.ends_with("/ButtonChatMenu")).unwrap();
        for channel in 0..5 {
            c.pages.insert("chat_channel", channel);
            for hovered in [None, Some(button)] {
                for pressed in [None, Some(button)] {
                    c.hovered = hovered;
                    c.pressed = pressed;
                    let icons: Vec<_> = layout.widgets.iter().filter(|w| w.id.contains("/ButtonChatMenu/") && w.id.contains("/IconLabel") && c.visible(&c.gate(CHAT_WINDOW, w))).collect();
                    assert_eq!(icons.len(), 1, "one current channel icon in every visual state");
                    assert!(icons[0].id.ends_with(["Map", "Region", "Party", "Guild", "Whisper"][channel as usize]));
                }
            }
        }
    }

    fn click_control(app: &mut App, primary: Entity, root: Entity, suffix: &str) {
        let controls = app.world().get::<WindowControls>(root).unwrap();
        let b = controls.buttons.iter().find(|b| b.id.ends_with(suffix) && controls.section_visible(b.section)).expect(suffix);
        let (k, [l, t, r, bottom]) = (b.k, b.rect);
        let offset = window_offset(app.world().get::<Transform>(root).unwrap());
        let mut window = app.world_mut().get_mut::<Window>(primary).unwrap();
        let scale = window.height() / BASE_H;
        let width = window.width() / scale;
        window.set_cursor_position(Some((Vec2::new((l + r) * 0.5 + k * (width - BASE_W), (t + bottom) * 0.5) + offset) * scale));
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    }

    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn map_cycles_compact_expanded_off_without_capturing_world_input() {
        let (mut app, primary, roots, _) = exported_app();
        let root = roots["locationmap"];
        let press = |app: &mut App, key| {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset(key);
        };
        for _ in 0..2 {
            for mode in [local_map::MapMode::Compact, local_map::MapMode::Expanded] {
                press(&mut app, KeyCode::Tab);
                assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Inherited);
                let controls = app.world().get::<WindowControls>(root).unwrap();
                assert_eq!(controls.map_mode, mode);
                assert!(controls.buttons.is_empty() && controls.drag_handle.is_none());
                for size in [Vec2::new(1024.0, 768.0), Vec2::new(1920.0, 1080.0)] {
                    let mut window = app.world_mut().get_mut::<Window>(primary).unwrap();
                    window.resolution.set(size.x, size.y);
                    window.set_cursor_position(Some(Vec2::new(120.0, 180.0) * (size.y / BASE_H)));
                    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
                    app.update();
                    assert!(!app.world().resource::<crate::ui::input::UiInput>().blocks(MouseButton::Left));
                    assert!(app.world().get::<crate::ui::input::UiRegions>(root).unwrap().0.is_empty());
                    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset(MouseButton::Left);
                }
            }
            press(&mut app, KeyCode::Tab);
            assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden);
        }
        // Escape opens/closes the game menu without dismissing the map.
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Escape);
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Inherited);
        assert_eq!(app.world().get::<WindowControls>(root).unwrap().map_mode, local_map::MapMode::Expanded);
        press(&mut app, KeyCode::Escape);
        press(&mut app, KeyCode::Tab);
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.world().get::<WindowControls>(root).unwrap().map_mode, local_map::MapMode::Compact);
    }

    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn spell_grid_matches_class_and_drag_owns_release_and_cancellation() {
        let (mut app, primary, roots, layouts) = exported_app();
        let book = roots["skilltalentswindow"];
        let bar = roots["bottombar"];
        *app.world_mut().get_mut::<Visibility>(book).unwrap() = Visibility::Inherited;
        *app.world_mut().get_mut::<Visibility>(bar).unwrap() = Visibility::Inherited;
        let cards: Vec<_> = layouts["skilltalentswindow"].widgets.iter().filter(|w| w.spell.is_some()).collect();
        for (class, count) in [(0, 15), (1, 18), (2, 16), (3, 16)] {
            let mut c = app.world_mut().get_mut::<WindowControls>(book).unwrap();
            c.spells.class = class;
            let visible: Vec<_> = cards.iter().filter(|w| c.visible(&c.gate("skilltalentswindow", w))).collect();
            assert!(visible.iter().all(|w| w.spell.as_ref().unwrap().class_id == class));
            assert_eq!(visible.iter().filter(|w| w.spell.as_ref().unwrap().wire.is_some()).count(), count);
        }
        app.world_mut().get_mut::<WindowControls>(book).unwrap().spells.class = 1;
        app.update();
        let spell = cards.iter().find(|w| w.spell.as_ref().unwrap().id == "magicmissile").unwrap();
        let center = |rect: [f32; 4]| Vec2::new((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
        let start = center(spell.rect);
        let target = center(app.world().get::<WindowControls>(bar).unwrap().buttons.iter().find(|b| b.action == Action::Skill(2)).unwrap().rect);
        let pointer_step = |app: &mut App, point, press, release| {
            app.world_mut().get_mut::<Window>(primary).unwrap().set_cursor_position(Some(point));
            if press { app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left); }
            if release { app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left); }
            app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        };
        pointer_step(&mut app, start, true, false);
        pointer_step(&mut app, target, false, false);
        assert!(app.world().resource::<Pointer>().spell_drag.as_ref().is_some_and(|d| d.active));
        assert!(app.world().resource::<crate::ui::input::UiInput>().blocks(MouseButton::Left));
        pointer_step(&mut app, target, false, true);
        assert!(app.world_mut().resource_mut::<Pointer>().spell_drop.take().is_some());
        assert!(app.world().resource::<SkillClicks>().0.is_none(), "dropping must not cast");

        // A spell-book drag released outside the bar must leave it unchanged.
        pointer_step(&mut app, start, true, false);
        pointer_step(&mut app, Vec2::new(10.0, 730.0), false, true);
        assert!(app.world().resource::<Pointer>().spell_drop.is_none());
        // The lock blocks edits, while normal slot clicks still cast.
        app.world_mut().get_mut::<WindowControls>(bar).unwrap().locked = true;
        pointer_step(&mut app, start, true, false);
        pointer_step(&mut app, target, false, true);
        assert!(app.world().resource::<Pointer>().spell_drop.is_none());
        click_control(&mut app, primary, bar, "/QuickSlot1");
        assert_eq!(app.world_mut().resource_mut::<SkillClicks>().0.take(), Some(2));
        app.world_mut().get_mut::<WindowControls>(bar).unwrap().locked = false;
        // Escape cancels the drag without closing the spell book.
        pointer_step(&mut app, start, true, false);
        pointer_step(&mut app, target, false, false);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset(KeyCode::Escape);
        pointer_step(&mut app, target, false, true);
        assert!(app.world().resource::<Pointer>().spell_drop.is_none());
        assert_eq!(*app.world().get::<Visibility>(book).unwrap(), Visibility::Inherited);
        // Losing focus also cancels a captured drag.
        pointer_step(&mut app, start, true, false);
        app.world_mut().get_mut::<Window>(primary).unwrap().focused = false;
        pointer_step(&mut app, target, false, true);
        assert!(app.world().resource::<Pointer>().spell_drop.is_none());
    }

    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn current_rankings_select_one_group_and_one_header() {
        let (mut app, primary, roots, layouts) = exported_app();
        assert!(!roots.contains_key("pvpleaderboards"));
        let root = roots["leaderboards"];
        *app.world_mut().get_mut::<Visibility>(root).unwrap() = Visibility::Inherited;
        app.update();
        for group in 0..3 {
            click_control(&mut app, primary, root, &format!("/RankGroupButton{group}"));
            let c = app.world().get::<WindowControls>(root).unwrap();
            for (i, part) in ["PvP", "PvE", "Event"].into_iter().enumerate() {
                let w = layouts["leaderboards"].widgets.iter().find(|w| w.id.ends_with(&format!("/{part}"))).unwrap();
                assert_eq!(c.visible(&c.gate("leaderboards", w)), group == i);
            }
            let headers: Vec<_> = layouts["leaderboards"].widgets.iter().filter(|w|
                ["/DefaultTableColumnHeading", "/Header", "/HeaderOverallRanking"].iter().any(|suffix| w.id.ends_with(suffix)))
                .filter(|w| c.visible(&c.gate("leaderboards", w))).collect();
            assert_eq!(headers.len(), 1);
            assert!(headers[0].id.ends_with("/HeaderOverallRanking"));
            if group != 2 {
                click_control(&mut app, primary, root, &format!("/RankGroup{group}/RankCategory1"));
                let c = app.world().get::<WindowControls>(root).unwrap();
                assert!(c.section_visible(Section::Ranking(group as u8, false)));
                assert!(!c.section_visible(Section::Ranking(group as u8, true)));
            }
        }
    }

    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn every_navigation_shortcut_opens_its_own_panel_and_tracks_close() {
        let (mut app, primary, roots, _) = exported_app();
        app.world_mut().spawn(crate::FreeCamera::default());
        app.update();
        for (button, window) in [("char", "charactersheet"), ("quest", "questlog"), ("skills", "skilltalentswindow"),
            ("inventory", "inventory"), ("dealer", "shopwindow"), ("travelmap", "collectorsalbum"), ("minimap", "locationmap"),
            ("socialscreen", "socialscreen"), ("pvp", "matchmakingwindow"), ("pvpleaderboards", "leaderboards"), ("events", "eventwindow")] {
            click_control(&mut app, primary, roots["ingamemenu"], &format!("/{button}"));
            assert_eq!(*app.world().get::<Visibility>(roots[window]).unwrap(), Visibility::Inherited, "{button}");
            assert_eq!(*app.world().get::<Visibility>(roots["travelmap"]).unwrap(), Visibility::Hidden);
            let c = app.world().get::<WindowControls>(roots["ingamemenu"]).unwrap();
            assert!(c.selected(&Action::Window(window)), "shortcut must follow the opened panel");
            let panel = app.world().get::<WindowControls>(roots[window]).unwrap();
            if window == "locationmap" {
                assert!(panel.drag_handle.is_none());
                for _ in 0..2 {
                    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Tab);
                    app.update();
                    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset(KeyCode::Tab);
                }
            } else {
                assert!(panel.drag_handle.is_some(), "{window} must be draggable");
                let close = panel.buttons.iter().find(|b| b.action == Action::Close).unwrap().id.clone();
                click_control(&mut app, primary, roots[window], &close);
            }
            assert_eq!(*app.world().get::<Visibility>(roots[window]).unwrap(), Visibility::Hidden, "close {window}");
            let c = app.world().get::<WindowControls>(roots["ingamemenu"]).unwrap();
            assert!(!c.selected(&Action::Window(window)), "closed shortcut must lose its highlight");
        }
    }

    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn panel_tabs_switch_pages_and_exported_hud_uses_one_variant() {
        let (mut app, primary, roots, layouts) = exported_app();
        app.update();
        for (window, suffix, group, page) in [("questlog", "/DailyAchievementsTab", "quest", 3),
            ("skilltalentswindow", "/KnowledgeTab", "talents", 1),
            // The shop's tabs are controls::systems::shop's (its own test).
            ("socialscreen", "/TabButtonFriends", "social", 1),
            ("leaderboards", "/RankGroupButton1", "rank_group", 1), ("matchmakingwindow", "/TabButton02", "matchmaking", 1)] {
            *app.world_mut().get_mut::<Visibility>(roots[window]).unwrap() = Visibility::Inherited;
            app.update();
            click_control(&mut app, primary, roots[window], suffix);
            let c = app.world().get::<WindowControls>(roots[window]).unwrap();
            assert_eq!(c.pages.get(group), Some(&page), "{window}");
            assert!(c.section_visible(Section::Page(group, 1 << page)));
            assert!(!c.section_visible(Section::Page(group, 1)));
            *app.world_mut().get_mut::<Visibility>(roots[window]).unwrap() = Visibility::Hidden;
        }
        let gamebar = app.world().get::<WindowControls>(roots["gamebar"]).unwrap();
        for w in &layouts["gamebar"].widgets {
            if w.id.contains("/bgAutoplay") || w.id.contains("/Premium/") || w.id.contains("/Deluxe/") || w.id.contains("/SigninBtn/") {
                assert!(!gamebar.visible(&gamebar.gate("gamebar", w)), "variant leaked: {}", w.id);
            }
        }
        for name in ["bottombar_xp_bar", "bottombar_wisdom_bar"] {
            let bar = layouts["bottombar"].widgets.iter().find(|w| w.id.ends_with(&format!("/{name}/progress"))).unwrap();
            assert!(!bar.hidden && bar.art.is_some(), "progress bar must be exported: {name}");
            assert!(gauge_of(&bar.id).is_some());
            let label = layouts["bottombar"].widgets.iter().find(|w| w.id.contains(name) && w.id.ends_with("/Level")).unwrap();
            assert!(!label.hidden && value_of(&label.id).is_some());
        }
        for w in &layouts["ingamemenu"].widgets {
            if w.id.contains("/icon/Label_highlight") { assert!(w.hidden, "a common highlight must not stay lit"); }
        }
        // The album's pages and list are the client's (controls::systems::album);
        // the converted layout keeps its animations and flags hidden.
        let album = app.world().get::<WindowControls>(roots["collectorsalbum"]).unwrap();
        for w in &layouts["collectorsalbum"].widgets {
            if w.id.contains("Template") || ["/AnimUnlock", "/AnimUse", "/AnimIconGlow", "/FlagLabelCategory"].iter().any(|p| w.id.contains(p)) {
                assert!(!album.visible(&album.gate("collectorsalbum", w)), "stickerbook placeholder leaked: {}", w.id);
            }
            assert!(!w.id.contains("/AlbumPage") && !w.id.contains("/AlbumCategory"), "an added album page: {}", w.id);
        }
        for suffix in ["/WindowTitle", "/Firstpage/AdvertisementArtwork", "/Firstpage/background_info/TextLabel_fav", "/Categorypage/background_preview"] {
            let widget = layouts["collectorsalbum"].widgets.iter().find(|w| w.id.ends_with(suffix)).unwrap();
            assert!(album.visible(&album.gate("collectorsalbum", widget)), "stickerbook content missing: {suffix}");
        }
    }

    #[test]
    fn text_entries_edit_unicode_at_the_caret_and_replace_selected_text() {
        let mut entry = TextEntry::default();
        entry.edit(KeyCode::KeyA, Some("Алиса"), false);
        entry.edit(KeyCode::ArrowLeft, None, false);
        entry.edit(KeyCode::Backspace, None, false);
        assert_eq!(entry.text, "Алиа");
        entry.edit(KeyCode::KeyA, None, true);
        entry.edit(KeyCode::KeyX, Some("XYZ"), false);
        assert_eq!(entry.text, "XYZ");
        entry.edit(KeyCode::Home, None, false);
        entry.edit(KeyCode::Delete, None, false);
        assert_eq!(entry.text, "YZ");
    }

    /// Exercise the actual ECS systems and shipped widget IDs, not just the
    /// action/state helpers. Run with --ignored after exporting game assets.
    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn exported_windows_respond_to_pointer_keyboard_tabs_and_resize() {
        let (mut app, primary, roots, layouts) = exported_app();
        let gear = app.world_mut().spawn((Element { root: roots["inventory"], gate: Gate { section: Section::Inventory(0), button: None } }, Visibility::Hidden)).id();
        let ingredient = app.world_mut().spawn((Element { root: roots["inventory"], gate: Gate { section: Section::Inventory(1), button: None } }, Visibility::Hidden)).id();
        app.update();
        let click = |app: &mut App, name: &str, suffix: &str| {
            let w = layouts[name].widgets.iter().find(|w| w.id.ends_with(suffix) && matches!(w.kind.as_str(), "Button" | "TextButton" | "SlotButton")).unwrap();
            let offset = window_offset(app.world().get::<Transform>(roots[name]).unwrap());
            let mut window = app.world_mut().get_mut::<Window>(primary).unwrap();
            let scale = window.height() / BASE_H;
            let [l, t, r, b] = w.rect;
            let x = ((l + r) * 0.5 + offset.x) * scale + w.k * (window.width() - BASE_W * scale);
            window.set_cursor_position(Some(Vec2::new(x, ((t + b) * 0.5 + offset.y) * scale)));
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        };
        click(&mut app, "ingamemenu", "/inventory");
        assert_eq!(*app.world().get::<Visibility>(roots["inventory"]).unwrap(), Visibility::Inherited);
        assert_eq!(*app.world().get::<Visibility>(gear).unwrap(), Visibility::Inherited);
        assert_eq!(*app.world().get::<Visibility>(ingredient).unwrap(), Visibility::Hidden);
        click(&mut app, "inventory", "/IngredientBagButton");
        assert_eq!(app.world().get::<WindowControls>(roots["inventory"]).unwrap().inventory, 1);
        assert_eq!(*app.world().get::<Visibility>(gear).unwrap(), Visibility::Hidden);
        assert_eq!(*app.world().get::<Visibility>(ingredient).unwrap(), Visibility::Inherited);
        click(&mut app, "inventory", "/InventoryReturnButton");
        click(&mut app, "inventory", "/unten/Label_bg/BagButton1");
        assert_eq!(app.world().get::<WindowControls>(roots["inventory"]).unwrap().bag, 1);
        assert!(!app.world().get::<WindowControls>(roots["inventory"]).unwrap().section_visible(Section::OverflowFooter));
        click(&mut app, "inventory", "/unten/Label_bg/BagButton2");
        assert_eq!(app.world().get::<WindowControls>(roots["inventory"]).unwrap().bag, 2);
        click(&mut app, "inventory", "/FrameLabel/Button");
        assert_eq!(*app.world().get::<Visibility>(roots["inventory"]).unwrap(), Visibility::Hidden);
        assert!(app.world().get::<crate::ui::input::UiRegions>(roots["inventory"]).unwrap().0.is_empty());
        click(&mut app, "ingamemenu", "/char");
        click(&mut app, "charactersheet", "/TabButtonPVP");
        assert_eq!(app.world().get::<WindowControls>(roots["charactersheet"]).unwrap().character, 1);
        click(&mut app, "ingamemenu", "/skills");
        assert_eq!(*app.world().get::<Visibility>(roots["charactersheet"]).unwrap(), Visibility::Hidden);
        assert_eq!(*app.world().get::<Visibility>(roots["skilltalentswindow"]).unwrap(), Visibility::Inherited);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();
        assert_eq!(*app.world().get::<Visibility>(roots["skilltalentswindow"]).unwrap(), Visibility::Hidden);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset(KeyCode::Escape);
        app.world_mut().get_mut::<Window>(primary).unwrap().resolution.set(1920.0, 1080.0);
        click(&mut app, "ingamemenu", "/inventory");
        click(&mut app, "inventory", "/TrinketBagButton");
        assert_eq!(app.world().get::<WindowControls>(roots["inventory"]).unwrap().inventory, 2);
    }

    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn exported_title_drags_move_children_and_hits_without_leaking_world_input() {
        let (mut app, primary, roots, _) = exported_app();
        // UI interaction must leave the game's custom cursor installed, even
        // while hovering a title or dragging it outside the window's bounds.
        use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage};
        let game_cursor = CursorIcon::Custom(CustomCursor::Image(CustomCursorImage::default()));
        app.world_mut().entity_mut(primary).insert(game_cursor.clone());
        let move_cursor = |app: &mut App, point: Vec2| {
            let mut window = app.world_mut().get_mut::<Window>(primary).unwrap();
            let scale = window.height() / BASE_H;
            window.set_cursor_position(Some(point * scale));
        };
        let press = |app: &mut App| {
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        };
        let release = |app: &mut App| {
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        };
        for size in [(1024.0, 768.0), (1920.0, 1080.0)] {
            app.world_mut().get_mut::<Window>(primary).unwrap().resolution.set(size.0, size.1);
            let width = BASE_H * size.0 / size.1;
            for name in ["inventory", "charactersheet", "skilltalentswindow", "gamemenu"] {
                let root = roots[name];
                for entity in roots.values() { *app.world_mut().get_mut::<Visibility>(*entity).unwrap() = Visibility::Hidden; }
                *app.world_mut().get_mut::<Visibility>(root).unwrap() = Visibility::Inherited;
                app.world_mut().get_mut::<Transform>(root).unwrap().translation.x = 0.0;
                app.world_mut().get_mut::<Transform>(root).unwrap().translation.y = 0.0;
                let child = app.world_mut().spawn((Transform::from_xyz(15.0, 25.0, 0.0), ChildOf(root))).id();
                let handle = app.world().get::<WindowControls>(root).unwrap().drag_handle.expect("every menu needs a title handle");
                let [l, t, r, _] = handle.rect;
                let start = Vec2::new((l + r) * 0.5 + handle.k * (width - BASE_W), t + 12.0);
                move_cursor(&mut app, start);
                app.update();
                assert_eq!(app.world().get::<CursorIcon>(primary), Some(&game_cursor));
                let child_before = app.world().get::<GlobalTransform>(child).unwrap().translation();
                press(&mut app);
                assert_eq!(app.world().resource::<Pointer>().drag.as_ref().unwrap().root, root);
                assert!(app.world().resource::<crate::ui::input::UiInput>().blocks(MouseButton::Left));
                let delta = Vec2::new(if name == "inventory" { -160.0 } else if name == "skilltalentswindow" { -60.0 } else { 160.0 }, 45.0);
                move_cursor(&mut app, start + delta);
                app.update();
                assert!(window_offset(app.world().get::<Transform>(root).unwrap()).distance(delta) < 0.001);
                let moved = app.world().get::<GlobalTransform>(child).unwrap().translation() - child_before;
                assert!(moved.truncate().distance(Vec2::new(delta.x, -delta.y)) < 0.001);
                assert!(app.world().resource::<crate::ui::input::UiInput>().blocks(MouseButton::Left));
                assert_eq!(app.world().get::<CursorIcon>(primary), Some(&game_cursor));
                release(&mut app);
                assert!(app.world().resource::<Pointer>().drag.is_none());
                move_cursor(&mut app, start);
                app.update();
                assert!(window_offset(app.world().get::<Transform>(root).unwrap()).distance(delta) < 0.001, "released windows must stop following the mouse");

                // The close button must use its moved rect and keep priority
                // where it overlaps the title strip.
                let c = app.world().get::<WindowControls>(root).unwrap();
                let close = c.buttons.iter().find(|b| b.action == Action::Close).unwrap();
                let [l, t, r, b] = close.rect;
                let close_point = Vec2::new((l + r) * 0.5 + close.k * (width - BASE_W), (t + b) * 0.5) + delta;
                move_cursor(&mut app, close_point);
                press(&mut app);
                assert!(app.world().resource::<Pointer>().drag.is_none());
                assert!(app.world().resource::<crate::ui::input::UiInput>().blocks(MouseButton::Left), "moved buttons must block world input");
                release(&mut app);
                assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Hidden);
                assert!(app.world().get::<crate::ui::input::UiRegions>(root).unwrap().0.is_empty());
                app.world_mut().despawn(child);
            }
        }

        let root = roots["inventory"];
        app.world_mut().get_mut::<Window>(primary).unwrap().resolution.set(1024.0, 768.0);
        app.world_mut().get_mut::<Transform>(root).unwrap().translation = Vec3::new(0.0, 0.0, 100.0);
        *app.world_mut().get_mut::<Visibility>(root).unwrap() = Visibility::Inherited;
        app.update();
        let title = Vec2::new(800.0, 60.0);
        // A world-origin hold crossing the title never starts dragging it.
        move_cursor(&mut app, Vec2::new(500.0, 700.0));
        press(&mut app);
        move_cursor(&mut app, title);
        app.update();
        assert!(!app.world().resource::<crate::ui::input::UiInput>().blocks(MouseButton::Left));
        assert!(app.world().resource::<Pointer>().drag.is_none());
        release(&mut app);
        let handle = app.world().get::<WindowControls>(root).unwrap().drag_handle.unwrap();
        move_cursor(&mut app, Vec2::new(handle.rect[0] + 1.0, handle.rect[1] + 12.0));
        press(&mut app);
        // Bevy returns no cursor position outside the native window. Drag
        // from the left of the title to the viewport edge to exercise clamps.
        move_cursor(&mut app, Vec2::new(BASE_W - 1.0, 0.0));
        app.update();
        let c = app.world().get::<WindowControls>(root).unwrap();
        let handle = c.drag_handle.unwrap();
        let offset = window_offset(app.world().get::<Transform>(root).unwrap());
        assert!((handle.rect[0] + offset.x - (BASE_W - 64.0)).abs() < 0.001);
        assert!((handle.rect[1] + offset.y).abs() < 0.001, "title must remain vertically reachable");
        assert!(app.world().resource::<crate::ui::input::UiInput>().blocks(MouseButton::Left));
        app.world_mut().get_mut::<Window>(primary).unwrap().focused = false;
        app.update();
        assert!(app.world().resource::<Pointer>().drag.is_none(), "focus loss must cancel dragging");
        release(&mut app);
        app.world_mut().get_mut::<Window>(primary).unwrap().focused = true;
        move_cursor(&mut app, title);
        app.update();
        assert_eq!(window_offset(app.world().get::<Transform>(root).unwrap()), offset);

        // Closing and reopening preserves the offset during this session.
        for _ in 0..2 {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyI);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset(KeyCode::KeyI);
        }
        assert_eq!(window_offset(app.world().get::<Transform>(root).unwrap()), offset);
        assert_eq!(*app.world().get::<Visibility>(root).unwrap(), Visibility::Inherited);

        // Overlapping windows use their visual stacking order. Opening one
        // raises it; the modal game menu still prevents grabbing either.
        let character = roots["charactersheet"];
        *app.world_mut().get_mut::<Visibility>(character).unwrap() = Visibility::Inherited;
        app.world_mut().get_mut::<Transform>(character).unwrap().translation = Vec3::new(0.0, 0.0, 120.0);
        app.world_mut().get_mut::<Transform>(root).unwrap().translation = Vec3::new(-589.0, 0.0, 100.0);
        let overlap = Vec2::new(200.0, 60.0);
        move_cursor(&mut app, overlap);
        app.update();
        press(&mut app);
        assert_eq!(app.world().resource::<Pointer>().drag.as_ref().unwrap().root, character);
        release(&mut app);
        for _ in 0..2 {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyI);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset(KeyCode::KeyI);
        }
        press(&mut app);
        assert_eq!(app.world().resource::<Pointer>().drag.as_ref().unwrap().root, root);
        *app.world_mut().get_mut::<Visibility>(roots["gamemenu"]).unwrap() = Visibility::Inherited;
        app.update();
        assert!(app.world().resource::<Pointer>().drag.is_none(), "opening a modal must cancel an underlying drag");
        release(&mut app);
        press(&mut app);
        assert!(app.world().resource::<Pointer>().drag.is_none(), "cannot grab a window beneath a modal");
        assert!(app.world().resource::<crate::ui::input::UiInput>().blocks(MouseButton::Left));
        release(&mut app);
    }
}

