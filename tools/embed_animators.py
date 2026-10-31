#!/usr/bin/env python3
"""Copy the shader-variable animations of each .n3 model into its .glb.

Usage: embed_animators.py <export_win32/models> <assets> [model/path ...]

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
applies them (client/src/rendering/materials.rs, ShaderAnim).
"""
import os, struct, sys

sys.path.insert(0, os.path.dirname(__file__))
from fix_materials_io import read_glb, write_glb  # noqa: E402

models, assets = sys.argv[1], sys.argv[2]
# Optional model paths allow repairing a small set without rewriting the pack.
only = {p.replace("\\", "/").removesuffix(".n3").removesuffix(".glb") for p in sys.argv[3:]}


def string(d, i):
    n = struct.unpack_from("<H", d, i)[0]
    return d[i + 2:i + 2 + n].decode("latin1"), i + 2 + n


def sprite_nodes(d):
    """Names of transform nodes flagged as sprites (n3 tag "RPSS" = 1) or billboards
    ("BLBS" = 1): Nebula turns
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
        # A billboard ("BLBS" = 1, SBLB read backwards) faces the viewer too.
        # EVIDENCE: t000_general/exit_general_colors_01 pPlane2 carries it; its
        # child is a 3.4 x 14 vertical glow (p_glow_4_blue), the light over an
        # exit arrow -- drawn unturned it showed as a flat plate from the side
        # ("y'a un genre de plane sur le FX").
        k = d.find(b"BLBS", i, end)
        if (j >= 0 and d[j + 4] == 1) or (k >= 0 and d[k + 4] == 1):
            out.add(name)
        at = i


def billboard_nodes(d):
    """Names of transform nodes flagged as billboards ("BLBS" = 1) and not as
    sprites ("RPSS"): they turn to the viewer about the vertical axis only
    (client FacesViewerUpright). EVIDENCE: three of them looked right only so:
    the mage's Singularity rings (flat, 2026-10-21), the exit glow (a vertical
    plane that must face the camera's heading) and the Manticore's summoning fire
    vortex (effects/mount_manticore_summon_01 moveMeBillboard_1_1: turned fully to
    the camera its helices lay flat on the ground, 2026-10-25; the user's
    release261_192 recording shows an upright swirl).
    UNVERIFIED: the engine's own code (no Nebula source at hand)."""
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
        k = d.find(b"BLBS", i, end)
        if k >= 0 and d[k + 4] == 1 and not (j >= 0 and d[j + 4] == 1):
            out.add(name)
        at = i


def flat_subtree(doc, nodes, start):
    """Whether the meshes under a node lie flat: their height (local Y) under a
    fifth of their narrower width (X, Z). None without meshes."""
    lo, hi = [1e30] * 3, [-1e30] * 3
    stack, seen = [start], False
    while stack:
        nd = nodes[stack.pop()]
        stack.extend(nd.get("children", []))
        if "mesh" not in nd:
            continue
        for prim in doc["meshes"][nd["mesh"]].get("primitives", []):
            acc = doc["accessors"][prim["attributes"]["POSITION"]]
            if "min" in acc and "max" in acc:
                seen = True
                lo = [min(a, b) for a, b in zip(lo, acc["min"])]
                hi = [max(a, b) for a, b in zip(hi, acc["max"])]
    if not seen:
        return None
    x, y, z = (b - a for a, b in zip(lo, hi))
    return y < 0.2 * min(x, z)


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


def transform_clips(d, models):
    """[(node path, period s, cycles)]: the nax3 clip each transform animator plays.

    n3 layout: ONNA path, INAS u16 len "ani:<anim>.nax3", TPLS u16 len "loop"|"clamp",
    RGAS u32 clip index. nax3: "0HAN", u32 clips, u32 keys, then per clip u16
    curves, start key, keys, key stride, key duration (ms), u8 pre and post
    infinity (0 constant, 1 cycle), u16 events, 50 byte name, curves x 24 bytes.
    EVIDENCE: every n3 transform animator says "loop", but the clips differ:
      all_drop_bag_01 item_spawn (0.44 s) post 0, item_rotate post 1 (the drop
      pops once, then turns); groupskill_frostnova_aura both post 0 (bursts
      once); mount_manticore_summon_01 null1 (20 keys x 40 ms) post 1, the
      helices' growth (2.04-4.04 s) post 0.
    """
    out = []
    cache = {}
    at = 0
    while True:
        at = d.find(b"INAS", at)
        if at < 0:
            return out
        try:
            ani, i = string(d, at + 4)
            start = d.rfind(b"ONNA", 0, at)
            if start < 0 or not ani.startswith("ani:") or d[i:i + 4] != b"TPLS":
                at += 4
                continue
            path, _ = string(d, start + 4)
            _loop, i = string(d, i + 4)
            group = struct.unpack_from("<I", d, i + 4)[0] if d[i:i + 4] == b"RGAS" else 0
            if ani not in cache:
                try:
                    n = open(os.path.join(models, "..", "anims", ani[4:].removesuffix(".nax3") + ".nax3"), "rb").read()
                except OSError:
                    n = b""
                clips = []
                if n[:4] == b"0HAN":
                    o = 12
                    for _ in range(struct.unpack_from("<I", n, 4)[0]):
                        curves, _first, keys, _stride, ms = struct.unpack_from("<HHHHH", n, o)
                        clips.append((keys * ms / 1000.0, n[o + 11] == 1))
                        o += 64 + 24 * curves
                cache[ani] = clips
            if group < len(cache[ani]):
                out.append((path, *cache[ani][group]))
        except struct.error:
            pass
        at += 4



