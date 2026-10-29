//! Groups, guilds and the player search against the release 261 server
//! (tools/social_vectors.py): the statuses and answers as protocol/social_protocol.py
//! writes them, and the client's requests as its decoders read them.
use dsor_proto::commands::player::PlayerQuery;
use dsor_proto::commands::social::{Foundation, GroupInvitationResponse, GuildInvite, Leave};
use dsor_proto::commands::{decode_message, encode_chain, ClientCommand, ServerCommand};
use serde_json::Value;

fn vectors() -> Vec<Value> {
    serde_json::from_str(include_str!("data/social_release261.json")).unwrap()
}

fn bytes(v: &Value) -> (Vec<u8>, usize) {
    let hex = v["hex"].as_str().unwrap();
    let bytes = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
    (bytes, v["bits"].as_u64().unwrap() as usize)
}

fn decoded(name: &str) -> Vec<(ServerCommand, Option<u32>)> {
    let v = vectors().into_iter().find(|v| v["name"] == name).unwrap();
    let (data, bits) = bytes(&v);
    let commands = decode_message(&data, bits).unwrap();
    assert!(commands.iter().all(|(c, _)| !c.is_unknown()), "{name}: {commands:?}");
    // Every layout writes back the server's bytes.
    let chain: Vec<_> = commands.iter().map(|(c, actor)| (c.clone(), actor.unwrap_or(0))).collect();
    let (encoded, encoded_bits) = encode_chain(&chain);
    assert_eq!((encoded, encoded_bits), (data, bits), "{name}");
    commands
}

#[test]
fn group_status_carries_the_members_as_the_server_writes_them() {
    let commands = decoded("group_status");
    let (ServerCommand::GroupStatus(s), actor) = &commands[0] else { panic!() };
    assert_eq!(*actor, None);
    assert!(s.grouped());
    assert_eq!((s.group_id, s.leader_id, s.recipient_id, s.listed), (7, 11, 11, true));
    assert_eq!((s.name.as_str(), s.private, s.description.as_str()), ("Leader's group", false, "desc"));
    let [leader, member] = s.members.as_slice() else { panic!("{:?}", s.members) };
    assert_eq!((leader.player_id, leader.actor, leader.name.as_str(), leader.map_name.as_str()), (11, 1, "Leader", "a0200_kingscity"));
    assert_eq!((leader.character_class, leader.level, leader.role, leader.relation, leader.unknown_u32_5), (1, 45, 0, 0xFF, 0xFFFFFFFF));
    assert!(leader.leads() && leader.online && leader.available && leader.invitable);
    assert_eq!((member.player_id, member.actor, member.role, member.online, member.timestamp.as_str()), (22, 0, 1, false, "2026-10-20 01:02:03"));
    assert!(!member.leads());
    let (ServerCommand::GroupStatus(none), _) = &decoded("no_group")[0] else { panic!() };
    assert!(!none.grouped());
    assert_eq!(none.recipient_id, 11);
}

#[test]
fn group_answers_and_failures_decode() {
    let (ServerCommand::GroupFoundationResponse(r), _) = &decoded("group_made")[0] else { panic!() };
    assert_eq!((r.status, r.tick, r.message.as_str()), (1, 5, "name taken"));
    let (ServerCommand::GroupInvitationResponse(r), actor) = &decoded("group_answer")[0] else { panic!() };
    assert_eq!((r.code, r.a, r.b, r.name.as_str(), *actor), (0, 22, 7, "Joiner", Some(1)));
    let (ServerCommand::GuildInvitationResponse(r), actor) = &decoded("guild_answer")[0] else { panic!() };
    assert_eq!((r.code, r.a, r.b, r.name.as_str(), *actor), (2, 22, 3, "Joiner", Some(1)));
    let (ServerCommand::GroupingFailed(f), actor) = &decoded("grouping_failed")[0] else { panic!() };
    assert_eq!((f.tick, f.name.as_str(), f.reason, *actor), (9, "Joiner", 2, None));
    assert_eq!(f.text(), "The invitation has expired.");
    let (ServerCommand::GuildFoundationResponse(r), _) = &decoded("guild_made")[0] else { panic!() };
    assert_eq!((r.status, r.message.as_str(), r.guild.as_str(), r.unknown_bool_0), (3, "name in use", "Knights", false));
}

