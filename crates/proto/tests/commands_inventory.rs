//! Inventory, quick bar, skill book, shop, talent and OCP codecs against bytes the
//! experimental server's own encoders produced (dsor/shop.py, talents.py,
//! skillbook.py, actionbar.py, inventory.py, trading.py) and real captured client
//! payloads.

use dsor_proto::commands::inventory::{ItemInfo, QuickSlot};
use dsor_proto::commands::{decode_message, encode_chain, encode_single, ClientCommand, ServerCommand};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// Decode a server message, check it re-encodes to the same bytes, return its commands.
fn server(h: &str) -> Vec<(ServerCommand, Option<u32>)> {
    let data = hex(h);
    let got = decode_message(&data, data.len() * 8).unwrap();
    let with: Vec<(ServerCommand, u32)> = got.iter().map(|(c, a)| (c.clone(), a.unwrap_or(0))).collect();
    let (bytes, _) = if data[0] == 0x84 { encode_single(&with[0].0, with[0].1) } else { encode_chain(&with) };
    assert_eq!(bytes, data, "re-encoding changed the message");
    got
}

/// Decode a captured client message and check it re-encodes bit for bit.
fn client(bits: usize, h: &str) -> ClientCommand {
    let data = hex(h);
    let got = ClientCommand::decode(&data, bits).unwrap();
    assert_eq!(got.encode(), (data, bits));
    got
}

#[test]
fn ocp_offer_response() {
    // shop.encode_offer_response(OfferResponse([Offer(7, 1.5, "a", "bc", "", "2026-01-02 03:04:05")], 0x10015))
    let got = server("84130101000000070000000000c03f0100610200626300001300323032362d30312d30322030333a30343a303515000100");
    let ServerCommand::OcpOfferResponse(r) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!(got[0].1, Some(0x10015));
    assert_eq!(r.offers[0].unknown_i32_0, 7);
    assert_eq!(r.offers[0].unknown_f32_1, 1.5);
    assert_eq!(r.offers[0].unknown_string_3, "bc");
    assert_eq!(r.offers[0].expires, "2026-01-02 03:04:05");
}

#[test]
fn ocp_methods() {
    let got = server("8411010100000002007070010078000000400000803e15000100");
    let ServerCommand::OcpMethods(m) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!(m.methods[0].unknown_string_0, "pp");
    assert_eq!(m.methods[0].unknown_f32_3, 0.25);
}

#[test]
fn talent_selection_info() {
    let got = server("852201030102010500745f6f6e65010200743215000100ff");
    let ServerCommand::TalentSelectionInfo(t) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!((t.context, t.preset), (3, 1));
    assert_eq!(t.talents, vec![(1, "t_one".to_string()), (1, "t2".to_string())]);
}

#[test]
fn skill_book_info() {
    // skillbook.encode_book([(5, True), (9, False)], 0x10015): 142 bits, 2 padding.
    let data = hex("855b000205000000812000000054000403fc");
    let got = decode_message(&data, 142).unwrap();
    let ServerCommand::SkillBookInfo(b) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!(b.entries.len(), 2);
    assert_eq!((b.entries[0].skill, b.entries[0].owned), (5, true));
    assert_eq!((b.entries[1].skill, b.entries[1].owned), (9, false));
    assert_eq!(got[0].1, Some(0x10015));
    let (bytes, bits) = encode_chain(&[(got[0].0.clone(), 0x10015)]);
    assert_eq!((bytes, bits), (data, 142));
}

#[test]
fn quick_slots_info() {
    let got = server("8553000200000002000000000000000b00616e677279737472696b65ffffffff01000000ffffffff15000100ff");
    let ServerCommand::QuickSlotsInfo(q) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!(q.bars.len(), 2);
    assert_eq!(q.bars[0][0], QuickSlot { kind: 0, id: Some("angrystrike".into()) });
    assert_eq!(q.bars[0][1], QuickSlot { kind: -1, id: None });
}

fn sample_item(item: &ItemInfo) {
    assert_eq!(item.id, 0x20001);
    assert_eq!(item.template, "sword_x");
    assert_eq!(item.kind, 2);
    assert_eq!(item.level, 7);
    assert_eq!(item.durability, 55);
    assert_eq!(item.appearance, "app");
    assert_eq!(item.tier, 3);
    assert_eq!(item.position, [1.0, 2.0, 3.0]);
    assert_eq!(item.statistics[0].name, "Strength");
    assert_eq!(item.statistics[0].value, 4.5);
    assert_eq!(item.statistics[0].kind, 1);
    assert_eq!(item.names, vec!["n1".to_string()]);
    assert_eq!(item.stamped, [2026, 10, 5, 1, 2, 3]);
    assert!(item.flag_0 && !item.flag_1);
}

