//! Collector's Bag stickers dragged onto the quick bar: a mount onto the mount
//! slot (O), a companion onto its slot (H) or a skill slot. The slot becomes a
//! sticker shortcut (QuickSlot kind 2, the owned item's resource id), which
//! hotbar::input uses with UseStickerBookItem.
//! EVIDENCE: DSOR_Server services/customization.py writes Shadow Souls as
//!   QuickSlotInfo(2, resource_id) in slot 9; inventory_interactions.py
//!   `_handle_quick_slots` keeps slots 8.. on every bar.
//! FAILURE (2026-10-24): stickers could not be dragged out of the bag at all
//!   ("in the original game you can drag the icons to put them in the O slot").
use super::*;
use dsor_proto::commands::{inventory::{QuickSlot as Slot, QuickSlots}, ClientCommand};

const BAR_SIZE: usize = 17;
/// Skill slots (0..8) and the collection slots (8..13: mount, companion...);
/// never the four consumable slots.
const STICKER_SLOTS: usize = 13;
/// The four consumable slots (U, G, R, T): potions, elixirs.
pub(super) const CONSUMABLE_SLOTS: std::ops::Range<usize> = 13..17;
/// The skill slots (mouse buttons, 1-5, the last two): skills, stickers and the
/// Universal Map.
pub(super) const SKILL_SLOTS: usize = 8;

pub(super) struct StickerDrag {
    id: String,
    origin: Vec2,
    active: bool,
    /// Picked up from this quick slot (a sticker or item shortcut already on the
    /// bar) rather than from the Collector's Bag, on this bar.
    from: Option<usize>,
    bar: usize,
}

impl StickerDrag {
    /// The bar and slot a shortcut is being carried from, once the drag is under
    /// way: that slot shows empty until it is let go (crate::ui update_slots), as
    /// a skill's does (spells::DragSpell::lifted).
    /// FAILURE (2026-10-24): the slot kept its icon and the cursor carried none
    ///   ("when I drag, there's no icons and it still remains there").
    pub(super) fn lifted(&self) -> Option<(usize, usize)> {
        if !self.active { return None; }
        Some((self.bar, self.from?))
    }
}

/// A press on an owned sticker that moves 4 px picks it up; released over a quick
/// slot it lands there. A press on a quick slot does the same with what the slot
/// holds: released on another slot it moves there, off the bar it leaves the bar
/// (apply_drop acts only on sticker and item shortcuts; skills have
/// spells::drag). Returns true when the release belongs to a drag, so it does
/// not also preview the sticker or use the slot.
/// FAILURE (2026-10-24): a pet put on the bar could not be taken off again.
pub(super) fn drag(pointer: &mut Pointer, windows: &Windows, views: &systems::Views, mouse: &ButtonInput<MouseButton>,
    hit: Option<(Entity, usize)>, over: Option<Entity>, point: Option<Vec2>, allowed: bool) -> bool {
    let action = |(root, button): (Entity, usize)| windows.get(root).ok().map(|(_, name, c, _, _)| (name.0.clone(), c.buttons[button].action.clone()));
    let bar = windows.iter().find(|(_, n, _, v, _)| n.0 == "bottombar" && **v != Visibility::Hidden);
    let unlocked = bar.is_some_and(|(_, _, c, _, _)| !c.locked);
    // A skill's slot belongs to spells::drag (its swap would be undone here).
    let skill_at = |slot: usize| bar.is_some_and(|(_, _, c, _, _)| c.spells.bar.get(slot).is_some_and(|s| s.is_some()));
    if mouse.just_pressed(MouseButton::Left) {
        pointer.sticker_drag = None;
        match (allowed, point, hit.and_then(action)) {
            (true, Some(origin), Some((_, Action::System(systems::Request::Pick(id, _))))) => {
                if views.album.sticker(&id).is_some_and(|s| s.owned) {
                    pointer.sticker_drag = Some(StickerDrag { id, origin, active: false, from: None, bar: 0 });
                }
            }
            (true, Some(origin), Some((name, Action::Skill(slot)))) if name == "bottombar" && unlocked && slot < CONSUMABLE_SLOTS.end && !skill_at(slot) => {
                let bar = bar.map_or(0, |(_, _, c, _, _)| c.spells.active);
                pointer.sticker_drag = Some(StickerDrag { id: String::new(), origin, active: false, from: Some(slot), bar });
            }
            _ => {}
        }
    }
    if let (Some(d), Some(p)) = (&mut pointer.sticker_drag, point) {
        if p.distance(d.origin) >= 4.0 { d.active = true; }
    }
    if mouse.just_released(MouseButton::Left) {
        if let Some(d) = pointer.sticker_drag.take().filter(|d| d.active) {
            let target = hit.and_then(action).and_then(|(n, a)| match a { Action::Skill(slot) if n == "bottombar" => Some(slot), _ => None });
            match d.from {
                None => if let Some(slot) = target.filter(|s| *s < STICKER_SLOTS) { pointer.sticker_drop = Some((2, d.id, slot)); },
                // Onto another slot: moved; off the bar window: removed.
                Some(from) => match target {
                    Some(to) if to != from => pointer.slot_move = Some((from, Some(to))),
                    None if point.is_some() && bar.is_some_and(|(root, _, _, _, _)| Some(root) != over) => pointer.slot_move = Some((from, None)),
                    _ => {}
                },
            }
            pointer.down = None;
            // A drag under way is never also a click, even let go on its own slot.
            // FAILURE (2026-10-24): "the map is used every time I put it back in the
            //   same slot".
            return true;
        }
    }
    if !mouse.pressed(MouseButton::Left) { pointer.sticker_drag = None; }
    false
}

