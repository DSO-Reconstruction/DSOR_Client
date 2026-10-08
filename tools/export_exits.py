#!/usr/bin/env python3
"""Map exits (the glowing arrows on the ground), from the 2018 client's data.

Usage: export_exits.py <export_win32> <assets>

Writes maps/<map>.exits.json: a list of the level's _Instance_InteractExit rows
(maps/<map>.db4), each
  {id, name, url, entry, graphics, range, event, m}
  id        the exit's template Id (UnlockMapCommand's exit id)
  name      the instance Name
  url       ExitURL, the destination map (UnlockMapCommand's map id and exit URL)
  entry     EntryName, the arrival point in the destination
  graphics  the model drawn
  range     PickingRange
  event     EventSetRow: -1 for an exit that is always there, else the row of the
            map event that brings it (Kingshill stacks several event exits on one
            spot)
  m         the 4x4 placement (16 floats, DirectX row-major: the last row is the
            position), read as is by glam
EVIDENCE: the 2018 client's own UnlockMapCommand, captured
  (session-maps1.jsonl frame 1074): 8b 68 00 | 10 00 "a0302_wildforest" |
  10 00 "a0302_wildforest" | 0f 00 "exit_general_01" | 0 -- Kingshill's exit row
  with Id exit_general_01 and ExitURL a0302_wildforest.
"""
import json, os, sqlite3, struct, sys

export, assets = sys.argv[1], sys.argv[2]

COLUMNS = ["Id", "Name", "ExitURL", "EntryName", "Graphics", "PickingRange", "EventSetRow", "Transform"]

maps = 0
exits = 0
for name in sorted(os.listdir(os.path.join(export, "maps"))):
    if not name.endswith(".db4"):
        continue
    try:
        db = sqlite3.connect(f"file:{os.path.join(export, 'maps', name)}?mode=ro", uri=True)
        rows = db.execute(f"select {','.join(COLUMNS)} from _Instance_InteractExit").fetchall()
    except sqlite3.Error:
        continue
    out = []
    for rid, iname, url, entry, graphics, picking, event, transform in rows:
        if not transform or len(transform) != 64 or not url or not graphics:
            continue
        out.append({
            "id": rid,
            "name": iname or "",
            "url": url,
            "entry": entry or "",
            "graphics": graphics,
            "range": float(picking or 0.0),
            "event": int(event if event is not None else -1),
            "m": [round(v, 5) for v in struct.unpack("<16f", transform)],
        })
    if not out:
        continue
    with open(os.path.join(assets, "maps", name[:-4] + ".exits.json"), "w") as f:
        json.dump(out, f, separators=(",", ":"), ensure_ascii=False)
    maps += 1
    exits += len(out)
print({"maps": maps, "exits": exits})
