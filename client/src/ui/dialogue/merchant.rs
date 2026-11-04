//! NPC shops use the authored trading screen and the current server snapshot.
use super::*;
use controls::currency::{self, Wallet};
use controls::items;

/// _Globals LargeRCExpensesNotificationThreshold (static.db4: 20000): from this
/// Andermant price on, a purchase asks for consent first (controls::consent).
/// EVIDENCE: release261 UI::TradingWidget::BuyItemClick (0x14055C418) compares
///   the offer's Andermant price with TemplateManager +0x1C3C.
pub(super) const LARGE_ANDERMANT: u32 = 20_000;

const ROOT: &str = "Canvas/Label_blur/Label_texture";
const LIST: &str = "Canvas/Label_blur/Label_texture/ListBackground/List";
const ENTRY: &str = "Canvas/Label_blur/Label_texture/ListBackground/List/TemplateListEntry/TemplateItemEntry1";

#[derive(Deserialize, Default)]
pub(super) struct Catalog {
    categories: HashMap<u16, Category>,
    offers: HashMap<String, OfferLook>,
    currencies: HashMap<String, String>,
    /// What a shop's PriceIC is paid in: its _Template_ItemCurrency.CurrencyItem
    /// (tools/trading_catalog.py).
    #[serde(default)]
    currency_items: HashMap<String, String>,
    glyph: String,
}
#[derive(Deserialize, Clone, PartialEq, Eq)]
struct Category { name: String, icon: String }
#[derive(Deserialize)]
struct OfferLook { icon: String, class_name: String }
#[derive(Default)]
pub(super) struct Selection { pub category: Option<u16>, pub buyback: bool, pub usable: bool }
#[derive(Clone, PartialEq, Eq)]
struct Price { amount: u32, icon: String, label: String, texture: String }
impl Price {
    fn width(&self) -> f32 { 22.0 + self.amount.to_string().len() as f32 * 6.0 }
}
#[derive(Clone, PartialEq, Eq)]
pub(super) struct MerchantItem { name: String, icon: String, background: String, frame: String, amount: u32, prices: Vec<Price>, action: Option<Action>,
    /// Whether the wallet pays it (affordable): its item greyed and no order.
    affordable: bool }
#[derive(Clone, PartialEq, Eq)]
pub(super) struct MerchantView {
    pub items: Vec<MerchantItem>,
    tabs: Vec<(u16, Category)>,
    category: Option<u16>,
    pub buyback: bool,
    usable: bool,
    pub can_sell: bool,
    pub page: usize,
    pages: usize,
    row_height: u32,
}
impl MerchantView {
    pub(super) fn choices(&self, choices: &mut Vec<Choice>) {
        choices.push(Choice::new("Buy", "shop", Action::Buyback(false)));
        if self.can_sell { choices.push(Choice::new("Buy back", "shop", Action::Buyback(true))); }
        if !self.buyback {
            for (code, tab) in &self.tabs { choices.push(Choice::new(&tab.name, "shop", Action::Category(*code))); }
            choices.push(Choice::new("Show only usable items", "shop", Action::Usable(!self.usable)));
        }
        if self.page > 0 { choices.push(Choice::new("Previous page", "talk", Action::Previous)); }
        if self.page + 1 < self.pages { choices.push(Choice::new("Next page", "talk", Action::Next)); }
        // Each card is a choice of its own: a right click on its item buys the
        // offer or buys the stack back (prepare).
        for item in &self.items {
            if let Some(action) = &item.action { choices.push(Choice::new(&item.name, "shop", action.clone())); }
        }
    }
}

fn class_matches(allowed: &str, class: Option<u8>) -> bool {
    let Some(class) = class.map(crate::actors::character::item_class) else { return true };
    allowed.is_empty() || allowed.split(';').any(|c| c.trim().eq_ignore_ascii_case(class) || c.trim().eq_ignore_ascii_case("any"))
}

fn suitable(offer: &inventory::ShopOffer, items: Option<&items::Catalog>, catalog: &Catalog, class: Option<u8>, level: Option<u32>) -> bool {
    let definition = items.and_then(|c| c.items.get(&offer.item));
    let offer_class = catalog.offers.get(&offer.id).map_or("", |o| o.class_name.as_str());
    class_matches(offer_class, class) && definition.is_none_or(|d| class_matches(&d.class_name, class))
        && level.is_none_or(|level| {
            level >= offer.min_player_level.max(0) as u32
                && (offer.max_player_level <= 0 || level <= offer.max_player_level as u32)
                && (offer.scale_to_player_level || definition.is_none_or(|d| level >= d.required_level))
        })
}

