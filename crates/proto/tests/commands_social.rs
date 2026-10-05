//! Social / PvP commands: bytes produced by the experimental server's own encoders
//! (dsor/groups.py, dsor/pvp.py at multiplayer-2018 HEAD b842ec6, with the timestamp
//! and the notification guid pinned) and client payloads captured in session-merged*.

use dsor_proto::commands::{decode_message, encode_chain, encode_single, ClientCommand, ServerCommand};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// Decode one server message, require exactly one command, and re-encode it.
fn one(h: &str, bits: usize) -> (ServerCommand, Option<u32>) {
    let data = hex(h);
    let mut got = decode_message(&data, bits).unwrap();
    assert_eq!(got.len(), 1);
    let (cmd, actor) = got.remove(0);
    assert!(!cmd.is_unknown(), "{cmd:?}");
    let (bytes, n) = if data[0] == 0x84 {
        encode_single(&cmd, actor.unwrap_or(0))
    } else {
        encode_chain(&[(cmd.clone(), actor.unwrap_or(0))])
    };
    assert_eq!((bytes, n), (data, bits));
    (cmd, actor)
}

#[test]
fn group_status_with_two_members() {
    // groups.encode_group_status(5, 7, True, [simo (actor 0x10086), DSOReconstruction offline])
    let (cmd, actor) = one("84a000000500000007000000000000008100000000000000000000000380000043000080020039b4b6b78900309818181b2fb3b934b6b337b9322fb43ab10106000000000000000000709801918191b1698981698199018991d18181d181800000000000000000000000000048000000000000008802229a7a932b1b7b739ba393ab1ba34b7b700000001800000000000000000509801918191b1698981698199018991d18181d181800008000000000000000", 1426);
    assert_eq!(actor, None, "GroupStatus carries no actor");
    let ServerCommand::GroupStatus(s) = cmd else { panic!() };
    assert_eq!((s.group_id, s.leader_id, s.recipient_leads), (5, 7, true));
    assert_eq!(s.members.len(), 2);
    let m = &s.members[0];
    assert_eq!((m.player_id, m.actor, m.name.as_str(), m.map_name.as_str()), (7, 0x10086, "simo", "a0006_grimford_hub"));
    assert_eq!((m.character_class, m.level, m.online, m.slot), (2, 12, true, 0));
    assert_eq!(m.timestamp, "2026-10-03 12:00:00");
    assert_eq!((s.members[1].name.as_str(), s.members[1].online, s.members[1].slot), ("DSOReconstruction", false, 1));
}

#[test]
fn group_status_no_group() {
    let (cmd, _) = one("84a00000000000000000000000000000000000000000000000", 194);
    let ServerCommand::GroupStatus(s) = cmd else { panic!() };
    assert!(s.members.is_empty());
    assert_eq!(s.group_id, 0);
}

#[test]
fn invitation_response() {
    let (cmd, actor) = one("84a500030000000000000000040073696d6f86000100", 176);
    assert_eq!(actor, Some(0x10086));
    let ServerCommand::GroupInvitationResponse(r) = cmd else { panic!() };
    assert_eq!((r.code, r.a, r.b, r.name.as_str()), (3, 0, 0, "simo"));
}

#[test]
fn invite_notification() {
    let (cmd, actor) = one("8491000101000110000102030405060708090a0b0c0d0e0f100000000005000000070000008a007b22706c617965724e616d65223a2273696d6f222c22706c617965724c6576656c223a31322c22706c61796572436c617373223a322c22686f6e6f72506f696e7473223a302c2267726f75704e616d65223a22222c22706c6179657247656e646572223a312c227469746c6554797065223a302c22616368696576656d656e745469746c65223a22227d1300323032362d31302d30332031323a30303a303087000100", 1616);
    assert_eq!(actor, Some(0x10087));
    let ServerCommand::Notification(n) = cmd else { panic!() };
    assert_eq!((n.kind, n.entries.len()), (1, 1));
    let e = &n.entries[0];
    assert_eq!((e.note_type, e.guid.len(), e.group_id, e.inviter_id), (1, 16, 5, 7));
    assert!(e.params.starts_with("{\"playerName\":\"simo\""));
}

