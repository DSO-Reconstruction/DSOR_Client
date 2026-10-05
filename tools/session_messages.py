#!/usr/bin/env python3
"""Print the game-level messages of a captured session, per connection, in order.

Usage: session_messages.py SESSION.jsonl [--port P] [--limit N] [--bytes B]

A session is the server's DSOR_DATAGRAM_LOG capture (one JSON line per datagram,
both directions). Frames are reassembled with the experimental server's own RakNet
package (EXPERIMENTAL env var, default ~/Documents/experimental), so this shows what
the real 2018 client sent and received, message by message.
"""
import json, os, sys, argparse
sys.path.insert(0, os.environ.get("EXPERIMENTAL", os.path.expanduser("~/Documents/experimental")))
from raknet.datagram import parse_datagram_header
from raknet.frame import parse_frames

p = argparse.ArgumentParser()
p.add_argument("session"); p.add_argument("--port", type=int); p.add_argument("--limit", type=int, default=400)
p.add_argument("--bytes", type=int, default=48); p.add_argument("--skip-moves", action="store_true")
a = p.parse_args()
splits = {}
shown = 0
for line in open(a.session):
    d = json.loads(line)
    if a.port and d["server_port"] != a.port:
        continue
    raw = bytes.fromhex(d["hex"])
    side = "S>C" if d["from_server"] else "C>S"
    if not raw[0] & 0x80:
        print(f"{d['frame']:>6} {d['server_port']} {side} offline {raw[0]:#04x}")
        continue
    if raw[0] & 0x40 or raw[0] & 0x20 and not raw[0] & 0x40:
        continue  # ACK / NAK
    _h, off = parse_datagram_header(raw)
    for f in parse_frames(raw, off):
        payload = f.payload
        if f.is_split:
            key = (d["conn"], d["from_server"], f.split_id)
            buf = splits.setdefault(key, {})
            buf[f.split_index] = payload
            if len(buf) < f.split_count:
                continue
            payload = b"".join(buf[i] for i in range(f.split_count)); del splits[key]
        mid = payload[0]
        sub = int.from_bytes(payload[1:3], "little") if mid in (0x84, 0x85, 0x8B) and len(payload) > 2 else None
        if a.skip_moves and mid in (0x00, 0x03) :
            continue
        if a.skip_moves and mid == 0x8B and sub == 103:
            continue
        print(f"{d['frame']:>6} {d['server_port']} {side} {d['conn'][-5:]} rel{int(f.reliability)} "
              f"{mid:#04x}{'' if sub is None else '/'+str(sub)} len{len(payload)} {payload[:a.bytes].hex()}")
        shown += 1
        if shown >= a.limit:
            sys.exit(0)
