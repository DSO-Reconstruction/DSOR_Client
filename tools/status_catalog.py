"""The bottom bar's status effects (bottombar BuffBar/Buffs and BuffBar/Debuffs).

Usage (alone, without re-exporting any window):
  status_catalog.py <export_win32> <assets> <DSO_Godot> [--database static.db4]
export_ui.py also writes it whenever it exports `bottombar`.

Writes <assets>/interface/status.catalog.json and the effects' icons as PNG:
  {"units": [seconds, minutes, hours, days formats],
   "configs": {IconConfiguration: [background, frame]},
   "effects": {wire index: {"id", "debuff", "icon", "config", "name", "stacks",
                            "lines": [{"text", "minus", "args"}]}}}

_Template_StatusEffect.Groups decides where an effect shows: BuffUI on the
positive (left) half, DebuffUI on the negative (right) half. Every other
effect (boss idles, triggers, the carriers of auras) is never drawn. IconBrush
is the image; IconConfiguration names its background and frame
(_Template_IconConfig: statuseffect_buff / _debuff, item_costume...).
The name is db.statuseffect.table <LocaleId, else Id + "Title">.

Hover lines are the client's modifier texts (gui.statuseffects.modifier): a
modifier's `#key` override, else the generic text of its kind and sign
(damageRelativePlus "+ {0:s}% damage"); an override takes the same arguments
as its kind's generic text (DropAmount: the item, then the value). A value is
a wire parameter ($0: the element's own parameters), an immutable actor
variable or a literal; a mutable variable or a level-scaled one stays
unresolved (the client shows an em dash).
"""
import json
import os
import re
import sys

ELEMENTAL = ("Fire", "Ice", "Lightning", "DarkMagic", "Poison")
# (relative plus, relative minus, absolute plus, absolute minus): the generic
# texts, spelt as the localization spells them.
GENERIC = {
    "MaxHealthPoints": ("maxHealthPointsRelativePlus", "maxHealthPointsRelativetMinus",
                        "maxHealthPointsAbsolutPlus", "maxHealthPointsAbsolutMinus"),
    "BlockValue": ("blockvalueRelativePlus", "blockvalueRelativeMinus", "blockvalueAbsolutPlus", "blockvalueAbsolutMinus"),
    "Block": ("blockRelativePlus", "blockRelativeMinus", "blockvalueAbsolutPlus", "blockvalueAbsolutMinus"),
    "CriticalValue": ("critRelativPlus", "critRelativMinus", "critAbsolutPlus", "critAbsolutMinus"),
    "XPGain": ("xpGainRelativePlus", "xpGainRelativeMinus", "xpGainAbsolutPlus", "xpGainAbsolutMinus"),
    "HonorGain": ("honorGainRelativePlus", "honorGainRelativeMinus", "honorGainAbsolutePlus", "honorGainAbsoluteMinus"),
    "DamageFactorPvP": ("damageFactorPvPRelativePlus", "damageFactorPvPRelativeMinus",
                        "damageFactorPvPAbsolutPlus", "damageFactorPvPAbsolutMinus"),
    "DamageFactorPvE": ("damageFactorPvERelativePlus", "damageFactorPvERelativeMinus",
                        "damageFactorPvEAbsolutPlus", "damageFactorPvEAbsolutMinus"),
    "DamagePermeabilityPvP": ("damagePermeabilityPvPRelativePlus", "damagePermeabilityPvPRelativeMinus",
                              "damagePermeabilityPvPAbsolutPlus", "damagePermeabilityPvPAbsolutMinus"),
    # The Cute Doggy's pet effect: "Selling price of items increased by 20.00%".
    "VCSellPrice": ("vcSellPriceRelativePlus", "vcSellPriceRelativeMinus", "vcSellPriceAbsolutPlus", "vcSellPriceAbsolutMinus"),
}


def _number(token, relative, variables, scaled):
    """A modifier's value: {"p": n} a wire parameter, {"v": x} a known value,
    {"var": name} a mutable actor variable (the client may know it: the pets'
    Collector's Bag bonuses, crate::ui::controls::systems::album), {} unknown."""
    scale = {"scale": 100.0} if relative else {}
    token = (token or "").strip()
    if scaled:
        return scale
    if token.startswith("$"):
        name = token[1:]
        if name.isdigit():
            return {"p": int(name), **scale}
        value = variables.get(name)
        return {"v": round(value, 6), **scale} if value is not None else {"var": name, **scale}
    try:
        values = [float(v) for v in token.split("|")]
    except ValueError:
        return scale
    # The database stores f32s: -0.02 reads as -0.019999999552965164.
    return {"v": round(max(values, key=abs), 6), **scale}


