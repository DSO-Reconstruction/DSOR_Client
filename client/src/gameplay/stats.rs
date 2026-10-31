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
//!   floor(v + 0.5); "fmtN" numbers with a decimal point and N decimals
//!   (FloatToString 0x75BFF9, floatingPointNumberN); armor and resistance % =
//!   100 min(v / (curve.<Armor|Resistance>DamageReductionValue + v), cap); block and
//!   critical % = 100 clamp(v × ChanceMultiplier × curve.<Block|Critical>ChanceFactor,
//!   0, cap) (0xA3D9DE, 0xA3F055); base damage (ComputeBaseDamageRange 0x9BFE61)
//!   x DamageFactorPvE; a mouse button's damage (0x9C0577) x the attack speed when
//!   the skill uses the weapon's DPS, x its DamageModifier; curve = the
//!   _Template_DmgLvlCurves row of the player's level, caps _Globals Max*.
//! Equipment, seeded enchantments, sockets, sets and active status programs use
//! the server's layered arithmetic in calculation.rs. Conditional combat procs
//! remain server driven; only their received active statuses affect this sheet.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::Deserialize;

mod calculation;
#[cfg(test)]
mod tests;

/// Recomputed when equipment, level or active status instances change.
#[derive(Resource, Default, Debug)]
pub struct CharacterStats(pub HashMap<String, f64>);

/// The talent ranks the equipment adds (TalentLevelBonus: unique items, their
/// sockets, set thresholds), by talent id in lower case; recomputed with
/// CharacterStats. The Skills & Talents window shows them (a green "7/17").
#[derive(Resource, Default, Debug, PartialEq)]
pub struct TalentBonuses(pub HashMap<String, u32>);
impl CharacterStats {
    pub fn value(&self, key: &str) -> Option<f32> { self.0.get(key).map(|v| *v as f32) }
    pub fn animation_speed(&self) -> f32 {
        // SkillExecutionSpeed stops at its 4.0 limit (_Template_ActorAttributes):
        // past it the original attacks no faster, the 6-frame rate 4.1667,
        // whatever the sheet adds up to (the server caps it the same way).
        // Offline: DSOR_TEST_ATTACK_SPEED=<sheet value> stands in for the sheet's.
        static TEST: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
        let test = *TEST.get_or_init(|| std::env::var("DSOR_TEST_ATTACK_SPEED").ok().and_then(|v| v.parse().ok()));
        let raw = test.or_else(|| self.0.get("SkillExecutionSpeed").copied()).unwrap_or(1.0).clamp(0.01, 4.0);
        if raw < 1.0 { return raw as f32; }
        // Server local_combat_state.effective_attack_speed: discrete sequence
        // breakpoints, distinct from the raw speed printed on the sheet.
        [(1.0256,1.25),(1.0811,1.3158),(1.1429,1.3889),(1.2121,1.4706),
         (1.2903,1.5625),(1.3793,1.6667),(1.4815,1.7857),(1.6,1.9231),
         (1.7391,2.0833),(1.9048,2.2727),(2.1053,2.5),(2.3529,2.7778),
         (2.6667,3.125),(3.0769,3.5714),(3.6364,4.1667),(4.4444,5.0),
         (5.7143,6.25),(8.0,8.3333),(13.3333,12.5),(40.0,25.0)]
            .into_iter().take_while(|(threshold,_)| raw+1e-9 >= *threshold).last().map_or(1.0, |(_,rate)| rate)
    }
    /// What the player's skills cost relative to their ResourceCost (1 without
    /// a modifier; 0 inside Banner of War).
    pub fn skill_cost_factor(&self) -> f32 {
        self.value("SkillCostFactor").unwrap_or(1.0).max(0.0)
    }