/// One copy's price in a currency: the release 261 server applies the strongest
/// supplied veteran discount with round-to-nearest-even
/// (protocol/commerce_protocol.py price_for_offer).
fn discounted(offer: &inventory::ShopOffer) -> impl Fn(u32) -> u32 {
    let discount = offer.veteran_discounts.iter().map(|(_, v)| *v as f64).filter(|v| v.is_finite()).fold(0.0, f64::max).clamp(0.0, 1.0);
    move |v: u32| (v as f64 * (1.0 - discount)).round_ties_even() as u32
}

/// One copy's Andermant price.
pub(super) fn andermant(offer: &inventory::ShopOffer) -> u32 { discounted(offer)(offer.price_rc) }

/// Whether the wallet pays one copy, as the server debits it (DSOR_Server
/// commerce_protocol.debit_purchase_currency): RC from the Andermant, VC the
/// gold, GC the Glyphs of Power, EC the Draken, UC the legacy balance, Arena
/// the Badges of Honor, IC the shop's currency (the Honor or the Draken after
/// their own price). A shop the catalog has no currency for is left to the
/// server.
/// EVIDENCE: release261 ClientTradingManager::IsOfferPurchasable (0x14027C184)
///   refuses an offer one of these does not cover, before any order.
pub(super) fn affordable(offer: &inventory::ShopOffer, shop: &str, catalog: &Catalog, wallet: &Wallet) -> bool {
    let price = discounted(offer);
    let [rc, vc, gc, ec, uc, arena, ic] = [offer.price_rc, offer.price_vc, offer.price_gc, offer.price_ec, offer.price_uc,
        offer.price_arena, offer.price_ic].map(|v| price(v) as u64);
    let (mut honor, mut draken) = (arena, ec);
    let items = ic == 0 || match catalog.currency_items.get(shop).map(String::as_str) {
        None => true,
        Some(currency::HONOR) => { honor += ic; true }
        Some(currency::DRAKEN) => { draken += ic; true }
        Some(item) => wallet.keyed.get(item).copied().unwrap_or(0) >= ic,
    };
    items && rc <= wallet.andermant && vc <= wallet.copper && gc <= wallet.glyphs && uc <= wallet.legacy
        && honor <= wallet.honor && draken <= wallet.draken
}

fn prices(offer: &inventory::ShopOffer, shop: &str, catalog: &Catalog) -> Vec<Price> {
    let discounted = discounted(offer);
    let mut out = Vec::new();
    let mut push = |amount, icon: &str, label: &str, texture: &str| {
        out.push(Price { amount, icon: icon.into(), label: label.into(), texture: texture.into() });
    };
    let vc = discounted(offer.price_vc);
    for (amount, name, icon) in [(vc / 10000, "Gold", "gold"), (vc / 100 % 100, "Silver", "silber"), (vc % 100, "Copper", "kupfer")] {
        if amount > 0 { push(amount, &format!("TemplateVC/icon_money_{icon}"), &format!("TemplateVC/{name}"), ""); }
    }
    for (amount, icon, label, texture) in [
        (offer.price_rc, "RealCurrency/icon_andermant", "RealCurrency/andermant_display", ""),
        (offer.price_arena, "ArenaCurrency/icon_ac", "ArenaCurrency/ac_display", ""),
        (offer.price_ec, "EventCurrency/icon_ec", "EventCurrency/ec_display", ""),
        (offer.price_gc, "ArenaCurrency/icon_ac", "ArenaCurrency/ac_display", catalog.glyph.as_str()),
        (offer.price_uc, "", "TemplateVC/Copper", ""),
        (offer.price_ic, "", "RealCurrency/andermant_display", catalog.currencies.get(shop).map_or("", String::as_str)),
    ] {
        if amount > 0 { push(discounted(amount), icon, label, texture); }
    }
    out
}

fn price_rows(prices: &[Price]) -> u32 {
    let (mut rows, mut x) = (1, 0.0);
    for price in prices {
        if x + price.width() > 168.0 { rows += 1; x = 0.0; }
        x += price.width();
    }
    rows
}