#[test]
fn kick_notice() {
    let (cmd, actor) = one("84a800040073696d6f000000000000000087000100", 168);
    assert_eq!(actor, Some(0x10087));
    let ServerCommand::GroupKick(k) = cmd else { panic!() };
    assert_eq!(k.name, "simo");
}

#[test]
fn pvp_flag_and_faction() {
    let (cmd, actor) = one("85cc00a0710080430000807f80", 97);
    assert_eq!(actor, Some(0x10086));
    assert_eq!(cmd, ServerCommand::PvpFlag(dsor_proto::commands::social::PvpFlag { flag: true, tick: 123456 }));
    let (cmd, _) = one("858300860001008600010000a18000403fc0", 138);
    let ServerCommand::FactionInfo(f) = cmd else { panic!() };
    assert_eq!((f.faction_id, f.owner, f.kind, f.pvp, f.unknown_bool_0), (0x10086, 0x10086, 0, true, false));
}

/// Captured client payloads re-encode bit for bit.
#[test]
fn captured_client_social_commands() {
    for (h, bits, id) in [
        ("8ba300110044534f5265636f6e737472756374696f6e040073696d6f00000000", 256, 163),
        ("8ba300040073696d6f110044534f5265636f6e737472756374696f6e00000000", 256, 163),
        ("8bda00000000000b00426573744f66335f31763100", 168, 218),
        ("8bb10000000000000007007173717364716500", 145, 177),
        ("8bb100000000000000060071736471736400", 137, 177),
        ("8baf000000000000000000000000000000000000ff", 168, 175),
        ("8bb9000000000000000000", 81, 185),
        ("8bbf000000000000100000000000000000000000000000000000", 208, 191),
    ] {
        let data = hex(h);
        let cmd = ClientCommand::decode(&data, bits).unwrap();
        assert_eq!(cmd.id(), id);
        assert!(!matches!(cmd, ClientCommand::Unknown { .. }));
        assert_eq!(cmd.encode(), (data, bits), "{cmd:?}");
    }
    let ClientCommand::GroupInvite(g) = ClientCommand::decode(&hex("8ba300110044534f5265636f6e737472756374696f6e040073696d6f00000000"), 256).unwrap() else { panic!() };
    assert_eq!((g.inviter_name.as_str(), g.invitee_name.as_str()), ("DSOReconstruction", "simo"));
    let ClientCommand::GuildFoundation(f) = ClientCommand::decode(&hex("8bb100000000000000060071736471736400"), 137).unwrap() else { panic!() };
    assert_eq!(f.name, "qsdqsd");
}

/// Client commands with no capture, built as the server's decoders read them.
#[test]
fn client_group_commands_round_trip() {
    use dsor_proto::commands::social::*;
    let cmds = [
        ClientCommand::GroupList(GroupList { kind: 2, unknown_u32_0: 0, unknown_u32_1: 0, target: 9 }),
        ClientCommand::GroupInvitationResponse(GroupInvitationResponse { code: 0, a: 7, b: 5, name: "simo".into() }),
        ClientCommand::GroupKick(GroupKick { name: "simo".into(), unknown_u32_0: 0, unknown_u32_1: 0 }),
        ClientCommand::PvpFlag(PvpFlag { flag: true, tick: 42 }),
    ];
    for c in cmds {
        let (bytes, bits) = c.encode();
        assert_eq!(ClientCommand::decode(&bytes, bits).unwrap(), c);
    }
    // GroupList as the server's decode_group_list reads it: u8 type, three u32.
    let (bytes, bits) = ClientCommand::GroupList(GroupList { kind: 2, unknown_u32_0: 1, unknown_u32_1: 2, target: 9 }).encode();
    assert_eq!((bytes, bits), (hex("8b9f0002010000000200000009000000"), 128));
}
