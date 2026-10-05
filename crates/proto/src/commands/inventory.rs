//! Inventory, action bar, skill book, shops and talents.
//!
//! SOURCE: experimental dsor/inventory.py (encode, _write_item, inventory_command),
//!   dsor/actionbar.py (encode_quick_slots), dsor/quickslots.py (decode),
//!   dsor/skillbook.py (encode_book), dsor/trading.py (_encode_shop, _write_offer,
//!   encode_buyback, decode_request, decode_transaction, decode_conversion),
//!   dsor/shop.py (OCP), dsor/talents.py, dsor/travel.py (decode_use_travel_item),
//!   dsor/usable.py (name_in).

use dsor_raknet::{BitReader, BitWriter};

use super::wire::{counted, list, Body, ReadExt, WriteExt, MOST};
use super::DecodeError;

fn byte_count(r: &mut BitReader<'_>) -> Result<usize, DecodeError> {
    Ok(r.u8()? as usize)
}

fn strings_u8(r: &mut BitReader<'_>) -> Result<Vec<String>, DecodeError> {
    let n = byte_count(r)?;
    list(r, n, |r| r.string())
}

fn write_strings_u8(w: &mut BitWriter, v: &[String]) {
    w.u8(v.len() as u8);
    for s in v {
        w.string(s);
    }
}

fn u32_list(r: &mut BitReader<'_>, what: &'static str) -> Result<Vec<u32>, DecodeError> {
    counted(r, MOST, what, |r| r.u32())
}

fn write_u32_list(w: &mut BitWriter, v: &[u32]) {
    w.count(v.len());
    for &x in v {
        w.u32(x);
    }
}

fn pair_list(r: &mut BitReader<'_>, what: &'static str) -> Result<Vec<(u32, u32)>, DecodeError> {
    counted(r, MOST, what, |r| Ok((r.u32()?, r.u32()?)))
}

fn write_pair_list(w: &mut BitWriter, v: &[(u32, u32)]) {
    w.count(v.len());
    for &(a, b) in v {
        w.u32(a);
        w.u32(b);
    }
}

/// A list the server always sends empty and whose element layout is not established:
/// a u32 count that must be zero.
fn empty_list(r: &mut BitReader<'_>, what: &'static str) -> Result<(), DecodeError> {
    let n = r.u32()?;
    if n != 0 {
        return Err(DecodeError::Invalid { what, value: n as i64 });
    }
    Ok(())
}

// ── the item record ─────────────────────────────────────────────────────────

/// One enchantment line of an item.
/// SOURCE: dsor/inventory.py Statistic / _write_item.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemStatistic {
    pub value: f32,
    /// The enchantment's level (stored at the entry's +4 by 0x980AFC); at least 1.
    pub kind: i8,
    pub third: u32,
    pub fourth: u32,
    /// The attribute's name.
    pub name: String,
}

/// Game::ItemInfo, the item record read by 0x980AFC. Carried by InventoryInfo (87),
/// Offer (93, the buyback list) and NewItemCommand (51).
/// SOURCE: dsor/inventory.py _write_item; dsor/items.py encode_new_item_2018 (same
///   reader). Field names are the server's ordinals where meaning is not established.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemInfo {
    /// The item's actor.
    pub id: u32,
    pub second: u32,
    pub third: u32,
    /// Rarity; the reader refuses 6 or more.
    pub kind: u8,
    pub tenth: u8,
    pub eleventh: u8,
    pub twelfth: u8,
    pub eighteenth: u32,
    pub twentieth: u32,
    pub twenty_eighth: u32,
    pub named: String,
    /// Current durability (ItemInfo+40; GameItem+52).
    pub durability: u32,
    /// GameItem::SetBaseLevel; must be above 0.
    pub level: u32,
    pub fifty_second: u32,
    pub unknown_u32_0: u32,
    pub flag_0: bool,
    pub flag_1: bool,
    /// The item template (blueprint) id.
    pub template: String,
    pub second_name: String,
    /// Where it lies on the ground.
    pub position: [f32; 3],
    /// The appearance id (ItemInfo+128).
    pub appearance: String,
    pub tier: u32,
    pub unknown_u32_1: u32,
    pub fourth_name: String,
    pub statistics: Vec<ItemStatistic>,
    pub names: Vec<String>,
    /// The third u8-counted string list (nothing the server sends fills it).
    pub third_names: Vec<String>,
    /// Acquired year, month, day, hour, minute, second (UNKNOWN (2018) order).
    pub stamped: [u32; 6],
}

