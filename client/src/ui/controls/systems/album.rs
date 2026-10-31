//! The collector's album (collectorsalbum, UI::StickerbookWidget).
//!
//! The album's category list holds its books by category (the front page, the
//! costumes, emotes, mounts...), the open category's books under it. The
//! front page shows the banners (their counters switch them), the new stickers
//! (a click opens their book) and the favourites' grid; a book's page shows
//! its title and text, its stickers' grid (six by four a page, the owned ones
//! bright; a click shows one in the preview, a second click uses an owned
//! one: a mount, a costume, a pet, an emote), the preview (its name, origin,
//! background, favourite flag, its offer to buy, the sticker turning in 3D --
//! crate::ui::preview -- or, where it has no model, its icon) and the book's
//! progress: the stickers collected along a bar with the book's rewards.
//! SOURCE: DSOR_Server services/customization.py (_handle_use_stickerbook_item,
//!   _handle_add_stickerbook_from_inventory, _handle_item_customization),
//!   protocol/item_customization_protocol.py (the skin must fit the item's slot
//!   type and class; ApplyPriceRC/VC); _Template_Stickerbook
//!   (StickerbookRewardItems "count:item:amount"), _Template_StickerbookItem
//!   (IconBrush, BackgroundTexture, StickerbookOffer, IsNew),
//!   _Template_StickerbookBanner.
use dsor_proto::commands::systems::StickerbookEntry;

use super::super::template::{clip_x, clone_at, clone_rows, dim, has_part, icon_layers, image_art, is_template, leaf, make_button, rename, selected, set_text, without, Icon};
use super::{Sources, ALBUM};
use crate::gameplay::systems::texture_path;
use crate::ui::Layout;

const INLAY: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay";
pub(super) const LIST: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/CategoriesListBackground/CategoriesList";
const PAGE: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/Categorypage";
const FRONT: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/Firstpage";
const PREVIEW: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/Categorypage/background_preview";
const BAR: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/Categorypage/ProgressLabel/ProgressBarLabel/ProgressBarLabel2/ProgressBar";
const REWARD: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/Categorypage/ProgressLabel/ItemTemplates/ItemTemplate";
const ENTRY: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/TemplateListEntry";
/// A sticker cooling down: a dark sweep from 12 o'clock, shrinking clockwise,
/// and the seconds left ("0.4 s" .. "0.0 s") big in the middle, on EVERY owned
/// sticker of the used one's cooldown category (all mounts and pets after a
/// mount), as the quick slots draw it; no glow after. The text is the layout's
/// AnimUse/slot_anim (its colour and outline), centred.
/// EVIDENCE: the user's original recording 2026-10-30 (20261030-1640-20, 11.3 s
///   on, frame by frame): the sweep darkens to 0.58 (black at 0.42), the
///   digits are about 15 units high-em in the 53-unit icon.
/// FAILURE (2026-10-30): only the used sticker, its colours dimmed, the text
///   small in its corner (slot_anim's align 2 is our right/bottom), then a
///   green glow the original has not.
const ANIM_USE_TEXT: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/AnimUse/slot_anim";
pub(in crate::ui) const SWEEP_ALPHA: f32 = 0.42;
/// The front page's text, gui.window favoritesDesc (the layout's own text is
/// an older one: "Here you will find all your favourite items...").
/// EVIDENCE: the user's release261_1024 recording 20261030-1714-37.
const FAVOURITES_TEXT: &str = "Select up to thirty of your favorite items to be shown here for quick access to your most frequently used items.";
/// The digits' size: the original's "4 s" is 0.17 of the sticker's height, ours
/// at 15 was 0.28 (the user's recording 20261030-1714-37: "double the size").
pub(in crate::ui) const COUNT_SIZE: f32 = 9.3;
/// The countdown's marker in a grid: `{page}/Count{i}#{cooldown key}`, drawn
/// every frame by crate::ui::album_overlays; the pop's: `{page}/Pop#{sticker}`.
pub(in crate::ui) const COUNT_MARK: &str = "/Count";
pub(in crate::ui) const POP_MARK: &str = "/Pop#";
/// A sticker cell spans 214..286 by 202..271.
const CELL: (f32, f32) = (72.0, 69.0);
const CELL_ORIGIN: (f32, f32) = (214.0, 202.0);
/// A book's grid: six by four from (214, 202); the front page's favourites:
/// ten by three from (227, 418).
const COLUMNS: usize = 6;
const PAGE_ROWS: usize = 4;
pub(super) const PER_PAGE: usize = COLUMNS * PAGE_ROWS;
const FRONT_ORIGIN: (f32, f32) = (227.0, 418.0);
const FRONT_COLUMNS: usize = 10;
const FRONT_ROWS: usize = 3;
/// The category list: from 109 to 603.
const LIST_TOP: f32 = 109.0;
const LIST_BOTTOM: f32 = 603.0;
/// The preview's sticker icon, in its turning area (no model to turn).
const PREVIEW_ICON: [f32; 4] = [741.0, 245.0, 881.0, 385.0];
/// The widget drawing the sticker turning (crate::ui::preview's album stage),
/// over the preview's background, under its flags and offer.
pub(in crate::ui) const TURNING: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/Categorypage/background_preview/previewBackground/background_image/Turning";
const FRONT_BOOK: &str = "sb_frontpage";
const FAVOURITE: &str = "textures/ui/sticker_favorite.png";
const PREMIUM_ORIGIN: &str = "ui/icon_origin_premium";
const MOST_NEW: usize = 4;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Sticker {
    pub id: String,
    pub book: String,
    pub name: String,
    pub icon: Icon,
    /// The item template its card shows (crate::ui::controls::tooltips): the tier
    /// owned, else the sticker's own item.
    pub template: String,
    pub owned: bool,
    /// The name the server owns it under (Net::stickerbook's key: the item's
    /// resource id, mythical_mount_manticore_01, not the sticker's
    /// sb_mount_wild_31_...): what a favourite request names. Empty: `id`.
    pub key: String,
    pub favourite: bool,
    pub addable: bool,
    /// Has a next rarity tier: its arrow shows, owned or not, as the original's.
    pub upgradable: bool,
    /// That next tier's item and its price in Altered Crescerite (the tier
    /// owned, else the sticker's own item, is the one it follows).
    pub next: Option<(String, u32)>,
    /// Owned, and its next tier affordable: the arrow green, else grey.
    pub affordable: bool,
    /// Its origin's icon, its preview background (textures), its offer.
    pub origin: String,
    pub background: String,
    pub offer: String,
    /// Its cooldown key: every cell carries its countdown and pop markers, drawn
    /// when they run (crate::ui::album_overlays), so a use needs no rebuild.
    pub countdown: Option<String>,
}

