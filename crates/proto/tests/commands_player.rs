//! Player / session commands against real captured bytes (session-walk4,
//! session-combat1).

use dsor_proto::commands::player::*;
use dsor_proto::commands::{decode_message, encode_chain, encode_single, ClientCommand, ServerCommand};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn one(h: &str, bits: usize) -> (ServerCommand, Option<u32>) {
    let mut v = decode_message(&hex(h), bits).unwrap();
    assert_eq!(v.len(), 1);
    v.remove(0)
}

#[test]
fn instance_config_client_info() {
    // walk4 frame 105: 0x84/29, 90 bits.
    let (c, actor) = one("841d00000000000540004000", 90);
    assert_eq!(c, ServerCommand::InstanceConfigClientInfo(InstanceConfigClientInfo::default()));
    assert_eq!(actor, Some(0x0001_0015));
    let (b, n) = encode_single(&c, 0x0001_0015);
    assert_eq!((b, n), (hex("841d00000000000540004000"), 90));
}

#[test]
fn switch_map_trailer_bit() {
    let (c, actor) = one("8479000e003132372e302e302e313a3231393280", 160);
    assert_eq!(actor, None);
    assert_eq!(
        c,
        ServerCommand::SwitchMap(SwitchMap { target: "127.0.0.1:2192".into(), character_service: true })
    );
    let (c, _) = one("8479000f003132372e302e302e313a333030303000", 168);
    assert!(matches!(c, ServerCommand::SwitchMap(SwitchMap { character_service: false, .. })));
    let (c, _) = one("847900000000", 48);
    assert_eq!(c, ServerCommand::SwitchMap(SwitchMap::default()));
}

#[test]
fn exit_info_with_guid() {
    let h = "84480001000000160061303230315f656e7472795f66726f6d5f61303230300f0061303230305f6b696e677363697479000000001000c88e3c47b6a2489b8ea457bfcd459b3a15000100";
    let (c, actor) = one(h, 592);
    assert_eq!(actor, Some(0x0001_0015));
    let ServerCommand::ExitInfo(e) = &c else { panic!("{c:?}") };
    assert_eq!(e.exits.len(), 1);
    assert_eq!(e.exits[0].name, "a0201_entry_from_a0200");
    assert_eq!(e.exits[0].destination, "a0200_kingscity");
    assert_eq!(e.exits[0].guid.len(), 16);
    assert_eq!(encode_single(&c, 0x0001_0015), (hex(h), 592));
}

#[test]
fn currency_changed_in_a_chain() {
    let h = "85890098909800ad18a4007f9698007f9698007f9698007f969800000000008a8000807f80";
    let v = decode_message(&hex(h), 289).unwrap();
    let ServerCommand::CurrencyChanged(c) = &v[0].0 else { panic!() };
    assert_eq!(c.wallet[0], 0x0098_9098);
    assert!(c.notify);
    let (b, n) = encode_chain(&[(v[0].0.clone(), v[0].1.unwrap())]);
    assert_eq!((b, n), (hex(h), 289));
}

#[test]
fn client_commands_reencode_exactly() {
    for (h, bits) in [
        ("8b220000010100", 56),
        ("8b29008a80008000", 57),
        ("8b2b0000", 25),
        ("8b6900060054616c656e74030048756200", 129),
        ("8b6b0000", 26),
        ("8b78001509f4f5", 56),
        ("8b90000300010000000000000000000000000000000000000000000000000000", 249),
        ("8bbd0001000000000000", 80),
        ("8bc40000000000", 56),
        ("8bed000000000001000000", 88),
        ("8bf30001000000ebb0307571da903f800700003804000000", 185),
        ("8bf400010000000020700dc74e833f05d3233c0cb2c03b1072833cf3ad1143397b8642000000002998383b00000000", 376),
        ("8bf50001000000ad00000000000000020000000900696e745f706172616d000000000a00696e745f706172616d32010000000000000000000000", 464),
        ("8b0101ffffffff0a0000000000000011000000", 152),
        ("8b03010000000000", 64),
        ("8b1a011700426167457874656e73696f6e4f666632355369676e616c0115000100", 264),
        ("8b31010000000000000000", 88),
    ] {
        let c = ClientCommand::decode(&hex(h), bits).unwrap();
        assert!(!matches!(c, ClientCommand::Unknown { .. }), "{h}");
        assert_eq!(c.encode(), (hex(h), bits), "{h}");
    }
}

#[test]
fn travel_fields() {
    let c = ClientCommand::decode(&hex("8b6900060054616c656e74030048756200"), 129).unwrap();
    assert_eq!(
        c,
        ClientCommand::Travel(Travel { point: "Talent".into(), destination: "Hub".into(), pay_with_rc: false })
    );
}
