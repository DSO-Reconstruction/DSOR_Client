//! Equipment snapshots shared by local inventory and remote presence packets.
use crate::{actors::character::ItemSkins, gameplay::inventory::InventoryState};
pub use dsor_proto::commands::player::WornSlot as Equipment;

/// Only slots 0..7 create geometry. Presence may put several semicolon-separated
/// parts in one string, and sends the same two-handed skin for both hand slots.
pub fn normalize(mut equipment: Vec<Equipment>) -> Vec<Equipment> {
    equipment.retain(|e| e.slot <= 7);
    for e in &mut equipment {
        e.skin_parts = e.skin_parts.iter().flat_map(|s| s.split(';'))
            .map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect();
        e.skin_parts.sort();
        e.skin_parts.dedup();
        e.colour = e.colour.filter(|c| c.iter().all(|v| v.is_finite()))
            .map(|c| c.map(|v| v.clamp(0.0, 1.0)));
    }
    equipment.sort_by_key(|e| e.slot);
    equipment
}

pub fn local(inventory: &InventoryState, table: &ItemSkins) -> Option<(Vec<Equipment>, i8)> {
    if !inventory.ready { return None; }
    let mut equipment = Vec::new();
    for (id, slots) in &inventory.allocation.slots {
        if !slots.iter().any(|s| (0..=7).contains(s)) { continue; }
        // Wait for the item record when allocation and contents arrive separately.
        let item = inventory.items.get(id)?;
        let appearance = if item.fourth_name.is_empty() { &item.appearance } else { &item.fourth_name };
        let parts = table.appearances.get(appearance).or_else(|| table.items.get(&item.template));
        let colour = (item.twentieth != 0).then(|| [16, 8, 0, 24].map(|shift| ((item.twentieth >> shift) & 255) as f32 / 255.0));
        for &slot in slots.iter().filter(|s| (0..=7).contains(*s)) {
            equipment.push(Equipment { slot: slot as u8, skin_parts: parts.cloned().unwrap_or_default(), colour });
        }
    }
    let allocations = &inventory.allocation.slots;
    let right = allocations.iter().any(|(_, slots)| slots.contains(&5));
    let left = allocations.iter().any(|(_, slots)| slots.contains(&6));
    let two = allocations.iter().any(|(_, slots)| slots.contains(&5) && slots.contains(&6));
    Some((normalize(equipment), hand_state(right, left, two)))
}

fn hand_state(right: bool, left: bool, two: bool) -> i8 {
    if two { 5 } else if right && left { 4 } else if right { 3 } else { 0 }
}

/// release261's byte historically called `armament` in the remote codec is a
/// player-kind enum (normally 5). Use the advertised animation set and hands.
pub fn remote_armament(equipment: &[Equipment], animation_set: &str) -> i8 {
    // The Night Walker's sets (niwalk_off...) hold its daggers in both hands.
    if animation_set.ends_with("2h_weapon") || animation_set.ends_with("large_weapon") || animation_set.starts_with("niwalk") { return 5; }
    if animation_set.ends_with("1h_weapon_shield") { return 4; }
    if animation_set.ends_with("1h_weapon") || animation_set.ends_with("small_weapon") { return 3; }
    let right = equipment.iter().find(|e| e.slot == 5);
    let left = equipment.iter().find(|e| e.slot == 6);
    let two = right.zip(left).is_some_and(|(r, l)| !r.skin_parts.is_empty() && r.skin_parts == l.skin_parts);
    hand_state(right.is_some(), left.is_some(), two)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsor_proto::commands::inventory::{InventoryInfo, ItemInfo};

    #[test]
    fn inventory_uses_transmog_then_appearance_then_template_and_keeps_dye() {
        let table: ItemSkins = serde_json::from_str(r#"{"items":{"staff":["original"]},"appearances":{"roll":["rolled"],"skin":["transmog"]}}"#).unwrap();
        let mut inv = InventoryState::default();
        inv.apply(InventoryInfo { items: vec![ItemInfo { id: 10, template: "staff".into(), appearance: "roll".into(),
            fourth_name: "skin".into(), twentieth: 0xD1050505, ..Default::default() }], slots: vec![(10, vec![5, 6])], ..Default::default() });
        let (parts, hands) = local(&inv, &table).unwrap();
        assert_eq!(hands, 5);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].skin_parts, ["transmog"]);
        assert_eq!(parts[0].colour, Some([5.0/255.0, 5.0/255.0, 5.0/255.0, 209.0/255.0]));
        inv.items.get_mut(&10).unwrap().fourth_name.clear();
        assert_eq!(local(&inv, &table).unwrap().0[0].skin_parts, ["rolled"]);
        inv.items.get_mut(&10).unwrap().appearance = "unknown".into();
        assert_eq!(local(&inv, &table).unwrap().0[0].skin_parts, ["original"]);
        inv.apply(InventoryInfo { slots: vec![(10, vec![5, 6])], ..Default::default() });
        assert_eq!(local(&inv, &table).unwrap().1, 5, "allocation-only final packet keeps item records");
        inv.apply(InventoryInfo::default());
        assert_eq!(local(&inv, &table), Some((vec![], 0)), "unequip removes geometry and pose");
    }

    #[test]
    fn legacy_tables_and_delayed_item_records_are_supported() {
        let table: ItemSkins = serde_json::from_str(r#"{"helmet":["helm"]}"#).unwrap();
        let mut inv = InventoryState::default();
        inv.apply(InventoryInfo { slots: vec![(1, vec![0])], ..Default::default() });
        assert!(local(&inv, &table).is_none());
        inv.items.insert(1, ItemInfo { id: 1, template: "helmet".into(), ..Default::default() });
        assert_eq!(local(&inv, &table).unwrap().0[0].skin_parts, ["helm"]);
    }

    #[test]
    fn remote_parts_and_pose_follow_server_graphics_and_animation_set() {
        let mut equipment = normalize(vec![Equipment { slot: 5, skin_parts: vec!["staff; staff_fx ;staff".into()], colour: Some([1.0, 0.0, 0.0, 1.0]) },
            Equipment { slot: 6, skin_parts: vec!["staff_fx;staff".into()], colour: None },
            Equipment { slot: 10, skin_parts: vec!["not_geometry".into()], colour: None }]);
        assert_eq!(equipment.len(), 2);
        assert_eq!(equipment[0].skin_parts, ["staff", "staff_fx"]);
        assert_eq!(remote_armament(&equipment, ""), 5);
        equipment.pop();
        assert_eq!(remote_armament(&equipment, ""), 3);
        assert_eq!(remote_armament(&equipment, "ranger_small_weapon"), 3);
        assert_eq!(remote_armament(&equipment, "ranger_female_large_weapon"), 5);
        assert_eq!(remote_armament(&equipment, "mage_female_1h_weapon_shield"), 4);
        assert_eq!(remote_armament(&[], "male"), 0);
    }
}
