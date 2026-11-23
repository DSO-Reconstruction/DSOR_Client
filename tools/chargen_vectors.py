"""Character creation fixtures from the server's codec (crates/proto/tests/chargen_release261.rs).

Usage: python tools/chargen_vectors.py <server> <output-directory>

CharacterGenerationCommand both ways, written by the server's own
encode_character_generation: the editor the server opens (operation 2), a
refusal (4) and a creation (5), and the client's requests for the editor (1)
and for a new character (3), checked by its decoder. Only pure protocol modules
are imported; no player database is opened.
"""
from pathlib import Path
import json
import sys
from prod_vectors import load_package

load_package(Path(sys.argv[1]).resolve())
from dso_server.protocol import character_generation as cg

out = Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
vectors = []
def record(name, packet):
    data, bits = packet; vectors.append(dict(name=name, hex=data.hex(), bits=bits))

# The client's requests: the editor (the selection screen's new-character button)
# and a character (the editor's Create button). Their 16-bit tail is an empty
# string: decode_character_generation reads it as tail bits.
identity = tuple(1.0 if i % 5 == 0 else 0.0 for i in range(16))
request = cg.CharacterGeneration(operation=1, transform=identity)
packet = cg.encode_character_generation(request, server_to_client=False)
assert cg.decode_character_generation(*packet).operation == 1
record("open_editor", packet)
request = cg.CharacterGeneration(operation=3, class_id=3, gender=1, hair_style=31, beard_style=16,
                                 body_style=2, body_stature=0, name="Coppergear", transform=identity)
packet = cg.encode_character_generation(request, server_to_client=False)
decoded = cg.decode_character_generation(*packet)
assert (decoded.class_id, decoded.gender, decoded.hair_style, decoded.beard_style, decoded.body_style, decoded.name) \
    == (3, 1, 31, 16, 2, "Coppergear"), decoded
record("create", packet)

# The server's answers.
record("editor", cg.encode_character_generation(cg.character_generation_editor_response(locale="en", preview_id=7, andermant=1362)))
record("refused", cg.encode_character_generation(cg.character_generation_result(request, operation=4, deny_reason=1)))
record("created", cg.encode_character_generation(cg.character_generation_result(request, operation=5, character_id=1234)))

path = out / "chargen_release261.json"
path.write_text(json.dumps(vectors, indent=1) + "\n", encoding="utf-8")
print(f"{len(vectors)} vectors -> {path}")