/// The trader's selection on screen; `wallet` (None before the server sent it)
/// marks the offers it does not pay.
#[allow(clippy::too_many_arguments)]
pub(super) fn build(offers: Option<&inventory::Offer>, selection: &Selection, page: usize,
    items: Option<&items::Catalog>, catalog: &Catalog, class: Option<u8>, level: Option<u32>, wallet: Option<&Wallet>) -> MerchantView {
    let can_sell = offers.is_some_and(|o| o.can_sell);
    let buyback = selection.buyback && can_sell;
    let mut codes = Vec::new();
    if let Some(offers) = offers {
        // Preserve server order, including empty categories. Some servers omit
        // the category list; in that case derive it from the returned offers.
        for code in offers.categories.iter().copied().chain(offers.offers.iter().flatten().map(|o| o.category)) {
            if !codes.contains(&code) { codes.push(code); }
        }
    }
    let category = selection.category.filter(|c| codes.contains(c)).or_else(|| codes.first().copied());
    let tabs = codes.into_iter().map(|code| (code, catalog.categories.get(&code).cloned()
        .unwrap_or(Category { name: format!("Category {}", code + 1), icon: String::new() }))).collect();
    let mut cards = Vec::new();
    if let Some(offers) = offers {
        if buyback {
            for item in &offers.items {
                let def = items.and_then(|c| c.items.get(&item.template));
                let (icon, background, frame) = items.map(|c| c.icon_parts(&item.template, Some(item))).unwrap_or_default();
                cards.push(MerchantItem { name: def.map(|d| d.name.clone()).unwrap_or_else(|| readable(&item.template)),
                    icon: icon.unwrap_or_default().into(), background: background.unwrap_or_default().into(), frame: frame.unwrap_or_default().into(),
                    amount: item.third, prices: vec![], action: Some(Action::Restore(item.id)), affordable: true });
            }
        } else {
            for offer in offers.offers.iter().flatten().filter(|o| Some(o.category) == category) {
                if (selection.usable || offer.hide_not_suitable) && !suitable(offer, items, catalog, class, level) { continue; }
                let def = items.and_then(|c| c.items.get(&offer.item));
                let rarity_name = def.map_or("common", |d| d.rarity.as_str()).to_lowercase();
                let rarity = if offer.rarity == 0 { ["common", "uncommon", "magic", "rare", "epic", "unique", "set", "mythic"]
                    .iter().position(|r| *r == rarity_name).unwrap_or(0) as u8 } else { offer.rarity };
                let record = inventory::ItemInfo { kind: rarity, ..default() };
                let (icon, background, frame) = items.map(|c| c.icon_parts(&offer.item, Some(&record))).unwrap_or_default();
                let look = catalog.offers.get(&offer.id);
                let icon = if !offer.icon.is_empty() { format!("textures/{}.png", offer.icon.replace('\\', "/")) }
                    else { look.map(|l| l.icon.clone()).filter(|i| !i.is_empty()).unwrap_or_else(|| icon.unwrap_or_default().into()) };
                // Older fixed items (including the Kingshill set) have no icon
                // configuration. Use the current equipment frame for their
                // template rarity; the offer still controls the background.
                let config = items.and_then(|c| c.icon_configs.get(&offer.icon_config)
                    .or_else(|| def.and_then(|d| c.icon_configs.get(&d.icon_config)))
                    .or_else(|| c.icon_configs.get(&format!("item_equipment_{rarity_name}")))
                    .or_else(|| c.icon_configs.get("item_generic")));
                let background = config.and_then(|c| if c.rarity { items.and_then(|i| i.rarity_backgrounds.get(rarity as usize)).map(String::as_str) }
                    else { Some(c.background.as_str()) }).or(background).unwrap_or_default().into();
                let frame = config.map(|c| c.frame.as_str()).filter(|f| !f.is_empty()).or(frame).unwrap_or_default().into();
                cards.push(MerchantItem { name: def.map(|d| d.name.clone()).unwrap_or_else(|| readable(&offer.item)), icon, background, frame,
                    amount: offer.amount, prices: prices(offer, &offers.shop, catalog), action: Some(Action::Buy(offer.id.clone())),
                    affordable: wallet.is_none_or(|w| affordable(offer, &offers.shop, catalog, w)) });
            }
        }
    }
    let rows = cards.iter().map(|c| price_rows(&c.prices)).max().unwrap_or(1);
    let row_height = 64 + (rows - 1) * 17;
    let page_size = (384 / row_height).max(1) as usize * 2;
    let pages = cards.len().div_ceil(page_size).max(1);
    let page = page.min(pages - 1);
    let items = cards.into_iter().skip(page * page_size).take(page_size).collect();
    MerchantView { items, tabs, category, buyback, usable: selection.usable, can_sell, page, pages, row_height }
}

