//! Item sections follow tooltip.bxml's text, gem, separator and currency
//! templates. Only fields whose wire meaning is known are shown as values.
use super::*;
use crate::ui::controls::tooltips::card::{Card, WHITE, MUTED, BLUE, GOLD, GREEN, plain};

fn number(value: f32, advanced: bool) -> String {
    let s = if advanced || value.abs()<1.0 { format!("{value:.3}") } else { format!("{value:.0}") };
    if s.contains('.') {s.trim_end_matches('0').trim_end_matches('.').to_owned()} else {s}
}

fn attribute(value: &values::Value, advanced: bool) -> String {
    format!("{} {}{} {}{}", if value.amount < 0.0 { "-" } else { "+" },
        number(value.amount.abs()*if value.relative {100.0} else {1.0},advanced),
        if value.relative {"%"} else {""}, value.name.to_lowercase(),
        if value.item_local {" on this item"} else {""})
}

fn readable(id: &str) -> String {
    let mut result = String::new();
    for (i,c) in id.chars().enumerate() {
        if c == '_' { result.push(' '); }
        else { if i > 0 && c.is_uppercase() { result.push(' '); } result.push(c); }
    }
    result.trim().to_owned()
}

/// The sticker card's text against an item card's (Card::scale_text): the
/// original's "Use, is not depleted when used" is 2.04 sticker cells long, ours
/// was 1.57 at 0.48 (the user's clip of the original's White Tiger card and their
/// shot of our Battle-boar's, 2026-10-30).
/// FAILURE (2026-10-29): the two shots of the Gray's card were taken the wrong
///   way round, so the text was made smaller (0.55 -> 0.48) and the card wider.
/// UNVERIFIED: measured from screenshots.
const STICKER_TEXT: f32 = 0.565;
/// The widest a sticker card grows (reference pixels) before its lines wrap,
/// its heading's size and its lines' height (in text sizes).
/// EVIDENCE: the user's recording 2026-10-30 (20261030-1640-20: ours at 2.0 s,
///   the original's Manticore card at 11.0 s, same video scale): the body lines
///   as wide as ours, but the original's heading 0.79 of ours, its lines 23
///   video pixels apart (ours 27), and its description wraps at 760 pixels
///   (278 reference pixels; card 301), where ours ran on to the 360 cap.
const STICKER_MAX_WIDTH: f32 = 312.0;
const STICKER_TITLE: f32 = 0.87;
const STICKER_LEADING: f32 = 1.24;
/// Its spaces twice a Tahoma space: the original's words stand apart, its
/// letters a little smaller (0.91 of ours), so its lines are as wide as ours were.
/// EVIDENCE: the original's Cute Doggy card (recording 20261030-1714-37):
///   "(Right click to call your Companion over)" glyphs Tahoma at 18.5 video px,
///   line 286 px = 257 natural + 5 spaces x 5.8 px; lines 23 px apart.
/// FAILURE (2026-10-30): our lines tight, the letters bigger ("the writing is
///   a different style").
const STICKER_WORD_SPACE: f32 = 0.31;

/// A Collector's Bag sticker's card, as the original's (the user's release261_192
/// shots of the Manticore and the armoured war horse): its name and kind in its
/// rarity's colour, then gui.window consumableUnderTitleInfinite, its description,
/// noSale and howToUseMount / howToUsePet.
/// FAILURE (2026-10-24): a bare "Sticker / Shows it in the preview" line.
/// A card in the original's small hand (the sticker and buff cards): the
/// sticker card's letters, line spacing and word spacing, its dark background.
pub(super) fn small_card(layout: Option<&Layout>) -> Card {
    let mut card = Card::new(STICKER_MAX_WIDTH, layout);
    card.scale_text(STICKER_TEXT);
    card.scale_title(STICKER_TITLE);
    card.set_leading(STICKER_LEADING);
    card.set_word_space(STICKER_WORD_SPACE);
    card.background = Color::srgba(0.01, 0.01, 0.005, 0.985);
    card
}

/// A small card as wide as its longest line, as the original's.
pub(super) fn fit_small<'a>(card: &mut Card, lines: impl Iterator<Item = &'a str>) {
    let size = card.text_size();
    let longest = |text: &str| plain(text).lines()
        .map(|l| crate::network::chat::text_width(l, size) + l.matches(' ').count() as f32 * STICKER_WORD_SPACE * size).fold(0.0, f32::max);
    let widest = lines.map(longest).fold(0.0, f32::max);
    card.width = (widest.ceil() + 36.0).clamp(120.0, STICKER_MAX_WIDTH);
}