/// Rarities the item reader accepts (`item+8 >= 6` is refused).
pub const MOST_RARITIES: u8 = 6;

impl Body for ItemInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let id = r.u32()?;
        let second = r.u32()?;
        let third = r.u32()?;
        let kind = r.u8()?;
        if kind >= MOST_RARITIES {
            return Err(DecodeError::Invalid { what: "item rarity", value: kind as i64 });
        }
        let mut item = Self {
            id,
            second,
            third,
            kind,
            tenth: r.u8()?,
            eleventh: r.u8()?,
            twelfth: r.u8()?,
            eighteenth: r.u32()?,
            twentieth: r.u32()?,
            twenty_eighth: r.u32()?,
            named: r.string()?,
            durability: r.u32()?,
            level: r.u32()?,
            fifty_second: r.u32()?,
            unknown_u32_0: r.u32()?,
            flag_0: r.bit()?,
            flag_1: r.bit()?,
            template: r.string()?,
            second_name: r.string()?,
            position: r.vec3()?,
            appearance: r.string()?,
            tier: r.u32()?,
            unknown_u32_1: r.u32()?,
            fourth_name: r.string()?,
            ..Default::default()
        };
        let n = byte_count(r)?;
        item.statistics = list(r, n, |r| {
            Ok(ItemStatistic { value: r.f32()?, kind: r.i8()?, third: r.u32()?, fourth: r.u32()?, name: r.string()? })
        })?;
        item.names = strings_u8(r)?;
        item.third_names = strings_u8(r)?;
        for v in item.stamped.iter_mut() {
            *v = r.u32()?;
        }
        Ok(item)
    }

    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.id);
        w.u32(self.second);
        w.u32(self.third);
        w.u8(self.kind);
        w.u8(self.tenth);
        w.u8(self.eleventh);
        w.u8(self.twelfth);
        w.u32(self.eighteenth);
        w.u32(self.twentieth);
        w.u32(self.twenty_eighth);
        w.string(&self.named);
        w.u32(self.durability);
        w.u32(self.level);
        w.u32(self.fifty_second);
        w.u32(self.unknown_u32_0);
        w.bit(self.flag_0);
        w.bit(self.flag_1);
        w.string(&self.template);
        w.string(&self.second_name);
        w.vec3(self.position);
        w.string(&self.appearance);
        w.u32(self.tier);
        w.u32(self.unknown_u32_1);
        w.string(&self.fourth_name);
        w.u8(self.statistics.len() as u8);
        for s in &self.statistics {
            w.f32(s.value);
            w.i8(s.kind);
            w.u32(s.third);
            w.u32(s.fourth);
            w.string(&s.name);
        }
        write_strings_u8(w, &self.names);
        write_strings_u8(w, &self.third_names);
        for &v in &self.stamped {
            w.u32(v);
        }
    }
}

// ── 87 InventoryInfo ────────────────────────────────────────────────────────

/// InventoryInfoCommand (87): the whole inventory.
/// Nine collections, then six u32, two f32 and one bit.
/// EVIDENCE: reader 0x9A7AA3 / writer 0x9ABBCB (dsor/inventory.py encode);
///   ClientInventoryManager::LocateItem 0x535D75 names the storages.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InventoryInfo {
    pub items: Vec<ItemInfo>,
    /// Storage 1, equipment: item -> the equipment slots it occupies (i8 each).
    pub slots: Vec<(u32, Vec<i8>)>,
    /// Storage 0, the backpack: (item, cell).
    pub placements: Vec<(u32, u32)>,
    /// Storage 2, the extra bag: (item, cell).
    pub extra: Vec<(u32, u32)>,
    /// Storage 3, the bank: (item, cell).
    pub bank: Vec<(u32, u32)>,
    /// Storage 4, the extra bank: (item, cell).
    pub extra_bank: Vec<(u32, u32)>,
    /// Storage 5 (an array searched by value): overflow.
    pub overflow: Vec<u32>,
    /// Storage 6: the invisible storage.
    pub invisible: Vec<u32>,
    /// Items removed (dsor/inventory.py three_hundred_twelfth).
    pub removed: Vec<u32>,
    /// Scalars +204..+224: 2 is the bag's cell count (<= 224), 3 bank, 4 extra bank,
    /// 5 extra bag (dsor/inventory.py load_inventory); 0 and 1 unnamed.
    pub storage_sizes: [u32; 6],
    pub health: f32,
    pub resource: f32,
    pub flag: bool,
}

