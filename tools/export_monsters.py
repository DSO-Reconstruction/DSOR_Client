#!/usr/bin/env python3
"""Monsters as the 2018 client draws them, from its own data.

Usage: export_monsters.py <export_win32> <assets>

Writes:
  characters/monster_templates.json
      _Template_Monster Id -> {graphics, set, anim_set, state, loop, title,
      hit: {damage type -> sequence}, critical, heavy, death, death_s,
      despawn, despawn_s, dye [PrimaryColor, SecondaryColor], radius, height}
      (title: language/fr/db.monster.xml "<Id>Title")
  characters/monster_anims.json
      every animation set of data/tables/anims.xml -> state -> clip base name,
      "*<set>" cells resolved. The client looks the name up in the model's own
      clips, with or without the converter's "-loop" suffix (character_tables.py
      keeps only what the human skeleton has).
  characters/<model>.sets.json   (whole models)
      character set -> {skins: [...], variation: [{bone, t, s}]}
      from the model's .n3 skin lists (SKNL: name, u32 n, n skins, u8 n, variation)
      and the variation's clip ("var_" + its name) at its first key.
And it tags each skinned mesh node of characters/<model>.glb with extras
dsor_skin (the skin it draws, from the mesh path in <model>.fx.json:
".../<skin>_sk_<n>.nvx2"), so the client shows only the set's skins: one file
holds every look of a rig (troll.glb: troll, cyclops, gnob, golems...).
"""
import json, os, re, sqlite3, struct, sys
import xml.etree.ElementTree as ET

sys.path.insert(0, os.path.dirname(__file__))
from fix_materials_io import read_glb, write_glb  # noqa: E402

export, assets = sys.argv[1], sys.argv[2]
chars = os.path.join(assets, "characters")

# --- templates -------------------------------------------------------------
titles = {}
xml = open(os.path.join(export, "language", "fr", "db.monster.xml"), encoding="utf-8-sig").read()
for name, text in re.findall(r'<item name="([^"]+)Title"><!\[CDATA\[(.*?)\]\]></item>', xml, re.S):
    titles[name] = text

HIT = {
    "Physical": "PhysicalHitSequence", "Fire": "FireHitSequence", "Ice": "IceHitSequence",
    "Lightning": "LightningHitSequence", "DarkMagic": "DarkMagicHitSequence",
    "Poison": "PoisonHitSequence",
}
COLS = ["Id", "Graphics", "CharacterSet", "AnimSet", "StartAnimationState", "LoopStartAnimation",
        "CriticalHitSequence", "HeavyHitSequence", "DeathSequence", "DeathDuration",
        "DespawnSequence", "DespawnDuration", "CapsuleRadius", "CapsuleHeight",
        "PrimaryColor", "SecondaryColor"] + list(HIT.values())
db = sqlite3.connect(f"file:{os.path.join(export, 'db', 'static.db4')}?mode=ro", uri=True)
templates = {}
for row in db.execute(f"select {','.join(COLS)} from _Template_Monster"):
    r = dict(zip(COLS, row))
    templates[r["Id"]] = {
        "graphics": r["Graphics"] or "",
        "set": r["CharacterSet"] or "",
        "anim_set": r["AnimSet"] or "",
        "state": r["StartAnimationState"] or "Idle",
        "loop": bool(r["LoopStartAnimation"]),
        "title": titles.get(r["Id"], ""),
        "hit": {k: r[c] for k, c in HIT.items() if r[c]},
        "critical": r["CriticalHitSequence"] or "",
        "heavy": r["HeavyHitSequence"] or "",
        "death": r["DeathSequence"] or "",
        "death_s": float(r["DeathDuration"] or 0.0),
        "despawn": r["DespawnSequence"] or "",
        "despawn_s": float(r["DespawnDuration"] or 0.0),
        "dye": [list(struct.unpack("<4f", r["PrimaryColor"])) if r["PrimaryColor"] else [1, 1, 1, 0],
                list(struct.unpack("<4f", r["SecondaryColor"])) if r["SecondaryColor"] else [1, 1, 1, 0]],
        "radius": float(r["CapsuleRadius"] or 0.0),
        "height": float(r["CapsuleHeight"] or 0.0),
    }
with open(os.path.join(chars, "monster_templates.json"), "w") as f:
    json.dump(templates, f, separators=(",", ":"), ensure_ascii=False)

# --- animation sets --------------------------------------------------------
NS = {"ss": "urn:schemas-microsoft-com:office:spreadsheet"}
IDX = "{urn:schemas-microsoft-com:office:spreadsheet}Index"


def cells(row):
    out, i = {}, 0
    for c in row.findall("ss:Cell", NS):
        if c.get(IDX):
            i = int(c.get(IDX)) - 1
        d = c.find("ss:Data", NS)
        out[i] = d.text if d is not None else None
        i += 1
    return out


