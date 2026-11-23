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

// ── the item record ─────────────────────────────────────────────────────────

/// One enchantment line of an item.
/// SOURCE: dsor/inventory.py Statistic / _write_item.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemStatistic {
    pub value: f32,
    /// The enchantment's level (stored at the entry's +4 by 0x980AFC); at least 1.
    pub kind: i8,
    /// Number of items in this stack (production amount).
    pub third: u32,
    pub fourth: u32,
    /// The attribute's name.
    pub name: String,
}

/// Game::ItemInfo, the item record read by 0x980AFC. Carried by InventoryInfo (87),
/// Offer (93, the buyback list) and NewItemCommand (51).
/// SOURCE: dso-server-production protocol/inventory_codec.py:149-193 (writer),
///   :195-264 (reader). Against 2018: no u32 after fifty_second nor after tier, a
///   bool after fourth_name, no third u8-counted string list. Field names are the
///   2018 server's ordinals where meaning is not established.
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
    /// Item level in release261 (production item_level).
    pub durability: u32,
    /// Stack sale value in release261 (production field_30).
    pub level: u32,
    pub fifty_second: u32,
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
    /// The equipped transmog skin id (production field_f8).
    pub fourth_name: String,
    /// production field_3e (True by default; meaning UNKNOWN).
    pub flag_2: bool,
    pub statistics: Vec<ItemStatistic>,
    pub names: Vec<String>,
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
        // release261: the 2018 reader's range check is not established for this
        // client (dso-server-production does not enforce it); not enforced here.
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
            flag_0: r.bit()?,
            flag_1: r.bit()?,
            template: r.string()?,
            second_name: r.string()?,
            position: r.vec3()?,
            appearance: r.string()?,
            tier: r.u32()?,
            fourth_name: r.string()?,
            flag_2: r.bit()?,
            ..Default::default()
        };
        let n = byte_count(r)?;
        item.statistics = list(r, n, |r| {
            Ok(ItemStatistic { value: r.f32()?, kind: r.i8()?, third: r.u32()?, fourth: r.u32()?, name: r.string()? })
        })?;
        item.names = strings_u8(r)?;
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
        w.bit(self.flag_0);
        w.bit(self.flag_1);
        w.string(&self.template);
        w.string(&self.second_name);
        w.vec3(self.position);
        w.string(&self.appearance);
        w.u32(self.tier);
        w.string(&self.fourth_name);
        w.bit(self.flag_2);
        w.u8(self.statistics.len() as u8);
        for s in &self.statistics {
            w.f32(s.value);
            w.i8(s.kind);
            w.u32(s.third);
            w.u32(s.fourth);
            w.string(&s.name);
        }
        write_strings_u8(w, &self.names);
        for &v in &self.stamped {
            w.u32(v);
        }
    }
}

// ── 87 InventoryInfo ────────────────────────────────────────────────────────

/// InventoryInfoCommand (release261: 84): the whole inventory.
/// Twelve collections, then six u32, three f32 (release261_219+) and one bit.
/// SOURCE (release261): dso-server-production protocol/inventory_codec.py:437-524
///   (three collections after `removed`: a u32 map, a u32 array, a string array).
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
    /// production field_158 (u32 map), field_180 (u32 array), field_1a0 (strings);
    /// meanings UNKNOWN.
    pub map_158: Vec<(u32, u32)>,
    pub words_180: Vec<u32>,
    pub strings_1a0: Vec<String>,
    /// Scalars +204..+224: 2 is the bag's cell count (<= 224), 3 bank, 4 extra bank,
    /// 5 extra bag (dsor/inventory.py load_inventory); 0 and 1 unnamed.
    pub storage_sizes: [u32; 6],
    pub health: f32,
    pub resource: f32,
    /// release261_219+: the tertiary resource (+0x1e0), ahead of the final bit.
    /// SOURCE: dso-server-production protocol/inventory_codec.py
    ///   write_inventory_info_body (b2f44c2).
    pub tertiary: f32,
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
            map_158: pair_list(r, "inventory +0x158")?,
            words_180: u32_list(r, "inventory +0x180")?,
            strings_1a0: counted(r, MOST, "inventory +0x1a0", |r| r.string())?,
            ..Default::default()
        };
        for v in got.storage_sizes.iter_mut() {
            *v = r.u32()?;
        }
        got.health = r.f32()?;
        got.resource = r.f32()?;
        got.tertiary = r.f32()?;
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
        write_pair_list(w, &self.map_158);
        write_u32_list(w, &self.words_180);
        w.count(self.strings_1a0.len());
        for t in &self.strings_1a0 {
            w.string(t);
        }
        for &v in &self.storage_sizes {
            w.u32(v);
        }
        w.f32(self.health);
        w.f32(self.resource);
        w.f32(self.tertiary);
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
        // release261: the 2018 reader's range check is not established for this
        // client (dso-server-production does not enforce it); not enforced here.
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

