//! The systems' windows: the leaderboards and the PvP queue, the premium shop,
//! the events, the collector's album and the transmog desk, the smith's salvage
//! and workbench desks and the jeweller's gem desk, the quest log's achievement,
//! daily and Wanted Thief pages, the character sheet's titles, the social
//! window's lists and the currency bag's currencies (crate::ui::controls::currency).
//!
//! Each is an authored template (crate::ui::controls::template) filled from what
//! the server said (crate::network::net) and the systems' data
//! (crate::gameplay::systems). `sync` builds the open windows' views when what
//! they show changed and rebuilds the windows whose view changed (never every
//! frame: docs/ui.md, "Per-frame cost"); `send` asks the server what an opened window
//! needs and sends what its buttons asked for (`Request`, from
//! crate::ui::controls::interact through Pointer).
//! SOURCE: DSOR_Server services/{ranking,matchmaking,commerce,events,customization,
//!   inventory_interactions,achievements,daily_challenges,wanted_thief,
//!   presence_identity,social}.py; protocol/gem_protocol.py (the gem actions are
//!   InventoryCommand operations 0x11 insert [item, socket], 0x14 remove [socket],
//!   0x15 unlock a socket).
pub(super) mod boards;
pub(super) mod shop;
pub(super) mod events;
pub(in crate::ui) mod album;
pub(super) mod transmog;
pub(super) mod desks;
pub(super) mod log;
pub(super) mod rosters;
pub(in crate::ui) mod talents;

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use dsor_proto::commands::{inventory, player, social, systems as proto, ClientCommand};

use crate::gameplay::inventory::Place;
use crate::gameplay::systems::SystemsCatalog;
use crate::network::net::Net;
use crate::ui::{Layout, Loaded, Window2018};
use super::template::Icon;
use super::{items, Action, Pointer, Widget, WindowControls};

pub(in crate::ui) const SALVAGE: &str = "salvagewindow";
/// The jeweller's desk: the 2026 layout (ten sockets; gemdesk is the old one).
pub(in crate::ui) const GEMDESK: &str = "gemdeskwb";
pub(in crate::ui) const WORKBENCH: &str = "recipebookwb";
pub(in crate::ui) const TRANSMOG: &str = "transmogwindow";
/// The NPC desks (crate::ui::dialogue opens them by the NPC's profession) and
/// their depth.
pub(in crate::ui) const DESKS: [(&str, f32); 4] = [(SALVAGE, 420.0), (GEMDESK, 425.0), (WORKBENCH, 430.0), (TRANSMOG, 435.0)];
pub(in crate::ui) const LEADERBOARDS: &str = "leaderboards";
pub(in crate::ui) const MATCHMAKING: &str = "matchmakingwindow";
pub(in crate::ui) const SHOP: &str = "shopwindow";
pub(in crate::ui) const EVENTS: &str = "eventwindow";
pub(in crate::ui) const ALBUM: &str = "collectorsalbum";
pub(in crate::ui) const SHEET: &str = "charactersheet";
pub(in crate::ui) const QUESTLOG: &str = super::quests::WINDOW;
pub(in crate::ui) const SOCIAL: &str = rosters::WINDOW;
/// The rows a board's page holds (ranking_protocol.py MAX_RANKING_PAGE_SIZE).
const PAGE_ROWS: u32 = 10;
const UNRANKED: u32 = 0xFFFF_FFFF;
const FILTER_ALL: u16 = 0x01;
const FILTER_PLAYER_PAGE: u16 = 0x10;
/// The lifetime honour board: the only one this server records.
const LIFETIME_BOARD: u8 = 4;
const EVERY_CLASS: u8 = 4;
/// The queue the matchmaking window's tabs join: team (best of 5, 5 on 5),
/// solo (best of 3, the duel) (pvp_match_protocol.py MATCH_MODE_*).
const QUEUE_MODES: [i8; 2] = [1, 0];
/// EquipmentSlotType of an equipment slot (item_customization_protocol.py
/// CUSTOMIZATION_SLOTS): the cloak's slot 7 is type 12.
fn slot_type(slot: i8) -> u32 { if slot == 7 { 12 } else { slot as u32 } }

/// Where a board's page turns to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Paging { First, Previous, Next, Last, Player }

/// What a button of these windows asks for; `send` carries it out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Page(Paging),
    /// Join the PvP queue of the tab (QUEUE_MODES; -1 the open tab's), leave
    /// it, or accept the match found.
    Queue(i8),
    /// An event tab of the event window.
    Event(usize),
    /// Turn the event bar's page.
    EventPage(i32),
    /// Travel to the open event's area.
    Teleport,
    /// Buy a premium offer (its product key).
    Buy(String),
    /// Use an owned sticker (a mount, a costume, a pet, an emote).
    Sticker(String),
    /// Preview a sticker; a right click (true) also uses it, if owned.
    Pick(String, bool),
    /// Open an album book, a sticker of it previewed.
    AlbumBook(String, Option<String>),
    /// Show the album's n-th banner.
    Banner(usize),
    Favorite(String, bool),
    Upgrade(String),
    /// The album book's n-th reward, hovered for its card.
    Reward(usize),
    /// Add the carried item that fills this sticker.
    AddSticker(String),
    Salvage,
    /// Take a picked item out of the salvage desk.
    Unpick(usize),
    /// A gem socket's button: take its gem out.
    Socket(usize),
    /// A socket's gem, hovered (its card); a click does nothing.
    SocketGem(usize),
    /// A workbench ingredient (its index) or its result (usize::MAX), hovered.
    BenchItem(usize),
    /// A socket's gem right-clicked: back to the inventory.
    TakeGem(usize),
    /// Buy the next socket of the gem desk's item.
    BuySocket,
    RecipeCategory(String),
    /// The workbench's tab, a filter (0 craftable, 1 locked), the amount
    /// (a step; i32::MIN the least, i32::MAX the most).
    WorkbenchTab(usize),
    WorkbenchFilter(u8),
    CraftAmount(i32),
    Recipe(String),
    /// Craft the open recipe, with Andermant when true.
    Craft(bool),
    Skin(String),
    /// The transmog desk's tab, slot, a slot's reset, a filter (0 owned, 1 shop).
    TransmogTab(u8),
    TransmogSlot(i8),
    TransmogReset(i8),
    TransmogFilter(u8),
    /// Apply the chosen skin to the transmog desk's item.
    Apply,
    /// An achievement category of the log (empty: its overview).
    AchievementCategory(String),
    /// Open or close an achievement's row.
    Achievement(String),
    /// The wheel over a list: (its key, rows).
    Scroll(String, i32),
    /// Accept a daily challenge, or (true) have it replaced.
    Daily(String, bool),
    Wanted(u32),
    WantedRefresh(u32),
    /// Show no title (0), an achievement's (1, its id) or the PvP rank's (2).
    Title(u8, String),
    /// Sort the honour board by a column; toggle a filter (0x02 friends, 0x04 guild).
    Sort(u8),
    Filter(u16),
    /// The matchmaking window's tab.
    MatchTab(u8),
    /// The shop's tab; a category of the open tab.
    ShopTab(usize),
    ShopCategory(String),
    /// Redeem the voucher code typed.
    Redeem,
    /// Turn a window's grid page.
    Turn(&'static str, i32),
    /// The social window: search the name typed (all the map's players with
    /// none), ask again, sort the lists by a column.
    SocialSearch,
    SocialRefresh,
    SocialSort(rosters::Column),
    /// A player's row: the player menu (crate::ui::controls::interact opens it).
    Player(String),
    /// The Skills & Talents window (talents): a talent, trait or focus talent
    /// clicked; a skill clicked (by its skill id) to level it up; a right
    /// click (the split dialog, or a focus point taken back; "skill:<id>" for
    /// a skill); the split dialog's ranks.
    Talent(String),
    Skill(String),
    TalentMore(String),
    TalentLevels(String, u32),
    /// A tree's preset (tree, preset); a locked Skill preset bought.
    Preset(u8, u8),
    UnlockPreset(u8),
    /// A tree's reset; a Wisdom or Group category's (tree, category id).
    Reset(u8),
    ResetBranch(u8, String),
    /// The reset's (or unlock's) confirmation answered.
    TalentAnswer(bool),
    /// The Wisdom tab's portable workbench; a trader the window opens by name.
    Workbench,
    TalentShop(String),
}

/// The windows' selections and what their buttons asked for.
#[derive(Resource, Default)]
pub(crate) struct State {
    pub catalog: Handle<SystemsCatalog>,
    pub achievement_category: Option<String>,
    pub achievement_open: Option<String>,
    /// Rows scrolled off the lists' tops, by list (crate::ui::controls::quests
    /// and the systems' windows read them).
    pub scroll: HashMap<String, usize>,
    /// The workbench's tab, its category open and book chosen, the amount to
    /// craft, its filters (craftable now, locked only).
    pub workbench_tab: usize,
    pub recipe_category: Option<String>,
    pub craft_amount: u32,
    pub workbench_craftable_only: bool,
    pub workbench_locked_only: bool,
    pub recipe: Option<String>,
    /// The items picked for melting (right-clicked while the desk is open).
    pub salvage: Vec<u32>,
    /// The equipment on the gem desk, the one on the transmog desk.
    pub gem_target: Option<u32>,
    /// Gems placed on the desk's sockets but not set yet (socket, gem item):
    /// the socket's open padlock sets one (GemDeskWidget::SocketUI
    /// HandleLockUnlockButtonClick 0x140436bf0: a placed gem, state 2, is only
    /// sent to the server when locked).
    /// By template: a locked gem's stack changes id, and several of one
    /// stack can be placed before any is locked.
    /// FAILURE: "je devrais pouvoir ajouter plusieurs pierres et les lock toutes
    ///   1 par 1".
    pub gem_preview: Vec<(usize, String)>,
    /// Set gems being unlocked (socket, template): once the server has taken
    /// one out, it is placed again, its padlock open, until it is locked
    /// again or right-clicked away.
    pub gem_reopen: Vec<(usize, String)>,
    /// An item right-clicked while the workbench is open: its recipe opens
    /// (send resolves it against the catalog).
    pub workbench_pick: Option<String>,
    /// The recipe's slots the player filled (a right-clicked carried item of
    /// that ingredient); Craft needs them all.
    /// FAILURE: the recipe took the bag's items itself ("je veux devoir placer
    ///   ma pierre et les poussieres dans les slots").
    pub workbench_placed: std::collections::BTreeSet<usize>,
    /// The book's recipe a right-clicked item chose (desks::workbench).
    pub workbench_recipe: Option<String>,
    /// The transmog desk's slot picked (an equipment slot), its tab (skins,
    /// dyes) and filters (owned, the shop's).
    pub skin_slot: Option<i8>,
    pub transmog_tab: u8,
    pub transmog_owned: bool,
    pub transmog_shop: bool,
    pub skin: Option<String>,
    /// The title asked for, as SetTitle (kind, achievement).
    pub title: Option<(u8, String)>,
    pub event: usize,
    /// The open event's bar page chosen (None: the one with the next reward).
    pub event_page: Option<usize>,
    /// The album's book open (None: the front page), the sticker previewed,
    /// the front page's banner.
    pub album_book: Option<String>,
    /// The category whose books the list shows under it (its top book), apart
    /// from the book shown: as in the original, a category opens and closes its
    /// list and the page keeps the last book opened.
    pub album_open: Option<String>,
    pub album_sticker: Option<String>,
    /// The item cooldowns running (hotbar::ItemCooldowns::running): what the
    /// album's stickers of their category count down (album::Sticker::countdown).

