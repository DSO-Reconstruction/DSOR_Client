"""Produce exact-byte protocol test vectors from the production Python server.

The production server (``dso-server-production-*``) is treated as the protocol
spec.  This script imports its pure ``protocol`` modules (no server is started,
no client assets are read), calls the encoders the server actually uses on the
wire with distinctive non-zero field values, and writes a JSON list of
vectors that the Rust ``proto`` crate tests decode against.

Each vector is::

    {"name": "<CommandName>", "id": <u16 command id>, "hex": "<full message>",
     "bits": <exact bit length>, "envelope": "0x84"|"0x85"|"0x8B",
     "fields": {...inputs, named as production names them...},
     "batch": [...command names in order]   # only for 0x85 multi-command
    }

``fields["built_by"]`` is ``"hand"`` for client-direction messages that the
production server only decodes (bytes built with its BitWriter in its
decoder's order, then checked by that decoder), and ``"helper:<fn>"`` when a
lower-level production writer was used.  ``fields["encoder"]`` names the
production function that produced the bytes.

Command ids resolve against the server's default table and the codecs take their
default layouts: ``DSO_COMMAND_IDS``, ``DSO_SKILL_HIT_MARKER_BITS`` and
``DSO_SELECTION_WIRE`` are cleared before import so the output is deterministic.
Since 2026-10-21 that default is the release261_192 client's
(``docs/command_ids_rel261_192.json``, the skill and Hit marker bits on).

Usage::

    python3 tools/prod_vectors.py <production_dir> <out.json>
"""

from __future__ import annotations

import dataclasses
import json
import os
import sys
from importlib.util import module_from_spec, spec_from_file_location
from pathlib import Path


def load_package(root: Path):
    """Load ``dso_server`` from ``root`` exactly like the production run.py."""

    for variable in ("DSO_COMMAND_IDS", "DSO_SKILL_HIT_MARKER_BITS",
                     "DSO_SELECTION_WIRE"):
        os.environ.pop(variable, None)
    spec = spec_from_file_location(
        "dso_server", root / "__init__.py", submodule_search_locations=[str(root)]
    )
    package = module_from_spec(spec)
    sys.modules[spec.name] = package
    spec.loader.exec_module(package)
    return package


