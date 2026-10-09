#!/usr/bin/env python3
"""Skills and the sequences that draw them, from the 2018 client's data.

Usage: export_skills.py <export_win32> <assets>

Writes:
  skills/skills.json     wire index -> the skill (static.db4 _Template_Skill, row order;
                         the wire index is the row minus one), its bullet
                         (_Template_SkillBullet) and its sequence names per weapon kind
                         (data/tables/attacksequence.xml maps an Execute/Pre/Post
                         "SequenceMapping" to one sequence per armament column)
  skills/sequences.json  sequence name -> its tracks (sequences/*.pbxml: KCAP packs of
                         BXML TrackEditor documents, 25 frames a second)

Tracks kept, by their TrackBar type:
  AnimationHijackTrackBar     the caster's animation (anims.xml state name)
  DrasaGraphicsObjectTrackBar an effect model placed at the actor
  AttachedGraphicsTrackBar    an effect model on one of the actor's joints
  SkillPhaseTrackBar          the skill phase (its end frame bounds the sequence)
  PointLightTrackBar          a point light
  ObserverCameraShakeTrackBar a camera shake
  HideEntityTrackBar          the actor hidden (teleports)
  PlaySoundTrackBar           a sound (kept for later; the client has no audio yet)
"""
import json, os, re, struct, sys
import xml.etree.ElementTree as ET
import sqlite3

export, assets = sys.argv[1], sys.argv[2]
INVALID = 0xFFFFFFFF


class Layout:
    """One BXML document ("LMXB"): nodes with attributes over a string pool."""

    def __init__(self, raw):
        if raw[:4] != b"LMXB":
            raise ValueError("not bxml")
        na, nn, ns = struct.unpack_from("<III", raw, 4)
        attrs_at = 16
        nodes_at = attrs_at + 8 * na
        pool_at = nodes_at + 24 * nn
        flat = [struct.unpack_from("<2I", raw, attrs_at + 8 * i) for i in range(na)]
        pool = raw[pool_at:]
        self.strings = [s.decode("latin1") for s in pool.split(b"\x00")]
        self.nodes = []
        for i in range(nn):
            name, child, sibling, parent, at, count = struct.unpack_from("<6I", raw, nodes_at + 24 * i)
            self.nodes.append((name, child, sibling, flat[at:at + count]))

    def s(self, i):
        return None if i == INVALID or i >= len(self.strings) else self.strings[i]

    def tag(self, i):
        return self.s(self.nodes[i][0])

    def attrs(self, i):
        return {self.s(k): self.s(v) for k, v in self.nodes[i][3]}

    def kids(self, i):
        c = self.nodes[i][1]
        while c != INVALID and c != 0x7FFFFFFF:
            yield c
            c = self.nodes[c][2]


def kcap(raw):
    """(name, bytes) of every document in a KCAP pack."""
    if raw[:4] != b"KCAP":
        return
    at = 8
    while at + 2 <= len(raw):
        n = struct.unpack_from("<H", raw, at)[0]
        at += 2
        name = raw[at:at + n].decode("latin1")
        at += n
        size = struct.unpack_from("<I", raw, at)[0]
        at += 4
        yield name, raw[at:at + size]
        at += size


def values(layout, i):
    """A TrackBar's <Attribute name=... value=...> children, nested ones (transforms,
    colours) as dicts."""
    out = {}
    for c in layout.kids(i):
        a = layout.attrs(c)
        name = a.get("name")
        if not name:
            continue
        sub = list(layout.kids(c))
        out[name] = {layout.attrs(x).get("name"): layout.attrs(x).get("value") for x in sub} if sub else a.get("value")
    return out