    pub album_banner: usize,
    pub rank_start: u32,
    /// The grids' pages, by window.
    pub pages: HashMap<&'static str, usize>,
    /// The windows open last frame, for the requests an opening sends.
    opened: HashSet<String>,
    /// The quest the log's overview showed (its scroll restarts with another).
    last_quest: Option<String>,
    /// The leaderboards' tabs last asked for: (group, category, class).
    rank_tabs: Option<(u8, u8, u8)>,
    /// The honour board's sort column and filters (ranking_protocol FILTER_*).
    pub rank_sort: u8,
    pub rank_filters: u16,
    /// The matchmaking window's tab (QUEUE_MODES).
    pub matchmaking_tab: u8,
    /// The shop's tab and each tab's open category.
    pub shop_tab: usize,
    pub shop_category: Vec<Option<String>>,
    /// The social window's sort (column, descending), its last player search
    /// (kind, text) and the id the next one takes.
    pub social_sort: Option<(rosters::Column, bool)>,
    social_query: Option<(u8, String)>,
    social_query_id: u32,
    /// A request the server has not answered: (inventory revision, time).
    pending: Option<(u64, f64)>,
    /// The talents catalog (crate::gameplay::talents), the reset or unlock
    /// waiting for its confirmation, the split dialog's talent and most ranks.
    pub talents: Handle<crate::gameplay::talents::TalentCatalog>,
    pub talent_dialog: Option<talents::Ask>,
    pub talent_split: Option<(String, u32)>,
    /// A window the talents window opens (the portable workbench), a trader it
    /// opens by name (talents::dialogs carries them out).
    pub talent_open: Option<&'static str>,
    pub talent_trader: Option<String>,
}

/// What every window shows. Only the open windows' views are kept current
/// (`sync`); a closed window's is rebuilt when it opens.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(in crate::ui) struct Views {
    pub boards: boards::Boards,
    pub shop: shop::Shop,
    pub events: events::Events,
    pub album: album::Album,
    pub transmog: transmog::Transmog,
    pub salvage: desks::Salvage,
    pub gems: desks::Gems,
    pub workbench: desks::Workbench,
    pub log: log::Log,
    pub titles: log::Titles,
    pub rosters: rosters::Rosters,
    pub currencies: super::currency::Bag,
    pub talents: talents::Talents,
    pub talent_dialog: Option<talents::Ask>,
    pub talent_split: Option<(String, u32)>,
}

/// What the views are built from.
pub(in crate::ui) struct Sources<'a> {
    pub net: &'a Net,
    pub catalog: &'a SystemsCatalog,
    pub items: Option<&'a items::Catalog>,
    /// The maps' names (interface/maps.names.json).
    pub names: Option<&'a crate::ui::local_map::MapNames>,
    pub state: &'a State,
    pub class: Option<u8>,
    pub talents: Option<&'a crate::gameplay::talents::TalentCatalog>,
    /// The talent ranks the equipment adds (crate::gameplay::stats::TalentBonuses).
    pub bonuses: Option<&'a HashMap<String, u32>>,
}

impl Sources<'_> {
    /// An item template's icon layers (a carried item's own look when given).
    pub fn icon(&self, template: &str, item: Option<&inventory::ItemInfo>) -> Icon {
        let Some(items) = self.items else { return Icon::default() };
        let (image, background, frame) = items.icon_parts(template, item);
        Icon { background: background.unwrap_or_default().to_owned(), image: image.unwrap_or_default().to_owned(), frame: frame.unwrap_or_default().to_owned(),
            info: items.icon_info(template, item).unwrap_or_default().to_owned() }
    }
    pub fn name(&self, template: &str, item: Option<&inventory::ItemInfo>) -> String {
        self.items.map(|i| i.name(template, item).to_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| crate::ui::dialogue::readable(template))
    }
    /// Our class's name (crate::actors::character::CLASSES).
    pub fn class_name(&self) -> &'static str {
        crate::actors::character::CLASSES.get(self.class.unwrap_or(0) as usize).copied().unwrap_or("warrior")
    }
    /// The carried items of a template, by id.
    pub fn carried(&self, template: &str) -> Vec<u32> {
        let inventory = &self.net.inventory;
        // The ingredient bag's stacks count too (allocation words_180): the
        // workbench's Shiny Dust lives there.
        // FAILURE: "ca doit compter ca comme un item alors que la gem powder
        //   est dans l'inventaire mais dans son onglet special".
        let mut ids: Vec<u32> = inventory.items.values().filter(|i| i.template == template)
            .filter(|i| matches!(inventory.location(i.id), Some(Place::Bag(_) | Place::Extra(_))) || inventory.allocation.words_180.contains(&i.id))
            .map(|i| i.id).collect();
        ids.sort_unstable();
        ids
    }
}

/// The wall clock, in seconds since 1970 (std's clock panics in the browser).
pub(in crate::ui) fn unix_seconds() -> u64 {
    crate::world::props::unix_seconds()
}

/// The windows each view (`Views`, in `refresh`'s order) fills.
const VIEW_WINDOWS: [&[&str]; 15] = [&[LEADERBOARDS, MATCHMAKING], &[SHOP], &[EVENTS], &[ALBUM], &[TRANSMOG],
    &[SALVAGE], &[GEMDESK], &[WORKBENCH], &[QUESTLOG], &[SHEET], &[SOCIAL], &[super::currency::BAG],
    &[talents::WINDOW], &[talents::DIALOG], &[talents::SPLIT]];

/// Builds view `index` of `VIEW_WINDOWS` into `views`; true when it changed.
fn refresh(index: usize, sources: &Sources, views: &mut Views) -> bool {
    fn set<T: PartialEq>(slot: &mut T, next: T) -> bool {
        if *slot == next { return false; }
        *slot = next;
        true
    }
    match index {
        0 => set(&mut views.boards, boards::boards(sources)),
        1 => set(&mut views.shop, shop::shop(sources)),
        2 => set(&mut views.events, events::events(sources)),
        3 => set(&mut views.album, album::album(sources)),
        4 => set(&mut views.transmog, transmog::transmog(sources)),
        5 => set(&mut views.salvage, desks::salvage(sources)),
        6 => set(&mut views.gems, desks::gems(sources)),
        7 => set(&mut views.workbench, desks::workbench(sources)),
        8 => set(&mut views.log, log::log(sources)),
        9 => set(&mut views.titles, log::titles(sources)),
        10 => set(&mut views.rosters, rosters::rosters(sources)),
        11 => set(&mut views.currencies, super::currency::bag(sources)),
        12 => set(&mut views.talents, talents::talents(sources)),
        13 => set(&mut views.talent_dialog, sources.state.talent_dialog.clone()),
        // The split dialog's talent mode takes the talents' texts.
        _ => set(&mut views.talent_split, sources.state.talent_split.clone()) | set(&mut views.talents, talents::talents(sources)),
    }
}

/// What the views are built from, besides `State` (by its change ticks).
#[derive(Clone, Copy, PartialEq)]
struct Inputs {
    /// Net::ui_revision and the inventory's revision.
    net: (u64, u64),
    /// Which catalogs have loaded: the systems', the items', the map names,
    /// the talents'.
    catalogs: (bool, bool, bool, bool),
    class: Option<u8>,
    /// The clock's minute while the events or the quest log (the Wanted
    /// board) show their countdowns; 0 otherwise.
    minute: u64,
    /// While the talents window is open, what it shows besides the server's
    /// commands: the action bar's skills (their glow) and the player's preset
    /// and reset effects (StatusEffect stays out of Net::ui_revision); 0 otherwise.
    talents: u64,
}

/// Which views are current, and what for.
#[derive(Default)]
pub(in crate::ui) struct Built {
    inputs: Option<Inputs>,
    current: [bool; VIEW_WINDOWS.len()],
    frame: u32,
}

/// The open windows' views, rebuilt when what they are built from changes; the
/// windows whose view changed are rebuilt.
/// CONTRACT: nothing per frame while nothing changed. A closed window's view
///   waits until it opens; an open one's waits for its inputs to move (a server
///   command, an inventory or State edit, a catalog loading, our class).
/// FAILURE (2026-10-21): every view was rebuilt every frame and compared, open
///   or not: 1 ms a frame offline, more with a real account's achievements.
#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn sync(
    mut commands: Commands,
    net: Option<NonSend<Net>>,
    state: Res<State>,
    catalogs: Res<Assets<SystemsCatalog>>,
    item_data: Option<Res<items::Data>>,
    item_catalogs: Res<Assets<items::Catalog>>,
    players: Query<&crate::actors::character::Character, With<crate::network::net::LocalPlayer>>,
    mut views: ResMut<Views>,
    mut built: Local<Built>,
    mut work: ResMut<crate::ui::UiWork>,
    mut loaded: ResMut<Loaded>,
    layouts: Res<Assets<Layout>>,
    windows: Query<(Entity, &Window2018, &Visibility)>,
    map_names: Res<Assets<crate::ui::local_map::MapNames>>,
    talent_catalogs: Option<Res<Assets<crate::gameplay::talents::TalentCatalog>>>,
    bonuses: Option<Res<crate::gameplay::stats::TalentBonuses>>,
) {
    let Some(net) = net else { return };
    let mut open = [false; VIEW_WINDOWS.len()];
    for (_, window, visibility) in &windows {
        if *visibility == Visibility::Hidden { continue; }
        if let Some(index) = VIEW_WINDOWS.iter().position(|names| names.contains(&window.0.as_str())) { open[index] = true; }
    }
    let items = item_data.as_ref().and_then(|d| item_catalogs.get(&d.catalog));
    let names = map_names.get(&loaded.map_names);
    let class = players.iter().next().map(|c| c.desc.class);
    let countdown = VIEW_WINDOWS.iter().zip(open).any(|(names, open)| open && (names.contains(&EVENTS) || names.contains(&QUESTLOG)));
    let talent_catalog = talent_catalogs.as_ref().and_then(|c| c.get(&state.talents));
    let inputs = Inputs { net: (net.ui_revision, net.inventory.revision),
        catalogs: (catalogs.get(&state.catalog).is_some(), items.is_some(), names.is_some(), talent_catalog.is_some()), class,
        minute: if countdown { unix_seconds() / 60 } else { 0 },
        talents: if open[12] || open[14] { talents_key(&net, talent_catalog, bonuses.as_deref().map(|b| &b.0)) } else { 0 } };
    if built.inputs != Some(inputs) || state.is_changed() {
        built.inputs = Some(inputs);
        built.current = [false; VIEW_WINDOWS.len()];
    }
    // Development builds (and DSOR_FLAGS=uiaudit) rebuild the current views now
    // and then: one that changed means an input above was missed (a Net field
    // edited without Net::touch_ui).
    built.frame = built.frame.wrapping_add(1);
    let audit = (cfg!(debug_assertions) || crate::flag("uiaudit")) && built.frame % 60 == 0;
    if !(0..open.len()).any(|i| open[i] && (!built.current[i] || audit)) { return; }
    let fallback = SystemsCatalog::default();
    let catalog = catalogs.get(&state.catalog).unwrap_or(&fallback);
    let sources = Sources { net: &net, catalog, items, names, state: &state, class, talents: talent_catalog, bonuses: bonuses.as_deref().map(|b| &b.0) };
    let mut changed: Vec<&str> = Vec::new();
    for index in (0..open.len()).filter(|&i| open[i]) {
        let current = built.current[index];
        if current && !audit { continue; }
        if refresh(index, &sources, &mut views) {
            if current { error!("{} changed without an input change: a Net field it shows was edited without Net::touch_ui", VIEW_WINDOWS[index][0]); }
            changed.extend(VIEW_WINDOWS[index]);
        }
        if !current { work.system_views += 1; }
        built.current[index] = true;
    }
    if changed.is_empty() { return; }
    // The windows keep their roots (and whether they are open): their parts are
    // rebuilt from the new view (crate::ui::spawn_windows).
    for (entity, window, _) in &windows {
        if changed.contains(&window.0.as_str()) { commands.entity(entity).despawn_related::<Children>(); }
    }
    for i in 0..loaded.layouts.len() {
        if layouts.get(&loaded.layouts[i].0).is_some_and(|l| changed.contains(&l.window.as_str())) { loaded.spawned[i] = false; }
    }
}

