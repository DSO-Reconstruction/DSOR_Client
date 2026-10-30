#!/usr/bin/env python3
"""Export the client's interface windows (export_win32/ui) for client/src/ui.

Usage: export_ui.py <export_win32> <assets> <DSO_Godot> [window ...] [--database static.db4]

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
                "text": {"text", "key", "size", "color", "align": [h, v], "bold",
                         "outline": [size, r, g, b, a] (optional)}}]}
in drawing order (parents first), positions in reference pixels (y down), the
artwork as the mesh group's own triangles, UVs and vertex colours (the engine
draws them as they are: quads, nine-slices, the globes' polygons), plus the
atlases as <assets>/textures/ui/*.png and <assets>/interface/loca_<lang>.json
(the texts of the "gui." keys the windows use).
Button states and runtime pages are preserved for the client controller.
Decorative alternatives and data templates remain hidden until needed.
"""
import json, os, sys, sqlite3
from PIL import Image  # Fail before writing any layouts if conversion deps are missing.

root, assets, godot = sys.argv[1], sys.argv[2], sys.argv[3]
# The converter's Unix default omits .exe, so Windows silently skipped new
# Crunch artwork even when the bundled transcoder was installed.
crunch = os.path.abspath(os.path.join(godot, 'build', 'crn2dds.exe'))
if os.name == 'nt' and os.path.isfile(crunch):
    os.environ.setdefault('DSO_CRN2DDS', crunch)
# Every window by default ("--hud": the four the HUD needs).
only = sys.argv[4:]
database = os.path.join(root, "db", "static.db4")
if "--database" in only:
    at = only.index("--database")
    database = only[at + 1]
    del only[at:at + 2]
only = only or sorted(f[:-5] for f in os.listdir(os.path.join(root, "ui")) if f.endswith(".bxml"))
from ui_catalog import localized, spell_grid, ranking_categories
catalog = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
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
    r"^(cover|selected|highlight|flash|checked)$"
    r"|glow|Glow|sparkle|Sparkle|shine|Shine|flare"
    r"|^EmptyQuickbar|^ButtonBuy|^ButtonDeluxe|^MultipleBars|_locked$|^Quickslots_old$"
    r"|^LatencyIndicator(Red|Yellow)$|^figur$|^FlagLabel(Category|Subcategory)$"
    r"|^ammo_|^ProgressBar_ammo|^TemplateGlow|(?:^|_)[Hh]ighlight\d*$")


