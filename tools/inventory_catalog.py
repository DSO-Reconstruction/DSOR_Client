"""Export release-matched item metadata and authored ingredient collection cells."""
import json
import os
import struct
import re
from item_tooltip_values import export_values, unique_effects

SLOTS = {'HelmetSlot':[0], 'ShoulderSlot':[1], 'TorsoSlot':[2], 'GlovesSlot':[3],
         'BootsSlot':[4], 'RightHandSlot':[5], 'LeftHandSlot':[6],
         'RightHandSlot,LeftHandSlot':[5,6], 'CloakSlot':[7], 'RingSlot':[8,9],
         'AmuletSlot':[10], 'BeltSlot':[11], 'WeaponModSlot':[12],
         'RuneTrinketSlot':list(range(13,20)), 'JewelTrinketSlot':[20,21,22],
         'AncientTrinketSlot':[24], 'MercenaryAmuletSlot':[25],
         'MercenaryCloakSlot':[26], 'MercenaryBeltSlot':[27], 'ConsumableSlot':[37],
         'AmmoSlot':[35,34]}


def modifier_lines(modifiers, table):
    """An item's BaseModifier as its card writes it: each shown modifier's own
    text (its "#key"), the value written in with two decimals, a relative one as
    a percentage (the essences: "300.00% increased damage", "No effect in
    official arenas.")."""
    lines = []
    for part in (modifiers or '').split(';'):
        body, _, key = part.partition('#')
        key = key.strip()
        if not key or key == '~':
            continue
        text = re.sub(r'<[^>]*>', '', table.get('gui.statuseffects.modifier_' + key, ''))
        if not text:
            continue
        fields = body.split(':', 1)[1].split(',') if ':' in body else []
        number = ''
        if fields:
            try:
                value = float(fields[0])
                number = '%.2f' % (value * 100 if len(fields) > 1 and fields[1].strip() == 'relative' else value)
            except ValueError:
                pass
        lines.append(re.sub(r'\{\d+:s\}', number, text))
    return lines


def reward_cards(db, table, local):
    """A sticker book's status-effect rewards as their hover card says them:
    {status: {title, lines}} ("Triad of Life (Permanent)", "+ 3% health points
    while accompanied by a pet"): its title, and each shown modifier's own text
    (its "#key"), the value written in for a placeholder (the Andermant's
    "+{0:s}%")."""
    rewards = set()
    for (encoded,) in db.execute('SELECT StickerbookRewardItems FROM _Template_Stickerbook WHERE StickerbookRewardItems IS NOT NULL'):
        for part in (encoded or '').split(';'):
            fields = part.strip().split(':')
            if len(fields) >= 2:
                rewards.add(fields[1].strip())
    cards = {}
    for sid, modifiers in db.execute('SELECT Id, StartModifiers FROM _Template_StatusEffect'):
        if sid not in rewards:
            continue
        lines = []
        for part in (modifiers or '').split(';'):
            body, _, key = part.partition('#')
            key = key.strip()
            if not key or key == '~':
                continue
            text = re.sub(r'<[^>]*>', '', table.get('gui.statuseffects.modifier_' + key, ''))
            if not text:
                continue
            value = re.search(r':([-0-9.]+),', body)
            number = ('%g' % round(float(value.group(1)) * 100, 4)) if value else ''
            lines.append(re.sub(r'\{\d+:s\}', number, text))
        cards[sid] = dict(title=local(sid + 'Title', sid), lines=lines)
    return cards