/// What the talents window shows outside the server's commands, as a number
/// that moves with it: the bar's skills and the preset and reset effects.
fn talents_key(net: &Net, catalog: Option<&crate::gameplay::talents::TalentCatalog>, bonuses: Option<&HashMap<String, u32>>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    net.bar.hash(&mut hasher);
    let mut ranks: Vec<_> = bonuses.into_iter().flatten().collect();
    ranks.sort_unstable();
    ranks.hash(&mut hasher);
    if let Some(catalog) = catalog {
        let mut effects: Vec<(u16, u32, u32)> = net.stat_effects.values()
            .filter(|e| catalog.reset_effects.contains(&(e.index as u32)) || catalog.preset_effects.contains_key(&e.index.to_string()))
            .map(|e| (e.index, e.instance, e.tail.as_ref().and_then(|t| t.parameters.first()).map_or(0, |v| v.to_bits()))).collect();
        effects.sort_unstable();
        effects.hash(&mut hasher);
    }
    hasher.finish().max(1)
}

/// The window a layout is, prepared from its view; None for the others.
pub(in crate::ui) fn prepare(layout: &Layout, views: &Views, quests: &super::quests::Quests) -> Option<Layout> {
    Some(match layout.window.as_str() {
        LEADERBOARDS => boards::prepare_leaderboards(layout, &views.boards),
        MATCHMAKING => boards::prepare_matchmaking(layout, &views.boards),
        SHOP => shop::prepare_shop(layout, &views.shop),
        EVENTS => events::prepare_events(layout, &views.events),
        ALBUM => album::prepare_album(layout, &views.album),
        TRANSMOG => transmog::prepare_transmog(layout, &views.transmog),
        SALVAGE => desks::prepare_salvage(layout, &views.salvage),
        GEMDESK => desks::prepare_gemdesk(layout, &views.gems),
        WORKBENCH => desks::prepare_workbench(layout, &views.workbench),
        QUESTLOG => log::prepare_questlog(&super::quests::prepare_log(layout, quests), &views.log),
        SHEET => log::prepare_sheet(layout, &views.titles),
        SOCIAL => rosters::prepare_social(layout, &views.rosters),
        talents::WINDOW => talents::prepare_window(layout, &views.talents),
        talents::DIALOG => talents::prepare_dialog(layout, views.talent_dialog.as_ref()),
        talents::SPLIT => talents::prepare_split(layout, views.talent_split.as_ref(), &views.talents),
        _ => return None,
    })
}

/// The buttons of these windows: the rows `prepare` clones carry `Sys:<request>`
/// events; the authored buttons are named by their window.
pub(super) fn action(window: &str, w: &Widget) -> Option<Action> {
    let event = w.event.as_str();
    let leaf = super::template::leaf(&w.id);
    if let Some(rest) = event.strip_prefix("Sys:") {
        let (kind, arg) = rest.split_once(':').unwrap_or((rest, ""));
        let flag = arg.ends_with(":1");
        let bare = arg.rsplit_once(':').map_or(arg, |(a, _)| a);
        let request = match kind {
            "Buy" => Request::Buy(arg.to_owned()),
            "Event" => Request::Event(arg.parse().ok()?),
            "Sticker" => Request::Sticker(arg.to_owned()),
            "Pick" => Request::Pick(arg.to_owned(), false),
            "AlbumBook" => match arg.split_once(':') { Some((book, sticker)) => Request::AlbumBook(book.to_owned(), Some(sticker.to_owned())), None => Request::AlbumBook(arg.to_owned(), None) },
            "Banner" => Request::Banner(arg.parse().ok()?),
            "Favorite" => Request::Favorite(bare.to_owned(), flag),
            "Upgrade" => Request::Upgrade(arg.to_owned()),
            "Reward" => Request::Reward(arg.parse().ok()?),
            "Add" => Request::AddSticker(arg.to_owned()),
            "Unpick" => Request::Unpick(arg.parse().ok()?),
            "Socket" => Request::Socket(arg.parse().ok()?),
            "SocketGem" => Request::SocketGem(arg.parse().ok()?),
            "BenchItem" => Request::BenchItem(arg.parse().ok()?),
            "Category" => Request::RecipeCategory(arg.to_owned()),
            "Recipe" => Request::Recipe(arg.to_owned()),
            "WorkbenchTab" => Request::WorkbenchTab(arg.parse().ok()?),
            "WorkbenchFilter" => Request::WorkbenchFilter(arg.parse().ok()?),
            "CraftAmount" => Request::CraftAmount(match arg { "min" => i32::MIN, "max" => i32::MAX, step => step.parse().ok()? }),
            "ShopCategory" => Request::ShopCategory(arg.to_owned()),
            "Skin" => Request::Skin(arg.to_owned()),
            "TransmogTab" => Request::TransmogTab(arg.parse().ok()?),
            "TransmogSlot" => Request::TransmogSlot(arg.parse().ok()?),
            "TransmogReset" => Request::TransmogReset(arg.parse().ok()?),
            "TransmogFilter" => Request::TransmogFilter(arg.parse().ok()?),
            "Achievements" => Request::AchievementCategory(arg.to_owned()),
            "Achievement" => Request::Achievement(arg.to_owned()),
            "Daily" => Request::Daily(bare.to_owned(), flag),
            "Wanted" => Request::Wanted(arg.parse().ok()?),
            "WantedRefresh" => Request::WantedRefresh(arg.parse().ok()?),
            "Title" => Request::Title(1, arg.to_owned()),
            // A list's scroll arrows: "<key>:<rows>".
            "Scroll" => { let (key, rows) = arg.rsplit_once(':')?; Request::Scroll(key.to_owned(), rows.parse().ok()?) }
            "Player" => Request::Player(arg.to_owned()),
            "Talent" => Request::Talent(arg.to_owned()),
            "Preset" => { let (tree, preset) = arg.split_once(':')?; Request::Preset(tree.parse().ok()?, preset.parse().ok()?) }
            "UnlockPreset" => Request::UnlockPreset(arg.parse().ok()?),
            "Reset" => Request::Reset(arg.parse().ok()?),
            "ResetBranch" => { let (tree, branch) = arg.split_once(':')?; Request::ResetBranch(tree.parse().ok()?, branch.to_owned()) }
            "Workbench" => Request::Workbench,
            "TalentShop" => Request::TalentShop(arg.to_owned()),
            _ => return None,
        };
        return Some(Action::System(request));
    }
    // The talents' reset confirmation.
    if window == talents::DIALOG {
        return matches!(event, "confirm" | "cancel").then(|| Action::System(Request::TalentAnswer(event == "confirm")));
    }
    // The gem desk's "Open Workbench".
    if window == GEMDESK && event == "Crafting" { return Some(Action::Window(WORKBENCH)); }
    // The privileged Wanted poster's button: the membership page.
    if window == QUESTLOG && event == "vip" { return Some(Action::Membership); }
    let request = match (window, event) {
        (LEADERBOARDS, sort) if boards::SORTS.iter().any(|(e, _)| *e == sort) => Request::Sort(boards::SORTS.iter().find(|(e, _)| *e == sort).map_or(0, |(_, c)| *c)),
        (LEADERBOARDS, filter) if filter.starts_with("FilterFriendsOnly") => Request::Filter(0x02),
        (LEADERBOARDS, filter) if filter.starts_with("FilterGuildMembersOnly") => Request::Filter(0x04),
        (MATCHMAKING, _) if leaf == "TabButton01" => Request::MatchTab(0),
        (MATCHMAKING, _) if leaf == "TabButton02" => Request::MatchTab(1),
        (SHOP, _) if leaf.starts_with("ShopTab") => Request::ShopTab(leaf[7..].parse::<usize>().ok()?.saturating_sub(1)),
        (SHOP, "Payment") => Request::ShopTab(1),
        (SHOP, "RedeemCode") => Request::Redeem,
        (LEADERBOARDS, "PageFirst") => Request::Page(Paging::First),
        (LEADERBOARDS, "PageUp") => Request::Page(Paging::Previous),
        (LEADERBOARDS, "PageDown") => Request::Page(Paging::Next),
        (LEADERBOARDS, "PageLast") => Request::Page(Paging::Last),
        (LEADERBOARDS, "PagePlayer") => Request::Page(Paging::Player),
        (MATCHMAKING, "JoinBattle") => Request::Queue(-1),
        (EVENTS, "TeleportEventArea") => Request::Teleport,
        (EVENTS, "NextPage") => Request::EventPage(1),
        (EVENTS, "PrevPage") => Request::EventPage(-1),
        (EVENTS, _) if leaf.starts_with("TabEvent") && w.id.contains("/TabBarLabel/") => Request::Event(leaf[8..].parse().ok()?),
        (ALBUM, "NextPage") => Request::Turn(ALBUM, 1),
        (ALBUM, "PrevPage") => Request::Turn(ALBUM, -1),
        (TRANSMOG, "NextPage") => Request::Turn(TRANSMOG, 1),
        (TRANSMOG, "PrevPage") => Request::Turn(TRANSMOG, -1),
        (TRANSMOG, "Apply") => Request::Apply,
        (SALVAGE, "Salvage") => Request::Salvage,
        (GEMDESK, "SocketBuyButton") => Request::BuySocket,
        (GEMDESK, "SocketButton") => Request::Socket(w.id.split('/').find_map(|p| p.strip_prefix("Socket").and_then(|n| n.parse().ok()))?),
        (WORKBENCH, "Crafting") => Request::Craft(false),
        (WORKBENCH, "CraftingRC") => Request::Craft(true),
        (WORKBENCH, "ClearAllSlots") => Request::Recipe(String::new()),
        (QUESTLOG, "Confirm") => Request::WantedRefresh(0),
        (QUESTLOG, "ShowOverview") => Request::AchievementCategory(String::new()),
        (SHEET, "DoNotShowTitle") => Request::Title(0, String::new()),
        (SHEET, "ShowPVPTitle") => Request::Title(2, String::new()),
        (SOCIAL, "Enter") => Request::SocialSearch,
        (SOCIAL, "Refresh") => Request::SocialRefresh,
        (SOCIAL, sort) if rosters::COLUMNS.iter().any(|(e, _, _)| *e == sort) => Request::SocialSort(rosters::COLUMNS.iter().find(|(e, _, _)| *e == sort).map(|c| c.1)?),
        _ => return None,
    };
    Some(Action::System(request))
}

/// A clicked request, with what only the window knows (its open tab).
pub(super) fn clicked(mut request: Request, controls: &mut WindowControls) -> Request {
    match &mut request {
        Request::MatchTab(tab) => { controls.pages.insert("matchmaking", *tab); }
        Request::Event(index) => { controls.pages.insert("event", *index as u8); }
        _ => {}
    }
    request
}

