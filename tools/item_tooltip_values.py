"""Numeric modifier metadata for the client's compact/advanced item cards.

Preserve authored order: the native dSFMT stream consumes ranged base
enchantments in this order. Unsupported scripted modifiers stay in prose.
"""
import re


def program(raw, local=lambda key, fallback='': fallback):
    """Keep item-local and character modifiers distinct, including their scope."""
    result = []
    names = {'Damage': 'Damage', 'Armor': 'Armor', 'CriticalValue': 'Critical value',
             'Resistance': 'Resistance', 'HealthPoints': 'Health Points', 'MaxHealthPoints': 'Health Points',
             'BlockValue': 'Block value', 'Block': 'Block value', 'SkillSpeed': 'Weapon attack speed',
             'MaxSkillResource': 'Maximum skill resource', 'HealthPointsRegeneration': 'Health regeneration per second',
             'SkillResourceRegeneration': 'Skill resource regeneration per second',
             'DamageFactorPvP': 'Damage against players', 'DamagePermeabilityPvP': 'Damage received from players',
             'ResourceCost': 'Skill resource cost', 'CoolDown': 'Cooldown',
             'StatusEffectGroupDuration': 'Effect duration', 'WeaponDamageFactor': 'Weapon damage',
             'DropAmount': 'Drop stack size', 'DropChance': 'Drop chance'}
    factors = {'Damage': 'Damage', 'Armor': 'Armor', 'CriticalValue': 'Critical', 'Resistance': 'Resistance',
               'HealthPoints': 'HealthPoints', 'MaxHealthPoints': 'HealthPoints', 'BlockValue': 'Block',
               'Block': 'Block', 'MaxSkillResource': 'MaxResource'}
    elements = {'Physical': 'Physical', 'Fire': 'Fire', 'Ice': 'Ice', 'Lightning': 'Lightning',
                'Poison': 'Poison', 'DarkMagic': 'Andermagic', 'Andermagic': 'Andermagic'}
    for part in (raw or '').split(';'):
        body, _, override = part.partition('#')
        if override == '~':
            continue  # Authored hidden companion to a visible effect.
        kind, sep, args = body.partition(':')
        if not sep:
            continue
        args = [a.strip() for a in args.split(',')]
        # "hideComparison": a modifier the card does not list as a value (the
        # essences' DamageFactorPvP, shown only as "No effect in official
        # arenas"; the original's essence card has no "Damage against players").
        if any(a.lower() == 'hidecomparison' for a in args):
            continue
        try:
            values = [float(v) for v in args[0].split('|')]
        except ValueError:
            continue
        item_local = kind.startswith('Item')
        stat = kind.removeprefix('Item')
        relative = len(args) > 1 and args[1].lower() == 'relative'
        scopes = args[2:]
        factor = factors.get(stat, '')
        name = names.get(stat)
        if stat == 'Speed':
            attack = any(s in ('Skills', 'AllPlayerSkills') for s in scopes)
            name = 'Attack speed' if attack else 'Movement speed'
            factor = 'AttacksPerSecond' if attack else 'MovementSpeed'
        elif stat == 'Resistance':
            selected = list(dict.fromkeys(elements[s] for s in scopes if s in elements))
            if selected and set(selected) != {'Fire', 'Ice', 'Lightning', 'Poison', 'Andermagic'}:
                name = '/'.join(selected) + ' resistance'
        elif stat in ('CoolDown', 'ResourceCost'):
            skills = [s for s in scopes if s not in ('noScaling', 'LevelModifier')]
            if len(skills) == 1:
                name += ': ' + local('db.skill2.table_' + skills[0] + 'Title', skills[0])
        elif stat == 'StatusEffectGroupDuration':
            effects = {'debuff_dot_burn': 'Burn', 'debuff_dot_poison': 'Poison',
                       'debuff_cc_frost': 'Frost', 'debuff_cc_freeze': 'Freeze'}
            name = '/'.join(effects.get(s, s.replace('_', ' ')) for s in scopes) + ' duration'
        elif stat == 'WeaponDamageFactor' and 'LargeWeapon' in scopes:
            name = 'Two-handed weapon damage'
        elif stat == 'DropAmount' and scopes:
            name = local(scopes[0] + 'Title', scopes[0].replace('_', ' ')) + ' drop stack size'
        elif stat == 'DropChance' and len(args) == 1:
            values = [v - 1.0 for v in values]  # Authored multiplicative factor.
            relative = True
        if name is None:
            continue
        factor = factor + 'Factor' + ('Relative' if relative else 'Absolute') if factor else ''
        explicit = next((a for a in scopes if a.endswith(('FactorAbsolute', 'FactorRelative'))), '')
        if 'noScaling' in scopes:
            factor = explicit = ''
        result.append(dict(name=name, low=min(values), high=max(values), relative=relative,
                           item_local=item_local, factor=explicit or factor,
                           explicit=bool(explicit or 'LevelModifier' in scopes)))
    return result