impl Album {
    /// A sticker shown on the page, among the new ones or in the preview.
    pub(in crate::ui) fn sticker(&self, id: &str) -> Option<&Sticker> {
        self.stickers.iter().chain(&self.new).chain(&self.preview).find(|s| s.id == id)
    }
}

impl Sticker {
    /// The name to send for it: the server's key once owned, else its id.
    pub(in crate::ui) fn server_key(&self) -> &str {
        if self.key.is_empty() { &self.id } else { &self.key }
    }
}

/// A row of the category list: a category (its top book) or a book under it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Book {
    pub id: String,
    pub title: String,
    pub sub: bool,
    pub selected: bool,
}

/// The album: its list, the front page or the open book's page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Album {
    pub books: Vec<Book>,
    pub front: bool,
    pub title: String,
    pub description: String,
    /// The page's stickers (the front page's: the favourites), the grid's
    /// page shown, of how many.
    pub stickers: Vec<Sticker>,
    pub page: usize,
    pub pages: usize,
    pub preview: Option<Sticker>,
    /// The stickers collected of the book's, its rewards (count, icon).
    pub progress: (u32, u32),
    pub rewards: Vec<(u32, Icon)>,
    /// Each reward's hover card.
    pub reward_cards: Vec<RewardView>,
    /// The front page's banner, of how many, and the new stickers.
    pub banner: String,
    pub banner_index: usize,
    pub banners: usize,
    pub new: Vec<Sticker>,
    pub scroll: usize,
    /// The Collector's Bag bonuses earned (actor variable -> millionths): each
    /// book's rewards reached by its owned stickers, as the variables they
    /// raise (sb_pets_increased_hitpoints_01: pet_hitpoint_01 + 0.03), up to
    /// their limits. A pet's card shows them (items::preview::sticker).
    /// EVIDENCE: the user's original account: 5 resistance rewards = "+ 15.000%
    ///   all resistance values", 3 damage = "+ 6.00% damage", 1 block = 2.00%.
    /// UNVERIFIED: that the server applies them (ours does not track them yet).
    pub bag: std::collections::BTreeMap<String, u32>,
    /// Our class ("warrior"...): a pet card's resource line (Rage, Mana...).
    pub class: String,
}

impl Album {
    /// The bonuses earned, as values (bag).
    pub(in crate::ui) fn bag_values(&self) -> std::collections::HashMap<String, f32> {
        self.bag.iter().map(|(k, v)| (k.clone(), *v as f32 / 1e6)).collect()
    }
}

/// A sticker's owned entry: the server keys them by resource id (the sticker's
/// item template, customization.py `StickerbookEntry(2, 0, resource_id)`), not
/// by the sticker's id; either is accepted. Before, an unlocked mount stayed
/// "not owned" and its second click did nothing.
/// A pet owned at a higher tier is keyed by that tier's item (the highest wins).
fn owned_entry<'a>(book: &'a std::collections::HashMap<String, StickerbookEntry>, id: &str, def: &crate::gameplay::systems::StickerDef) -> Option<&'a StickerbookEntry> {
    book.get(id).or_else(|| templates(def).filter_map(|t| book.get(t)).last())
}

/// The item templates that fill a sticker: its own, then its rarity tiers'
/// (`upgrades`: "template:cost", _Template_StickerbookItem.RarityUpgradeItems).
/// Any of them is added to the album (DSOR_Server gameplay/stickerbook_state.py
/// load_sticker_definition accepts a tier's item and keys it by that item).
/// FAILURE (2026-10-23): an Epic Mortis Doll in the bag could not be added.
pub(super) fn templates(def: &crate::gameplay::systems::StickerDef) -> impl Iterator<Item = &str> {
    def.items.iter().map(String::as_str).chain(def.upgrades.iter().map(|u| u.rsplit_once(':').map_or(u.as_str(), |(t, _)| t)))
}

/// The tier above the one owned (or above the sticker's own item): its item and
/// price ("pet_karabossa_magic:5").
fn next(def: &crate::gameplay::systems::StickerDef, tier: Option<&str>) -> Option<(String, u32)> {
    let at = tier.and_then(|t| templates(def).position(|x| x == t)).unwrap_or(0);
    let above = at.saturating_sub(def.items.len().saturating_sub(1));
    def.upgrades.get(above).and_then(|u| u.rsplit_once(':')).map(|(t, cost)| (t.to_owned(), cost.parse().unwrap_or(0)))
}

/// A book reward's hover card, and the amount on its icon.
/// EVIDENCE: the original's Tamed Foes bar (the user's recording
///   20261031-1601-00 and screenshot): a bonus centred, "Triad of Life
///   (Permanent)" / its bonus green / "Reward for 3 x Pets" yellow; an item left
///   aligned under its centred name, "Gilded Clover" / "Consumed when used" /
///   its description yellow ("Right click to add the currency to your Currency
///   Bag.") / "Reward for 12 x Pets", its icon marked "100".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct RewardView {
    /// The reward: a status effect (a bonus) or an item template.
    pub item: String,
    pub title: String,
    /// A bonus's lines (green), or None for an item.
    pub bonus: Option<Vec<String>>,
    /// An item's description lines.
    pub description: Vec<String>,
    pub footer: String,
    pub amount: u32,
    /// An item's lifetime, seconds ("Duration: 7d 0h 0m 0s", red).
    pub lifetime: u32,
    /// Its description is the currencies' hint, written in the hint's colour
    /// (db.itemcurrency rightClickForCurrencyBag: "<cE0C884FF>Right click to add
    /// the currency to your Currency Bag.</>"; the export drops the markup).
    pub hint: bool,
}

