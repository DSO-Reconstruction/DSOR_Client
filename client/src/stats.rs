//! The character sheet's numbers, computed as the 2018 client computes them
//! (it receives none of them: its own attribute system works them out).
//!
//! EVIDENCE (dro_client.exe rel 206.7, static reading; the spec with the quoted
//!   disassembly lives with the export, tools/export_stats.py):
//! - an attribute is `clamp((initial + Σabsolute) × (1 + Σrelative), lower, upper)`,
//!   initial = clamp(base, lower, upper) (Attribute::Update 0x8D9458, and the debug
//!   string of 0x503066);
//! - a player's bases (ClientGameWorld::SetupPlayerAttributes 0x51EF69):
//!   _Template_XPLevels of the class and level: BaseHP -> MaxHealthPoints, BaseMana
//!   -> MaxSkillResource, BaseDamage -> MinDamage and MaxDamage; _Template_Player
//!   of the class: Armor + Resistances[Physical] -> Armor, the other resistances,
//!   BlockRating, BlockDamageReduction, CriticalRating, CriticalDamageFactor;
//!   SkillExecutionSpeed 1.0 with SkillSpeedMin as its lower limit; the rest
//!   _Template_ActorAttributes' defaults;
//! - the texts (CharacterSheetWidget::Refresh 0x641996): integers rounded
//!   floor(v + 0.5); "fmtN" numbers with a comma and N decimals
//!   (FloatToString 0x75BFF9, floatingPointNumberN); armor and resistance % =
//!   100 min(v / (curve.<Armor|Resistance>DamageReductionValue + v), cap); block and
//!   critical % = 100 clamp(v × ChanceMultiplier × curve.<Block|Critical>ChanceFactor,
//!   0, cap) (0xA3D9DE, 0xA3F055); base damage (ComputeBaseDamageRange 0x9BFE61)
//!   x DamageFactorPvE; a mouse button's damage (0x9C0577) x the attack speed when
//!   the skill uses the weapon's DPS, x its DamageModifier; curve = the
//!   _Template_DmgLvlCurves row of the player's level, caps _Globals Max*.
//! UNVERIFIED: worn items are not counted yet (their enchantments, rolled by the
//!   client's own random generator from the item's seed, come next); the XP
//!   figures are the server's (NewPlayer / XPChanged), whose rows the client's
//!   widget reads is not traced; skill damage ignores talents.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "PascalCase")]
pub struct PlayerRow {
    pub skill_speed_min: f32,
    pub armor: f32,
    pub block_rating: f32,
    pub block_damage_reduction: f32,
    pub critical_rating: f32,
    pub critical_damage_factor: f32,
    #[serde(rename = "resist")]
    pub resist: [f32; 6],
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Curve {
    pub armor_damage_reduction_value: f32,
    pub block_chance_factor: f32,
    pub critical_chance_factor: f32,
    pub resistance_damage_reduction_value: f32,
}

#[derive(Asset, TypePath, Deserialize, Debug)]
pub struct StatsData {
    pub attrs: HashMap<String, [f32; 3]>,
    /// class -> level -> [LevelXP, BaseMana, BaseHP, BaseDamage]
    pub levels: HashMap<String, HashMap<String, [f64; 4]>>,
    pub player: HashMap<String, PlayerRow>,
    pub curves: HashMap<String, Curve>,
    pub chance: f32,
    pub globals: HashMap<String, f32>,
    /// id -> [UseWeaponDPS, DamageModifier, title]
    pub skills: HashMap<String, (bool, f32, String)>,
    pub texts: HashMap<String, String>,
}

#[derive(Default, TypePath)]
pub struct StatsLoader;

impl AssetLoader for StatsLoader {
    type Asset = StatsData;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<StatsData, Self::Error> {
        let mut b = Vec::new();
        r.read_to_end(&mut b).await?;
        serde_json::from_slice(&b).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] {
        &["stats.json"]
    }
}

/// The sheet's texts by widget name (charactersheet.ui.json leaves).
#[derive(Resource, Default)]
pub struct Sheet(pub HashMap<&'static str, String>);

#[derive(Resource)]
struct Data(Handle<StatsData>);

pub struct StatsPlugin;

impl Plugin for StatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<StatsData>()
            .register_asset_loader(StatsLoader)
            .init_resource::<Sheet>()
            .add_systems(Startup, |mut commands: Commands, assets: Res<AssetServer>| {
                commands.insert_resource(Data(assets.load("characters/stats.json")));
            })
            .add_systems(Update, compute);
    }
}

/// One actor attribute.
#[derive(Clone, Copy)]
struct Attr {
    initial: f32,
    lower: f32,
    upper: f32,
    abs: f32,
    rel: f32,
}