/// A sticker's card; with `upgrade` (Altered Crescerite owned, price) the card of
/// its arrow: the next tier's, headed "Update to", its price at the bottom.
/// EVIDENCE: the original's arrow card (the user's recording 20261031-1414-12):
///   "Update to" / "Karabossa Doll" in the tier's blue / Pet / Use, is not
///   depleted when used / Effect (the tier's values) / "Upgrade costs" /
///   "Altered Crescerite: 0/5"; no description, no hints.
/// With `reward` (a book reward's "Reward for 9 x Pets") that line ends it in
/// place of "Not for sale" and the hint.
#[allow(clippy::too_many_arguments)]
pub(super) fn sticker(catalog: &Catalog, layout: Option<&Layout>, template: &str, title: &str, status: Option<&super::super::effects::Catalog>, bag: &HashMap<String, f32>, class: &str, upgrade: Option<(u64, u32)>, reward: Option<&str>) -> Card {
    let meta = catalog.items.get(template);
    let mut card = Card::new(STICKER_MAX_WIDTH, layout);
    // The original writes it in a small hand, its name no larger than the rest.
    // FAILURE (2026-10-25): about 1.8 times the original's (the user's shots of the
    //   Battle-boar's card, measured against the sticker icon).
    card.scale_text(STICKER_TEXT);
    card.scale_title(STICKER_TITLE);
    card.set_leading(STICKER_LEADING);
    card.set_word_space(STICKER_WORD_SPACE);
    card.background = Color::srgba(0.01, 0.01, 0.005, 0.985);
    let (rarity, tint) = rarity(catalog, template, None);
    // A template has no instance colour; the original writes a Mythic one red.
    // UNVERIFIED: the exact red, matched by eye on the user's Manticore shot.
    let tint = if rarity == "Mythic" { Color::srgb(0.86, 0.12, 0.08) } else { tint };
    // The item's name ("Cake Boss"); the sticker's own is often its id or a
    // shared draft ("# Cake Loving Meanie" for both cake pets), so it is only
    // the fallback.
    // FAILURE (2026-10-31): both cake pets "A Cake-Loving Meanie" where the
    //   original's cards say "Cake Meanie" and "Cake Boss".
    let item_name = catalog.name(template, None).trim();
    let name = format!("<i>{}</>", if item_name.is_empty() || item_name == template { title } else { item_name });
    let kind = meta.filter(|m| !m.category.is_empty()).map(|m| format!("<i>{}</>", m.category));
    let description = meta.filter(|m| !m.description.is_empty() && upgrade.is_none())
        .map(|m| if m.description.contains('<') { m.description.clone() } else { format!("<i>{}</>", m.description) });
    let pet = meta.is_some_and(|m| matches!(m.item_category.as_str(), "Pet" | "Mercenary"));
    let how = if pet { "(Right click to call your Companion over)" } else { "(Right click to mount or equip)" };
    let used = "<i>Use, is not depleted when used</>";
    // As wide as its longest line, as the original's.
    // FAILURE (2026-10-30): a fixed wide card, its right half empty.
    // Its spaces widened (STICKER_WORD_SPACE); the Effect lines count too.
    let effect = meta.filter(|m| !m.status.is_empty()).zip(status)
        .map(|(m, s)| super::super::effects::card_lines(s, &m.status, &m.status_values, bag, class)).unwrap_or_default();
    let longest = |text: &str, size: f32| plain(text).lines()
        .map(|l| crate::network::chat::text_width(l, size) + l.matches(' ').count() as f32 * STICKER_WORD_SPACE * size).fold(0.0, f32::max);
    let size = card.text_size();
    // What is owned counts up to the price ("5/5" with more in the wallet), as
    // the original's.
    let price = upgrade.map(|(have, cost)| format!("Altered Crescerite: {}/{cost}", have.min(u64::from(cost))));
    let ending = if reward.is_some() { [reward, None] } else { [Some("Not for sale"), Some(how)] };
    let body = [kind.as_deref(), Some(used), description.as_deref(), ending[0], ending[1], price.as_deref()];
    let widest = body.into_iter().flatten().chain(effect.iter().map(|(t, _)| t.as_str())).map(|t| longest(t, size))
        .fold(longest(&name, card.title_size()), f32::max);
    card.width = (widest.ceil() + 36.0).clamp(120.0, STICKER_MAX_WIDTH);
    // A little room before the description and more before "Not for sale"
    // (the original: 29 and 38 video pixels where its lines are 23 apart).
    if upgrade.is_some() { card.centered("Update to", WHITE, false); }
    card.heading(&name, tint);
    if let Some(kind) = &kind { card.text(kind, tint); }
    card.text(used, MUTED);
    card.gap(2.0);
    if let Some(description) = &description { card.text(description, WHITE); }
    // A pet's Effect section: its own bonus (by the tier owned) and the
    // Collector's Bag bonuses its owner has earned, as the original's card.
    // FAILURE (2026-10-30): no Effect section ("the pets look wrong").
    if !effect.is_empty() {
        card.gap(2.0);
        card.rule();
        card.centered("Effect", MUTED, false);
        for (text, header) in &effect { card.text(text, if *header { WHITE } else { BLUE }); }
    }
    if let Some(price) = &price {
        card.gap(2.0);
        card.rule();
        card.centered("Upgrade costs", MUTED, false);
        card.text(price, WHITE);
        return card;
    }
    if let Some(reward) = reward {
        card.gap(2.5);
        card.text(reward, Color::srgb(0.95, 0.85, 0.2));
        return card;
    }
    card.gap(5.0);
    card.text("Not for sale", WHITE);
    card.text(how, Color::srgb(0.95, 0.85, 0.2));
    card
}