/// QuickSlotsInfoCommand (0x50 in release261): every quick bar, each a counted
/// list of slots. The release261 server stores 17 slots per bar.
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

/// QuickSlotsCommand (0x51 in release261), client -> server: one complete
/// 17-slot bar, then its u32 bar index (0..9).
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

/// RequestOfferCommand (89), client -> server: open a shop, by NPC (by_name false,
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

/// One release 261 Offer. SOURCE: protocol/shop_protocol.py _write_shop_offer,
/// including nonempty discount, privilege, quest and conversion metadata.
#[derive(Debug, Clone, PartialEq, Default)]
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
    pub once_per_character: bool,
    pub event_timed: bool,
    pub hide_not_suitable: bool,
    /// _Template_Offer.DirectlyBuyOffer (release261_192's ninth flag).
    pub directly_buy: bool,
    /// Release 261 ItemRarity (Common through Mythic).
    pub rarity: u8,
    /// Release 261 OfferCategory (0..48).
    pub category: u16,
    pub icon: String,
    pub icon_config: String,
    /// The item template sold.
    pub item: String,
    pub discount_events: Vec<(String, f32)>,
    pub privilege_discounts: Vec<(String, f32)>,
    pub disabled_events: Vec<String>,
    pub completed_quests: Vec<String>,
    pub conversion_stories: Vec<(String, f32)>,
    pub veteran_discounts: Vec<(String, f32)>,
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
        // Release 261 protocol/shop_protocol.py _write_shop_offer: nine flags
        // (DirectlyBuyOffer last, since 2026-10-21), icon configuration before the
        // item, six lists, then timed_offer.
        o.is_recommended = r.bit()?;
        o.dynamic_price = r.bit()?;
        o.is_new = r.bit()?;
        o.auto_identify = r.bit()?;
        o.once_per_character = r.bit()?;
        o.scale_to_player_level = r.bit()?;
        o.hide_not_suitable = r.bit()?;
        o.event_timed = r.bit()?;
        o.directly_buy = r.bit()?;
        o.rarity = r.u8()?;
        if o.rarity >= 8 {
            return Err(DecodeError::Invalid { what: "offer rarity", value: o.rarity as i64 });
        }
        o.category = r.u16()?;
        if o.category >= 48 {
            return Err(DecodeError::Invalid { what: "offer category", value: o.category as i64 });
        }
        o.icon = r.string()?;
        o.icon_config = r.string()?;
        o.item = r.string()?;
        o.discount_events = offer_modifiers(r)?;
        o.privilege_discounts = offer_modifiers(r)?;
        o.disabled_events = counted(r, MOST, "offer disabled events", |r| r.string())?;
        o.completed_quests = counted(r, MOST, "offer quests", |r| r.string())?;
        o.conversion_stories = offer_modifiers(r)?;
        o.veteran_discounts = offer_modifiers(r)?;
        o.timed_offer = r.bit()?;
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
            self.is_recommended,
            self.dynamic_price,
            self.is_new,
            self.auto_identify,
            self.once_per_character,
            self.scale_to_player_level,
            self.hide_not_suitable,
            self.event_timed,
            self.directly_buy,
        ] {
            w.bit(b);
        }
        w.u8(self.rarity);
        w.u16(self.category);
        w.string(&self.icon);
        w.string(&self.icon_config);
        w.string(&self.item);
        write_offer_modifiers(w, &self.discount_events);
        write_offer_modifiers(w, &self.privilege_discounts);
        for strings in [&self.disabled_events, &self.completed_quests] {
            w.count(strings.len());
            for s in strings { w.string(s); }
        }
        write_offer_modifiers(w, &self.conversion_stories);
        write_offer_modifiers(w, &self.veteran_discounts);
        w.bit(self.timed_offer);
    }
}

