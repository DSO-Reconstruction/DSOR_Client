#!/usr/bin/env python3
"""Export the 2018 client's interface windows for client/src/ui.

Usage: export_ui.py <export_win32> <assets> <DSO_Godot> [window ...]

A window (export_win32/ui/<name>.bxml, Nebula binary XML) is a tree of widgets;
each one has a rect in fractions of a 1024 x 768 reference screen and an anchor
to its parent (HorizontalParentAlignment / VerticalParentAlignment); its artwork
is a group of the window's mesh (meshes/ui/<name>_s_0.nvx2: geometry, atlas UVs,
vertex colours). DSO_Godot's export_ui.py decodes all of that (reused here).

The client scales the interface by the window height (768 reference pixels) and
widens the reference screen to the window's aspect (dW = width - 1024). Widget
sizes are fixed, but a top-level widget spanning the whole reference width (a
full-screen canvas) stretches with the screen; a widget's x is then its 1024 x
768 one plus k x dW, k following the anchors down the tree (a child at its
parent's right edge of a stretched canvas gets k = 1, one at a fixed-size
parent's anything gets the parent's k). k is all the client needs.

Writes <assets>/interface/<name>.ui.json:
  {"window", "base": [1024, 768],
   "widgets": [{"id": "Canvas/Label_bg/...", "type", "k": k,
                "rect": [l, t, r, b], "hidden": bool,
                "art": {"tex": "textures/ui/x.png" | null, "pos": [x, y, ...],
                        "uv": [u, v, ...], "color": [r, g, b, a, ...], "idx": [...]},
                "text": {"text", "key", "size", "color", "align": [h, v], "bold"}}]}
in drawing order (parents first), positions in reference pixels (y down), the
artwork as the mesh group's own triangles, UVs and vertex colours (the engine
draws them as they are: quads, nine-slices, the globes' polygons), plus the
atlases as <assets>/textures/ui/*.png and <assets>/interface/loca_<lang>.json
(the texts of the "gui." keys the windows use).
Alternative states stacked on one rect (button states, latency colours...) are
marked hidden but the first, as DSO_Godot does.
"""
import json, os, sys

root, assets, godot = sys.argv[1], sys.argv[2], sys.argv[3]
# Every window by default ("--hud": the four the HUD needs).
only = sys.argv[4:] or sorted(f[:-5] for f in os.listdir(os.path.join(root, "ui")) if f.endswith(".bxml"))
if only == ["--hud"]:
    only = ["bottombar", "currencybar", "inventory", "gamebar"]
sys.path.insert(0, godot)
import export_ui as gx  # noqa: E402  DSO_Godot's exporter
from dsoexp import bxml  # noqa: E402
from dsoexp.resources import Resolver  # noqa: E402

BASE_W, BASE_H = gx.BASE_W, gx.BASE_H

import re  # noqa: E402

# Widgets the game shows only in some state (the engine switches them at run
# time): drawn as they are, they pile up -- every button state at once, glows,
# the cooldown covers, the "buy a slot" offers, the other classes' resource
# globes stay (the client shows its class's). They are exported hidden.
RUNTIME = re.compile(
    r"^(mouseover|pressed|mouseoverpressed|disabled|cover|selected|highlight|flash|checked)$"
    r"|glow|Glow|sparkle|Sparkle|shine|Shine|flare"
    r"|^EmptyQuickbar|^ButtonBuy|^ButtonDeluxe|^MultipleBars|_locked$|^Quickslots_old$"
    r"|^LatencyIndicator(Red|Yellow)$|^figur$"
    r"|^ammo_|^ProgressBar_ammo|^wisdomAndXpbar$|^TemplateGlow")


# Slots the game clones from a template over an area, row by row: (window,
# template id, area id). The template itself is a runtime ghost ("?" icon).
GRIDS = [("inventory", "TemplateSlot", "icon_area")]
GRID_FOOTER = 45.0  # the area's bottom left free for the page buttons