impl Attr {
    fn value(&self) -> f32 {
        ((self.initial + self.abs) * (1.0 + self.rel)).clamp(self.lower, self.upper.max(self.lower))
    }
}

const CLASSES: [&str; 4] = ["warrior", "mage", "ranger", "dwarf"];
const CLASS_KEYS: [&str; 4] = ["Warrior", "Mage", "Ranger", "Dwarf"];
const RESOURCE_KEYS: [&str; 4] = ["rage", "mana", "concentration", "mechanicResource"];
/// (widget, resistance index in PlayerRow.resist / the attribute name).
const RESISTANCES: [(&str, &str, usize); 5] = [
    ("Fire", "FireResistance", 1),
    ("Nature", "PoisonResistance", 5),
    ("Ice", "IceResistance", 2),
    ("Lightning", "LightningResistance", 3),
    ("DarkMagic", "DarkMagicResistance", 4),
];

/// The client's rounding of a float to an int.
fn round(v: f32) -> i64 {
    (v + 0.5).floor() as i64
}

/// FloatToString(v, decimals): comma, trailing zeros kept.
fn fmt(v: f32, decimals: u32) -> String {
    let p = 10i64.pow(decimals);
    let n = (v as f64 * p as f64 + 0.5).floor() as i64;
    format!("{},{:0width$}", n / p, (n % p).abs(), width = decimals as usize)
}