/// The colour an item template's name is written in (rarity).
pub(super) fn rarity_tint(catalog: &Catalog, template: &str) -> Color { rarity(catalog, template, None).1 }

/// A Collector's Bag item reward's card: a pet's own card, else the item's name
/// in its rarity's colour, "Consumed when used" for what is used up, its
/// description and lifetime; then "Reward for n x <category>".
/// EVIDENCE: the original's Enigmatic Hatchlings bar (the user's recording
///   20261031-1621-02) and Draken/Premium cards: "Legendary Equipment" orange /
///   Consumed when used / Contains a Legendary piece of equipment for your
///   level. / Reward for 2 x Pets; lines 30 px apart where the icon is 120
///   (the small hand's 24), 7 more before "Reward for"; a pet ("Spiky...") its
///   whole pet card, then "Reward for 9 x Pets".
pub(super) fn reward_item(catalog: &Catalog, layout: Option<&Layout>, template: &str, footer: &str, lifetime: u32,
    status: Option<&super::super::effects::Catalog>, bag: &HashMap<String, f32>, class: &str) -> Card {
    let meta = catalog.items.get(template);
    let yellow = Color::srgb(0.95, 0.85, 0.2);
    let footer = format!("<i>{footer}</>");
    if meta.is_some_and(|m| matches!(m.item_category.as_str(), "Pet" | "Mercenary")) {
        return sticker(catalog, layout, template, "", status, bag, class, None, Some(&footer));
    }

    // Larger than the sticker cards' hand: the original's "Right click to add the
    // currency to your Currency Bag." is 4.85 icon widths long where ours was 4.06,
    // its lines 0.27 of the icon apart where ours were 0.19 (the user's Draken and
    // Community Coin shots, 2026-10-31).
    let mut card = small_card(layout);
    card.scale_text(1.19);
    card.set_leading(STICKER_LEADING * 1.19);
    let title = format!("<i>{}</>", catalog.name(template, None).trim());
    let consumed = meta.is_some_and(|m| matches!(m.item_category.as_str(), "ItemCurrency" | "NoAutoUseCurrency" | "Consumable"
        | "ConsumablePaymentItem" | "LootGenItem" | "InstantConsumable" | "Key" | "TravelMapItem"));
    // The currencies' description is their hint, in its own tan (db.itemcurrency
    // rightClickForCurrencyBag "<cE0C884FF>...</>"; the export drops the markup).
    let hint = meta.is_some_and(|m| matches!(m.item_category.as_str(), "ItemCurrency" | "NoAutoUseCurrency"));
    // The essences' text is a "Localize:" placeholder in the game files (the
    // original builds it from their values): left out.
    // UNVERIFIED/TODO: the original's "Increases attack power. Used up after
    //   each attack. / 300.00% increased damage / No effect in official arenas."
    let description: Vec<String> = meta.filter(|m| !m.description.starts_with("Localize:")).map(|m| m.description.replace("\\n", "\n")).unwrap_or_default()
        .split('\n').map(|l| if l.is_empty() { String::new() } else { format!("<i>{l}</>") }).collect();
    let duration = (lifetime > 0).then(|| format!("Duration: {}d {}h {}m {}s", lifetime / 86400, lifetime / 3600 % 24, lifetime / 60 % 60, lifetime % 60));
    // An essence: its slot's line in italics, its modifiers upright in blue,
    // set in under it, a rule, then the footer (the original's Essence of
    // Vigor reward card, recording 20261031-1658-42 at 1.6 s).
    let essence: Vec<String> = meta.map(|m| m.card_lines.clone()).unwrap_or_default();
    if !essence.is_empty() {
        let first = format!("<i>{}</>", essence[0]);
        let values: Vec<String> = essence[1..].iter().map(|l| format!("        {l}")).collect();
        fit_small(&mut card, std::iter::once(title.as_str()).chain(std::iter::once(first.as_str())).chain(values.iter().map(String::as_str)).chain(std::iter::once(footer.as_str())));
        card.centered(&title, rarity_tint(catalog, template), false);
        card.text(&first, WHITE);
        for line in &values { card.text(line, BLUE); }
        card.gap(3.0);
        card.rule();
        card.text(&footer, yellow);
        return card;
    }
    fit_small(&mut card, std::iter::once(title.as_str()).chain(std::iter::once("<i>Consumed when used</>"))
        .chain(description.iter().map(String::as_str)).chain(duration.as_deref()).chain(std::iter::once(footer.as_str())));
    card.centered(&title, rarity_tint(catalog, template), false);
    if consumed { card.text("<i>Consumed when used</>", WHITE); }
    for line in description.iter().skip_while(|l| l.is_empty() && !consumed) {
        if line.is_empty() { card.gap(8.0); } else { card.text(line, if hint { Color::srgb(0.88, 0.78, 0.52) } else { WHITE }); }
    }
    if let Some(duration) = &duration { card.text(duration, Color::srgb(0.9, 0.12, 0.1)); }
    card.gap(2.5);
    card.text(&footer, yellow);
    card
}