/// What a right click on a button asks for: an album sticker is used (a left
/// click only shows it in the preview).
// FAILURE (2026-10-23): a second left click on the sticker previewed used it;
//   the user found that unusable: one right click uses it.
// A right click on a talent opens the split dialog (or takes a focus point
// back), on a skill of the talents window too ("skill:<id>").
pub(super) fn right_clicked(action: &Action, window: &str, spell: Option<&str>) -> Option<Request> {
    match action {
        Action::System(Request::Pick(id, _)) => Some(Request::Pick(id.clone(), true)),
        Action::System(Request::SocketGem(n)) => Some(Request::TakeGem(*n)),
        Action::System(Request::Talent(id)) => Some(Request::TalentMore(id.clone())),
        Action::Spell if window == talents::WINDOW => spell.map(|s| Request::TalentMore(format!("skill:{s}"))),
        _ => None,
    }
}

/// Use an owned sticker of the album shown, named as the server owns it
/// (album::Sticker::server_key); None for one not collected.
fn use_sticker(album: &album::Album, id: &str) -> Option<ClientCommand> {
    album.stickers.iter().find(|s| s.id == id && s.owned)
        .map(|s| ClientCommand::UseStickerBookItem(inventory::UseStickerBookItem { template: s.server_key().to_owned() }))
}

/// The lists the wheel scrolls: (window, page shown (group, page), key, rect, k).
/// Paged ones turn instead (scrolled).
const SCROLL_AREAS: [(&str, Option<(&str, u8)>, &str, [f32; 4], f32); 10] = [
    (WORKBENCH, None, desks::LIST, [7.0, 143.0, 274.0, 435.0], 0.5),
    (ALBUM, None, album::LIST, [62.0, 103.0, 202.0, 639.0], 0.5),
    (SHOP, None, shop::OFFERS, [215.0, 130.0, 719.0, 623.0], 0.5),
    (QUESTLOG, Some(("quest", 0)), super::quests::LIST, [123.0, 145.0, 433.0, 627.0], 0.5),
    (QUESTLOG, Some(("quest", 0)), super::quests::OVERVIEW, [446.0, 140.0, 904.0, 468.0], 0.5),
    (QUESTLOG, Some(("quest", 2)), log::CATEGORIES, [117.0, 201.0, 406.0, 632.0], 0.5),
    (QUESTLOG, Some(("quest", 2)), log::ROWS, [410.0, 140.0, 907.0, 632.0], 0.5),
    // The social window's List, per tab (rosters::LISTS).
    (SOCIAL, Some(("social", rosters::LISTS[0].1)), rosters::LISTS[0].2, [561.0, 168.0, 1017.0, 568.0], 1.0),
    (SOCIAL, Some(("social", rosters::LISTS[1].1)), rosters::LISTS[1].2, [561.0, 168.0, 1017.0, 568.0], 1.0),
    (SOCIAL, Some(("social", rosters::LISTS[2].1)), rosters::LISTS[2].2, [561.0, 168.0, 1017.0, 568.0], 1.0),
];

/// The request the wheel makes over the topmost open window's list, if any.
fn scrolled(window: &Window, windows: &Query<(&Window2018, &Visibility, &WindowControls, &Transform)>, rows: i32) -> Option<Request> {
    let height = window.height().max(1.0);
    let point = window.cursor_position()? * (crate::ui::BASE_H / height);
    let width = crate::ui::BASE_H * window.width() / height;
    let mut open: Vec<_> = windows.iter().filter(|(_, v, _, _)| **v != Visibility::Hidden).collect();
    open.sort_by(|a, b| b.3.translation.z.total_cmp(&a.3.translation.z));
    for (name, _, controls, tf) in open {
        let offset = super::window_offset(tf);
        let inside = |rect: [f32; 4], k: f32| {
            let p = point - offset - Vec2::new(k * (width - crate::ui::BASE_W), 0.0);
            p.x >= rect[0] && p.x <= rect[2] && p.y >= rect[1] && p.y <= rect[3]
        };
        // A list scrolls with its window's own horizontal anchor (its widest
        // region's): the workbench is drawn on the left, not centred.
        // FAILURE: on a wide screen its area sat hundreds of pixels right of
        //   the list ("je peux pas scroll les recettes").
        let window_k = controls.regions.iter()
            .max_by(|a, b| ((a.1[2] - a.1[0]) * (a.1[3] - a.1[1])).total_cmp(&((b.1[2] - b.1[0]) * (b.1[3] - b.1[1]))))
            .map(|r| r.0);
        for (window, page, key, rect, k) in SCROLL_AREAS {
            if window != name.0 || page.is_some_and(|(group, n)| controls.pages.get(group).copied().unwrap_or(0) != n) { continue; }
            if inside(rect, if window == WORKBENCH { window_k.unwrap_or(k) } else { k }) { return Some(Request::Scroll(key.to_owned(), rows)); }
        }
        if name.0 == LEADERBOARDS && inside([406.0, 297.0, 1048.0, 560.0], 0.5) {
            return Some(Request::Page(if rows > 0 { Paging::Next } else { Paging::Previous }));
        }
        if (name.0 == ALBUM || name.0 == TRANSMOG) && controls.covers(point - offset, width) {
            return Some(Request::Turn(if name.0 == ALBUM { ALBUM } else { TRANSMOG }, rows.signum()));
        }
        // The topmost window under the cursor takes the wheel, list or not.
        if controls.covers(point - offset, width) { return None; }
    }
    None
}

/// How far a list scrolls: its entries less those its area shows.
fn scroll_limit(key: &str, views: &Views, quests: &super::quests::Quests) -> usize {
    use super::quests::{LIST, OVERVIEW};
    match key {
        LIST => quests.entries().saturating_sub(10),
        OVERVIEW => quests.list.get(quests.selected).map_or(0, |q| q.overview_rows()).saturating_sub(12),
        log::CATEGORIES => views.log.categories.len().saturating_sub(9),
        album::LIST => album::list_limit(&views.album),
        desks::LIST => desks::list_limit(&views.workbench),
        log::ROWS if views.log.category.is_none() => log::overview_entries(&views.log).saturating_sub(log::OVERVIEW_ROWS),
        log::ROWS => views.log.achievements.len().saturating_sub(5),
        shop::OFFERS => views.shop.cards.len().div_ceil(2).saturating_sub(shop::VISIBLE_ROWS),
        _ => rosters::scroll_limit(&views.rosters, key),
    }
}

/// A request's hover text (crate::ui::controls::tooltips): heading and body;
/// None for the rows and cards, which show their own text.
pub(super) fn tooltip(request: &Request) -> Option<(&'static str, String)> {
    Some(match request {
        Request::Page(Paging::First) => ("First page", String::new()),
        Request::Page(Paging::Previous) => ("Previous page", String::new()),
        Request::Page(Paging::Next) => ("Next page", String::new()),
        Request::Page(Paging::Last) => ("Last page", String::new()),
        Request::Page(Paging::Player) => ("Your rank", "Show the page with your own rank.".into()),
        Request::Queue(_) => ("Join battle", "Join the queue of this battle mode; click again to leave it, or to enter the match found.".into()),
        Request::Teleport => ("Travel to the event", String::new()),
        Request::EventPage(1) => ("Next rewards", String::new()),
        Request::EventPage(_) => ("Previous rewards", String::new()),
        Request::Sort(_) => ("Sort", "Sort the board by this column.".into()),
        Request::Filter(0x02) => ("Friends only", String::new()),
        Request::Filter(_) => ("Guild members only", String::new()),
        Request::ShopTab(0) => ("Andermant shop", "Items for Andermant.".into()),
        Request::ShopTab(_) => ("Premium shop", "Andermant and special offers.".into()),
        Request::Redeem => ("Redeem", String::new()),
        Request::Buy(_) => ("Buy", String::new()),
        Request::Pick(..) => ("Sticker", "Shows it in the preview; a right click uses it.".into()),
        Request::AlbumBook(..) => ("Open", "Opens this part of the album.".into()),
        Request::Banner(_) => ("Banner", String::new()),
        Request::Favorite(_, true) => ("Add to favourites", String::new()),
        Request::Favorite(_, false) => ("Remove from favourites", String::new()),
        Request::Upgrade(_) => ("Upgrade", "Raise this item's rarity with its upgrade items.".into()),
        Request::Reward(_) => ("Reward", String::new()),
        Request::AddSticker(_) => ("Add to the album", "Adds the carried item to your collection.".into()),
        Request::Salvage => ("Melt down", "Melts the items placed here into Glyphs of Power and materials.".into()),
        Request::Unpick(_) => ("Take back", String::new()),
        Request::SocketGem(_) | Request::BenchItem(_) => ("Item", String::new()),
        Request::TakeGem(_) => ("Take out", String::new()),
        Request::Socket(_) => ("Lock / unlock", "Locks the placed gem into the item, or unlocks it; a right click on the gem takes it out.".into()),
        Request::BuySocket => ("Unlock socket", "Unlocks the next socket for Andermant.".into()),
        Request::WorkbenchTab(_) => ("Recipes", "Shows this kind of recipes.".into()),
        Request::WorkbenchFilter(0) => ("Craftable only", "Lists the recipes your items make now.".into()),
        Request::WorkbenchFilter(_) => ("Locked only", "Lists the recipes still to learn.".into()),
        Request::CraftAmount(i32::MIN) => ("Fewest", String::new()),
        Request::CraftAmount(i32::MAX) => ("Most", "As many as the ingredients allow.".into()),
        Request::CraftAmount(step) if *step < 0 => ("Fewer", String::new()),
        Request::CraftAmount(_) => ("More", String::new()),
        Request::Craft(false) => ("Craft", "Crafts the recipe with gold.".into()),
        Request::Craft(true) => ("Craft", "Crafts the recipe with Andermant.".into()),
        Request::Apply => ("Apply changes", "Applies the chosen look to the item.".into()),
        Request::TransmogTab(0) => ("Skins", "Change the look of the worn items.".into()),
        Request::TransmogTab(_) => ("Dyes", "Colour the worn items.".into()),
        Request::TransmogSlot(_) => ("Choose", "Shows what fits this item.".into()),
        Request::TransmogReset(_) => ("Reset", "Restores the item's own look.".into()),
        Request::TransmogFilter(0) => ("Owned only", String::new()),
        Request::TransmogFilter(_) => ("Shop only", String::new()),
        Request::Daily(_, false) => ("Accept challenge", "Accept this challenge and complete it today to receive its reward.".into()),
        Request::Daily(_, true) => ("Exchange", "Exchange this challenge for another one.".into()),
        Request::Wanted(_) => ("Accept mission", String::new()),
        Request::WantedRefresh(0) => ("Refresh all", "Replaces every open mission.".into()),
        Request::WantedRefresh(_) => ("Refresh", "Replaces this mission.".into()),
        Request::Title(0, _) => ("Hide title", String::new()),
        Request::Title(2, _) => ("Display PvP rank", String::new()),
        Request::Turn(_, delta) if *delta < 0 => ("Previous page", String::new()),
        Request::Turn(..) => ("Next page", String::new()),
        // gui.window.social_searchButton / refreshButton / *Column.
        Request::SocialSearch => ("Player Search", "Finds the players whose name contains the text typed; with no text, lists the players on this map.".into()),
        Request::SocialRefresh => ("Update", "Asks the server again for the players, the group and the guild.".into()),
        Request::SocialSort(column) => (rosters::COLUMNS.iter().find(|c| c.1 == *column).map_or("Sort", |c| c.2), "Sort the list by this column; again to reverse it.".into()),
        _ => return None,
    })
}

