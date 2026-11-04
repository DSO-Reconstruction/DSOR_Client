"""Instantiate current UI templates using the release database's own ordering."""
import copy

# niwalk (4): the Night Walker, whose spell book the original laid out
# (_Template_SkillTalent / _Template_SkillUIConfig) but never made live; the
# spells DSOR_Server's tools/install_niwalk_content.py installs show.
CLASSES = ("warrior", "mage", "ranger", "dwarf", "niwalk")


def translated_subtree(widgets, source, target, dx=0, dy=0):
    result = []
    for original in widgets:
        if original["id"] != source and not original["id"].startswith(source + "/"):
            continue
        w = copy.deepcopy(original)
        w["id"] = w["id"].replace(source, target, 1)
        if w.get("button"):
            w["button"] = w["button"].replace(source, target, 1)
        w["rect"] = [v + (dx if i % 2 == 0 else dy) for i, v in enumerate(w["rect"])]
        if "art" in w:
            w["art"]["pos"] = [v + (dx if i % 2 == 0 else dy) for i, v in enumerate(w["art"]["pos"])]
        result.append(w)
    return result


def quad(rect, texture, color=(1, 1, 1, 1)):
    l, t, r, b = rect
    return dict(tex=texture, pos=[l,t,r,t,r,b,l,b], uv=[0,0,1,0,1,1,0,1], color=list(color)*4, idx=[0,1,2,0,2,3])


def localized(loca, key, fallback):
    return loca.get("en", {}).get(key) or fallback


def spell_grid(widgets, db, textures, loca):
    from tooltip_catalog import SkillPreview
    preview = SkillPreview(db)
    source = next(w for w in widgets if w["id"].endswith("/Skillgrid/TemplateSkill"))
    talent = next(w for w in widgets if w["id"].endswith("/Skillgrid/TemplateTalent"))
    background = next(w for w in widgets if w["id"].endswith("/ExperienceLabel/background_classes"))
    l, t, r, b = source["rect"]
    width, height = r-l, b-t
    grid = source["id"].rsplit("/", 1)[0]
    # A cell's centre is the grid's corner + (Column + 0.5, Row + 0.5) cells,
    # TemplateSkill's size (release261 SkillTalentsWidget 0x14036c93c).
    gl, gt = next(w for w in widgets if w["id"] == grid)["rect"][:2]
    rows = db.execute('''SELECT t.Id, t.CharClass, t.Icon, t.SkillTemplateId,
        t.UnlockLevel, t.Category, u.Column, u.Row, s.rowid, s.UnlockLevel,
        s.Name, s.IconBrush FROM _Template_SkillTalent t
        JOIN _Template_SkillUIConfig u ON u.Id=t.Id
        LEFT JOIN _Template_Skill s ON s.Id=t.SkillTemplateId
        WHERE t.CharClass IN ('warrior','mage','ranger','dwarf','niwalk')
        AND (t.Category != 'Skill' OR (s.Hidden=0 AND s.IsLive=1))
        ORDER BY t.CharClass, u.Row, u.Column''').fetchall()
    added = []
    for tid, cls, icon, sid, unlock, category, column, row, wire, skill_unlock, name, skill_icon in rows:
        class_id = CLASSES.index(cls)
        active = category == "Skill" and wire is not None
        template = source if active else talent
        target = f"{grid}/Class{class_id}/{'Spell'+str(wire-1) if active else 'Talent'+tid}"
        tl, tt, tr, tb = template["rect"]
        dx = gl + (column + 0.5)*width - (tl + tr)/2
        dy = gt + (row + 0.5)*height - (tt + tb)/2
        copies = translated_subtree(widgets, template["id"], target, dx, dy)
        brush = skill_icon if active else icon
        if not brush or not textures.get(brush):
            continue
        caption = localized(loca, f"db.skill2.table_{sid}Title", localized(loca, f"db.skill.table_{name}_defaultTitle", sid)) if active else localized(loca, f"db.skilltalent2.table_{tid}Title", tid)
        meta = dict(id=sid if active else tid, wire=wire-1 if active else None, class_id=class_id,
                    unlock=max(1, (skill_unlock if active else unlock) or 1), name=caption,
                    icon=f"textures/{brush}.png", column=column, row=row,
                    target=localized(loca, f"db.skill2.table_{sid}SkillTarget", "") if active else "",
                    description=localized(loca, f"db.skill2.table_{sid}Description", localized(loca, f"db.skill.table_{name}_defaultDesc", "")) if active else localized(loca, f"db.skilltalent2.table_{tid}Description", ""))
        meta['preview_description'] = preview.resolve(meta['description'], sid)
        meta['growth'] = preview.resolve(localized(loca, f"db.skilltalent2.table_{tid}Description", ""), sid) if active else ''
        talent_meta = preview.talents.get(tid, {})
        meta['talent_cost'] = int(talent_meta.get('CostsPerLevel') or 0)
        for w in copies:
            leaf = w["id"].rsplit("/", 1)[-1]
            if "glow" in w["id"].lower() and not w.get("state"):
                w["hidden"] = True
            if leaf in ("SlotButton", "TalentButton"):
                w["spell"] = meta
                w["event"] = "DragSpell" if active else "InspectTalent"
            is_icon = (active and leaf == "SkillIcon" and w.get("state")) or (not active and leaf in ("normal", "pressed", "mouseover", "disabled"))
            if is_icon:
                tint = (.30,.30,.30,1) if w.get("state") == "disabled" else (1,1,1,1)
                w["art"] = quad(w["rect"], meta["icon"], tint)
                w["hidden"] = False
                w.pop("text", None)
            if leaf == "UnlockInfo":
                w["text"].update(text=str(meta["unlock"]), key=str(meta["unlock"]))
            if leaf == "UpgradeCounter":
                w["hidden"] = True  # actual allocation arrives separately
        added.extend(copies)
    # The class artwork is the current grid backdrop. The generic blue artwork
    # and older preset controls are separate alternatives, not extra overlays.
    for w in widgets:
        if w["id"].endswith("/ExperienceLabel/artworkExtension"):
            w.pop("art", None)
    index = widgets.index(background)
    backdrops = []
    # The Night Walker has no backdrop of its own (ui/skilltalent_backgound_niwalk
    # is named by the client, absent from the data): the warrior's.
    for cls, suffix in enumerate(("dragonknight", "mage", "ranger", "dwarf", "dragonknight")):
        brush = f"ui/skilltalent_backgound_{suffix}_crunch"
        if textures.get(brush):
            w = copy.deepcopy(background)
            w["id"] = background["id"].rsplit("/", 1)[0] + f"/Class{cls}/background_classes"
            w["art"]["tex"] = f"textures/{brush}.png"
            backdrops.append(w)
    widgets[index:index+1] = backdrops
    return widgets + added