def jsonable(value):
    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        return {
            f.name: jsonable(getattr(value, f.name))
            for f in dataclasses.fields(value)
        }
    if isinstance(value, (bytes, bytearray)):
        return {"bytes_hex": bytes(value).hex()}
    if isinstance(value, dict):
        return {str(k): jsonable(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [jsonable(v) for v in value]
    return value


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(__doc__)
        return 2
    root = Path(argv[1]).resolve()
    out = Path(argv[2])
    load_package(root)

    from dso_server.protocol import character_selection as cs
    from dso_server.protocol import combat_protocol as cp
    from dso_server.protocol import gameplay_protocol as gp
    from dso_server.protocol import inventory_codec as ic
    from dso_server.protocol import map_packets as mp
    from dso_server.protocol import new_player_codec as npc
    from dso_server.protocol import player_init_protocol as pi
    from dso_server.protocol import social_protocol as sp

    assert cs.SELECTION_PROGRESSION_BITS == 64
    assert cs.SELECTION_EQUIPMENT_STRINGS == 3

    vectors: list[dict] = []

    def add(name, cmd_id, encoded, fields, *, encoder, batch=None,
            built_by=None):
        data, bits = encoded
        data = bytes(data)
        assert (bits + 7) // 8 == len(data), (name, bits, len(data))
        envelope = f"0x{data[0]:02X}"
        assert envelope in ("0x84", "0x85", "0x8B"), (name, envelope)
        fields = dict(jsonable(fields))
        fields["encoder"] = encoder
        if built_by:
            fields["built_by"] = built_by
        entry = {
            "name": name,
            "id": int(cmd_id),
            "hex": data.hex(),
            "bits": int(bits),
            "envelope": envelope,
            "fields": fields,
        }
        if batch is not None:
            entry["batch"] = list(batch)
        vectors.append(entry)

    # ------------------------------------------------------------------ Move
    move = gp.Move(
        quantized_x=-1234, quantized_y=2345, quantized_z=-3456,
        scalar_byte=0x41, yaw_byte=0x82, pitch_byte=0xC3,
        start_tick=0x0A0B0C0D, duration_ticks=0x1234,
    )
    add("MoveCommand", gp.MOVE_COMMAND,
        gp.encode_move(move, actor_id=0x00010203),
        {"move": move, "actor_id": 0x00010203}, encoder="encode_move")
    add("MoveCommand", gp.MOVE_COMMAND,
        gp.encode_move_batch(gp.MoveBatch((
            gp.MoveServerCommand(move, 0x00010203),
            gp.MoveServerCommand(
                gp.Move(111, -222, 333, 7, 8, 9, 0x01020304, 0x0506),
                0x00020304),
        ))),
        {"commands": [
            {"move": move, "actor_id": 0x00010203},
            {"move": gp.Move(111, -222, 333, 7, 8, 9, 0x01020304, 0x0506),
             "actor_id": 0x00020304}]},
        encoder="encode_move_batch", batch=["MoveCommand", "MoveCommand"])
    client_move = bytes((0x8B,)) + gp.MOVE_COMMAND.to_bytes(2, "little") \
        + move.encode_body()
    assert gp.decode_move(client_move) == move
    add("MoveCommand", gp.MOVE_COMMAND, (client_move, len(client_move) * 8),
        {"move": move}, encoder="Move.encode_body",
        built_by="helper:Move.encode_body")

    # ------------------------------------------------------- shared helpers
    placement = gp.ActorPlacement(
        field_00=0x00011111, actor_id=0x00022222, field_08=0x33,
        field_0c=True, field_0d=False)
    entry_plain = gp.StatusEffectEntry(
        effect_id=0x0123, instance_id=0x04040404, end_tick=0x0C0C0C0C,
        tick_interval=0x14141414, start_tick=0x08080808, stack_index=0x18181818,
        total_duration=0x10101010, source_level=0x1C1C1C1C, source_actor_id=0x20202020,
        update_timing=True, stacked=False, permanent=True, has_details=False)
    entry_aura = gp.StatusEffectEntry(
        effect_id=0x0456, instance_id=0x00000044, end_tick=0x000000CC,
        tick_interval=0x00000014, start_tick=0x00000088, stack_index=3,
        total_duration=0x00000010, source_level=0x0000001C, source_actor_id=0x00000020,
        update_timing=False, stacked=True, permanent=False, has_details=True,
        values=(1.5, -2.25, 100.125), field_38=False, field_3c=2,
        shape_values=(4.5, 2.75, 1.25, 10.5, -20.25, 30.0))
    status_effect = gp.StatusEffect(clear_existing=True,
                                    entries=(entry_plain, entry_aura))

    # ----------------------------------------------------------- Monsters
    new_monster = gp.NewMonster(
        template_id="MonsterTemplateA",
        stats=(gp.MonsterStat(3, 12.5), gp.MonsterStat(7, -0.75)),
        field_88=0x1_2345_6789, field_90=0x2_3456_789A,
        field_98=0x98989898, field_9c=0x9C9C9C9C, field_a0=0.625,
        position=(101.5, 202.25, -3.125), category=0x5A,
        field_c4=0xC4C4C4C4, placement=placement, field_d8=0xD8D8D8D8,
        field_dc=True, field_e0=0xE0E0E0E0, field_e4=True,
        rank="Champion", field_f0="ExtraF0", actor_id=0x00030405)
    spawn_status = gp.StatusEffect(False, (entry_plain,))
    add("NewMonsterCommand", gp.NEW_MONSTER_COMMAND,
        gp.encode_monster_spawn_batch(gp.MonsterSpawnBatch((
            gp.NewMonsterServerCommand(new_monster),
            gp.StatusEffectServerCommand(spawn_status, 0x00030405),
        ))),
        {"new_monster": new_monster,
         "status_effect": spawn_status, "status_actor_id": 0x00030405},
        encoder="encode_monster_spawn_batch",
        batch=["NewMonsterCommand", "StatusEffectCommand"])

    mu1 = gp.MonsterUpdate(0x1_0000_2020, 0x2_0000_2828, 0x30303030,
                           0x34343434, 0x38383838)
    mu2 = gp.MonsterUpdate(0x3_0000_0001, 0x4_0000_0002, 0x31, 0x35, 0x39)
    vic = gp.ActorsEnterVicinity((0x00040001, 0x00040002))
    add("MonsterUpdateCommand", gp.MONSTER_UPDATE_COMMAND,
        gp.encode_monster_vicinity_batch(gp.MonsterVicinityBatch(
            updates=(gp.MonsterUpdateServerCommand(mu1, 0x00040001),
                     gp.MonsterUpdateServerCommand(mu2, 0x00040002)),
            vicinity=vic, vicinity_actor_id=0x00050006)),
        {"updates": [{"value": mu1, "actor_id": 0x00040001},
                     {"value": mu2, "actor_id": 0x00040002}],
         "vicinity": vic, "vicinity_actor_id": 0x00050006},
        encoder="encode_monster_vicinity_batch",
        batch=["MonsterUpdateCommand", "MonsterUpdateCommand",
               "ActorsEnterVicinityCommand"])
    faction = gp.FactionInfo(placement)
    add("MonsterUpdateCommand", gp.MONSTER_UPDATE_COMMAND,
        gp.encode_monster_update_batch(gp.MonsterUpdateBatch(
            monster=mu1, monster_actor_id=0x00040001, faction=faction,
            faction_actor_id=0x00060007,
            commands=(gp.StatusEffectServerCommand(spawn_status, 0x00040001),))),
        {"monster": mu1, "monster_actor_id": 0x00040001, "faction": faction,
         "faction_actor_id": 0x00060007,
         "commands": [{"value": spawn_status, "actor_id": 0x00040001}]},
        encoder="encode_monster_update_batch",
        batch=["MonsterUpdateCommand", "FactionInfoCommand",
               "StatusEffectCommand"])
    add("DiscardMonsterCommand", gp.DISCARD_MONSTER_COMMAND,
        gp.encode_discard_monster_batch((0x00070001, 0x00070002)),
        {"actor_ids": [0x00070001, 0x00070002]},
        encoder="encode_discard_monster_batch",
        batch=["DiscardMonsterCommand", "DiscardMonsterCommand"])

    # ---------------------------------------------------------------- NPC
    new_npc = gp.NewNPC(
        template_id="NpcTemplateB", instance_guid=bytes(range(1, 17)),
        placement=placement, rotation=1.75, position=(5.5, -6.25, 7.125),
        enabled=True, actor_id=0x00080009)
    # Since 2026-10-21 the batch announces its new NPCs first (the client only
    # picks actors it saw enter the vicinity).
    add("NewNPCCommand", gp.NEW_NPC_COMMAND,
        gp.encode_npc_batch(gp.NPCBatch((new_npc,))),
        {"new_npc": new_npc}, encoder="encode_npc_batch",
        batch=["ActorsEnterVicinityCommand", "NewNPCCommand"])

    # ------------------------------------------------------------- Combat
    hit = cp.Hit(
        server_tick=0x11223344, hit_tags=(0x0A, 0x0B, 0x0C),
        field_48=True, field_49=False,
        current_health=0x1_0203_0405, maximum_health=0x2_0607_0809,
        absorbed_damage=0x0A0A0A0A, shield_damage=0x0B0B0B0B,
        damage=0x0C0C0C0C, damage_type=0x0D, attacker_actor_id=0x000E0E0E,
        source_actor_id=0x000F0F0F, skill_id=0x00101010,
        impact_strength=2.5, field_80=False, field_81=True,
        combat_text_type="DamageDonePoison")
    add("HitCommand", cp.HIT_COMMAND,
        cp.encode_combat_batch(cp.CombatBatch(
            (cp.HitServerCommand(hit, 0x00090001),))),
        {"hit": hit, "actor_id": 0x00090001,
         "skill_hit_marker_bits": cp.SKILL_HIT_MARKER_BITS},
        encoder="encode_combat_batch", batch=["HitCommand"])
    kill = cp.Kill(
        server_tick=0x21222324, hit_tags=(0x1A, 0x1B), animation_time=0x2526,
        field_4c=0x4C4C4C4C, killer_actor_id=0x00092222,
        position=(11.5, 22.25, -33.75), skill_id=0x00093333,
        projectile_id=0x00094444, field_78=True)
    add("KillCommand", cp.KILL_COMMAND,
        cp.encode_combat_batch(cp.CombatBatch(
            (cp.KillServerCommand(kill, 0x00090002),))),
        {"kill": kill, "actor_id": 0x00090002},
        encoder="encode_combat_batch", batch=["KillCommand"])
    add("HitCommand", cp.HIT_COMMAND,
        cp.encode_combat_batch(cp.CombatBatch((
            cp.HitServerCommand(hit, 0x00090001),
            gp.StatusEffectServerCommand(spawn_status, 0x00090001),
            cp.KillServerCommand(kill, 0x00090002),
        ))),
        {"commands": [{"hit": hit, "actor_id": 0x00090001},
                      {"status_effect": spawn_status, "actor_id": 0x00090001},
                      {"kill": kill, "actor_id": 0x00090002}]},
        encoder="encode_combat_batch",
        batch=["HitCommand", "StatusEffectCommand", "KillCommand"])
    add("StatusEffectCommand", gp.STATUS_EFFECT_COMMAND,
        gp.encode_status_effect_batch(status_effect, actor_id=0x000A0001),
        {"status_effect": status_effect, "actor_id": 0x000A0001},
        encoder="encode_status_effect_batch", batch=["StatusEffectCommand"])

    # ---------------------------------------------------- Location effects
    loc = gp.LocationEffectInfo(
        instance_id=0x000B0001, effect_type="AuraEffectType",
        instance_guid="guid-1234", enabled=True, transform_mode=0,
        position=(1.25, 2.5, 3.75), yaw=0.5, scale=(1.5, 2.5, 3.5),
        effect_resource_id="fx_resource_x", field_78=True, field_79=False,
        expires_tick=0x0B0B0B0B, ticks_per_second=0x0000000A,
        starts_tick=0x0A0A0A0A, values=(9.5, -8.25),
        strength_percent=0x00000064, owner_actor_id=0x000B0002,
        placement=placement, field_b0=0x000000B0)
    add("NewLocationEffectCommand", gp.NEW_LOCATION_EFFECT_COMMAND,
        gp.encode_new_location_effect_batch(
            gp.LocationEffectInfoBatch((loc,), actor_id=0x000B0003)),
        {"entries": [loc], "actor_id": 0x000B0003},
        encoder="encode_new_location_effect_batch",
        batch=["NewLocationEffectCommand"])
    add("DiscardLocationEffectCommand", gp.DISCARD_LOCATION_EFFECT_COMMAND,
        gp.encode_discard_location_effect_batch(
            (0x000B0001, 0x000B0005), actor_id=0x000B0003),
        {"instance_ids": [0x000B0001, 0x000B0005], "actor_id": 0x000B0003},
        encoder="encode_discard_location_effect_batch",
        batch=["DiscardLocationEffectCommand", "DiscardLocationEffectCommand"])

    # -------------------------------------------------------------- Skills
    ctx = cp.SkillExecutionContext(
        impact_tick=0x0C0C0001, hit_frame=0x0000000F,
        motion_unblock_frame=0x00000017, skill_unblock_frame=0x0000001F,
        reserved=0x00000029, position=(12.5, -13.25, 14.75, 1.0),
        predicted=True)
    attack_move = gp.MoveServerCommand(move, 0x000C0001)

    def skill_vector(name, cmd_id, attack, fields):
        add(name, cmd_id,
            cp.encode_monster_attack_batch(
                cp.MonsterAttackBatch((attack_move,), (attack,))),
            {"movements": [{"move": move, "actor_id": 0x000C0001}],
             "attack": fields},
            encoder="encode_monster_attack_batch",
            batch=["MoveCommand", name])

    skill = cp.SkillCast(skill_level=0x0102, skill_index=0x0304,
                         aim_angle=0.875, server_tick=0x05060708,
                         cast_id=0x090A0B0C)
    skill_vector("SkillCommand", cp.SKILL_COMMAND,
                 cp.SkillServerCommand(skill, 0x000C0002, ctx),
                 {"value": skill, "actor_id": 0x000C0002, "context": ctx})
    tskill = cp.TargetSkill(skill_level=0x0112, skill_index=0x0314,
                            aim_angle=-0.375, server_tick=0x15161718,
                            cast_id=0x191A1B1C, target_actor_id=0x000C0099,
                            context=ctx)
    skill_vector("TargetSkillCommand", cp.TARGET_SKILL_COMMAND,
                 cp.TargetSkillServerCommand(tskill, 0x000C0003),
                 {"value": tskill, "actor_id": 0x000C0003})
    bullet = cp.BulletSkillServer(
        skill_level=0x0122, skill_index=0x0324, aim_angle=1.125,
        server_tick=0x25262728, cast_id=0x292A2B2C,
        direction=(0.5, -0.25, 0.125, 1.0), variant=0x2D,
        bullet_id="mage_icemissile_bullet_new")
    skill_vector("BulletSkillCommand", cp.BULLET_SKILL_COMMAND,
                 cp.BulletSkillServerCommand(bullet, 0x000C0004, ctx),
                 {"value": bullet, "actor_id": 0x000C0004, "context": ctx})
    tpb = cp.TargetPointBulletSkill(
        skill_level=0x0132, skill_index=0x0334, aim_angle=2.25,
        server_tick=0x35363738, cast_id=0x393A3B3C,
        direction=(0.75, 0.5, -0.5, 1.0), variant=0x3D,
        target_position=(40.5, 41.25, 42.125),
        additional_target_positions=((43.5, 44.5, 45.5),),
        bullet_id="ranger_arrow_bullet")
    skill_vector("TargetPointBulletSkillCommand",
                 cp.TARGET_POINT_BULLET_SKILL_COMMAND,
                 cp.TargetPointBulletSkillServerCommand(
                     tpb, 0x000C0005, ctx, hit_range=0x00000046),
                 {"value": tpb, "actor_id": 0x000C0005, "context": ctx,
                  "hit_range": 0x00000046})
    shifted = cp.ShiftedSkill(
        skill_level=0x0142, skill_index=0x0344, aim_angle=-1.5,
        server_tick=0x45464748, cast_id=0x494A4B4C, variant=0x4D,
        target_position=(50.5, 51.25, 52.125),
        additional_target_positions=((53.5, 54.5, 55.5),))
    skill_vector("ShiftedSkillCommand", cp.SHIFTED_SKILL_COMMAND,
                 cp.ShiftedSkillServerCommand(
                     shifted, 0x000C0006, ctx, pre_impact_delay=0x00000056),
                 {"value": shifted, "actor_id": 0x000C0006, "context": ctx,
                  "pre_impact_delay": 0x00000056,
                  "note": "variant is not written by the server encoder; "
                          "the u8 on the wire is the position count"})
    tbullet = cp.TargetBulletSkill(
        skill_level=0x0152, skill_index=0x0354, aim_angle=0.25,
        server_tick=0x55565758, cast_id=0x595A5B5C,
        direction=(-0.5, 0.25, 0.75, 1.0), variant=0x5D,
        target_actor_id=0x000C0077,
        additional_target_actor_ids=(0x000C0078, 0x000C0079),
        target_positions=((60.5, 61.25, 62.125), (63.5, 64.5, 65.5)),
        bullet_id="dwarf_turret_bullet")
    skill_vector("TargetBulletSkillCommand", cp.TARGET_BULLET_SKILL_COMMAND,
                 cp.TargetBulletSkillServerCommand(tbullet, 0x000C0007, ctx),
                 {"value": tbullet, "actor_id": 0x000C0007, "context": ctx})

    # Client direction, built in the production decoders' read order.
    w = cs.BitWriter()
    w.write_uint8(0x8B)
    w.write_bytes(cp.TARGET_SKILL_COMMAND.to_bytes(2, "little"))
    cp._write_uint16_le(w, 0x0162)
    cp._write_uint16_le(w, 0x0364)
    w.write_float(-2.5)
    w.write_uint32_le(0x65666768)
    w.write_uint32_le(0x696A6B6C)
    w.write_uint32_le(0x000C0088)
    enc = w.finish()
    decoded = cp.decode_target_skill(enc[0], enc[1])
    assert decoded == cp.TargetSkillCast(0x0162, 0x0364, -2.5, 0x65666768,
                                         0x696A6B6C, 0x000C0088, None)
    add("TargetSkillCommand", cp.TARGET_SKILL_COMMAND, enc, decoded,
        encoder="BitWriter (decode_target_skill order)", built_by="hand")

    w = cs.BitWriter()
    w.write_uint8(0x8B)
    w.write_bytes(cp.TARGET_BULLET_SKILL_COMMAND.to_bytes(2, "little"))
    cp._write_uint16_le(w, 0x0172)
    cp._write_uint16_le(w, 0x0374)
    w.write_float(3.25)
    w.write_uint32_le(0x75767778)
    w.write_uint32_le(0x797A7B7C)
    for c in (0.25, -0.75, 0.5, 1.0):
        w.write_float(c)
    w.write_uint8(0x7D)
    w.write_string("mage_firebolt_bullet")  # release261_221 SkillBulletId
    w.write_uint32_le(0x000C0066)
    w.write_uint8(2)
    w.write_uint32_le(0x000C0067)
    w.write_uint32_le(0x000C0068)
    w.write_uint8(2)
    for pos in ((70.5, 71.25, 72.125), (73.5, 74.5, 75.5)):
        for c in pos:
            w.write_float(c)
    enc = w.finish()
    decoded = cp.decode_target_bullet_skill(enc[0], enc[1])
    assert decoded == cp.TargetBulletSkill(
        0x0172, 0x0374, 3.25, 0x75767778, 0x797A7B7C,
        (0.25, -0.75, 0.5, 1.0), 0x7D, 0x000C0066, (0x000C0067, 0x000C0068),
        ((70.5, 71.25, 72.125), (73.5, 74.5, 75.5)), None,
        "mage_firebolt_bullet")
    add("TargetBulletSkillCommand", cp.TARGET_BULLET_SKILL_COMMAND, enc,
        decoded, encoder="BitWriter (decode_target_bullet_skill order)",
        built_by="hand")

    # ------------------------------------------------- Actor stats/vicinity
    stats = gp.ActorStatsUpdate(stat_id=0x1_1122_3344, value=77.5,
                                tertiary_resource=12.25)
    add("ActorStatsUpdateCommand", gp.ACTOR_STATS_UPDATE_COMMAND,
        gp.encode_actor_stats_update_batch(stats, actor_id=0x000D0001),
        {"value": stats, "actor_id": 0x000D0001},
        encoder="encode_actor_stats_update_batch",
        batch=["ActorStatsUpdateCommand"])
    avic = gp.ActorsEnterVicinity((0x000D0002, 0x000D0003, 0x000D0004))
    add("ActorsEnterVicinityCommand", gp.ACTORS_ENTER_VICINITY_COMMAND,
        gp.encode_actors_enter_vicinity_batch(gp.ActorsEnterVicinityBatch(
            value=avic, actor_id=0x000D0005,
            commands=(gp.StatusEffectServerCommand(spawn_status, 0x000D0002),
                      gp.ActorStatsServerCommand(stats, 0x000D0003)))),
        {"value": avic, "actor_id": 0x000D0005,
         "commands": [{"status_effect": spawn_status, "actor_id": 0x000D0002},
                      {"actor_stats": stats, "actor_id": 0x000D0003}]},
        encoder="encode_actors_enter_vicinity_batch",
        batch=["ActorsEnterVicinityCommand", "StatusEffectCommand",
               "ActorStatsUpdateCommand"])

    # ------------------------------------------------------------ NewPlayer
    nplayer = npc.NewPlayerWire(
        flags_20=(True, False, True, True, False, True),
        field_28=0x28282828,
        value_block_2c=tuple(0x2C000000 + i for i in range(1, 9)),
        value_block_4c=tuple(0x4C000000 + i for i in range(1, 9)),
        field_6c=True, field_6d=False, field_70=0x70,
        position=(100.5, 200.25, -300.125), field_90=0x90909090,
        name="HeroName", appearance=(1, 2, 3, 4, 5, 6),
        template_row=0x0000E0E0, field_e8="FieldE8Str",
        field_f0=0x1_F0F0_F0F0, field_f8=0xF8F8F8F8, field_fc=0xFCFCFCFC, field_100=0xFC,
        field_104=placement,
        fields_110=tuple(0x11000000 + i for i in range(1, 9)),
        field_130=(0x13000001, 0x13000002),
        field_150=("str150a", "str150b"),
        fields_170=tuple(0x17000000 + i for i in range(1, 8)),
        field_190=0x2_1919_1919,
        fields_198=(0x19800001, 0x19800002, 0x19800003),
        field_1a8=0x3_1A1A_1A1A,
        field_1b0=(("pair1b0a", 0x4_0000_0001), ("pair1b0b", 0x5_0000_0002)),
        fields_1d0=(0x1D000001, 0x1D000002),
        fields_1d8=tuple(0x1D800000 + i for i in range(1, 9)),
        field_1f8=0x1F81F81F, field_200="Str200",
        fields_208=(0x20800001, 0x20800002),
        player_state_scalars=tuple(0x5C000000 + i for i in range(1, 23)),
        player_state_records=(
            npc.PlayerStateRecord("recA", "descA",
                                  tuple(0x6A000000 + i for i in range(1, 8))),
            npc.PlayerStateRecord("recB", "descB",
                                  tuple(0x6B000000 + i for i in range(1, 8))),
        ),
        player_state_fixed=(0x7F000001, 0x7F000002, 0x7F000003, 0x7F000004),
        player_state_tables=tuple(0x7A000000 + i for i in range(1, 15)),
        bulk_records=(npc.PlayerBulkRecord(0x8B000001, 0x8B000002, 0x83),
                      npc.PlayerBulkRecord(0x8C000001, 0x8C000002, 0x84)),
        field_388=0x00000388,
        fields_38c=tuple(0x38C00000 + i for i in range(1, 9)),
        field_3ac=0xAC, field_3b0="Str3b0",
        fields_3b8=tuple(0x3B800000 + i for i in range(1, 9)),
        actor_id=0x000E0001)
    add("NewPlayerCommand", npc.NEW_PLAYER_COMMAND,
        npc.encode_new_player_wire(nplayer, message_id=0x84),
        {"value": nplayer, "message_id": 0x84},
        encoder="encode_new_player_wire")
    add("NewPlayerCommand", npc.NEW_PLAYER_COMMAND,
        pi.encode_player_init_prefix(
            (pi.ServerCommand(npc.NEW_PLAYER_COMMAND, nplayer, None),),
            include_terminator=True),
        {"value": nplayer, "include_terminator": True},
        encoder="encode_player_init_prefix", batch=["NewPlayerCommand"])

    # --------------------------------------------------------- Remote player
    remote = sp.RemotePlayer(
        actor_id=0x000F0101, name="RemoteHero", guild_name="GuildX",
        class_id=2, gender=1, hair_style=3, beard_style=4, body_style=5,
        body_stature=6, template_row=0x0000F0F0,
        position=(15.5, 16.25, 17.125), heading=1.5,
        character_id=0x1_0000_0ABC, account_id=0x2_0000_0DEF, level=0x2A,
        admin_privileges=True, admin_name_color=False,
        equipment=(
            sp.RemoteEquipmentVisual(1, ("skinA", "skinB"),
                                     (0.25, 0.5, 0.75, 1.0)),
            sp.RemoteEquipmentVisual(4, ("skinC",), None),
        ),
        emblem_id="emblem_x",
        permanent_statuses=("statusP1", "statusP2"),
        animation_set="animset_x", additional_sequences=("seq1", "seq2"),
        fighting_power=0x0000ABCD, title_type=1,
        achievement_title="ach_title_1", pvp_rank_title="pvptitle03",
        honor_points=0x00012345, team=2)
    remote_fields = {"value": remote, "faction_id": remote.faction_id}
    add("NewRemotePlayerCommand", sp.NEW_REMOTE_PLAYER_COMMAND,
        sp.encode_new_remote_player(remote), remote_fields,
        encoder="encode_new_remote_player", batch=["NewRemotePlayerCommand"])
    add("RemotePlayerInfoCommand", sp.REMOTE_PLAYER_INFO_COMMAND,
        sp.encode_remote_player_info(remote), remote_fields,
        encoder="encode_remote_player_info", batch=["RemotePlayerInfoCommand"])
    add("DiscardPlayerCommand", sp.DISCARD_PLAYER_COMMAND,
        sp.encode_discard_player(0x000F0101), {"actor_id": 0x000F0101},
        encoder="encode_discard_player", batch=["DiscardPlayerCommand"])

    # ------------------------------------------------ Player progress batches
    def server_batch(name, cmd_id, value, actor_id):
        add(name, cmd_id,
            pi.encode_server_command_batch(
                (pi.ServerCommand(cmd_id, value, actor_id),),
                include_terminator=True),
            {"value": value, "actor_id": actor_id},
            encoder="encode_server_command_batch", batch=[name])

    server_batch("XPChangedCommand", pi.XP_CHANGED_COMMAND,
                 pi.XPChanged(xp=0x00012345, level=0x00000021,
                              level_base_xp=0x00010000,
                              next_level_base_xp=0x00020000, level_up=True),
                 0x00000001)
    server_batch("PlayerLevelUpdateCommand", pi.PLAYER_LEVEL_UPDATE_COMMAND,
                 pi.PlayerLevelUpdate(level=0x00000022, field_24=0x24242424,
                                      field_28=0x1_2828_2828,
                                      field_30=0x30303030,
                                      field_34=0x34343434),
                 0x00000001)
    server_batch("CurrencyChangedCommand", pi.CURRENCY_CHANGED_COMMAND,
                 pi.CurrencyChanged(currency_type=0x00000111,
                                    amount=0x1_0000_0222,
                                    values=(0x00000333, 0x00000444,
                                            0x00000555),
                                    delta=0x2_0000_0666, notify=True),
                 0x00000001)

    # ------------------------------------------------------------ Inventory
    item1 = ic.ItemInfo(
        item_id=0x00A00001, field_04=0x04, amount=0x0000000E, rarity=0x08,
        socket_count=2, field_11=0x11, field_12=0x12, field_18=0x00001818,
        dye_argb=0xFF102030, dye_color_id=0x0000001C, dye_id="dye_red",
        item_level=0x00000087, stack_vc_value=0x0000000B, glyph_value=0x00000034,
        needs_identification=True, field_3d=False, template_id="item_template_1",
        field_88="str88", ground_position=(1.5, 2.5, 3.5), appearance="str80",
        base_enchantment_tier=0x00000007, equipment_skin="transmog_1", field_3e=True,
        enchantments=[
            ic.ItemEnchantmentInfo(1.25, 0x01, 0x00000E08, 0x00000E0C,
                                   "ench_a"),
            ic.ItemEnchantmentInfo(-2.5, 0x02, 0x00000F08, 0x00000F0C,
                                   "ench_b"),
        ],
        socketed_templates=["gem_a", "gem_b"],
        created_at=(2026, 1, 2, 3, 4, 5))
    item2 = ic.ItemInfo(
        item_id=0x00A00002, field_04=0x44, amount=1, rarity=0x88,
        socket_count=1, field_11=0x21, field_12=0x22, field_18=0x00002828,
        dye_argb=0x80405060, dye_color_id=0x0000002C, dye_id="dye_blue",
        item_level=0x00000050, stack_vc_value=0x0000001B, glyph_value=0x00000044,
        needs_identification=False, field_3d=True, template_id="item_template_2",
        field_88="s88b", ground_position=(4.5, 5.5, 6.5), appearance="s80b",
        base_enchantment_tier=0x00000003, equipment_skin="", field_3e=False,
        enchantments=[ic.ItemEnchantmentInfo(9.75, 0x03, 0x00001008,
                                             0x0000100C, "ench_c")],
        socketed_templates=["gem_c"],
        created_at=(2025, 12, 31, 23, 59, 58))
    inventory = ic.InventoryInfo(
        items=[item1, item2],
        equipment_slots={0x00A00001: (1, 2), 0x00A00002: (7,)},
        storage_slots={0x00A00002: 5}, extra_storage_slots={0x00A00001: 6},
        bank_storage_slots={0x00B00001: 7}, extra_bank_storage_slots={0x00B00002: 8},
        overflow_storage_item_ids=[0x0F800001], invisible_storage_item_ids=[0x11800001, 0x11800002],
        changed_item_ids=[0x00A00001],
        item_id_replacements={0x15800001: 0x15800002},
        ingredient_bag_item_ids=[0x18000001], ingredient_bag_templates=["s1a0a", "s1a0b"],
        storage_upgrade=0x1E, bank_storage_upgrade=0x35, storage_size=0xE0,
        bank_storage_size=0x10E, extra_bank_storage_size=0x0B,
        extra_storage_size=0x0C, health_points=1234.5,
        secondary_resource=67.25, tertiary_resource=8.5, field_1e0=True, actor_id=0x00000001)
    server_batch("InventoryInfoCommand", ic.INVENTORY_INFO_COMMAND,
                 inventory, 0x00000001)
    add("InventoryInfoCommand", ic.INVENTORY_INFO_COMMAND,
        ic.encode_inventory_info(inventory), {"value": inventory},
        encoder="encode_inventory_info")

    server_batch("QuickSlotsInfoCommand", pi.QUICK_SLOTS_INFO_COMMAND,
                 pi.QuickSlotsInfo(quickbars=(
                     (pi.QuickSlotInfo(1, "angrystrike"),
                      pi.QuickSlotInfo(0xFFFFFFFF, None),
                      pi.QuickSlotInfo(2, "potion_x")),
                     (pi.QuickSlotInfo(3, "mightyswing"),),
                 )),
                 0x00000001)

    # ---------------------------------------------------- CharacterSelection
    transform_a = tuple(float(i) + 0.5 for i in range(16))
    transform_b = tuple(-float(i) - 0.25 for i in range(16))
    selection = cs.CharacterSelection(
        operation=1, deny_reason=0x02, active_character_id=0x0BEE0002,
        header_field_b=0x0000B0B0, character_slots=0x00000005,
        header_flag=True, header_field_d=0x0000D0D0, header_field_e=0x0000E0E0,
        locale="DE",
        characters=[
            cs.CharacterRecord(
                name="Alpha", map_id="map_alpha", class_id=0, gender=1,
                hair_style=0x11, beard_style=0x12, body_style=0x13,
                body_stature=0x14, character_id=0x0BEE0001,
                unknown_u32_a=0x0A0A0A0A, premium_currency=0x00000F0F,
                progression_u64=0x1_2345_6789, progression_u32=0x0000ABCD,
                level=0x00000015, transform=transform_a, flag_a=True,
                flag_b=False),
            cs.CharacterRecord(
                name="Bravo", map_id="map_bravo", class_id=2, gender=0,
                hair_style=0x21, beard_style=0x22, body_style=0x23,
                body_stature=0x24, character_id=0x0BEE0002,
                unknown_u32_a=0x0B0B0B0B, premium_currency=0x00001F1F,
                progression_u64=0x2_3456_789A, progression_u32=0x0000BCDE,
                level=0x00000025, transform=transform_b, flag_a=False,
                flag_b=True),
        ],
        equipment=[
            cs.EquipmentRecord("eq_item_1", "eq_app_1", "eq_fx_1",
                               (0.25, 0.5, 0.75, 1.0), True),
            cs.EquipmentRecord("eq_item_2", "eq_app_2", "eq_fx_2",
                               (1.5, 2.5, 3.5, 4.5), False),
        ])
    sel_enc = cs.encode_character_selection(selection)
    assert cs.decode_character_selection(*sel_enc) is not None
    add("CharacterSelectionCommand", cs.COMMAND_ID, sel_enc,
        {"value": selection, "wire": cs.SELECTION_WIRE},
        encoder="encode_character_selection")

    # ------------------------------------------------------------ SwitchMap
    sw = mp.SwitchMapPacket("127.0.0.1:2190", True)
    add("SwitchMapCommand", mp.SWITCH_MAP_COMMAND, sw.encode(),
        {"value": sw}, encoder="SwitchMapPacket.encode")
    gsw = mp.GameplaySwitchMapPacket("10.0.0.2:2190", True)
    add("SwitchMapCommand", mp.SWITCH_MAP_COMMAND, gsw.encode(),
        {"value": gsw}, encoder="GameplaySwitchMapPacket.encode",
        batch=["SwitchMapCommand"])

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(vectors, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {len(vectors)} vectors to {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