def merged_decal(doc, m):
    """{color, mask} texture indices of a decal whose base colour is the merged
    <colour>__<mask>.png (fix_materials.py), or None."""
    base = m.get("pbrMetallicRoughness", {}).get("baseColorTexture")
    images, textures = doc.get("images", []), doc.get("textures", [])
    if base is None:
        return None
    merged = images[textures[base["index"]]["source"]]["uri"]
    stem, _, mask_name = os.path.splitext(merged)[0].rpartition("__")
    if not stem:
        return None
    find = lambda tail: next((i for i, t in enumerate(textures)
                              if os.path.splitext(images[t["source"]]["uri"])[0].endswith(tail)), None)
    color, mask = find(os.path.basename(stem)), find(mask_name)
    return None if color is None or mask is None else {"color": color, "mask": mask}

stats = {"models": 0, "animated nodes": 0, "unmatched": 0}
for root, _dirs, files in os.walk(models):
    for name in files:
        if not name.endswith(".n3"):
            continue
        n3 = os.path.join(root, name)
        rel = os.path.relpath(n3, models)[:-3]
        if only and rel.replace("\\", "/") not in only:
            continue
        glb = os.path.join(assets, rel + ".glb")
        if not os.path.exists(glb):
            continue
        raw = open(n3, "rb").read()
        anims = animators(raw)
        clips = transform_clips(raw, models)
        floats = shader_floats(raw)
        sprites = sprite_nodes(raw)
        billboards = billboard_nodes(raw)
        uvs = uv_animators(raw)
        vectors = shader_vectors(raw)
        states = node_states(raw)
        if not anims and not floats and not sprites and not uvs and not vectors and not states and not clips:
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
        # restore_pivots inserts transform helpers. Match the authored parent
        # when this postprocessor is rerun on already repaired models.
        for child, up in list(parent.items()):
            while nodes[up].get("extras", {}).get("dsor_pivot_helper") and up in parent:
                up = parent[up]
            parent[child] = up
        for nd in nodes:
            nd.get("extras", {}).pop("dsor_anim", None)
            nd.get("extras", {}).pop("dsor_shader", None)
            nd.get("extras", {}).pop("dsor_sprite", None)
            nd.get("extras", {}).pop("dsor_sprite_upright", None)
            nd.get("extras", {}).pop("dsor_uvanim", None)
            nd.get("extras", {}).pop("dsor_vector", None)
            nd.get("extras", {}).pop("dsor_pass", None)
            nd.get("extras", {}).pop("dsor_ints", None)
            nd.get("extras", {}).pop("dsor_clip", None)
            if nd.get("name") in sprites:
                nd.setdefault("extras", {})["dsor_sprite"] = True
                stats["sprite nodes"] = stats.get("sprite nodes", 0) + 1
        # A billboard over geometry lying flat (rings, ground circles) turns to
        # the viewer about the vertical axis only (client FacesViewerUpright).
        # FAILURE: turned fully to the camera, the mage's Singularity rings
        #   (mage_arcanevortex_loop rotating_vortex, 31 wide by 2 high) stood
        #   tilted ("la singularite est toujours inclinee a 70 degres",
        #   2026-10-21).
        # UNVERIFIED: how the 2018 engine orients such a billboard (no Nebula
        #   source at hand); a quad sprite (upright geometry) is unchanged.
        for i, nd in enumerate(nodes):
            if nd.get("extras", {}).get("dsor_sprite") and (nd.get("name") in billboards or flat_subtree(doc, nodes, i)):
                nd["extras"]["dsor_sprite_upright"] = True
                stats["upright sprites"] = stats.get("upright sprites", 0) + 1
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
        # What hangs under a sprite turned fully to the viewer (client FacesViewer)
        # is drawn from both sides: turned that way, its planes show the camera
        # their back, and their single-sided materials culled them.
        # EVIDENCE: of the 1214 single-sided sprite surfaces in the release261
        #   models, 1193 faced away from the camera as the client turns them and
        #   16 towards it. dropitems/glow_magic blue_pPlane2 (the big blue glow
        #   over a magic drop in the original) never showed; drawn from both
        #   sides it is the original's glow.
        # UNVERIFIED: why the original shows their front (its sprite turn is
        #   likely mirrored from the client's); a mirror would also swap which of
        #   two stacked planes is nearer.
        def sprite_meshes(i):
            nd = nodes[i]
            if "mesh" in nd and "dsor_emitter" not in nd.get("extras", {}):
                yield nd["mesh"]
            for c in nd.get("children", []):
                yield from sprite_meshes(c)
        for i, nd in enumerate(nodes):
            ex = nd.get("extras", {})
            if not ex.get("dsor_sprite") or ex.get("dsor_sprite_upright"):
                continue
            for mesh in set(sprite_meshes(i)):
                for prim in doc["meshes"][mesh].get("primitives", []):
                    m = doc["materials"][prim["material"]] if "material" in prim else None
                    if m is not None and not m.get("doubleSided", False):
                        m["doubleSided"] = True
                        stats["sprite sides"] = stats.get("sprite sides", 0) + 1
                        shaded += 1
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
        # How each animated node's clip runs past its end (client
        # skills::clip_graph): a cycling one repeats on its own period, the
        # others play once and hold, as the nax3 says. A pivot helper of the
        # node carries part of its curves, so it gets the same.
        for path, period, cycles in clips:
            parts = path.split("/")
            leaf, up = parts[-1], (parts[-2] if len(parts) > 1 else None)
            cands = [i for i, nd in enumerate(nodes) if nd.get("name") == leaf]
            if len(cands) > 1 and up:
                cands = [i for i in cands if parent.get(i) is not None and nodes[parent[i]].get("name") == up] or cands
            if not cands:
                continue
            clip = {"period": round(period, 4), "cycle": cycles}
            for i, nd in enumerate(nodes):
                if i == cands[0] or (nd.get("extras", {}).get("dsor_pivot_helper") and nd.get("name", "").startswith(f"__dsor_pivot_{cands[0]}_")):
                    nd.setdefault("extras", {})["dsor_clip"] = clip
            stats["clip nodes"] = stats.get("clip nodes", 0) + 1
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
        # Every node filter of a b_alpha batch in frame/default.xml, lit or unlit
        # (lines 70, 96-101, 103, 105-107).
        # FAILURE: AlphaBgLit and AlphaLit were left out: Balor's fire breath
        #   (boss_07_balor_firebreath firewall, uvanimated, fire_big_02 whose alpha
        #   is 255 throughout) drew as a solid violet fan 140 units wide instead
        #   of a fire fading out by its vertex alpha ("une sorte de cone", 2026-10-24).
        ALPHA_PASSES = {"AlphaUnlit", "PostAlphaUnlit", "PreAlphaUnlit", "Alpha", "AlphaUnlitDepthtest", "ParticleLit",
                        "AlphaBgLit", "AlphaLit", "AlphaLitOnlyNormal", "AlphaLitOnlyNormalNoZWrite",
                        "DecalReceiveAlphaLit", "SequenceAlpha"}
        for m in doc.get("materials", []):
            m.get("extras", {}).pop("dsor_decal_effect", None)
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
                # A node of an alpha batch (frame/default.xml: b_alpha, SrcAlpha /
                # InvSrcAlpha) is blended even when its texture has no alpha
                # channel (DSO_Godot then exports it OPAQUE): its opacity is the
                # vertex alpha x mayaAnimableAlpha (Intensity0), animated or not.
                # mage_arcanevortex_start's smoke_tileable_noalpha ring, invisible
                # until 0.76 s, showed as a solid disc from the first frame.
                # An effect's decal (DecalEffect batch) is not the map's Decal: its
                # opacity is the colour map's own alpha x mayaAnimableAlpha, and
                # its second map is added light, not a mask.
                # EVIDENCE: shaders_sm30 "decal" DecalEffect ps_3_0: rgb = light x
                #   effectDecalColor x 2 + decalAlphaMask x MatEmissiveIntensity,
                #   a = effectDecalColor.a x mayaAnimableAlpha. Merged as the map's
                #   decals (mask -> alpha), pw019_lightning_field_aura's flat yellow
                #   "mask" filled its whole box: a dark square under lightningstrike.
                dec = mex.get("dsor_decal") or merged_decal(doc, m)
                if ex.get("dsor_pass") == "DecalEffect" and mex.get("dsor_state") == "Decal" and dec:
                    mex["dsor_decal_effect"] = {"color": dec["color"], "glow": dec["mask"],
                                                "intensity": ex.get("dsor_shader", {}).get("MatEmissiveIntensity", 1.0)}
                    shaded += 1
                if ex.get("dsor_pass") in ALPHA_PASSES and m.get("alphaMode", "OPAQUE") == "OPAQUE" \
                        and shader in ALPHA_SHADERS and mex.get("dsor_state") is None:
                    m["alphaMode"] = "BLEND"
                    shaded += 1
        if by_node or shaded or sprites:
            write_glb(glb, doc, rest)
            stats["models"] += 1
print(stats)