def ranking_categories(widgets, db, loca):
    templates = {name: next(w for w in widgets if w["id"].endswith('/'+name)) for name in
                 ("TemplateCategoryPvP", "TemplateCategoryPvE", "TemplateCategoryEvent", "TemplateSubCategory")}
    categories = db.execute('SELECT RankCategory, RankParent, RankPriority FROM _Template_LeaderBoardCategory WHERE IsActive=1 ORDER BY RankPriority').fetchall()
    parent = templates["TemplateCategoryPvP"]["id"].rsplit('/', 1)[0]
    added = []
    for group, (category, template_name) in enumerate((("PVP_Parent", "TemplateCategoryPvP"), ("PVE_Parent", "TemplateCategoryPvE"), ("Event_Parent", "TemplateCategoryEvent"))):
        template = templates[template_name]
        entries = [(category, template, f'{parent}/RankGroupButton{group}', 108.5+group*42, f'RankGroup:{group}')]
        for i, (name, _, _) in enumerate(c for c in categories if c[1] == category):
            entries.append((name, templates['TemplateSubCategory'], f'{parent}/RankGroup{group}/RankCategory{i}', 245+i*32, f'RankCategory:{i}'))
        for name, template, target, top, event in entries:
            copies = translated_subtree(widgets, template['id'], target, 0, top-template['rect'][1])
            for w in copies:
                w['hidden'] = False
                if w['id'] == target: w['event'] = event
                if w.get('text'):
                    w['text'].update(text=localized(loca, f'db.leaderboard.table_{name}Name', name), key='gui.client.rank.category', size=8, word_break=True)
            added.extend(copies)
    for w in widgets:
        if '/TabButton0' in w['id'] and w.get('text'):
            number = int(w['id'].split('/TabButton0')[1][0])
            caption = ['All', 'Dragonknight', 'Spellweaver', 'Ranger', 'Steam Mechanicus'][number-1]
            class_key = [None, 'Warrior', 'Mage', 'Ranger', 'Dwarf'][number-1]
            key = f'gui.window.leaderboards_filterClass{class_key}' if class_key else 'gui.client.rank.class'
            # Keep the full English class name within the fixed-width tab.
            w['text'].update(text=localized(loca, key, caption), key=key, size=7 if number == 5 else 8)
        if w['id'].endswith('/TextLabel') and '/DropDownMenuButton/' in w['id']:
            w['text'].update(text='Rankings', key='gui.client.rank.selection')
    empty_key = 'gui.window.leaderboards_noDataMessage'
    added.append(dict(id='Canvas/WindowBackground/ContentFrame/EmptyMessage', type='TextLabel', k=0.5,
        rect=[270,365,930,420], hidden=False,
        text=dict(text=localized(loca, empty_key, 'No rankings available.'), key=empty_key, size=12, color=[.9,.78,.54,1], align=[1,1])))
    return widgets + added
