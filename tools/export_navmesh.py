#!/usr/bin/env python3
"""Export every map's walkable ground (PathEngine .tok) for the client.

Usage: export_navmesh.py <export_win32> <assets>   (EXPERIMENTAL env: the server repo)

Writes <assets>/maps/<map>.nav.bin: u32 triangle count, then per triangle 9 little-
endian f32 -- three corners (x, elevation, z) in the map frame, the frame of the
placement manifests. Read with the experimental server's own PathEngine reader
(dsor/navmesh.py), which swaps PathEngine's axes into the world's.
"""
import os, struct, sys
sys.path.insert(0, os.environ.get("EXPERIMENTAL", os.path.expanduser("~/Documents/experimental")))
from dsor import navmesh

src, assets = sys.argv[1], sys.argv[2]
root = os.path.join(src, "navigation")
out_dir = os.path.join(assets, "maps")
os.makedirs(out_dir, exist_ok=True)
done = 0
for name in sorted(os.listdir(root)):
    if not os.path.exists(os.path.join(root, name, f"{name}.tok")):
        continue
    mesh = navmesh.load(name, root)
    if mesh is None:
        continue
    with open(os.path.join(out_dir, f"{name}.nav.bin"), "wb") as f:
        f.write(struct.pack("<I", len(mesh.triangles)))
        for corners, _face in mesh.triangles:
            for x, ground, elevation in corners:
                f.write(struct.pack("<fff", x, elevation, ground))
    done += 1
print(f"{done} navigation meshes")
