#!/usr/bin/env python3
"""What the client cannot yet draw as the 2018 client does, skill by skill.

Usage: audit_fx.py <export_win32> <assets> [class prefix ...] > report.txt

For every player skill (warrior_, mage_, ranger_, dwarf_ *_default by default) it
follows what the skill draws -- execute sequences, impact, bullet loop and death,
Shifted sequence, the status effects it starts (start / tick / done / stop) -- and
reports, per skill:
  - sequences or effect models that do not exist in the converted assets;
  - effect joints the player skeleton does not have (they fall back to the feet);
  - track types of the 2018 sequences the export drops (export_skills.py keeps
    anim, fx, phase, light, shake, hide, sound, color);
  - effect surfaces whose Nebula shader or render state the client has no
    treatment for (client/src/materials.rs, surfaces.rs, particles.rs).
It reads data only; nothing is rendered. Run it after a change to see what is left.
"""
import collections, json, os, struct, sys

export, assets = sys.argv[1], sys.argv[2]
prefixes = sys.argv[3:] or ["warrior_", "mage_", "ranger_", "dwarf_"]
sys.argv = ["x", export, "/nonexistent"]
src = open(os.path.join(os.path.dirname(__file__), "export_skills.py")).read()
ns = {}
exec(compile(src[: src.find("\ndef sequence(")], "export_skills_head", "exec"), ns)

KEPT = {"AnimationHijackTrackBar", "DrasaGraphicsObjectTrackBar", "AttachedGraphicsTrackBar", "SkillPhaseTrackBar",
        "PointLightTrackBar", "ObserverCameraShakeTrackBar", "HideEntityTrackBar", "PlaySoundTrackBar",
        "ColorShaderParameterTrackBar"}
HANDLED_SHADERS = {"standard", "particle", "environment", "decal", "refraction", "uvanimated", "uvanimated2", "water",
                   "simplelayer", "unlit", "volumefog", "glow", "monster", "character", "characterfalloff", "monsterexp",
                   "unlitalphavertexcolors"}
HANDLED_STATES = {"Solid", "AlphaTest", "DecalReceiveSolid", "DecalReceiveAlphaTest", "Additive", "Alpha", "AlphaUnlit",
                  "PostAlphaUnlit", "PreAlphaUnlit", "AlphaLit", "Refraction", "DecalEffect", "Decal", None, ""}

# The raw track types of every sequence, from the 2018 files.
raw_types = collections.defaultdict(collections.Counter)
seq_dir = os.path.join(export, "sequences")
for name in os.listdir(seq_dir):
    if not name.endswith(".pbxml"):
        continue
    for doc, data in ns["kcap"](open(os.path.join(seq_dir, name), "rb").read()):
        try:
            layout = ns["Layout"](data)
        except Exception:
            continue
        key = doc.split("/", 1)[-1]
        for i in range(len(layout.nodes)):
            if layout.tag(i) == "TrackBar" and layout.attrs(i).get("mute") != "True":
                raw_types[key][layout.attrs(i).get("type")] += 1

skills = {v["id"]: v for v in json.load(open(os.path.join(assets, "skills", "skills.json"))).values()}
sequences = json.load(open(os.path.join(assets, "skills", "sequences.json")))
statuses = {v["id"]: v for v in json.load(open(os.path.join(assets, "skills", "status_effects.json"))).values()}
b = open(os.path.join(assets, "characters", "uniskel", "__animations.glb"), "rb").read()
n = struct.unpack("<I", b[12:16])[0]
bones = {node.get("name") for node in json.loads(b[20:20 + n])["nodes"]}

fx_cache = {}


def fx_problems(graphics):
    if graphics in fx_cache:
        return fx_cache[graphics]
    out = []
    glb = os.path.join(assets, graphics + ".glb")
    if not os.path.exists(glb):
        out.append(f"model missing: {graphics}")
    else:
        side = os.path.join(assets, graphics + ".fx.json")
        if os.path.exists(side):
            for e in json.load(open(side)).get("emitters", []):
                shader = (e.get("shader") or "").removeprefix("shd:")
                state = (e.get("emitter") or {}).get("type_name")
                if shader not in HANDLED_SHADERS:
                    out.append(f"{graphics}: shader {shader} ({e.get('node')})")
                if state not in HANDLED_STATES:
                    out.append(f"{graphics}: render state {state} ({e.get('node')}, {shader})")
    fx_cache[graphics] = out
    return out


def sequence_names(skill):
    names = set(skill.get("execute", {}).values()) | {skill.get("impact", ""), skill.get("shifted", "")}
    bullet = skill.get("bullet") or {}
    names |= {bullet.get("loop", ""), bullet.get("death", "")}
    for status, _ in skill.get("user_status", []) + skill.get("victim_status", []) + skill.get("location_status", []):
        d = statuses.get(status)
        if d:
            for k in ("start", "tick", "done", "stop"):
                names |= set((d.get(k) or {}).values())
    return {x for x in names if x and x != "empty_sequence"}


totals = collections.Counter()
for sid in sorted(skills):
    if not any(sid.startswith(p) for p in prefixes) or not sid.endswith("_default"):
        continue
    problems = []
    for name in sorted(sequence_names(skills[sid])):
        seq = sequences.get(name)
        if seq is None:
            problems.append(f"sequence missing: {name}")
            continue
        dropped = {t: c for t, c in raw_types.get(name, {}).items() if t not in KEPT}
        for t, c in dropped.items():
            problems.append(f"{name}: track type not drawn: {t} x{c}")
            totals[t] += c
        for t in seq["tracks"]:
            if t["kind"] == "fx":
                if t.get("joint") and t["joint"] not in bones:
                    problems.append(f"{name}: joint {t['joint']} not on the player skeleton")
                problems += [f"{name}: {p}" for p in fx_problems(t["graphics"])]
    for status, _ in skills[sid].get("user_status", []) + skills[sid].get("victim_status", []) + skills[sid].get("location_status", []):
        if status not in statuses and not status.startswith("item_"):
            problems.append(f"status effect unknown: {status}")
    if problems:
        print(sid)
        for p in sorted(set(problems)):
            print("   ", p)
            totals[p.split(":")[-1].split("(")[0].strip() if "shader" in p or "state" in p else p.split(":")[0]] += 0
print("\ndropped track types overall:", dict(totals))
