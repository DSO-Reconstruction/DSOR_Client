//! Item sections follow tooltip.bxml's text, gem, separator and currency
//! templates. Only fields whose wire meaning is known are shown as values.
use super::*;
use crate::ui::controls::tooltips::card::{Card, WHITE, MUTED, BLUE, GOLD, GREEN};

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

/// A Collector's Bag sticker's card, as the original's (the user's release261_192
/// shots of the Manticore and the armoured war horse): its name and kind in its
/// rarity's colour, then gui.window consumableUnderTitleInfinite, its description,
/// noSale and howToUseMount / howToUsePet.
/// FAILURE (2026-10-24): a bare "Sticker / Shows it in the preview" line.
pub(super) fn sticker(catalog: &Catalog, layout: Option<&Layout>, template: &str) -> Card {
    let meta = catalog.items.get(template);
    let mut card = Card::new(285.0, layout);
    card.background = Color::srgba(0.01, 0.01, 0.005, 0.985);
    let (rarity, tint) = rarity(catalog, template, None);
    // A template has no instance colour; the original writes a Mythic one red.
    // UNVERIFIED: the exact red, matched by eye on the user's Manticore shot.
    let tint = if rarity == "Mythic" { Color::srgb(0.86, 0.12, 0.08) } else { tint };
    card.title(catalog.name(template, None), None, tint);
    if let Some(m) = meta.filter(|m| !m.category.is_empty()) { card.text(&format!("<i>{}</>", m.category), tint); }
    card.text("<i>Use, is not depleted when used</>", MUTED);
    if let Some(m) = meta.filter(|m| !m.description.is_empty()) {
        let text = if m.description.contains('<') { m.description.clone() } else { format!("<i>{}</>", m.description) };
        card.text(&text, WHITE);
    }
    card.gap(4.0);
    card.text("Not for sale", WHITE);
    let pet = meta.is_some_and(|m| matches!(m.item_category.as_str(), "Pet" | "Mercenary"));
    card.text(if pet { "(Right click to call your Companion over)" } else { "(Right click to mount or equip)" }, Color::srgb(0.95, 0.85, 0.2));
    card
}

/// An item's rarity and the colour its name is written in.
pub(super) fn rarity<'a>(catalog: &'a Catalog, template: &str, item: Option<&ItemInfo>) -> (&'a str, Color) {
    let rarity = item.map(|i| ["Common", "Improved", "Magic", "Extraordinary", "Legendary", "Unique", "Set", "Mythic"].get(i.kind as usize).copied().unwrap_or("Item"))
        .unwrap_or_else(||catalog.items.get(template).map_or("Item",|m|m.rarity.as_str()));
    let tint = match rarity { "Unique" => Color::srgb(1.0,0.9,0.0), "Set" => GREEN, "Magic" => BLUE,
        "Extraordinary" => Color::srgb(0.75,0.4,1.0), "Legendary" => Color::srgb(1.0,0.65,0.15), _ => color(catalog,item) };
    (rarity, tint)
}

pub(super) fn card(d: &DisplayItem, catalog: &Catalog, layout: Option<&Layout>, advanced: bool) -> Card {
    let meta = catalog.items.get(d.template);
    let mut card = Card::new(if advanced { 370.0 } else { 285.0 },layout);
    card.background = Color::srgba(0.01, 0.01, 0.005, 0.985);
    let (rarity, tint) = rarity(catalog, d.template, d.item);
    card.title(catalog.name(d.template,d.item),None,tint);
    card.text(&format!("{rarity} Item"),tint);
    if let Some(m) = meta.filter(|m|!m.category.is_empty()) { card.text(&format!("<i>({})</>",m.category),MUTED); }
    if let Some(i) = d.item { card.text(&format!("Item level: {}",i.durability),Color::srgb(0.85,0.75,0.5)); }
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
            if !item.names.is_empty() || item.tenth>0 {
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
                    if values.gems.is_empty() {for name in &item.names {card.text(&catalog.items.get(name).map_or_else(||readable(name),|g|g.name.clone()),BLUE);}}
                    if item.tenth as usize>item.names.len() {card.hint(&format!("{} empty sockets",item.tenth as usize-item.names.len()));}
                    card.gap(3.0);
                } else {
                    for value in values.gems.iter().filter(|v| !v.item_local || !values.base.iter().any(|b| b.item_local && !b.relative && b.name==v.name)) {
                        card.text(&attribute(value,false),BLUE);
                    }
                }
                card.icons(item.names.iter().filter_map(|name|catalog.icon(name,None).0.map(str::to_owned)));
                if item.names.is_empty() {card.hint("No gems socketed");}
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
