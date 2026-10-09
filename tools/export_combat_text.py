#!/usr/bin/env python3
"""Floating combat texts (damage numbers, "Parade !"...), from the 2018 client's data.

Usage: export_combat_text.py <export_win32> <assets>

Writes interface/combat_text.json:
  types: CombatTextType name -> {color [r,g,b,a], duration, fade, offset_y [x,y,z],
         offset_x [x,y,z], offset_x_var, dynamic_x, velocity [x,y,z], velocity_var,
         size, label}   (static.db4 _Template_CombatText, one row per type)
  texts: block, critical ("{0:i} !"), immunity, low_health
         (language/fr/gui.window.xml floatingBlock, floatingCritical,
         floatingDamageImmunity, lowhealthhit)
EVIDENCE: TemplateManager's combat text getters read these columns by type
  (GetCombatTextColor 0x51212B: +0x10 colour; 0x5121E1 +0x30 x offset; 0x51226A
  +0x40 its variation; 0x5122C9 +0x50 y offset; 0x5123FD +0x60 velocity; 0x512486
  +0x70 its variation; 0x512352 +0x74 size; 0x5123B5 +0x78 template name;
  0x512196 +0xB8 dynamic x offset).
"""
import json, os, re, sqlite3, struct, sys

export, assets = sys.argv[1], sys.argv[2]
db = sqlite3.connect(f"file:{os.path.join(export, 'db', 'static.db4')}?mode=ro", uri=True)


def vec(blob, n):
    return [round(v, 6) for v in struct.unpack("<4f", blob)[:n]]


types = {}
for row in db.execute(
    "select CombatTextType, CombatTextColor, CombatTextDuration, CombatTextFadeOutDuration,"
    " CombatTextOffsetYBase, CombatTextOffsetXBase, CombatTextOffsetXVariation,"
    " CombatTextDynamicXOffsetEnabled, CombatTextVelocity, CombatTextVelocityVariation,"
    " CombatTextSize, CombatTextTemplateName from _Template_CombatText"
):
    name, color, duration, fade, oy, ox, oxv, dyn, vel, velv, size, label = row
    types[name] = {
        "color": vec(color, 4), "duration": duration, "fade": fade,
        "offset_y": vec(oy, 3), "offset_x": vec(ox, 3), "offset_x_var": oxv,
        "dynamic_x": bool(dyn), "velocity": vec(vel, 3), "velocity_var": velv,
        "size": size, "label": label or "",
    }

xml = open(os.path.join(export, "language", "fr", "gui.window.xml"), encoding="utf-8-sig").read()
loca = dict(re.findall(r'<item name="([^"]+)"><!\[CDATA\[(.*?)\]\]></item>', xml, re.S))
texts = {
    "block": loca.get("floatingBlock", "Block!"),
    "critical": loca.get("floatingCritical", "{0:i}!"),
    "immunity": loca.get("floatingDamageImmunity", "Immune"),
    "low_health": loca.get("lowhealthhit", ""),
}
out = os.path.join(assets, "interface", "combat_text.json")
with open(out, "w") as f:
    json.dump({"types": types, "texts": texts}, f, ensure_ascii=False, separators=(",", ":"))
print({"types": len(types), "texts": texts})
