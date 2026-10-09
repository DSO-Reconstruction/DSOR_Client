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


def sprite_nodes(d):
    """Names of transform nodes flagged as sprites (n3 tag "RPSS" = 1): Nebula turns
    them to face the viewer. EVIDENCE: mage_frostnova auraSprite carries it, its
    plane child sits 4.7 units down its -Z; drawn unturned, the nova's aura stood
    beside the caster instead of over them."""
    out = set()
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
        j = d.find(b"RPSS", i, end)
        if j >= 0 and d[j + 4] == 1:
            out.add(name)
        at = i


UV_KEYS = {b"KPDA": "pos", b"KSDA": "scale", b"KEDA": "rot"}


def uv_animators(d):
    """[{path, loop, pos/scale/rot: [[t, x, y], ...]}]: n3 UV animators (texture
    layer offset / scale / rotation keys: u32 count, then count x (u32 layer, f32
    time, float4)). EVIDENCE: e_ks_infested_ranger_markshot_bullet static_0_3 (a
    uvanimated2 swoosh): KPDA (0,0) at 0 s -> (1,0) at 0.24 s, looping; the
    uvanimated2 vertex shader turns (u, v, 1) by its uvTransform rows (c6, c7)."""
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
            entry = {"path": path, "loop": loop}
            while d[i:i + 4] in UV_KEYS:
                kind = UV_KEYS[d[i:i + 4]]
                count = struct.unpack_from("<I", d, i + 4)[0]
                i += 8
                keys = []
                for _ in range(count):
                    _layer, t, x, y, z, _w = struct.unpack_from("<Iffff f".replace(" ", ""), d, i)
                    keys.append([round(t, 4), round(x, 4), round(y, 4), round(z, 4)])
                    i += 24
                entry[kind] = keys
            if len(entry) > 2:
                out.append(entry)
            at = i
        except struct.error:
            at += 4


def shader_vectors(d):
    """[(node name, {var: [x, y, z, w]})]: each node's n3 "CEVS" shader vectors
    (Velocity: the uvanimated shader's texture scroll)."""
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
        vecs = {}
        j = i
        while True:
            j = d.find(b"CEVS", j, end)
            if j < 0:
                break
            try:
                var, k = string(d, j + 4)
                vecs[var] = [round(x, 4) for x in struct.unpack_from("<4f", d, k)]
            except (struct.error, UnicodeDecodeError):
                pass
            j += 4
        if vecs:
            out.append((name, vecs))
        at = i


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