def splines(layout, attr):
    """An animated Attribute's curve: its Graph's BezierSpline segments, each as
    [x0, y0, cx0, cy0, cx1, cy1, x1, y1] with x in sequence frames."""
    segs = []
    for g in layout.kids(attr):
        if layout.tag(g) != "Graph":
            continue
        for b in layout.kids(g):
            a = layout.attrs(b)
            pts = []
            for k in ("startPoint", "startCP", "endCP", "endPoint"):
                pts += [float(x) for x in (a.get(k) or "0, 0").split(",")]
            segs.append(pts)
    return segs


def curves(layout, i):
    """Every animated value of a TrackBar (isAnimated="True"), by name: "intensity",
    "lighttrans.ty", "graphicstrans.sx"..."""
    out = {}
    for c in layout.kids(i):
        a = layout.attrs(c)
        name = a.get("name")
        if not name:
            continue
        if a.get("isAnimated") == "True":
            segs = splines(layout, c)
            if segs:
                out[name] = segs
        for sub in layout.kids(c):
            sa = layout.attrs(sub)
            if sa.get("isAnimated") == "True" and sa.get("name"):
                segs = splines(layout, sub)
                if segs:
                    out[f"{name}.{sa['name']}"] = segs
    return out


def num(v, default=0.0):
    try:
        return float(v)
    except (TypeError, ValueError):
        return default


def transform(t):
    t = t or {}
    return [num(t.get(k), d) for k, d in (("tx", 0), ("ty", 0), ("tz", 0), ("rx", 0), ("ry", 0), ("rz", 0), ("sx", 1), ("sy", 1), ("sz", 1))]


def sequence(layout):
    root = 0
    tracks, length, repeat = [], 0, False
    for i in range(len(layout.nodes)):
        tag = layout.tag(i)
        if tag == "GlobalParameter":
            length = int(num(layout.attrs(i).get("playLength")))
            repeat = layout.attrs(i).get("repeat") == "True"
        if tag != "TrackBar":
            continue
        a = layout.attrs(i)
        if a.get("mute") == "True":
            continue
        v = values(layout, i)
        start, end = int(num(a.get("startFrame"))), int(num(a.get("endFrame")))
        kind = a.get("type")
        t = None
        if kind == "AnimationHijackTrackBar":
            t = {"kind": "anim", "name": v.get("animName", ""), "fadein": num(v.get("fadein")), "fadeout": num(v.get("fadeout")),
                 "speed": num(v.get("animSpeed"), 100) / 100.0, "offset": num(v.get("animOffset"))}
        elif kind == "DrasaGraphicsObjectTrackBar":
            t = {"kind": "fx", "graphics": v.get("graphicsname", ""), "at": transform(v.get("graphicstrans"))}
        elif kind == "AttachedGraphicsTrackBar":
            pos, rot = v.get("posOffset") or {}, v.get("rotOffset") or {}
            t = {"kind": "fx", "graphics": v.get("graphicsName", ""), "joint": v.get("jointName", ""),
                 "at": [num(pos.get("x")), num(pos.get("y")), num(pos.get("z")), num(rot.get("x")), num(rot.get("y")), num(rot.get("z")), 1, 1, 1]}
        elif kind == "SkillPhaseTrackBar":
            t = {"kind": "phase", "event": v.get("eventName", ""), "event_frame": int(num(v.get("eventFrame")))}
        elif kind == "PointLightTrackBar":
            c = v.get("color") or {}
            t = {"kind": "light", "color": [num(c.get(k)) / 255.0 for k in "xyz"], "intensity": num(v.get("intensity"), 1),
                 "range": num(v.get("range"), 10), "at": transform(v.get("lighttrans")), "flicker": v.get("flicker") == "True"}
        elif kind == "ObserverCameraShakeTrackBar":
            t = {"kind": "shake", "intensity": num(v.get("intensity"), 1), "range": num(v.get("range"), 1), "delay": num(v.get("startdelay"))}
        elif kind == "HideEntityTrackBar":
            t = {"kind": "hide"}
        elif kind == "PlaySoundTrackBar":
            t = {"kind": "sound", "name": v.get("soundname", ""), "volume": num(v.get("volume"), 100)}
        elif kind == "ColorShaderParameterTrackBar":
            # A shader colour of the actor while the track runs (HitColor: the red
            # flash of a hit), its alpha x the "intensity" curve; defaultColor after.
            c, d = v.get("color") or {}, v.get("defaultColor") or {}
            t = {"kind": "color", "param": v.get("shaderParameter", ""),
                 "color": [num(c.get(k)) / 255.0 for k in "xyzw"], "default": [num(d.get(k)) / 255.0 for k in "xyzw"],
                 "intensity": num((v.get("intensity") or {}).get("intensity"), 1) if isinstance(v.get("intensity"), dict) else num(v.get("intensity"), 1)}
        if t and (t.get("graphics", "x") not in ("", "empty")):
            t["start"], t["end"] = start, end
            # Values that change over the track (a light dimming, an effect moving).
            c = curves(layout, i)
            if c:
                t["curves"] = c
            tracks.append(t)
    return {"length": length, "repeat": repeat, "tracks": tracks}