fn event(w: &mut Widget, view: &View, action: Action) {
    if let Some(index) = view.options.iter().position(|c| c.action == action) { w.event = format!("NpcChoice:{index}"); }
    else { w.enabled = false; w.event.clear(); }
}

fn selected(w: &mut Widget, selected: bool) {
    if !selected { return; }
    // Keep the source pressed artwork active, including checked checkboxes.
    if let Some(state) = &mut w.state {
        match state.as_str() {
            "normal" | "mouseover" => w.hidden = true,
            "pressed" => *state = "normal".into(),
            "mouseoverpressed" => *state = "mouseover".into(),
            _ => {}
        }
    }
}

fn clone_widget(widget: &Widget, from: &str, to: &str) -> Widget {
    let mut w = widget.clone();
    w.id = w.id.replacen(from, to, 1);
    w.button = w.button.map(|b| b.replacen(from, to, 1));
    w.hidden = false;
    w
}

pub(super) fn prepare(source: &Layout, view: &View, merchant: &MerchantView, layout: &mut Layout) {
    for widget in &source.widgets {
        let id = &widget.id;
        if ["/PremiumBtn", "/RestoreEntry", "/Tabs", "/TraderName", "/Template"].iter().any(|p| id.contains(p)) { continue; }
        if id.starts_with(&format!("{ROOT}/ListBackground/")) && id != LIST && !id.starts_with(&format!("{LIST}/")) { continue; }
        if merchant.buyback && id.contains("/showuse_bg") { continue; }
        if merchant.pages == 1 && ["/PrevPageBtn", "/NextPageBtn", "/PageNumText"].iter().any(|p| id.contains(p)) { continue; }
        let mut w = widget.clone();
        match w.event.as_str() {
            "Close" => w.event = format!("NpcChoice:{CLOSE}"),
            "PrevPage" => event(&mut w, view, Action::Previous),
            "NextPage" => event(&mut w, view, Action::Next),
            "SwitchFilter" => event(&mut w, view, Action::Usable(!merchant.usable)),
            "SwitchToBuy" => event(&mut w, view, Action::Buyback(false)),
            "SwitchToRestore" => event(&mut w, view, Action::Buyback(true)),
            _ => {}
        }
        if id.ends_with("/PageNumText") {
            w.id = id.replace("PageNumText", "MerchantPage");
            set_text(&mut w, &format!("{} / {}", merchant.page + 1, merchant.pages));
        }
        selected(&mut w, (id.contains("/BuyTabBtn/") && !merchant.buyback)
            || (id.contains("/RestoreTabBtn/") && merchant.buyback)
            || (id.contains("/ShowUsableBtn/") && merchant.usable));
        layout.widgets.push(w);
    }
    if !merchant.buyback {
        let prefix = format!("{ROOT}/Tabs/TradeTab1");
        // Six source tabs span the window; larger server category sets fit the
        // same strip rather than falling off the right edge.
        let width = (467.86 / merchant.tabs.len().max(1) as f32).min(77.98);
        for (index, (code, category)) in merchant.tabs.iter().enumerate() {
            let replacement = format!("{ROOT}/Tabs/Category{code}");
            for widget in source.widgets.iter().filter(|w| w.id == prefix || w.id.starts_with(&(prefix.clone() + "/"))) {
                if widget.id.contains("/PremiumLabel") { continue; }
                let mut w = clone_widget(widget, &prefix, &replacement);
                if widget.id == prefix { event(&mut w, view, Action::Category(*code)); }
                if widget.id.ends_with("/icon_tab") {
                    w.art = (!category.icon.is_empty()).then(|| image_art(w.rect, category.icon.clone()));
                    if category.icon.is_empty() {
                        w.text = Some(Label { text: category.name.clone(), key: "gui.runtime.dialogue".into(), size: 7.0,
                            color: [0.9, 0.78, 0.54, 1.0], align: [1, 1], bold: false, word_break: true, outline: Vec::new() });
                    }
                }
                selected(&mut w, Some(*code) == merchant.category);
                map_widget(&mut w, |x| 8.84 + (x - 8.84) * width / 77.98 + index as f32 * width, |y| y);
                layout.widgets.push(w);
            }
        }
    }
    for (index, item) in merchant.items.iter().enumerate() {
        let replacement = format!("{LIST}/Offer{index}");
        let (dx, dy) = ((index % 2) as f32 * 235.46, (index / 2) as f32 * merchant.row_height as f32);
        for widget in source.widgets.iter().filter(|w| w.id == ENTRY || w.id.starts_with(&format!("{ENTRY}/"))) {
            if ["/MoneyDisplay", "/FlagDiscount", "/LabelDiscount"].iter().any(|p| widget.id.contains(p)) { continue; }
            let mut w = clone_widget(widget, ENTRY, &replacement);
            if widget.id == ENTRY && merchant.row_height > 64 {
                map_widget(&mut w, |x| x, |y| stretch(y, 202.0, 223.38, (merchant.row_height - 64) as f32));
            }
            if w.id.ends_with("/ItemDescription") { set_text(&mut w, &item.name); }
            if w.id.ends_with("/ItemSlot/Icon") {
                // Authored icon configuration supplies the rarity background and
                // gold/silver frame, layered around the actual offer icon.
                for (suffix, texture) in [("Background", &item.background), ("Image", &item.icon), ("Frame", &item.frame)] {
                    if texture.is_empty() { continue; }
                    let mut layer = w.clone();
                    layer.id = format!("{replacement}/ItemSlot/{suffix}");
                    layer.text = None;
                    let mut art = image_art(layer.rect, texture.clone());
                    // An offer the wallet does not pay: greyscale, as the
                    // layouts' disabled states are (Art::saturation 0).
                    if !item.affordable { art.saturation = 0.0; }
                    layer.art = Some(art);
                    map_widget(&mut layer, |x| x + dx, |y| y + dy);
                    layout.widgets.push(layer);
                }
                w.art = None;
                set_text(&mut w, &if item.amount > 1 { item.amount.to_string() } else { String::new() });
                // The item is the original's Icon: a right click on it buys the
                // offer (or the stack back) and the trader's cursor shows over it
                // (controls::interact, crate::ui::cursor); a left click orders
                // nothing. The rest of the card takes no clicks.
                // EVIDENCE: release261 UI::TradingWidget::HandleEvent (0x14055FB8C):
                //   right button (event +0x3C 0xB, +0x40 1) on an Icon ->
                //   BuyItemClick (0x14055C418); the left button only names
                //   buttons (Close, SwitchFilter, NextPage, ..., Premium).
                //   UpdateMouseCursor (0x140565F3C): an Icon under the pointer
                //   sets CursorManager mode 2 (mousecursor/buy).
                if let Some(index) = item.action.as_ref().and_then(|a| view.options.iter().position(|c| c.action == *a)) {
                    w.kind = "Button".into();
                    w.event = format!("NpcOffer:{index}");
                    // Greyed: no click, no buy cursor, so no order the server
                    // would refuse (DEVIATION: the original takes the click and
                    // names the missing currency, insufficientVCToBuy...).
                    w.enabled = item.affordable;
                }
            }
            map_widget(&mut w, |x| x + dx, |y| y + dy);
            layout.widgets.push(w);
        }
        let (mut x, mut y) = (70.0, 201.0);
        for (price_index, price) in item.prices.iter().enumerate() {
            if x + price.width() > 238.0 { x = 70.0; y += 17.0; }
            let money = format!("{ENTRY}/MoneyDisplay/");
            if let Some(template) = source.widgets.iter().find(|w| w.id == money.clone() + &price.label) {
                let mut label = clone_widget(template, ENTRY, &replacement);
                label.id = format!("{replacement}/Price{price_index}/Amount");
                label.rect = [x + 19.0, y - 1.0, x + price.width(), y + 19.0];
                set_text(&mut label, &price.amount.to_string());
                if let Some(text) = &mut label.text { text.align = [0, 1]; }
                map_widget(&mut label, |x| x + dx, |y| y + dy);
                layout.widgets.push(label);
                if !price.texture.is_empty() {
                    let rect = [x + dx, y + dy, x + dx + 18.0, y + dy + 18.0];
                    let mut icon = template.clone();
                    icon.id = format!("{replacement}/Price{price_index}/Icon");
                    icon.text = None; icon.rect = rect; icon.hidden = false;
                    icon.art = Some(image_art(rect, price.texture.clone()));
                    layout.widgets.push(icon);
                } else if let Some(template) = source.widgets.iter().find(|w| w.id == money.clone() + &price.icon) {
                    let mut icon = clone_widget(template, ENTRY, &replacement);
                    icon.id = format!("{replacement}/Price{price_index}/Icon");
                    // Preserve the original Andermant glow, which extends past
                    // its 18px icon; other currencies use their authored quad.
                    let left = if price.icon.contains("andermant") { 69.91 } else { icon.rect[0] };
                    let top = if price.icon.contains("andermant") { 199.83 } else { icon.rect[1] };
                    map_widget(&mut icon, |p| p - left + x + dx, |p| p - top + y + dy);
                    layout.widgets.push(icon);
                }
            }
            x += price.width();
        }
        if item.prices.is_empty() && !merchant.buyback {
            caption(source, layout, &format!("{replacement}/Free"), [71.56 + dx, 201.0 + dy, 232.46 + dx, 221.0 + dy], "Free");
        }
    }
    if merchant.items.is_empty() {
        caption(source, layout, &format!("{ROOT}/SelectionCaption"), [22.0, 180.0, 464.0, 240.0], view.body.as_deref().unwrap_or(""));
    }
    for w in &mut layout.widgets { map_widget(w, |x| x + 12.0, |y| y + 12.0); w.k = 0.0; }
}

