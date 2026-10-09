#!/usr/bin/env python3
"""Add to the converted .glb files the textures DSO_Godot leaves out.

Usage: embed_textures.py <export_win32> <assets> <DSO_Godot>

- Cube maps (CubeMap0 of shd:environment, water, coast, ocean...): DSO_Godot
  keeps only their name (extras.nebula_textures). Each one becomes
  textures/<path>.cube.png, its six faces stacked top to bottom in the DDS order
  (+X, -X, +Y, -Y, +Z, -Z, the order wgpu's cube layers take), top mip only;
  the material gets extras.dsor_cube = that asset path.
- The second layer of shd:simplelayer (438 ground surfaces): DiffMap2 (colour),
  BumpMap1 (normal map), DiffMap3 (mask), tiled by the node's Intensity1 on the
  second UV set. DSO_Godot drops them; they become textures of the glb and the
  material gets extras.dsor_layer = {color, normal, mask: texture index, tiling}.
  EVIDENCE: shaders_sm30 "simplelayer" SimpleLayerSolid ps_3_0 (disassembled):
    colour = lerp(DiffMap0(uv0), DiffMap2(uv1 x layerTiling), DiffMap3(uv1).r),
    layerTiling's semantic Intensity1.
Idempotent: a material already marked is left alone.
"""
import io, os, re, struct, sys

from PIL import Image

root, assets, godot = sys.argv[1], sys.argv[2], sys.argv[3]
sys.path.insert(0, os.path.dirname(__file__))
sys.path.insert(0, godot)
from fix_materials_io import read_glb, write_glb  # noqa: E402
from dsoexp.resources import Resolver, bump_to_normal, dds_to_png, is_bump, _decodable  # noqa: E402

res = Resolver(root)
texdir = os.path.join(assets, "textures")


# -- cube maps --------------------------------------------------------------------
def cube_faces(path):
    """The six top-mip faces of a DDS cube map, as PIL images."""
    d = open(path, "rb").read()
    if d[:4] != b"DDS " or not struct.unpack_from("<I", d, 112)[0] & 0x200:
        raise ValueError("not a DDS cube map")
    h, w = struct.unpack_from("<II", d, 12)
    mips = max(1, struct.unpack_from("<I", d, 28)[0])
    pf_flags, fourcc, bits = struct.unpack_from("<I4sI", d, 80)
    if pf_flags & 0x4:
        block = 8 if fourcc == b"DXT1" else 16
        size = lambda w, h: max(1, (w + 3) // 4) * max(1, (h + 3) // 4) * block
    else:
        size = lambda w, h: w * h * (bits // 8)
    face = sum(size(max(1, w >> m), max(1, h >> m)) for m in range(mips))
    header = bytearray(d[:128])
    struct.pack_into("<I", header, 28, 1)          # one mip
    struct.pack_into("<I", header, 8, struct.unpack_from("<I", header, 8)[0] & ~0x20000)
    struct.pack_into("<II", header, 108, 0x1000, 0)  # caps: texture; caps2: none
    out = []
    for f in range(6):
        at = 128 + f * face
        img = Image.open(io.BytesIO(bytes(header) + d[at:at + size(w, h)]))
        img.load()
        out.append(img.convert("RGB"))
    return out


cube_done = {}


def cube_png(ref):
    """'tex:t001_hub/env_reflect_01_cube' -> 'textures/t001_hub/env_reflect_01_cube.cube.png'."""
    if ref in cube_done:
        return cube_done[ref]
    rel = ref.split(":", 1)[-1]
    out = os.path.join(texdir, rel + ".cube.png")
    asset = os.path.relpath(out, assets).replace(os.sep, "/")
    src = res.path(ref)
    try:
        if not os.path.exists(out):
            src = _decodable(src) if src else None
            if not src:
                raise FileNotFoundError(ref)
            faces = cube_faces(src)
            w, h = faces[0].size
            strip = Image.new("RGB", (w, h * 6))
            for i, f in enumerate(faces):
                strip.paste(f.resize((w, h)), (0, i * h))
            os.makedirs(os.path.dirname(out), exist_ok=True)
            strip.save(out)
    except Exception as e:  # noqa: BLE001 -- a broken texture leaves the surface without
        print(f"cube {ref}: {e}")
        asset = None
    cube_done[ref] = asset
    return asset


# -- n3 node textures and params ----------------------------------------------------
def node_textures(d):
    """{node name: {slot: tex ref}} for every node of an .n3 ("TXTS": u16 len slot, u16 len ref)."""
    out = {}
    for m in re.finditer(rb"DNM>....(..)", d, re.S):
        try:
            n = struct.unpack_from("<H", d, m.start(1))[0]
            name = d[m.start(1) + 2:m.start(1) + 2 + n].decode("latin1")
        except (struct.error, UnicodeDecodeError):
            continue
        start = m.start(1) + 2 + n
        end = d.find(b"DNM", start)
        end = len(d) if end < 0 else end
        tex = {}
        j = start
        while True:
            j = d.find(b"TXTS", j, end)
            if j < 0:
                break
            try:
                ln = struct.unpack_from("<H", d, j + 4)[0]
                slot = d[j + 6:j + 6 + ln].decode("latin1")
                k = j + 6 + ln
                ln2 = struct.unpack_from("<H", d, k)[0]
                tex[slot] = d[k + 2:k + 2 + ln2].decode("latin1")
            except (struct.error, UnicodeDecodeError):
                pass
            j += 4
        if tex:
            out[name] = tex
    return out


def add_texture(doc, glb_dir, ref):
    """Convert a texture ref and append it to the glb; its texture index, or None."""
    if not ref or "system/" in ref:
        return None
    src = res.path(ref)
    if not src:
        return None
    rel = ref.split(":", 1)[-1]
    bump = is_bump(ref)
    png = os.path.join(texdir, rel + ("_nrm.png" if bump else ".png"))
    if not (bump_to_normal if bump else dds_to_png)(src, png):
        return None
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


stats = {"cube materials": 0, "layer materials": 0, "files": 0}
models = os.path.join(root, "models")
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
        n3_tex = None
        for mat in doc.get("materials", []):
            ex = mat.setdefault("extras", {})
            cube = ex.get("nebula_textures", {}).get("CubeMap0")
            if cube and "dsor_cube" not in ex:
                asset = cube_png(cube)
                if asset:
                    ex["dsor_cube"] = asset
                    stats["cube materials"] += 1
                    changed = True
            if ex.get("nebula_shader") == "shd:simplelayer" and "dsor_layer" not in ex:
                if n3_tex is None:
                    rel = os.path.relpath(path, assets)[: -len(".glb")]
                    n3 = os.path.join(models, rel + ".n3")
                    n3_tex = node_textures(open(n3, "rb").read()) if os.path.exists(n3) else {}
                node = mat.get("name", "").rsplit("_simplelayer", 1)[0]
                tex = n3_tex.get(node, {})
                color = add_texture(doc, dirpath, tex.get("DiffMap2"))
                mask = add_texture(doc, dirpath, tex.get("DiffMap3"))
                if color is None or mask is None:
                    continue
                tiling = 1.0
                for nd in doc.get("nodes", []):
                    if nd.get("name") == node:
                        tiling = nd.get("extras", {}).get("dsor_shader", {}).get("Intensity1", 1.0)
                ex["dsor_layer"] = {"color": color, "mask": mask, "normal": add_texture(doc, dirpath, tex.get("BumpMap1")), "tiling": tiling}
                stats["layer materials"] += 1
                changed = True
        if changed:
            stats["files"] += 1
            write_glb(path, doc, rest)
print(stats)
