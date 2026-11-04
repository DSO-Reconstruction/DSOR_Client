#!/usr/bin/env python3
"""The Night Walker's spell visuals: each spell its own sequence and clip.

Usage: niwalk_visuals.py <assets>

The server authors the Night Walker's spells over a live skill of another
class (DSOR_Server services/niwalk_content.py, installed into static.db4), so
export_skills.py gives each one its donor's execute sequence. A sequence names
an animation STATE (Skill04_execute...), and every class numbers its states
its own way: in the Night Walker's sets (niwalk_off...) Skill04_execute is the
warrior's two-handed whirl, so the mage's teleport would play it.

This tool copies each spell's donor sequence to niwalk_<skill>, renames its
animation states to Niwalk_<skill>[_<n>], points the spell's execute mapping
at the copy, and adds those states to the Night Walker's animation sets with
the clip chosen for the spell (CLIPS, uniskel's own clips: a dagger stab, a
throw...). Effects, lights and sounds stay the donor's.

Run after export_skills.py and export_deco.py, before vfx_apply.py:
  python3 tools/export_skills.py <export_win32> <assets> --database <static.db4>
  python3 tools/export_deco.py ...
  python3 tools/niwalk_visuals.py <assets>
  python3 tools/vfx_apply.py <assets>
"""
import json
import sys
from pathlib import Path

# The Night Walker's sets (characters/anims.json), both genders, all stances.
SETS = ("niwalk_off", "niwalk_female_off", "niwalk_def", "niwalk_female_def", "niwalk_1h", "niwalk_female_1h")
# The column its daggers fill (client gameplay/skills.rs weapon_column).
COLUMN = "2h_weapon"
# Per spell, the clip of each animation track of its donor's sequence, in
# order (fewer clips than tracks: the remaining tracks keep the clip the
# Night Walker's set already gives their state).
CLIPS = {
    "zero_slicedice": ["warrior_1h_skill_01"],          # a quick cut
    "zero_bladecut_off": ["ranger_1h_skill_12_fast"],   # a thrust
    "move_shoulderdash": ["warrior_1h_skill_03_start", "warrior_2h_skill_03_loop", "warrior_1h_skill_03_end"],
    "move_flashbang": ["mage_1hwand_skill_04_start", "mage_1hwand_skill_04_end"],
    "sacrifice_cut": ["warrior_1h_skill_09"],           # a hard blow
    "trickery_gasbomb": ["mage_1hwand_skill_07"],       # a flick of the wrist
    "sacrifice_vial": ["jester_april_fool_throw_ball_01"],  # a lobbed throw
    "crowd_glassdagger": ["warrior_1h_skill_06"],       # a sweeping fan
    "ultimate_windmill_off": ["warrior_2h_skill_04"],   # the whirl
}


def donor_sequence(execute):
    for key in (COLUMN, "*", "empty"):
        if execute.get(key):
            return execute[key]
    return next(iter(execute.values()), None)


def apply(assets):
    assets = Path(assets)
    skills_path = assets / "skills" / "skills.json"
    sequences_path = assets / "skills" / "sequences.json"
    anims_path = assets / "characters" / "anims.json"
    skills = json.loads(skills_path.read_text(encoding="utf-8"))
    sequences = json.loads(sequences_path.read_text(encoding="utf-8"))
    anims = json.loads(anims_path.read_text(encoding="utf-8"))
    done = []
    for entry in skills.values():
        skill = entry.get("id") if isinstance(entry, dict) else None
        if skill not in CLIPS:
            continue
        name = f"niwalk_{skill}"
        source = donor_sequence(entry.get("execute") or {})
        if source == name and name in sequences:
            source_sequence = sequences[f"{name}__donor"] if f"{name}__donor" in sequences else None
        else:
            source_sequence = sequences.get(source)
        if source_sequence is None:
            print(f"{skill}: no donor sequence ({source}); is the niwalk content installed and the skills exported?")
            continue
        sequences[f"{name}__donor"] = source_sequence
        copy = json.loads(json.dumps(source_sequence))
        states = []
        for track in copy.get("tracks", []):
            if track.get("kind") != "anim":
                continue
            state = f"Niwalk_{skill}" + (f"_{len(states)}" if states else "")
            states.append((state, track["name"]))
            track["name"] = state
        sequences[name] = copy
        entry["execute"] = {"*": name, COLUMN: name, "empty": name}
        for set_name in SETS:
            rows = anims.get(set_name)
            if rows is None:
                continue
            for index, (state, original) in enumerate(states):
                clips = CLIPS[skill]
                rows[state] = clips[index] if index < len(clips) else rows.get(original, original)
        done.append(skill)
    skills_path.write_text(json.dumps(skills, separators=(",", ":"), ensure_ascii=False), encoding="utf-8")
    sequences_path.write_text(json.dumps(sequences, separators=(",", ":"), ensure_ascii=False), encoding="utf-8")
    anims_path.write_text(json.dumps(anims, separators=(",", ":"), ensure_ascii=False), encoding="utf-8")
    print(f"niwalk visuals: {len(done)} spells ({', '.join(done)})")


if __name__ == "__main__":
    apply(sys.argv[1])