    pub fn movement_speed(&self) -> Option<f32> {
        self.value("MovementSpeed").map(|v| v + self.value("MovementBonus").unwrap_or(0.0).max(0.0))
    }
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "PascalCase")]
pub struct PlayerRow {
    #[serde(default = "unit")]
    pub class_hitpoints_factor: f32,
    #[serde(default)]
    pub hit_points_regeneration: f32,
    #[serde(default = "unit")]
    pub hit_points_reg_frequency: f32,
    #[serde(default = "unit")]
    pub skill_resource_reg_frequency: f32,
    #[serde(default)]
    pub run_speed: f32,
    #[serde(default)]
    pub skill_resource_regeneration: f32,
    #[serde(default)]
    pub absolute_skill_resource_regeneration: f32,
    #[serde(default)]
    pub absolute_hit_points_regeneration: f32,
    pub skill_speed_min: f32,
    pub armor: f32,
    pub block_rating: f32,
    pub block_damage_reduction: f32,
    pub critical_rating: f32,
    pub critical_damage_factor: f32,
    #[serde(rename = "resist")]
    pub resist: [f32; 6],
}

fn unit() -> f32 { 1.0 }

/// UNVERIFIED for release261: its _Template_DmgLvlCurves has none of these columns
/// (tools/export_stats.py leaves them out), so they read as 0 there.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct Curve {
    pub armor_damage_reduction_value: f32,
    pub block_chance_factor: f32,
    pub critical_chance_factor: f32,
    pub resistance_damage_reduction_value: f32,
}

/// One `skills` row of stats.json: [UseWeaponDPS, DamageModifier, title] and, since
/// release261 (whose class skills have bare ids: "angrystrike"), its CharClass.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(from = "Vec<serde_json::Value>")]
pub struct SkillRow {
    pub weapon_dps: bool,
    pub factor: f32,
    pub title: String,
    /// "" in the 2018 export.
    pub class: String,
}

impl From<Vec<serde_json::Value>> for SkillRow {
    fn from(v: Vec<serde_json::Value>) -> Self {
        let text = |i: usize| v.get(i).and_then(|x| x.as_str()).unwrap_or_default().to_owned();
        Self {
            weapon_dps: v.first().and_then(|x| x.as_bool()).unwrap_or(false),
            factor: v.get(1).and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
            title: text(2),
            class: text(3),
        }
    }
}

#[derive(Asset, TypePath, Deserialize, Debug)]
pub struct StatsData {
    #[serde(default)]
    pub combat: Option<calculation::Catalog>,
    pub attrs: HashMap<String, [f32; 3]>,
    /// class -> level -> [LevelXP, BaseMana, BaseHP, BaseDamage]
    pub levels: HashMap<String, HashMap<String, [f64; 4]>>,
    pub player: HashMap<String, PlayerRow>,
    pub curves: HashMap<String, Curve>,
    pub chance: f32,
    pub globals: HashMap<String, f32>,
    /// id -> [UseWeaponDPS, DamageModifier, title, CharClass]
    pub skills: HashMap<String, SkillRow>,
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

/// The local player's maximum skill resource (rage, mana, focus...), as the
/// character page computes it. A skill's ResourceCost is a fraction of it: the
/// server debits cost_fraction x this maximum (DSOR_Server
/// services/player_resources.py _apply_player_skill_resource).
#[derive(Resource, Clone, Copy, PartialEq)]
pub struct ResourceMax(pub f32);

impl Default for ResourceMax {
    fn default() -> Self { Self(100.0) }
}

/// The character page's list (release261): its headlines, sub-headlines and
/// attribute rows, in the client's order, with their texts as shown.
/// EVIDENCE: dro_client64 (Ghidra dro_client64_2026) 0x1403c6574 fills the list:
///   offensiveValuesHeadline, basedamage, attackspeedNew, criticalvalue (+ its
///   attributePercentage), the class resource and its regeneration, movementSpeed
///   (FUN_1405ce7c0(value, 1000): three decimals), finaldamageincreasevalue,
///   elementDamageIncreaseSubHeadline + fire/ice/lightning/poison/light increases,
///   defensiveValuesHeadline, healthPoints, hpRegen, blockvalue, riftforcevalue,
///   resistenceAndAmorSubHeadline, armor, resistenceFire/Ice/Lightning/DarkMagic/
///   Nature, otherValuesHeadline, xpgain, honordrop, andermant/coin/wisdom/materi
///   drops; rows through 0x1403bdd30, headlines 0x1403bdf34.
#[derive(Resource, Default, PartialEq, Clone)]
pub struct SheetRows(pub Vec<SheetRow>);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind { Headline, SubHeadline, Attribute }

#[derive(Clone, PartialEq, Debug)]
pub struct SheetRow {
    pub kind: RowKind,
    pub label: String,
    pub value: String,
    pub right: String,
}

#[derive(Resource)]
struct Data(Handle<StatsData>);

pub struct StatsPlugin;

impl Plugin for StatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<StatsData>()
            .register_asset_loader(StatsLoader)
            .init_resource::<Sheet>()
            .init_resource::<ResourceMax>()
            .init_resource::<SheetRows>()
            .init_resource::<CharacterStats>()
            .init_resource::<TalentBonuses>()
            .add_systems(Startup, |mut commands: Commands, assets: Res<AssetServer>| {
                commands.insert_resource(Data(assets.load("characters/stats.json")));
            })
            .add_systems(Update, update_stats.before(crate::network::net::NetSystems))
            .add_systems(Update, compute.after(update_stats));
    }
}