/// What an opened window asks the server (and the leaderboards on a tab change).
fn opening(window: &str, state: &State, tabs: Option<(u8, u8, u8)>) -> Vec<ClientCommand> {
    match window {
        // The premium shop's offers and the Andermant item shop's (by its name).
        SHOP => vec![ClientCommand::OcpOfferRequest(proto::OcpOfferRequest::default()),
            ClientCommand::RequestOffer(inventory::RequestOffer { by_name: true, actor: 0, shop: shop::ITEM_SHOP.into() })],
        // The running events, and the Andermant shop's offers (their useful ones).
        EVENTS => vec![ClientCommand::ActiveEvents(proto::ActiveEvents::default()),
            ClientCommand::RequestOffer(inventory::RequestOffer { by_name: true, actor: 0, shop: shop::ITEM_SHOP.into() })],
        QUESTLOG => vec![ClientCommand::WantedThiefInfoRequest(proto::WantedThiefInfoRequest::default())],
        LEADERBOARDS => ranking_requests(state, tabs.unwrap_or_default(), FILTER_ALL),
        _ => Vec::new(),
    }
}

/// The honour board's page (the overall board's on the first category tab) and
/// our own rank; the class tabs filter (1: every class, then CLASSES), the
/// check boxes to friends or guild members.
fn ranking_requests(state: &State, (_, category, class): (u8, u8, u8), flags: u16) -> Vec<ClientCommand> {
    let class = if class == 0 { EVERY_CLASS } else { class - 1 };
    let flags = if state.rank_filters != 0 { (flags & !FILTER_ALL) | state.rank_filters } else { flags };
    let start = if flags & FILTER_PLAYER_PAGE != 0 { UNRANKED } else { state.rank_start };
    if category == 0 {
        vec![ClientCommand::OverallRankingRequest(proto::OverallRankingRequest { start, count: PAGE_ROWS, context: 0, flags, class }),
            ClientCommand::OverallLocalRankingRequest(proto::OverallLocalRankingRequest { context: 0 })]
    } else {
        vec![ClientCommand::HonorRankingRequest(proto::HonorRankingRequest { board: LIFETIME_BOARD, start, count: PAGE_ROWS, context: 0, flags, class, sort: state.rank_sort }),
            ClientCommand::LocalRankingRequest(proto::LocalRankingRequest { context: 0, board: LIFETIME_BOARD })]
    }
}

/// The leaderboards' tabs: (group, category, class) from its controls.
fn rank_tabs(controls: &WindowControls) -> (u8, u8, u8) {
    let page = |g| controls.pages.get(g).copied().unwrap_or(0);
    (page("rank_group"), page("rank_category"), page("rank_class"))
}

/// The server's part: the requests of the windows opened and of their buttons.
#[allow(clippy::too_many_arguments)]
/// The album sticker used last and when (Time<Real> seconds): its icon pops
/// (crate::ui::album_overlays). Kept out of State, whose changes rebuild the
/// window: a use must show at once.
/// FAILURE (2026-10-31): the window rebuilt on the use, the countdown and pop
///   came a moment late ("in ours it's a delay").
#[derive(Resource, Default)]
pub(in crate::ui) struct AlbumPopAt(pub Option<(String, f64)>);

/// The currency a sticker's next tier costs (Altered Crescerite).
pub(in crate::ui) const UPGRADE_CURRENCY: &str = "currency_stickerbook_upgrade";