pub(super) fn album(sources: &Sources) -> Album {
    let catalog = sources.catalog;
    let state = sources.state;
    let net = sources.net;
    let book_of = |id: &str| catalog.stickerbooks.iter().find(|b| b.id == id);
    let sticker = |id: &String, def: &crate::gameplay::systems::StickerDef| {
        let entry = owned_entry(&net.stickerbook, id, def);
        let owned = entry.is_some_and(|e| e.state >= StickerbookEntry::OWNED);
        // The tier owned shows (its rarity's background), else the sticker's own item.
        let tier = entry.map(|e| e.id.as_str()).filter(|k| templates(def).any(|t| t == *k));
        Sticker {
            id: id.clone(), key: entry.map(|e| e.id.clone()).unwrap_or_default(), book: def.book.clone(), name: def.name.clone(),
            icon: tier.or(def.items.first().map(String::as_str)).map(|t| sources.icon(t, None)).unwrap_or_default(),
            template: tier.or(def.items.first().map(String::as_str)).unwrap_or_default().to_owned(),
            owned, favourite: entry.is_some_and(|e| e.state == StickerbookEntry::FAVOURITE),
            addable: !owned && templates(def).any(|t| !sources.carried(t).is_empty()),
            // The original draws the arrow on every sticker with a tier above
            // the one shown, owned or not (the user's recording 20261031-1414-12:
            //   arrows over a whole Tamed Foes page owned 0 of 33).
            upgradable: next(def, tier).is_some(),
            next: next(def, tier),
            affordable: owned && next(def, tier).is_some_and(|(_, cost)|
                net.game_currencies.get(super::UPGRADE_CURRENCY).copied().unwrap_or(0) >= u64::from(cost)),
            origin: def.origin.clone(), background: def.background.clone(), offer: def.offer.clone(),
            // Owned or not: the original's grey, not owned pets count down too
            // (recording 20261031-1722-23, Avians).
            countdown: tier.or(def.items.first().map(String::as_str))
                .and_then(|t| super::super::hotbar::ItemCooldowns::key_of(sources.items, t)),
        }
    };
    // The list: each category's top book; the open one's books under it.
    let chosen = state.album_book.clone().filter(|b| book_of(b).is_some()).unwrap_or_else(|| FRONT_BOOK.to_owned());
    let category = book_of(&chosen).map_or(0, |b| b.category);
    let mut tops: Vec<_> = catalog.stickerbooks.iter().filter(|b| b.subcategory < 0).collect();
    tops.sort_by_key(|b| (b.category, b.priority));
    let mut books = Vec::new();
    for top in &tops {
        // The front page's row is its book's own title ("Frontpage"); its page
        // reads "Favorites" (FAVOURITES_TEXT).
        // FAILURE (2026-10-30): the row read "Favorites" (the user's recording
        //   beside the original's "Frontpage").
        let title = top.title.clone();
        let open = state.album_open.as_deref() == Some(top.id.as_str());
        books.push(Book { id: top.id.clone(), title, sub: false, selected: open || top.id == chosen });
        if !open || top.id == FRONT_BOOK { continue; }
        let mut subs: Vec<_> = catalog.stickerbooks.iter().filter(|b| b.category == top.category && b.subcategory >= 0).collect();
        subs.sort_by_key(|b| (b.priority, b.subcategory));
        books.extend(subs.iter().map(|b| Book { id: b.id.clone(), title: b.title.clone(), sub: true, selected: b.id == chosen }));
    }
    // The stickers in the books' order.
    let order = |book: &str| catalog.stickerbooks.iter().position(|b| b.id == book).unwrap_or(usize::MAX);
    let mut all: Vec<(&String, &crate::gameplay::systems::StickerDef)> = catalog.stickers.iter().collect();
    // Within a book, by priority, then as the database lists them (`order`).
    // FAILURE (2026-10-31): sorted by id, the grid's places differed from the
    //   original's ("the placement is also wrong").
    all.sort_by(|(a, x), (b, y)| order(&x.book).cmp(&order(&y.book)).then(x.priority.cmp(&y.priority)).then(x.order.cmp(&y.order)).then(a.cmp(b)));
    let front = chosen == FRONT_BOOK;
    let top = book_of(&chosen).is_some_and(|b| b.subcategory < 0);
    let stickers: Vec<Sticker> = all.iter().filter(|(id, def)| {
        if front { owned_entry(&net.stickerbook, id, def).is_some_and(|e| e.state == StickerbookEntry::FAVOURITE) }
        else if top { book_of(&def.book).is_some_and(|b| b.category == category) }
        else { def.book == chosen }
    }).map(|(id, def)| sticker(id, def)).collect();
    let per_page = if front { FRONT_COLUMNS * FRONT_ROWS } else { PER_PAGE };
    let pages = stickers.len().div_ceil(per_page).max(1);
    let page = state.pages.get(ALBUM).copied().unwrap_or(0).min(pages - 1);
    let preview = state.album_sticker.as_ref().and_then(|id| stickers.iter().find(|s| &s.id == id)).cloned();
    let book = book_of(&chosen);
    let progress = (stickers.iter().filter(|s| s.owned).count() as u32, stickers.len() as u32);
    let rewards = if top || front { Vec::new() } else {
        // A reward that is a Collector's Bag bonus (a status effect) shows its
        // own icon (items::Catalog reward_icons).
        // On the bonus's own icon configuration, statuseffect_buff: the green
        // glow (icons_toolkit/bg_status_buff) and the status frame.
        // FAILURE (2026-10-31): the paw alone ("it has those lines around the icon").
        let buff = sources.items.and_then(|i| i.icon_configs.get("statuseffect_buff"));
        book.map(|b| b.rewards.iter().map(|r| (r.at, sources.items.and_then(|i| i.reward_icons.get(&r.item))
            .map_or_else(|| sources.icon(&r.item, None), |image| Icon { image: image.clone(),
                background: buff.map(|c| c.background.clone()).unwrap_or_default(), frame: buff.map(|c| c.frame.clone()).unwrap_or_default(), ..Default::default() }))).collect()).unwrap_or_default()
    };
    // Their cards: a bonus's title and text, else the item's name; under them
    // "Reward for <n> x <category>" (gui.window.stickerbook rewardAmountTemplate).
    // EVIDENCE: the original's Tamed Foes bar (the user's recording
    //   20261031-1601-00): "Triad of Life (Permanent)" / "+ 3% health points
    //   while accompanied by a pet" / "Reward for 15 x Pets".
    let kind = book.and_then(|b| catalog.stickerbooks.iter().find(|t| t.category == b.category && t.subcategory < 0)).map(|t| t.title.clone()).unwrap_or_default();
    let reward_cards = if top || front { Vec::new() } else {
        book.map(|b| b.rewards.iter().map(|r| {
            let card = sources.items.and_then(|i| i.reward_cards.get(&r.item));
            let title = card.map(|c| c.title.clone()).or_else(|| sources.items.map(|i| i.name(&r.item, None).trim().to_owned())).unwrap_or_else(|| r.item.clone());
            // The data writes its line breaks as "\n" (two characters).
            let description = sources.items.and_then(|i| i.items.get(&r.item)).map(|d| d.description.replace("\\n", "\n"))
                .map(|d| d.split('\n').map(str::to_owned).collect()).unwrap_or_default();
            let def = sources.items.and_then(|i| i.items.get(&r.item));
            let lifetime = def.map_or(0, |d| d.lifetime);
            let hint = def.is_some_and(|d| matches!(d.item_category.as_str(), "ItemCurrency" | "NoAutoUseCurrency"));
            RewardView { item: r.item.clone(), title, bonus: card.map(|c| c.lines.clone()), description, footer: format!("Reward for {} x {kind}", r.at), amount: r.amount, lifetime, hint }
        }).collect()).unwrap_or_default()
    };
    let mut earned: std::collections::HashMap<&str, f32> = std::collections::HashMap::new();
    if let Some(items) = sources.items {
        for book in catalog.stickerbooks.iter().filter(|b| !b.rewards.is_empty()) {
            let owned = catalog.stickers.iter().filter(|(id, def)| def.book == book.id
                && owned_entry(&net.stickerbook, id, def).is_some_and(|e| e.state >= StickerbookEntry::OWNED)).count() as u32;
            for (variable, amount) in book.rewards.iter().filter(|r| r.at <= owned).filter_map(|r| items.raises.get(&r.item)) {
                *earned.entry(variable.as_str()).or_insert(0.0) += amount;
            }
        }
        for (variable, value) in earned.iter_mut() {
            if let Some(limit) = items.variables.get(*variable) { *value = value.min(*limit); }
        }
    }
    let bag = earned.into_iter().map(|(k, v)| (k.to_owned(), (v * 1e6).round() as u32)).collect();
    let banners = catalog.album_banners.len();
    let banner_index = if banners == 0 { 0 } else { state.album_banner % banners };
    let new = all.iter().filter(|(_, def)| def.new).take(MOST_NEW).map(|(id, def)| sticker(id, def)).collect();
    Album {
        books, front,
        title: if front { "Favorites".into() } else { book.map(|b| b.title.clone()).unwrap_or_default() },
        description: if top { format!("{} of {} collected.", progress.0, progress.1) } else { book.map(|b| b.description.clone()).unwrap_or_default() },
        stickers, page, pages, preview, progress, rewards, reward_cards,
        banner: catalog.album_banners.get(banner_index).cloned().unwrap_or_default(), banner_index, banners, new,
        scroll: state.scroll.get(LIST).copied().unwrap_or(0),
        bag,
        class: sources.class_name().to_owned(),
    }
}