fn offer_modifiers(r: &mut BitReader<'_>) -> Result<Vec<(String, f32)>, DecodeError> {
    counted(r, MOST, "offer modifiers", |r| Ok((r.string()?, r.f32()?)))
}
fn write_offer_modifiers(w: &mut BitWriter, values: &[(String, f32)]) {
    w.count(values.len());
    for (name, value) in values { w.string(name); w.f32(*value); }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentItem { pub id: String, pub payment_item: u32 }

/// OfferCommand (90): a shop's offers, or (keep_shop true) only its buyback list.
/// Release 261 protocol/shop_protocol.py write_offer_command_body: nullable
/// offers, nullable payment items, u16 category codes, then sold item records.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Offer {
    pub shop: String,
    /// True: keep the current shop and only replace its buyback list.
    pub keep_shop: bool,
    /// _Template_Shop.PlayerCanSell.
    pub can_sell: bool,
    /// None for a null Ptr<Offer> (presence bit clear).
    pub offers: Vec<Option<ShopOffer>>,
    pub payment_items: Vec<Option<PaymentItem>>,
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
        let payment_items = counted(r, MOST, "shop payment items", |r| {
            Ok(if r.bit()? { Some(PaymentItem { id: r.string()?, payment_item: r.u32()? }) } else { None })
        })?;
        let categories = counted(r, MOST, "offer category count", |r| r.u16())?;
        let items = counted(r, MOST, "buyback item count", ItemInfo::decode)?;
        Ok(Self { shop, keep_shop, can_sell, offers, payment_items, categories, items })
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
        w.count(self.payment_items.len());
        for item in &self.payment_items {
            w.bit(item.is_some());
            if let Some(item) = item { w.string(&item.id); w.u32(item.payment_item); }
        }
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

/// One line of a TransactionCommand: an offer bought (`offer` set, `amount` copies),
/// or an item sold (`offer` empty, `item`, `amount` 4) or bought back (`amount` 5).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransactionEntry {
    pub offer: String,
    /// The item sold or bought back (production field_58).
    pub item: u32,
    /// Production field_5c, always 0.
    pub extra: u32,
    /// Copies bought (1..1000), or the item operation (4 sell, 5 restore).
    pub amount: u32,
}

/// TransactionCommand (91), client -> server: buy, sell or buy back in the open shop.
/// u32-counted entries, each `bool present` then a TransactionEntry (string offer,
/// u32, u32, u32); string shop (must be the shop the server opened); bool mode
/// (the server refuses true).
/// SOURCE: dso-server-production protocol/commerce_protocol.py decode_transaction
///   (:93-131) and services/commerce.py _handle_transaction: operation 4 (sale) and
///   5 (restore) sit in the final u32 with an empty offer id; a purchase names the
///   offer's resource id and the copies wanted. The 2018 layout (string shop, u8
///   kind, offer or item) is gone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Transaction {
    pub entries: Vec<Option<TransactionEntry>>,
    pub shop: String,
    pub mode: bool,
}

impl Transaction {
    pub const SELL_ITEM: u32 = 4;
    pub const RESTORE_ITEM: u32 = 5;

    fn one(shop: &str, entry: TransactionEntry) -> Self {
        Self { entries: vec![Some(entry)], shop: shop.to_owned(), mode: false }
    }
    /// Buy `amount` copies of the offer with that resource id.
    pub fn buy(shop: &str, offer: &str, amount: u32) -> Self {
        Self::one(shop, TransactionEntry { offer: offer.to_owned(), item: 0, extra: 0, amount })
    }
    /// Sell a stack from the backpack or the extra bag.
    pub fn sell(shop: &str, item: u32) -> Self {
        Self::one(shop, TransactionEntry { offer: String::new(), item, extra: 0, amount: Self::SELL_ITEM })
    }
    /// Buy back a stack sold to this shop (its id as the Offer's buyback list has it).
    pub fn restore(shop: &str, item: u32) -> Self {
        Self::one(shop, TransactionEntry { offer: String::new(), item, extra: 0, amount: Self::RESTORE_ITEM })
    }
}

