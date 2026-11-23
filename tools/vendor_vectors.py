"""Vendor fixtures from the local server's real codecs (crates/proto/tests/vendor_release261.rs).

Usage: python tools/vendor_vectors.py <server> <output-directory>

TransactionCommand bytes are built with the server's BitWriter in its decoder's
order and checked by that decoder (protocol/commerce_protocol.py
decode_transaction); a purchase's reply is encoded with the batch writer the
server sends it with (services/commerce.py _handle_transaction). Only pure
protocol modules are imported; no player database is opened.
"""
from pathlib import Path
import json
import sys
from prod_vectors import load_package

load_package(Path(sys.argv[1]).resolve())
from dso_server.protocol.character_selection import BitWriter
from dso_server.protocol.commerce_protocol import TRANSACTION_COMMAND, decode_transaction
from dso_server.protocol.currency_protocol import GAME_CURRENCY_CHANGED_COMMAND, GameCurrencyChanged
from dso_server.protocol.inventory_codec import INVENTORY_INFO_COMMAND, InventoryInfo, ItemInfo
from dso_server.protocol.inventory_economy_protocol import InventoryMutationBuilder
from dso_server.protocol import player_init_protocol as init
from dso_server.protocol.player_init_protocol import (
    CURRENCY_CHANGED_COMMAND, QUICKBAR_INFO_COMMAND, TIMED_OFFERS_INFO_COMMAND,
    UPDATE_PROGRESS_TIER_SPENDER_CHALLENGES_COMMAND, CurrencyChanged, QuickbarInfo,
    ServerCommand, TimedOffersInfo, UpdateProgressTierSpenderChallenges, encode_server_command_batch,
)

out = Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
vectors = []
def record(name, packet):
    data, bits = packet; vectors.append(dict(name=name, hex=data.hex(), bits=bits))

# Client -> server: one entry each, the shop the server opened, mode False.
for name, shop, entries in [
    ("buy", "jewelry_hub_all", [("jewelry_ring_01", 0, 0, 2)]),
    ("sell", "smith_hub_a0200", [("", 800, 0, 4)]),
    ("restore", "smith_hub_a0200", [("", 800, 0, 5)]),
]:
    w = BitWriter(); w.write_uint8(0x8B); w.write_bytes(TRANSACTION_COMMAND.to_bytes(2, "little"))
    w.write_uint32_le(len(entries))
    for offer_id, item_id, extra, amount in entries:
        w.write_bool(True); w.write_string(offer_id); w.write_uint32_le(item_id); w.write_uint32_le(extra); w.write_uint32_le(amount)
    w.write_string(shop); w.write_bool(False)
    packet = w.finish()
    decoded = decode_transaction(packet[0], packet[1])
    assert decoded.shop_id == shop and not decoded.field_80
    assert [(e.offer_id, e.field_58, e.field_5c, e.amount) for e in decoded.entries] == entries
    record(name, packet)

# Server -> client: everything a purchase's reply chains (an andermant-priced,
# timed offer paying a keyed currency), in the server's order.
inventory = InventoryInfo(items=[ItemInfo(101, amount=3, template_id="gem_ruby_a", item_level=12)], storage_slots={101: 0}, storage_size=21)
builder = InventoryMutationBuilder(inventory, 500)
builder.restore_item(ItemInfo(0, amount=2, template_id="jewelry_ring_01", item_level=12))
mutation = builder.finish()
currency = CurrencyChanged(1234, 56789, (11, 22, 33), 44, False)
actor = 1
record("purchase_reply", encode_server_command_batch((
    ServerCommand(UPDATE_PROGRESS_TIER_SPENDER_CHALLENGES_COMMAND, UpdateProgressTierSpenderChallenges(40), actor),
    ServerCommand(TIMED_OFFERS_INFO_COMMAND, TimedOffersInfo(0, ()), actor),
    ServerCommand(INVENTORY_INFO_COMMAND, mutation.delta, actor),
    ServerCommand(CURRENCY_CHANGED_COMMAND, currency, actor),
    ServerCommand(GAME_CURRENCY_CHANGED_COMMAND, GameCurrencyChanged("materi_fragment_01", 5), actor),
    ServerCommand(INVENTORY_INFO_COMMAND, mutation.final, actor),
    ServerCommand(QUICKBAR_INFO_COMMAND, QuickbarInfo(1, 2), actor),
    ServerCommand(CURRENCY_CHANGED_COMMAND, currency, actor),
), include_terminator=True))

timed = next(getattr(init, n) for n in dir(init) if n in ("TimedOffer", "TimedOfferEntry", "TimedOfferInfoEntry"))
record("timed_offer", encode_server_command_batch((
    ServerCommand(TIMED_OFFERS_INFO_COMMAND, TimedOffersInfo(1700000000, (timed("daily_ring", 7, 9, (2026, 10, 21, 12, 30, 0)),)), actor),
), include_terminator=True))

(out / "vendor_release261.json").write_text(json.dumps(vectors, indent=2) + "\n", encoding="utf-8")
print(f"wrote {len(vectors)} vectors")
