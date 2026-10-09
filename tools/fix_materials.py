#!/usr/bin/env python3
"""Put back, in the converted .glb files, what glTF could not carry from Nebula.

Usage: fix_materials.py <assets> [--dry-run]

Rewrites each .glb's JSON chunk in place (the binary chunk is untouched; images
are external) and marks every rewritten material with extras.dsor_state, which
the client turns into the matching render state (client/src/materials.rs):

  Decal     shd:decal -- DiffMap0 (base colour) is the artwork and the mask was
            exported as the EMISSIVE texture, so drawn as glTF it added white over
            its whole quad ("gros carres blancs") and covered the ground with its
            full square. Colour and mask are merged into one RGBA texture
            (<colour>__<mask>.png, mask -> alpha) and the emission is dropped.
  Additive  a glow: an emissive texture over no base colour (Nebula's additive
            pass adds EmsvMap0). The emission becomes the base colour; the client
            draws it additive and unlit.
  Hidden    particle emitter surfaces: not artwork, the particles are rebuilt
            from the .fx.json sidecars (client/src/particles.rs). Volume fog and
            refraction surfaces carry it too, so no StandardMaterial draws them:
            the client gives them materials of its own (client/src/surfaces.rs,
            refraction.rs).

Idempotent: a material already carrying dsor_state is left alone.
SEE: DSO_Godot docs/FORMATS.md "Shaders", godot/fx_to_scenes.gd.
"""
import json, os, struct, sys

from PIL import Image

sys.path.insert(0, os.path.dirname(__file__))
from fix_materials_io import read_glb, write_glb  # noqa: E402

assets = sys.argv[1]
dry = "--dry-run" in sys.argv
HIDDEN = {"shd:volumefog", "shd:refraction", "shd:particle"}


merged_cache = {}


def merge(model_dir, color_uri, mask_uri):
    """Colour RGB + mask (luminance) as alpha -> a new PNG beside the colour."""
    key = (color_uri, mask_uri)
    if key in merged_cache:
        return merged_cache[key]
    color_path = os.path.normpath(os.path.join(model_dir, color_uri))
    mask_path = os.path.normpath(os.path.join(model_dir, mask_uri))
    base = os.path.splitext(os.path.basename(color_path))[0]
    mname = os.path.splitext(os.path.basename(mask_path))[0]
    out_path = os.path.join(os.path.dirname(color_path), f"{base}__{mname}.png")
    if not os.path.exists(out_path):
        color = Image.open(color_path).convert("RGB")
        mask = Image.open(mask_path).convert("L")
        if mask.size != color.size:
            mask = mask.resize(color.size, Image.BILINEAR)
        rgba = color.copy()
        rgba.putalpha(mask)
        if not dry:
            rgba.save(out_path, optimize=False)
    uri = os.path.relpath(out_path, model_dir).replace(os.sep, "/")
    merged_cache[key] = uri
    return uri


stats = {"Decal": 0, "Additive": 0, "Hidden": 0, "files": 0, "errors": 0}
for root, _dirs, files in os.walk(assets):
    if "/textures" in root or root.endswith("/maps"):
        continue
    for name in files:
        if not name.endswith(".glb"):
            continue
        path = os.path.join(root, name)
        try:
            doc, rest = read_glb(path)
        except Exception:
            stats["errors"] += 1
            continue
        changed = False
        images = doc.get("images", [])
        textures = doc.get("textures", [])
        for mat in doc.get("materials", []):
            extras = mat.setdefault("extras", {})
            if "dsor_state" in extras:
                continue
            shader = extras.get("nebula_shader", "")
            pbr = mat.setdefault("pbrMetallicRoughness", {})
            emis = mat.get("emissiveTexture")
            base = pbr.get("baseColorTexture")
            state = None
            if shader in HIDDEN:
                state = "Hidden"
            elif shader == "shd:decal" and base is not None and emis is not None:
                color_uri = images[textures[base["index"]]["source"]]["uri"]
                mask_uri = images[textures[emis["index"]]["source"]]["uri"]
                try:
                    uri = merge(root, color_uri, mask_uri)
                except Exception as e:  # a missing texture: leave it hidden
                    print(f"{path}: {e}")
                    state = "Hidden"
                else:
                    images.append({"uri": uri})
                    textures.append({"source": len(images) - 1, "sampler": textures[base["index"]].get("sampler", 0)})
                    pbr["baseColorTexture"] = {"index": len(textures) - 1}
                    mat.pop("emissiveTexture", None)
                    mat.pop("emissiveFactor", None)
                    mat["alphaMode"] = "BLEND"
                    state = "Decal"
            elif shader == "shd:decal":
                state = "Decal"
                mat["alphaMode"] = "BLEND"
            elif emis is not None and base is None:
                pbr["baseColorTexture"] = dict(emis)
                mat.pop("emissiveTexture", None)
                mat.pop("emissiveFactor", None)
                mat["alphaMode"] = "BLEND"
                state = "Additive"
            if state:
                extras["dsor_state"] = state
                stats[state] += 1
                changed = True
        if changed:
            stats["files"] += 1
            if not dry:
                write_glb(path, doc, rest)