fn caption(source: &Layout, layout: &mut Layout, id: &str, rect: [f32; 4], text: &str) {
    if let Some(template) = source.widgets.iter().find(|w| w.id == format!("{ENTRY}/Label_empty/ItemDescription")) {
        let mut w = template.clone();
        w.id = id.into(); w.rect = rect; w.hidden = false;
        set_text(&mut w, text);
        layout.widgets.push(w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(id: &str, category: u16) -> Option<inventory::ShopOffer> {
        Some(inventory::ShopOffer { id: id.into(), item: id.into(), category, amount: 1, max_player_level: 100, ..default() })
    }

    #[test]
    fn server_categories_filter_cards_before_paging_and_keep_empty_tabs() {
        let mut shop = inventory::Offer { categories: vec![46, 4, 7, 4], ..default() };
        shop.offers = (0..25).map(|i| offer(&format!("gem{i}"), 4)).collect();
        shop.offers.extend([None, offer("misc", 46)]);
        let first = build(Some(&shop), &Selection::default(), 0, None, &Catalog::default(), None, None, None);
        assert_eq!(first.tabs.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [46, 4, 7]);
        assert_eq!(first.items[0].name, "Misc");
        assert_eq!(first.items[0].action, Some(Action::Buy("misc".into())));
        let selection = Selection { category: Some(4), ..default() };
        let last = build(Some(&shop), &selection, 99, None, &Catalog::default(), None, None, None);
        assert_eq!((last.page, last.pages, last.items.len()), (2, 3, 1));
        assert_eq!(last.items[0].name, "Gem24");
        let empty = build(Some(&shop), &Selection { category: Some(7), ..default() }, 0, None, &Catalog::default(), None, None, None);
        assert!(empty.items.is_empty());
        shop.categories.clear();
        let fallback = build(Some(&shop), &selection, 0, None, &Catalog::default(), None, None, None);
        assert_eq!(fallback.category, Some(4));
        assert_eq!(fallback.items.len(), 12);
    }

    #[test]
    fn usable_filter_and_hidden_offers_follow_level_and_class() {
        let mut catalog = Catalog::default();
        catalog.offers.insert("mage".into(), OfferLook { class_name: "mage".into(), icon: String::new() });
        let mut shop = inventory::Offer { offers: vec![offer("mage", 4), offer("high", 4), offer("all", 4)], ..default() };
        shop.offers[1].as_mut().unwrap().min_player_level = 50;
        let selection = Selection { usable: true, ..default() };
        let result = build(Some(&shop), &selection, 0, None, &catalog, Some(3), Some(20), None);
        assert_eq!(result.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["All"]);
        shop.offers[0].as_mut().unwrap().hide_not_suitable = true;
        let result = build(Some(&shop), &Selection::default(), 0, None, &catalog, Some(3), Some(20), None);
        assert_eq!(result.items.len(), 2);
        assert!(class_matches("mage;dwarf;", Some(3)));
    }

    #[test]
    fn prices_preserve_currency_domains_and_server_discount_rounding() {
        let catalog = Catalog { currencies: HashMap::from([("shop".into(), "materi.png".into())]), ..default() };
        let mut o = inventory::ShopOffer { price_rc: 40, price_vc: 12345, price_arena: 13, price_ic: 500, ..default() };
        let p = prices(&o, "shop", &catalog);
        assert_eq!(p.iter().map(|p| p.amount).collect::<Vec<_>>(), [1, 23, 45, 40, 13, 500]);
        assert_eq!(p.last().unwrap().texture, "materi.png");
        o.price_vc = 0; o.price_arena = 0; o.price_ic = 0;
        o.veteran_discounts = vec![("Veteran3".into(), 0.05)];
        assert_eq!(prices(&o, "shop", &catalog)[0].amount, 38);
        assert!(price_rows(&p) > 1);
    }

    #[test]
    fn buyback_refresh_preserves_the_current_catalogue() {
        let mut network = crate::gameplay::dialogue::NetworkDialogue::default();
        network.receive(&dsor_proto::commands::ServerCommand::Offer(inventory::Offer {
            shop: "shop".into(), can_sell: true, categories: vec![4], offers: vec![offer("gem", 4)], ..default()
        }), None);
        network.receive(&dsor_proto::commands::ServerCommand::Offer(inventory::Offer {
            shop: "shop".into(), keep_shop: true, items: vec![inventory::ItemInfo { id: 91, template: "sold".into(), third: 2, ..default() }], ..default()
        }), None);
        let shop = network.offers.get("shop").unwrap();
        assert!(shop.can_sell);
        assert_eq!(shop.categories, [4]);
        assert_eq!(shop.offers.len(), 1);
        let result = build(Some(shop), &Selection { buyback: true, ..default() }, 0, None, &Catalog::default(), None, None, None);
        assert!(result.buyback);
        assert_eq!(result.items[0].name, "Sold");
        assert_eq!(result.items[0].amount, 2);
        assert_eq!(result.items[0].action, Some(Action::Restore(91)));
        let mut choices = Vec::new();
        result.choices(&mut choices);
        assert!(choices.iter().any(|c| c.label == "Sold" && c.action == Action::Restore(91)));
    }

    #[test]
    fn the_item_icon_takes_the_right_click_and_the_card_no_click() {
        let (icon, name) = (format!("{ENTRY}/ItemSlot/Icon"), format!("{ENTRY}/Label_empty/ItemDescription"));
        let source = controls::template::fixture::layout("tradingscreen", &[(ENTRY, "Label", false), (&icon, "Icon", true), (&name, "TextLabel", true)]);
        let shop = inventory::Offer { offers: vec![offer("gem", 4), offer("rune", 4)], ..default() };
        let merchant = build(Some(&shop), &Selection::default(), 0, None, &Catalog::default(), None, None, None);
        let mut options = Vec::new();
        merchant.choices(&mut options);
        let view = View { name: String::new(), portrait: String::new(), body: None, merchant: None, quest: None, options };
        let mut layout = Layout { window: "tradingscreen".into(), widgets: Vec::new() };
        prepare(&source, &view, &merchant, &mut layout);
        let rune = view.options.iter().position(|c| c.action == Action::Buy("rune".into())).unwrap();
        let find = |id: String| layout.widgets.iter().find(|w| w.id == id).unwrap();
        let icon = find(format!("{LIST}/Offer1/ItemSlot/Icon"));
        assert_eq!((icon.kind.as_str(), icon.event.clone()), ("Button", format!("NpcOffer:{rune}")));
        let card = find(format!("{LIST}/Offer1"));
        assert!(card.kind != "Button" && card.event.is_empty(), "a left click on the card orders nothing");
    }

    /// As the server debits a purchase: each price from its own balance, the
    /// shop's currency (IC) from its wallet key, the Honor or the Draken after
    /// their own price; the veteran discount first.
    #[test]
    fn the_wallet_pays_an_offer_as_the_server_debits_it() {
        let catalog = Catalog { currency_items: HashMap::from([("arena".into(), currency::HONOR.into()), ("cook".into(), "money_cc".into())]), ..default() };
        let wallet = Wallet { andermant: 100, copper: 5_000, honor: 30, legacy: 2, draken: 4, glyphs: 9,
            keyed: HashMap::from([("money_cc".into(), 7)]) };
        let pays = |shop: &str, set: fn(&mut inventory::ShopOffer)| {
            let mut o = inventory::ShopOffer::default();
            set(&mut o);
            affordable(&o, shop, &catalog, &wallet)
        };
        assert!(pays("x", |o| o.price_rc = 100) && !pays("x", |o| o.price_rc = 101));
        assert!(pays("x", |o| o.price_vc = 5_000) && !pays("x", |o| o.price_vc = 5_001));
        assert!(pays("x", |o| o.price_gc = 9) && !pays("x", |o| o.price_gc = 10));
        assert!(pays("x", |o| o.price_uc = 2) && !pays("x", |o| o.price_uc = 3));
        assert!(pays("x", |o| o.price_ec = 4) && !pays("x", |o| o.price_ec = 5));
        assert!(pays("arena", |o| { o.price_arena = 20; o.price_ic = 10; }) && !pays("arena", |o| { o.price_arena = 20; o.price_ic = 11; }));
        assert!(pays("cook", |o| o.price_ic = 7) && !pays("cook", |o| o.price_ic = 8));
        assert!(pays("unknown", |o| o.price_ic = 1_000), "a shop without a known currency is left to the server");
        // 105 with 5% off: 99.75, rounded to 100.
        assert!(pays("x", |o| { o.price_rc = 105; o.veteran_discounts = vec![("Veteran3".into(), 0.05)]; }));
        assert_eq!(andermant(&inventory::ShopOffer { price_rc: 40, veteran_discounts: vec![("Veteran3".into(), 0.05)], ..default() }), 38);
    }

    /// What the wallet does not pay: its item greyscale and no click (no
    /// order, no buy cursor); before the wallet is known, nothing is greyed.
    #[test]
    fn offers_the_wallet_does_not_pay_are_greyed_and_take_no_click() {
        let (icon, name) = (format!("{ENTRY}/ItemSlot/Icon"), format!("{ENTRY}/Label_empty/ItemDescription"));
        let source = controls::template::fixture::layout("tradingscreen", &[(ENTRY, "Label", false), (&icon, "Icon", true), (&name, "TextLabel", true)]);
        let mut shop = inventory::Offer { offers: vec![offer("gem", 4), offer("statue", 4)], ..default() };
        for o in shop.offers.iter_mut().flatten() { o.icon = o.id.clone(); }
        shop.offers[1].as_mut().unwrap().price_rc = 30_000;
        let wallet = Wallet { andermant: 20_000, ..default() };
        let merchant = build(Some(&shop), &Selection::default(), 0, None, &Catalog::default(), None, None, Some(&wallet));
        assert_eq!(merchant.items.iter().map(|i| i.affordable).collect::<Vec<_>>(), [true, false]);
        let unknown = build(Some(&shop), &Selection::default(), 0, None, &Catalog::default(), None, None, None);
        assert!(unknown.items.iter().all(|i| i.affordable));
        let mut options = Vec::new();
        merchant.choices(&mut options);
        let view = View { name: String::new(), portrait: String::new(), body: None, merchant: None, quest: None, options };
        let mut layout = Layout { window: "tradingscreen".into(), widgets: Vec::new() };
        prepare(&source, &view, &merchant, &mut layout);
        let find = |id: String| layout.widgets.iter().find(|w| w.id == id).unwrap();
        let saturation = |card: usize| find(format!("{LIST}/Offer{card}/ItemSlot/Image")).art.as_ref().unwrap().saturation;
        assert!(find(format!("{LIST}/Offer0/ItemSlot/Icon")).enabled && saturation(0) == 1.0);
        assert!(!find(format!("{LIST}/Offer1/ItemSlot/Icon")).enabled && saturation(1) == 0.0);
    }

    #[test]
    fn fixed_items_without_icon_configuration_use_current_frames_and_offer_rarity() {
        let items: items::Catalog = serde_json::from_value(serde_json::json!({
            "items": {"carbine": {"name":"Carbine", "description":"", "category":"Weapon", "icon":"carbine.png",
                "max_stack":1, "required_level":1, "slots":[5,6], "class_name":"dwarf", "rarity":"Unique"}},
            "ingredients":[], "colors":{}, "icon_configs": {
                "item_equipment_unique": {"rarity":true, "background":"", "frame":"gold.png"},
                "override": {"rarity":false, "background":"override.png", "frame":"override-frame.png"}
            }, "rarity_backgrounds":["", "", "", "", "", "yellow.png", "blue.png"]
        })).unwrap();
        let mut shop = inventory::Offer { offers: vec![offer("carbine", 46)], ..default() };
        shop.offers[0].as_mut().unwrap().rarity = 6;
        let result = build(Some(&shop), &Selection::default(), 0, Some(&items), &Catalog::default(), Some(3), Some(1), None);
        assert_eq!((&result.items[0].background, &result.items[0].frame), (&"blue.png".to_string(), &"gold.png".to_string()));
        shop.offers[0].as_mut().unwrap().icon_config = "override".into();
        let result = build(Some(&shop), &Selection::default(), 0, Some(&items), &Catalog::default(), Some(3), Some(1), None);
        assert_eq!(result.items[0].frame, "override-frame.png");
    }
}