fn fill(template: &str, args: &[String]) -> String {
    let mut out = template.to_owned();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}:i}}"), a).replace(&format!("{{{i}:s}}"), a);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn compute(
    data: Res<Data>,
    stats: Res<Assets<StatsData>>,
    net: Option<NonSend<crate::net::Net>>,
    players: Query<&crate::character::Character, With<crate::net::LocalPlayer>>,
    demo: Query<&crate::character::Character>,
    time: Res<Time>,
    mut next: Local<f32>,
    mut sheet: ResMut<Sheet>,
) {
    // Twice a second is plenty for a window of figures.
    *next -= time.delta_secs();
    if *next > 0.0 {
        return;
    }
    *next = 0.5;
    let Some(d) = stats.get(&data.0) else { return };
    let character = players.iter().next().or_else(|| demo.iter().next());
    let class = character.map(|c| c.desc.class as usize).unwrap_or(1).min(3);
    let female = character.is_some_and(|c| c.desc.gender == 1);
    let level = net.as_ref().and_then(|n| n.level).unwrap_or(1).clamp(1, 60);
    let cname = CLASSES[class];
    let Some(row) = d.levels.get(cname).and_then(|l| l.get(&level.to_string())) else { return };
    let Some(p) = d.player.get(cname) else { return };
    let curve = d.curves.get(&level.to_string()).cloned().unwrap_or_default();
    let t = |k: &str| d.texts.get(k).cloned().unwrap_or_default();
    let g = |k: &str| d.globals.get(k).copied().unwrap_or(0.8);
    let attr = |name: &str, base: Option<f32>, lower: Option<f32>| {
        let [init, lo, hi] = d.attrs.get(name).copied().unwrap_or([0.0, 0.0, 1.0e8]);
        let lower = lower.unwrap_or(lo);
        Attr { initial: base.unwrap_or(init).clamp(lower, hi.max(lower)), lower, upper: hi, abs: 0.0, rel: 0.0 }
    };
    let max_health = attr("MaxHealthPoints", Some(row[2] as f32), None);
    let max_resource = attr("MaxSkillResource", Some(row[1] as f32), None);
    let min_damage = attr("MinDamage", Some(row[3] as f32), None);
    let max_damage = attr("MaxDamage", Some(row[3] as f32), None);
    let pve = attr("DamageFactorPvE", None, None);
    let speed = attr("SkillExecutionSpeed", Some(1.0), Some(p.skill_speed_min));
    let armor = attr("Armor", Some(p.armor + p.resist[0]), None);
    let block_rating = attr("BlockRating", Some(p.block_rating), None);
    let block_reduction = attr("BlockDamageReduction", Some(p.block_damage_reduction), None);
    let crit_rating = attr("CriticalRating", Some(p.critical_rating), None);
    let crit_damage = attr("CriticalDamage", Some(p.critical_damage_factor), None);

    let mut s: HashMap<&'static str, String> = HashMap::new();
    let mut class_key = CLASS_KEYS[class].to_owned();
    if female && d.texts.contains_key(&format!("{class_key}.f")) {
        class_key.push_str(".f");
    }
    s.insert("CharacterClass", t(&class_key));
    s.insert("Guildname", String::new());
    s.insert("Level", fill(&t("levelShort"), &[level.to_string()]));
    s.insert("NextLevel", fill(&t("levelShort"), &[(level + 1).min(55).to_string()]));
    if let Some((total, _floor, ceiling)) = net.as_ref().and_then(|n| n.xp) {
        s.insert("XP", fill(&t("xp_pve"), &[total.to_string(), ceiling.to_string()]));
    }
    // Base damage: (initial + abs) × (DamageFactorPvE − 1 + rel + 1), no clamp.
    let base = |a: &Attr| (a.initial + a.abs) * (pve.value() + a.rel);
    let damage_text = |lo: f32, hi: f32| {
        let (a, b) = ((lo + 0.5) as i64, (hi + 0.5) as i64);
        if a == b { fill(&t("skillDamage"), &[a.to_string()]) } else { fill(&t("skillDamageRange"), &[a.to_string(), b.to_string()]) }
    };
    let (lo, hi) = (base(&min_damage), base(&max_damage));
    s.insert("BaseDamage", damage_text(lo, hi));
    s.insert("AttackSpeed", fill(&t("attackspersecond"), &[fmt(speed.value(), 3)]));
    // The mouse buttons' skills: quick slot 0 left, 1 right.
    let bar = net.as_ref().map(|n| n.bar.clone()).unwrap_or_default();
    for (slot, name_w, dmg_w) in [(0usize, "LMKSkill", "DamageLMK"), (1, "RMKSkill", "DamageRMK")] {
        match bar.get(slot).cloned().flatten().and_then(|id| d.skills.get(&id).cloned()) {
            Some((weapon_dps, factor, title)) => {
                let k = if weapon_dps { speed.value() } else { 1.0 } * factor;
                s.insert(name_w, title);
                s.insert(dmg_w, damage_text(lo * k, hi * k));
            }
            None => {
                if slot == 1 {
                    s.insert(name_w, t("noskillselected"));
                }
                s.insert(dmg_w, "0".into());
            }
        }
    }
    s.insert("CurHealth", round(net.as_ref().and_then(|n| n.health).unwrap_or(max_health.value())).to_string());
    s.insert("MaxHealth", round(max_health.value()).to_string());
    s.insert("SkillResourceTitle", t(RESOURCE_KEYS[class]));
    s.insert("CurSkillResource", round(net.as_ref().and_then(|n| n.resource).unwrap_or(max_resource.value())).to_string());
    s.insert("MaxSkillResource", round(max_resource.value()).to_string());
    let pct = |v: f32| fill(&t("attributePercentage"), &[fmt(v * 100.0, 2)]);
    let a = armor.value();
    s.insert("Armor", round(a).to_string());
    s.insert("Armor_Percentage", pct((a / (curve.armor_damage_reduction_value + a)).min(g("MaxArmorDamageReduction"))));
    let chance = |v: f32, factor: f32, cap: f32| if v >= 1e-7 { (v * d.chance * factor).clamp(0.0, cap) } else { 0.0 };
    let br = block_rating.value();
    s.insert("BlockRating", round(br).to_string());
    s.insert("BlockRating_Percentage", pct(chance(br, curve.block_chance_factor, g("MaxBlockChance"))));
    let bdr = block_reduction.value();
    s.insert("BlockValue", fill(&t("attributeValueDivision"), &[fmt(bdr, 2)]));
    s.insert("BlockValue_Percentage", fill(&t("attributePercentageMinus"), &[fmt((1.0 - 1.0 / bdr) * 100.0, 2)]));
    let cr = crit_rating.value();
    s.insert("CriticalHit", round(cr).to_string());
    s.insert("CriticalHit_Percentage", pct(chance(cr, curve.critical_chance_factor, g("MaxCriticalChance"))));
    let cd = crit_damage.value();
    s.insert("CriticalDamage", fill(&t("attributeValueMultiplication"), &[fmt(cd, 2)]));
    s.insert("CriticalDamage_Percentage", fill(&t("attributePercentagePlus"), &[fmt((cd - 1.0) * 100.0, 2)]));
    for (widget, name, i) in RESISTANCES {
        let r = attr(name, Some(p.resist[i]), None).value();
        s.insert(widget, round(r).to_string());
        let key: &'static str = match widget {
            "Fire" => "Fire_Percentage",
            "Nature" => "Nature_Percentage",
            "Ice" => "Ice_Percentage",
            "Lightning" => "Lightning_Percentage",
            _ => "DarkMagic_Percentage",
        };
        s.insert(key, pct((r / (curve.resistance_damage_reduction_value + r)).min(g("MaxResistanceDamageReduction"))));
    }
    if sheet.0 != s {
        sheet.0 = s;
    }
}
