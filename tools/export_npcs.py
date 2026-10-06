#!/usr/bin/env python3
"""NPC looks and placements, from the 2018 client's data.

Usage: export_npcs.py <export_win32> <assets>

Writes:
  characters/npc_templates.json  template Id -> {graphics, outfit, anim_set, state, loop, title}
                           (static.db4 _Template_NPC; title from language/fr/db.npc.xml,
                           item "<Id>Title")
  maps/<map>.npcs.json     level Guid (hex, upper) -> {id, m}: the instance's template
                           and its 4x4 transform (16 floats, DirectX row-major: the
                           last row is the position), from maps/<map>.db4 _Instance_NPC

NewNPCCommand carries the template name, the level Guid and a position, not the
facing: the 2018 client takes the placement from the level by Guid, and so does
client/src/npc.rs. Instance rows override template columns (same names).
"""
import json, os, re, sqlite3, struct, sys

export, assets = sys.argv[1], sys.argv[2]

titles = {}
xml = open(os.path.join(export, "language", "fr", "db.npc.xml"), encoding="utf-8-sig").read()
for name, text in re.findall(r'<item name="([^"]+)Title"><!\[CDATA\[(.*?)\]\]></item>', xml, re.S):
    titles[name] = text

COLUMNS = ["Id", "Graphics", "CharacterSet", "AnimSet", "StartAnimationState", "LoopStartAnimation"]


def look(row):
    rid, graphics, outfit, anim_set, state, loop = row
    return {
        "graphics": graphics or "",
        "outfit": outfit or "",
        "anim_set": anim_set or "",
        "state": state or "Idle",
        "loop": bool(loop),
        "title": titles.get(rid, ""),
    }


db = sqlite3.connect(os.path.join(export, "db", "static.db4"))
templates = {r[0]: look(r) for r in db.execute(f"select {','.join(COLUMNS)} from _Template_NPC")}
with open(os.path.join(assets, "characters", "npc_templates.json"), "w") as f:
    json.dump(templates, f, separators=(",", ":"), ensure_ascii=False)

maps = 0
placed = 0
for name in sorted(os.listdir(os.path.join(export, "maps"))):
    if not name.endswith(".db4"):
        continue
    try:
        mdb = sqlite3.connect(os.path.join(export, "maps", name))
        rows = mdb.execute(f"select hex(Guid), Transform, {','.join(COLUMNS)} from _Instance_NPC").fetchall()
    except sqlite3.Error:
        continue
    if not rows:
        continue
    out = {}
    for guid, transform, *cols in rows:
        if not transform or len(transform) != 64:
            continue
        entry = {"id": cols[0], "m": [round(v, 5) for v in struct.unpack("<16f", transform)]}
        # Instance columns that differ from the template's.
        own = look(cols)
        base = templates.get(cols[0])
        if base is None or any(own[k] != base[k] for k in ("graphics", "outfit", "anim_set", "state")):
            entry["look"] = own
        out[guid.upper()] = entry
    with open(os.path.join(assets, "maps", name[:-4] + ".npcs.json"), "w") as f:
        json.dump(out, f, separators=(",", ":"), ensure_ascii=False)
    maps += 1
    placed += len(out)
print({"templates": len(templates), "titles": len(titles), "maps": maps, "placed": placed})
