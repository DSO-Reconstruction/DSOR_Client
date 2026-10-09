# Converted game assets

The client renders the 2018 client's own data (rel 206.7), converted to glTF 2.0
and PNG by [DSO_Godot](https://github.com/DSO-Reconstruction/DSO_Godot). The
data belongs to the publisher, so it is generated locally and never committed:
`assets/` at the workspace root is gitignored and is normally a symlink to a
folder outside the repository.

## Producing it

```sh
# <export_win32> is the unpacked client tree (models/, meshes/, maps/, textures/, ...)
tools/convert_assets.sh ~/Downloads/DSOClient2018_rel_206-7-32bit/export_win32 \
                        ~/Documents/Drakensang/dsor_assets
ln -sfn ~/Documents/Drakensang/dsor_assets assets
```

The script clones DSO_Godot if it is not given one, builds its `.crn`
transcoder, then runs `batch_export.py` (models), `split_character.py` (the two
player skeletons) and `export_maps.py` (maps). Logs and a per-model report land
in `<out>/logs/`. On a 24-core machine the whole run takes a few minutes; the
2018 data comes to about 2.8 GB.

Natively the client reads `<workspace>/assets` (override with the `DSOR_ASSETS`
environment variable). In the browser it fetches `assets/...` relative to the
page, so the web server must serve the same tree next to `index.html`.

## What is in it

| Path | What |
|---|---|
| `<category>/<model>.glb` | one model per client `.n3`: `models/t001_hub/arch_house_01.n3` -> `t001_hub/arch_house_01.glb`. Same sub-path, extension swapped. |
| `<category>/<model>.fx.json` | particle emitter and render-state sidecar of a `.glb`, embedded into it by `tools/embed_emitters.py` |
| `textures/<category>/<name>_color.png` etc. | textures, referenced by the `.glb` files through **relative** URIs (`../textures/t001_hub/mat_wood_01_color.png`). Never move a `.glb` out of the tree. Bump maps become `_bump_nrm.png` tangent-space normal maps, spec maps `_spec_rough.png`. |
| `characters/uniskel/`, `characters/uniskel_dwarf/` | the player characters split up: `__animations.glb` (skeleton + every clip), `parts/*.glb`, `outfits.json` (outfit -> parts), `variations.json` (body-shape variations). |
| `maps/<map>.map.json` | one placement manifest per client `.map` (format below). |
| `maps/<map>.ambience.json` | the level's global light, post effect (fog, bloom, saturation, vignette) and placed lights (`tools/export_ambience.py`). |
| `textures/**/<name>.cube.png` | cube maps, six faces stacked top to bottom (+X -X +Y -Y +Z -Z), `tools/embed_textures.py`. |
| `maps/build_map.gd` | DSO_Godot's Godot-side builder; ignored here. |
| `logs/` | conversion logs and `export_report.json`. |

Every `.glb` has one scene (`Scene0`); the client loads it as
`GltfAssetLabel::Scene(0)`.

## Map manifest (`maps/<map>.map.json`)

```json
{
  "map": "a0200_kingscity",
  "size": [61, 11, 62],                 // the map's grid size, in cells
  "center": [121.5, 13.399, 71.3854],    // bounding-box centre, game frame
  "extents": [122.5, 22.399, 123.5315],  // bounding-box half extents
  "models": ["t001_hub/tile_wall_corner_02", "..."],
  "groups": [{"name": "...", "type": 0, "parent": -1}],
  "instances": [
    {"m": 0, "p": [174.0, 8.0, 6.0], "r": [0.0, 1.0, 0.0, 0.0],
     "s": [1.0, 1.0, 1.0], "n": "", "g": -1, "col": true}
  ],
  "nav_blockers": [[[x, z], "..."]]
}
```

- `models` is the map's list of distinct models, each a path relative to the
  asset root without extension: the model file is `<models[i]>.glb`.
- Each instance places `models[m]` at position `p`, rotation `r` (a unit
  quaternion, **x, y, z, w**) and scale `s`; the transform is
  `T(p) * R(r) * S(s)`, applied to the model's own glTF scene. `n` is the
  instance's name (often empty), `g` its group index (-1 = none), `col` whether
  it collides.
- Instances without graphics (pure collision / logic) are dropped by the export.
- There is **no separate terrain**: the ground of every map is made of tile
  models (`*/tile_ground_*`, `*/tile_wall_*`, `*/tile_ramp_*`) placed like any
  other prop.

The manifest comes from the map's `TMPL` (template) and `INST` (instance)
sections: `templates[instance.templateIndex].gfxResId` names the `.n3`.

## Coordinate frame

The map placements are in the game's map frame, which is also the server's
"description frame" (creature and spawn-point positions in `experimental`'s
`dsor/mapdata.py` are `(x, elevation, y)` in it): Nebula3's right-handed, Y-up
frame, one unit = one metre, ground on X/Z, elevation on Y. glTF and Bevy are
right-handed Y-up too, and DSO_Godot writes the models without any axis
conversion, so a game position is a Bevy position unchanged. The client keeps
that mapping in exactly one place, `client/src/map.rs::game_to_bevy`, so any
later correction lands there. Rotations and scales are used as-is.

## Running the map viewer

```sh
cargo run -p dsor-client --release -- a0200_kingscity      # Kingshill (default)
cargo run -p dsor-client --release -- a0001_start_tutorial_dun
cargo run -p dsor-client --release -- a0302_wildforest
# screenshot after loading, then exit:
cargo run -p dsor-client --release -- a0200_kingscity --screenshot
cargo run -p dsor-client --release -- a0200_kingscity --screenshot-to out.png --cam 120,60,130,120,10,70
```

On the web: `index.html?map=a0302_wildforest` (optionally `&cam=x,y,z,tx,ty,tz`).

## Known gaps of the 2018 conversion

See the run's `logs/`. With the 2018 client, every model, map and character
converted without a failure; what is missing is data the client itself does not
ship or features glTF cannot carry:

- 64 models are referenced by some map but absent from the 2018 data (mostly
  `m001_arena/*`, `e003_essence_desert/veg_grass_02`); those placements are
  skipped with a load error in the log.
- A couple of hundred textures referenced by models are absent from the data;
  those materials have no base-colour map.
- A few animation clips and mesh groups referenced by boss death models are
  missing from their `.nax3`/`.nvx2` files.
- What glTF cannot carry is put back by `tools/` after the conversion (step 5 of
  `convert_assets.sh`): Nebula render states and cull modes, emitters, shader
  animators, cube maps and second texture layers, the levels' light and
  post-effect settings. What the client draws of it, and what is still missing,
  is in `docs/fx.md`.