impl Body for InventoryInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let items = counted(r, MOST, "item count", ItemInfo::decode)?;
        let slots = counted(r, MOST, "slot entries", |r| {
            let key = r.u32()?;
            let cells = counted(r, MOST, "slot cells", |r| r.i8())?;
            Ok((key, cells))
        })?;
        let mut got = Self {
            items,
            slots,
            placements: pair_list(r, "placements")?,
            extra: pair_list(r, "extra bag")?,
            bank: pair_list(r, "bank")?,
            extra_bank: pair_list(r, "extra bank")?,
            overflow: u32_list(r, "overflow")?,
            invisible: u32_list(r, "invisible")?,
            removed: u32_list(r, "removed")?,
            ..Default::default()
        };
        for v in got.storage_sizes.iter_mut() {
            *v = r.u32()?;
        }
        got.health = r.f32()?;
        got.resource = r.f32()?;
        got.flag = r.bit()?;
        Ok(got)
    }

    fn encode(&self, w: &mut BitWriter) {
        w.count(self.items.len());
        for item in &self.items {
            item.encode(w);
        }
        w.count(self.slots.len());
        for (key, cells) in &self.slots {
            w.u32(*key);
            w.count(cells.len());
            for &c in cells {
                w.i8(c);
            }
        }
        write_pair_list(w, &self.placements);
        write_pair_list(w, &self.extra);
        write_pair_list(w, &self.bank);
        write_pair_list(w, &self.extra_bank);
        write_u32_list(w, &self.overflow);
        write_u32_list(w, &self.invisible);
        write_u32_list(w, &self.removed);
        for &v in &self.storage_sizes {
            w.u32(v);
        }
        w.f32(self.health);
        w.f32(self.resource);
        w.bit(self.flag);
    }
}

// ── 83 QuickSlotsInfo / 84 QuickSlots ───────────────────────────────────────

/// One quick slot: an i32 kind (-1 empty, 0 skill, 1 item, 2 item template; the
/// reader refuses the rest) and, unless empty, the id string.
/// EVIDENCE: per-slot reader 0x9BAA43 (dsor/quickslots.py decode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickSlot {
    pub kind: i32,
    /// None exactly when kind is -1.
    pub id: Option<String>,
}

impl QuickSlot {
    pub const EMPTY: i32 = -1;

    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let kind = r.i32()?;
        if !(-1..=2).contains(&kind) {
            return Err(DecodeError::Invalid { what: "quick slot kind", value: kind as i64 });
        }
        let id = if kind == Self::EMPTY { None } else { Some(r.string()?) };
        Ok(Self { kind, id })
    }

    fn encode(&self, w: &mut BitWriter) {
        w.i32(self.kind);
        if self.kind != Self::EMPTY {
            w.string(self.id.as_deref().unwrap_or(""));
        }
    }
}

fn slot_list(r: &mut BitReader<'_>) -> Result<Vec<QuickSlot>, DecodeError> {
    counted(r, MOST, "quick slot count", QuickSlot::decode)
}

fn write_slot_list(w: &mut BitWriter, v: &[QuickSlot]) {
    w.count(v.len());
    for s in v {
        s.encode(w);
    }
}

/// QuickSlotsInfoCommand (83): every quick bar, each a counted list of slots (the
/// client keeps only bars of exactly 18 slots).
/// EVIDENCE: reader chain 0x9A99A6 -> 0x998748 -> 0x998F15 -> 0x9BAA43
///   (dsor/actionbar.py encode_quick_slots).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuickSlotsInfo {
    pub bars: Vec<Vec<QuickSlot>>,
}

impl Body for QuickSlotsInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { bars: counted(r, MOST, "quick bar count", slot_list)? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.bars.len());
        for bar in &self.bars {
            write_slot_list(w, bar);
        }
    }
}

