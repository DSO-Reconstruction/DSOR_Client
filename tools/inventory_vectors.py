"""Generate inventory/correction fixtures using the local server's real encoders.

Usage: python tools/inventory_vectors.py <server> <output-directory>
Only pure protocol modules are imported; no player database is opened.
"""
from pathlib import Path
import json
import sys
from prod_vectors import load_package

load_package(Path(sys.argv[1]).resolve())
from dso_server.protocol.inventory_codec import InventoryInfo, ItemInfo, INVENTORY_INFO_COMMAND
from dso_server.protocol.player_init_protocol import ServerCommand, encode_server_command_batch
from dso_server.protocol.gameplay_protocol import ForceTransform, encode_force_transform_batch, decode_inventory_item_action
from dso_server.protocol.gem_protocol import encode_item_info_response
from dso_server.protocol.inventory_economy_protocol import InventoryMutationBuilder
from dso_server.protocol.character_selection import BitWriter
from dso_server.protocol.membership_protocol import MEMBERSHIP_UPDATE_COMMAND, membership_update_from_expiry

out = Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
inventory = InventoryInfo(items=[ItemInfo(101, amount=10, template_id="consumable_hp", item_level=12),
                                 ItemInfo(102, amount=8, template_id="consumable_hp", item_level=12)],
                          storage_slots={101:0,102:1},storage_size=224)
vectors=[]
def record(name, packet):
    data,bits=packet;vectors.append(dict(name=name,hex=data.hex(),bits=bits))
record('initial', encode_server_command_batch((ServerCommand(INVENTORY_INFO_COMMAND, inventory, 1),)))
builder=InventoryMutationBuilder(inventory,200)
builder.split_stack(101,7,storage_name='storage_slots',storage_slot=2)
m=builder.finish()
record('split',encode_server_command_batch((ServerCommand(INVENTORY_INFO_COMMAND,m.delta,1),ServerCommand(INVENTORY_INFO_COMMAND,m.final,1))))
record('force',encode_force_transform_batch(ForceTransform((14.0,0.0,45.0),-1.6433599),actor_id=1))
record('item_update',encode_item_info_response(ItemInfo(101,amount=7,item_level=19,socket_count=2,socketed_templates=['gem_a','gem_b'],equipment_skin='skin_a'),actor_id=1))
record('premium',encode_server_command_batch((ServerCommand(MEMBERSHIP_UPDATE_COMMAND,
    membership_update_from_expiry('2099-06-07T08:09:10Z',membership_type=1,active=True),1),)))
record('expired',encode_server_command_batch((ServerCommand(MEMBERSHIP_UPDATE_COMMAND,
    membership_update_from_expiry('2000-01-01T00:00:00Z',membership_type=1,active=False),1),)))
record('overflow',encode_server_command_batch((ServerCommand(INVENTORY_INFO_COMMAND,
    InventoryInfo(items=[ItemInfo(301,amount=9,template_id='loose_item')],overflow_storage_item_ids=[301]),1),)))
for name,operation,args in [('split_request',0x1e,[7,2]),('merge_request',0x0d,[102])]:
    w=BitWriter();w.write_uint8(0x8b);w.write_bytes((85).to_bytes(2,'little'));w.write_uint32_le(101)
    w.write_uint8(operation);w.write_uint32_le(len(args))
    for a in args:w.write_uint32_le(a)
    w.write_uint32_le(0);w.write_uint32_le(0)
    packet=w.finish();request=decode_inventory_item_action(packet[0]);assert request.operation==operation and tuple(request.arguments)==tuple(args)
    record(name,packet)
(out/'inventory_release261.json').write_text(json.dumps(vectors,indent=2)+'\n',encoding='utf-8')
