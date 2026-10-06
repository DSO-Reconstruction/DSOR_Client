#!/usr/bin/env python3
"""Copy the shader-variable animations of each .n3 model into its .glb.

Usage: embed_animators.py <export_win32/models> <assets>

Nebula animates material variables over time with "animator" nodes; glTF has no
place for them, so DSO_Godot dropped them, and effects stayed lit for as long as
they lived ("l'effet reste trop longtemps, il devrait baisser l'opacite").

n3 layout of one animated variable (FourCCs are stored reversed):
  ESAB u32                 block
  ONNA u16 len, path       the animated node, "model/<group>/.../static_0_0"
  TPLS u16 len, "loop"|"clamp"
  MNPS u16 len, variable   MatEmissiveIntensity, Intensity0, Intensity1, ...
  KDDA u16 len, "Float", u32 count, count x (f32 time, f32 value)
EVIDENCE: 6 351 of 6 660 key lists in the 2018 models read with strictly ordered
  times this way (mage_fireball_charm bullet_glow1: 1 -> 0.1 -> 1 -> 0 over 0.72 s).

It also copies each node's static shader floats (n3 "TLFS": u16 len, name, f32),
MatEmissiveIntensity above all, into extras.dsor_shader = {name: value}.
EVIDENCE: the particle pixel shader (shaders_sm30, effect "particle", ps_3_0 at
  +22592, disassembled): colour = DiffMap0 x particle colour x (1 + c3), c3 being
  MatEmissiveIntensity -- particles were drawn without it, dim on every skill.

Each glb node named by the path's last component (its parent's name breaking ties)
gets extras.dsor_anim = [{"var", "loop", "keys": [[t, v], ...]}]; the client
applies them (client/src/materials.rs, ShaderAnim).
"""
import os, struct, sys

sys.path.insert(0, os.path.dirname(__file__))
from fix_materials_io import read_glb, write_glb  # noqa: E402

models, assets = sys.argv[1], sys.argv[2]


def string(d, i):
    n = struct.unpack_from("<H", d, i)[0]
    return d[i + 2:i + 2 + n].decode("latin1"), i + 2 + n


def shader_floats(d):
    """[(node name, {var: value})] in file order: each node's TLFS params."""
    out = []
    at = 0
    while True:
        at = d.find(b"DNM>", at)
        if at < 0:
            return out
        try:
            name, i = string(d, at + 8)
        except (struct.error, UnicodeDecodeError):
            at += 4
            continue
        end = d.find(b"DNM", i)
        end = len(d) if end < 0 else end
        params = {}
        j = i
        while True:
            j = d.find(b"TLFS", j, end)
            if j < 0:
                break
            try:
                var, k = string(d, j + 4)
                params[var] = round(struct.unpack_from("<f", d, k)[0], 4)
            except (struct.error, UnicodeDecodeError):
                pass
            j += 4
        if params:
            out.append((name, params))
        at = i


def animators(d):
    out = []
    at = 0
    while True:
        at = d.find(b"ONNA", at)
        if at < 0:
            return out
        try:
            path, i = string(d, at + 4)
            if d[i:i + 4] != b"TPLS":
                at += 4
                continue
            loop, i = string(d, i + 4)
            if d[i:i + 4] != b"MNPS":
                at += 4
                continue
            var, i = string(d, i + 4)
            if d[i:i + 4] != b"KDDA":
                at += 4
                continue
            kind, i = string(d, i + 4)
            if kind != "Float":
                at += 4
                continue
            count = struct.unpack_from("<I", d, i)[0]
            i += 4
            if not 0 < count < 1000:
                at += 4
                continue
            flat = struct.unpack_from("<%df" % (2 * count), d, i)
            keys = [[round(flat[2 * k], 4), round(flat[2 * k + 1], 4)] for k in range(count)]
            out.append({"path": path, "var": var, "loop": loop, "keys": keys})
            at = i + 8 * count
        except struct.error:
            at += 4


stats = {"models": 0, "animated nodes": 0, "unmatched": 0}
for root, _dirs, files in os.walk(models):
    for name in files:
        if not name.endswith(".n3"):
            continue
        n3 = os.path.join(root, name)
        rel = os.path.relpath(n3, models)[:-3]
        glb = os.path.join(assets, rel + ".glb")
        if not os.path.exists(glb):
            continue
        raw = open(n3, "rb").read()
        anims = animators(raw)
        floats = shader_floats(raw)
        if not anims and not floats:
            continue
        try:
            doc, rest = read_glb(glb)
        except Exception:
            continue
        nodes = doc.get("nodes", [])
        parent = {}
        for i, nd in enumerate(nodes):
            for c in nd.get("children", []):
                parent[c] = i
        for nd in nodes:
            nd.get("extras", {}).pop("dsor_anim", None)
            nd.get("extras", {}).pop("dsor_shader", None)
        shaded = 0
        for name, params in floats:
            for nd in nodes:
                if nd.get("name") == name:
                    nd.setdefault("extras", {})["dsor_shader"] = params
                    shaded += 1
        stats["shader nodes"] = stats.get("shader nodes", 0) + shaded
        by_node = {}
        for a in anims:
            parts = a["path"].split("/")
            leaf = parts[-1]
            up = parts[-2] if len(parts) > 1 else None
            cands = [i for i, nd in enumerate(nodes) if nd.get("name") == leaf]
            if len(cands) > 1 and up:
                narrowed = [i for i in cands if parent.get(i) is not None and nodes[parent[i]].get("name") == up]
                cands = narrowed or cands
            if not cands:
                stats["unmatched"] += 1
                continue
            by_node.setdefault(cands[0], []).append({"var": a["var"], "loop": a["loop"], "keys": a["keys"]})
        for i, lst in by_node.items():
            nodes[i].setdefault("extras", {})["dsor_anim"] = lst
            stats["animated nodes"] += 1
        if by_node or shaded:
            write_glb(glb, doc, rest)
            stats["models"] += 1
print(stats)