#[test]
fn guild_status_comes_with_its_members_and_the_ignore_list() {
    let commands = decoded("guild_status");
    let (ServerCommand::GuildStatus(g), _) = &commands[0] else { panic!() };
    assert_eq!((g.guild, g.name.as_str(), g.recipient_id, g.in_guild, g.leader_id, g.member_count), (3, "Knights", 11, true, 11, 2));
    let [leader, officer] = g.members.as_slice() else { panic!() };
    assert_eq!((leader.player_id, leader.actor, leader.name.as_str(), leader.character_class, leader.level), (11, 1, "Leader", 1, 45));
    assert_eq!((leader.online, leader.active, leader.rank, leader.joined), (true, true, 2, [2026, 1, 2, 3, 4, 5]));
    assert_eq!((officer.actor, officer.rank, officer.joined[0]), (0, 1, 2025));
    let (ServerCommand::IgnoredStatus(i), _) = &commands[1] else { panic!("{:?}", commands[1]) };
    assert_eq!((i.status, i.recipient_id, i.count), (0, 11, 0));
    let commands = decoded("no_guild");
    let (ServerCommand::GuildStatus(g), _) = &commands[0] else { panic!() };
    assert!(!g.in_guild && g.members.is_empty());
    assert_eq!(commands.len(), 2);
}

#[test]
fn a_player_query_answers_with_social_rows() {
    let commands = decoded("player_query_result");
    let (ServerCommand::PlayerQueryResult(q), actor) = &commands[0] else { panic!("{commands:?}") };
    assert_eq!((q.query_id, *actor), (7, None));
    let [here, away] = q.players.as_slice() else { panic!("{:?}", q.players) };
    assert_eq!((here.player_id, here.actor, here.name.as_str(), here.map_name.as_str()), (33, 0x10316, "Someone", "Knights"));
    assert_eq!((here.character_class, here.level, here.role), (2, 31, 0xFF));
    assert!(here.online && here.available && here.invitable);
    assert_eq!((away.actor, away.character_class, away.level), (0, 3, 12));
    assert!(!away.online && !away.available && !away.invitable);
    let (ServerCommand::PlayerQueryResult(none), _) = &decoded("player_query_empty")[0] else { panic!() };
    assert_eq!((none.query_id, none.players.len()), (8, 0));
}

#[test]
fn requests_encode_as_the_server_decodes_them() {
    let me = "Leader";
    let mut seen = 0;
    for v in vectors() {
        let command = match v["name"].as_str().unwrap() {
            "found_group" => ClientCommand::GroupFoundation(Foundation { unknown_u32_0: 0, source_name: me.into(), name: "Knights".into(), unknown_bool_1: true }),
            "found_guild" => ClientCommand::GuildFoundation(Foundation { unknown_u32_0: 0, source_name: me.into(), name: "Knights".into(), unknown_bool_1: true }),
            "guild_invite" => ClientCommand::GuildInvite(GuildInvite { inviter_name: me.into(), invitee_name: "Friend".into(), unknown_u32_0: 0, unknown_u32_1: 0, message: "come".into() }),
            "guild_accept" => ClientCommand::GuildInvitationResponse(GroupInvitationResponse { code: 0, a: 11, b: 3, name: "Joiner".into() }),
            "leave_group" => ClientCommand::GroupLeave(Leave { unknown_u32_0: 0 }),
            "leave_guild" => ClientCommand::GuildLeave(Leave { unknown_u32_0: 0 }),
            "player_query" => ClientCommand::PlayerQuery(PlayerQuery { kind: PlayerQuery::ON_MAP, query_id: 7, text: "Som".into() }),
            _ => continue,
        };
        seen += 1;
        assert_eq!(command.encode(), bytes(&v), "{}", v["name"]);
    }
    assert_eq!(seen, 7);
}

/// An appearance update after the weapon is put away, as DSOR_Server
/// encode_remote_player_info writes it (a torso, then a torso and a mace).
/// FAILURE: "je retire mon arme l'autre me voit toujours avec l'arme".
#[test]
fn remote_player_info_carries_the_weapon_and_then_its_absence() {
    let cases = [
        ("85220000000200000002010000000f0077617272696f725f746f72736f5f78028080000007803bb0b93934b7b92f99342fb6b0b1b2c000200fc000000ff373130f8000200fc4401dd85c9c9a5bdc97cc9a17ddd9585c1bdb8000000000000000190000000115ffc0400140008000064000000ffffffff0000000000000005000200ff0", 1044, 2),
        ("85220000000100000002010000000f0077617272696f725f746f72736f5f7808803bb0b93934b7b92f99342fbbb2b0b837b7000000000000000032000000022bff808002800100000c8000001fffffffe000000000000000a000401fe0", 739, 1),
    ];
    for (hex, bits, slots) in cases {
        let data: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        let commands = decode_message(&data, bits).unwrap();
        let (ServerCommand::RemotePlayerInfo(info), actor) = &commands[0] else { panic!("{commands:?}") };
        assert_eq!(*actor, Some(0x20005));
        assert_eq!(info.equipment.len(), slots, "{:?}", info.equipment);
    }
}