/// An item's rarity and the colour its name is written in.
pub(super) fn rarity<'a>(catalog: &'a Catalog, template: &str, item: Option<&ItemInfo>) -> (&'a str, Color) {
    let rarity = item.map(|i| ["Common", "Improved", "Magic", "Extraordinary", "Legendary", "Unique", "Set", "Mythic"].get(i.kind as usize).copied().unwrap_or("Item"))
        .unwrap_or_else(||catalog.items.get(template).map_or("Item",|m|m.rarity.as_str()));
    // A template's rarity is the catalog's name for it (items::RARITIES: Rare,
    // Epic), an instance's the game's (Extraordinary, Legendary).
    // FAILURE (2026-10-30): an Epic pet's name white where the original's
    //   Cute Doggy (Epic) is orange.
    // EVIDENCE: Uncommon measured on the original's Magotina Doll card (the
    //   user's recording 20261031-1414-12): (108, 225, 83).
    // Magic and Rare measured on the original's Blue Dragonspawn arrow cards
    // (recording 20261031-1506-03): (19, 120, 236) and (226, 0, 236).
    let tint = match rarity { "Unique" => Color::srgb(1.0,0.9,0.0), "Set" => GREEN, "Magic" => Color::srgb(0.07,0.47,0.93),
        "Uncommon" => Color::srgb(0.42,0.88,0.33),
        "Extraordinary" | "Rare" => Color::srgb(0.89,0.0,0.93), "Legendary" | "Epic" => Color::srgb(1.0,0.65,0.15), _ => color(catalog,item) };
    (rarity, tint)
}