fn update_stats(
    data: Res<Data>, stats: Res<Assets<StatsData>>,
    net: Option<NonSend<crate::network::net::Net>>,
    players: Query<&crate::actors::character::Character, With<crate::network::net::LocalPlayer>>,
    mut output: ResMut<CharacterStats>,
    mut previous: Local<Option<(u8, u32, u64, u64)>>,
    mut bonuses: ResMut<TalentBonuses>,
) {
    let _profile = crate::diagnostics::hud::FrameSpan::new("stats::update_stats");
    let (Some(d), Some(net), Some(player)) = (stats.get(&data.0), net, players.iter().next()) else {
        *previous = None;
        if !output.0.is_empty() { output.0.clear(); }
        return;
    };
    let class = player.desc.class.min(3);
    let level = net.level.unwrap_or(1).max(1);
    let key = (class, level, net.inventory.revision, net.stat_revision);
    if *previous == Some(key) && !stats.is_changed() { return; }
    *previous = Some(key);
    let mut effects: Vec<_> = net.stat_effects.values().cloned().collect();
    effects.sort_by_key(|e| e.instance);
    output.0 = calculation::calculate(d, CLASSES[class as usize], level, &net.inventory, effects);
    let talents = calculation::talent_bonuses(d, &net.inventory);
    if bonuses.0 != talents { bonuses.0 = talents; }
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

/// FloatToString(v, decimals): English decimal point, trailing zeros kept.
fn fmt(v: f32, decimals: u32) -> String {
    let p = 10i64.pow(decimals);
    let n = (v as f64 * p as f64 + 0.5).floor() as i64;
    format!("{}.{:0width$}", n / p, (n % p).abs(), width = decimals as usize)
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
    net: Option<NonSend<crate::network::net::Net>>,
    players: Query<&crate::actors::character::Character, With<crate::network::net::LocalPlayer>>,
    demo: Query<&crate::actors::character::Character>,
    time: Res<Time>,
    mut next: Local<f32>,
    mut sheet: ResMut<Sheet>,
    mut rows_out: ResMut<SheetRows>,
    mut resource_max: ResMut<ResourceMax>,
    riding: Query<&crate::actors::mounts::Riding, With<crate::network::net::LocalPlayer>>,
    calculated: Res<CharacterStats>,
) {
    // Twice a second is plenty for a window of figures, but a new calculation
    // (equipment, level, effects: update_stats) shows at once.
    // FAILURE (2026-10-23): a piece put on took up to 0.5 s to reach the sheet.
    *next -= time.delta_secs();
    if *next > 0.0 && !calculated.is_changed() {
        return;
    }
    *next = 0.5;
    let Some(d) = stats.get(&data.0) else { return };
    let character = players.iter().next().or_else(|| demo.iter().next());
    let class = character.map(|c| c.desc.class as usize).unwrap_or(1).min(3);
    let female = character.is_some_and(|c| c.desc.gender == 1);
    let level = net.as_ref().and_then(|n| n.level).unwrap_or(1).max(1);
    let cname = CLASSES[class];
    let Some(row) = d.levels.get(cname).and_then(|l| l.get(&level.to_string())) else { return };
    let Some(p) = d.player.get(cname) else { return };
    let curve = d.curves.get(&level.to_string()).cloned().unwrap_or_default();
    let t = |k: &str| d.texts.get(k).cloned().unwrap_or_default();
    let g = |k: &str| d.globals.get(k).copied().unwrap_or(0.8);
    let attr = |name: &str, base: Option<f32>, lower: Option<f32>| {
        let [init, lo, hi] = d.attrs.get(name).copied().unwrap_or([0.0, 0.0, 1.0e8]);
        let lower = lower.unwrap_or(lo);
        if let Some(value) = calculated.value(name) {
            return Attr { initial: value, lower: f32::MIN, upper: f32::MAX, abs: 0.0, rel: 0.0 };
        }
        Attr { initial: base.unwrap_or(init).clamp(lower, hi.max(lower)), lower, upper: hi, abs: 0.0, rel: 0.0 }
    };
    let max_health = attr("MaxHealthPoints", Some(row[2] as f32), None);
    let max_resource = attr("MaxSkillResource", Some(row[1] as f32), None);
    let max = ResourceMax(max_resource.value().max(1.0));
    if *resource_max != max { *resource_max = max; }
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
    let max_level = d.levels.get(cname).into_iter().flat_map(|l| l.keys()).filter_map(|s| s.parse::<u32>().ok()).max().unwrap_or(level);
    s.insert("NextLevel", fill(&t("levelShort"), &[(level + 1).min(max_level).to_string()]));
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
            Some(SkillRow { weapon_dps, factor, title, .. }) => {
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
    s.insert("BlockRating_Percentage", pct(calculated.value("BlockChance").unwrap_or_else(|| chance(br, curve.block_chance_factor, g("MaxBlockChance")))));
    let bdr = block_reduction.value();
    s.insert("BlockValue", fill(&t("attributeValueDivision"), &[fmt(bdr, 2)]));
    let blocked = calculated.value("BlockDamageReduction").unwrap_or_else(|| if bdr > 0.0 { 1.0 - 1.0 / bdr } else { 0.0 });
    s.insert("BlockValue_Percentage", fill(&t("attributePercentageMinus"), &[fmt(blocked * 100.0, 2)]));
    let cr = crit_rating.value();
    s.insert("CriticalHit", round(cr).to_string());
    s.insert("CriticalHit_Percentage", pct(calculated.value("CriticalChance").unwrap_or_else(|| chance(cr, curve.critical_chance_factor, g("MaxCriticalChance")))));
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
    // The release261 list (SheetRows).
    // Rift and elemental damage increases have no corresponding calculation in
    // the server stat snapshot; conditional skill damage remains server driven.
    let mut rows = Vec::new();
    let head = |rows: &mut Vec<SheetRow>, key: &str| rows.push(SheetRow { kind: RowKind::Headline, label: t(key), value: String::new(), right: String::new() });
    let sub = |rows: &mut Vec<SheetRow>, key: &str| rows.push(SheetRow { kind: RowKind::SubHeadline, label: t(key), value: String::new(), right: String::new() });
    let row = |rows: &mut Vec<SheetRow>, label: String, value: String, right: String| rows.push(SheetRow { kind: RowKind::Attribute, label, value, right });
    let percent = |v: f32| fill(&t("percentageValue"), &[round(v).to_string()]);
    let plus = |v: f32| fill(&t("percentageValuePlus"), &[round(v).to_string()]);
    // The official page: "0.0%" (riftAttributePercentage "{0:s}%", one decimal).
    let rift = |v: f32| fill(&t("riftAttributePercentage"), &[fmt(v, 1)]);
    let initial = |name: &str| calculated.value(name).unwrap_or_else(|| d.attrs.get(name).map(|a| a[0]).unwrap_or(0.0));
    head(&mut rows, "offensiveValuesHeadline");
    row(&mut rows, t("basedamage"), s["BaseDamage"].clone(), String::new());
    row(&mut rows, t("attackspeedNew"), fmt(speed.value(), 3), String::new());
    row(&mut rows, t("criticalvalue"), s["CriticalHit"].clone(), s["CriticalHit_Percentage"].clone());
    row(&mut rows, t(RESOURCE_KEYS[class]), round(max_resource.value()).to_string(), String::new());
    let regen = ["rageRegen", "manaRegen", "concentrationRegen", "mechanicResourceRegen"][class];
    // _Template_Player: relative x the resource + absolute (the official
    // page: a level 10 mage's 12.00, AbsoluteSkillResourceRegeneration 12).
    let regen_value = calculated.value("ResourceRegeneration").unwrap_or(p.skill_resource_regeneration * max_resource.value() + p.absolute_skill_resource_regeneration);
    row(&mut rows, t(regen), fmt(regen_value, 2), String::new());
    // The class's RunSpeed plus the MovementBonus of a ride (crate::actors::mounts).
    let bonus = riding.iter().next().map_or(0.0, |r| r.bonus);
    let run = if p.run_speed > 0.0 { p.run_speed } else { crate::network::net::RUN_SPEED };
    row(&mut rows, t("movementSpeed"), fmt(calculated.movement_speed().unwrap_or(run + bonus), 3), String::new());
    row(&mut rows, t("finaldamageincreasevalue"), rift(0.0), String::new());
    sub(&mut rows, "elementDamageIncreaseSubHeadline");
    for key in ["fireDamageIncrease", "iceDamageIncrease", "lightningDamageIncrease", "poisonDamageIncrease", "lightDamageIncrease"] {
        row(&mut rows, t(key), rift(0.0), String::new());
    }
    head(&mut rows, "defensiveValuesHeadline");
    row(&mut rows, t("healthPoints"), round(max_health.value()).to_string(), String::new());
    // Its absolute part (the official page: 0.00 for a level 10 mage, whose
    // relative HitPointsRegeneration is 0.0083).
    row(&mut rows, t("hpRegen"), fmt(calculated.value("HealthRegeneration").unwrap_or(p.absolute_hit_points_regeneration), 2), String::new());
    row(&mut rows, t("blockvalue"), s["BlockRating"].clone(), s["BlockRating_Percentage"].clone());
    row(&mut rows, t("riftforcevalue"), rift(0.0), String::new());
    sub(&mut rows, "resistenceAndAmorSubHeadline");
    // Armor and resistances read "1.27 %" (percentageValue), not "(1.27 %)".
    let share = |v: f32| fill(&t("percentageValue"), &[fmt(v * 100.0, 2)]);
    row(&mut rows, t("armor"), round(a).to_string(), share((a / (curve.armor_damage_reduction_value + a).max(1e-6)).min(g("MaxArmorDamageReduction"))));
    for (label, name, i) in [("resistenceFire", "FireResistance", 1), ("resistenceIce", "IceResistance", 2), ("resistenceLightning", "LightningResistance", 3),
        ("resistenceDarkMagic", "DarkMagicResistance", 4), ("resistenceNature", "PoisonResistance", 5)] {
        let r = attr(name, Some(p.resist[i]), None).value();
        row(&mut rows, t(label), round(r).to_string(), share((r / (curve.resistance_damage_reduction_value + r).max(1e-6)).min(g("MaxResistanceDamageReduction"))));
    }
    head(&mut rows, "otherValuesHeadline");
    row(&mut rows, t("xpgain"), percent(initial("XPGain") * 100.0), String::new());
    row(&mut rows, t("honordrop"), percent(initial("HonorGain") * 100.0), String::new());
    for key in ["andermantdrop", "coindrop", "wisdomdrop", "materidrop"] {
        row(&mut rows, t(key), plus(calculated.value(key).unwrap_or(0.0) * 100.0), String::new());
    }
    if rows_out.0 != rows {
        rows_out.0 = rows;
    }
    if sheet.0 != s {
        sheet.0 = s;
    }
}