#[test]
fn inventory_info() {
    // inventory.info_command(bag(30, 100.0, 50.0) + one item, equipment slots [3, -1],
    // and a bag placement), 0x10015.
    let data = hex("855700010000000100020000000000010000000201000000000000000000000000000004006e6f6e653700000007000000000000000000000081c01cdddbdc9917de00000000200fc00000100000101000c0185c1c00c000000000000000000040002410004080000000c00000020014dd1c995b99dd1a0040801b8c403a81c0000280000001400000004000000080000000c0000000400000004000800080000000ffc040000000800080010000000000000000000000000000000000000000000000000000000000000000000000078000000000000000000000000000000000321080001210a2a000201fe0");
    let got = decode_message(&data, data.len() * 8).unwrap();
    let ServerCommand::InventoryInfo(inv) = &got[0].0 else { panic!("{got:?}") };
    sample_item(&inv.items[0]);
    assert_eq!(inv.slots, vec![(0x20001, vec![3, -1])]);
    assert_eq!(inv.placements, vec![(0x20002, 4)]);
    assert!(inv.overflow.is_empty());
    assert_eq!(inv.storage_sizes[2], 30);
    assert_eq!((inv.health, inv.resource), (100.0, 50.0));
    let (bytes, _) = encode_chain(&[(got[0].0.clone(), 0x10015)]);
    assert_eq!(bytes, data);
}

#[test]
fn offer_buyback() {
    // trading.encode_buyback("shopx", [item], 0x10015, True)
    let data = hex("855d00050073686f7078c00000000000000000000000004000000040008000000000004000000080400000000000000000000000000001001b9bdb994dc0000001c0000000000000000000002070073776f72645f7800000000803f000000400000404003006170700300000000000000000001000090400102000000030000000800537472656e6774680102006e3100ea0700000a0000000500000001000000020000000300000015000100ff0");
    let got = decode_message(&data, data.len() * 8).unwrap();
    let ServerCommand::Offer(o) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!(o.shop, "shopx");
    assert!(o.keep_shop && o.can_sell);
    assert!(o.offers.is_empty() && o.categories.is_empty());
    sample_item(&o.items[0]);
    let (bytes, _) = encode_chain(&[(got[0].0.clone(), 0x10015)]);
    assert_eq!(bytes, data);
}

#[test]
fn captured_inventory_commands() {
    // An equip (operation 1), then operations carrying one and two arguments.
    let ClientCommand::Inventory(c) = client(160, "8b5800e502010001000000000000000000000000") else { panic!() };
    assert_eq!((c.item, c.operation, c.arguments.len()), (0x102e5, 1, 0));
    let ClientCommand::Inventory(c) = client(192, "8b58006d0201000b01000000760201000000000000000000") else { panic!() };
    assert_eq!((c.operation, c.arguments.clone()), (11, vec![0x10276]));
    let ClientCommand::Inventory(c) = client(224, "8b58009001010011020000008c010100ffffffff0000000000000000") else { panic!() };
    assert_eq!((c.operation, c.arguments.clone()), (17, vec![0x1018c, -1]));
}

#[test]
fn captured_shop_commands() {
    let ClientCommand::RequestOffer(r) = client(73, "8b5c000a808080000000") else { panic!() };
    assert!(!r.by_name && r.shop.is_empty());
    let ClientCommand::RequestOffer(r) = client(201, "8b5c0080000000080033b637b130b62fb931afba3930b232b900") else { panic!() };
    assert!(r.by_name);
    assert_eq!(r.shop, "global_rc_trader");
    let ClientCommand::Transaction(t) = client(152, "8b5e000900616c6c5f6974656d730497010100") else { panic!() };
    assert_eq!((t.shop.as_str(), t.kind, t.item), ("all_items", 4, 0x10197));
    let ClientCommand::Transaction(t) = client(248, "8b5e000900616c6c5f6974656d73010e00616c6c3a67656d5f6f6e79785f61") else { panic!() };
    assert_eq!((t.kind, t.offer.as_str()), (1, "all:gem_onyx_a"));
}

#[test]
fn captured_quick_slots() {
    let ClientCommand::QuickSlots(q) = client(2048, "8b5400120000000000000019006d6167655f6d616769636d697373696c655f64656661756c740000000015006d6167655f6669726562616c6c5f64656661756c740000000014006d6167655f69636562616c6c5f64656661756c740000000016006d6167655f66726f73746e6f76615f64656661756c740000000015006d6167655f74656c65706f72745f64656661756c74000000001c006d6167655f6c696768746e696e67737472696b655f64656661756c740000000016006d6167655f66726f737477696e645f64656661756c74ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff00000000") else { panic!() };
    assert_eq!(q.slots.len(), 18);
    assert_eq!(q.slots[0].id.as_deref(), Some("mage_magicmissile_default"));
    assert_eq!(q.slots[17], QuickSlot { kind: -1, id: None });
    assert_eq!(q.bar, 0);
}