/// QuickSlotsCommand (84), client -> server: one bar's slots, then a u32 the client
/// writes from the command's +24 (differs per bar; index or counter, UNKNOWN).
/// EVIDENCE: decoder 0x9A995C, encoder 0x9AD554 (dsor/quickslots.py).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuickSlots {
    pub slots: Vec<QuickSlot>,
    pub bar: u32,
}

impl Body for QuickSlots {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { slots: slot_list(r)?, bar: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        write_slot_list(w, &self.slots);
        w.u32(self.bar);
    }
}

// ── 88 Inventory (client) ───────────────────────────────────────────────────

/// InventoryCommand (88), client -> server: an item operation.
/// u32 item, i8 operation (refused when op + 1 > 0x23), u32-counted i32 arguments,
/// i32, i32.
/// EVIDENCE: reader 0x9A7A15 (vtable 0xFEEB28 slot 5); operations from the creator
///   handlers: 0 delete, 1 equip, 2..5 store, 6..10 move, 11 swap, 12..16 merge,
///   17 combine, 18 identify, 19 use, 20 upgrade, 21 remove gem, 22 extend sockets,
///   24 flush invisible, 25..30, 31..34 split stack (dsor/inventory.py).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inventory {
    pub item: u32,
    pub operation: i8,
    pub arguments: Vec<i32>,
    pub first: i32,
    pub last: i32,
}

impl Inventory {
    pub const DELETE: i8 = 0;
    pub const EQUIP: i8 = 1;
    pub const STORE: i8 = 2;
    pub const SWAP: i8 = 11;
    pub const USE: i8 = 19;
}

impl Body for Inventory {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let item = r.u32()?;
        let operation = r.i8()?;
        if operation as i32 + 1 > 0x23 {
            return Err(DecodeError::Invalid { what: "inventory operation", value: operation as i64 });
        }
        Ok(Self {
            item,
            operation,
            arguments: counted(r, MOST, "inventory arguments", |r| r.i32())?,
            first: r.i32()?,
            last: r.i32()?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.item);
        w.i8(self.operation);
        w.count(self.arguments.len());
        for &a in &self.arguments {
            w.i32(a);
        }
        w.i32(self.first);
        w.i32(self.last);
    }
}

// ── 91 SkillBookInfo ────────────────────────────────────────────────────────

/// One skill book entry: the skill's index and three bits (the first = owned).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SkillBookEntry {
    pub skill: u32,
    pub owned: bool,
    pub unknown_flag_1: bool,
    pub unknown_flag_2: bool,
}

/// Skills::SkillBookInfoCommand (91): u8 count (0 < n < 255), each a u32 skill index
/// and three bits.
/// EVIDENCE: Encode sub_9FEC10, skillbookinfocommand.cc:42 (dsor/skillbook.py encode_book).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillBookInfo {
    pub entries: Vec<SkillBookEntry>,
}

impl Body for SkillBookInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let n = byte_count(r)?;
        Ok(Self {
            entries: list(r, n, |r| {
                Ok(SkillBookEntry { skill: r.u32()?, owned: r.bit()?, unknown_flag_1: r.bit()?, unknown_flag_2: r.bit()? })
            })?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.entries.len() as u8);
        for e in &self.entries {
            w.u32(e.skill);
            w.bit(e.owned);
            w.bit(e.unknown_flag_1);
            w.bit(e.unknown_flag_2);
        }
    }
}

// ── 92 RequestOffer / 93 Offer / 94 Transaction / 90 CurrencyConversion ─────

/// RequestOfferCommand (92), client -> server: open a shop, by NPC (by_name false,
/// the NPC's actor) or by shop name (by_name true, actor 0).
/// EVIDENCE: reader 0x9A9D5D; senders 0x4E3DE1, 0x57BBA6 (dsor/trading.py decode_request).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestOffer {
    pub by_name: bool,
    pub actor: u32,
    pub shop: String,
}

impl Body for RequestOffer {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { by_name: r.bit()?, actor: r.u32()?, shop: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.bit(self.by_name);
        w.u32(self.actor);
        w.string(&self.shop);
    }
}

