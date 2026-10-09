#!/usr/bin/env python3
"""What the debug menu (client/src/debug.rs, F3) lists, and the debug map.

Usage: export_debug.py <assets>

Writes:
  debug_index.json   {"models": [...]} every converted model (asset path without
                     .glb), the interface, textures and the player skeletons'
                     parts left out; effects are the ones under effects*/
  maps/debug.map.json  an empty level: a 12 x 12 floor of Kingshill's ground tile
                     (t001_hub/tile_ground_plain_01, its own size measured from the
                     glb), at height 0, no NPCs, no exits, no navigation mesh (the
                     client walks on the plane at the player's feet)
"""
import json, os, struct, sys

assets = sys.argv[1]

models = []
for root, dirs, files in os.walk(assets, followlinks=True):
    rel = os.path.relpath(root, assets)
    top = rel.split(os.sep)[0]
    if top in ("textures", "interface", "maps", "logs", "fonts", "skills") or "/parts" in rel.replace(os.sep, "/"):
        dirs[:] = []
        continue
    for name in files:
        if name.endswith(".glb") and not name.startswith("__"):
            models.append(os.path.join(rel, name[:-4]).replace(os.sep, "/").lstrip("./"))
models.sort()
with open(os.path.join(assets, "debug_index.json"), "w") as f:
    json.dump({"models": models}, f, separators=(",", ":"))

TILE = "t001_hub/tile_ground_plain_01"


def extent(path):
    """The model's x and z size, from its POSITION accessors' bounds."""
    b = open(path, "rb").read()
    n = struct.unpack_from("<I", b, 12)[0]
    doc = json.loads(b[20:20 + n])
    lo, hi = [1e9] * 3, [-1e9] * 3
    for mesh in doc.get("meshes", []):
        for p in mesh.get("primitives", []):
            a = doc["accessors"][p["attributes"]["POSITION"]]
            lo = [min(x, y) for x, y in zip(lo, a["min"])]
            hi = [max(x, y) for x, y in zip(hi, a["max"])]
    return hi[0] - lo[0], hi[2] - lo[2]


sx, sz = extent(os.path.join(assets, TILE + ".glb"))
N = 12
instances = []
for i in range(N):
    for j in range(N):
        instances.append({"m": 0, "p": [i * sx, 0.0, j * sz], "r": [0.0, 0.0, 0.0, 1.0], "s": [1.0, 1.0, 1.0],
                          "n": "", "g": -1, "col": True})
half = [N * sx / 2, 1.0, N * sz / 2]
manifest = {"map": "debug", "size": [N, 1, N], "center": [half[0] - sx / 2, 0.0, half[2] - sz / 2],
            "extents": half, "models": [TILE], "groups": [], "instances": instances, "nav_blockers": []}
with open(os.path.join(assets, "maps", "debug.map.json"), "w") as f:
    json.dump(manifest, f)
print({"models": len(models), "tile": [sx, sz], "debug map": f"{N}x{N}"})
