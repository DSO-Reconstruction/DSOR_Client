//! The server's layered stat arithmetic, evaluated from the same DB4 programs.
//! Item-relative bonuses stay within their item before weapon and actor layers.
use super::*;
use crate::gameplay::{inventory::InventoryState, item_random::Rolls};
use dsor_proto::commands::{combat::StatusEffectElement, inventory::ItemInfo};
use std::collections::HashSet;

#[derive(Debug, Deserialize, Default)]
pub struct Catalog {
    items: HashMap<String, Item>,
    enchantments: HashMap<String, Enchantment>,
    statuses: HashMap<u16, Status>,
    sets: Vec<ItemSet>,
    variables: HashMap<String, f64>,
    progressions: HashMap<u32, HashMap<String, f64>>,
    tiers: HashMap<u32, Vec<[f64; 2]>>,
    base_tiers: HashMap<u32, Vec<[f64; 2]>>,
    globals: HashMap<String, f64>,
}

type Program = Vec<(String, Vec<String>)>;
#[derive(Debug, Deserialize)]
struct Item { damage: [f64; 2], scaled: bool, modifiers: Program, base: Vec<String>, enchantments: Vec<String> }
#[derive(Debug, Deserialize)]
struct Enchantment { modifiers: Program, scaled: bool, tier: bool, minimum: u32, maximum: u32, reference: u32 }
#[derive(Debug, Deserialize)]
struct Status { id: String, stacks: u32, modifiers: Program }
#[derive(Debug, Deserialize)]
struct ItemSet { parts: Vec<String>, tiers: Vec<(usize, Program)> }

#[derive(Clone)]
struct Instruction {
    kind: String, args: Vec<String>, scale: f64, multiplier: f64, roll: Option<f64>, details: Vec<f64>,
}
impl Instruction {
    fn relative(&self) -> bool { self.args.get(1).is_some_and(|s| s == "relative") }
    fn has(&self, scope: &str) -> bool { self.args.iter().skip(2).any(|s| s == scope) }
    fn amount(&self, variables: &HashMap<String, f64>) -> Option<[f64; 2]> {
        let token = self.args.first()?;
        let range = if let Some(key) = token.strip_prefix('$') {
            let value = key.parse::<usize>().ok().map_or_else(|| *variables.get(key).unwrap_or(&0.0),
                |i| self.details.get(i).copied().unwrap_or(0.0));
            [value, value]
        } else {
            let values: Vec<f64> = token.split('|').filter_map(|s| s.parse().ok()).collect();
            if values.is_empty() { return None; }
            [values.iter().copied().fold(f64::INFINITY, f64::min), values.iter().copied().fold(f64::NEG_INFINITY, f64::max)]
        };
        let range = self.roll.map_or(range, |q| { let v = range[0] + (range[1]-range[0])*q.clamp(0.0,1.0); [v,v] });
        Some(range.map(|v| v * self.scale * self.multiplier))
    }
}