pub(super) fn card(d: &DisplayItem, catalog: &Catalog, layout: Option<&Layout>, advanced: bool) -> Card {
    let meta = catalog.items.get(d.template);
    let mut card = Card::new(if advanced { 370.0 } else { 285.0 },layout);
    card.background = Color::srgba(0.01, 0.01, 0.005, 0.985);
    let (rarity, tint) = rarity(catalog, d.template, d.item);
    // The heading as Dofus draws it: icon, name, "Level n \u{2022} type"
    // (the item's level, else the level it requires; the rarity in the name's colour).
    let (icon, background, frame) = catalog.icon_parts(d.template, d.item);
    let icons: Vec<String> = [background, icon, frame].into_iter().flatten().map(str::to_owned).collect();
    let level = d.item.map(|i| i.durability).filter(|l| *l > 0).or_else(|| meta.map(|m| m.required_level.max(d.unlock)));
    // "EquipmentGem" -> "Equipment Gem".
    let kind = meta.map(|m| m.category.as_str()).filter(|c| !c.is_empty()).map(|c| {
        let mut out = String::new();
        for (i, ch) in c.chars().enumerate() {
            if i > 0 && ch.is_uppercase() && !out.ends_with(' ') { out.push(' '); }
            out.push(ch);
        }
        out
    });
    let subtitle = match (level, kind) {
        (Some(l), Some(k)) => format!("Level {l} \u{2022} {k}"),
        (Some(l), None) => format!("Level {l}"),
        (None, Some(k)) => k,
        (None, None) => String::new(),
    };
    card.header(catalog.name(d.template,d.item), tint, &icons, &subtitle);
    if let Some(m) = meta {
        if m.required_level.max(d.unlock)>1 { card.hint(&format!("Required level: {}",m.required_level.max(d.unlock))); }
        if m.max_stack>1 { card.text(&format!("Quantity: {} / {}",d.amount,m.max_stack),WHITE); }
    }

    if let Some(item) = d.item {
        if item.flag_0 { card.section("Identification"); card.text("Unidentified",GOLD); }
        else {
            let values=catalog.values.item(item);
            if !values.base.is_empty() {
                card.section("Base Values");
                if advanced {
                    for value in &values.base {
                        let quality=value.quality.map_or(String::new(),|q|format!(" ({:.0}%)",q*100.0));
                        card.text(&format!("{}{}",attribute(value,true),quality),WHITE);
                    }
                } else {
                    let mut names=std::collections::HashSet::new();
                    for value in &values.base {
                        if !value.item_local || value.relative {card.text(&attribute(value,false),WHITE);}
                        else if names.insert(&value.name) {card.text(&format!("+ <GreenStart>{}</>  {}",number(values.total(&value.name),false),value.name),WHITE);}
                    }
                }
            }
            if item.names.iter().any(|g|!g.is_empty()) || item.tenth>0 {
                card.section("Gems");
                if advanced {
                    let mut groups=std::collections::HashSet::new();
                    for value in &values.gems {
                        if !groups.insert((&value.name,value.relative,value.item_local)) {continue;}
                        let group:Vec<_>=values.gems.iter().filter(|v|v.name==value.name && v.relative==value.relative && v.item_local==value.item_local).collect();
                        let factor=if value.relative {100.0} else {1.0};
                        let details=if group.len()>1 {format!(" ({})",group.iter().map(|v|number(v.amount*factor,true)).collect::<Vec<_>>().join(", "))} else {String::new()};
                        let mut combined=value.clone(); combined.amount=group.iter().map(|v|v.amount).sum();
                        card.text(&format!("{}{}",attribute(&combined,true),details),BLUE);
                    }
                    card.gap(3.0);
                } else {
                    for value in values.gems.iter().filter(|v| !v.item_local || !values.base.iter().any(|b| b.item_local && !b.relative && b.name==v.name)) {
                        card.text(&attribute(value,false),BLUE);
                    }
                }
                // Every socket: its gem, or empty (names holds "" for an empty
                // one; tenth is the item's socket count).
                // FAILURE: "ça affiche pas dans la description d'un items ses
                //   slots et les gems qu'il a equipé".
                let sockets=(item.tenth as usize).max(item.names.len());
                for n in 0..sockets {
                    match item.names.get(n).filter(|g|!g.is_empty()) {
                        Some(gem)=>card.text(&format!("Socket {}: {}",n+1,catalog.items.get(gem).map_or_else(||readable(gem),|g|g.name.clone())),BLUE),
                        None=>card.hint(&format!("Socket {}: empty",n+1)),
                    }
                }
                card.icons(item.names.iter().filter(|g|!g.is_empty()).filter_map(|name|catalog.icon(name,None).0.map(str::to_owned)));
            }
            let enchantments:Vec<_>=values.enchantments.iter().filter(|v| advanced || !v.item_local
                || !values.base.iter().any(|b| b.item_local && !b.relative && b.name==v.name)).collect();
            if !enchantments.is_empty() {
                card.section("Enchantments");
                for value in enchantments {
                    let quality=value.quality.filter(|_|advanced).map_or(String::new(),|q|format!(" ({:.0}%)",q*100.0));
                    let contribution=values.contribution(value).filter(|_|advanced).map_or(String::new(),|v|
                        format!("  <GreenStart>({}{})</>",if v<0.0 {"-"} else {"+"},number(v.abs(),true)));
                    card.text(&format!("{}{}{}",attribute(value,advanced),quality,contribution),BLUE);
                }
            }
            // Unknown enchantments remain readable, but their normalized roll
            // must never masquerade as an absolute item stat.
            if advanced {
                for stat in &item.statistics {
                    if !catalog.values.knows(&stat.name) {
                        let name=catalog.enchantments.get(&stat.name).map_or_else(||readable(&stat.name),|e|e.name.clone());
                        card.hint(&format!("{} (roll: {:.0}%)",name,stat.value.clamp(0.0,1.0)*100.0));
                    }
                }
            }
            if !values.unique.is_empty() {
                card.section("Unique Values");
                for value in &values.unique {card.text(&attribute(value,true),GOLD);}
            } else if let Some(m)=meta.filter(|m| !m.unique_values.is_empty()) {
                card.section("Unique Values");
                for value in &m.unique_values {card.text(value,GOLD);}
            }
            if let Some(m)=meta.filter(|m|!m.unique_effects.is_empty()) {
                if values.unique.is_empty() && m.unique_values.is_empty() {card.section("Unique Values");}
                for effect in &m.unique_effects {card.text(effect,GOLD);}
            }
        }
        if meta.is_some_and(|m|!m.description.is_empty()) {
            card.section(if matches!(rarity,"Unique"|"Set") {"Item Effects"} else {"Description"});
            card.text(&meta.unwrap().description,if matches!(rarity,"Unique"|"Set") {GOLD} else {WHITE});
        }
        if meta.is_some_and(|m|!m.slots.is_empty() && m.max_stack==1) {
            card.section("Customization");
            if item.named.is_empty() || item.named == "none" {card.hint("Not yet dyed");}
            else {card.hint(&format!("Dye: {}",catalog.items.get(&item.named).map_or_else(||readable(&item.named),|m|m.name.clone())));}
            if !item.fourth_name.is_empty() {card.hint("Appearance customized");}
        }
        card.gap(4.0); card.rule();
        // release261: field_34 is per-instance glyph yield; field_30 is
        // the complete stack's selling value (server item_economy_values.py).
        if item.fifty_second>0 {card.melting(item.fifty_second);}
        card.price(item.level);
    } else if let Some(m)=meta.filter(|m|!m.description.is_empty()) {card.rule();card.text(&m.description,WHITE);}
    if d.waiting {card.hint("Free up a slot to collect this item automatically.");}
    if d.amount==0 {card.hint("Not yet collected");}
    if d.item.is_some() && d.amount>1 && !d.read_only {card.hint("Ctrl + click: split stack");}
    if !advanced && d.item.is_some() {card.gap(3.0);card.rule();card.hint("Hold Y for advanced view.");}
    card
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_bonuses_remain_visible_in_both_views_with_their_correct_scope() {
        let catalog:Catalog=serde_json::from_str(include_str!("../../../../tests/fixtures/item_tooltips.json")).unwrap();
        for advanced in [false,true] {
            let item=ItemInfo {template:"all_mythic_ring_death".into(),third:1,kind:7,durability:100,eighteenth:8477,..default()};
            let text=preview_card(&catalog,None,&item.template,Some(&item),1,advanced).text_content();
            for bonus in ["+ 5 attack speed", "+ 5 movement speed", "+ 66% damage against players"] {
                assert!(text.contains(bonus),"{text}");
                assert!(!text.contains(&format!("{bonus} on this item")),"character bonus must not be item-local: {text}");
            }
            // The character's +5 movement speed must not contaminate the ring's base total.
            let values=catalog.values.item(&item);
            assert!(values.total("Movement speed") < 5.0);
            assert!(values.unique.iter().all(|v|values.contribution(v).is_none()));
            for (template,bonus) in [("unique_ring_stego_yellow_01","+ 2% attack speed"),
                ("all_unique_ring_pw_death","+ 0.108 attack speed on this item"),
                ("all_unique_ring_pw_fire","- 33% burn duration")] {
                let item=ItemInfo {template:template.into(),..item.clone()};
                let text=preview_card(&catalog,None,template,Some(&item),1,advanced).text_content();
                assert!(text.contains(bonus),"{template}: {text}");
            }
        }
    }
}