# Slots the game clones from a template over an area, row by row: (window,
# template id, area id). The template itself is a runtime ghost ("?" icon).
GRIDS = [("inventory", "TemplateSlot", "icon_area"),
         ("inventory", "IngredientBag/Label_bg/icon_area/TemplateSlot", "IngredientBag/Label_bg/icon_area")]


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
    cols = int((ar - al + 2.0) // step_x)
    rows = int((ab - top + 2.0) // step_y)
    # Fit the grid to the AREA, not the template's arbitrary authored offset.
    # The inventory template starts 11px inside its area; using it as the left
    # boundary lost the seventh column in the current client's assets.
    start_x = al + ((ar - al) - (cols * step_x - 2.0)) / 2.0
    clones = []
    for row in range(rows):
        for col in range(cols):
            dx, dy = start_x - l + col * step_x, row * step_y
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
    "bottombar": ["Canvas/Label_new"],
    "inventory": ["Canvas/PositioningLabel"],
    # The money display the game shows in game, with the currency bag
    # (LabelIngamePayment; the client picks it whatever this export marks:
    # crate::ui::controls::currency).
    "currencybar": ["Canvas/LabelIngamePayment", "Canvas/AnchorSidebarCurrencyDisplay"],
    "skillwindow2": ["Canvas/Label"],
}

# These are application-controlled pages, not decorative alternatives. Preserve
# every page, including those gx.exclusive would hide by matching rectangles.
PANELS = {"oben", "IngredientBag", "TrinketBag", "mercenary", "StatsCharacter",
          "StatsPVP", "AchievementTitles", "VeteranStatus", "MembershipStatus",
          "Premium", "Deluxe", "NoPremium",
          "QuestlogLabel", "CompendiumLabel", "AchievementsLabel", "DailyAchievementsLabel", "WantedMissionsLabel",
          "OptionsGroup", "OptionsGroupFinder", "OptionsGuild", "OptionsFriends",
          "TableHeadSearch", "TableHeadFriends", "TableHeadGroup", "TableHeadGroupFinder", "TableHeadGuild", "TableHeadIgnore",
          "Duria", "Teganswall", "Northlands", "Dwarflands", "Parallelworlds", "Atlantis", "Lortac", "Qaizah", "LandOfImmortals",
          "DealsPage", "OffersPage", "TieredSpenderLabel", "PvP", "PvE", "Event"}
BUTTONS = {"Button", "TextButton", "SlotButton"}
STATES = {"normal", "mouseover", "pressed", "mouseoverpressed", "disabled"}


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
    art = {
        "tex": f"textures/{ref}.png" if t else None,
        "pos": [round(c, 2) for v in verts for c in (cx0 + v[0], cy0 + v[1])],
        "uv": [round(c, 6) for u in uvs for c in u],
        "color": [round(c, 4) for col in cols for c in col],
        "idx": [i for f in faces for i in f],
    }
    # Saturation (1 unchanged, 0 greyscale): the element's own, not its
    # children's (release261 UI::Element::SetSaturation 0x14072a908). Every
    # disabled button state is 0.
    saturation = gx.fvals(a.get("Saturation", "1"), 1, 1.0)[0]
    if saturation < 0.999:
        art["saturation"] = round(max(0.0, saturation), 4)
    return art, None


def local_map_controls(widgets):
    """The runtime overlay owns its size, caption and click-through behaviour."""
    return [dict(w, click_through=True) for w in widgets if w["id"] in {"Canvas", "Canvas/map"}]


def export_map_names(loca, out_dir):
    """Use the same English localization as the rest of the current interface."""
    prefix = "maps.names_"
    names = {key[len(prefix):]: text for key, text in loca.get("en", {}).items()
             if key.startswith(prefix) and text.strip()}
    names.update(travel_details(loca))
    with open(os.path.join(out_dir, "maps.names.json"), "w", encoding="utf-8") as f:
        json.dump(names, f, ensure_ascii=False, separators=(",", ":"))


def travel_details(loca):
    """The travel map's destinations: "travel_desc:<map>" (db.travelmap
    <map>Desc, "Urban Area", else gui.window MapTypePvE "Wilderness" for an
    unlocalized one), "travel_vc:<map>" / "travel_rc:<map>" (_Template_TravelCosts)."""
    import sqlite3
    out = {}
    en = loca.get("en", {})
    wilderness = en.get("gui.window_MapTypePvE") or "Wilderness"
    try:
        db = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
        for (map_id,) in db.execute("SELECT Id FROM _Template_TravelMap"):
            desc = en.get(f"db.travelmap_{map_id}Desc", "")
            out[f"travel_desc:{map_id}"] = wilderness if not desc.strip() or desc.startswith("Localize:") else desc
        for map_id, rc, vc in db.execute("SELECT Id, PriceRC, PriceVC FROM _Template_TravelCosts"):
            out[f"travel_vc:{map_id}"] = str(int(vc or 0))
            out[f"travel_rc:{map_id}"] = str(int(rc or 0))
        # The map access window's tiers (_Template_MapAccessGroup: a row per
        # difficulty of one AccessGroup) and the tiers' names.
        # Per row also what the window shows: the artwork, the descriptions
        # (gui.window.dungeonaccess_<key>), the resistances, the possible rewards
        # and the items the travel item's teleport takes (TeleportKeyId).
        access = lambda key: en.get(f"gui.window.dungeonaccess_{key}", "") if key else ""
        for row, group, difficulty, key, bg, desc, desc2, res, rewards, needs, quest in db.execute(
                "SELECT Id, AccessGroup, Difficulty, KeyId, BackgroundImage, DescriptionText, DescriptionText2, Resistances,"
                " RewardItems, TeleportKeyId, UnlockingQuest FROM _Template_MapAccessGroup"):
            out[f"access_group:{row}"] = group or ""
            out[f"access_diff:{row}"] = str(int(difficulty or 0))
            if key: out[f"access_key:{row}"] = key
            # UnlockingQuest: the quest whose completion opens the row (DSOR_Server
            # map_unlocks.map_unlocking_quests refuses the travel before it).
            for name, value in (("bg", bg), ("desc", access(desc)), ("desc2", access(desc2)), ("res", res), ("rewards", rewards), ("needs", needs),
                                ("quest", quest)):
                if value: out[f"access_{name}:{row}"] = value
        for group, level, players in db.execute("SELECT AccessGroup, MapLevel, MaxPlayer FROM _Template_MapTemplate"):
            out[f"access_level:{group}"] = str(int(level or 0))
            out[f"access_players:{group}"] = str(int(players or 0))
        for level, plus, magic, rare, epic, uncommon in db.execute("SELECT Id, MonsterLevelPlus, Magic, Rare, Epic, Uncommon FROM _Template_MapDifficulty"):
            out[f"difficulty_mods:{level}"] = f"{int(plus or 0)},{int(magic or 0)},{int(rare or 0)},{int(epic or 0)},{int(uncommon or 0)}"
        # MapAccessWidget::SetDifficultyInfoText's icon lines (_Template_DifficultyFactors).
        for row in db.execute("SELECT Id, MonsterDMG, MonsterHP, RMESecondaryMin, RMESecondaryMax, XPGain, VCStackSize FROM _Template_DifficultyFactors"):
            out[f"difficulty_factors:{row[0]}"] = ",".join(repr(float(v or 0)) for v in row[1:])
        for key in ("possibleRewardsAndRisks", "dungeonPrerequisites", "maxGroupSize", "enterDungeon", "difficultyModifications", "recommendedLevel",
                    "enterDungeonDisabledTooltip"):
            out[f"access_text:{key}"] = access(key)
        for level, name in db.execute("SELECT Id, Name FROM _Template_MapDifficulty"):
            out[f"difficulty:{level}"] = name or ""
    except sqlite3.Error as e:
        print("travel details:", e)
    return out


def export(name):
    path = os.path.join(root, "ui", name + ".bxml")
    doc = bxml.parse(open(path, "rb").read())
    # Keep a readable, lossless source beside the generated layout for audits.
    xml_dir = os.path.join(out_dir, "xml")
    os.makedirs(xml_dir, exist_ok=True)
    with open(os.path.join(xml_dir, name + ".xml"), "w", encoding="utf-8") as f:
        f.write(bxml.to_xml(doc))
    mpath = os.path.join(root, "meshes", "ui", name + "_s_0.nvx2")
    mesh = gx.UiMesh(mpath) if os.path.isfile(mpath) else None
    widgets, stats = [], {"widgets": 0, "art": 0, "triangles": 0, "no texture": 0, "texts": 0}

    roots = ROOTS.get(name)

    def walk(node, prefix, parent_rect, pk, hidden_parent, button=None, state=None):
        hide_idx = gx.exclusive(node)
        children = list(enumerate(node.children))
        if name == "inventory" and prefix.endswith("/IngredientBag/Label_bg"):
            # The authored background comes last in XML but belongs behind
            # the category/tier controls and the collection slots.
            children.sort(key=lambda pair: pair[1].attrs.get("id") != "Artwork")
        if name == "skilltalentswindow":
            # These authored backgrounds appear after the controls in the XML;
            # the original window controller sends them behind its content: the
            # Group tab's artwork (Label2) covered its level bar, the Honor tab's
            # boxes (Talents_Background, Traits_Background) its talents.
            children.sort(key=lambda pair: pair[1].attrs.get("id") not in {"background", "background_classes", "Label2",
                                                                            "Talents_Background", "Traits_Background"})
        if node.tag in BUTTONS:
            # Nebula buttons paint their state background before their common
            # icon/label children, even when the XML lists the icon first.
            # Flattening source order put opaque button faces over menu icons.
            children.sort(key=lambda pair: pair[1].attrs.get("id") not in STATES)
        for i, c in children:
            if c.tag in ("Nebula3", "Window"):
                walk(c, prefix, parent_rect, pk, hidden_parent)
                continue
            a = c.attrs
            state_child = node.tag in BUTTONS and a.get("id") in STATES
            owner = prefix if state_child else button
            visual_state = a.get("id") if state_child else state
            rect = gx.fvals(a.get("rect", "0,0,1,1"), 4)
            l, t, r, b = rect[0] * BASE_W, rect[1] * BASE_H, rect[2] * BASE_W, rect[3] * BASE_H
            # No alignment: the engine's default, the parent's top left (the
            # currency bar's canvas has none and sits in the screen's corner).
            ax = gx.H_ANCHOR.get(a.get("HorizontalParentAlignment"),
                                 0.5 if name == "ingamemenu" and c.tag == "Canvas" else 0.0)
            ay = gx.V_ANCHOR.get(a.get("VerticalParentAlignment"), 0.0)
            # (k of the left edge, k of the right edge) of the parent.
            kl = pk[0] + ax * (pk[1] - pk[0])
            stretch = parent_rect == (0.0, 0.0, BASE_W, BASE_H) and l <= 0.02 * BASE_W and r >= 0.98 * BASE_W
            ks = (0.0, 1.0) if stretch else (kl, kl)
            wid = (prefix + "/" if prefix else "") + (a.get("id") or c.tag)
            outside = roots is not None and not any(wid == r or wid.startswith(r + "/") or r.startswith(wid + "/") for r in roots)
            alternative = i in hide_idx and not state_child and a.get("id") not in PANELS
            hidden = (hidden_parent or alternative or a.get("Visible") == "false"
                      or (runtime_only(c) and visual_state is None) or outside)
            w, h = max(r - l, 0.0), max(b - t, 0.0)
            art, why = widget_art(c, mesh, l + w / 2.0, t + h / 2.0)
            if why:
                stats[why] += 1
            entry = {"id": wid, "type": c.tag, "k": round(ks[0], 4),
                     "rect": [round(l, 2), round(t, 2), round(r, 2), round(b, 2)], "hidden": hidden}
            if owner:
                entry["button"] = owner
                entry["state"] = visual_state
            if a.get("event"):
                entry["event"] = a["event"]
            if a.get("ClickThrough") == "true":
                entry["click_through"] = True
            if a.get("Enabled") == "false":
                entry["enabled"] = False
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
                    text = localized(loca, raw, raw)
                entry["text"] = {
                    "text": text, "key": raw,
                    "size": float(a.get("fontSize", "10") or 10),
                    "color": gx.fvals(a.get("fontColor", "1,1,1,1"), 4, 1.0),
                    "align": [gx.H_ALIGN.get(a.get("HorizontalAlignment"), 0), gx.V_ALIGN.get(a.get("VerticalAlignment"), 0)],
                    "bold": a.get("bold") == "true",
                    "word_break": a.get("WordBreak") == "true",
                }
                # TextEffect 1: an outline of TextEffectSize pixels in TextEffectColor
                # (every combat, name and money label).
                if a.get("TextEffect") == "1" and float(a.get("TextEffectSize", "0") or 0) > 0:
                    entry["text"]["outline"] = [float(a.get("TextEffectSize"))] + gx.fvals(a.get("TextEffectColor", "0,0,0,1"), 4, 1.0)
                stats["texts"] += 1
            widgets.append(entry)
            stats["widgets"] += 1
            # A window's root Canvas lays its children out against the whole
            # screen, whatever its own rect: a panel aligned Left or Right
            # follows that screen edge (the character sheet, the recipe book
            # and the chat on the left; the social screen, the party frames
            # and the quest tracker on the right), HCenter its centre.
            # FAILURE (2026-10-20): with the canvas's own (centred) k, the
            # quest tracker stood 170 px from the right edge of a 16:9 screen.
            child_k = (0.0, 1.0) if c.tag == "Canvas" and not prefix else ks
            walk(c, wid, (l, t, r, b), child_k, hidden, owner, visual_state)

    walk(doc, "", (0.0, 0.0, BASE_W, BASE_H), (0.0, 1.0), False)
    if name == "inventory":
        for w in widgets:
            if w["id"].endswith("/unten/Label_bg/Label_empty/TextLabel_message"):
                key = "gui.window.inventory_overflowSlotsDescribtion"
                w["text"].update(text=localized(loca, key, "Free up slots to collect these items automatically."), key=key)
            if "/IngredientBag/" in w["id"] and any(part in w["id"].split('/') for part in ("icon_area_cores", "RecipesInfotextArea")):
                # Alternative runtime templates must not cover the tier buttons.
                w["hidden"] = True
    if name == "skilltalentswindow":
        for w in widgets:
            # These four vertical progression models are not supplied yet.
            # Keep their frames, without the source's full-bar sample values.
            if w["id"].endswith("/LevelBar/progress"):
                w["hidden"] = True
            if "/LevelBar/" in w["id"] and w["id"].endswith("/LevelLabel"):
                w["text"].update(text="—", key="—")
    if name == "skilltalentswindow": widgets = spell_grid(widgets, catalog, tex, loca)
    if name == "leaderboards": widgets = ranking_categories(widgets, catalog, loca)
    # collectorsalbum: the client fills Categorypage and the category list
    # (controls::systems::album); no pages are added.
    if name == "locationmap": widgets = local_map_controls(widgets)
    if name == "shopwindow":
        # The game assigns these labels along with its catalogue. Keep the
        # front-end pages usable while no catalogue service is connected.
        for w in list(widgets):
            if w["id"].endswith("/Headline/WindowTitle"):
                # The source keeps the skills-window placeholder title.
                key = "gui.window.shop_windowTitle"
                w["text"].update(text=localized(loca, key, "Shop"), key=key)
            caption = {"ShopTab1": "Offers", "ShopTab2": "Items", "ShopTab3": "Bonus Code"}.get(w["id"].rsplit("/", 1)[-1])
            if caption and w["type"] == "Button":
                widgets.append(dict(id=w["id"]+"/Caption", type="TextLabel", k=w["k"], rect=w["rect"], hidden=w["hidden"],
                                    text=dict(text=caption, key="gui.client.shop.tab", size=10, color=[.90,.78,.54,1], align=[1,1])))
    if name == "matchmakingwindow":
        for w in widgets:
            leaf = w["id"].rsplit("/", 1)[-1]
            if leaf == "LeaderboardRankNumber" or leaf in {"LeaderboardScoreValue", "MatchesPlayedValue", "MatchesWonValue", "MatchesDrawValue", "PlayersDefeatedValue"}:
                w["text"].update(text="—", key="—")
            if leaf == "NoLeaderBoardSeasonText":
                # The source spans all three columns although the stats panel
                # occupies the first one. Keep this message in the other two.
                w["rect"][0] = 376.0
                w["text"]["word_break"] = True
    if name == "eventwindow":
        empty = next(w for w in widgets if w["id"].endswith("/BottomNoEvent"))
        widgets.append(dict(id=empty["id"]+"/EmptyMessage", type="TextLabel", k=empty["k"],
                            rect=[90,270,920,320], hidden=False,
                            text=dict(text="No events available.",key="gui.client.events.empty",size=12,color=[.90,.78,.54,1],align=[1,1])))
    for (win, template, area) in GRIDS:
        if win == name:
            widgets = grid(widgets, template, area)
    # Some source icons contain an identical copy of themselves as a child.
    # Drawing both makes the normal icon look permanently highlighted.
    by_id = {w["id"]: w for w in widgets}
    for w in widgets:
        parent = by_id.get(w["id"].rsplit("/", 1)[0])
        if (not w["hidden"] and w.get("art") and parent and not parent["hidden"]
                and parent.get("art") == w["art"] and parent.get("state") == w.get("state")):
            parent.pop("art")
    used_keys.update(w["text"]["key"] for w in widgets if w.get("text") and w["text"]["key"].startswith(("gui.", "db.")))
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
con = catalog
icons = {}
for sid, brush, char_class in con.execute("select Id, IconBrush, CharClass from _Template_Skill"):
    if brush and tex.get(brush):
        icons[sid] = f"textures/{brush}.png"
        # Keep the old aliases readable; release261 packets and offline play
        # use the canonical template id.
        if char_class in {"warrior", "mage", "ranger", "dwarf"}:
            icons[f"{char_class}_{sid}_default"] = icons[sid]
with open(os.path.join(out_dir, "skills.icons.json"), "w") as f:
    json.dump(icons, f, separators=(",", ":"))
print(f"skill icons: {len(icons)}")
# Runtime map markers are not referenced by the static window meshes.
from pathlib import Path
# Cursors are selected at runtime, so no window mesh references these textures.
# Preserve the original RGBA pixels and colour variants for native/web cursors.
for source in sorted((Path(root) / "textures" / "mousecursor").glob("*.*")):
    tex.get("mousecursor/" + source.stem)
for source in sorted((Path(root) / "textures" / "ui").glob("icon_map_*.*")):
    tex.get("ui/" + source.stem)
# The notification column's icons, chosen per NotificationType at run time
# (UI::NotificationWidget, 2018 client 0x6CAE52).
for source in sorted((Path(root) / "textures" / "ui").glob("icon_notifications_*.*")):
    tex.get("ui/" + source.stem)
# The bottom bar's resource gauge per class, set at run time (release261
# 0x1403ACD74): rage, mana, adrenaline, steam.
for name in ("rage", "mana", "adrenaline", "steam"):
    tex.get("ui/statusbar_bottombar_" + name)
# The social window's rows: the online / offline status and the class icons
# (UI::SocialWidget's list elements, release261 0x14052c55c).
for name in ("status_online", "status_offline", "member_warrior", "member_wizard", "member_ranger", "member_dwarf"):
    tex.get("ui/icon_" + name)
for table in ("_Template_NPC", "_Template_StateObject"):
    for (brush,) in con.execute(f"SELECT DISTINCT LocationMapIcon FROM {table}"):
        if brush:
            tex.get(brush.replace("\\", "/"))
export_map_names(loca, out_dir)
for lang in ("fr", "en"):
    table = loca.get(lang, {})
    with open(os.path.join(out_dir, f"loca_{lang}.json"), "w", encoding="utf-8") as f:
        json.dump({k: table[k] for k in sorted(used_keys) if k in table}, f, ensure_ascii=False, indent=0)
if tex.missing:
    print("missing textures:", sorted(tex.missing))
if "inventory" in only or "tradingscreen" in only:
    from inventory_catalog import export_inventory
    print("inventory catalog (items, collection entries):", export_inventory(catalog, tex, loca, assets))
if "inventory" in only:
    from equipment_visuals import export_equipment
    export_equipment(catalog, assets)
if "bottombar" in only:
    # The status effects above the action bar (BuffBar): which, where, their icons.
    from status_catalog import export_status
    print("status effects:", export_status(catalog, tex, loca, assets))
if "skilltalentswindow" in only:
    # The Skills & Talents window's trees: costs, levels, names, icons.
    from talent_catalog import export_talents
    print("talents:", export_talents(catalog, tex, loca, assets))
if "npcscreen" in only or "tradingscreen" in only:
    from dialogue_catalog import export_dialogue
    print("NPC dialogue:", export_dialogue(catalog, tex, loca, assets))