/// One Game::Offer (reader sub_A260E1, writer sub_A26554; columns from
/// TradingManager::LoadOffers 0x9C5012). The two maps and two lists at the end are
/// always sent empty and their element layout is not established, so a non-zero
/// count is refused.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShopOffer {
    pub id: String,
    pub price_rc: u32,
    pub price_vc: u32,
    pub price_arena: u32,
    pub price_gc: u32,
    pub price_ec: u32,
    pub price_uc: u32,
    pub price_ic: u32,
    pub amount: u32,
    pub difficulty: u32,
    pub timed_offer_amount: i32,
    pub min_player_level: i32,
    pub max_player_level: i32,
    pub scale_to_player_level: bool,
    pub dynamic_price: bool,
    pub timed_offer: bool,
    pub auto_identify: bool,
    pub is_new: bool,
    pub is_recommended: bool,
    pub hide_not_suitable: bool,
    /// Rarity code (reader sub_9D59DC refuses 6 or more).
    pub rarity: u8,
    /// Game::OfferCategory::Code (reader sub_A166F6 refuses 43 or more).
    pub category: u16,
    pub icon: String,
    /// The item template sold.
    pub item: String,
    /// Offer +224, never loaded from the database.
    pub unknown_string_0: String,
}

impl ShopOffer {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let id = r.string()?;
        let mut o = Self {
            id,
            price_rc: r.u32()?,
            price_vc: r.u32()?,
            price_arena: r.u32()?,
            price_gc: r.u32()?,
            price_ec: r.u32()?,
            price_uc: r.u32()?,
            price_ic: r.u32()?,
            amount: r.u32()?,
            difficulty: r.u32()?,
            timed_offer_amount: r.i32()?,
            min_player_level: r.i32()?,
            max_player_level: r.i32()?,
            ..Default::default()
        };
        o.scale_to_player_level = r.bit()?;
        o.dynamic_price = r.bit()?;
        o.timed_offer = r.bit()?;
        o.auto_identify = r.bit()?;
        o.is_new = r.bit()?;
        o.is_recommended = r.bit()?;
        o.hide_not_suitable = r.bit()?;
        o.rarity = r.u8()?;
        if o.rarity >= 6 {
            return Err(DecodeError::Invalid { what: "offer rarity", value: o.rarity as i64 });
        }
        o.category = r.u16()?;
        if o.category >= 43 {
            return Err(DecodeError::Invalid { what: "offer category", value: o.category as i64 });
        }
        o.icon = r.string()?;
        o.item = r.string()?;
        o.unknown_string_0 = r.string()?;
        empty_list(r, "offer discount events")?;
        empty_list(r, "offer conversion stories")?;
        empty_list(r, "offer event ids")?;
        empty_list(r, "offer disabled-for events")?;
        Ok(o)
    }

    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.id);
        for v in [
            self.price_rc,
            self.price_vc,
            self.price_arena,
            self.price_gc,
            self.price_ec,
            self.price_uc,
            self.price_ic,
            self.amount,
            self.difficulty,
        ] {
            w.u32(v);
        }
        w.i32(self.timed_offer_amount);
        w.i32(self.min_player_level);
        w.i32(self.max_player_level);
        for b in [
            self.scale_to_player_level,
            self.dynamic_price,
            self.timed_offer,
            self.auto_identify,
            self.is_new,
            self.is_recommended,
            self.hide_not_suitable,
        ] {
            w.bit(b);
        }
        w.u8(self.rarity);
        w.u16(self.category);
        w.string(&self.icon);
        w.string(&self.item);
        w.string(&self.unknown_string_0);
        for _ in 0..4 {
            w.u32(0);
        }
    }
}

/// OfferCommand (93): a shop's offers, or (keep_shop true) only the buyback items.
/// string shop, bit keep_shop (+80), bit can_sell (+81), u32-counted Ptr<Offer> (a
/// presence bit each), u32-counted offer groups (always empty, layout UNKNOWN),
/// u32-counted u16 category codes (the tabs), u32-counted item records (0x980AFC).
/// EVIDENCE: reader 0x9A9172 / writer 0x9ACEF3; HandleOfferCommand 0x579949
///   (dsor/trading.py _encode_shop, encode_buyback).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Offer {
    pub shop: String,
    /// True: keep the current shop and only replace its buyback list.
    pub keep_shop: bool,
    /// _Template_Shop.PlayerCanSell (HYPOTHESIS in the server).
    pub can_sell: bool,
    /// None for a null Ptr<Offer> (presence bit clear).
    pub offers: Vec<Option<ShopOffer>>,
    pub categories: Vec<u16>,
    /// The buyback list.
    pub items: Vec<ItemInfo>,
}