rows = ET.parse(os.path.join(export, "data/tables/anims.xml")).getroot().findall(".//ss:Row", NS)
header = cells(rows[0])
sets = {col: name for col, name in header.items() if col > 0 and name}
raw = {name: {} for name in sets.values()}
for row in rows[1:]:
    c = cells(row)
    if c.get(0):
        for col, name in sets.items():
            if c.get(col):
                raw[name][c[0]] = c[col]


def resolve(set_name, state, depth=0):
    v = raw.get(set_name, {}).get(state)
    if v and v.startswith("*") and depth < 8:
        return resolve(v[1:], state, depth + 1)
    return v


anims = {}
for name in raw:
    out = {}
    for state in raw[name]:
        v = resolve(name, state)
        if v and v != "x_rotation_test":
            out[state] = v.split(";")[0].split("(")[0].strip()
    if out:
        anims[name] = out
with open(os.path.join(chars, "monster_anims.json"), "w") as f:
    json.dump(anims, f, separators=(",", ":"))

# --- character sets of whole models ---------------------------------------


def n3_string(b, i):
    n = struct.unpack_from("<H", b, i)[0]
    return b[i + 2:i + 2 + n].decode("latin-1"), i + 2 + n


def skin_lists(path):
    """[(set name, [skins], variation or "")] of an n3's SKNL records."""
    b = open(path, "rb").read()
    out = []
    i = b.find(b"LNKS")
    while i >= 0:
        try:
            name, j = n3_string(b, i + 4)
            (n,) = struct.unpack_from("<I", b, j)
            j += 4
            skins = []
            for _ in range(n):
                s, j = n3_string(b, j)
                skins.append(s)
            variation = ""
            if b[j] >= 1:
                variation, j = n3_string(b, j + 1)
            out.append((name, skins, variation))
        except (struct.error, IndexError):
            pass
        i = b.find(b"LNKS", i + 4)
    return out


COMPONENTS = {5120: ("b", 1), 5121: ("B", 1), 5122: ("h", 2), 5123: ("H", 2), 5125: ("I", 4), 5126: ("f", 4)}
WIDTH = {"SCALAR": 1, "VEC3": 3, "VEC4": 4}


def first_value(doc, rest, accessor):
    """An accessor's first element (float output of an animation sampler)."""
    a = doc["accessors"][accessor]
    view = doc["bufferViews"][a["bufferView"]]
    fmt, size = COMPONENTS[a["componentType"]]
    n = WIDTH[a["type"]]
    # rest = binary chunk header (8 bytes) + buffer 0.
    at = 8 + view.get("byteOffset", 0) + a.get("byteOffset", 0)
    return list(struct.unpack_from("<" + fmt * n, rest, at))


def variation_shape(doc, rest, clip):
    anim = next((a for a in doc.get("animations", []) if a.get("name") in (clip, clip + "-loop")), None)
    if anim is None:
        return None
    nodes = doc["nodes"]
    shape = {}
    for ch in anim["channels"]:
        t = ch["target"]
        path = t.get("path")
        if path not in ("translation", "scale") or "node" not in t:
            continue
        node = nodes[t["node"]]
        bone = node.get("name", "")
        entry = shape.setdefault(bone, {"bone": bone, "t": node.get("translation", [0, 0, 0]), "s": [1, 1, 1]})
        value = first_value(doc, rest, anim["samplers"][ch["sampler"]]["output"])
        entry["t" if path == "translation" else "s"] = [round(v, 6) for v in value]
    return list(shape.values())


models = tagged = 0
for name in sorted(os.listdir(chars)):
    if not name.endswith(".glb"):
        continue
    model = name[:-4]
    n3 = os.path.join(export, "models", "characters", model + ".n3")
    fx_path = os.path.join(chars, model + ".fx.json")
    if not os.path.exists(n3) or not os.path.exists(fx_path):
        continue
    lists = skin_lists(n3)
    if not lists:
        continue
    glb = os.path.join(chars, name)
    try:
        doc, rest = read_glb(glb)
    except ValueError as err:
        print(f"{name}: unreadable ({err}), skipped")
        continue
    changed = False
    for e in json.load(open(fx_path)).get("emitters", []):
        m = re.search(r"/([^/]+)_sk_\d+\.nvx2$", e.get("mesh", ""))
        node = e.get("gltf_node")
        if not m or node is None or node >= len(doc["nodes"]):
            continue
        extras = doc["nodes"][node].setdefault("extras", {})
        if extras.get("dsor_skin") != m.group(1):
            extras["dsor_skin"] = m.group(1)
            changed = True
        tagged += 1
    if changed:
        write_glb(glb, doc, rest)
    out = {}
    for set_name, skins, variation in lists:
        entry = {"skins": skins}
        if variation:
            shape = variation_shape(doc, rest, "var_" + variation)
            if shape:
                entry["variation"] = shape
        out[set_name] = entry
    with open(os.path.join(chars, model + ".sets.json"), "w") as f:
        json.dump(out, f, separators=(",", ":"))
    models += 1
print({"templates": len(templates), "titles": len(titles), "anim_sets": len(anims), "models": models, "skin_nodes": tagged})