print(stats)

# -- decal tiling -------------------------------------------------------------
# A decal's colour (DiffMap0) is not stretched over its box: Nebula repeats it by
# the node's `Scale` shader parameter (0.2 for Kingshill's stones and sand, read
# from the original .n3) while the mask covers the box once. The merged texture
# above cannot do that, so the client draws decals with two UV sets
# (client/src/decals.rs); this records what it needs as extras.dsor_decal:
#   color / mask: glTF texture indices; scale: the n3 Scale (1.0 when absent).
# Usage: fix_materials.py <assets> --n3 <export_win32/models>
if "--n3" in sys.argv:
    models = sys.argv[sys.argv.index("--n3") + 1]

    def n3_scales(path):
        """node name -> Scale, for every node of an .n3 that sets one."""
        data = open(path, "rb").read()
        out = {}
        at = 0
        while True:
            at = data.find(b"Scale", at)
            if at < 0:
                return out
            value = struct.unpack_from("<f", data, at + 5)[0]
            # The owning node: the last "static_..."-style name before it.
            head = data.rfind(b"DNPS", 0, at)
            if head >= 0:
                size = struct.unpack_from("<H", data, head + 4)[0]
                name = data[head + 6 : head + 6 + size].decode("ascii", "replace")
                out[name] = value
            at += 5

    dstats = {"decals": 0, "no_n3": 0}
    for root, _dirs, files in os.walk(assets):
        if "/textures" in root or root.endswith("/maps"):
            continue
        for name in files:
            if not name.endswith(".glb"):
                continue
            path = os.path.join(root, name)
            try:
                doc, rest = read_glb(path)
            except Exception:
                continue
            images = doc.get("images", [])
            textures = doc.get("textures", [])
            changed = False
            for mat in doc.get("materials", []):
                extras = mat.get("extras", {})
                if extras.get("dsor_state") != "Decal" or "dsor_decal" in extras:
                    continue
                base = mat.get("pbrMetallicRoughness", {}).get("baseColorTexture")
                if base is None:
                    continue
                merged = images[textures[base["index"]]["source"]]["uri"]
                stem, _, mask_name = os.path.splitext(merged)[0].rpartition("__")
                if not stem:
                    continue
                find = lambda tail: next((i for i, t in enumerate(textures)
                                          if os.path.splitext(images[t["source"]]["uri"])[0].endswith(tail)), None)
                color = find(os.path.basename(stem))
                mask = find(mask_name)
                if color is None or mask is None:
                    continue
                rel = os.path.relpath(path, assets)[: -len(".glb")]
                n3 = os.path.join(models, rel + ".n3")
                node = mat.get("name", "").rsplit("_decal", 1)[0]
                scale = 1.0
                if os.path.exists(n3):
                    scale = n3_scales(n3).get(node, 1.0)
                else:
                    dstats["no_n3"] += 1
                extras["dsor_decal"] = {"color": color, "mask": mask, "scale": scale}
                dstats["decals"] += 1
                changed = True
            if changed and not dry:
                write_glb(path, doc, rest)
    print(dstats)

# -- character vertex colours ---------------------------------------------------
# The character and monster shaders read COLOR_0 as data (female_head carries
# (0.2, 0.3, 0.0) on every vertex), not as a tint; a stock PBR material multiplies
# it into the base colour and every skin came out dark green. The attribute is
# dropped from those primitives (the buffer is left as is).
# Usage: fix_materials.py <assets> --character-colors
DATA_COLOUR_SHADERS = {"shd:character", "shd:characterfalloff", "shd:monster", "shd:monsterexp"}
if "--character-colors" in sys.argv:
    cstats = {"primitives": 0, "files": 0}
    for root, _dirs, files in os.walk(assets):
        if "/textures" in root:
            continue
        for name in files:
            if not name.endswith(".glb"):
                continue
            path = os.path.join(root, name)
            try:
                doc, rest = read_glb(path)
            except Exception:
                continue
            mats = doc.get("materials", [])
            changed = False
            for mesh in doc.get("meshes", []):
                for prim in mesh.get("primitives", []):
                    m = prim.get("material")
                    if m is None or "COLOR_0" not in prim.get("attributes", {}):
                        continue
                    if mats[m].get("extras", {}).get("nebula_shader") in DATA_COLOUR_SHADERS:
                        del prim["attributes"]["COLOR_0"]
                        cstats["primitives"] += 1
                        changed = True
            if changed:
                cstats["files"] += 1
                if not dry:
                    write_glb(path, doc, rest)
    print(cstats)
