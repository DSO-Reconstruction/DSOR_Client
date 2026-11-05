# DSOR_Client

A Drakensang Online client for the browser (WebAssembly) and the desktop, written
in Rust on Bevy, that plays against the DSO-Reconstruction server
(`experimental`, branch `rust-18`, or the Python `multiplayer-2018`).

The goal is a 1:1 reconstruction of the rel 261 client.

## Quick start

To play without building anything, download a
[release](https://github.com/DSO-Reconstruction/DSOR_Client/releases)
(`DSOR-<version>-windows-x64.zip`, about 100 MB; Linux and macOS builds are
experimental), unpack it and start `dsor-client`: it downloads the game data
from Bigpoint's CDN and converts it itself ([the release](docs/release.md)).

To build it yourself, follow [Getting started](docs/getting-started.md) to install prerequisites and
convert the game's data into `assets/` (generated locally and never committed).
Then build and launch the native client:

```sh
cargo build --locked --release -p dsor-client
cargo run --locked --release -p dsor-client
```

On Windows, `Play.cmd` builds the client (`tools/Build-Client.ps1`: the
`fast` profile, about 20 s after a small change and at once when nothing
changed) and launches it. Signing in requires a running
account service (default: `http://127.0.0.1:8080`). The guide also covers
[offline viewing](docs/getting-started.md#3-native-client),
[browser builds](docs/getting-started.md#4-browser-client), and
[in-game controls](docs/getting-started.md#5-in-game).

## Editors

- **[Level editor](docs/level-editor.md):** run `LevelEditor.cmd` or
  `cargo run --release -p dsor-client -- --editor` to edit maps, DB4 records,
  cameras, and lighting. Saved edits live in `level-overrides/`.
- **[VFX editor](docs/vfx-editor.md):** run `VfxEditor.cmd` or
  `python3 tools/vfx_editor/server.py`, then open <http://localhost:8091> to edit
  spell effects with a live preview. Saved edits live in `vfx-overrides/`.

## Layout

| Path | Purpose |
|---|---|
| `client` | Bevy game client for native and WebAssembly builds. |
| `crates/raknet` | Transport-independent RakNet 4.035 client protocol. |
| `crates/proto` | Game commands and RakNet BitStream codec. |
| `crates/relay` | WebSocket-to-UDP relay for browser clients; carries datagrams unchanged. |
| `crates/level-data` | Level overrides, DB4 composition, event layers, and the headless `dsor-levels` tool. |
| `tools` | Asset conversion via DSO_Godot and development helpers. |

## Documentation

- [Source layout](docs/source-layout.md): where client features and supporting crates live.
- [Getting started](docs/getting-started.md): prerequisites, asset conversion, native and browser clients, controls.
- [Testing and diagnostics](docs/testing-and-diagnostics.md): automated checks, environment variables, profiling.
- [Game data](docs/game-data.md): the original client's data, downloaded from Bigpoint's CDN by the account screen.
- [The release](docs/release.md): the client and its converter without the workspace (Windows; Linux and macOS experimental); how it converts and how to build it.
- [Asset reference](docs/assets.md): converted data formats and conversion details.
- [Effects and rendering](docs/fx.md): shaders, lighting, particles, and rendering diagnostics.
- [Interface](docs/ui.md): UI assets, implementation, and interactions.
- [Native map performance](docs/native-map-performance.md): investigations and comparison commands.
- [Client progress](docs/client-progress.md): feature gaps and implementation results.

## Copyright, trademarks, and third-party material

This is an independent, unofficial research project. It is not affiliated with, endorsed by, or sponsored by Bigpoint GmbH or the owners of Drakensang Online. “Drakensang Online,” related names, logos, characters, artwork, music, client software, and game content remain the property of their respective rights holders. This repository does not claim ownership of or grant rights to that material.

Any client files mentioned or listed above are not distributed here, this is a purely technical project. Do not commit or redistribute game client files, database dumps, maps, language files, account data, captures, or other third-party material unless you have the necessary rights and permission. Keep local-only assets out of Git. Repository privacy does not grant rights to third-party material.