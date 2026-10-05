#!/bin/sh
# Convert an unpacked 2018 Drakensang Online client into the assets this client
# loads (glTF models + PNG textures + map placement manifests), using DSO_Godot.
#
#   tools/convert_assets.sh <export_win32> <out_dir> [<DSO_Godot>]
#
#   <export_win32>  the client's unpacked data tree (models/, meshes/, maps/,
#                   textures/, anims/, ...). If you only have the packed client,
#                   run DSO_Godot's unpack.py first.
#   <out_dir>       where the converted assets go (~10 GB). Keep it OUTSIDE any
#                   git-tracked path: the game data is proprietary.
#   <DSO_Godot>     a checkout of https://github.com/DSO-Reconstruction/DSO_Godot
#                   (cloned into <out_dir>/.DSO_Godot when omitted).
#
# Afterwards point the client at it:  ln -sfn <out_dir> <workspace>/assets
#
# Needs python3 with Pillow and numpy, a C++ compiler and curl (the .crn
# transcoder), git (only when cloning DSO_Godot). Logs go to <out_dir>/logs/.
set -eu

if [ $# -lt 2 ]; then
    sed -n '2,18p' "$0"; exit 2
fi
root=$(cd "$1" && pwd)
mkdir -p "$2"
out=$(cd "$2" && pwd)
godot=${3:-$out/.DSO_Godot}
logs=$out/logs
mkdir -p "$logs"

[ -d "$root/models" ] && [ -d "$root/maps" ] || {
    echo "error: $root does not look like an unpacked export_win32 tree" >&2; exit 1; }

if [ ! -f "$godot/export_model.py" ]; then
    git clone --depth 1 https://github.com/DSO-Reconstruction/DSO_Godot "$godot"
fi
python3 -c "import PIL, numpy" || {
    echo "error: python3 needs Pillow and numpy (pip install -r $godot/requirements.txt)" >&2; exit 1; }

# 1. The Crunch (.crn) texture transcoder; without it ~2,600 textures are skipped.
[ -x "$godot/build/crn2dds" ] || sh "$godot/tools/build_crn2dds.sh"

cd "$godot"

# 2. Every model -> <out>/<models subpath>.glb, textures -> <out>/textures/**.png.
#    The two player-character skeletons are split separately (step 3).
echo "[convert] models (log: $logs/export_models.log)"
python3 batch_export.py --root "$root" --out "$out" \
    --skip uniskel uniskel_dwarf --report "$logs/export_report.json" \
    > "$logs/export_models.log" 2>&1
tail -n 12 "$logs/export_models.log"

# 3. Player characters: skeleton + clips, one .glb per part, outfits.json.
for c in uniskel uniskel_dwarf; do
    if [ -f "$root/models/characters/$c.n3" ]; then
        echo "[convert] character $c (log: $logs/split_$c.log)"
        python3 split_character.py "$root/models/characters/$c.n3" \
            --root "$root" --export-root "$out" > "$logs/split_$c.log" 2>&1
        tail -n 4 "$logs/split_$c.log"
    fi
done

# 4. Maps -> <out>/maps/<map>.map.json placement manifests.
echo "[convert] maps (log: $logs/export_maps.log)"
python3 export_maps.py --root "$root" --out "$out/maps" > "$logs/export_maps.log" 2>&1
tail -n 6 "$logs/export_maps.log"

du -sh "$out"
echo "[convert] done -> $out"