def export_inventory(db, textures, loca, assets):
    table = loca.get('en', {})
    # A status effect's title loses to the item's own: both are keyed
    # "pet_meanie_anniversaryTitle", the pet's "Cake Meanie" (db.pet), its
    # effect's "Cake Meanie (Pet)" (db.statuseffect); the original's card names
    # the item.
    # FAILURE (2026-10-31): pets named after their effect ("Karabossa Doll
    #   (Pet)", where the original's card says "Karabossa Doll").
    ranked = sorted((k for k in table if k.startswith('db.')), key=lambda k: not k.startswith('db.statuseffect.'))
    short = {k.split('_', 1)[-1]: table[k] for k in ranked}
    def local(key, fallback=''):
        if not key:
            return fallback
        for candidate in (key, 'db.item.table_' + key, 'db.item2.table_' + key, 'gui.window.tooltip_' + key):
            if candidate in table:
                return re.sub(r'<[^>]*>', '', table[candidate])
        if key in short:
            return re.sub(r'<[^>]*>', '', short[key])
        return fallback

    def cooldown(r):
        seconds = float(r.get('CoolDown') or 0)
        return dict(cooldown=seconds, cooldown_category=r.get('CoolDownCategory') or '') if seconds > 0 else {}

    # A pet's status effect and its values ($0..$4: the pet's own bonus by
    # rarity), for its card's Effect section (crate::ui::controls::items).
    def pet_status(r):
        sid = r.get('StatusEffectId') or ''
        if not sid or r.get('ItemCategory') not in ('Pet', 'Mercenary'):
            return {}
        values = [round(float(r.get(f'StatusEffectValue{i}') or 0), 6) for i in range(5)]
        while values and values[-1] == 0:
            values.pop()
        return dict(status=sid, status_values=values)
    result = dict(items={}, ingredients=[], colors={}, appearances={}, icon_configs={}, rarity_backgrounds=[], enchantments={})
    cursor = db.execute('SELECT * FROM _Template_Item')
    columns = [c[0] for c in cursor.description]
    for values in cursor:
        r = dict(zip(columns, values))
        category = local(r.get('LocaleIdCategory') or '', r.get('ItemCategory') or '')
        if r.get('ItemCategory') == 'Weapon' and r.get('SlotType') == 'RightHandSlot,LeftHandSlot':
            category = local('weaponSlotBothHand', 'Double-Handed') + ', ' + category
        brush = r.get('IconBrush') or ''
        icon = 'textures/' + brush + '.png' if brush else ''
        if brush and not os.path.isfile(os.path.join(assets, icon)):
            if not textures.get(brush):
                icon = ''
        result['items'][r['Id']] = dict(name=local(r.get('LocaleIdTitle') or r['Id']+'Title', r.get('Name') or r['Id']),
            description=local(r.get('LocaleIdDesc') or r['Id']+'Desc'),
            category=category,
            icon=icon, max_stack=max(1, int(r.get('MaxStackSize') or 1)),
            required_level=max(0, int(r.get('RequiredLevel') or 0)),
            slots=SLOTS.get(r.get('SlotType'), []), class_name=(r.get('CharClass') or '').lower(),
            rarity=r.get('DefaultRarity') or 'Common',
            # The model it lies on the ground as (crate::drops), and the sequence
            # that replaces its rarity glow, if any.
            graphics=r.get('Graphics') or '', drop_sequence=r.get('DropSequenceOverride') or '',
            icon_config=r.get('IconConfiguration') or '',
            # An essence's card text: its slot's line and its modifiers' (their
            # description is only a "Localize:" placeholder).
            **({'card_lines': [table.get('gui.window.tooltip_ammoSlotInfo', '')] + modifier_lines(r.get('BaseModifier'), table)}
               if r.get('ItemCategory') == 'Ammunition' else {}),
            # Seconds it lasts once received (ItemLifeTime: Premium (2 weeks)
            # "Duration: 7d 0h 0m 0s").
            **({'lifetime': int(r['ItemLifeTime'])} if r.get('ItemLifeTime') else {}),
            # The raw category (the localized one is `category`): Mount items are
            # used, not equipped (crate::ui::controls::items).
            item_category=r.get('ItemCategory') or '',
            # Its use cooldown, shared by its category (every mount VanityItem).
            **cooldown(r),
            **pet_status(r),
            unique_effects=unique_effects(r.get('UniqueModifier'), local))
    for eid, name, base in db.execute('SELECT Id, Name, BaseEnchantment FROM _Template_Enchantment'):
        result['enchantments'][eid] = dict(name=(name or eid).removeprefix('Base Enchantment ').removeprefix('Item Enchantment '), base=bool(base))
    # A generated item shows its rolled appearance's icon (ItemInfo.appearance),
    # not its template's: _Template_ItemAppearance.Icon, and its icon frame/background
    # configuration (_Template_IconConfig).
    def texture(brush):
        brush = (brush or '').replace('\\', '/')
        return 'textures/' + brush + '.png' if brush and textures.get(brush) else ''
    # ... and its name: a rolled piece is named by its appearance
    # (db.itemappearance.table_<id>Title: all_amulet_02 "Medallion"), its
    # template's name being the generic one ("Headband" for every hat).
    for app_id, brush, config in db.execute('SELECT Id, Icon, IconConfiguration FROM _Template_ItemAppearance'):
        result['appearances'][app_id] = dict(icon=texture(brush), config=config or '',
            name=local('db.itemappearance.table_' + app_id + 'Title'))
    # `info`: drawn over the frame (InfoTexture: the time-limited items'
    # hourglass, icons_toolkit/frame_icon_time).
    for cid, rarity, background, frame, info in db.execute('SELECT Id, RarityControlled, BackgroundTexture, FrameTexture, InfoTexture FROM _Template_IconConfig'):
        result['icon_configs'][cid] = dict(rarity=bool(rarity), background=texture(background), frame=texture(frame), info=texture(info))
    # RarityControlled configurations take the background of the item's rarity
    # (ItemInfo.kind: Common .. Mythic, icons_toolkit/bg_rarity_<name>).
    result['rarity_backgrounds'] = [texture('icons_toolkit/bg_rarity_' + r) for r in
        ('common', 'uncommon', 'magic', 'rare', 'epic', 'unique', 'set', 'mythic')]
    for row in db.execute('SELECT ItemTemplateId, IngredientBagTab, IngredientBagRarity, IngredientBagSlot, CharClass, UnlockLevel FROM _Template_IngredientBag WHERE IsLive=1 ORDER BY IngredientBagTab, IngredientBagRarity, IngredientBagSlot, Id'):
        item, tab, rarity, slot, cls, level = row
        if item not in result['items']:
            continue
        result['ingredients'].append(dict(template=item, tab=tab, rarity=int(rarity[-2:]) if rarity.startswith('Rarity_') else 0,
            slot=int(slot)-1, class_name=(cls or 'any').lower(), unlock=int(level or 0)))
    for color, rarity, color_id in db.execute('SELECT PrimaryColor,Rarity,ColorId FROM _Template_ItemRarityColors'):
        rgba = list(struct.unpack('<4f', color)) if isinstance(color, bytes) and len(color)==16 else [1,1,1,1]
        rgba[3] = 1.0
        result['colors'][str(color_id)] = rgba
    # A book reward that raises an actor variable is a status effect
    # (sb_pets_increased_hitpoints_01: pet_hitpoint_01 + 0.03): the pets'
    # Collector's Bag bonuses (crate::ui::controls::systems::album).
    # Its icon on the book's progress bar (IconBrush: icons/sb_pets_increased_
    # hitpoints_01_color), as an item's.
    result['raises'], result['reward_icons'] = {}, {}
    for sid, modifiers, brush in db.execute('SELECT Id, StartModifiers, IconBrush FROM _Template_StatusEffect'):
        raised = re.search(r'ActorVariable:([-0-9.]+),absolute,(\w+)', modifiers or '')
        if raised:
            result['raises'][sid] = [raised.group(2), round(float(raised.group(1)), 6)]
            if texture(brush):
                result['reward_icons'][sid] = texture(brush)
    result['reward_cards'] = reward_cards(db, table, local)
    # The actor variables' upper limits (pet_hitpoint_01 10, pet_andermant_01 0.15).
    result['variables'] = {v: round(float(hi), 6) for v, hi in db.execute('SELECT Id, UpperLimit FROM _Template_ActorVariables')}
    path = os.path.join(assets, 'interface', 'inventory.items.json')
    result['values'] = export_values(db, local)
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(result, f, ensure_ascii=False, separators=(',', ':'))
    return len(result['items']), len(result['ingredients'])