impl Body for Transaction {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let entries = counted(r, MOST, "transaction entries", |r| {
            Ok(if r.bit()? {
                Some(TransactionEntry { offer: r.string()?, item: r.u32()?, extra: r.u32()?, amount: r.u32()? })
            } else {
                None
            })
        })?;
        Ok(Self { entries, shop: r.string()?, mode: r.bit()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.count(self.entries.len());
        for entry in &self.entries {
            w.bit(entry.is_some());
            if let Some(e) = entry {
                w.string(&e.offer);
                w.u32(e.item);
                w.u32(e.extra);
                w.u32(e.amount);
            }
        }
        w.string(&self.shop);
        w.bit(self.mode);
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

/// Talents::SelectTalentCommand (279), client -> server: the talent id and how
/// many ranks to add (1 per click; 1..255). The server checks the class, the
/// unlock level, the rank cap, the preset's category and the points left, and
/// answers with the effects' StatusEffects and a TalentSelectionInfo (a snapshot
/// with TalentCategoryPointsInfo on refusal).
/// SOURCE: dso-server-production protocol/talent_protocol.py:128-155, services/
///   talents.py:54-377. The 2018 writer (0xA3CAA0) sent the id alone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelectTalent {
    pub talent: String,
    pub ranks: u32,
}

impl Body for SelectTalent {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { talent: r.string()?, ranks: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.talent);
        w.u32(self.ranks);
    }
}

/// Talents::EnableTalentPreselectionCommand (281), client -> server: switch a
/// category (0 skill, 1 wisdom, 2 group, 3 fame) to one of its presets (skill 0..7,
/// wisdom and group 0..3, fame 0). u8 preset, then u8 category.
/// SOURCE: dso-server-production protocol/talent_protocol.py:198-210.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TalentPreselection {
    pub preselection: u8,
    pub category: u8,
}

impl Body for TalentPreselection {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { preselection: r.u8()?, category: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.preselection);
        w.u8(self.category);
    }
}

/// Talents::ResetTalentsCommand (286), client -> server: clear the active preset
/// of a category. u8 category, then u8 preset (the order of 281 reversed).
/// SOURCE: dso-server-production protocol/talent_protocol.py:238-250.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResetTalents {
    pub category: u8,
    pub preselection: u8,
}

impl Body for ResetTalents {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { category: r.u8()?, preselection: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.category);
        w.u8(self.preselection);
    }
}

/// Talents::ResetTalentCategoryCommand (287), client -> server: clear one branch
/// of the wisdom or group category's active preset. u8 category, str branch
/// resource id, u8 preset.
/// SOURCE: dso-server-production protocol/talent_protocol.py:278-316.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResetTalentCategory {
    pub category: u8,
    pub branch: String,
    pub preselection: u8,
}

impl Body for ResetTalentCategory {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { category: r.u8()?, branch: r.string()?, preselection: r.u8()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.category);
        w.string(&self.branch);
        w.u8(self.preselection);
    }
}

/// Talents::UnlockSkillTalentPreselectionCommand (284), client -> server: buy the
/// next Skill tree preset (UnlockPreselectionCost), u32 the presets bought then
/// (those bought so far + 1). DSOR_Server does not handle it (it always sends
/// UnlockSkillTalentPreselectionInfo 0).
/// EVIDENCE: release261 SkillTalentsWidget "SkillPresetLock" (data/diagnostics
///   skilltalents-widget.annotated.txt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnlockSkillTalentPreselection {
    pub unlocked: u32,
}

impl Body for UnlockSkillTalentPreselection {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { unlocked: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unlocked);
    }
}

/// Talents::TalentCategoryPointsInfoCommand (282), actor-scoped: the progression
/// behind the talent points. u8 category (0xFF: all of them), then four u32 in the
/// wire order skill, fame, wisdom, group.
/// SOURCE: dso-server-production protocol/player_init_protocol.py:865-872 (the
///   wire order TALENT_CATEGORY_POINTS_WIRE_ORDER = 0, 3, 1, 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TalentCategoryPointsInfo {
    pub category: u8,
    pub points: [u32; 4],
}

impl TalentCategoryPointsInfo {
    pub const ALL: u8 = 0xFF;
    /// The value for a category (0 skill, 1 wisdom, 2 group, 3 fame).
    pub fn of(&self, category: u8) -> u32 {
        let slot = match category { 0 => 0, 3 => 1, 1 => 2, _ => 3 };
        self.points[slot]
    }
}

