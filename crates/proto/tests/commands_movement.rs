//! MoveCommand and the other movement-group commands against real captured bytes.

use dsor_proto::commands::movement::{ActorStatsUpdate, ActorsEnterVicinity, Move};
use dsor_proto::commands::{decode_message, encode_chain, ClientCommand, ServerCommand};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

#[test]
fn server_move_record() {
    // session-combat1 frame 422, port 30001: one MoveCommand for actor 0x00010015.
    let data = hex("8567004ff94d019408000000ff010000000015000100ff");
    let got = decode_message(&data, 184).unwrap();
    assert_eq!(
        got,
        vec![(
            ServerCommand::Move(Move {
                x: -1713,
                elevation: 333,
                y: 2196,
                speed: 0,
                heading: 0,
                facing: 0,
                start_tick: 0x1ff,
                duration: 0
            }),
            Some(0x0001_0015)
        )]
    );
    let (bytes, bits) = encode_chain(&[(got[0].0.clone(), 0x0001_0015)]);
    assert_eq!((bytes, bits), (data, 184));
}

#[test]
fn client_move_reencodes_exactly() {
    // session-combat1: the client's own MoveCommand, 144 bits, no actor.
    let data = hex("8b67001ef41900e909cc8080a70100001400");
    let cmd = ClientCommand::decode(&data, 144).unwrap();
    let ClientCommand::Move(m) = &cmd else { panic!("{cmd:?}") };
    assert_eq!((m.x, m.elevation, m.y), (-3042, 25, 2537));
    assert_eq!((m.speed, m.heading, m.facing), (0xcc, 0x80, 0x80));
    assert_eq!((m.start_tick, m.duration), (0x1a7, 20));
    assert_eq!(cmd.encode(), (data, 144));
}

#[test]
fn chained_vicinity_and_stats() {
    let chain = vec![
        (ServerCommand::ActorsEnterVicinity(ActorsEnterVicinity { actors: vec![0x10086, 0x10087] }), 0x10015),
        (ServerCommand::ActorStatsUpdate(ActorStatsUpdate { health: 235.0, resource: 0.2 }), 0x10015),
    ];
    let (bytes, bits) = encode_chain(&chain);
    assert_eq!(bits, 8 + (16 + 96 + 40) + (16 + 64 + 40));
    let back = decode_message(&bytes, bits).unwrap();
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].0, chain[0].0);
    assert_eq!(back[1], (chain[1].0.clone(), Some(0x10015)));
}
