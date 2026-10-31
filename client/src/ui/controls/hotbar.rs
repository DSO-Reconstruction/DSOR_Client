//! Release261's eight skill, five collection and four consumable shortcuts.
//! Essences are inventory equipment 35/34, independent of those seventeen slots.
use super::*;
use crate::gameplay::inventory::{InventoryState, Place};
use dsor_proto::commands::{inventory::{ItemInfo, QuickSlot as Slot, UseStickerBookItem}, ClientCommand};

pub(super) const PRIMARY_ESSENCE: usize = 17;
pub(super) const BACKUP_ESSENCE: usize = 18;

/// Item and sticker use cooldowns (_Template_Item.CoolDown, shared by its
/// CoolDownCategory: every mount VanityItem 0.5 s, pets 2-5 s, Shadow Souls 30
/// s): (start, ready) on Time<Real>, by category, else by template. A use while
/// one runs is refused, and the quick slots draw it as a skill's.
/// EVIDENCE: static.db4 _Template_Item; the original sweeps and counts "0.4 s"
///   .. "0.0 s" over every sticker of the category (the user's recording
///   2026-10-30, album::grid).
/// FAILURE (2026-10-24): no cooldown at all on mounts and pets.
#[derive(Resource, Default)]
pub(in crate::ui) struct ItemCooldowns(std::collections::HashMap<String, (f64, f64)>);

impl ItemCooldowns {
    fn key(catalog: &items::Catalog, template: &str) -> Option<(String, f64)> {
        let d = catalog.items.get(template).filter(|d| d.cooldown > 0.0)?;
        Some((if d.cooldown_category.is_empty() { template.to_owned() } else { d.cooldown_category.clone() }, d.cooldown as f64))
    }
    /// Starts `template`'s cooldown; false while it still runs (the use is refused).
    pub(in crate::ui) fn start(&mut self, catalog: Option<&items::Catalog>, template: &str, now: f64) -> bool {
        let Some((key, seconds)) = catalog.and_then(|c| Self::key(c, template)) else { return true };
        if self.0.get(&key).is_some_and(|&(_, ready)| now < ready) { return false; }
        self.0.insert(key, (now, now + seconds));
        true
    }
    /// The cooldown key `template` shares (its category, else itself).
    pub(in crate::ui) fn key_of(catalog: Option<&items::Catalog>, template: &str) -> Option<String> {
        catalog.and_then(|c| Self::key(c, template)).map(|(key, _)| key)
    }
    /// The part left (1 just started) and the seconds left of `key`'s cooldown,
    /// while it runs.
    pub(in crate::ui) fn left(&self, key: &str, now: f64) -> Option<(f32, f64)> {
        self.0.get(key).filter(|&&(_, ready)| now < ready)
            .map(|&(start, ready)| ((((ready - now) / (ready - start).max(1e-6)) as f32).clamp(0.0, 1.0), ready - now))
    }
    /// The cooldowns running: key, tenths of a second left, part left in `steps`.
    pub(in crate::ui) fn running(&self, now: f64, steps: u8) -> Vec<(String, u32, u8)> {
        let mut out: Vec<_> = self.0.iter().filter(|(_, &(_, ready))| now < ready).map(|(key, &(start, ready))| {
            let left = ((ready - now) / (ready - start).max(1e-6)).clamp(0.0, 1.0);
            (key.clone(), crate::gameplay::skills::cooldown_tenths(ready - now), (left * steps as f64).ceil() as u8)
        }).collect();
        out.sort();
        out
    }
    /// The part of `template`'s cooldown left (1 just used, 0 ready) and its seconds.
    pub(in crate::ui) fn remaining(&self, catalog: Option<&items::Catalog>, template: &str, now: f64) -> (f32, f64) {
        let Some((key, _)) = catalog.and_then(|c| Self::key(c, template)) else { return (0.0, 0.0) };
        self.0.get(&key).filter(|&&(_, ready)| now < ready)
            .map_or((0.0, 0.0), |&(start, ready)| (((ready - now) / (ready - start).max(1e-6)) as f32, ready - now))
    }
}

/// The item or sticker a shortcut slot holds (its template), for its cooldown.
pub(in crate::ui) fn slot_template(index: usize, bar: usize, net: Option<&crate::network::net::Net>) -> Option<String> {
    let net = net?;
    let s = slot(net, bar, index)?;
    match s.kind {
        1 => item(s, &net.inventory).map(|i| i.template.clone()).or_else(|| s.id.clone()),
        2 => s.id.clone(),
        _ => None,
    }
}

