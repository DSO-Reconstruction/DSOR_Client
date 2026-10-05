#!/usr/bin/env python3
"""Build the character tables the client reads, from the client's own data.

Usage: character_tables.py <export_win32> <assets>

Writes:
  <assets>/characters/anims.json   animation set -> state -> clip name, as
      data/tables/anims.xml says ("*<set>" cells resolved to that set's clip). Clip
      names are the converted ones: a looping clip carries "-loop" in
      characters/<skeleton>/__animations.glb, so the name is looked up both ways.
  <assets>/characters/<skeleton>/parts.json   the part names that exist, so the
      client can pick a "_female" variant without probing files.
"""
import json, os, struct, sys
import xml.etree.ElementTree as ET

src, assets = sys.argv[1], sys.argv[2]
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


def clip_names(glb):
    with open(glb, "rb") as f:
        head = f.read(20)
        n = struct.unpack("<I", head[12:16])[0]
        doc = json.loads(f.read(n))
    return {a["name"] for a in doc.get("animations", [])}


rows = ET.parse(os.path.join(src, "data/tables/anims.xml")).getroot().findall(".//ss:Row", NS)
header = cells(rows[0])
sets = {col: name for col, name in header.items() if col > 0 and name}
raw = {name: {} for name in sets.values()}
for row in rows[1:]:
    c = cells(row)
    state = c.get(0)
    if not state:
        continue
    for col, name in sets.items():
        if c.get(col):
            raw[name][state] = c[col]

clips = set()
for skel in ("uniskel", "uniskel_dwarf"):
    p = os.path.join(assets, "characters", skel, "__animations.glb")
    if os.path.exists(p):
        clips |= clip_names(p)


def resolve(set_name, state, depth=0):
    v = raw.get(set_name, {}).get(state)
    if v and v.startswith("*") and depth < 8:
        return resolve(v[1:], state, depth + 1)
    return v


table = {}
for name in raw:
    out = {}
    for state in raw[name]:
        v = resolve(name, state)
        if not v or v == "x_rotation_test":
            continue
        first = v.split(";")[0].split("(")[0].strip()
        if first + "-loop" in clips:
            out[state] = first + "-loop"
        elif first in clips:
            out[state] = first
    if out:
        table[name] = out
os.makedirs(os.path.join(assets, "characters"), exist_ok=True)
with open(os.path.join(assets, "characters", "anims.json"), "w") as f:
    json.dump(table, f, separators=(",", ":"))
print(f"anims.json: {len(table)} animation sets")

for skel in ("uniskel", "uniskel_dwarf"):
    d = os.path.join(assets, "characters", skel, "parts")
    if os.path.isdir(d):
        names = sorted(n[:-4] for n in os.listdir(d) if n.endswith(".glb"))
        with open(os.path.join(assets, "characters", skel, "parts.json"), "w") as f:
            json.dump(names, f)
        print(f"{skel}/parts.json: {len(names)} parts")

# Item template -> its Skin parts (_Template_Item.Skin, ';'-separated), so the client
# can dress its own player from the inventory, which names templates, not skins.
import sqlite3
db = sqlite3.connect(os.path.join(src, "db", "static.db4"))
skins = {
    template: [p.strip() for p in skin.split(";") if p.strip()]
    for template, skin in db.execute("select Id, Skin from _Template_Item where Skin <> ''")
}
with open(os.path.join(assets, "characters", "item_skins.json"), "w") as f:
    json.dump(skins, f, separators=(",", ":"))
print(f"item_skins.json: {len(skins)} items")