impl Catalog {
    fn factor(&self, level: u32, name: &str) -> f64 {
        self.progressions.get(&level).or_else(|| self.progressions.iter()
            .min_by_key(|(n, _)| (n.abs_diff(level), **n)).map(|(_, r)| r))
            .and_then(|r| r.get(name)).copied().unwrap_or(1.0).max(0.0)
    }
    fn global(&self, key: &str, default: f64) -> f64 { self.globals.get(key).copied().unwrap_or(default) }
    fn instruction(&self, m: &(String, Vec<String>), level: u32, reference: u32, scaled: bool, actor: bool) -> Instruction {
        let (kind, args) = m;
        let relative = args.get(1).is_some_and(|s| s == "relative");
        let scopes = &args[args.len().min(2)..];
        let explicit = scopes.iter().find(|s| s.ends_with("factorabsolute") || s.ends_with("factorrelative"));
        let factor = explicit.cloned().unwrap_or_else(|| {
            if !scaled && !scopes.iter().any(|s| s == "levelmodifier") { return String::new(); }
            let base = match kind.as_str() {
                "damage" | "itemdamage" | "weapondamagebonus" => "damage",
                "speed" | "itemspeed" => if scopes.iter().any(|s| matches!(s.as_str(), "skills" | "allplayerskills")) { "attackspersecond" } else { "movementspeed" },
                "criticalvalue" | "itemcriticalvalue" => "critical",
                "armor" | "itemarmor" => "armor",
                "resistance" | "itemresistance" => "resistance",
                "healthpoints" | "itemhealthpoints" | "maxhealthpoints" => "healthpoints",
                "block" | "blockvalue" | "itemblockvalue" | "smallweaponblock" | "largeweaponblock" | "offhandblock" => "block",
                "maxskillresource" => "maxresource",
                _ => return String::new(),
            };
            format!("{base}factor{}", if relative { "relative" } else { "absolute" })
        });
        let scale = if factor.is_empty() || (relative && (actor || !scaled)) { 1.0 } else {
            let r = self.factor(reference, &factor);
            self.factor(level, &factor) / if r == 0.0 { 1.0 } else { r }
        };
        Instruction { kind: kind.clone(), args: args.clone(), scale, multiplier: 1.0, roll: None, details: Vec::new() }
    }
    fn tier(table: &HashMap<u32, Vec<[f64; 2]>>, tier: u32, rarity: usize, roll: f64) -> f64 {
        table.get(&tier).or_else(|| table.get(&0)).and_then(|r| r.get(rarity.min(r.len().saturating_sub(1))))
            .map_or(1.0, |[a,b]| a+(b-a)*roll.clamp(0.0,1.0))
    }
    fn enchantment(&self, id: &str, level: u32) -> Vec<Instruction> {
        let Some(e) = self.enchantments.get(id) else { return Vec::new(); };
        let level = level.max(e.minimum);
        let level = if e.maximum > 0 { level.min(e.maximum) } else { level };
        e.modifiers.iter().map(|m| self.instruction(m, level, e.reference, e.scaled, false)).collect()
    }
    fn item(&self, item: &ItemInfo) -> Vec<Instruction> {
        let Some(def) = self.items.get(&item.template) else { return Vec::new(); };
        let level = item.durability.max(1);
        let mut result: Vec<_> = def.modifiers.iter().map(|m| self.instruction(m, level, 1,
            def.scaled && matches!(m.0.as_str(), "itemdamage"|"itemarmor"|"itemcriticalvalue"|"itemresistance"|"itemhealthpoints"|"itemblockvalue"|"itemspeed"), false)).collect();
        let mut rolls = Rolls::new((item.eighteenth as f32 * 0.5) as u32);
        for id in &def.base {
            for mut instruction in self.enchantment(id, level) {
                if instruction.amount(&HashMap::new()).is_some_and(|[a,b]| a != b) {
                    let roll = rolls.next() as f64;
                    instruction.roll = Some(if self.enchantments[id].tier {
                        Self::tier(&self.base_tiers, item.tier, item.kind as usize, roll)
                    } else { roll });
                }
                result.push(instruction);
            }
        }
        if item.statistics.is_empty() {
            for id in &def.enchantments { result.extend(self.enchantment(id, level)); }
        }
        for stat in &item.statistics {
            let Some(e) = self.enchantments.get(&stat.name) else { continue; };
            for mut instruction in self.enchantment(&stat.name, level) {
                if e.tier { instruction.multiplier = Self::tier(&self.tiers, stat.third, stat.kind.max(0) as usize, stat.value as f64); }
                else { instruction.roll = Some(stat.value as f64); }
                result.push(instruction);
            }
        }
        for id in &item.names {
            let Some(socket) = self.items.get(id) else { continue; };
            result.extend(socket.modifiers.iter().map(|m| self.instruction(m, level, 1, socket.scaled, false)));
            for id in socket.base.iter().chain(&socket.enchantments) { result.extend(self.enchantment(id, level)); }
        }
        result
    }
    fn expand(&self, result: &mut Vec<Instruction>, explicit: &HashSet<u16>, level: u32) {
        let by_name: HashMap<_, _> = self.statuses.iter().map(|(i,s)| (s.id.as_str(), *i)).collect();
        let mut seen = HashSet::new();
        let mut cursor = 0;
        while cursor < result.len().min(256) {
            let i = &result[cursor]; cursor += 1;
            if i.kind != "actorstatuseffect" || i.args.len() < 2 || !matches!(i.args[0].as_str(), "addae"|"addle") { continue; }
            let metadata: HashMap<_, _> = i.args.iter().skip(2).filter_map(|s| s.split_once(':')).collect();
            if !matches!(metadata.get("on").copied().unwrap_or(""), ""|"immediate"|"immediatemanaged")
                || metadata.get("c").is_some_and(|v| v.parse::<f64>().unwrap_or(0.0) < 1.0)
                || metadata.get("enableonruleset").is_some_and(|s| !s.is_empty()) { continue; }
            let Some(&id) = by_name.get(i.args[1].as_str()) else { continue; };
            if explicit.contains(&id) { continue; }
            let mut details = vec![0.0; 5];
            for (key, val) in metadata {
                let Some(index) = key.strip_prefix('$').and_then(|s| s.parse::<usize>().ok()).filter(|i| *i < 64) else { continue; };
                details.resize(details.len().max(index+1), 0.0);
                details[index] = if let Some(parent) = val.strip_prefix('$') { parent.parse::<usize>().ok().and_then(|n| i.details.get(n)).copied().unwrap_or(0.0) }
                    else { val.parse().unwrap_or(0.0) };
            }
            if !seen.insert((id, i.multiplier.to_bits(), details.iter().map(|v| v.to_bits()).collect::<Vec<_>>())) { continue; }
            let multiplier = i.multiplier;
            for m in &self.statuses[&id].modifiers {
                let mut child = self.instruction(m, level, 1, false, true);
                child.multiplier = multiplier; child.details = details.clone(); result.push(child);
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Amount { absolute: f64, relative: f64 }
impl Amount {
    fn add(&mut self, value: f64, relative: bool) { if relative { self.relative += value; } else { self.absolute += value; } }
    fn value(self, base: f64) -> f64 { (base+self.absolute)*(1.0+self.relative).max(0.0) }
}

const ELEMENTS: [&str; 5] = ["fire", "ice", "lightning", "darkmagic", "poison"];
fn resistance_targets(i: &Instruction, item: bool) -> Vec<&'static str> {
    let selected: Vec<_> = ["physical", "fire", "ice", "lightning", "darkmagic", "poison"].into_iter()
        .filter(|s| i.has(s) || (*s == "darkmagic" && i.has("andermagic"))).collect();
    if !selected.is_empty() { return selected; }
    if i.args.iter().skip(2).all(|s| matches!(s.as_str(), "all"|"levelmodifier"|"resistancefactorabsolute"|"resistancefactorrelative")) {
        if item { vec!["all_resistances"] } else { ELEMENTS.to_vec() }
    } else { Vec::new() }
}

/// The talent ranks the equipped items add (TalentLevelBonus: amount, talent):
/// their own and their sockets' modifiers, the sets' reached thresholds; by
/// talent id in lower case (the programs are lowered).
/// EVIDENCE: release261 Game::TalentLevelBonus (talentlevelbonus.cc); the
///   talents window shows them as bonus ranks (data/diagnostics
///   skilltalents-widget.annotated.txt, 0x14036c93c).
pub fn talent_bonuses(d: &StatsData, inventory: &InventoryState) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    let Some(db) = &d.combat else { return out };
    let mut equipped = HashSet::new();
    let mut add = |kind: &str, args: &[String]| {
        if kind != "talentlevelbonus" { return; }
        let (Some(amount), Some(talent)) = (args.first().and_then(|a| a.parse::<f64>().ok()), args.get(1)) else { return };
        *out.entry(talent.clone()).or_insert(0) += amount.max(0.0).round() as u32;
    };
    for (id, slots) in &inventory.allocation.slots {
        // The parked essence (36; 34 on servers before DSOR_Server #43) does not act.
        if slots.contains(&34) || slots.contains(&36) { continue; }
        let Some(item) = inventory.items.get(id) else { continue };
        if !db.items.contains_key(&item.template) { continue; }
        equipped.insert(item.template.as_str());
        for i in db.item(item) { add(&i.kind, &i.args); }
    }
    for set in &db.sets {
        let count = set.parts.iter().filter(|part| equipped.contains(part.as_str())).count();
        for (_, program) in set.tiers.iter().filter(|(n, _)| count >= *n) {
            for (kind, args) in program { add(kind, args); }
        }
    }
    out
}

pub fn calculate(d: &StatsData, class: &str, level: u32, inventory: &InventoryState,
    effects: impl IntoIterator<Item = StatusEffectElement>) -> HashMap<String, f64> {
    let Some(db) = &d.combat else { return HashMap::new(); };
    let Some(p) = d.player.get(class) else { return HashMap::new(); };
    let Some(levels) = d.levels.get(class) else { return HashMap::new(); };
    let Some(base) = levels.get(&level.to_string()) else { return HashMap::new(); };
    let mut groups = Vec::new();
    for (id, slots) in &inventory.allocation.slots {
        // The parked essence (36; 34 on servers before DSOR_Server #43) does not act.
        if slots.contains(&34) || slots.contains(&36) { continue; }
        if let Some(item) = inventory.items.get(id) {
            if let Some(def) = db.items.get(&item.template) { groups.push((item, def, slots, db.item(item))); }
        }
    }
    // Stable ordering matches the wire allocation, including variable writers.
    let equipped: HashSet<_> = groups.iter().map(|(item,_,_,_)| item.template.as_str()).collect();
    let mut active = Vec::new();
    let effects: Vec<_> = effects.into_iter().collect();
    let explicit = effects.iter().map(|e| e.index).collect();
    for effect in effects {
        let Some(status) = db.statuses.get(&effect.index) else { continue; };
        for m in &status.modifiers {
            let mut instruction = db.instruction(m, level, 1, false, true);
            instruction.multiplier = effect.spare.saturating_add(1).min(status.stacks).max(1) as f64;
            instruction.details = effect.tail.as_ref().map(|t| t.parameters.iter().map(|v| *v as f64).collect()).unwrap_or_default();
            active.push(instruction);
        }
    }
    for set in &db.sets {
        let count = set.parts.iter().filter(|part| equipped.contains(part.as_str())).count();
        for (_, program) in set.tiers.iter().filter(|(n,_)| count >= *n) {
            active.extend(program.iter().map(|m| db.instruction(m, level, 1, false, true)));
        }
    }
    // Immediate jewel/rune children are real effects; conditional registrations
    // wait for a server status instance instead of granting their bonus early.
    active.extend(groups.iter().flat_map(|(_,_,_,g)| g.iter()).filter(|i| i.kind == "actorstatuseffect").cloned());
    db.expand(&mut active, &explicit, level);
    let mut variables = db.variables.clone();
    for i in groups.iter().flat_map(|(_,_,_,g)| g.iter()).chain(&active) {
        if i.kind == "actorvariable" && i.args.len() > 2 {
            if let Some([a,b]) = i.amount(&variables) {
                let current = variables.entry(i.args[2].clone()).or_default();
                if i.relative() { *current *= 1.0+(a+b)*0.5; } else { *current += (a+b)*0.5; }
            }
        }
    }
    let mut global = Vec::new();
    let mut equipment: HashMap<&str, f64> = HashMap::new();
    let mut weapon = [0.0; 2];
    let mut other_damage = [0.0; 2];
    let mut weapon_speed = 0.0;
    let mut other_speed = 0.0;
    let mut weapon_category = "";
    let has_offhand = groups.iter().any(|(_,_,s,_)| s.contains(&6));
    for (item, def, slots, program) in &groups {
        let mut attributes: HashMap<&str, Amount> = HashMap::new();
        let mut damage = [Amount::default(); 2];
        for i in program {
            let Some(amount) = i.amount(&variables) else { continue; };
            let value = (amount[0]+amount[1])*0.5;
            let keys = match i.kind.as_str() {
                "itemdamage" => {
                    for n in 0..2 { if !(i.has("min")||i.has("max")) || i.has(if n==0 {"min"} else {"max"}) { damage[n].add(amount[n], i.relative()); } }
                    continue;
                }
                "itemarmor" => vec!["physical"], "itemcriticalvalue" => vec!["critical"],
                "itemresistance" => resistance_targets(i, true),
                "itemblockvalue" => vec!["block"], "itemhealthpoints" => vec!["health"],
                "itemspeed" => {
                    let mut targets = Vec::new();
                    if i.has("movement") || i.has("all") { targets.push("movement"); }
                    if i.has("skills") || i.has("allplayerskills") || i.has("all") { targets.push("speed"); }
                    targets
                }
                _ => { global.push(i.clone()); continue; }
            };
            for key in keys {
                let value = if key == "health" && !i.relative() && i.has("useclasshitpoints") { value*p.class_hitpoints_factor as f64 } else { value };
                attributes.entry(key).or_default().add(value, i.relative());
            }
        }
        let scaling = if def.scaled { db.factor(item.durability.max(1), "damagefactorabsolute") } else { 1.0 };
        let item_damage = [damage[0].value(def.damage[0]*scaling), damage[1].value(def.damage[1]*scaling)];
        let speed = attributes.remove("speed").unwrap_or_default().value(0.0);
        if slots.contains(&5) {
            weapon_category = if slots.contains(&6) { "largeweapon" } else { "smallweapon" };
            weapon_speed += speed;
            for n in 0..2 { weapon[n] += item_damage[n]; }
        } else { other_speed += speed; for n in 0..2 { other_damage[n] += item_damage[n]; } }
        for (key, amount) in attributes { *equipment.entry(key).or_default() += amount.value(0.0); }
    }
    global.extend(active);
    let mut attrs: HashMap<String, Amount> = HashMap::new();
    let mut damage = [Amount::default(); 2];
    let mut active_weapon = [Amount::default(); 2];
    let mut weapon_bonus = 0.0;
    let mut weapon_factor = 0.0;
    let mut speed_bonus = 0.0;
    let mut speed_factor = 0.0;
    let mut regen_multiplier = 0.0;
    for i in &global {
        let Some(amount) = i.amount(&variables) else { continue; };
        let value = (amount[0]+amount[1])*0.5;
        let mut add = |key: &str| attrs.entry(key.to_owned()).or_default().add(value, i.relative());
        match i.kind.as_str() {
            "damage"|"itemdamage" => {
                let bucket = if i.kind == "damage" { &mut damage } else { &mut active_weapon };
                for n in 0..2 { if !(i.has("min")||i.has("max")) || i.has(if n==0 {"min"} else {"max"}) { bucket[n].add(amount[n],i.relative()); } }
            }
            "weapondamagebonus"|"weapondamagefactor"|"weaponspeedbonus"|"weaponspeedfactor" => {
                if (i.has("smallweapon")||i.has("largeweapon")) && !i.has(weapon_category) { continue; }
                match i.kind.as_str() {
                    "weapondamagebonus" => weapon_bonus += value,
                    "weapondamagefactor" if i.relative() => weapon_factor += value,
                    "weaponspeedbonus" => speed_bonus += value,
                    "weaponspeedfactor" => speed_factor += value,
                    _ => (),
                }
            }
            "speed" => {
                if i.has("movement")||i.has("all") { add("movement"); }
                if i.has("skills")||i.has("allplayerskills")||i.has("all") { add("speed"); }
                if i.has("movementbonus") && !i.relative() { add("movement_bonus"); }
            }
            "healthpoints"|"maxhealthpoints" => add("health"), "maxskillresource" => add("resource"),
            "healthpointsregeneration" => {
                if i.relative() && i.has("increaserelative") { regen_multiplier += value; }
                else { add("health_regen"); }
            }
            "skillresourceregeneration" => add("resource_regen"),
            "armor"|"itemarmor" => add("physical"),
            "resistance"|"itemresistance" => for target in resistance_targets(i,false) { add(target); },
            "criticalvalue"|"itemcriticalvalue" => add("critical"),
            "critical" => if i.has("rating") { add("critical"); } else if i.has("damage") { add("critical_damage"); },
            "smallweaponcritical"|"largeweaponcritical"|"offhandcritical" => {
                if i.kind == format!("{weapon_category}critical") || (i.kind=="offhandcritical" && has_offhand) { add("critical_damage"); }
            }
            "block"|"blockvalue"|"smallweaponblock"|"largeweaponblock"|"offhandblock" => {
                if (i.kind.starts_with("smallweapon")||i.kind.starts_with("largeweapon")) && i.kind != format!("{weapon_category}block") { continue; }
                if i.kind=="offhandblock" && !has_offhand { continue; }
                add(if i.has("damagereduction") { "block_reduction" } else { "block" });
            }
            "damagefactorpve" => add("pve"), "damagereflection" => add("reflection"),
            "xpgain" => add("xp"), "honorgain" => add("honor"),
            "virtualcurrencydropamount" => add("coindrop"),
            "dropamount" => {
                for (scope,key) in [("money_rc","andermantdrop"),("money_vc","coindrop"),("money_gc","wisdomdrop"),("materi_fragment_01","materidrop")] {
                    if i.has(scope) { add(key); }
                }
            }
            // Keep additional authored attribute programs available to UI and
            // consumers without interpreting conditional/skill registrations.
            "finaldamageincrease"|"finaldamagereduction" => add(&i.kind),
            // What the player's skills cost (Banner of War's
            // ResourceCost:$0,relative,player_skills, $0 -1: free inside it).
            // UNVERIFIED: scopes naming one skill group are not applied.
            "resourcecost" if i.relative() && i.has("player_skills") => add("skill_cost"),
            _ => (),
        }
    }
    let eq = |key| equipment.get(key).copied().unwrap_or(0.0);
    let a = |key: &str| attrs.get(key).copied().unwrap_or_default();
    let mut out = HashMap::new();
    let mut put = |name: &str, value: f64| { out.insert(name.to_owned(), value); };
    let collector = variables.get("pet_damage_01").copied().unwrap_or(0.0).max(0.0);
    for n in 0..2 {
        damage[n].relative += collector;
        let weapon = (active_weapon[n].value(weapon[n])+weapon_bonus)*(1.0+weapon_factor).max(0.0);
        put(if n==0 { "MinDamage" } else { "MaxDamage" }, damage[n].value(base[3]+weapon+other_damage[n]).max(1.0)*a("pve").value(1.0).max(0.0));
    }
    put("DamageFactorPvE", 1.0); // Already included above.
    let health = a("health").value(base[2]+eq("health")).round_ties_even().max(1.0);
    let resource = a("resource").value(base[1]).round_ties_even().max(1.0);
    put("MaxHealthPoints", health); put("MaxSkillResource", resource);
    put("SkillExecutionSpeed", a("speed").value(1.0+other_speed+(weapon_speed+speed_bonus)*(1.0+speed_factor).max(0.0)).max(p.skill_speed_min as f64));
    let limits = d.attrs.get("MovementSpeed").copied().unwrap_or([5.0,1.0,15.0]);
    put("MovementSpeed", a("movement").value(p.run_speed as f64+eq("movement")).clamp(limits[1] as f64, limits[2] as f64));
    put("MovementBonus", a("movement_bonus").absolute);
    put("SkillCostFactor", a("skill_cost").value(1.0).max(0.0));
    put("Armor", a("physical").value(p.armor as f64+eq("physical")).max(0.0));
    for (index,key) in ELEMENTS.iter().enumerate() {
        put(["FireResistance","IceResistance","LightningResistance","DarkMagicResistance","PoisonResistance"][index],
            a(key).value(p.resist[index+1] as f64+eq(key)+eq("all_resistances")).max(0.0));
    }
    let critical = a("critical").value(eq("critical")).max(0.0);
    let block = a("block").value(eq("block")).max(0.0);
    put("CriticalRating", critical); put("BlockRating", block);
    put("CriticalChance", (critical*db.factor(level,"criticalchancefactor")).min(db.global("MaxCriticalChance",0.8)));
    put("BlockChance", (block*db.factor(level,"blockchancefactor")).min(db.global("MaxBlockChance",0.8)));
    put("CriticalDamage", a("critical_damage").value(db.global("CriticalDamageBonus",2.0)).clamp(1.0,db.global("MaxCriticalDamage",6.0)));
    put("BlockDamageReduction", a("block_reduction").value(0.0).clamp(0.0,db.global("MaxBlockDamageReduction",1.0)));
    let hr = ((p.hit_points_regeneration.max(0.0) as f64+a("health_regen").relative)*(1.0+regen_multiplier).max(0.0)-p.hit_points_regeneration.max(0.0) as f64).max(0.0);
    put("HealthRegeneration", (health*hr+a("health_regen").absolute.max(0.0))/(p.hit_points_reg_frequency as f64).max(0.05));
    put("ResourceRegeneration", (resource*(p.skill_resource_regeneration as f64+a("resource_regen").relative)+p.absolute_skill_resource_regeneration as f64+a("resource_regen").absolute)/(p.skill_resource_reg_frequency as f64).max(0.05));
    put("DamageReflection", a("reflection").absolute.max(0.0));
    put("XPGain", a("xp").value(1.0)); put("HonorGain", a("honor").value(1.0));
    for key in ["andermantdrop","coindrop","wisdomdrop","materidrop","finaldamageincrease","finaldamagereduction"] { put(key,a(key).relative); }
    out
}
