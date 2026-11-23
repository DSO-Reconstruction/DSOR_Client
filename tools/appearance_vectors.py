"""Create equipment appearance fixtures with the server's actual packet writers.

Usage: python tools/appearance_vectors.py <server> <output-directory>
The preview binaries can be replayed offline with DSOR_INVENTORY_PREVIEW.
No account data is read or modified.
"""
from pathlib import Path
from dataclasses import replace
import json
import sys
from prod_vectors import load_package

load_package(Path(sys.argv[1]).resolve())
from dso_server.protocol.character_selection import BitReader, BitWriter
from dso_server.protocol.inventory_codec import InventoryInfo, ItemInfo, INVENTORY_INFO_COMMAND
from dso_server.protocol.player_init_protocol import ServerCommand, encode_server_command_batch
from dso_server.protocol.gem_protocol import encode_item_info_response
from dso_server.protocol.social_protocol import RemotePlayer, RemoteEquipmentVisual, encode_new_remote_player, encode_remote_player_info

out = Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
vectors = []


def batch(*packets):
    writer = BitWriter()
    writer.write_uint8(0x85)
    for data, bits in packets:
        reader = BitReader(data)
        assert reader.read_uint8() == 0x85
        for _ in range(bits - 8):
            writer.write_bits(reader.read_bits(1), 1)
    return writer.finish()


def record(name, packet):
    data, bits = packet
    vectors.append(dict(name=name, hex=data.hex(), bits=bits))
    (out / (name + '.bin')).write_bytes(data)


def inventory(items, slots):
    return encode_server_command_batch((ServerCommand(INVENTORY_INFO_COMMAND,
        InventoryInfo(items=items, equipment_slots=slots, storage_size=224), 1),))


chest = ItemInfo(101, template_id='mage_base_torso_armor_block_critical', dye_argb=0xFFFF3030)
initial = inventory([chest], {101: (2,)})
changed = encode_item_info_response(replace(chest, equipment_skin='mage_basic_torso_06', dye_argb=0xFF3050FF), actor_id=1)
remote = RemotePlayer(actor_id=0x103C1, name='Appearance preview', guild_name='', class_id=1, gender=1,
    hair_style=0, beard_style=0, body_style=0, body_stature=0, template_row=0,
    position=(24.2, 0.0, 22.0), heading=0.0, character_id=2, account_id=2, level=10,
    equipment=(RemoteEquipmentVisual(2, ('mage_torso_01',), (0.1, 0.9, 0.2, 1.0)),),
    animation_set='mage_female_1h_weapon')
spawn = encode_new_remote_player(remote)
update = encode_remote_player_info(replace(remote, equipment=(RemoteEquipmentVisual(2, ('mage_torso_06',), (0.1, 0.9, 0.2, 1.0)),)))
record('appearance_initial', batch(initial, spawn))
record('appearance_changed', batch(initial, changed, spawn, update))
record('appearance_unequipped', batch(initial, inventory([], {}), spawn, encode_remote_player_info(replace(remote, equipment=()))))
(out / 'appearance_release261.json').write_text(json.dumps(vectors, indent=2)+'\n', encoding='utf-8')
