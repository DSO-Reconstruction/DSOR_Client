#!/usr/bin/env python3
"""Copy each particle emitter of a model's .fx.json sidecar into its .glb.

Usage: embed_emitters.py <assets> [--dry-run]

The emitter's parameters land in the extras of the glTF node that carries the
emitter mesh (`gltf_node` in the sidecar), as `extras.dsor_emitter`. The client
reads them while the model loads (client/src/particles.rs), so a map needs no
extra file per effect. The JSON chunk is rewritten in place; the binary chunk is
untouched. Re-running replaces what an earlier run wrote.

It also hides helper surfaces: a `shd:standard` surface whose only colour is
`tex:system/white` (405 of them: dummies, and boxes such as the one inside
s03_deco_campfire_pot_01) drew as a plain white block. Their material gets
dsor_state Hidden, as tools/fix_materials.py does for effect surfaces.

And it gives every surface its Nebula render state (the sidecar's type_name)
instead of the exporter's guess: Solid / DecalReceiveSolid draw opaque,
AlphaTest / DecalReceiveAlphaTest cut out at 0.5. The exporter made any texture
with an alpha channel BLEND: blended surfaces write no depth, so a cape (Nebula:
AlphaTest) vanished behind blended foliage and ground decals drawn after it.
SEE: tools/fix_materials.py (same in-place rewrite), DSO_Godot fx_to_scenes.gd.
"""
import json, os, sys

sys.path.insert(0, os.path.dirname(__file__))
from fix_materials_io import read_glb, write_glb  # noqa: E402

assets = sys.argv[1]
dry = "--dry-run" in sys.argv

ENVELOPES = [
    "emission_frequency", "lifetime", "spread_min", "spread_max", "start_velocity",
    "rotation_velocity", "particle_size", "particle_mass", "time_manipulator",
    "velocity_factor", "air_resistance", "color_red", "color_green", "color_blue",
    "color_alpha",
]
SCALARS = [
    "emission_duration", "looping", "activity_distance", "render_oldest_first",
    "billboard", "start_rotation_min", "start_rotation_max", "gravity", "stretch",
    "texture_tile", "velocity_randomize", "rotation_randomize", "size_randomize",
    "precalc_time", "randomize_rotation", "start_delay",
]


def envelope(e):
    if not isinstance(e, dict):
        v = float(e or 0.0)
        return [v, v, v, v, 0.33, 0.66, 0.0, 0.0, 0.0]
    v = (list(e.get("values", [])) + [0.0] * 4)[:4]
    k = (list(e.get("keypos", [])) + [0.33, 0.66])[:2]
    return [float(x) for x in v + k + [e.get("freq", 0.0), e.get("amp", 0.0), e.get("mod", 0)]]


stats = {"emitters": 0, "helpers": 0, "opaque": 0, "cutout": 0, "files": 0, "skipped": 0}
OPAQUE = {"Solid", "DecalReceiveSolid"}
CUTOUT = {"AlphaTest", "DecalReceiveAlphaTest"}
for root, _dirs, files in os.walk(assets):
    for name in files:
        if not name.endswith(".fx.json"):
            continue
        fx_path = os.path.join(root, name)
        glb_path = fx_path[: -len(".fx.json")] + ".glb"
        if not os.path.exists(glb_path):
            continue
        try:
            fx = json.load(open(fx_path))
            doc, rest = read_glb(glb_path)
        except Exception:
            stats["skipped"] += 1
            continue
        nodes = doc.get("nodes", [])
        changed = False
        materials = {m.get("name"): m for m in doc.get("materials", [])}
        for e in fx.get("emitters", []):
            state = (e.get("emitter") or {}).get("type_name")
            shader = (e.get("shader") or "").removeprefix("shd:")
            m = materials.get(f"{e.get('node')}_{shader}")
            if m is not None and "dsor_state" not in m.get("extras", {}):
                if state in OPAQUE and m.get("alphaMode", "OPAQUE") != "OPAQUE":
                    m.pop("alphaMode", None)
                    m.pop("alphaCutoff", None)
                    stats["opaque"] += 1
                    changed = True
                elif state in CUTOUT and m.get("alphaMode") != "MASK":
                    m["alphaMode"] = "MASK"
                    m["alphaCutoff"] = 0.5
                    stats["cutout"] += 1
                    changed = True
            tex = e.get("textures") or {}
            if e.get("shader") == "shd:standard" and tex.get("DiffMap0") == "tex:system/white":
                m = materials.get(f"{e.get('node')}_standard")
                if m is not None and "baseColorTexture" not in m.get("pbrMetallicRoughness", {}):
                    m.setdefault("extras", {})["dsor_state"] = "Hidden"
                    stats["helpers"] += 1
                    changed = True
            em = e.get("emitter") or {}
            node = e.get("gltf_node")
            if "emission_frequency" not in em or node is None or node >= len(nodes):
                continue
            packed = {k: envelope(em.get(k)) for k in ENVELOPES}
            packed.update({k: float(em.get(k, 0.0)) for k in SCALARS})
            packed["additive"] = em.get("type_name") == "Additive"
            nodes[node].setdefault("extras", {})["dsor_emitter"] = packed
            stats["emitters"] += 1
            changed = True
        if changed:
            stats["files"] += 1
            if not dry:
                write_glb(glb_path, doc, rest)
print(stats)