/// How far the category list scrolls: its rows less those that fit.
pub(super) fn list_limit(album: &Album) -> usize {
    let mut room = LIST_BOTTOM - LIST_TOP;
    let mut fitting = 0;
    for book in album.books.iter().rev() {
        room -= if book.sub { 24.0 } else { 31.0 };
        if room < 0.0 { break; }
        fitting += 1;
    }
    album.books.len() - fitting
}

/// The sticker cells of a grid: a click previews, the flags favour, add, upgrade;
/// up to `cells`, the free places show the empty cell's backing.
fn grid(source: &Layout, layout: &mut Layout, page: &str, stickers: &[Sticker], origin: (f32, f32), columns: usize, cells: usize, preview: Option<&str>) {
    let offset = |i: usize| (origin.0 - CELL_ORIGIN.0 + (i % columns) as f32 * CELL.0, origin.1 - CELL_ORIGIN.1 + (i / columns) as f32 * CELL.1);
    for (i, sticker) in stickers.iter().enumerate() {
        let (dx, dy) = offset(i);
        let replacement = format!("{page}/Sticker{i}");
        let shown = preview == Some(sticker.id.as_str());
        // The flags (favourite star, add, upgrade) come before the icon in the
        // layout; drawn after it, over its corner.
        // FAILURE (2026-10-23): the icon covered the favourite star ("you can see
        //   the star underneath").
        let mut flags = Vec::new();
        for mut w in clone_at(source, ENTRY, &replacement, dx, dy) {
            // The empty cell's backing (after the icon in the layout) is for
            // places without a sticker.
            // FAILURE (2026-10-23): drawn over every icon, owned stickers looked
            //   locked (the user's favourites beside the original's).
            if has_part(&w.id, "EmptyBackground") { continue; }
            rename(&mut w, "TemplateSlot", "Slot");
            rename(&mut w, "TemplateIcon", "Icon");
            if (has_part(&w.id, "FlagLabelPremium") && sticker.origin != PREMIUM_ORIGIN) || (has_part(&w.id, "highlight") && !shown)
                || (has_part(&w.id, "FlagButtonFav") && !sticker.owned) || (has_part(&w.id, "FlagButtonAdd") && !sticker.addable)
                || (has_part(&w.id, "UpgradeButton") && !sticker.upgradable) { continue; }
            if has_part(&w.id, "highlight") { w.hidden = false; }
            // An arrow that cannot be used yet (not owned, or too few Altered
            // Crescerite) wears its grey face, still hovered for its card: its
            // disabled face drawn as its only one.
            // EVIDENCE: the user's recordings 20261031-1414-12 (grey arrows on a
            //   page owned 0 of 33) and 20261031-1506-03 (green ones on owned pets).
            if has_part(&w.id, "UpgradeButton") && w.button.is_some() && !sticker.affordable {
                if w.state.as_deref() != Some("disabled") { continue; }
                w.state = Some("normal".into());
            }
            if has_part(&w.id, "FlagButtonFav") && sticker.favourite { if let Some(art) = &mut w.art { art.tex = Some(FAVOURITE.into()); } }
            let name = leaf(&w.id).to_owned();
            match name.as_str() {
                "FlagButtonFav" => w.event = format!("Sys:Favorite:{}:{}", sticker.server_key(), u8::from(!sticker.favourite)),
                "FlagButtonAdd" => w.event = format!("Sys:Add:{}", sticker.id),
                "UpgradeButton" => w.event = format!("Sys:Upgrade:{}", sticker.id),
                _ => {}
            }
            if w.id == replacement { make_button(&mut w, &format!("Sys:Pick:{}", sticker.id)); }
            if name == "Icon" {
                let mut layers = icon_layers(&mut w, &sticker.icon);
                // Not owned: greyscale at full strength, as the original's (was
                // faded to 35 %, which read as a dark, empty slot).
                // Counting down after a use: drawn as it is, only its countdown over it.
                // FAILURE (2026-10-30): greyed, then dimmed; both read as grey
                //   next to the original's (the user's recordings).
                if !sticker.owned { for a in layers.iter_mut().filter_map(|l| l.art.as_mut()) { a.saturation = 0.0; } }
                set_text(&mut w, "");
                layout.widgets.extend(layers);
            }
            if w.id.contains("/Slot/FlagLabel") { flags.push(w); } else { layout.widgets.push(w); }
        }
        layout.widgets.extend(flags);
        // The countdown and the pop are drawn every frame over the cell
        // (crate::ui::album_overlays): markers with its place.
        // FAILURE (2026-10-31): the window rebuilt for each step of the sweep,
        //   a few times a second ("it still feels ticky, snappy").
        if let Some(icon) = clone_at(source, ENTRY, &replacement, dx, dy).find(|w| leaf(&w.id) == "TemplateIcon") {
            let mark = |id: String| { let mut m = icon.clone(); m.id = id; m.kind = "Label".into(); m.text = None; m.art = None; m.event.clear(); m.button = None; m.state = None; m.hidden = false; m };
            if let Some(key) = &sticker.countdown { layout.widgets.push(mark(format!("{page}{COUNT_MARK}{i}#{key}"))); }
            if sticker.owned { layout.widgets.push(mark(format!("{page}{POP_MARK}{}", sticker.id))); }
        }
    }
    for i in stickers.len()..cells {
        let (dx, dy) = offset(i);
        layout.widgets.extend(clone_at(source, ENTRY, &format!("{page}/Sticker{i}"), dx, dy).filter(|w| has_part(&w.id, "EmptyBackground")));
    }
}