# -- sequences -------------------------------------------------------------------
sequences = {}
seq_dir = os.path.join(export, "sequences")
for name in sorted(os.listdir(seq_dir)):
    if not name.endswith(".pbxml"):
        continue
    for doc, data in kcap(open(os.path.join(seq_dir, name), "rb").read()):
        try:
            sequences[doc.split("/", 1)[-1]] = sequence(Layout(data))
        except (ValueError, struct.error, IndexError):
            pass

# -- attack sequence mapping -------------------------------------------------------
ns = {"ss": "urn:schemas-microsoft-com:office:spreadsheet"}
SS = "{urn:schemas-microsoft-com:office:spreadsheet}"
mapping = {}
sheet = ET.parse(os.path.join(export, "data", "tables", "attacksequence.xml")).getroot().find("ss:Worksheet", ns)
rows = sheet.findall(".//ss:Row", ns)


def cells(r):
    out = []
    for c in r.findall("ss:Cell", ns):
        idx = c.get(SS + "Index")
        if idx:
            out += [""] * (int(idx) - 1 - len(out))
        d = c.find("ss:Data", ns)
        out.append(d.text.strip() if d is not None and d.text else "")
    return out


header = cells(rows[0])
for r in rows[1:]:
    c = cells(r)
    if c and c[0]:
        # A cell can end with ";" or list several sequences ("a;b"): the first.
        mapping[c[0]] = {header[i]: c[i].split(";")[0].strip() for i in range(1, min(len(header), len(c))) if c[i].split(";")[0].strip()}


def mapped(name):
    """A SequenceMapping -> {armament column: sequence}; a bare sequence name maps to
    itself for every column."""
    if not name:
        return {}
    if name in mapping:
        return mapping[name]
    return {"*": name.split(";")[0].strip()}


# -- skills ------------------------------------------------------------------------
db = sqlite3.connect(os.path.join(export, "db", "static.db4"))
db.row_factory = sqlite3.Row
bullets = {r["Id"]: r for r in db.execute("select * from _Template_SkillBullet")}


def statuses(text):
    """'name,C:1.0,D:8.0,...;name2,...' -> [[name, duration]] (D:, 0 when absent)."""
    out = []
    for part in (text or "").split(";"):
        fields = [f.strip() for f in part.split(",") if f.strip()]
        if not fields:
            continue
        d = next((float(f[2:]) for f in fields[1:] if f.startswith("D:")), 0.0)
        out.append([fields[0], d])
    return out