impl Body for TalentCategoryPointsInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let category = r.u8()?;
        let mut points = [0; 4];
        for p in &mut points {
            *p = r.u32()?;
        }
        Ok(Self { category, points })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.category);
        for p in self.points {
            w.u32(p);
        }
    }
}

/// Talents::UnlockSkillTalentPreselectionInfoCommand (285), actor-scoped: how
/// many skill presets are unlocked. One u32.
/// SOURCE: dso-server-production protocol/player_init_protocol.py:1193-1196.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnlockSkillTalentPreselectionInfo {
    pub unlocked: u32,
}

impl Body for UnlockSkillTalentPreselectionInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { unlocked: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.unlocked);
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

/// One selected talent: its rank (u16), its bonus ranks (u16: equipment's
/// TalentLevelBonus; the client keeps its own and ignores this), its resource id.
/// EVIDENCE: release261 0x140a25254 (diagnostics talents-manager.annotated.txt).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TalentEntry {
    pub rank: u16,
    pub bonus: u16,
    pub id: String,
}

/// One tree of a selection: its active preset, the talents picked in it, and
/// (Knowledge, Group) each branch's ranks together, as {ranks, 0, branch id}.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TalentCategory {
    pub preset: u8,
    pub picks: Vec<TalentEntry>,
    pub branches: Vec<TalentEntry>,
}

/// TalentSelectionInfoCommand (release261: 283): the talents picked in the
/// trees' active presets. u8 context: 0 every tree (Skill, Knowledge, Group,
/// Fame, each u8 preset, u8-counted picks, u8-counted branch counters); 1 to 4
/// one tree (1 Fame, 2 Knowledge, 3 Skill, 4 Group: u8 preset, u8-counted picks,
/// and for Knowledge and Group u8-counted branch counters). The lists replace
/// the tree's and the preset becomes its active one.
/// SOURCE: dso-server-production protocol/player_init_protocol.py:875-950 (the
///   server sends contexts 0 and 3); release261 decode 0x140a25254 (contexts 1,
///   2 and 4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TalentSelectionInfo {
    pub context: u8,
    /// Contexts 1 to 4: the tree's preset, picks and branch counters.
    pub preset: u8,
    pub picks: Vec<TalentEntry>,
    pub branches: Vec<TalentEntry>,
    /// Context 0: the four trees.
    pub categories: Vec<TalentCategory>,
}

impl TalentSelectionInfo {
    pub const SKILL_CONTEXT: u8 = 3;
    pub const ALL_CONTEXT: u8 = 0;

    /// The tree (0 Skill, 1 Knowledge, 2 Group, 3 Fame) a one-tree context is.
    pub fn tree(context: u8) -> Option<u8> {
        match context { 1 => Some(3), 2 => Some(1), 3 => Some(0), 4 => Some(2), _ => None }
    }
}

fn talent_entries(r: &mut BitReader<'_>) -> Result<Vec<TalentEntry>, DecodeError> {
    let n = byte_count(r)?;
    list(r, n, |r| Ok(TalentEntry { rank: r.u16()?, bonus: r.u16()?, id: r.string()? }))
}

fn write_talent_entries(w: &mut BitWriter, entries: &[TalentEntry]) {
    w.u8(entries.len() as u8);
    for e in entries {
        w.u16(e.rank);
        w.u16(e.bonus);
        w.string(&e.id);
    }
}

impl Body for TalentSelectionInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let context = r.u8()?;
        match (context, Self::tree(context)) {
            (Self::ALL_CONTEXT, _) => {
                let mut categories = Vec::with_capacity(4);
                for _ in 0..4 {
                    let preset = r.u8()?;
                    let picks = talent_entries(r)?;
                    categories.push(TalentCategory { preset, picks, branches: talent_entries(r)? });
                }
                Ok(Self { context, categories, ..Default::default() })
            }
            (_, Some(tree)) => {
                let preset = r.u8()?;
                let picks = talent_entries(r)?;
                let branches = if matches!(tree, 1 | 2) { talent_entries(r)? } else { Vec::new() };
                Ok(Self { context, preset, picks, branches, categories: Vec::new() })
            }
            (other, None) => Err(DecodeError::Invalid { what: "talent selection context (0 to 4)", value: other as i64 }),
        }
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u8(self.context);
        if self.context == Self::ALL_CONTEXT {
            for i in 0..4 {
                let c = self.categories.get(i).cloned().unwrap_or_default();
                w.u8(c.preset);
                write_talent_entries(w, &c.picks);
                write_talent_entries(w, &c.branches);
            }
        } else {
            w.u8(self.preset);
            write_talent_entries(w, &self.picks);
            if matches!(Self::tree(self.context), Some(1 | 2)) { write_talent_entries(w, &self.branches); }
        }
    }
}

