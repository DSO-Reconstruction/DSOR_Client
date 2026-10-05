//! World-entity and quest commands against real captured bytes (session-walk4 /
//! session-combat1, the experimental server's DSOR_DATAGRAM_LOG).

use dsor_proto::commands::{decode_message, encode_chain, ClientCommand, ServerCommand};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// Decode a captured server message, check it walks to its end, re-encode it exactly.
fn walk(h: &str, bits: usize) -> Vec<(ServerCommand, Option<u32>)> {
    let data = hex(h);
    let got = decode_message(&data, bits).expect("decodes");
    let again: Vec<_> = got.iter().map(|(c, a)| (c.clone(), a.unwrap_or(0))).collect();
    let (bytes, n) = encode_chain(&again);
    assert_eq!(n, bits);
    assert_eq!(bytes, data);
    got
}

#[test]
fn new_monster_from_walk4() {
    // session-walk4 frame 4637, 1267 bits.
    let got = walk("853000200061303330325f6e6f726d616c5f735f67656e5f62656173746d616e5f737461670b0000000200002e43080000804209000090420f00c002441100c002441200c002441300c002441400c002441500c0024417000000001900000040ae0000001100000000000000008045430000000000802d430000000000000001001b02010002000000003fffffffe0e00dadedce6e8cae4000036040201fe0", 1267);
    let ServerCommand::NewMonster(m) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!(m.blueprint, "a0302_normal_s_gen_beastman_stag");
    assert_eq!(m.attributes.len(), 11);
    assert_eq!(m.attributes[0], (2, 174.0));
    assert_eq!(m.health, 174);
    assert_eq!(m.level, 17);
    assert_eq!(m.faction.faction_id, 0x0001_0000);
    assert_eq!(m.faction.faction_type, 2);
    assert_eq!(m.unknown_u32_1, 0xFFFF_FFFF);
    assert_eq!(m.rank, "monster");
    assert_eq!(got[0].1, Some(0x0001_021B));
}

#[test]
fn new_npc_and_npc_info_from_walk4() {
    // frame 143, 587 bits: NewNPC.
    let got = walk("852c00140066343130305f6d65726368616e745f6172656e61100074ee71872a424bfab8f7753a08f1fa7100000000000000000000000000363810d0c1b90fd0774d7f10a00020201fe0", 587);
    let ServerCommand::NewNpc(n) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!(n.name, "f4100_merchant_arena");
    assert_eq!(n.guid.len(), 16);
    assert!(n.visible);
    // frame 146, 1273 bits: NPCInfo with two quest options.
    let got = walk("852e00140066343130305f6d65726368616e745f6172656e61020000001c007076702d66343130302d70617274696369706174696f6e2d30312d6e001c007076702d66343130302d70617274696369706174696f6e2d30312d6e0916007076702d66343130302d766963746f72792d30312d6e0016007076702d66343130302d766963746f72792d30312d6e0900000000000000000000000080000080807f80", 1273);
    let ServerCommand::NpcInfo(i) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!(i.interactions.len(), 2);
    assert_eq!(i.interactions[0].name, "pvp-f4100-participation-01-n");
    assert_eq!(i.interactions[0].kind, 0);
    assert_eq!(i.interactions[0].state, 9);
    assert!(i.enabled);
}

#[test]
fn prop_info_from_combat1() {
    // session-combat1 frame 115, 2088 bits: three props.
    let got = walk("853b0003000000010000002000743030305f73746174655f6372616674696e675f776f726b62656e63685f3031100013c32234698e45e9aecc3076549cdf39ffffffff0100000000fb5a0e43000060417082d84129613f3f0000000000ff020000002000743030305f73746174655f6372616674696e675f776f726b62656e63685f3031100033aac0e026ad4e7bb457a253d9b0efb8ffffffff0100000000ee1cff42000000410b66bd42784a883f0000000000ff030000001400655f6368725f6269675f70726573656e745f30311000cfc1311de5934a2187d7f0bfd97ecc7effffffff0100000000fe258b4200002041dcbc7dbf591f7e3e0000000000ff15000100ff", 2088);
    let ServerCommand::PropInfo(p) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!(p.entries.len(), 3);
    assert_eq!(p.entries[2].template, "e_chr_big_present_01");
    assert_eq!(p.entries[0].team, -1);
    assert_eq!(p.entries[0].state, 1);
    assert_eq!(p.entries[0].quest_type, -1);
    assert_eq!(got[0].1, Some(0x0001_0015));
}

#[test]
fn client_requests_reencode_exactly() {
    for (h, bits) in [
        ("8b2f0013010100", 56), // NPCRequest, session-combat1
        ("8b6e0013010100", 56), // Encounter
        ("8b6c003e020100", 56), // PickupItem
    ] {
        let data = hex(h);
        let c = ClientCommand::decode(&data, bits).unwrap();
        assert!(!matches!(c, ClientCommand::Unknown { .. }), "{c:?}");
        assert_eq!(c.encode(), (data, bits));
    }
    let q = ClientCommand::Quest(dsor_proto::commands::actors::Quest {
        quest_id: "a0001-start-tutorial-01-n".into(),
        state: 1,
        parameter: 0,
    });
    let (bytes, bits) = q.encode();
    assert_eq!(ClientCommand::decode(&bytes, bits).unwrap(), q);
}