impl Body for Offer {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let shop = r.string()?;
        let keep_shop = r.bit()?;
        let can_sell = r.bit()?;
        let offers = counted(r, MOST, "offer count", |r| {
            if r.bit()? {
                Ok(Some(ShopOffer::decode(r)?))
            } else {
                Ok(None)
            }
        })?;
        empty_list(r, "offer groups")?;
        let categories = counted(r, MOST, "offer category count", |r| r.u16())?;
        let items = counted(r, MOST, "buyback item count", ItemInfo::decode)?;
        Ok(Self { shop, keep_shop, can_sell, offers, categories, items })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.shop);
        w.bit(self.keep_shop);
        w.bit(self.can_sell);
        w.count(self.offers.len());
        for o in &self.offers {
            w.bit(o.is_some());
            if let Some(o) = o {
                o.encode(w);
            }
        }
        w.u32(0);
        w.count(self.categories.len());
        for &c in &self.categories {
            w.u16(c);
        }
        w.count(self.items.len());
        for item in &self.items {
            item.encode(w);
        }
    }
}

/// TransactionCommand (94), client -> server: buy or sell in the current shop.
/// string shop, u8 kind, then the offer id (kinds 1 buy, 2 buy group, 3 buy without
/// shop) or the item's u32 (4 sell, 5 restore), then a u32 amount for kind 3.
/// EVIDENCE: reader 0xA26C19 / writer 0xA26D7C (dsor/trading.py decode_transaction).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Transaction {
    pub shop: String,
    pub kind: u8,
    /// Kinds 1..3.
    pub offer: String,
    /// Kinds 4..5.
    pub item: u32,
    /// Kind 3 only.
    pub amount: u32,
}

impl Transaction {
    pub const BUY_OFFER: u8 = 1;
    pub const BUY_OFFER_GROUP: u8 = 2;
    pub const BUY_WITHOUT_SHOP: u8 = 3;
    pub const SELL_ITEM: u8 = 4;
    pub const RESTORE_ITEM: u8 = 5;
}

impl Body for Transaction {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let shop = r.string()?;
        let kind = r.u8()?;
        let mut t = Self { shop, kind, ..Default::default() };
        match kind {
            1..=3 => t.offer = r.string()?,
            4 | 5 => t.item = r.u32()?,
            _ => return Err(DecodeError::Invalid { what: "transaction kind", value: kind as i64 }),
        }
        if kind == Self::BUY_WITHOUT_SHOP {
            t.amount = r.u32()?;
        }
        Ok(t)
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.shop);
        w.u8(self.kind);
        match self.kind {
            1..=3 => w.string(&self.offer),
            _ => w.u32(self.item),
        }
        if self.kind == Self::BUY_WITHOUT_SHOP {
            w.u32(self.amount);
        }
    }
}

/// CurrencyConversionCommand (90), client -> server: buy this much item-upgrade
/// currency with andermant. One i32.
/// EVIDENCE: reader 0x9A6CCA / writer 0x9AAF80 (dsor/trading.py decode_conversion).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CurrencyConversion {
    pub amount: i32,
}

impl Body for CurrencyConversion {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { amount: r.i32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.i32(self.amount)
    }
}

/// UseTravelItemCommand (130), client -> server: u32 item, then the map config id.
/// EVIDENCE: writer 0x9ADE74 / reader 0x9AA430 (dsor/travel.py decode_use_travel_item).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UseTravelItem {
    pub item: u32,
    pub map: String,
}

impl Body for UseTravelItem {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { item: r.u32()?, map: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.item);
        w.string(&self.map);
    }
}

/// UseStickerBookItemCommand (313), client -> server: use an item by template name
/// (a mount, among others). One string.
/// SOURCE: dsor/usable.py name_in; server.py USE_ITEM_OPCODE.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UseStickerBookItem {
    pub template: String,
}

impl Body for UseStickerBookItem {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { template: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.template)
    }
}

// ── talents ─────────────────────────────────────────────────────────────────

/// Talents::SelectTalentCommand (286), client -> server: the talent id (StringAtom).
/// EVIDENCE: writer 0xA3CAA0 (dsor/talents.py talent_id_in).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelectTalent {
    pub talent: String,
}