// ── OCP shop (character service) ────────────────────────────────────────────

/// One OCPOfferResponseCommand entry (reader sub_A5BD02 / writer sub_A5BE04).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OcpOffer {
    pub unknown_i32_0: i32,
    pub unknown_f32_1: f32,
    /// Release 261: a bool after the price (production `featured`; the 2018
    /// reader had none). SOURCE: protocol/gameplay_protocol.py:2726-2736.
    pub featured: bool,
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
                    featured: r.bit()?,
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
            w.bit(o.featured);
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

// ── 82, 130, 310, 314: the rest of a purchase's reply ────────────────────────

/// QuickbarInfoCommand (82), actor-scoped: how many quick bars the player has and
/// how many are still locked. Sent after a purchase (services/commerce.py).
/// SOURCE: dso-server-production protocol/player_init_protocol.py:449-451, writer
///   :1182-1184.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QuickbarInfo {
    pub quickbars: u32,
    pub locked: u32,
}

impl Body for QuickbarInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { quickbars: r.u32()?, locked: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.quickbars);
        w.u32(self.locked);
    }
}

/// One timed shop offer: its id, two u32 and a six-part end time (y, m, d, h, min, s).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimedOffer {
    pub offer: String,
    pub unknown_u32_0: u32,
    pub unknown_u32_1: u32,
    pub ends_at: [u32; 6],
}

/// TimedOffersInfoCommand (130), actor-scoped: the server time and the timed offers
/// still running. Precedes every Offer and a timed purchase's reply.
/// SOURCE: dso-server-production protocol/player_init_protocol.py:1532-1542 and
///   protocol/shop_protocol.py encode_offer_response_batch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimedOffersInfo {
    pub server_time: u64,
    pub offers: Vec<TimedOffer>,
}

impl Body for TimedOffersInfo {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        let server_time = r.u64()?;
        let offers = counted(r, MOST, "timed offers", |r| {
            let (offer, unknown_u32_0, unknown_u32_1) = (r.string()?, r.u32()?, r.u32()?);
            let mut ends_at = [0; 6];
            for v in &mut ends_at {
                *v = r.u32()?;
            }
            Ok(TimedOffer { offer, unknown_u32_0, unknown_u32_1, ends_at })
        })?;
        Ok(Self { server_time, offers })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u64(self.server_time);
        w.count(self.offers.len());
        for o in &self.offers {
            w.string(&o.offer);
            w.u32(o.unknown_u32_0);
            w.u32(o.unknown_u32_1);
            for v in o.ends_at {
                w.u32(v);
            }
        }
    }
}

/// GameCurrencyChangedCommand (310), actor-scoped: one keyed currency's new balance
/// (materi fragments, event coins, stickerbook upgrade currency...), by its resource
/// id. The ordinary wallet comes as CurrencyChanged (128).
/// SOURCE: dso-server-production protocol/currency_protocol.py:173-187.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GameCurrencyChanged {
    pub currency: String,
    pub amount: u64,
}

impl Body for GameCurrencyChanged {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { currency: r.string()?, amount: r.u64()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.string(&self.currency);
        w.u64(self.amount);
    }
}

/// UpdateProgressTierSpenderChallengesCommand (314), actor-scoped: andermant spent,
/// counted towards the tiered spender rewards. One u32.
/// SOURCE: dso-server-production protocol/player_init_protocol.py:1545-1555.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TierSpenderProgress {
    pub amount: u32,
}

impl Body for TierSpenderProgress {
    fn decode(r: &mut BitReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self { amount: r.u32()? })
    }
    fn encode(&self, w: &mut BitWriter) {
        w.u32(self.amount);
    }
}