#[derive(Resource, Default)]
pub(in crate::ui) struct Hotbar {
    pub active: usize,
    pub change: i32,
    pub swap_essence: bool,
}

pub(in crate::ui) fn slot_leaf(leaf: &str) -> Option<usize> {
    Some(match leaf {
        "LMB" => 0, "RMB" => 1, "TAB" => 7, "Mountslot" => 8, "Mercenaryslot" => 9,
        "LeftPotionTop" => 13, "LeftPotionBottom" => 14, "RightPotionTop" => 15, "RightPotionBottom" => 16,
        "Ammo" => PRIMARY_ESSENCE, "AmmoInactive" => BACKUP_ESSENCE,
        s => s.strip_prefix("QuickSlot")?.parse::<usize>().ok().filter(|s| (1..=5).contains(s))? + 1,
    })
}

pub(in crate::ui) fn label_slot(id: &str) -> Option<usize> {
    let leaf = id.rsplit('/').next()?;
    Some(match leaf {
        "TextLabel_Primary" => 0, "TextLabel_Secondary" => 1,
        "TextLabel_MSlot" => 8, "TextLabel_MercenarySlot" => 9,
        "TextLabel_PotionSlotLeftTop" => 13, "TextLabel_PotionSlotLeftBottom" => 14,
        "TextLabel_PotionSlotRightTop" => 15, "TextLabel_PotionSlotRightBottom" => 16,
        "tab_arrows" => 7,
        _ => leaf.strip_prefix("TextLabel_QSlot")?.parse::<usize>().ok()? + 1,
    })
}

pub(in crate::ui) fn prepare(layout: &Layout) -> Layout {
    let mut out = layout.clone();
    for w in &mut out.widgets {
        let leaf = w.id.rsplit('/').next().unwrap_or("");
        if w.id.contains("/ammo_bg/") || w.id.contains("/ammo_bg1/") || matches!(leaf, "ammo_bg" | "ammo_bg1") {
            // The source hides runtime-populated essence sockets and counts.
            w.hidden = false;
            if matches!(leaf, "Ammo" | "AmmoInactive") { w.kind = "SlotButton".into(); w.click_through = false; }
            if matches!(leaf, "IconNorm" | "IconSmall") { w.hidden = true; }
        }
        // Home teleport has no implemented binding yet; never show the sample 1.
        if leaf == "TextLabel_HomeTeleport" { if let Some(t) = &mut w.text { t.text.clear(); } }
    }
    // Text labels share the binding source even for keyboard alternatives to
    // the two mouse attacks. Replace the baked mouse indicators with LMB/RMB.
    out.widgets.retain(|w| !w.id.contains("/LMB/Label_lm") && !w.id.contains("/RMB/Label_rm"));
    for (id,rect) in [("Primary",[563.0,759.5,595.0,767.5]),("Secondary",[597.5,759.5,630.0,767.5])] {
        out.widgets.push(Widget { id:format!("Canvas/Runtime/TextLabel_{id}"),kind:"TextLabel".into(),event:String::new(),click_through:true,enabled:true,button:None,state:None,spell:None,k:0.5,rect,hidden:false,art:None,
            text:Some(Label{text:String::new(),key:"gui.hotkey".into(),size:7.0,color:[0.9,0.78,0.54,1.0],align:[1,1],bold:false,word_break:false,outline:Vec::new()}) });
    }
    out
}

/// Kind 1 may contain an instance ID or an item-template name (the server's
/// default potion uses the latter). Only carried stacks are usable.
pub(super) fn item<'a>(slot: &Slot, inventory: &'a InventoryState) -> Option<&'a ItemInfo> {
    let id = slot.id.as_ref()?;
    let carried = |i: &&ItemInfo| i.third > 0 && matches!(inventory.location(i.id), Some(Place::Bag(_) | Place::Extra(_) | Place::Equipment(_)));
    if let Ok(id) = id.parse::<u32>() { return inventory.items.get(&id).filter(carried); }
    inventory.items.values().filter(|i| i.template == *id).filter(carried).min_by_key(|i| i.id)
}

pub(super) fn slot<'a>(net: &'a crate::network::net::Net, bar: usize, index: usize) -> Option<&'a Slot> {
    match &net.hidden_bar {
        Some(hidden) => hidden.get(index),
        None => net.quickbars.bars.get(bar)?.get(index),
    }
}