pub(in crate::ui) fn send(
    net: Option<NonSendMut<Net>>,
    mut pointer: ResMut<Pointer>,
    mut state: ResMut<State>,
    catalogs: Res<Assets<SystemsCatalog>>,
    views: Res<Views>,
    windows: Query<(&Window2018, &Visibility, &WindowControls, &Transform)>,
    time: Res<Time<Real>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    primary: Query<&Window, With<bevy::window::PrimaryWindow>>,
    quests: Res<super::quests::Quests>,
    (mut item_cooldowns, items_data, items_catalogs, mut pop_at): (ResMut<super::hotbar::ItemCooldowns>, Res<items::Data>, Res<Assets<items::Catalog>>, ResMut<AlbumPopAt>),
) {
    let Some(mut net) = net else { return };
    let fallback = SystemsCatalog::default();
    let catalog = catalogs.get(&state.catalog).unwrap_or(&fallback);
    // The wheel over a list scrolls it (a notch a row); over a paged grid or
    // board it turns the page.
    let notches: f32 = wheel.read().map(|w| if w.unit == bevy::input::mouse::MouseScrollUnit::Line { w.y } else { w.y / 40.0 }).sum();
    if notches.abs() >= 0.5 {
        if let Some(request) = primary.single().ok().and_then(|window| scrolled(window, &windows, -notches.round() as i32)) { pointer.system.push(request); }
    }
    // State is written only when something changes: its change ticks tell
    // `sync` to rebuild the views.
    // A newly opened quest's overview starts at its top.
    let open_quest = quests.list.get(quests.selected).map(|q| q.id.as_str());
    if state.last_quest.as_deref() != open_quest { state.scroll.remove(super::quests::OVERVIEW); state.last_quest = open_quest.map(str::to_owned); }
    let tabs = windows.iter().find(|(w, _, _, _)| w.0 == LEADERBOARDS).map(|(_, _, c, _)| rank_tabs(c));
    let mut commands = Vec::new();
    let visible = || windows.iter().filter(|(_, v, _, _)| **v != Visibility::Hidden).map(|(w, _, _, _)| w.0.as_str());
    let boards_were_open = state.opened.contains(LEADERBOARDS);
    if visible().count() != state.opened.len() || !visible().all(|w| state.opened.contains(w)) {
        let open: HashSet<String> = visible().map(str::to_owned).collect();
        let opened: Vec<String> = open.difference(&state.opened).cloned().collect();
        for window in &opened {
            if window == LEADERBOARDS { state.rank_start = 0; }
            // The social window: our group, our guild, the players on our map.
            if window == SOCIAL {
                commands.extend(social_lists());
                commands.push(social_query(&mut state, player::PlayerQuery::ON_MAP, String::new()));
            }
            commands.extend(opening(window, &state, tabs));
        }
        state.opened = open;
    }
    // The boards follow their tabs; closed, they ask again when reopened.
    let boards_open = state.opened.contains(LEADERBOARDS);
    if boards_open && tabs.is_some() && tabs != state.rank_tabs {
        if boards_were_open { state.rank_start = 0; commands.extend(ranking_requests(&state, tabs.unwrap(), FILTER_ALL)); }
        state.rank_tabs = tabs;
    }
    if !boards_open && state.rank_tabs.is_some() { state.rank_tabs = None; }
    if state.pending.is_some_and(|(revision, at)| net.inventory.revision != revision || time.elapsed_secs_f64() - at > 2.0) { state.pending = None; }
    // One inventory-changing request at a time: the server answers with the
    // inventory (or refuses silently; two seconds then).
    let busy = state.pending.is_some();
    let mut wait = false;
    // A workbench used in the world: the crafting window, on its tab.
    if let Some(tab) = net.open_workbench.take() {
        if let Some(n) = catalog.workbench_tabs.iter().position(|t| t.id == tab) { state.workbench_tab = n; state.recipe_category = None; state.recipe = None; }
        pointer.open_window = Some(WORKBENCH);
    }
    // Closed, the desk forgets its item too: it opens empty, as the original's.
    // FAILURE: "quand je reouvre le work on gems and items j'ai toujours mon
    //   ancien item dedans".
    if (state.gem_target.is_some() || !state.gem_preview.is_empty() || !state.gem_reopen.is_empty()) && !state.opened.contains(GEMDESK) {
        state.gem_target = None; state.gem_preview.clear(); state.gem_reopen.clear();
    }
    // A placed gem the item now holds (locked in) or that left the bags is
    // dropped; an unlocked gem out of the item is placed again.
    if let Some(item) = state.gem_target.and_then(|id| net.inventory.items.get(&id)).cloned() {
        let held = |s: usize| item.names.get(s).is_some_and(|g| !g.is_empty());
        // As many placed of a gem as are carried.
        let carried = |t: &str| net.inventory.items.values().filter(|i| i.template == t
            && matches!(net.inventory.location(i.id), Some(Place::Bag(_) | Place::Extra(_)))).map(|i| i.third.max(1) as usize).sum::<usize>();
        let fits = |list: &[(usize, String)]| list.iter().enumerate().all(|(n, (s, t))| !held(*s) && list[..=n].iter().filter(|(_, u)| u == t).count() <= carried(t));
        if !fits(&state.gem_preview) {
            let mut kept: Vec<(usize, String)> = Vec::new();
            for (s, t) in std::mem::take(&mut state.gem_preview) {
                if !held(s) && kept.iter().filter(|(_, u)| *u == t).count() < carried(&t) { kept.push((s, t)); }
            }
            state.gem_preview = kept;
        }
        if state.gem_reopen.iter().any(|(s, _)| !held(*s)) {
            for (s, template) in std::mem::take(&mut state.gem_reopen) {
                if held(s) { state.gem_reopen.push((s, template)); continue; }
                if state.gem_preview.iter().filter(|(_, u)| *u == template).count() < carried(&template) { state.gem_preview.push((s, template)); }
            }
        }
    }
    // A right-clicked item on the workbench opens the book of a recipe it is
    // the first ingredient of: a known one first, a combination (a gem with
    // its dust to the next tier: GemCombinationCraftingCyanit_a_to_b) before
    // its dismantling (GemDustCreation).
    // FAILURE: "je peux pas fusionner quand j'ai le workbench ouvert et que je
    //   clique droit sur mes pierres".
    let picked = state.workbench_pick.take();
    let in_recipe = picked.as_ref().and_then(|t| views.workbench.detail.as_ref().filter(|d| state.recipe.is_some() && !d.locked)
        .and_then(|d| d.ingredients.iter().position(|i| &i.template == t)));
    if let Some(n) = in_recipe { state.workbench_placed.insert(n); }
    // Another item of the open book: its own recipe there (any gem or rune on
    // a dismantling book, not the one it showed).
    // FAILURE: "je devrais pouvoir mettre les runes / gemmes que je veux pas
    //   celle qui est deja affichee ... pour les destruction".
    let first_is = |r: &crate::gameplay::systems::RecipeDef, t: &str| r.ingredients.first().is_some_and(|i| i.split(':').next() == Some(t));
    let in_book = picked.as_ref().filter(|_| in_recipe.is_none()).and_then(|t| state.recipe.as_ref()
        .and_then(|b| catalog.recipe_books.get(b)).and_then(|b| b.recipes.iter().find(|r| catalog.recipes.get(*r).is_some_and(|r| first_is(r, t)))).cloned());
    if let Some(recipe) = &in_book {
        state.workbench_recipe = Some(recipe.clone());
        state.workbench_placed = std::iter::once(0).collect();
    }
    if let Some(template) = picked.filter(|_| in_recipe.is_none() && in_book.is_none()) {
        let known: Vec<String> = net.recipes.iter().map(|(book, _)| book.to_lowercase()).collect();
        let first_is = |r: &crate::gameplay::systems::RecipeDef| r.ingredients.first().is_some_and(|i| i.split(':').next() == Some(template.as_str()));
        let mut books: Vec<(&String, &crate::gameplay::systems::RecipeBookDef)> = catalog.recipe_books.iter()
            .filter(|(_, b)| b.recipes.iter().filter_map(|r| catalog.recipes.get(r)).any(first_is)).collect();
        books.sort_by_key(|(id, _)| (!catalog.book_known(id, &known), !id.contains("Combination"), (*id).clone()));
        if let Some((id, book)) = books.first() {
            let tab_id = catalog.workbench_categories.iter().find(|c| c.id == book.category).map(|c| c.tab.clone()).unwrap_or_default();
            if let Some(tab) = catalog.workbench_tabs.iter().position(|t| t.id == tab_id) { state.workbench_tab = tab; }
            state.recipe_category = Some(book.category.clone());
            state.recipe = Some((*id).clone());
            state.craft_amount = 1;
            state.scroll.remove(desks::LIST);
            // The clicked item goes in its first slot, its recipe chosen.
            state.workbench_placed = std::iter::once(0).collect();
            state.workbench_recipe = book.recipes.iter().find(|r| catalog.recipes.get(*r).is_some_and(first_is)).cloned();
        }
    }
    for request in std::mem::take(&mut pointer.system) {
        if busy && matches!(request, Request::AddSticker(_) | Request::Salvage | Request::Socket(_) | Request::BuySocket | Request::Craft(_) | Request::Apply | Request::TransmogReset(_)) { continue; }
        match request {
            Request::Page(paging) => {
                let total = net.ranking.as_ref().map(|p| p.total).or(net.overall_ranking.as_ref().map(|p| p.total)).unwrap_or(0);
                let last = total.saturating_sub(PAGE_ROWS) / PAGE_ROWS * PAGE_ROWS;
                let mut flags = FILTER_ALL;
                state.rank_start = match paging {
                    Paging::First => 0,
                    Paging::Previous => state.rank_start.saturating_sub(PAGE_ROWS),
                    Paging::Next => (state.rank_start + PAGE_ROWS).min(last),
                    Paging::Last => last,
                    Paging::Player => { flags |= FILTER_PLAYER_PAGE; state.rank_start }
                };
                commands.extend(ranking_requests(&state, tabs.unwrap_or_default(), flags));
            }
            Request::Queue(mode) => {
                let mode = if mode < 0 { QUEUE_MODES.get(state.matchmaking_tab as usize).copied().unwrap_or(1) } else { mode };
                if let Some(found) = net.battleground.take() {
                    net.touch_ui();
                    commands.push(ClientCommand::JoinBattlegroundResponse(proto::JoinBattlegroundResponse { tick: found.tick, battleground: found.battleground, accepted: true, unknown_u32_0: 0, mode: found.mode }));
                } else if net.matchmaking.as_ref().is_some_and(|m| m.queued()) {
                    commands.push(ClientCommand::AbortMatchQueue(proto::AbortMatchQueue { tick: 0, flag: true, modes: vec![mode], all: false }));
                    net.matchmaking = None;
                    net.touch_ui();
                } else {
                    commands.push(ClientCommand::JoinMatchQueue(social::JoinMatchQueue { tick: 0, mode, kind: 0 }));
                }
            }
            Request::Event(index) => { state.event = index; state.event_page = None; }
            Request::EventPage(delta) => {
                if let Some(event) = views.events.list.get(views.events.open) {
                    state.event_page = Some((event.page as i32 + delta).clamp(0, event.pages.saturating_sub(1) as i32) as usize);
                }
            }
            Request::Teleport => {
                let key = views.events.list.get(views.events.open).map(|e| e.key.clone()).unwrap_or_default();
                if let Some((map, entry)) = catalog.events.get(&key).and_then(|e| e.teleport.split_once(':')) {
                    commands.push(ClientCommand::Travel(player::Travel { source: String::new(), map: map.to_owned(), entry: entry.to_owned(), force: false }));
                }
            }
            // A premium offer's product key books it (the server's local payment
            // provider); an item offer is bought from the item shop.
            Request::Buy(offer) if offer.contains("realCurrency") => commands.push(ClientCommand::OcpBooking(proto::OcpBooking { offer, method: "external".into() })),
            Request::Buy(offer) => {
                commands.push(ClientCommand::Transaction(inventory::Transaction::buy(shop::ITEM_SHOP, &offer, 1)));
                wait = true;
            }
            Request::Sort(column) => {
                state.rank_sort = column;
                state.rank_start = 0;
                commands.extend(ranking_requests(&state, tabs.unwrap_or_default(), FILTER_ALL));
            }
            Request::Filter(bit) => {
                state.rank_filters ^= bit;
                state.rank_start = 0;
                commands.extend(ranking_requests(&state, tabs.unwrap_or_default(), FILTER_ALL));
            }
            Request::MatchTab(tab) => state.matchmaking_tab = tab,
            Request::ShopTab(tab) => {
                state.shop_tab = tab;
                state.scroll.remove(shop::OFFERS);
                if tab == 1 && state.shop_category.get(1).is_none_or(|c| c.is_none()) {
                    state.shop_category.resize(2, None);
                    state.shop_category[1] = Some("Andermant".into());
                }
            }
            Request::ShopCategory(category) => {
                let tab = state.shop_tab;
                if state.shop_category.len() <= tab { state.shop_category.resize(tab + 1, None); }
                state.shop_category[tab] = Some(category);
                state.scroll.remove(shop::OFFERS);
            }
            Request::Redeem => net.social_notes.push("Vouchers are not redeemed on this server.".to_owned()),
            Request::Sticker(template) => commands.push(ClientCommand::UseStickerBookItem(inventory::UseStickerBookItem { template })),
            // A sticker clicked is shown in the preview; right-clicked, also used.
            Request::Pick(id, use_it) => {
                // Not while its cooldown runs (a mount 0.5 s, a pet 2-5 s).
                if use_it {
                    if let Some(command) = use_sticker(&views.album, &id) {
                        let allowed = match &command {
                            ClientCommand::UseStickerBookItem(u) => {
                                item_cooldowns.start(items_catalogs.get(&items_data.catalog), &u.template, time.elapsed_secs_f64())
                            }
                            _ => true,
                        };
                        if allowed {
                            commands.push(command);
                            // Its icon pops (crate::ui::album_overlays), the window untouched.
                            pop_at.0 = Some((id.clone(), time.elapsed_secs_f64()));
                        }
                    }
                }
                if state.album_sticker.as_ref() != Some(&id) { state.album_sticker = Some(id); }
            }
            Request::AlbumBook(book, sticker) => {
                // A category with books under it only opens or closes their list;
                // the page keeps the book shown. A book (or a category without
                // books) is shown, its category's list open.
                // FAILURE (2026-10-23): a second click on Mounts did not close it;
                //   the original (the user's release261_192 shots) closes the list
                //   and still shows the last book.
                let def = catalog.stickerbooks.iter().find(|b| b.id == book);
                let top_of = |category| catalog.stickerbooks.iter().find(|b| b.category == category && b.subcategory < 0).map(|t| t.id.clone());
                let has_books = def.is_some_and(|d| d.subcategory < 0 && catalog.stickerbooks.iter().any(|b| b.category == d.category && b.subcategory >= 0));
                if has_books {
                    state.album_open = if state.album_open.as_deref() == Some(book.as_str()) { None } else { Some(book) };
                } else {
                    state.album_open = def.filter(|d| d.subcategory >= 0).and_then(|d| top_of(d.category));
                    state.album_book = Some(book);
                    state.album_sticker = sticker;
                    state.pages.remove(ALBUM);
                }
            }
            Request::Banner(n) => state.album_banner = n,
            Request::Favorite(id, favorite) => commands.push(ClientCommand::SetStickerbookFavorite(proto::SetStickerbookFavorite { id, favorite })),
            // The tier owned goes up, under the name the server keeps it by
            // (its item); not owned, or too few Altered Crescerite: nothing.
            // FAILURE (2026-10-31): sent the sticker's id, which the server
            //   (services/customization.py) does not own under.
            // Asked first, as the original (upgrade::sync sends it on Confirm).
            Request::Upgrade(id) => {
                let have = net.game_currencies.get(UPGRADE_CURRENCY).copied().unwrap_or(0);
                if let Some(sticker) = views.album.sticker(&id).filter(|s| s.owned && s.next.as_ref().is_some_and(|(_, cost)| have >= u64::from(*cost))) {
                    let (next, cost) = sticker.next.clone().unwrap_or_default();
                    let name = items_catalogs.get(&items_data.catalog).map(|c| c.name(&next, None).trim().to_owned()).unwrap_or_else(|| next.clone());
                    pointer.upgrade_ask = Some(super::upgrade::Ask { key: sticker.server_key().to_owned(), name, cost });
                }
            }
            Request::AddSticker(id) => {
                let sources = Sources { net: &net, catalog, items: None, names: None, state: &state, class: None, talents: None, bonuses: None };
                let item = catalog.stickers.get(&id).into_iter().flat_map(album::templates).find_map(|t| sources.carried(t).first().copied());
                if let Some(item) = item { commands.push(ClientCommand::AddStickerbookFromInventory(proto::AddStickerbookFromInventory { item })); wait = true; }
            }
            Request::Salvage => {
                let items: Vec<u32> = state.salvage.iter().copied().filter(|id| net.inventory.items.contains_key(id)).collect();
                if !items.is_empty() { commands.push(ClientCommand::Salvage(proto::Salvage { items, mode: 0 })); wait = true; }
                state.salvage.clear();
            }
            Request::SocketGem(_) | Request::BenchItem(_) | Request::Reward(_) => {}
            Request::Unpick(index) => { if index < state.salvage.len() { state.salvage.remove(index); } }
            // A placed gem right-clicked goes back; a set one is taken out.
            Request::TakeGem(socket) if state.gem_preview.iter().any(|(s, _)| *s == socket) => state.gem_preview.retain(|(s, _)| *s != socket),
            Request::TakeGem(socket) => {
                if let Some(target) = state.gem_target.filter(|id| net.inventory.items.get(id).is_some_and(|i| i.names.get(socket).is_some_and(|g| !g.is_empty()))) {
                    commands.push(crate::gameplay::inventory::command(target, 0x14, vec![socket as i32]));
                    wait = true;
                }
            }
            // The open padlock locks the placed gem in (it stays shown until the
            // item has it); the closed one unlocks it: taken out, then placed again.
            Request::Socket(socket) if state.gem_preview.iter().any(|(s, _)| *s == socket) => {
                let template = state.gem_preview.iter().find(|(s, _)| *s == socket).map(|(_, g)| g.clone()).unwrap_or_default();
                let sources = Sources { net: &net, catalog, items: None, names: None, state: &state, class: None, talents: None, bonuses: None };
                let gem = sources.carried(&template).first().copied();
                if let (Some(target), Some(gem)) = (state.gem_target.filter(|id| net.inventory.items.contains_key(id)), gem) {
                    commands.push(crate::gameplay::inventory::command(gem, 0x11, vec![target as i32, socket as i32]));
                    wait = true;
                }
            }
            Request::Socket(socket) => {
                let gem = state.gem_target.and_then(|id| net.inventory.items.get(&id)).and_then(|i| i.names.get(socket).filter(|g| !g.is_empty()).cloned());
                if let (Some(target), Some(gem)) = (state.gem_target, gem) {
                    commands.push(crate::gameplay::inventory::command(target, 0x14, vec![socket as i32]));
                    state.gem_reopen.push((socket, gem));
                    wait = true;
                }
            }
            Request::BuySocket => {
                if let Some(target) = state.gem_target.filter(|id| net.inventory.items.contains_key(id)) {
                    commands.push(crate::gameplay::inventory::command(target, 0x15, vec![]));
                    wait = true;
                }
            }
            // A category opens; clicked again, it closes.
            Request::RecipeCategory(category) => {
                state.recipe_category = if state.recipe_category.as_ref() == Some(&category) { None } else { Some(category) };
                state.recipe = None;
            }
            Request::Recipe(id) => { state.recipe = Some(id).filter(|id| !id.is_empty()); state.craft_amount = 1; state.workbench_placed.clear(); state.workbench_recipe = None; }
            Request::WorkbenchTab(tab) => {
                state.workbench_tab = tab;
                state.recipe_category = None;
                state.recipe = None;
                state.scroll.remove(desks::LIST);
            }
            Request::WorkbenchFilter(0) => { state.workbench_craftable_only = !state.workbench_craftable_only; state.scroll.remove(desks::LIST); }
            Request::WorkbenchFilter(_) => { state.workbench_locked_only = !state.workbench_locked_only; state.scroll.remove(desks::LIST); }
            Request::CraftAmount(step) => {
                let most = views.workbench.detail.as_ref().map_or(1, |d| d.most.max(1)) as i64;
                state.craft_amount = match step {
                    i32::MIN => 1,
                    i32::MAX => most,
                    step => (state.craft_amount.max(1) as i64 + step as i64).clamp(1, most),
                } as u32;
            }
            // The chosen book's recipe, the amount chosen, a carried item of each
            // ingredient (the server takes the amounts by template).
            Request::Craft(andermant) => {
                let mut crafted = false;
                let sources = Sources { net: &net, catalog, items: None, names: None, state: &state, class: None, talents: None, bonuses: None };
                if let Some(detail) = views.workbench.detail.as_ref().filter(|d| !d.locked && d.most > 0 && !d.recipe.is_empty()
                    && (0..d.ingredients.len()).all(|n| state.workbench_placed.contains(&n))) {
                    crafted = true;
                    let mut items = Vec::new();
                    for ingredient in &detail.ingredients {
                        let carried = sources.carried(&ingredient.template);
                        if let Some(item) = carried.iter().find(|id| !items.contains(*id)).or(carried.first()) { items.push(*item); }
                    }
                    if items.len() == detail.ingredients.len() {
                        commands.push(ClientCommand::Crafting(proto::Crafting { items, currency: andermant as u8, slot: 0, amount: detail.amount.max(1), recipe: detail.recipe.clone(), mode: 0, ..default() }));
                        wait = true;
                    }
                }
                if crafted { state.workbench_placed.clear(); }
            }
            Request::Skin(id) => state.skin = Some(id),
            // The chosen skin (or dye) on the picked slot's item; the old one
            // named as removed (item_customization_protocol checks it).
            Request::Apply => {
                let item = state.skin_slot.and_then(|slot| net.inventory.at(Place::Equipment(slot)).map(|i| (slot, i.clone())));
                if let (Some((slot, item)), Some(choice)) = (item, state.skin.clone()) {
                    let change = if state.transmog_tab == 1 {
                        proto::CustomizationChange { slot: slot_type(slot), apply_color: choice, remove_color: dye_of(&item), ..default() }
                    } else {
                        proto::CustomizationChange { slot: slot_type(slot), apply_skin: choice.clone(), remove_skin: item.fourth_name.clone(), skin: choice, ..default() }
                    };
                    commands.push(ClientCommand::ApplyItemCustomization(proto::ApplyItemCustomization { changes: vec![change] }));
                    wait = true;
                }
            }
            Request::TransmogReset(slot) => {
                if let Some(item) = net.inventory.at(Place::Equipment(slot)).cloned() {
                    let change = if state.transmog_tab == 1 {
                        proto::CustomizationChange { slot: slot_type(slot), remove_color: dye_of(&item), ..default() }
                    } else {
                        proto::CustomizationChange { slot: slot_type(slot), remove_skin: item.fourth_name.clone(), ..default() }
                    };
                    if !change.remove_color.is_empty() || !change.remove_skin.is_empty() {
                        commands.push(ClientCommand::ApplyItemCustomization(proto::ApplyItemCustomization { changes: vec![change] }));
                        wait = true;
                    }
                }
            }
            Request::TransmogTab(tab) => {
                state.transmog_tab = tab;
                state.skin = None;
                state.pages.remove(TRANSMOG);
            }
            Request::TransmogSlot(slot) => {
                state.skin_slot = Some(slot);
                state.skin = None;
                state.pages.remove(TRANSMOG);
            }
            Request::TransmogFilter(0) => { state.transmog_owned = !state.transmog_owned; state.pages.remove(TRANSMOG); }
            Request::TransmogFilter(_) => { state.transmog_shop = !state.transmog_shop; state.pages.remove(TRANSMOG); }
            Request::AchievementCategory(category) => {
                state.achievement_category = Some(category).filter(|c| !c.is_empty());
                state.achievement_open = None;
                state.scroll.remove(log::ROWS);
            }
            Request::Achievement(id) => state.achievement_open = if state.achievement_open.as_ref() == Some(&id) { None } else { Some(id) },
            Request::Scroll(key, rows) => {
                let most = scroll_limit(&key, &views, &quests);
                let entry = state.scroll.entry(key).or_default();
                *entry = (*entry as i32 + rows).clamp(0, most as i32) as usize;
            }
            Request::Daily(id, refresh) => commands.push(ClientCommand::DailyChallengeSelection(proto::DailyChallengeSelection { id, refresh })),
            Request::Wanted(slot) => commands.push(ClientCommand::AcceptWantedThiefMission(proto::WantedThiefSlot { slot })),
            Request::WantedRefresh(slot) => commands.push(ClientCommand::RefreshWantedThiefMission(proto::WantedThiefSlot { slot })),
            Request::Title(kind, title) => {
                commands.push(ClientCommand::SetTitle(proto::SetTitle { kind, title: title.clone() }));
                state.title = Some((kind, title));
            }
            Request::Turn(window, delta) => {
                let last = match window {
                    ALBUM => views.album.pages,
                    _ => views.transmog.choices.len().div_ceil(transmog::per_page(views.transmog.tab)),
                }.max(1) - 1;
                let page = state.pages.entry(window).or_default();
                *page = (*page as i32 + delta).clamp(0, last as i32) as usize;
            }
            // The name typed in the search panel; none: the map's players.
            Request::SocialSearch => {
                let text = windows.iter().find(|(w, _, _, _)| w.0 == SOCIAL)
                    .and_then(|(_, _, c, _)| c.entries.iter().find(|(id, _)| id.ends_with("/InputQuery")).map(|(_, e)| e.text.trim().to_owned()))
                    .unwrap_or_default();
                let kind = if text.is_empty() { player::PlayerQuery::ON_MAP } else { player::PlayerQuery::EVERYONE };
                state.scroll.remove(rosters::LISTS[0].2);
                commands.push(social_query(&mut state, kind, text));
            }
            Request::SocialRefresh => {
                let (kind, text) = state.social_query.clone().unwrap_or((player::PlayerQuery::ON_MAP, String::new()));
                commands.extend(social_lists());
                commands.push(social_query(&mut state, kind, text));
            }
            // A column sorts up, then down, then not.
            Request::SocialSort(column) => {
                state.social_sort = match state.social_sort {
                    Some((sorted, false)) if sorted == column => Some((column, true)),
                    Some((sorted, true)) if sorted == column => None,
                    _ => Some((column, false)),
                };
            }
            Request::Player(_) => {}
            Request::Talent(_) | Request::Skill(_) | Request::TalentMore(_) | Request::TalentLevels(..) | Request::Preset(..) | Request::UnlockPreset(_)
            | Request::Reset(_) | Request::ResetBranch(..) | Request::TalentAnswer(_) | Request::Workbench | Request::TalentShop(_) => {
                commands.extend(talents::request(request, &views.talents, &mut state, &mut net));
            }
        }
    }
    if net.offline || commands.is_empty() { return; }
    for command in &commands { net.send(command); }
    if wait { state.pending = Some((net.inventory.revision, time.elapsed_secs_f64())); }
}

