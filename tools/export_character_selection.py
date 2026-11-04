"""Export character-selection camera, equipment slots, cosmetic choices and the
character creation screen's data.

Usage: export_character_selection.py <static.db4> <assets> [--language <export_win32>]
The scene itself is maps/a0000_char and the UI is charselectionwindow.bxml and
chargenwindow.bxml. The creation texts come from <export_win32>/language/en
(by default the folder the database sits in: <export_win32>/db/static.db4).
"""
import json
import sqlite3
import struct
import xml.etree.ElementTree as ET
from pathlib import Path

# The wire's class ids (CharacterGenerationCommand, NewPlayer) and the class
# names of the database's columns and rows.
# Niwalk (4, the Night Walker) was left unfinished by the original: it has
# _Template_PlayerCharacter rows and hair/beard columns, but no start
# equipment and no texts; it borrows the warrior's outfit (NIWALK_STAND_IN).
CLASSES = ['Warrior', 'Mage', 'Ranger', 'Dwarf', 'Niwalk']
NIWALK_STAND_IN = 'warrior'
# What the language files lack for the Night Walker (gui.window.chargen_*).
NIWALK_TEXTS = {
    'gui.window.chargen_charClass_niwalk': 'Night Walker',
    'gui.window.chargen_charClass_niwalk.f': 'Night Walker',
    'gui.window.chargen_chardesc_niwalk': (
        'A killer of the night who fights with a dagger in each hand. The Night Walker '
        'draws blood with every quick cut, then spends it on cruel tricks, sudden '
        'leaps and wounds that cost a little of their own life.'),
    'gui.window.chargen_charslogan_niwalk': 'Every cut feeds the next.',
}
GENDERS = ['Male', 'Female']


def selection_data(db):
    db.row_factory = sqlite3.Row
    globals_ = db.execute('SELECT * FROM _Globals').fetchone()
    slot_names = ['HelmetSlot', 'ShoulderSlot', 'TorsoSlot', 'GlovesSlot',
                  'BootsSlot', 'RightHandSlot', 'LeftHandSlot', 'CloakSlot']
    slots = {}
    for row in db.execute('SELECT Id, SlotType FROM _Template_Item'):
        indices = [slot_names.index(s.strip()) for s in (row['SlotType'] or '').split(',')
                   if s.strip() in slot_names]
        if indices:
            slots[row['Id']] = indices
    cosmetics = {}
    for kind, table in [('hair', '_Template_CharacterHair'), ('beard', '_Template_CharacterBeard')]:
        choices = list(db.execute(f'SELECT * FROM {table}'))
        cosmetics[kind] = {
            f'{cls.lower()}_{gender.lower()}': {
                str(r['Index']): r['Name'] for r in choices if r[cls + gender]
            }
            for cls in CLASSES for gender in GENDERS
        }
    return dict(camera=dict(pitch=globals_['CharGenAngleX'], yaw=globals_['CharGenAngleY'],
                            offset=struct.unpack('<4f', globals_['CharGenOffsetVector'])[:3],
                            fov=globals_['CharGenFov']), slots=slots, **cosmetics)


def language(root):
    """language/en/*.xml -> {'<category>_<item>': text}, the keys the client asks for."""
    table = {}
    for path in sorted((Path(root) / 'language' / 'en').glob('*.xml')):
        try:
            tree = ET.parse(path)
        except ET.ParseError:
            continue
        for category in tree.getroot().iter('category'):
            for item in category.iter('item'):
                table[f"{category.get('name', '')}_{item.get('name', '')}"] = item.text or ''
    return table


def creation_data(db, texts):
    """What the creation screen (UI::CharacterGenerationWidget) shows and sends:
    per class its preview outfit (_Template_StartEquipment.CharGenItems) and
    armament and, per gender, the showcase sequences (_Template_PlayerCharacter)
    and the hair and beard choices; the name's length bounds and the most
    characters an account has (_Globals); the screens' English texts (and the
    premium start panel's headline, premadepackagewindow).
    SOURCE (the 2026 client, data/diagnostics/chargen-widget.annotated.txt):
      TemplateManager::LoadCharGenInfos 0x140854f68 keys each class and gender's
      hair and beard by the tables' Index (the wire value) for the rows whose
      <Class><Gender> flag is 1, sorted: Prev/Next step through them in Index
      order. UpdateCharacterAppearance 0x14023b380 takes ArmamentState, one
      higher with OffHandEquipped (OneHandSmall 1/2, OneHandLarge 3/4)."""
    db.row_factory = sqlite3.Row
    globals_ = db.execute('SELECT * FROM _Globals').fetchone()
    start = {r['Id']: r for r in db.execute('SELECT * FROM _Template_StartEquipment')}
    characters = {r['Id']: r for r in db.execute('SELECT * FROM _Template_PlayerCharacter')}
    hair = list(db.execute('SELECT * FROM _Template_CharacterHair'))
    beard = list(db.execute('SELECT * FROM _Template_CharacterBeard'))
    split = lambda value: [v.strip() for v in (value or '').split(';') if v.strip()]
    armaments = {'OneHandSmall': 1, 'OneHandLarge': 3, 'TwoHand': 5}
    classes = []
    for cls in CLASSES:
        key = cls.lower()
        outfit = start.get(f'{key}_default_equipment') or start.get(f'{NIWALK_STAND_IN}_default_equipment')
        armament = armaments.get(outfit['ArmamentState'], 0) if outfit else 0
        if armament in (1, 3) and outfit['OffHandEquipped']:
            armament += 1
        genders = []
        for gender in GENDERS:
            row = characters.get(f'character_human_{gender.lower()}_{key}')
            genders.append(dict(
                hair=sorted(int(r['Index']) for r in hair if r[cls + gender]),
                beard=sorted(int(r['Index']) for r in beard if r[cls + gender]),
                start=row['CharGenStartSequence'] if row else '',
                idle=row['CharGenIdleSequence'] if row else '',
            ))
        classes.append(dict(items=split(outfit['CharGenItems']) if outfit else [], armament=armament, genders=genders))
    texts = {**NIWALK_TEXTS, **texts}
    wanted = ('gui.window.chargen_', 'gui.window.charselection_', 'gui.payment.packages_premadePackageHeadline')
    return dict(name=[int(globals_['CharacterNameMinLength']), int(globals_['CharacterNameMaxLength'])],
                most=int(globals_['MaxCharactersPerAccount']),
                classes=classes, texts={k: v for k, v in sorted(texts.items()) if k.startswith(wanted)})


def export_selection(db, assets, root=None):
    path = Path(assets) / 'characters' / 'selection.json'
    path.parent.mkdir(parents=True, exist_ok=True)
    data = selection_data(db)
    texts = language(root) if root else {}
    data['creation'] = creation_data(db, texts)
    path.write_text(json.dumps(data, separators=(',', ':')), encoding='utf-8')
    print(f'selection.json: camera, cosmetics, {len(data["slots"])} equipment slot mappings, '
          f'creation data and {len(data["creation"]["texts"])} texts')


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('database')
    parser.add_argument('assets')
    parser.add_argument('--language', help='the export_win32 folder holding language/en')
    args = parser.parse_args()
    root = args.language or Path(args.database).resolve().parent.parent
    with sqlite3.connect(Path(args.database).resolve().as_uri() + '?mode=ro', uri=True) as db:
        export_selection(db, args.assets, root)
