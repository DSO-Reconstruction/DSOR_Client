#!/usr/bin/env python3
"""Export each level's light and post-effect settings, and its placed lights.

Usage: export_ambience.py <export_win32> <assets>

Writes <assets>/maps/<map>.ambience.json (client/src/lighting.rs reads it):

  global   the level's default ambience bubble (_Instance_AmbienceBubble with
           PEDefaultEntity = 1): the global light (colour, opposite colour, back
           light factor, intensity, ambient, specular), the post effect
           (saturation, balance, bloom colour / scale / bright-pass threshold,
           fog colour / distances / heights / intensity, height fog colour,
           vignette) and the light's direction, its transform's z axis
  shadow   _Instance_GlobalShadow's z axis: where the shadows fall from
  bubbles  the other bubbles (event volumes: the box's transform, its priority,
           its event set row) with the same fields, kept for later
  lights   _Instance_Light rows that are always there (EventSetRow -1):
           {t, p, color, i, r, flicker: [freq, intensity] | null, cone, shadows}

Every colour is the level's own float4 (gamma space, as the 2018 renderer uses
it). Transforms are DirectX row-major 4x4: the last row is the position, the
third the z axis.
EVIDENCE: shaders_sm30 "lightsources" GlobalLight ps_3_0 (disassembled):
  light = sat(N.L) * globalLightColor + globalAmbientLightColor
        + sat(globalBackLightOffset - N.L) * globalBackLightColor, times the
  shadow buffer; PointLight: lightColor * sat(N.L) * sat(1 - d / range).
"""
import json, os, sqlite3, struct, sys

root, assets = sys.argv[1], sys.argv[2]
out_dir = os.path.join(assets, "maps")
os.makedirs(out_dir, exist_ok=True)

BUBBLE = [
    "LightColor", "LightOppositeColor", "LightIntensity", "BackLightFactor", "LightAmbient",
    "LightShadowIntensity", "LightSpecularIntensity", "Saturation", "Balance", "BloomScale",
    "BloomColor", "BrightPassThreshold", "FogColor", "HeightFogColor", "FogIntensity",
    "FogNearHeight", "FogFarHeight", "FogNearDist", "FogFarDist", "LocalLightColor",
    "LocalLightIntensity", "RimLightColor", "RimLightIntensity", "VignetteIntensity",
    "VignetteSize", "VignetteColor", "PEFadeTime",
]


def value(v):
    if isinstance(v, bytes):
        n = len(v) // 4
        return [round(x, 4) for x in struct.unpack("<%df" % n, v[: 4 * n])]
    if isinstance(v, float):
        return round(v, 4)
    return v


def matrix(v):
    return list(struct.unpack("<16f", v)) if isinstance(v, bytes) and len(v) == 64 else None


def axis_z(m):
    return [round(x, 4) for x in m[8:11]] if m else None


stats = {"maps": 0, "lights": 0, "no default": 0}
for name in sorted(os.listdir(os.path.join(root, "maps"))):
    if not name.endswith(".db4"):
        continue
    level = name[:-4]
    con = sqlite3.connect(f"file:{os.path.join(root, 'maps', name)}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    try:
        rows = con.execute("select * from _Instance_AmbienceBubble").fetchall()
        lights = con.execute("select * from _Instance_Light where EventSetRow = -1").fetchall()
        shadow = con.execute("select Transform from _Instance_GlobalShadow").fetchone()
    except sqlite3.OperationalError:
        continue
    doc = {"map": level, "global": None, "bubbles": [], "lights": []}
    for r in rows:
        b = {k: value(r[k]) for k in BUBBLE if k in r.keys()}
        m = matrix(r["Transform"])
        b["dir"] = axis_z(m)
        if r["PEDefaultEntity"] and doc["global"] is None:
            doc["global"] = b
        else:
            b.update({"m": [round(x, 4) for x in m] if m else None, "priority": r["AmbienceBubblePriority"],
                      "event": r["EventSetRow"], "shape": r["PEShapeType"]})
            doc["bubbles"].append(b)
    if shadow is not None:
        doc["shadow"] = axis_z(matrix(shadow["Transform"]))
    for r in lights:
        m = matrix(r["Transform"])
        if not m:
            continue
        doc["lights"].append({
            "t": r["LightType"],
            "p": [round(x, 3) for x in m[12:15]],
            "dir": axis_z(m),
            "color": value(r["LightColor"])[:3],
            "i": value(r["LightIntensity"]),
            "r": value(r["LightRange"]),
            "flicker": [value(r["LightFlickerFrequency"]), value(r["LightFlickerIntensity"])] if r["LightFlickerEnable"] else None,
            "cone": value(r["LightConeAngle"]),
            "shadows": bool(r["LightCastShadows"]),
        })
    if doc["global"] is None:
        stats["no default"] += 1
        if not doc["lights"]:
            continue
    with open(os.path.join(out_dir, f"{level}.ambience.json"), "w") as f:
        json.dump(doc, f, separators=(",", ":"))
    stats["maps"] += 1
    stats["lights"] += len(doc["lights"])
print(stats)