/// The album: the category list, then the front page or the book's page.
pub(super) fn prepare_album(source: &Layout, album: &Album) -> Layout {
    let mut layout = without(source, |id| {
        is_template(id) || id.contains("/AnimUnlock") || id.contains("/AnimUse") || id.contains("/AnimIconGlow")
            || has_part(id, "FlagLabelCategory") || has_part(id, "FlagLabelSubcategory")
            || (album.front && id.starts_with(PAGE)) || (!album.front && id.starts_with(FRONT))
            // Nothing to turn without a sticker.
            || (album.preview.is_none() && id.starts_with(&format!("{PREVIEW}/previewBackground/background_image/DragRotateAreaLabel")))
            || id.ends_with("/OriginIconLabel1") || id.contains("/ProgressLabel/ItemTemplates")
            || (album.preview.as_ref().is_none_or(|s| !s.owned) && id.contains("/background_image/FlagButtonFav"))
            || (album.preview.as_ref().is_none_or(|s| s.origin.is_empty()) && id.ends_with("/OriginIconLabel"))
            || (album.preview.as_ref().is_none_or(|s| s.owned || s.offer.is_empty()) && id.contains("/background_preview/PaymentButton"))
            || (album.pages <= 1 && id.contains("/Categorypage/pages/") && !id.ends_with("/PageNumText"))
            || id.split('/').find_map(|p| p.strip_prefix("Counter")).and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| id.contains("/DealsCounterLabel/") && n >= album.banners)
            || id.split('/').find_map(|p| p.strip_prefix("ItemSlot")).and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| id.contains("/Firstpage/AdvertisementArtwork/AdvertisedItems/") && n >= album.new.len())
    });
    let preview = album.preview.as_ref();
    // The sticker turning, drawn over the preview's background (the
    // TURNING widget, controls::Section shows it or the icon below).
    let image = format!("{PREVIEW}/previewBackground/background_image");
    if let (Some(_), Some(at)) = (preview, layout.widgets.iter().position(|w| w.id == image)) {
        let stage = crate::ui::preview::Stage::Album;
        let mut w = layout.widgets[at].clone();
        w.id = TURNING.to_owned();
        w.kind = "Label".into();
        (w.event, w.button, w.state, w.text, w.hidden, w.rect) = (String::new(), None, None, None, false, stage.area());
        w.art = Some(crate::ui::preview::quad_art(w.rect, stage));
        layout.widgets.insert(at + 1, w);
    }
    let mut extra = Vec::new();
    for w in &mut layout.widgets {
        let id = w.id.clone();
        match leaf(&id) {
            "TextLabel_fav" => set_text(w, &album.title),
            "TextLabel_info" if !album.front => set_text(w, &album.description),
            "TextLabel_info" => set_text(w, FAVOURITES_TEXT),
            "PageNumText" => set_text(w, &format!("{} / {}", album.page + 1, album.pages)),
            // The bar's caption (its points label is authored transparent).
            "ProgressBarTextLabel" => set_text(w, &format!("{} / {}", album.progress.0, album.progress.1)),
            "progresspoints" => set_text(w, ""),
            "progress" if id.starts_with(BAR) => clip_x(w, album.progress.0 as f32 / album.progress.1.max(1) as f32),
            "AdvertisementArtwork" if !album.banner.is_empty() => { if let Some(art) = &mut w.art { art.tex = Some(texture_path(&album.banner)); } }
            "TextLabel" if id.ends_with("/background_headline/TextLabel") => set_text(w, &preview.map_or_else(|| album.title.clone(), |s| s.name.clone())),
            "OriginIconLabel" => if let Some(s) = preview { w.art = Some(image_art(w.rect, texture_path(&s.origin))); },
            "background_image" | "background_t" => if let Some(s) = preview.filter(|s| !s.background.is_empty()) { w.art = Some(image_art(w.rect, texture_path(&s.background))); },
            "FlagButtonFav" if id.contains("/background_image/") => if let Some(s) = preview { w.event = format!("Sys:Favorite:{}:{}", s.server_key(), u8::from(!s.favourite)); },
            "PaymentButton" => if let Some(s) = preview { w.event = format!("Sys:Buy:{}", s.offer); },
            _ => {}
        }
        if id.contains("/background_image/FlagButtonFav/") && preview.is_some_and(|s| s.favourite) {
            if let Some(art) = &mut w.art { art.tex = Some(FAVOURITE.into()); }
        }
        // The banners' counters: the one shown pressed.
        if let Some(n) = id.split('/').find_map(|p| p.strip_prefix("Counter")).and_then(|n| n.parse::<usize>().ok()) {
            if id.contains("/DealsCounterLabel/") {
                if leaf(&id) == format!("Counter{n}") { w.event = format!("Sys:Banner:{n}"); }
                if n == album.banner_index { selected(w); }
            }
        }
        // The new stickers: their icons; a click opens their book.
        if let Some(n) = id.split('/').find_map(|p| p.strip_prefix("ItemSlot")).and_then(|n| n.parse::<usize>().ok()) {
            if id.contains("/Firstpage/AdvertisementArtwork/AdvertisedItems/") {
                if let Some(s) = album.new.get(n) {
                    if leaf(&id) == "Icon0" { extra.extend(icon_layers(w, &s.icon)); set_text(w, ""); }
                    if leaf(&id) == format!("Button{n}") { make_button(w, &format!("Sys:AlbumBook:{}:{}", s.book, s.id)); }
                }
            }
        }
    }
    layout.widgets.extend(extra);
    // The preview's sticker.
    if let Some(s) = preview.filter(|s| !s.icon.image.is_empty()) {
        if let Some(area) = source.widgets.iter().find(|w| w.id.starts_with(PREVIEW) && w.id.ends_with("/DragRotateAreaLabel")) {
            let mut w = area.clone();
            w.id = format!("{PREVIEW}/Sticker");
            w.kind = "Label".into();
            w.rect = PREVIEW_ICON;
            let mut layers = icon_layers(&mut w, &Icon { image: s.icon.image.clone(), ..Default::default() });
            if !s.owned { for l in &mut layers { dim(l, 0.6); } }
            layout.widgets.extend(layers);
        }
    }
    // The category list.
    let mut y = LIST_TOP;
    for (n, book) in album.books.iter().enumerate().skip(album.scroll) {
        let template = format!("{LIST}/{}", if book.sub { "TemplateSubCategory" } else { "TemplateCategory" });
        let Some(root) = source.widgets.iter().find(|w| w.id == template) else { continue };
        let height = root.rect[3] - root.rect[1];
        if y + height > LIST_BOTTOM + 0.5 { break; }
        let replacement = format!("{LIST}/Book{n}");
        for mut w in clone_rows(source, &template, &replacement, y - root.rect[1]) {
            if w.id == replacement { w.event = format!("Sys:AlbumBook:{}", book.id); }
            if w.id.contains("/Animation") { continue; }
            if w.text.is_some() { set_text(&mut w, &book.title); }
            if book.selected { selected(&mut w); }
            layout.widgets.push(w);
        }
        y += height;
    }
    // The grid.
    let shown: Vec<Sticker> = album.stickers.iter().skip(album.page * if album.front { FRONT_COLUMNS * FRONT_ROWS } else { PER_PAGE })
        .take(if album.front { FRONT_COLUMNS * FRONT_ROWS } else { PER_PAGE }).cloned().collect();
    if album.front {
        grid(source, &mut layout, FRONT, &shown, FRONT_ORIGIN, FRONT_COLUMNS, FRONT_COLUMNS * FRONT_ROWS, None);
    } else {
        grid(source, &mut layout, PAGE, &shown, CELL_ORIGIN, COLUMNS, shown.len(), preview.map(|s| s.id.as_str()));
        // The book's rewards along its bar, at their counts.
        if let (Some(bar), Some(root)) = (source.widgets.iter().find(|w| w.id == BAR), source.widgets.iter().find(|w| w.id == REWARD)) {
            let total = album.progress.1.max(1) as f32;
            for (n, (at, icon)) in album.rewards.iter().enumerate() {
                let x = bar.rect[0] + (bar.rect[2] - bar.rect[0]) * (*at as f32 / total).min(1.0);
                for mut w in clone_at(source, REWARD, &format!("{INLAY}/Categorypage/ProgressLabel/Reward{n}"), x - (root.rect[0] + root.rect[2]) / 2.0, 0.0) {
                    if w.id.ends_with("/IconHighlight") && *at > album.progress.0 { continue; }
                    // An item's amount on its icon ("100" Gilded Clovers).
                    let amount = album.reward_cards.get(n).map_or(1, |r| r.amount);
                    // In the icon's middle, larger than a slot's count (the user, 2026-10-31:
                    // the original's fills the icon, ours sat small in its corner;
                    // "maybe in the middle and a bit bigger").
                    if leaf(&w.id) == "Icon" {
                        layout.widgets.extend(icon_layers(&mut w, icon));
                        set_text(&mut w, &if amount > 1 { amount.to_string() } else { String::new() });
                        if let Some(label) = &mut w.text { label.align = [1, 1]; label.size *= 1.4; }
                    }
                    // Hovered, it shows its card (tooltips): its slot (the root is an
                    // 11 x 10 label at the stem's top).
                    if w.id.ends_with(&format!("/Reward{n}/Linie/ItemSlot")) { make_button(&mut w, &format!("Sys:Reward:{n}")); }
                    w.hidden = false;
                    layout.widgets.push(w);
                }
            }
        }
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::template::fixture::{find, layout, text_of};

    fn album_source() -> Layout {
        let front_items = format!("{FRONT}/AdvertisementArtwork/AdvertisedItems");
        let counters = format!("{FRONT}/AdvertisementArtwork/DealsCounterLabel");
        let image = format!("{PREVIEW}/previewBackground/background_image");
        let ids: Vec<(String, &str, bool)> = vec![(format!("{LIST}/TemplateCategory"), "Button", false), (format!("{LIST}/TemplateCategory/normal"), "Label", false), (format!("{LIST}/TemplateCategory/normal/TextLabel"), "TextLabel", true),
            (format!("{LIST}/TemplateSubCategory"), "Button", false), (format!("{LIST}/TemplateSubCategory/normal"), "Label", false), (format!("{LIST}/TemplateSubCategory/normal/TextLabel"), "TextLabel", true),
            (ENTRY.into(), "Label", false), (format!("{ENTRY}/TemplateSlot/FlagLabel/FlagButtonFav"), "Button", false), (format!("{ENTRY}/TemplateSlot/FlagLabel/FlagButtonAdd"), "Button", false),
            (format!("{ENTRY}/TemplateSlot/FlagLabel/UpgradeButton"), "Button", false), (format!("{ENTRY}/TemplateIcon"), "Icon", true), (format!("{ENTRY}/TemplateIcon/highlight"), "Label", false),
            (format!("{ENTRY}/EmptyBackground"), "Slot", false),
            (ANIM_USE_TEXT.into(), "TextLabel", true),
            (format!("{PAGE}/background_info/TextLabel_fav"), "TextLabel", true), (format!("{PAGE}/background_info/TextLabel_info"), "TextLabel", true),
            (format!("{PAGE}/pages/PageNumText"), "TextLabel", true), (format!("{PAGE}/pages/NextPageBtn"), "Button", false),
            (image.clone(), "Label", false), (format!("{image}/background_headline/TextLabel"), "TextLabel", true), (format!("{image}/FlagButtonFav"), "Button", false), (format!("{image}/OriginIconLabel"), "Label", false),
            (format!("{image}/DragRotateAreaLabel"), "Label", false), (format!("{PREVIEW}/PaymentButton"), "Button", false),
            (BAR.into(), "Label", false), (format!("{BAR}/progress"), "Label", false), (format!("{BAR}/ProgressBarTextLabel"), "TextLabel", true),
            (REWARD.into(), "Label", false), (format!("{REWARD}/Linie/ItemSlot/Icon"), "Icon", true),
            (format!("{FRONT}/background_info/TextLabel_fav"), "TextLabel", true), (format!("{FRONT}/AdvertisementArtwork"), "Label", false),
            (format!("{counters}/Counter0"), "Button", false), (format!("{counters}/Counter1"), "Button", false), (format!("{counters}/Counter1/normal"), "Label", false), (format!("{counters}/Counter2"), "Button", false),
            (format!("{front_items}/ItemSlot0/Icon0"), "Icon", true), (format!("{front_items}/ItemSlot0/Button0"), "Button", false), (format!("{front_items}/ItemSlot1/Icon0"), "Icon", true)];
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        layout(ALBUM, &refs)
    }

    #[test]
    fn the_album_lists_its_books_and_shows_a_book_s_page_with_its_preview() {
        let owned = Sticker { id: "s1".into(), book: "sb_jobs".into(), name: "Horse".into(), icon: Icon { image: "h.png".into(), ..Default::default() }, owned: true, favourite: true, upgradable: true, origin: "ui/icon_origin_event".into(), ..Default::default() };
        let missing = Sticker { id: "s2".into(), book: "sb_jobs".into(), name: "Cloak".into(), icon: Icon { image: "c.png".into(), ..Default::default() }, addable: true, offer: "global_x_RC_01".into(), ..Default::default() };
        let album = Album {
            books: vec![Book { id: "sb_frontpage".into(), title: "Favorites".into(), ..Default::default() }, Book { id: "sb_costume".into(), title: "Costumes".into(), ..Default::default() },
                Book { id: "sb_jobs".into(), title: "Jobs".into(), sub: true, selected: true }],
            title: "Jobs".into(), description: "Costumes of trades.".into(), stickers: vec![owned.clone(), missing.clone()], page: 0, pages: 1,
            preview: Some(owned.clone()), progress: (1, 2), rewards: vec![(1, Icon { image: "r.png".into(), ..Default::default() })], ..Default::default()
        };
        let prepared = prepare_album(&album_source(), &album);
        let ids: Vec<_> = prepared.widgets.iter().map(|w| w.id.clone()).collect();
        assert!(!ids.iter().any(|id| is_template(id) || id.starts_with(FRONT)));
        assert_eq!((find(&prepared, &format!("{LIST}/Book1")).event.as_str(), text_of(&prepared, &format!("{LIST}/Book1/normal/TextLabel"))), ("Sys:AlbumBook:sb_costume", "Costumes".into()));
        // The book open: its row pressed, a sub-category's height under the category's.
        assert!(find(&prepared, &format!("{LIST}/Book2/normal")).hidden);
        assert_eq!(text_of(&prepared, &format!("{PAGE}/background_info/TextLabel_fav")), "Jobs");
        let first = find(&prepared, &format!("{PAGE}/Sticker0"));
        assert_eq!((first.kind.as_str(), first.event.as_str(), first.rect[0]), ("Button", "Sys:Pick:s1", 10.0));
        assert_eq!(find(&prepared, &format!("{PAGE}/Sticker1")).rect[0], 10.0 + CELL.0);
        assert!(ids.contains(&format!("{PAGE}/Sticker0/Icon/highlight")) && !ids.contains(&format!("{PAGE}/Sticker1/Icon/highlight")));
        assert_eq!(find(&prepared, &format!("{PAGE}/Sticker0/Slot/FlagLabel/FlagButtonFav")).art.as_ref().unwrap().tex.as_deref(), Some(FAVOURITE));
        assert_eq!(find(&prepared, &format!("{PAGE}/Sticker1/Slot/FlagLabel/FlagButtonAdd")).event, "Sys:Add:s2");
        let unowned = find(&prepared, &format!("{PAGE}/Sticker1/Icon/Image")).art.as_ref().unwrap();
        assert_eq!((unowned.saturation, unowned.color[3]), (0.0, 1.0), "not owned: grey, not faded");
        // Its category cooling down: drawn as it is, a sweep and the seconds
        // centred over it.
        let counting = prepare_album(&album_source(), &Album { stickers: vec![Sticker { countdown: Some("Pet".into()), ..owned.clone() }], ..album.clone() });
        let used = find(&counting, &format!("{PAGE}/Sticker0/Icon/Image")).art.as_ref().unwrap();
        let plain = find(&prepared, &format!("{PAGE}/Sticker0/Icon/Image")).art.as_ref().unwrap();
        assert_eq!((used.saturation, &used.color), (plain.saturation, &plain.color));
        // A marker over the cell; the sweep and seconds are drawn every frame.
        let mark = find(&counting, &format!("{PAGE}{COUNT_MARK}0#Pet"));
        assert!(mark.art.is_none() && mark.text.is_none() && !mark.hidden);
        assert!(!ids.iter().any(|id| id.contains("/Use0")));
        // No empty backing over a sticker, none past a book's stickers.
        assert!(!ids.iter().any(|id| id.ends_with("/EmptyBackground")));
        // The preview: the sticker's name, origin, icon, favourite flag; no offer for an owned one.
        let image = format!("{PREVIEW}/previewBackground/background_image");
        assert_eq!(text_of(&prepared, &format!("{image}/background_headline/TextLabel")), "Horse");
        assert_eq!(find(&prepared, &format!("{image}/OriginIconLabel")).art.as_ref().unwrap().tex.as_deref(), Some("textures/ui/icon_origin_event.png"));
        assert_eq!(find(&prepared, &format!("{image}/FlagButtonFav")).event, "Sys:Favorite:s1:0");
        assert_eq!(find(&prepared, &format!("{PREVIEW}/Sticker/Image")).rect, PREVIEW_ICON);
        assert!(!ids.iter().any(|id| id.contains("/PaymentButton") || id.contains("/pages/NextPageBtn")));
        // ...and the sticker turning over the preview's background, which
        // its DragRotate buttons turn (crate::ui::preview; the page shows one).
        let turning = find(&prepared, TURNING);
        assert_eq!((turning.art.as_ref().unwrap().tex.as_deref(), turning.rect), (Some("preview:album"), crate::ui::preview::Stage::Album.area()));
        assert_eq!(ids.iter().position(|id| id == TURNING), ids.iter().position(|id| *id == image).map(|i| i + 1));
        assert!(ids.contains(&format!("{image}/DragRotateAreaLabel")));
        // The progress: collected of the book's; its reward along the bar.
        assert_eq!(text_of(&prepared, &format!("{BAR}/ProgressBarTextLabel")), "1 / 2");
        assert!(ids.contains(&format!("{INLAY}/Categorypage/ProgressLabel/Reward0/Linie/ItemSlot/Icon/Image")));
        // A missing sticker with an offer: the preview sells it.
        let selling = prepare_album(&album_source(), &Album { preview: Some(missing.clone()), ..album.clone() });
        assert_eq!(find(&selling, &format!("{PREVIEW}/PaymentButton")).event, "Sys:Buy:global_x_RC_01");
        assert!(!selling.widgets.iter().any(|w| w.id.contains("/background_image/FlagButtonFav")));
        // No sticker picked: nothing to turn.
        let none = prepare_album(&album_source(), &Album { preview: None, ..album.clone() });
        assert!(!none.widgets.iter().any(|w| w.id == TURNING || w.id.contains("DragRotateAreaLabel")));
    }

    #[test]
    fn the_album_front_page_shows_banners_new_stickers_and_favourites() {
        let favourite = Sticker { id: "s1".into(), book: "sb_jobs".into(), name: "Horse".into(), icon: Icon { image: "h.png".into(), ..Default::default() }, owned: true, favourite: true, ..Default::default() };
        let new = Sticker { id: "s2".into(), book: "sb_pets".into(), icon: Icon { image: "p.png".into(), ..Default::default() }, ..Default::default() };
        let album = Album { front: true, title: "Favorites".into(), stickers: vec![favourite], pages: 1, banner: "ui_collectorsalbum/stickerbookbanner_pets_en_crunch".into(), banner_index: 1, banners: 2, new: vec![new], ..Default::default() };
        let prepared = prepare_album(&album_source(), &album);
        let ids: Vec<_> = prepared.widgets.iter().map(|w| w.id.clone()).collect();
        assert!(!ids.iter().any(|id| id.starts_with(PAGE)));
        assert_eq!(find(&prepared, &format!("{FRONT}/Sticker0")).rect[1], 192.0 + FRONT_ORIGIN.1 - CELL_ORIGIN.1);
        // The favourite's icon is bare; the other 29 places show the empty backing.
        let empty: Vec<_> = ids.iter().filter(|id| id.ends_with("/EmptyBackground")).collect();
        assert_eq!(empty.len(), FRONT_COLUMNS * FRONT_ROWS - 1);
        assert!(!ids.contains(&format!("{FRONT}/Sticker0/EmptyBackground")) && ids.contains(&format!("{FRONT}/Sticker29/EmptyBackground")));
        assert_eq!(find(&prepared, &format!("{FRONT}/Sticker11/EmptyBackground")).rect[0], find(&prepared, &format!("{FRONT}/Sticker1/EmptyBackground")).rect[0]);
        assert_eq!(find(&prepared, &format!("{FRONT}/AdvertisementArtwork")).art.as_ref().unwrap().tex.as_deref(), Some("textures/ui_collectorsalbum/stickerbookbanner_pets_en_crunch.png"));
        let counters = format!("{FRONT}/AdvertisementArtwork/DealsCounterLabel");
        assert_eq!(find(&prepared, &format!("{counters}/Counter0")).event, "Sys:Banner:0");
        assert!(find(&prepared, &format!("{counters}/Counter1/normal")).hidden && !ids.contains(&format!("{counters}/Counter2")));
        let items = format!("{FRONT}/AdvertisementArtwork/AdvertisedItems");
        assert_eq!(find(&prepared, &format!("{items}/ItemSlot0/Button0")).event, "Sys:AlbumBook:sb_pets:s2");
        assert!(ids.contains(&format!("{items}/ItemSlot0/Icon0/Image")) && !ids.iter().any(|id| id.contains("/ItemSlot1")));
    }

    #[test]
    fn a_sticker_is_owned_under_its_item_s_resource_id() {
        let def: crate::gameplay::systems::StickerDef = serde_json::from_value(serde_json::json!({
            "name": "Manticore", "book": "sb_mounts_wild_animals", "items": ["mythical_mount_manticore_01"] })).unwrap();
        let entry = |id: &str, state| StickerbookEntry { state, unknown_u32_0: 0, id: id.into() };
        let id = "sb_mount_wild_31_mythical_mount_manticore_01";
        // The server's unlock (StickerbookItemInfo): keyed by the resource id.
        let book = std::collections::HashMap::from([("mythical_mount_manticore_01".to_owned(), entry("mythical_mount_manticore_01", 2))]);
        assert_eq!(owned_entry(&book, id, &def).map(|e| e.id.as_str()), Some("mythical_mount_manticore_01"));
        // Keyed by the sticker id still works; nothing owned, nothing found.
        let book = std::collections::HashMap::from([(id.to_owned(), entry(id, 3))]);
        assert_eq!(owned_entry(&book, id, &def).map(|e| e.state), Some(3));
        assert!(owned_entry(&std::collections::HashMap::new(), id, &def).is_none());
        // A favourite request names the key the server owns it under.
        let s = Sticker { id: id.into(), key: "mythical_mount_manticore_01".into(), ..Default::default() };
        assert_eq!(s.server_key(), "mythical_mount_manticore_01");
        assert_eq!(Sticker { id: id.into(), ..Default::default() }.server_key(), id);
    }

    #[test]
    fn a_sticker_s_arrow_offers_the_tier_above_the_one_owned() {
        let def: crate::gameplay::systems::StickerDef = serde_json::from_value(serde_json::json!({
            "name": "Mortis Doll", "book": "sb_pets_tamed_enemies", "items": ["pet_mortis_phase2_uncommon"],
            "upgrades": ["pet_mortis_phase2_magic:5", "pet_mortis_phase2_rare:25", "pet_mortis_phase2_epic:50"] })).unwrap();
        // Not owned, as owned at its own item: the first tier above.
        assert_eq!(next(&def, None), Some(("pet_mortis_phase2_magic".into(), 5)));
        assert_eq!(next(&def, Some("pet_mortis_phase2_uncommon")), Some(("pet_mortis_phase2_magic".into(), 5)));
        assert_eq!(next(&def, Some("pet_mortis_phase2_rare")), Some(("pet_mortis_phase2_epic".into(), 50)));
        // The last tier, or a sticker without tiers: no arrow.
        assert_eq!(next(&def, Some("pet_mortis_phase2_epic")), None);
        let plain: crate::gameplay::systems::StickerDef = serde_json::from_value(serde_json::json!({ "name": "Nian", "items": ["pet_nian"] })).unwrap();
        assert_eq!(next(&plain, None), None);
    }

    #[test]
    fn a_pet_counts_its_rarity_tiers() {
        let def: crate::gameplay::systems::StickerDef = serde_json::from_value(serde_json::json!({
            "name": "Mortis Doll", "book": "sb_pets_tamed_enemies", "items": ["pet_mortis_phase2_uncommon"],
            "upgrades": ["pet_mortis_phase2_magic:5", "pet_mortis_phase2_rare:25", "pet_mortis_phase2_epic:50"] })).unwrap();
        assert_eq!(templates(&def).collect::<Vec<_>>(),
            ["pet_mortis_phase2_uncommon", "pet_mortis_phase2_magic", "pet_mortis_phase2_rare", "pet_mortis_phase2_epic"]);
        // The server keys an Epic added from the bag by the Epic's item; the highest tier owned wins.
        let entry = |id: &str| (id.to_owned(), StickerbookEntry { state: 2, unknown_u32_0: 0, id: id.into() });
        let id = "sb_pet_enemies_20_pet_mortis_phase2";
        let book = std::collections::HashMap::from([entry("pet_mortis_phase2_uncommon"), entry("pet_mortis_phase2_epic")]);
        assert_eq!(owned_entry(&book, id, &def).map(|e| e.id.as_str()), Some("pet_mortis_phase2_epic"));
    }

    #[test]
    fn the_category_list_scrolls_by_what_fits() {
        let books = (0..30).map(|n| Book { id: format!("b{n}"), sub: n > 2, ..Default::default() }).collect();
        // Three categories (31 high) and 27 books (24 high) in 494: the last 20 books fit.
        assert_eq!(list_limit(&Album { books, ..Default::default() }), 10);
    }
}
