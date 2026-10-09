#!/usr/bin/env python3
"""Give character and monster surfaces their dye masks back.

Usage: embed_character_masks.py <export_win32> <assets> <DSO_Godot>

The 2018 character shaders (shd:character, characterfalloff, monster, monsterexp)
dye the colour map through the spec map's channels:
  colour = lerp(colour, MatDiffuse x lum(colour), spec.g x MatDiffuse.a)
  colour = lerp(colour, MatSpecular x lum(colour), spec.b x MatSpecular.a)
EVIDENCE: shaders_sm30 "monster" SkinnedSolid ps_3_0 (tools/shader_dis.py), the
  same in "character": customColor0 <MatDiffuse>, customColor1 <MatSpecular>,
  luminanceVector, specMapSampler .y / .z.
DSO_Godot keeps only a roughness map made from the spec map, so the green and
blue channels are gone. Each such material's SpecMap0 becomes
textures/<path>_mask.png (the DDS as is) and a texture of the glb; the material
gets extras.dsor_dye = {"mask": <texture index>} (client/src/materials.rs, kind
Character in crate::surfaces).
Idempotent: a material already marked is left alone.
"""
import os, sys

assets_root, assets, godot = sys.argv[1], sys.argv[2], sys.argv[3]
sys.path.insert(0, os.path.dirname(__file__))
sys.path.insert(0, godot)
from fix_materials_io import read_glb, write_glb  # noqa: E402
from dsoexp.resources import Resolver, dds_to_png  # noqa: E402

res = Resolver(assets_root)
texdir = os.path.join(assets, "textures")
SHADERS = {"shd:character", "shd:characterfalloff", "shd:monster", "shd:monsterexp"}


def add_texture(doc, glb_dir, png):
    uri = os.path.relpath(png, glb_dir).replace(os.sep, "/")
    images = doc.setdefault("images", [])
    for i, img in enumerate(images):
        if img.get("uri") == uri:
            for t, tx in enumerate(doc.get("textures", [])):
                if tx.get("source") == i:
                    return t
    images.append({"uri": uri})
    textures = doc.setdefault("textures", [])
    textures.append({"source": len(images) - 1, "sampler": 0} if doc.get("samplers") else {"source": len(images) - 1})
    return len(textures) - 1


stats = {"materials": 0, "files": 0, "missing": 0}
for dirpath, _dirs, files in os.walk(assets):
    if "/textures" in dirpath:
        continue
    for name in files:
        if not name.endswith(".glb"):
            continue
        path = os.path.join(dirpath, name)
        try:
            doc, rest = read_glb(path)
        except Exception:
            continue
        changed = False
        for mat in doc.get("materials", []):
            ex = mat.get("extras", {})
            if ex.get("nebula_shader") not in SHADERS or "dsor_dye" in ex:
                continue
            ref = ex.get("nebula_textures", {}).get("SpecMap0")
            if not ref or "system/" in ref:
                continue
            src = res.path(ref)
            png = os.path.join(texdir, ref.split(":", 1)[-1] + "_mask.png")
            if not src or not dds_to_png(src, png):
                stats["missing"] += 1
                continue
            ex["dsor_dye"] = {"mask": add_texture(doc, dirpath, png)}
            mat["extras"] = ex
            stats["materials"] += 1
            changed = True
        if changed:
            stats["files"] += 1
            write_glb(path, doc, rest)
print(stats)
