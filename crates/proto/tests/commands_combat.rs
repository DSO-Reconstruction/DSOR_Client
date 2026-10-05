//! Combat commands against real captured bytes (experimental server, 2018 client,
//! session-combat1 / session-merged4 captures): every message decodes to its end and
//! re-encodes to the same bits.

use dsor_proto::commands::{decode_message, encode_chain, ClientCommand, ServerCommand};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// Decode a server 0x85, check it re-encodes exactly, return the commands.
fn server(h: &str, bits: usize) -> Vec<(ServerCommand, Option<u32>)> {
    let data = hex(h);
    let got = decode_message(&data, bits).unwrap();
    let again: Vec<_> = got.iter().map(|(c, a)| (c.clone(), a.unwrap_or(0))).collect();
    let (bytes, n) = encode_chain(&again);
    // The capture may round the stream up to a byte (at most 7 padding bits).
    assert!(n <= bits && bits - n <= 7, "{n} vs {bits}");
    assert_eq!(bytes[..], data[..bytes.len()]);
    got
}

/// Decode a client 0x8B, check it re-encodes exactly.
fn client(h: &str, bits: usize) -> ClientCommand {
    let data = hex(h);
    let got = ClientCommand::decode(&data, bits).unwrap();
    assert_eq!(got.encode(), (data, bits));
    got
}

#[test]
fn hit_and_kill() {
    // session-combat1 frame 5937
    let got = server("857300be05000002000000000408874000134740000b00000000c0000021c0404021c04040000000000000000002a000201fe0", 403);
    let ServerCommand::Hit(hit) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!(hit.tick, 0x5be);
    assert_eq!(hit.damage_types, vec![0, 4]);
    // frame 5981
    let got = server("857400c305000001000000004b000000000000001500010000f819bf807b1abeb8f41342a005000000000000438080807f80", 393);
    let ServerCommand::Kill(kill) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!(kill.unknown_u32_0, 75);
    assert_eq!(kill.killer, 0x0001_0015);
}

#[test]
fn location_effect_and_status_effect() {
    // frame 24444: NewLocationEffect
    let got = server("854000010000000c0053706865726545666665637410000000000000000000000000000000000000a0305f2080700f5ddc4616a157d6bf1f800044200000401f910036b0b3b2afb9b4b733bab630b934ba3cafb0b936b7b92fb932b23ab1b2afb0bab930ca820000000000001181e0000060000013532b37e00000000000000006e0000002a000200000000000000000000000000000a8000807f8", 1237);
    let ServerCommand::NewLocationEffect(e) = &got[0].0 else { panic!("{got:?}") };
    assert_eq!(e.entity_class, "SphereEffect");
    // frame 116: StatusEffect
    let got = server("855200008000007805808100800ffb90000c8000004f80800000000000407a90001b8000000a80008020200000000000081029a9909400000c842b5774a3f9d025342000020416c5a8ebe15000100ff0", 636);
    assert!(matches!(got[0].0, ServerCommand::StatusEffect(_)));
}

#[test]
fn server_skills_carry_the_actor_slot() {
    // frame 56913: five moves then a TargetSkill with its slot-9 data
    let got = server("85670063dea8fe550e00000090210000000015000100ff6700b2dea8fe4c0d4021f690210000120008010100ff6700d6e9d4feeb0a0000009021000000000a010100ff6700e9ead4feba0b0000009021000000000b010100ff67008dd9f8fe291100005690210000000014010100ff4a000000660100853640932100000000000015000100080101009f2100000c0000001b0000001b00000000388cc2008041bac01467420000803f7f80", 1368);
    let (ServerCommand::TargetSkill(s), Some(actor)) = &got[5] else { panic!("{got:?}") };
    assert_eq!((s.base.skill_id, s.target, *actor), (0x166, 0x0001_0015, 0x0001_0108));
    let slot = s.server.unwrap();
    assert_eq!((slot.impact_tick, slot.phase_frame, slot.unblock_frame), (0x219f, 12, 27));
    assert_eq!(slot.position[3], 1.0);
    // frame 83497: BulletSkill
    let got = server("8567002d0c63ffdd0d00000028300000000000010100ff6700040a63ff850c00e6e629300000000015000100ff4b0000003501393a13c028300000936fde3c864b203fd52b99b640580f3f00000000001500010044300000050000000f000000060000000010a7424ff31f410050a8410000803f7f80", 944);
    assert!(matches!(&got[2].0, ServerCommand::BulletSkill(b) if b.server.is_some()));
    // frame 86019: ShiftedSkill, whose slot ends with the shift (+0x4C)
    let got = server("856700200663fff50600000077310000000000010100ff67008efd63ff7605007e7e78310000000015000100ff4d000000390181dde5bf7731000020357cc30147e18a420000204166661641150001009a31000009000000140000000a00000000706a424ff31f410080dd400000803f0d0000007f80", 944);
    let ServerCommand::ShiftedSkill(s) = &got[2].0 else { panic!("{got:?}") };
    assert_eq!(s.points.len(), 1);
    assert_eq!(s.server.unwrap().shift, Some(26));
}

#[test]
fn client_skills_reencode_exactly() {
    assert!(matches!(client("8b490000002801712dc53ce9200000ac2c9586", 152), ClientCommand::Skill(_)));
    // session-merged4 frame 101881
    let ClientCommand::TargetSkill(t) = client("8b4a000000da04257b0b40fd3f00005d8ef2558e010100", 184) else { panic!() };
    assert_eq!(t.target, 0x0001_018e);
    assert!(matches!(
        client("8b4b0000003401df9503bf30070000f361a5f50475d33e348c5eb9c3403bbf0000000000", 288),
        ClientCommand::BulletSkill(_)
    ));
    let ClientCommand::TargetPointBulletSkill(p) =
        client("8b4c00000027010986d03f89240000f401752decbc04bfb249cebc4fe4f73c0000000000019c8f45406a1f0a406f259a42", 392)
    else {
        panic!()
    };
    assert_eq!(p.points.len(), 1);
    let ClientCommand::ShiftedSkill(s) = client("8b4d00000038014e5526c031050000cd71c389019999f1c12a2b5ac0c2f5b241", 256) else { panic!() };
    assert_eq!(s.points.len(), 1);
    assert!(s.server.is_none());
}

#[test]
fn client_death_commands_round_trip() {
    use dsor_proto::commands::combat::{QuickResurrectGroupMember, Respawn, Resurrect};
    for cmd in [
        ClientCommand::Respawn(Respawn { at_respawn_point: true, instant: false }),
        ClientCommand::Resurrect(Resurrect { target: 0x0001_0015, originator: 0, state: 1 }),
        ClientCommand::QuickResurrectGroupMember(QuickResurrectGroupMember { target: 0x0001_0016 }),
    ] {
        let (bytes, bits) = cmd.encode();
        assert_eq!(ClientCommand::decode(&bytes, bits).unwrap(), cmd);
    }
    // Respawn is two bits: 8b 6a 00 then 0b10.
    let (bytes, bits) = ClientCommand::Respawn(Respawn { at_respawn_point: true, instant: false }).encode();
    assert_eq!((bytes, bits), (hex("8b6a0080"), 26));
}