/// What the social window asks for our group and guild: GroupList type 0 (a
/// GroupStatus), GuildList (a GuildStatus and the ignore list), its body as the
/// game client sent it (the server reads neither body).
fn social_lists() -> [ClientCommand; 2] {
    let guild = dsor_proto::commands::Raw { data: [0u8; 17].into_iter().chain([0xFF]).collect(), bits: 144 };
    [ClientCommand::GroupList(social::GroupList::default()), ClientCommand::GuildList(social::GuildList { unknown_body: guild })]
}

/// A player search under a new id, kept for Update (the server echoes the id in
/// its PlayerQueryResult).
fn social_query(state: &mut State, kind: u8, text: String) -> ClientCommand {
    state.social_query_id = state.social_query_id.wrapping_add(1);
    state.social_query = Some((kind, text.clone()));
    ClientCommand::PlayerQuery(player::PlayerQuery { kind, query_id: state.social_query_id, text })
}

/// An item's dye (ItemInfo.named, "none" without one).
fn dye_of(item: &inventory::ItemInfo) -> String {
    if item.named == "none" { String::new() } else { item.named.clone() }
}

/// A desk's pick of a carried or worn item (crate::ui::controls::items::input:
/// a right click while the desk is open): true when taken.
pub(in crate::ui) fn pick(state: &mut State, desk: &str, net: &Net, item: &inventory::ItemInfo, category: &str) -> Option<ClientCommand> {
    let carried = matches!(net.inventory.location(item.id), Some(Place::Bag(_) | Place::Extra(_)));
    let worn = matches!(net.inventory.location(item.id), Some(Place::Equipment(_)));
    match desk {
        SALVAGE if carried && !state.salvage.contains(&item.id) && state.salvage.len() < desks::SALVAGE_SLOTS => { state.salvage.push(item.id); None }
        GEMDESK if matches!(category, "EquipmentGem" | "EquipmentJewel" | "EquipmentRune") && carried => {
            // A gem is placed on the desk's item's first free socket; its
            // padlock then sets it (op 0x11, Request::Socket).
            // FAILURE: set at once, the padlock the player clicked to lock it
            //   took it out again ("quand je verrouille le slot ca me la remet
            //   en inventaire").
            let target = net.inventory.items.get(&state.gem_target?)?;
            let carried: u32 = net.inventory.items.values().filter(|i| i.template == item.template
                && matches!(net.inventory.location(i.id), Some(Place::Bag(_) | Place::Extra(_)))).map(|i| i.third.max(1)).sum();
            if state.gem_preview.iter().filter(|(_, t)| *t == item.template).count() as u32 >= carried { return None; }
            let socket = (0..target.tenth as usize).find(|n| target.names.get(*n).is_none_or(|g| g.is_empty())
                && !state.gem_preview.iter().any(|(s, _)| s == n))?;
            state.gem_preview.push((socket, item.template.clone()));
            None
        }
        GEMDESK if carried || worn => { state.gem_target = Some(item.id); state.gem_preview.clear(); None }
        WORKBENCH if carried || net.inventory.allocation.words_180.contains(&item.id) => { state.workbench_pick = Some(item.template.clone()); None }
        TRANSMOG if worn => {
            if let Some(Place::Equipment(slot)) = net.inventory.location(item.id) { state.skin_slot = Some(slot); }
            state.skin = None;
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An audit of the real layouts (needs the converted assets): every button
    /// a window offers once prepared, its action and whether its page shows.
    /// `cargo test -p dsor-client audit_buttons -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn audit_buttons() {
        let filter = std::env::var("DSOR_AUDIT").unwrap_or_default();
        for name in [LEADERBOARDS, MATCHMAKING, SHOP, EVENTS, ALBUM, TRANSMOG, SALVAGE, GEMDESK, WORKBENCH, QUESTLOG, SHEET, super::super::quests::TRACKER] {
            if !filter.is_empty() && !filter.split(',').any(|f| f == name) { continue; }
            let Ok(bytes) = std::fs::read(format!("../assets/interface/{name}.ui.json")) else { println!("{name}: no layout"); continue };
            let source: Layout = serde_json::from_slice(&bytes).unwrap();
            let layout = prepare(&source, &Views::default(), &super::super::quests::Quests::default()).unwrap_or(source);
            let controls = WindowControls::new(&layout);
            println!("== {name}: {} buttons", controls.buttons.len());
            for b in &controls.buttons {
                let shown = controls.section_visible(b.section);
                println!("  {:<11} {:<5} k={:.1} {:?} {}", format!("{:?}", b.action).chars().take(40).collect::<String>().split('(').next().unwrap_or(""), if shown { "shown" } else { "page" }, b.k, b.action, b.id);
            }
        }
    }

    fn widget(event: &str, id: &str) -> Widget {
        let layout = super::super::template::fixture::layout("w", &[(id, "Button", false)]);
        let mut w = layout.widgets[0].clone();
        w.event = event.into();
        w
    }

    #[test]
    fn events_name_their_requests() {
        assert_eq!(action(ALBUM, &widget("Sys:Favorite:sb_x:1", "a/b")), Some(Action::System(Request::Favorite("sb_x".into(), true))));
        assert_eq!(action(ALBUM, &widget("Sys:Sticker:sb_x", "a/b")), Some(Action::System(Request::Sticker("sb_x".into()))));
        assert_eq!(action(QUESTLOG, &widget("Sys:Daily:bonus_1:0", "a/b")), Some(Action::System(Request::Daily("bonus_1".into(), false))));
        assert_eq!(action(GEMDESK, &widget("SocketButton", "Canvas/BackGroundLabel/SocketsBackgroundLabel/SocketsLabel/Socket2/Button")), Some(Action::System(Request::Socket(2))));
        assert_eq!(action(EVENTS, &widget("", "Canvas/Eventwindow/TabBarLabel/TabEvent3")), Some(Action::System(Request::Event(3))));
        assert_eq!(action(LEADERBOARDS, &widget("PagePlayer", "a/ScrollToMe")), Some(Action::System(Request::Page(Paging::Player))));
        assert_eq!(action(SHEET, &widget("ShowPVPTitle", "a/RadioButtonShowPVPTitle")), Some(Action::System(Request::Title(2, String::new()))));
        assert_eq!(action("inventory", &widget("Close", "a/b")), None);
    }

    #[test]
    fn a_click_previews_a_sticker_and_a_right_click_uses_an_owned_one() {
        let pick = action(ALBUM, &widget("Sys:Pick:sb_x", "a/b")).unwrap();
        assert_eq!(pick, Action::System(Request::Pick("sb_x".into(), false)));
        assert_eq!(right_clicked(&pick, ALBUM, None), Some(Request::Pick("sb_x".into(), true)));
        assert_eq!(right_clicked(&Action::System(Request::Favorite("sb_x".into(), true)), ALBUM, None), None);
        // Used under the server's key; a sticker not collected is only shown.
        let owned = album::Sticker { id: "sb_x".into(), key: "mount_x".into(), owned: true, ..Default::default() };
        let missing = album::Sticker { id: "sb_y".into(), ..Default::default() };
        let shown = album::Album { stickers: vec![owned, missing], ..Default::default() };
        assert!(matches!(use_sticker(&shown, "sb_x"), Some(ClientCommand::UseStickerBookItem(u)) if u.template == "mount_x"));
        assert!(use_sticker(&shown, "sb_y").is_none());
    }

    /// docs/ui.md, "Per-frame cost". FAILURE (2026-10-21): every view was
    /// rebuilt every frame, open or not (1 ms a frame).
    #[test]
    fn views_are_built_only_for_open_windows_and_only_when_something_changed() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<SystemsCatalog>().init_asset::<items::Catalog>().init_asset::<Layout>().init_asset::<crate::ui::local_map::MapNames>()
            .init_resource::<State>().init_resource::<Views>().init_resource::<Loaded>().init_resource::<crate::ui::UiWork>()
            .add_systems(Update, sync);
        crate::network::net::start_offline(app.world_mut(), 1, 0);
        let album = app.world_mut().spawn((Window2018(ALBUM.into()), Visibility::Hidden)).id();
        let built = |app: &App| app.world().resource::<crate::ui::UiWork>().system_views;
        let show = |app: &mut App, visibility| *app.world_mut().get_mut::<Visibility>(album).unwrap() = visibility;
        for _ in 0..5 { app.update(); }
        assert_eq!(built(&app), 0, "a closed window's view waits until it opens");
        show(&mut app, Visibility::Inherited);
        app.update();
        assert_eq!(built(&app), 1, "opened: its view is built");
        for _ in 0..10 { app.update(); }
        assert_eq!(built(&app), 1, "idle frames rebuild nothing");
        app.world_mut().non_send_mut::<Net>().touch_ui();
        app.update();
        assert_eq!(built(&app), 2, "the server said something");
        app.world_mut().resource_mut::<State>().album_banner = 1;
        app.update();
        assert_eq!(built(&app), 3, "a selection changed");
        app.world_mut().non_send_mut::<Net>().inventory.revision += 1;
        app.update();
        assert_eq!(built(&app), 4, "the inventory changed");
        show(&mut app, Visibility::Hidden);
        app.world_mut().non_send_mut::<Net>().touch_ui();
        for _ in 0..5 { app.update(); }
        assert_eq!(built(&app), 4, "closed: changes wait for it to open");
        show(&mut app, Visibility::Inherited);
        app.update();
        app.update();
        assert_eq!(built(&app), 5, "reopened after a change: rebuilt once");
    }

    /// The social window's lists follow what the server said (the search, the
    /// group, the guild) and the sort, with no work on idle frames.
    #[test]
    fn the_social_window_follows_the_search_group_and_guild_without_idle_work() {
        use dsor_proto::commands::social::{GroupMember, GroupStatus, PlayerQueryResult};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<SystemsCatalog>().init_asset::<items::Catalog>().init_asset::<Layout>().init_asset::<crate::ui::local_map::MapNames>()
            .init_resource::<State>().init_resource::<Views>().init_resource::<Loaded>().init_resource::<crate::ui::UiWork>()
            .add_systems(Update, sync);
        crate::network::net::start_offline(app.world_mut(), 1, 0);
        app.world_mut().spawn((Window2018(SOCIAL.into()), Visibility::Inherited));
        let built = |app: &App| app.world().resource::<crate::ui::UiWork>().system_views;
        let rosters = |app: &App| app.world().resource::<Views>().rosters.clone();
        app.update();
        assert_eq!(built(&app), 1);
        for _ in 0..10 { app.update(); }
        assert_eq!(built(&app), 1, "idle frames rebuild nothing");
        let player = |name: &str, level: u16, role: u8| GroupMember { name: name.into(), level, online: true, available: true, invitable: true, role, ..Default::default() };
        {
            let mut net = app.world_mut().non_send_mut::<Net>();
            net.player_query = Some(PlayerQueryResult { query_id: 1, players: vec![player("Ann", 30, 0xFF), player("Bob", 12, 0xFF)] });
            net.touch_ui();
        }
        app.update();
        assert_eq!(built(&app), 2);
        let view = rosters(&app);
        assert_eq!(view.lists[0].iter().map(|p| (p.name.as_str(), p.grouped)).collect::<Vec<_>>(), [("Ann", false), ("Bob", false)]);
        assert!(!view.grouped);
        // Grouped with Ann: her search row shows the group icon, the group tab lists us both.
        {
            let mut net = app.world_mut().non_send_mut::<Net>();
            let mut me = player("Me", 40, 0);
            me.player_id = 9;
            net.group = Some(GroupStatus { group_id: 7, leader_id: 9, recipient_id: 9, members: vec![me, player("Ann", 30, 1)], ..Default::default() });
            net.touch_ui();
        }
        app.update();
        let view = rosters(&app);
        assert!(view.grouped && view.lists[0][0].grouped && !view.lists[0][1].grouped);
        assert_eq!(view.lists[1].iter().map(|p| (p.name.as_str(), p.leader, p.me)).collect::<Vec<_>>(), [("Me", true, true), ("Ann", false, false)]);
        // A column sorts the lists.
        app.world_mut().resource_mut::<State>().social_sort = Some((rosters::Column::Level, false));
        app.update();
        assert_eq!(rosters(&app).lists[0][0].name, "Bob");
        let before = built(&app);
        for _ in 0..10 { app.update(); }
        assert_eq!(built(&app), before);
    }

    #[test]
    fn the_social_window_asks_for_its_lists_and_searches_by_name() {
        let mut state = State::default();
        let lists = social_lists();
        assert!(matches!(&lists[0], ClientCommand::GroupList(g) if g.kind == 0));
        assert!(matches!(&lists[1], ClientCommand::GuildList(g) if g.unknown_body.bits == 144));
        let first = social_query(&mut state, player::PlayerQuery::ON_MAP, String::new());
        let second = social_query(&mut state, player::PlayerQuery::EVERYONE, "Ann".into());
        assert!(matches!(first, ClientCommand::PlayerQuery(q) if q.kind == 1 && q.query_id == 1 && q.text.is_empty()));
        assert!(matches!(second, ClientCommand::PlayerQuery(q) if q.kind == 0 && q.query_id == 2 && q.text == "Ann"));
        assert_eq!(state.social_query, Some((0, "Ann".into())));
        assert_eq!(action(SOCIAL, &widget("Sys:Scroll:social_search:-1", "a/b")), Some(Action::System(Request::Scroll("social_search".into(), -1))));
        assert_eq!(action(SOCIAL, &widget("Sys:Player:Ann", "a/b")), Some(Action::System(Request::Player("Ann".into()))));
        assert_eq!(action(SOCIAL, &widget("SortLevel", "a/b")), Some(Action::System(Request::SocialSort(rosters::Column::Level))));
        assert_eq!(action(SOCIAL, &widget("Enter", "a/b")), Some(Action::System(Request::SocialSearch)));
        assert_eq!(action(SOCIAL, &widget("Refresh", "a/b")), Some(Action::System(Request::SocialRefresh)));
    }

    #[test]
    fn opening_windows_ask_the_server_and_the_boards_follow_their_tabs() {
        let state = State::default();
        assert!(matches!(&opening(SHOP, &state, None)[..], [ClientCommand::OcpOfferRequest(_), ClientCommand::RequestOffer(r)] if r.by_name && r.shop == shop::ITEM_SHOP));
        assert!(matches!(&opening(EVENTS, &state, None)[..], [ClientCommand::ActiveEvents(_), ClientCommand::RequestOffer(r)] if r.shop == shop::ITEM_SHOP));
        let overall = ranking_requests(&state, (0, 0, 0), FILTER_ALL);
        assert!(matches!(&overall[0], ClientCommand::OverallRankingRequest(r) if r.class == EVERY_CLASS && r.count == PAGE_ROWS && r.start == 0));
        let honour = ranking_requests(&state, (0, 1, 2), FILTER_PLAYER_PAGE);
        assert!(matches!(&honour[0], ClientCommand::HonorRankingRequest(r) if r.board == LIFETIME_BOARD && r.class == 1 && r.start == UNRANKED && r.flags == FILTER_PLAYER_PAGE));
        assert!(matches!(&honour[1], ClientCommand::LocalRankingRequest(r) if r.board == LIFETIME_BOARD));
        assert_eq!(slot_type(7), 12);
        assert_eq!(slot_type(3), 3);
    }
}