def _generic(kind, args, local, item_name):
    """[(plus key, minus key, leading text arguments)] for a modifier without an override."""
    relative = len(args) > 1 and args[1].lower() == "relative"
    scopes = args[2:]

    def pick(keys):
        return (keys[0], keys[1]) if relative else (keys[2], keys[3])
    if kind in GENERIC:
        return [(*pick(GENERIC[kind]), [])]
    if kind == "Damage":
        low, high = "Min" in scopes, "Max" in scopes
        part = "" if low == high else ("Min" if low else "Max")
        mode = "Relative" if relative else "Absolut"
        return [(f"damage{mode}{part}Plus", f"damage{mode}{part}Minus", [])]
    if kind == "Speed":
        out = []
        if "MovementBonus" in scopes:
            out.append(("speedMovementBonusAbsolutPlus", "speedMovementBonusAbsolutMinus", []))
        elif "Movement" in scopes:
            out.append(pick(("speedMovementRelativePlus", "speedMovementRelativeMinus",
                             "speedMovementAbsolutPlus", "speedMovementAbsolutMinus")) + ([],))
        if any(s in ("Skills", "AllPlayerSkills") for s in scopes):
            out.append(pick(("speedSkillRelativePlus", "speedSkillRelativeMinus",
                             "speedSkillAbsolutPlus", "speedSkillAbsolutMinus")) + ([],))
        return out
    if kind == "Resistance":
        keys = pick(("resistanceRelativPlus", "resistanceRelativMinus", "resistanceAbsolutPlus", "resistanceAbsolutMinus"))
        elements = [s for s in scopes if s in ("Physical",) + ELEMENTAL]
        names = []
        if set(ELEMENTAL) <= set(elements):
            names.append(local("gui.statuseffects.modifier_resistanceAll", "all resistance values"))
            elements = [e for e in elements if e not in ELEMENTAL]
        names += [local("gui.statuseffects.modifier_resistance" + e, e) for e in elements]
        return [(*keys, [n]) for n in names]
    if kind == "DropAmount" and scopes:
        if scopes[0] == "money_vc":
            return [pick(("vcDropAmountRelativePlus", "vcDropAmountRelativeMinus",
                          "vcDropAmountAbsolutePlus", "vcDropAmountAbsoluteMinus")) + ([],)]
        return [pick(("dropAmountRelativePlus", "dropAmountRelativeMinus",
                      "dropAmountAbsolutePlus", "dropAmountAbsoluteMinus")) + ([item_name(scopes[0])],)]
    return []


def lines(raw, local, variables, item_name, tick=False):
    """The hover lines of a modifier program (StartModifiers, or TickModifiers per second)."""
    out = []
    for part in (raw or "").split(";"):
        body, _, key = part.partition("#")
        key = key.strip()
        if key == "~" or not body.strip():
            continue  # Authored hidden companion of a visible modifier.
        kind, sep, params = body.partition(":")
        kind = kind.strip()
        args = [a.strip() for a in params.split(",")] if sep else []
        if tick:
            # A heal over time, per second (the effects' tick rate is checked by the caller).
            if kind != "CurrHealthPoints" or key:
                continue
            relative = len(args) > 1 and args[1].lower() == "relative"
            mode = "Relative" if relative else "Absolute"
            plus, minus = local(f"gui.statuseffects.modifier_HPRegen{mode}Plus"), local(f"gui.statuseffects.modifier_HPRegen{mode}Minus")
            if plus:
                out.append(dict(text=plus, minus=minus, args=[_number(args[0] if args else "", relative, variables, False)]))
            continue
        relative = (len(args) > 1 and args[1].lower() == "relative") or "forceShowAsPercentage" in args
        scaled = "LevelModifier" in args
        value = _number(args[0], relative, variables, scaled) if args and args[0] else None
        # The class's resource (Rage, Mana, Concentration, Steam): one text per
        # class, the warrior's as the default (the Mortis Doll's pet effect).
        if kind == "MaxSkillResource" and not key and value is not None:
            mode = "Relative" if relative else "Absolut"
            classes = {c.lower(): local(f"gui.statuseffects.modifier_maxSkillResource{mode}Plus{c}")
                       for c in ("Warrior", "Mage", "Ranger", "Dwarf")}
            minus = local(f"gui.statuseffects.modifier_maxSkillResource{mode}MinusWarrior")
            if classes["warrior"]:
                out.append(dict(text=classes["warrior"], minus=minus, args=[value], classes=classes))
            continue
        # A text of its own ("Localize:pet_PRK_special_drop": "Opening a Waste
        # Box has a 10% chance..."), the pets' special effects.
        # FAILURE (2026-10-31): 95 pet effects dropped, a "Pet Effect:" with
        # nothing under it (the Asar's "Reduces Boss armor values by 3%").
        if kind == "Localize" and not key and args and args[0]:
            text = local("gui.statuseffects.modifier_" + args[0])
            if text:
                out.append(dict(text=text, args=[]))
            continue
        # "Enables auto-pickup for Gems, Jewels, Runes and Mystic Cubes": the
        # modifier's own text with what it picks up (gui.window.tooltip), the
        # Summer Dog's and the Gem Gnome's pet effects.
        if kind == "AutoPickupModifier" and not key and len(args) > 1:
            text, what = local("gui.statuseffects.modifier_autoPickUpModifier"), local("gui.window.tooltip_" + args[1])
            if text and what:
                out.append(dict(text=text, args=[what]))
            continue
        generic = _generic(kind, args, local, item_name)
        if key:
            text = local("gui.statuseffects.modifier_" + key)
            if not text:
                continue
            leading = generic[0][2] if generic else []
            needed = max((int(n) for n in re.findall(r"\{(\d+):", text)), default=-1) + 1
            numbers = [value] if value is not None and needed > len(leading) else []
            # The number in its own place: "+ {1:s}% Coins drop stack size"
            # names the coins itself (the parrots' ParrotStatueEffect).
            if numbers and len(leading) < needed - 1:
                leading = leading + [item_name(args[2]) if kind == "DropAmount" and len(args) > 2 else ""] * (needed - 1 - len(leading))
            out.append(dict(text=text, args=leading + numbers))
            continue
        for plus_key, minus_key, leading in generic:
            plus = local("gui.statuseffects.modifier_" + plus_key)
            minus = local("gui.statuseffects.modifier_" + minus_key)
            if plus and value is not None:
                out.append(dict(text=plus, minus=minus or plus, args=leading + [value]))
    return out