def node_states(d):
    """{node name: {"pass": PTNM, "ints": {var: value}}}: each shape node's Nebula
    render state (the frame shader's node filter: Solid, AlphaTest, Additive...)
    and its integer shader params (n3 "TNIS": u16 len, name, i32) -- CullMode
    (1 none, 2 back faces: D3DCULL_CW with Nebula's counter-clockwise fronts),
    AlphaRef (the particle shader's numAnimPhases).
    EVIDENCE: 16 004 standard Solid and 1 578 AlphaTest surfaces of the 2018 models
      carry CullMode 2; DSO_Godot wrote every material double-sided, so leaves
      modelled with a reversed copy of each face drew both copies over each other."""
    out = {}
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
        ints, state = {}, None
        j = i
        while True:
            j = d.find(b"TNIS", j, end)
            if j < 0:
                break
            try:
                var, k = string(d, j + 4)
                ints[var] = struct.unpack_from("<i", d, k)[0]
            except (struct.error, UnicodeDecodeError):
                pass
            j += 4
        j = d.find(b"PTNM", i, end)
        if j >= 0:
            try:
                state = string(d, j + 4)[0]
            except (struct.error, UnicodeDecodeError):
                state = None
        if state or ints:
            out[name] = {"pass": state, "ints": ints}
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
        sprites = sprite_nodes(raw)
        uvs = uv_animators(raw)
        vectors = shader_vectors(raw)
        states = node_states(raw)
        if not anims and not floats and not sprites and not uvs and not vectors and not states:
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
            nd.get("extras", {}).pop("dsor_sprite", None)
            nd.get("extras", {}).pop("dsor_uvanim", None)
            nd.get("extras", {}).pop("dsor_vector", None)
            nd.get("extras", {}).pop("dsor_pass", None)
            nd.get("extras", {}).pop("dsor_ints", None)
            if nd.get("name") in sprites:
                nd.setdefault("extras", {})["dsor_sprite"] = True
                stats["sprite nodes"] = stats.get("sprite nodes", 0) + 1
        shaded = 0
        for name, params in floats:
            for nd in nodes:
                if nd.get("name") == name:
                    nd.setdefault("extras", {})["dsor_shader"] = params
                    shaded += 1
        stats["shader nodes"] = stats.get("shader nodes", 0) + shaded
        # The render state of each surface, and its faces: a material is drawn
        # single-sided when every node using it culls back faces (CullMode 2).
        culls = {}
        for nd in nodes:
            st = states.get(nd.get("name"))
            if not st:
                continue
            ex = nd.setdefault("extras", {})
            if st["pass"]:
                ex["dsor_pass"] = st["pass"]
            if st["ints"]:
                ex["dsor_ints"] = st["ints"]
            if "mesh" in nd:
                for prim in doc["meshes"][nd["mesh"]].get("primitives", []):
                    if "material" in prim:
                        culls.setdefault(prim["material"], set()).add(st["ints"].get("CullMode", 2))
            shaded += 1
        for mi, cs in culls.items():
            single = cs == {2}
            m = doc["materials"][mi]
            if m.get("doubleSided", False) == single:
                m["doubleSided"] = not single
                stats["single-sided" if single else "double-sided"] = stats.get("single-sided" if single else "double-sided", 0) + 1
        for name, vecs in vectors:
            for nd in nodes:
                if nd.get("name") == name and any(any(v) for v in vecs.values()):
                    nd.setdefault("extras", {})["dsor_vector"] = vecs
                    shaded += 1
        for u in uvs:
            parts = u["path"].split("/")
            leaf, up = parts[-1], (parts[-2] if len(parts) > 1 else None)
            cands = [i for i, nd in enumerate(nodes) if nd.get("name") == leaf]
            if len(cands) > 1 and up:
                cands = [i for i in cands if parent.get(i) is not None and nodes[parent[i]].get("name") == up] or cands
            if cands:
                nodes[cands[0]].setdefault("extras", {})["dsor_uvanim"] = {k: v for k, v in u.items() if k != "path"}
                stats["uv animated nodes"] = stats.get("uv animated nodes", 0) + 1
                shaded += 1
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
        # What a material needs from its node, where both client paths (scene and
        # flat map surfaces) can read it: the static alpha factor (Intensity0,
        # `mayaAnimableAlpha` of standard / unlit / uvanimated / decal: alpha x it)
        # and the uvanimated texture scroll (Velocity, `uvVelocity`: uv + it x t).
        # EVIDENCE: shaders_sm30 "uvanimated" AlphaUnlit: vs uv + uvVelocity x
        #   time, ps alpha x alphaBlendFactor x mayaAnimableAlpha (preshader c1).
        ALPHA_SHADERS = {"shd:standard", "shd:unlit", "shd:uvanimated", "shd:uvanimated2", "shd:decal", "shd:simplelayer"}
        SCROLL_SHADERS = {"shd:uvanimated", "shd:uvanimated2"}
        for m in doc.get("materials", []):
            if m.get("extras", {}).pop("dsor_alpha", None) is not None or m.get("extras", {}).pop("dsor_scroll", None) is not None:
                shaded += 1
        for i, nd in enumerate(nodes):
            ex = nd.get("extras", {})
            if "mesh" not in nd:
                continue
            anims = {a["var"] for a in by_node.get(i, [])}
            alpha = ex.get("dsor_shader", {}).get("Intensity0")
            vel = ex.get("dsor_vector", {}).get("Velocity", [0, 0])[:2]
            for prim in doc["meshes"][nd["mesh"]].get("primitives", []):
                if "material" not in prim:
                    continue
                m = doc["materials"][prim["material"]]
                mex = m.setdefault("extras", {})
                shader = mex.get("nebula_shader")
                if shader in ALPHA_SHADERS and alpha is not None and abs(alpha - 1.0) > 1e-3 and "Intensity0" not in anims:
                    mex["dsor_alpha"] = alpha
                    shaded += 1
                if shader in SCROLL_SHADERS and any(abs(v) > 1e-6 for v in vel) and "dsor_uvanim" not in ex and not anims:
                    mex["dsor_scroll"] = vel
                    shaded += 1
        if by_node or shaded or sprites:
            write_glb(glb, doc, rest)
            stats["models"] += 1
print(stats)