/// The skill a quick slot holds, if it holds one (not an item or essence).
pub(in crate::ui) fn skill_id(index: usize, bar: usize, net: Option<&crate::network::net::Net>, figures: &Figures) -> Option<String> {
    if index >= PRIMARY_ESSENCE { return None; }
    if let Some(s) = net.and_then(|n| slot(n, bar, index)) {
        return (s.kind == 0).then(|| s.id.clone()).flatten();
    }
    figures.bar.get(index)?.clone()
}

pub(super) fn essence(inventory: &InventoryState, index: usize) -> Option<&ItemInfo> {
    inventory.at(Place::Equipment(if index == PRIMARY_ESSENCE { 35 } else { 34 }))
}

/// A sticker shortcut's background (its rarity's or its icon configuration's),
/// drawn under its picture as the Collector's Bag draws it; the original's slot
/// shows the sticker's whole picture (the user's release261_192 bar, a snowman).
pub(in crate::ui) fn backdrop(index: usize, bar: usize, net: Option<&crate::network::net::Net>, catalog: Option<&items::Catalog>) -> Option<String> {
    let s = net.and_then(|n| slot(n, bar, index)).filter(|s| s.kind == 2)?;
    catalog?.icon(s.id.as_deref()?, None).1.map(str::to_owned)
}

/// A render key includes amount, so decrements and emptied stacks redraw as well.
pub(in crate::ui) fn appearance(index: usize, bar: usize, net: Option<&crate::network::net::Net>, figures: &Figures,
    catalog: Option<&items::Catalog>, skills: &SkillIcons) -> Option<(String, u32)> {
    if index >= PRIMARY_ESSENCE {
        let i = essence(&net?.inventory, index)?;
        return catalog?.icon(&i.template, Some(i)).0.map(|path| (path.into(), i.third));
    }
    if let Some(s) = net.and_then(|n| slot(n, bar, index)) {
        let id = s.id.as_deref()?;
        return match s.kind {
            0 => skills.0.get(id).map(|s| (s.clone(), 0)),
            1 => {
                let inventory = &net?.inventory;
                let stack = item(s, inventory);
                let template = stack.map_or(id, |i| i.template.as_str());
                let amount = if id.parse::<u32>().is_ok() { stack.map_or(0, |i| i.third) }
                    else { inventory.items.values().filter(|i| i.template == id && matches!(inventory.location(i.id), Some(Place::Bag(_) | Place::Extra(_)))).map(|i| i.third).sum() };
                catalog?.icon(template, stack).0.map(|p| (p.into(), amount))
            }
            2 => catalog?.icon(id, None).0.map(|p| (p.into(), 0)),
            _ => None,
        };
    }
    figures.bar.get(index)?.as_ref().and_then(|id| skills.0.get(id)).map(|s| (s.clone(), 0))
}