def grid(widgets, template, area):
    """Empty slots: the template's subtree (its icon left out) repeated over the area."""
    t = next((i for i, w in enumerate(widgets) if w["id"].endswith("/" + template)), None)
    a = next((w for w in widgets if w["id"].endswith("/" + area)), None)
    if t is None or a is None:
        return widgets
    tw = widgets[t]
    sub = [w for w in widgets if w["id"].startswith(tw["id"] + "/") and "TemplateIcon" not in w["id"]]
    l, top, r, b = tw["rect"]
    step_x, step_y = (r - l) + 2.0, (b - top) + 2.0
    al, at, ar, ab = a["rect"]
    cols = int((ar - l + 2.0) // step_x)
    rows = int((ab - GRID_FOOTER - top) // step_y)
    clones = []
    for row in range(rows):
        for col in range(cols):
            dx, dy = col * step_x, row * step_y
            for w in [tw] + sub:
                c = json.loads(json.dumps(w))
                c["id"] = w["id"].replace(template, f"{template}#{row}_{col}", 1)
                c["hidden"] = False
                c["rect"] = [c["rect"][0] + dx, c["rect"][1] + dy, c["rect"][2] + dx, c["rect"][3] + dy]
                if "art" in c:
                    c["art"]["pos"] = [round(v + (dx if k % 2 == 0 else dy), 2) for k, v in enumerate(c["art"]["pos"])]
                clones.append(c)
    for w in widgets:
        if w["id"] == tw["id"] or w["id"].startswith(tw["id"] + "/"):
            w["hidden"] = True
    end = t + 1 + sum(1 for w in widgets[t + 1:] if w["id"].startswith(tw["id"] + "/"))
    return widgets[:end] + clones + widgets[end:]


# Windows whose widgets outside these subtrees are templates the game clones at
# run time (drag and drop ghosts, slot highlights), not part of the window.
ROOTS = {
    "inventory": ["Canvas/PositioningLabel"],
    # The money display in its plain layout (not the payment / RC-only ones).
    "currencybar": ["Canvas/LabelDefault", "Canvas/AnchorSidebarCurrencyDisplay"],
}


def runtime_only(node):
    return bool(RUNTIME.search(node.attrs.get("id") or ""))
out_dir = os.path.join(assets, "interface")
os.makedirs(out_dir, exist_ok=True)
texdir = os.path.join(assets, "textures")
tex = gx.Textures(Resolver(root), texdir, "textures")
loca = gx.load_loca(root, ["fr", "en"])
used_keys = set()


def widget_art(node, mesh, cx0, cy0):
    """The widget's artwork: its mesh group in reference pixels, or (None, why)."""
    a = node.attrs
    if "meshGrpIdx" not in a or a.get("HideBackground") == "true" or mesh is None:
        return None, None
    sx, sy = gx.fvals(a.get("scale", "1,1"), 2, 1.0)
    grp = mesh.group(int(a["meshGrpIdx"]), sx or 1.0, sy or 1.0)
    if grp is None:
        return None, None
    verts, uvs, cols, faces = grp
    ref = a.get("texture", "")
    t = tex.get(ref)
    if t is None and ref and ref not in gx.BLANK_TEX:
        return None, "no texture"
    if t is None and all(c == (1.0, 1.0, 1.0, 1.0) for c in cols) and ref in ("system/white", ""):
        # Untextured white: a hit area, or a slot the game fills at run time.
        return None, None
    return {
        "tex": f"textures/{ref}.png" if t else None,
        "pos": [round(c, 2) for v in verts for c in (cx0 + v[0], cy0 + v[1])],
        "uv": [round(c, 6) for u in uvs for c in u],
        "color": [round(c, 4) for col in cols for c in col],
        "idx": [i for f in faces for i in f],
    }, None


def export(name):
    path = os.path.join(root, "ui", name + ".bxml")
    doc = bxml.parse(open(path, "rb").read())
    mpath = os.path.join(root, "meshes", "ui", name + "_s_0.nvx2")
    mesh = gx.UiMesh(mpath) if os.path.isfile(mpath) else None
    widgets, stats = [], {"widgets": 0, "art": 0, "triangles": 0, "no texture": 0, "texts": 0}

    roots = ROOTS.get(name)

    def walk(node, prefix, parent_rect, pk, hidden_parent):
        hide_idx = gx.exclusive(node)
        for i, c in enumerate(node.children):
            if c.tag in ("Nebula3", "Window"):
                walk(c, prefix, parent_rect, pk, hidden_parent)
                continue
            a = c.attrs
            rect = gx.fvals(a.get("rect", "0,0,1,1"), 4)
            l, t, r, b = rect[0] * BASE_W, rect[1] * BASE_H, rect[2] * BASE_W, rect[3] * BASE_H
            # No alignment: the engine's default, the parent's top left (the
            # currency bar's canvas has none and sits in the screen's corner).
            ax = gx.H_ANCHOR.get(a.get("HorizontalParentAlignment"), 0.0)
            ay = gx.V_ANCHOR.get(a.get("VerticalParentAlignment"), 0.0)
            # (k of the left edge, k of the right edge) of the parent.
            kl = pk[0] + ax * (pk[1] - pk[0])
            stretch = parent_rect == (0.0, 0.0, BASE_W, BASE_H) and l <= 0.02 * BASE_W and r >= 0.98 * BASE_W
            ks = (0.0, 1.0) if stretch else (kl, kl)
            wid = (prefix + "/" if prefix else "") + (a.get("id") or c.tag)
            outside = roots is not None and not any(wid == r or wid.startswith(r + "/") or r.startswith(wid + "/") for r in roots)
            hidden = hidden_parent or i in hide_idx or a.get("Visible") == "false" or runtime_only(c) or outside
            w, h = max(r - l, 0.0), max(b - t, 0.0)
            art, why = widget_art(c, mesh, l + w / 2.0, t + h / 2.0)
            if why:
                stats[why] += 1
            entry = {"id": wid, "type": c.tag, "k": round(ks[0], 4),
                     "rect": [round(l, 2), round(t, 2), round(r, 2), round(b, 2)], "hidden": hidden}
            if art and art["tex"] == "textures/ui/icon_dummy.png":
                # The "?" placeholder of an icon the game sets at run time.
                entry["hidden"] = True
            if art:
                entry["art"] = art
                stats["art"] += 1
                stats["triangles"] += len(art["idx"]) // 3
            if "font" in a:
                raw = a.get("Text", "")
                text = raw
                if raw.startswith("gui."):
                    used_keys.add(raw)
                    text = loca.get("fr", {}).get(raw) or loca.get("en", {}).get(raw) or raw
                entry["text"] = {
                    "text": text, "key": raw,
                    "size": float(a.get("fontSize", "10") or 10),
                    "color": gx.fvals(a.get("fontColor", "1,1,1,1"), 4, 1.0),
                    "align": [gx.H_ALIGN.get(a.get("HorizontalAlignment"), 0), gx.V_ALIGN.get(a.get("VerticalAlignment"), 0)],
                    "bold": a.get("bold") == "true",
                }
                stats["texts"] += 1
            widgets.append(entry)
            stats["widgets"] += 1
            walk(c, wid, (l, t, r, b), ks, hidden)

    walk(doc, "", (0.0, 0.0, BASE_W, BASE_H), (0.0, 1.0), False)
    for (win, template, area) in GRIDS:
        if win == name:
            widgets = grid(widgets, template, area)
    with open(os.path.join(out_dir, name + ".ui.json"), "w", encoding="utf-8") as f:
        json.dump({"window": name, "base": [BASE_W, BASE_H], "widgets": widgets}, f, ensure_ascii=False, separators=(",", ":"))
    return stats


totals, failed = {}, []
for name in only:
    if not os.path.exists(os.path.join(root, "ui", name + ".bxml")):
        print(f"{name}: no such window")
        continue
    try:
        st = export(name)
    except Exception as e:  # noqa: BLE001 -- one broken window does not stop the rest
        failed.append(f"{name}: {type(e).__name__}: {e}")
        continue
    for k, v in st.items():
        totals[k] = totals.get(k, 0) + v
print(f"{len(only) - len(failed)} windows -> {out_dir}", totals)
for f in failed:
    print("  !!", f)
# Skill icons: _Template_Skill.IconBrush ("icons/mage_skill02b") as PNG, by id.
import sqlite3  # noqa: E402
con = sqlite3.connect(f"file:{os.path.join(root, 'db', 'static.db4')}?mode=ro", uri=True)
icons = {}
for sid, brush in con.execute("select Id, IconBrush from _Template_Skill"):
    if brush and tex.get(brush):
        icons[sid] = f"textures/{brush}.png"
with open(os.path.join(out_dir, "skills.icons.json"), "w") as f:
    json.dump(icons, f, separators=(",", ":"))
print(f"skill icons: {len(icons)}")
for lang in ("fr", "en"):
    table = loca.get(lang, {})
    with open(os.path.join(out_dir, f"loca_{lang}.json"), "w", encoding="utf-8") as f:
        json.dump({k: table[k] for k in sorted(used_keys) if k in table}, f, ensure_ascii=False, indent=0)
if tex.missing:
    print("missing textures:", sorted(tex.missing))