/// The dragged shortcut's icon, for the cursor overlay: the sticker's, or what
/// the quick slot it was lifted from shows.
pub(super) fn dragged(pointer: &Pointer, views: &systems::Views, net: Option<&crate::network::net::Net>, catalog: Option<&items::Catalog>) -> Option<String> {
    let d = pointer.sticker_drag.as_ref().filter(|d| d.active)?;
    match d.from {
        None => views.album.sticker(&d.id).map(|s| s.icon.image.clone()).filter(|i| !i.is_empty()),
        Some(slot) => hotbar::appearance(slot, d.bar, net, &Figures::default(), catalog, &crate::ui::SkillIcons(Default::default())).map(|(path, _)| path),
    }
}

/// The active bar with `slot` holding the shortcut (`kind` 2 a sticker, on a
/// skill or collection slot; 1 an item, on a skill or consumable slot, which
/// one its caller decides); the same one leaves any other slot of the bar. None
/// when nothing changes.
fn edited_bar(slots: &[Slot], kind: i32, slot: usize, key: &str) -> Option<Vec<Slot>> {
    let fits = if kind == 2 { slot < STICKER_SLOTS } else { slot < SKILL_SLOTS || CONSUMABLE_SLOTS.contains(&slot) };
    if slots.len() != BAR_SIZE || !fits { return None; }
    let mut edited = slots.to_vec();
    for s in &mut edited {
        if s.kind == kind && s.id.as_deref() == Some(key) { *s = Slot { kind: Slot::EMPTY, id: None }; }
    }
    edited[slot] = Slot { kind, id: Some(key.to_owned()) };
    (edited != slots).then_some(edited)
}

/// A sticker or item shortcut moved from `from` to `to` (swapping with what is
/// there; an item stays in its group, skill slots or consumable slots) or, with
/// `to` None, taken off the bar. None for a skill, an empty slot or no change.
fn moved_bar(slots: &[Slot], from: usize, to: Option<usize>) -> Option<Vec<Slot>> {
    if slots.len() != BAR_SIZE || !matches!(slots.get(from).map(|s| s.kind), Some(1 | 2)) { return None; }
    let mut edited = slots.to_vec();
    match to {
        None => edited[from] = Slot { kind: Slot::EMPTY, id: None },
        Some(to) => {
            let fits = match slots[from].kind {
                2 => to < STICKER_SLOTS,
                _ => if from < SKILL_SLOTS { to < SKILL_SLOTS } else { CONSUMABLE_SLOTS.contains(&to) },
            };
            if !fits || to >= BAR_SIZE { return None; }
            edited.swap(from, to);
        }
    }
    (edited != slots).then_some(edited)
}