skills = {}
for r in db.execute("select rowid, * from _Template_Skill order by rowid"):
    b = bullets.get(r["SkillBulletId"] or "")
    skills[r["rowid"] - 1] = {
        "id": r["Id"],
        "type": r["SkillType"] or "",
        "targeting": r["TargetingType"] or "",
        "hit_frame": r["HitFrame"] or 0,
        "loop_start": r["LoopStartFrame"] or 0,
        "unblock": r["SkillUnblockFrame"] or 0,
        "motion_unblock": r["MotionUnblockFrame"] or 0,
        "range": r["AttackRange"] or 0.0,
        "cooldown": r["CoolDown"] or 0.0,
        "cooldown_category": r["CoolDownCategory"] or "",
        "resource_cost": r["ResourceCost"] or 0.0,
        "pre": mapped(r["PreExecuteSequenceMapping"]),
        "execute": mapped(r["ExecuteSequenceMapping"]),
        "post": mapped(r["PostExecuteSequenceMapping"]),
        "impact": r["SkillImpactSequence"] or "",
        # Shifted skills (lightning strike, meteor, singularity): SkillBulletId names
        # no _Template_SkillBullet row but a sequence, drawn on the aimed point (all
        # 107 of them in 2018).
        "shifted": (r["SkillBulletId"] or "") if b is None and r["SkillType"] == "Shifted" else "",
        # What the skill leaves on its victims and on the ground: (status id, D:).
        # The server places them (StatusEffect 82 / NewLocationEffect 64); kept for
        # offline play (client/src/monsters.rs).
        "victim_status": statuses(r["VictimStatusEffects"]),
        "location_status": statuses(r["LocationStatusEffects"]),
        "bullet": None if b is None else {
            "id": b["Id"],
            "loop": b["LoopSequence"] or "",
            "impact": b["BulletImpactSequence"] or "",
            "death": b["DeathSequence"] or "",
            "motion": b["MotionType"] or "",
            "velocity": b["Velocity"] or 0.0,
            "lifetime": b["LifeTime"] or 0.0,
            "radius": b["BulletRadius"] or 0.0,
            # Where the bullet leaves, in the caster's entity frame (Nebula: -Z ahead):
            # mage_fireball_bullet (0.043, 1.49, -1.788) -- the hand, 1.8 ahead.
            "start": list(struct.unpack("<3f", b["StartOffset"][:12])) if b["StartOffset"] and len(b["StartOffset"]) >= 12 else [0.0, 1.2, 0.0],
        },
    }

# -- status effects (what a skill puts on its caster, its victims or the ground) --
# _Template_StatusEffect by wire index (row - 1): its Start / Tick / Done / Stop
# sequences (or mappings) and the animation it plays on its holder. The server
# applies them with StatusEffectCommand (82) and NewLocationEffectCommand (64).
status = {}
for r in db.execute("select rowid, * from _Template_StatusEffect order by rowid"):
    status[r["rowid"] - 1] = {
        "id": r["Id"],
        "duration": r["StatusEffectDuration"] or 0.0,
        "start": mapped(r["StartSequenceMapping"]) or mapped(r["StartSequence"]),
        "tick": mapped(r["TickSequenceMapping"]) or mapped(r["TickSequence"]),
        "done": mapped(r["DoneSequenceMapping"]) or mapped(r["DoneSequence"]),
        "stop": mapped(r["StopSequenceMapping"]) or mapped(r["StopSequence"]),
        "animation": r["StartAnimation"] or "",
        "looped_animation": bool(r["LoopedStartAnimation"]),
        "stop_animation": r["StopAnimation"] or "",
    }

os.makedirs(os.path.join(assets, "skills"), exist_ok=True)
with open(os.path.join(assets, "skills", "status_effects.json"), "w") as f:
    json.dump(status, f, separators=(",", ":"))
with open(os.path.join(assets, "skills", "sequences.json"), "w") as f:
    json.dump(sequences, f, separators=(",", ":"))
with open(os.path.join(assets, "skills", "skills.json"), "w") as f:
    json.dump(skills, f, separators=(",", ":"))
used = {s for k in skills.values() for m in (k["execute"], k["pre"], k["post"]) for s in m.values()}
print({"sequences": len(sequences), "skills": len(skills), "mappings": len(mapping),
       "execute sequences missing": len([s for s in used if s not in sequences])})
