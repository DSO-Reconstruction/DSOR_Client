# Getting started

[Back to the README](../README.md) | [Testing and diagnostics](testing-and-diagnostics.md)

Run the commands below from the repository root unless a step says otherwise.

The client needs the game's own files, converted to glTF/PNG by
[DSO_Godot](https://github.com/DSO-Reconstruction/DSO_Godot). They are generated
locally into `assets/` and are never committed (see `tools/convert_assets.py`).
To play without building anything, use the [release](release.md): it
downloads and converts the game data by itself.

## 1. Prerequisites (once)

- Rust (rustup) with the browser target: `rustup target add wasm32-unknown-unknown`
- [trunk](https://trunkrs.dev) for the browser build (`cargo install trunk`, or its
  release binary in `~/.cargo/bin`)
- python3 with Pillow and numpy, a C++ compiler, git (asset conversion)
- [vgmstream-cli](https://github.com/vgmstream/vgmstream/releases) with its companion libraries
  (audio extraction; on `PATH` or selected with `DSOR_VGMSTREAM`)
- ALSA development headers for native Linux audio builds
- the game's own data: the client downloads it from Bigpoint's CDN into
  `game-data/` on its account screen, or `cargo run --release -p dsor-game-data`
  does without a window ([the game data](game-data.md)). Any unpacked client
  tree (`export_win32`) works too.

## 2. Convert the game data

The current build downloads into `game-data/` (2.2 GB, 4.75 GB on disk) when
the client's account screen opens, or with
`cargo run --release -p dsor-game-data`; see [the game data](game-data.md).
Convert its `export_win32` (or any other unpacked client tree):

```sh
python3 tools/convert_assets.py game-data/export_win32 ~/Documents/Drakensang/dsor_assets
ln -sfn ~/Documents/Drakensang/dsor_assets assets
```

(`tools/convert_assets.sh` runs the same; on Windows run the `.py` with a Python
that has Pillow and numpy, no shell needed.) This prepares audio banks as WAVs,
event bindings and map audio, runs DSO_Godot, then the tools of `tools/` in order
(render states, cube maps, ambience, skills, NPCs, exits, interface windows,
level decorations), the navigation meshes (`maps/<map>.nav.bin`, from the game
data's PathEngine `.tok` files with `tools/navmesh_tok.py`), the quest catalogue
(from the game data's `quests.pbxml`) and the KTX2 textures, and copies Tahoma
from the system's fonts. No server checkout is needed. It prints each step's
time; `<out>/conversion.json` records what was converted ([the release](release.md)).
See the [asset reference](assets.md) for conversion details and file formats.
Existing packs that already contain decoded audio only need
`python3 tools/export_audio_runtime.py <export_win32> <assets>` for playback.
Volume and mute controls are under Settings → Audio; browser playback starts
after a click or key press.

The conversion runs these tools too (character masks, monsters,
picking, statistics, combat texts, the debug map, plus the selection screen
and the trade window) and copies the font. After only `static.db4` changed,
`--database-only` reruns the steps that read it over an existing pack. To refresh an
existing pack without a full conversion, run them directly (same two paths,
`<export_win32>` then `<assets>`):

```sh
R=~/Downloads/DSOClient2018_rel_206-7-32bit/export_win32   # or the 261 one, below
O=~/Documents/Drakensang/dsor_assets
G=~/Documents/Drakensang/DSO_Godot
python3 tools/embed_character_masks.py "$R" "$O" "$G"   # character dyes
python3 tools/export_monsters.py "$R" "$O"              # monsters, summons
python3 tools/export_picking.py "$R" "$O"               # click shapes
python3 tools/export_stats.py "$R" "$O"                 # character sheet
python3 tools/export_combat_text.py "$R" "$O" "$G"      # damage numbers
python3 tools/export_debug.py "$O"                      # debug map (F3)
mkdir -p "$O/fonts"                                      # Tahoma, the game's font
cp /System/Library/Fonts/Supplemental/Tahoma.ttf "$O/fonts/tahoma.ttf"
cp "/System/Library/Fonts/Supplemental/Tahoma Bold.ttf" "$O/fonts/tahoma_bold.ttf"
```

(Windows: Tahoma is `C:\Windows\Fonts\tahoma.ttf` / `tahomabd.ttf`.)

### Release 261 data (branch `release261`)

The 261 client comes as Bigpoint's packed download cache (`dlcache`: `.nb`
bundles and `__toc` files with hash suffixes). The game only downloads what a
character has visited, so that cache misses files (12,960 in ours: skill
animations, meshes, textures -- half-drawn spells, a debug map without ground).

1. Unpack it with DSO_Godot:
   `python3 $G/unpack.py <packed client folder> ~/Documents/Drakensang/client261`
2. Fill the gaps with the 2018 files of the same path (only files the 261 TOCs
   list and the folder lacks; nothing is overwritten):
   ```sh
   cd ~/Documents/Drakensang/client261
   python3 - <<'PY' > /tmp/fill261.txt
   import glob, os
   listed = {l.split("|")[0] for t in glob.glob("*__toc*")
             for l in open(t, encoding="utf-8", errors="replace") if l.count("|") == 2 and "|f|" in l}
   print("\n".join(p for p in sorted(listed) if not os.path.exists(p)))
   PY
   rsync -a --ignore-existing --files-from=/tmp/fill261.txt \
         ~/Downloads/DSOClient2018_rel_206-7-32bit/ .
   ```
   (A file Bigpoint changed since 2018 stays in its 2018 version.)
3. Convert as above with `R=~/Documents/Drakensang/client261/export_win32` and
   another output folder, then run the extra tools, then point `assets` at it.

### October 2026 data (the upgraded server)

The client the upgraded server matches comes extracted from its bundles
(here `D:\dso-client-october-26`): `export_win32/` is the data tree with its
static database at `export_win32/db/static.db4` as usual (the extractor may
write that one under `global/export_win32/db/`: move it into
`export_win32/db/`), `_toc/` the bundles' listings. The client's own download
(`game-data/`, [the game data](game-data.md)) makes the same tree from the
CDN's current build, the database already in place. Convert into a new folder,
then put it in place of `assets`:

```powershell
# A Python with Pillow and numpy (Anaconda's); cargo on PATH with the Visual
# Studio linker (tools/Build-Client.ps1's setup) for the level and texture tools.
C:\ProgramData\anaconda3\python.exe tools/convert_assets.py D:\dso-client-october-26\export_win32 .local\assets-oct26 .local\DSO_Godot
```

The script runs every generator the client loads (the extra tools listed
above included). The quest catalogue comes from the data's own
`quests.pbxml` (the server's `quests.xml` is that pack decoded, from an older
build) with its texts from the client's database.

### After pulling new code

A tool that changed only takes effect once it is run again over your assets.
Rerunning `convert_assets.sh` always works; to save time, rerun just the tools
the commits touched, for instance:

| Changed | Rerun |
|---|---|
| `tools/export_skills.py` (sequences, statuses) | `export_skills.py`, then `export_deco.py` |
| `tools/export_deco.py` (town guards, townsfolk, animals) | `export_skills.py`, then `export_deco.py` |
| `tools/export_monsters.py` (summons) | `export_monsters.py` |
| `tools/export_pets.py` (pets) | `export_pets.py <export_win32> <assets> --database <server>/data/db/static.db4` |
| `tools/export_audio.py` (prepared sound files) | `export_audio.py <export_win32> <assets>`; see [audio preparation](assets.md#audio-preparation) |
| DB4 props/travel points | `dsor-levels props --assets assets --db-root <server>/data/maps --static-db <server>/data/db/static.db4` |
| `tools/embed_animators.py` (effect fades, decals) | `embed_animators.py <export_win32>/models <assets>` |
| `tools/restore_pivots.py` (effect positioning) | `restore_pivots.py <export_win32>/models <assets> <DSO_Godot>` after the other model postprocessors |
| `tools/restore_skin_fragments.py` (bosses' missing body parts) | `restore_skin_fragments.py <export_win32> <assets> <DSO_Godot>` |
| `tools/export_stats.py` | `export_stats.py` |
| `vfx-overrides/` (someone's VFX edits) | `python3 tools/vfx_apply.py <assets>` |

Mount rendering needs both `characters/mounts.json` and the `mount` /
`movement_bonus` fields in `skills/status_effects.json`, produced by
`export_skills.py`. Without them the summon cast can finish without displaying
the mount. Use the same static database version as the server: status-effect
wire IDs depend on its row order.

Pets need `characters/pets.json` (`export_pets.py`, after `export_monsters.py`
and `export_skills.py`): which status effects put out a pet, and each pet's
model, speeds and sequences. Without it the client logs that it did not load
and draws no pets; nothing else changes.

`export_deco.py` adds its animation sets to `characters/anims.json`, so it must
run after `export_skills.py` and after any `character_tables.py` run. Any tool
that rewrites an effect model or `skills/sequences.json` drops the VFX edits
from it: run `tools/vfx_apply.py <assets>` last.

## 3. Native client

### Native DLSS / DLAA

The `dlss` Cargo feature integrates Bevy's NVIDIA DLSS Super Resolution backend
on Windows/Linux with Vulkan. Native builds include it by default; browser and
macOS builds ignore it, as the dependency is Windows/Linux only. RTX support is
checked at runtime: a vendor name alone does not enable the menu option, and
other GPUs see "Unavailable on this device".

Building needs the [DLSS SDK v310.5.3](https://github.com/NVIDIA/DLSS/tree/v310.5.3),
the Vulkan headers and libclang. `.cargo/config.toml` points `DLSS_SDK`,
`VULKAN_SDK` and `LIBCLANG_PATH` at git-excluded copies under `.local/dlss/`
(`sdk/`, `vulkan/` and `clang/clang/native/`); variables already set in the
environment take precedence. This version matches `dlss_wgpu` 4.0.0 / Bevy 0.19.
A plain native build therefore includes it:

```sh
cargo build --release -p dsor-client
```

Without the SDK (CI does this) build with `--no-default-features`; the Settings
row then reads "Not in this build".

Place `nvngx_dlss.dll` from the SDK's `lib/Windows_x86_64/rel` beside the Windows
executable, or `libnvidia-ngx-dlss.so.310.5.3` from `lib/Linux_x86_64/rel` beside
the Linux executable. Distribution also requires the copyright and license
texts specified in section 9.5 of the SDK programming guide; see the
[wrapper's packaging instructions](https://github.com/bevyengine/dlss_wgpu#distributing-your-app).
SDK files and binaries are not checked into this repository.

This build uses Bevy's native Vulkan initialization. Build with
`--no-default-features` when you need another graphics backend. Settings > Graphics offers DLAA and DLSS Quality/Balanced/Performance
only when the active adapter is NVIDIA and the runtime reports support. Missing
runtime files and unsupported cards retain the selected
FXAA/SMAA fallback. DLAA keeps native resolution; DLSS renders at a lower
resolution and reconstructs the output. Frame generation and ray reconstruction
are not included.

The integration accounts for the reduced opaque viewport in bloom/refraction
and corrects Bevy 0.19's output-resolution assumptions in clustered lighting and
SSAO. A Bevy update that changes those shader anchors disables the DLSS control
until the compatibility fix is reviewed.

### Launching the client

Running `dsor-client.exe` without arguments opens its built-in login screen.
On Windows, `Play.cmd` first runs `tools/Build-Client.ps1`, then opens
`target/fast/dsor-client.exe`. That script builds the `fast` profile: release
optimisations without cross-crate link-time optimisation, compiled
incrementally. Cargo rebuilds only what changed, so an up-to-date client starts
at once; a small change takes about 20 seconds, many new commits a few minutes
(the progress bar rests on `dsor-client(bin)` meanwhile). Its frames are about
7% slower than the release build's (`cargo build --release`, 4–5 minutes after
a change), which releases and frame-time measurements use. A client still running is renamed aside so the new build can
replace it. Sign in or create an account,
then choose a character and press **Play Now** to enter the game in the same window. The client uses the
account-service protocol from `dso_custom_client` and passes the returned
account ID and session directly to its game connection.

Native launches use direct 3D and shadow draws to reduce preparation overhead
from small batches. CPU visibility checks, shadows, and GPU instance preprocessing
remain enabled where supported. Set `DSOR_FLAGS=indirectdraw` before launching to
restore the platform's default draw mode; this also applies to `Play.cmd`.
The mode is chosen at startup, so changing it requires restarting the client.

Build and run the complete client, including the login screen:

```sh
cargo build --locked --release -p dsor-client
cargo run --locked --release -p dsor-client
```

The account service defaults to `http://127.0.0.1:8080`. The Rust client connects
to the advertised game host on login port 2190 (or `launch.login_port` when
provided), rather than the retired client's gateway port 80. The service must
be running before signing in. **Settings** configures this address and the name,
class and gender suggested when creating a character. Existing accounts open
the original forest selection scene with their roster and an animated equipment
preview. Click a row or use Up/Down to select; drag the preview or hold its arrow
buttons to rotate it. The list arrows scroll additional characters. Creation and
deletion require their own confirmation. Preferences live in the ignored `client-auth.json`;
passwords and sessions stay in memory. Tab switches fields, Ctrl+V pastes, and
Enter submits the form. Failed authentication and connection errors appear in
the game window and allow another attempt.

The executable also retains the direct game and offline viewer commands:

```sh
cargo run -p dsor-client --release -- a0200_kingscity          # map viewer (free camera)
cargo run -p dsor-client --release -- a0302_wildforest --npcs   # with the level's NPCs
cargo run -p dsor-client --release -- --server 127.0.0.1:2190 --account <id> --sid <session> [--char <id>]
```

| Argument | What |
|---|---|
| `<map>` | the map to show offline (`a0200_kingscity` by default) |
| `--login` / `--offline` | explicitly show the account screen / open the default map viewer |
| `--cam x,y,z,tx,ty,tz` | camera position and look-at point, game frame |
| `--screenshot` / `--screenshot-to <png>` | once loaded, save a screenshot and quit |
| `--no-shadows`, `--npcs` | no sun shadows / place the level's NPCs |
| `--character <class> <gender>` `[--anim <state>]` | a dressed demo character at the map centre (class 0 warrior, 1 mage, 2 ranger, 3 dwarf) |
| `--server <host:port>` `--account <id>` `--sid <session>` `--char <id>` | play online against the login server |

Clicks over the HUD, open game windows, and F2/F3 panels are captured by the
interface until release. They do not issue movement, attacks or exit requests;
scrolling over the interface does not zoom the world camera.

Startup and map transitions use the in-client loading screen with
`splashart_crunch.png` and the frame/fill from `statusbar_connenctscreen_new.png`.
Map progress follows nearby geometry, textures and props. The initial loading
barrier also waits for navigation/ambience files, nearby NPC/decor scheduling,
scene instances, first poses, and the player's complete outfit. Failed optional
assets settle with their existing asset diagnostics; a failed player appearance
keeps the loading error visible.

Under the bar, a line says what the loading is doing in plain words ("Loading
the world: 312 of 488 models", "Dressing your character", "Preparing
graphics"...), with the seconds taken once past 5. A light crosses the bar while
work goes on. When neither the percent nor that step moves for 15 s, a note says
the game is still working (shader preparation has its own). After 45 s it offers
**Enter anyway**, once the map and the player are in, and **Close client**. Both
are logged (`loading screen: no progress for ...`, `entered by the player
...`) with the readiness stage.

The selected class preloads its skill and status-effect models, hit effects,
and the selected skin variants and spawn/hit/death effects of its summons.
These requests run four at a time after initial geometry/props settle, and
their handles stay resident for that class, including through a map change
(every actor is despawned before the next map's player arrives). Behind the opaque overlay, batches
of inert model/particle samples use the actual game camera's rendering settings.
The render thread acknowledges uploaded meshes/materials and compiled pipelines
before the samples are removed and the world is revealed. This does not play
skills, emit particles, or trigger audio. It prepares the initial class and view;
later equipment, other players' effects and changed graphics settings may still
introduce new assets or shader variants.
This moves preparation into the loading screen to reduce first-use stalls;
it can lengthen startup and does not reduce steady-state scene rendering cost.

The shared `LoadingScreen::begin_assets` API accepts asset batches for additional
preload stages. Completion is logged as `loading screen: ready`; the preceding
`preload:` and `render warm-up:` lines report the asset and render sample counts.

## 4. Browser client

`tools/build_web.sh` builds the client twice into `client/dist` (WebGPU and
WebGL2) with a loader page that picks WebGPU when the browser offers it;
`tools/serve_web.sh` serves it with the relay. See [effects and rendering](fx.md) for the
renderer's state and [the interface guide](ui.md) for the interface.

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
`noparticles`, `nodecals`, `norim` (no character rim light), `noanim`, `noanimviewcull` (disable NPC visibility-based
animation culling), `nonames`, `nonpcs-shadow`, `depth` (depth
pre-pass on WebGL2: soft particle and water edges), `nodepth` (natively / WebGPU:
without it).

WebGPU needs a secure context: `https://`, or `http://localhost`. A player on
another machine reaching plain `http://` gets the slower WebGL2 build; serve over
HTTPS (and the relay over `wss://`) for others.

## 5. In game

| Key | What |
|---|---|
| left click | walk (online); Shift + left / right button, 1-5: quick slots |
| I, C, S | inventory, character sheet, skills |
| drag an inventory item | move/swap slots; drop onto a compatible stack to merge |
| Ctrl + left click a stack | choose an amount to split into a free inventory slot |
| F2 | lighting panel: scale the level's sun, ambient, placed lights and fog; camera colours come from ambience bubbles. With `DSOR_STUDIO_PREVIEW=1` (natively, for developers) F2 opens the Lighting Menu instead: the same multipliers, live overrides of the map's and each zone's ambience (with a zone preview), and named presets, saved in `lighting-presets.json` |
| F9 or Ctrl+Shift+D | bug dump: downloads `dsor-dump-<time>.json` (natively: written in the working directory). Also written by itself on a crash or a rendering error. Send it with the bug report. |
| F3 → Free look | detach the camera from a spawned character; turn off to follow the character again at the previous zoom |
| W A S D, Q E, right mouse | free camera (offline viewer or Free look): move, down/up, and look; Shift flies faster. Close F3 to use the controls. |