pub(in crate::ui) fn apply_drop(mut pointer: ResMut<Pointer>, views: Res<systems::Views>, hotbar: Res<hotbar::Hotbar>,
    net: Option<NonSendMut<crate::network::net::Net>>) {
    let Some(mut net) = net else { return };
    if let Some((from, to)) = pointer.slot_move.take() {
        let bar = hotbar.active;
        if let Some(slots) = net.quickbars.bars.get(bar).and_then(|b| moved_bar(b, from, to)) {
            for slot in [Some(from), to].into_iter().flatten().filter(|s| *s >= 8) {
                for b in net.quickbars.bars.iter_mut().filter(|b| b.len() == BAR_SIZE) { b[slot] = slots[slot].clone(); }
            }
            net.bar = slots.iter().map(|s| if s.kind == 0 { s.id.clone() } else { None }).collect();
            net.quickbars.bars[bar] = slots.clone();
            info!("quick slot {from} -> {to:?}");
            net.send(&ClientCommand::QuickSlots(QuickSlots { slots, bar: bar as u32 }));
        }
    }
    let Some((kind, id, slot)) = pointer.sticker_drop.take() else { return };
    let key = if kind == 2 {
        let Some(sticker) = views.album.sticker(&id).filter(|s| s.owned) else { return };
        sticker.server_key().to_owned()
    } else { id.clone() };
    let bar = hotbar.active;
    let Some(slots) = net.quickbars.bars.get(bar).and_then(|b| edited_bar(b, kind, slot, &key)) else { return };
    // The collection slots are the same on every bar (the server copies them).
    if slot >= 8 {
        for b in net.quickbars.bars.iter_mut().filter(|b| b.len() == BAR_SIZE) { b[slot] = slots[slot].clone(); }
    }
    net.bar = slots.iter().map(|s| if s.kind == 0 { s.id.clone() } else { None }).collect();
    net.quickbars.bars[bar] = slots.clone();
    info!("shortcut {id} ({key}, kind {kind}) on quick slot {slot}");
    net.send(&ClientCommand::QuickSlots(QuickSlots { slots, bar: bar as u32 }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sticker_takes_its_slot_once_and_never_a_potion_slot() {
        let empty = Slot { kind: Slot::EMPTY, id: None };
        let mut bar = vec![empty.clone(); BAR_SIZE];
        bar[0] = Slot { kind: 0, id: Some("angrystrike".into()) };
        bar[3] = Slot { kind: 2, id: Some("mount_x".into()) };
        let edited = edited_bar(&bar, 2, 8, "mount_x").unwrap();
        assert_eq!(edited[8], Slot { kind: 2, id: Some("mount_x".into()) });
        assert_eq!(edited[3], empty, "moved, not copied");
        assert_eq!(edited[0], bar[0]);
        assert!(edited_bar(&edited, 2, 8, "mount_x").is_none(), "already there");
        assert!(edited_bar(&bar, 2, 13, "mount_x").is_none(), "consumable slot");
        // An item only on the consumable slots.
        assert_eq!(edited_bar(&bar, 1, 14, "misc_travelmap").unwrap()[14], Slot { kind: 1, id: Some("misc_travelmap".into()) });
        assert_eq!(edited_bar(&bar, 1, 2, "misc_travelmap").unwrap()[2], Slot { kind: 1, id: Some("misc_travelmap".into()) }, "the map on 2");
        assert!(edited_bar(&bar, 1, 9, "misc_travelmap").is_none(), "companion slot");
        // Taken off, or moved within its group.
        assert_eq!(moved_bar(&bar, 3, None).unwrap()[3], empty);
        assert_eq!(moved_bar(&bar, 3, Some(8)).unwrap()[8], bar[3]);
        assert!(moved_bar(&bar, 0, None).is_none(), "a skill: spells::drag's");
        let mut map = bar.clone();
        map[2] = Slot { kind: 1, id: Some("misc_travelmap".into()) };
        assert!(moved_bar(&map, 2, Some(14)).is_none(), "the map stays on the skill slots");
        assert_eq!(moved_bar(&map, 2, Some(4)).unwrap()[4], map[2]);
    }
}
