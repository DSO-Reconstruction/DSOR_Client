# DSOR_Client

A Drakensang Online client for the browser (WebAssembly) and the desktop, written
in Rust on Bevy, that plays against the DSO-Reconstruction server
(`experimental`, branch `rust-18`, or the Python `multiplayer-2018`).

The goal is a 1:1 reconstruction of the 2018 client (rel 206.7), built from its
decompilation, with performance and the combat system first.

## Layout

| Path | What |
|---|---|
| `crates/raknet` | RakNet 4.035 client protocol in pure Rust, transport-agnostic (no sockets): the same code runs natively over UDP and in the browser over the relay. |
| `crates/proto` | The 2018 game protocol: RakNet BitStream codec and the game commands. |
| `crates/relay` | WebSocket <-> UDP relay placed next to the server. Browsers cannot send UDP; the relay carries RakNet datagrams unchanged. |
| `client` | The game, on Bevy. Builds natively and for `wasm32-unknown-unknown`. |
| `tools` | Asset conversion (wraps DSO_Godot) and helpers. |

## Game data

The client needs the 2018 game's own files, converted to glTF/PNG by
[DSO_Godot](https://github.com/DSO-Reconstruction/DSO_Godot). They are generated
locally into `assets/` and are never committed (see `tools/convert_assets.sh`).