impl Body for SelectTalent {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { talent: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.talent)
    }
}

/// Talents::DeselectTalentCommand (287), client -> server: the talent id.
/// EVIDENCE: writer 0xA3C9B9 (dsor/talents.py talent_id_in).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeselectTalent {
    pub talent: String,
}

impl Body for DeselectTalent {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { talent: r.string()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.talent)
    }
}

/// TalentSelectionInfoCommand (290): the selected talents of one category.
/// Only context 3 (the Skill category: u8 preset, u8 count, each u8 rank + talent id)
/// is established; any other context is refused rather than guessed.
/// EVIDENCE: Encode 0xA3BEBA / reader 0xA3BA3E, entry sub_A64581 / sub_A64548;
///   handler 0x602F3F (dsor/talents.py encode_skill_talents).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TalentSelectionInfo {
    pub context: u8,
    pub preset: u8,
    /// (rank, talent id).
    pub talents: Vec<(u8, String)>,
}

impl TalentSelectionInfo {
    pub const SKILL_CONTEXT: u8 = 3;
}

impl Body for TalentSelectionInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let context = r.u8()?;
        if context != Self::SKILL_CONTEXT {
            return Err(DecodeError::Invalid { what: "talent selection context (only 3 known)", value: context as i64 });
        }
        let preset = r.u8()?;
        let n = byte_count(r)?;
        Ok(Self { context, preset, talents: list(r, n, |r| Ok((r.u8()?, r.string()?)))? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.context);
        w.u8(self.preset);
        w.u8(self.talents.len() as u8);
        for (rank, id) in &self.talents {
            w.u8(*rank);
            w.string(id);
        }
    }
}

// ── OCP shop (character service) ────────────────────────────────────────────

/// One OCPOfferResponseCommand entry (reader sub_A5BD02 / writer sub_A5BE04).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OcpOffer {
    pub unknown_i32_0: i32,
    pub unknown_f32_1: f32,
    pub unknown_string_2: String,
    pub unknown_string_3: String,
    pub unknown_string_4: String,
    /// "YYYY-MM-DD hh:mm:ss" or empty (parsed by sub_C5D980).
    pub expires: String,
}

/// OCPOfferResponseCommand (275): an i32-counted list of OcpOffer.
/// EVIDENCE: reader sub_998254, count bound sub_999A12 (dsor/shop.py).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OcpOfferResponse {
    pub offers: Vec<OcpOffer>,
}

impl Body for OcpOfferResponse {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            offers: counted(r, MOST, "OCP offer count", |r| {
                Ok(OcpOffer {
                    unknown_i32_0: r.i32()?,
                    unknown_f32_1: r.f32()?,
                    unknown_string_2: r.string()?,
                    unknown_string_3: r.string()?,
                    unknown_string_4: r.string()?,
                    expires: r.string()?,
                })
            })?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.offers.len());
        for o in &self.offers {
            w.i32(o.unknown_i32_0);
            w.f32(o.unknown_f32_1);
            w.string(&o.unknown_string_2);
            w.string(&o.unknown_string_3);
            w.string(&o.unknown_string_4);
            w.string(&o.expires);
        }
    }
}

/// One OCPMethodsCommand entry (reader sub_A5BCB0 / writer sub_A5BDAC).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OcpMethod {
    pub unknown_string_0: String,
    pub unknown_string_1: String,
    pub unknown_f32_2: f32,
    pub unknown_f32_3: f32,
}

/// OCPMethodsCommand (273): an i32-counted list of OcpMethod.
/// EVIDENCE: reader sub_997EC4, count bound sub_999970 (dsor/shop.py).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OcpMethods {
    pub methods: Vec<OcpMethod>,
}

impl Body for OcpMethods {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            methods: counted(r, MOST, "OCP method count", |r| {
                Ok(OcpMethod {
                    unknown_string_0: r.string()?,
                    unknown_string_1: r.string()?,
                    unknown_f32_2: r.f32()?,
                    unknown_f32_3: r.f32()?,
                })
            })?,
        })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.methods.len());
        for m in &self.methods {
            w.string(&m.unknown_string_0);
            w.string(&m.unknown_string_1);
            w.f32(m.unknown_f32_2);
            w.f32(m.unknown_f32_3);
        }
    }
}