def unique_effects(raw, local):
    """Export authored proc descriptions alongside numeric bonuses, not as fallback."""
    result = []
    for part in (raw or '').split(';'):
        body, _, override = part.partition('#')
        if not body or override == '~' or program(part, local):
            continue
        kind, _, params = body.partition(':')
        args = params.split(',')
        key = override.lstrip('#') or (args[0] if kind == 'Localize' else '')
        text = local('gui.statuseffects.modifier_' + key) if key else ''
        if kind == 'TalentLevelBonus' and len(args) > 1:
            name = local('db.skilltalent2.table_' + args[1] + 'Title', args[1].replace('_', ' '))
            text = f'Increases the {name} talent by {args[0]}.'
        if kind == 'ActorVariable' and text:
            try:
                amount = abs(float(args[0])) * (100 if 'forceShowAsPercentage' in args or 'relative' in args else 1)
                text = re.sub(r'\{0:s(?::[^}]*)?\}', f'{amount:g}', text)
            except ValueError:
                pass
        text = text.replace('\\n', '\n').strip()
        if text and text not in result:
            result.append(text)
    return result


def export_values(db, local=lambda key, fallback='': fallback):
    def rows(table):
        cursor = db.execute('SELECT * FROM ' + table)
        columns = [c[0] for c in cursor.description]
        return [dict(zip(columns, row)) for row in cursor]

    def ids(raw):
        return [s for s in (raw or '').split(';') if s and '|' not in s]

    items = {r['Id']: dict(base=program(r['BaseModifier'], local), unique=program(r['UniqueModifier'], local),
                          base_enchantments=ids(r['BaseEnchantments']), enchantments=ids(r['Enchantments']),
                          scaled=r['ItemLevelScalingType'] != 'NoScaling') for r in rows('_Template_Item')}
    enchantments = {r['Id']: dict(modifiers=program(r['Modifiers'], local), scaled=r['ScalingMode']=='Absolute',
                                tier=bool(r['UseTierFactor']), min_level=max(1,r['MinItemLevel']),
                                max_level=max(1,r['MaxItemLevel']), reference=max(1,r['ScaleReferenceLevel']))
                    for r in rows('_Template_Enchantment')}
    def tiers(table):
        rarities = ['Common','Uncommon','Magic','Rare','Epic','Unique','Set','Mythic']
        return {str(r['Tier']): [[r['MinPowerFactor'+rarity],r['MaxPowerFactor'+rarity]] for rarity in rarities] for r in rows(table)}
    return dict(items=items, enchantments=enchantments,
                progressions={str(r['Level']): {k: v for k,v in r.items() if k.endswith(('Absolute','Relative'))} for r in rows('_Template_Progressions')},
                tiers=tiers('_Template_EnchantmentTiers'), base_tiers=tiers('_Template_EnchantmentBaseTiers'))