def export_status(db, textures, loca, assets):
    table = loca.get("en", {})
    short = {k.split("_", 1)[-1]: v for k, v in table.items() if k.startswith("db.")}

    def local(key, fallback=""):
        return table.get(key) or fallback

    def item_name(item):
        name = short.get(item + "Title")
        return re.sub(r"<[^>]*>", "", name) if name else item.replace("_", " ")

    # Immutable actor variables only: the others follow talents, equipment or
    # progression (pet bonuses grow with the collection) and are not known here.
    variables = {r[0]: r[1] for r in db.execute(
        "SELECT Id, InitialValue FROM _Template_ActorVariables WHERE LowerLimit = UpperLimit")}

    def texture(brush):
        return f"textures/{brush}.png" if brush and textures.get(brush) else ""

    configs = {}
    for cid, background, frame in db.execute("SELECT Id, BackgroundTexture, FrameTexture FROM _Template_IconConfig"):
        configs[cid] = [texture(background), texture(frame)]
    effects, skipped = {}, 0
    cursor = db.execute("SELECT rowid - 1, * FROM _Template_StatusEffect ORDER BY rowid")
    columns = [c[0] for c in cursor.description]
    for values in cursor:
        r = dict(zip(columns, values))
        groups = {g.strip() for g in (r["Groups"] or "").split(";")}
        if not groups & {"BuffUI", "DebuffUI"}:
            continue
        icon = texture(r["IconBrush"])
        if not icon:
            skipped += 1  # Nothing to draw: no IconBrush, or one the client lacks.
            continue
        sid = r["Id"]
        title = r["LocaleId"] or sid + "Title"
        name = local("db.statuseffect.table_" + title) or local("db.statuseffect.table_" + sid + "Title")
        description = local("db.statuseffect.table_" + sid + "Desc")
        effect_lines = [dict(text=description)] if description else []
        effect_lines += lines(r["StartModifiers"], local, variables, item_name)
        if (r["StatusEffectTickRate"] or 0.0) == 1.0:
            effect_lines += lines(r["TickModifiers"], local, variables, item_name, tick=True)
        effects[str(r["rowid - 1"])] = dict(
            id=sid, debuff="BuffUI" not in groups, icon=icon, config=r["IconConfiguration"] or "",
            name=name or sid.replace("_", " ").strip().capitalize(),
            stacks=max(1, int(r["MaxStackSize"] or 1)), lines=effect_lines)
    used = {e["config"] for e in effects.values()}
    configs = {k: v for k, v in configs.items() if k in used}
    units = [local("gui.window_timeSecondsShort", "{0:s} s"), local("gui.window_timeMinutesShort", "{0:i} m"),
             local("gui.window_timeHourShort", "{0:i} hr"), local("gui.window_timeDaysShort", "{0:i} d")]
    out = os.path.join(assets, "interface", "status.catalog.json")
    with open(out, "w", encoding="utf-8") as f:
        json.dump(dict(units=units, configs=configs, effects=effects), f, ensure_ascii=False, separators=(",", ":"))
    return dict(effects=len(effects), debuffs=sum(e["debuff"] for e in effects.values()), without_icon=skipped)


if __name__ == "__main__":
    import sqlite3
    root, assets, godot = sys.argv[1], sys.argv[2], sys.argv[3]
    database = sys.argv[sys.argv.index("--database") + 1] if "--database" in sys.argv else os.path.join(root, "db", "static.db4")
    crunch = os.path.abspath(os.path.join(godot, "build", "crn2dds.exe"))
    if os.name == "nt" and os.path.isfile(crunch):
        os.environ.setdefault("DSO_CRN2DDS", crunch)
    sys.path.insert(0, godot)
    import export_ui as gx  # noqa: E402  DSO_Godot's exporter
    from dsoexp.resources import Resolver  # noqa: E402
    tex = gx.Textures(Resolver(root), os.path.join(assets, "textures"), "textures")
    print("status effects:", export_status(sqlite3.connect(f"file:{database}?mode=ro", uri=True), tex,
                                           gx.load_loca(root, ["en"]), assets))
    if tex.missing:
        print("missing textures:", sorted(tex.missing))