pub(in crate::ui) fn input(mut bar: ResMut<Hotbar>, keys: Res<ButtonInput<KeyCode>>, settings: Res<settings::Settings>,
    mut pointer: ResMut<Pointer>, windows: Query<(&Window2018, &Visibility)>, mut clicks: ResMut<SkillClicks>, net: Option<NonSendMut<crate::network::net::Net>>,
    (data, catalogs): (Res<items::Data>, Res<Assets<items::Catalog>>), (mut cooldowns, time): (ResMut<ItemCooldowns>, Res<Time<Real>>),
    (buttons, ui): (Res<ButtonInput<MouseButton>>, Res<crate::ui::input::UiInput>)) {
    let Some(mut net) = net else { return; };
    let blocked = pointer.focus.is_some() || settings.open || windows.iter().any(|(w,v)| *v != Visibility::Hidden && matches!(w.0.as_str(), "gamemenu" | "unstackwindow"));
    if blocked { bar.change = 0; bar.swap_essence = false; clicks.0 = None; return; }
    if settings.current.pressed("next_bar", &keys) { bar.change = 1; }
    if settings.current.pressed("previous_bar", &keys) { bar.change = -1; }
    if bar.change != 0 { pointer.spell_drag = None; pointer.spell_drop = None; pointer.down = None; }
    let count = net.quickbars.bars.len();
    if count > 0 {
        bar.active = (bar.active as i32 + bar.change).rem_euclid(count as i32) as usize;
        let shown = net.hidden_bar.as_ref().unwrap_or(&net.quickbars.bars[bar.active]);
        net.bar = shown.iter().map(|s| if s.kind == 0 { s.id.clone() } else { None }).collect();
    } else { bar.active = 0; }
    bar.change = 0;
    // The right button's skill and the TAB slot's change places (both kept by
    // the server: QuickSlotsCommand).
    if settings.current.pressed("swap_attack", &keys) && net.hidden_bar.is_none() {
        if let Some(mut slots) = net.quickbars.bars.get(bar.active).filter(|b| b.len() > 7).cloned() {
            slots.swap(1, 7);
            net.bar = slots.iter().map(|s| if s.kind == 0 { s.id.clone() } else { None }).collect();
            net.quickbars.bars[bar.active] = slots.clone();
            net.send(&ClientCommand::QuickSlots(dsor_proto::commands::inventory::QuickSlots { slots, bar: bar.active as u32 }));
        }
    }
    if bar.swap_essence || settings.current.pressed("essence", &keys) {
        if let Some(backup) = essence(&net.inventory, BACKUP_ESSENCE) {
            // Inventory's normal equipment move swaps the occupied primary slot.
            if let Some(command) = net.inventory.drop_on(backup.id, Place::Equipment(35), 1) { net.send(&command); }
        }
    }
    bar.swap_essence = false;
    if net.death.dead { clicks.0 = None; return; }
    // The mouse buttons' slots, when they hold a pet, mount or item rather than a
    // skill (crate::gameplay::skills casts those): one use a press, the right
    // button or Shift + the left one.
    // FAILURE (2026-10-24): a pet on the left button's slot did nothing on Shift +
    //   left click.
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let mouse = if !ui.blocks(MouseButton::Right) && buttons.just_pressed(MouseButton::Right) { Some(1) }
        else if shift && !ui.blocks(MouseButton::Left) && buttons.just_pressed(MouseButton::Left) { Some(0) } else { None };
    let mouse = mouse.filter(|i| slot(&net, bar.active, *i).is_some_and(|s| matches!(s.kind, 1 | 2)));
    let requested = clicks.0.or_else(|| settings.current.slot_pressed(&keys)).or(mouse);
    if let Some(index) = requested {
        if index >= PRIMARY_ESSENCE { clicks.0 = None; return; }
        // A shortcut used from the bar is no travel stone's.
        if net.travel_source.is_some() { net.travel_source = None; }
        if let Some(s) = slot(&net, bar.active, index) {
            // The Universal Map's slot opens the travel map, as its right click does.
            let map = s.kind == 1 && item(s, &net.inventory).zip(catalogs.get(&data.catalog)).is_some_and(|(i, c)| items::travel_map(c, &i.template));
            if map { pointer.open_window = Some("travelmap"); }
            // A mount or pet still cooling down is not used again.
            let ready = map || slot_template(index, bar.active, Some(&net)).is_none_or(|t| cooldowns.start(catalogs.get(&data.catalog), &t, time.elapsed_secs_f64()));
            let command = match s.kind {
                1 | 2 if map || !ready => None,
                1 => item(s, &net.inventory).map(|i| crate::gameplay::inventory::command(i.id, dsor_proto::commands::inventory::Inventory::USE, vec![])),
                2 => s.id.clone().map(|template| ClientCommand::UseStickerBookItem(UseStickerBookItem { template })),
                _ => None,
            };
            if s.kind != 0 { clicks.0 = None; }
            if let Some(command) = command { net.send(&command); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn server_slot_kinds_and_equipment_essences_stay_separate() {
        assert_eq!(slot_leaf("Mountslot"), Some(8)); assert_eq!(slot_leaf("TAB"), Some(7));
        assert_eq!(slot_leaf("LeftPotionTop"), Some(13)); assert_eq!(slot_leaf("Ammo"), Some(17));
        let mut state = InventoryState::default();
        state.items.insert(5, ItemInfo { id:5, template:"potion".into(), third:12, ..default() });
        state.allocation.placements.push((5,0));
        assert_eq!(item(&Slot { kind:1, id:Some("potion".into()) }, &state).unwrap().id, 5);
        state.allocation.placements.clear(); state.allocation.bank.push((5,0));
        assert!(item(&Slot { kind:1, id:Some("5".into()) }, &state).is_none());
        state.allocation.slots = vec![(5,vec![34])];
        assert!(essence(&state, PRIMARY_ESSENCE).is_none()); assert_eq!(essence(&state, BACKUP_ESSENCE).unwrap().third,12);
    }
}
