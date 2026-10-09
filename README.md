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

## Running and testing

### 1. Prerequisites (once)

- Rust (rustup) with the browser target: `rustup target add wasm32-unknown-unknown`
- [trunk](https://trunkrs.dev) for the browser build (`cargo install trunk`, or its
  release binary in `~/.cargo/bin`)
- python3 with Pillow and numpy, a C++ compiler, git (asset conversion)
- the unpacked 2018 client (`DSOClient2018_rel_206-7-32bit/export_win32`)

### 2. Convert the game data (once, ~10 minutes, ~3 GB)

```sh
tools/convert_assets.sh ~/Downloads/DSOClient2018_rel_206-7-32bit/export_win32 \
                        ~/Documents/Drakensang/dsor_assets
ln -sfn ~/Documents/Drakensang/dsor_assets assets
```

This runs DSO_Godot, then every tool of `tools/` in order (render states, cube
maps, ambience, skills, NPCs, exits, interface windows). With
`EXPERIMENTAL=<path of the server repo>` it also exports the navigation meshes.
Details: `docs/assets.md`.

### 3. Native client

```sh
cargo run -p dsor-client --release -- a0200_kingscity          # map viewer (free camera)
cargo run -p dsor-client --release -- a0302_wildforest --npcs   # with the level's NPCs
cargo run -p dsor-client --release -- --server 127.0.0.1:2190 --account <id> --sid <session> [--char <id>]
```

| Argument | What |
|---|---|
| `<map>` | the map to show offline (`a0200_kingscity` by default) |
| `--cam x,y,z,tx,ty,tz` | camera position and look-at point, game frame |
| `--screenshot` / `--screenshot-to <png>` | once loaded, save a screenshot and quit |
| `--no-shadows`, `--npcs` | no sun shadows / place the level's NPCs |
| `--character <class> <gender>` `[--anim <state>]` | a dressed demo character at the map centre (class 0 warrior, 1 mage, 2 ranger, 3 dwarf) |
| `--server <host:port>` `--account <id>` `--sid <session>` `--char <id>` | play online against the login server |

### 4. Browser client

```sh
tools/build_web.sh            # the WebGPU and WebGL2 builds + loader, in client/dist
tools/serve_web.sh            # relay on ws://0.0.0.0:2290 + web server on http://localhost:8080
                              # (builds first if client/dist is missing; --build to rebuild)
```

Then open, offline (map viewer):

```
http://localhost:8080/?map=a0200_kingscity
http://localhost:8080/?map=a0302_wildforest&cam=82,13,220,100,-5,202
```

or online (the game server must be running, e.g. experimental's `run_local_2018.sh`):

```
http://localhost:8080/?server=127.0.0.1:2190&relay=ws://127.0.0.1:2290&account=<id>&sid=<session>
```

| URL parameter | What |
|---|---|
| `map=<name>` | offline map viewer |
| `cam=x,y,z,tx,ty,tz` | camera position and look-at point |
| `npcs` | place the level's NPCs (offline) |
| `noshadows` | no sun shadows |
| `server=host:port`, `relay=ws://...`, `account=`, `sid=`, `char=` | play online (the browser reaches the server through the relay) |
| `gpu=webgpu` / `gpu=webgl2` | force a graphics API (default: WebGPU when the browser offers it) |
| `flag=a,b,...` | diagnostic switches, below |

Flags (`?flag=...` in the browser, `DSOR_FLAGS=...` natively): `nobloom`, `nolights`
(no placed level lights), `noshadows`, `lowshadows` (512 shadow map),
`noparticles`, `nodecals`, `noanim`, `nonames`, `nonpcs-shadow`, `depth` (depth
pre-pass on WebGL2: soft particle and water edges), `nodepth` (natively / WebGPU:
without it).

WebGPU needs a secure context: `https://`, or `http://localhost`. A player on
another machine reaching plain `http://` gets the slower WebGL2 build; serve over
HTTPS (and the relay over `wss://`) for others.

### 5. In game

| Key | What |
|---|---|
| left click | walk (online); Shift + left / right button, 1-5: quick slots |
| I, C, K | inventory, character sheet, skills |
| F2 | the look panel: scale the level's sun, ambient, fog, bloom, vignette...; tonemapping on / off |
| F9 or Ctrl+Shift+D | bug dump: downloads `dsor-dump-<time>.json` (natively: written in the working directory). Also written by itself on a crash or a rendering error. Send it with the bug report. |
| W A S D, Q E, right mouse | free camera (offline viewer) |

### 6. Automated checks

```sh
cargo test --workspace                       # protocol, relay, RakNet
cargo run -p dsor-client --release -- a0200_kingscity --cam 109,31.1,84.4,121.5,13.4,71.9 \
    --screenshot-to /tmp/kingshill.png        # renders, saves, quits
```

Useful environment variables natively: `DSOR_ASSETS=<dir>` (asset folder),
`DSOR_LIKE_WEB=1` (one thread, as in the browser: CPU cost per frame),
`DSOR_SHOT_WAIT=<frames>` (settle longer before the screenshot), `DSOR_PANEL=1`
(look panel open), `DSOR_UI_OPEN=inventory,charactersheet` (windows open at start),
`DSOR_CAST=<skill id>,<seconds>` with `--character 1 0` (cast a skill repeatedly;
`DSOR_SHOT_AFTER_CAST=<seconds>,<png>` screenshots it),
`DSOR_NO_DECALS=1`.

The HUD (top left) shows FPS, CPU milliseconds per frame (main + render), entities
and surfaces drawn; the browser console logs the same every five seconds.

## Browser build

`tools/build_web.sh` builds the client twice into `client/dist` (WebGPU and
WebGL2) with a loader page that picks WebGPU when the browser offers it;
`tools/serve_web.sh` serves it with the relay. See `docs/fx.md` for the
renderer's state and `docs/ui.md` for the interface.
