//! The data behind the systems windows (interface/systems.catalog.json,
//! tools/systems_catalog.py): the achievements and their categories, the events,
//! the daily challenges, the Wanted Thief's missions, the collector's album (its
//! books and stickers), the crafting recipes, the equipment skins and the socket
//! prices, each with its English text resolved. What the server says of them is
//! in crate::network::net; the windows are crate::ui::controls::systems.
use bevy::{asset::{io::Reader, AssetLoader, LoadContext}, prelude::*};
use serde::Deserialize;
use std::collections::HashMap;

fn one() -> u32 { 1 }

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct AchievementDef {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default = "one")]
    pub amount: u32,
    #[serde(default)]
    pub points: u32,
    #[serde(default)]
    pub event: String,
    /// Its icon (textures/<icon>.png).
    #[serde(default)]
    pub icon: String,
    /// The title the achievement unlocks (charactersheet's Titles tab), if any.
    #[serde(default)]
    pub title_text: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct AchievementCategory {
    pub id: String,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub icon: String,
    pub title: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct EventHint {
    pub text: String,
    /// The hint's artwork (an icon texture, without extension).
    #[serde(default)]
    pub icon: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct EventReward {
    /// The bar's page it is shown on.
    #[serde(default)]
    pub page: u32,
    /// The progress the reward is reached at.
    pub at: u32,
    pub item: String,
    #[serde(default = "one")]
    pub amount: u32,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct EventDef {
    pub title: String,
    /// Standard, StandardHidden, Silent, Sale or HighlightSale: which the event
    /// window lists (crate::ui::controls::systems::events::shown).
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub description: String,
    /// "map:entry" the event's Start button travels to.
    #[serde(default)]
    pub teleport: String,
    /// The tab icon.
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub hints: Vec<EventHint>,
    /// Item templates the event advertises.
    #[serde(default)]
    pub advertised: Vec<String>,
    /// The event's banner artwork.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub banner: String,
    /// The window's parts the event shows (ButtonTeleport, ButtonAchievements,
    /// ButtonShop, ButtonRealmoneyShop, EventShopLabel, ButtonLeaderboards...).
    #[serde(default)]
    pub elements: Vec<String>,
    /// The progress bar's rewards, by threshold.
    #[serde(default)]
    pub rewards: Vec<EventReward>,
    /// Its Andermant rewards (item empty).
    #[serde(default)]
    pub currency: Vec<EventReward>,
    /// The item its progress counts.
    #[serde(default)]
    pub collectible: String,
    /// Its offers (the Andermant item shop's ids, a premium product's key).
    #[serde(default)]
    pub offers: Vec<String>,
    #[serde(default)]
    pub achievement_category: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct ChallengeDef {
    pub description: String,
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub value: u32,
    #[serde(default)]
    pub reward: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tier: u32,
    #[serde(default)]
    pub difficulty: String,
    /// The hint shown under the objective.
    #[serde(default)]
    pub hint: String,
    /// The card's artwork.
    #[serde(default)]
    pub image: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct WantedDef {
    pub title: String,
    #[serde(default)]
    pub details: String,
    #[serde(default)]
    pub quest: String,
    #[serde(default)]
    pub rarity: String,
    /// The reward box's item template.
    #[serde(default)]
    pub reward: String,
    /// The card's artwork.
    #[serde(default)]
    pub image: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct StickerBook {
    pub id: String,
    /// The album's category (0 the front page); -1 for a category's own book.
    pub category: i32,
    #[serde(default)]
    pub subcategory: i32,
    #[serde(default)]
    pub priority: i32,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Its rewards by the stickers collected (`at`).
    #[serde(default)]
    pub rewards: Vec<EventReward>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct StickerDef {
    pub name: String,
    #[serde(default)]
    pub book: String,
    /// The item templates that fill the sticker (the first one's icon is shown).
    #[serde(default)]
    pub items: Vec<String>,
    #[serde(default)]
    pub upgrades: Vec<String>,
    #[serde(default)]
    pub priority: i32,
    /// Its row in the database: the album's order within a book and priority.
    #[serde(default)]
    pub order: u32,
    /// Its origin's icon and preview background (texture references).
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub background: String,
    /// The offer that sells it (the Andermant shop's).
    #[serde(default)]
    pub offer: String,
    /// New: the album's front page advertises it.
    #[serde(default)]
    pub new: bool,
    /// What the album's preview turns for each of its items (crate::ui::preview).
    /// Empty in catalogs exported before it: the preview shows the icon.
    #[serde(default)]
    pub looks: Vec<StickerLook>,
}

/// What a sticker's item shows once used (tools/systems_catalog.py): one of
/// its fields is set, or none (a decoration: an effect only).
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct StickerLook {
    pub item: String,
    /// The class it is for (CharClass: "warrior", "mage"...); empty: every class.
    #[serde(default)]
    pub class: String,
    /// The status effect it starts, by wire index.
    #[serde(default)]
    pub status: Option<u32>,
    /// A costume's skin part (SkinModifier).
    #[serde(default)]
    pub skin: String,
    /// An emote's animation state (StartAnimation).
    #[serde(default)]
    pub animation: String,
    /// A mount (_Template_Mount, characters/mounts.json).
    #[serde(default)]
    pub mount: String,
    /// A pet (_Template_PetMonster, characters/pets.json).
    #[serde(default)]
    pub pet: String,
    /// A Shadow Soul's monster (characters/monster_templates.json).
    #[serde(default)]
    pub monster: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct RecipeDef {
    pub crafted: String,
    #[serde(default)]
    pub ingredients: Vec<String>,
    #[serde(default)]
    pub price_vc: u32,
    #[serde(default)]
    pub price_rc: u32,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub level: u32,
    /// The recipe book that unlocks it (RecipeBookInfo lists the books known).
    #[serde(default)]
    pub book: String,
    /// Its RecipeItemReference: the item that unlocks it, what RecipeBookInfo
    /// lists (DSOR_Server player_init_world.py sends the character's resources);
    /// empty, it needs none.
    #[serde(default)]
    pub unlock: String,
}

impl SystemsCatalog {
    /// Whether a recipe book is known: named itself, or one of its recipes
    /// unlocked by an item the character has (or by none).
    /// FAILURE: books were matched against the unlock items' names only, so
    ///   every book showed locked ("faut que tu m'unlock les recettes").
    pub fn book_known(&self, book: &str, known: &[String]) -> bool {
        known.contains(&book.to_lowercase()) || self.recipe_books.get(book).is_some_and(|b| b.recipes.iter()
            .filter_map(|r| self.recipes.get(r)).any(|r| r.unlock.is_empty() || known.contains(&r.unlock.to_lowercase())))
    }
}

/// A workbench tab (WorkbenchTab) and its icon.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct WorkbenchTab {
    pub id: String,
    #[serde(default)]
    pub icon: String,
}

/// A node of a workbench tab's category tree.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct WorkbenchCategory {
    pub id: String,
    #[serde(default)]
    pub tab: String,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub priority: i32,
    pub title: String,
}

/// An ingredient slot's placeholder in a recipe book entry.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Placeholder {
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub rarity: String,
}

/// A recipe book entry (_Template_RecipeBook): what the workbench lists.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct RecipeBookDef {
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// How to unlock it, while locked.
    #[serde(default)]
    pub locked: String,
    /// Listed while locked.
    #[serde(default)]
    pub unlockable: bool,
    #[serde(default)]
    pub placeholders: Vec<Placeholder>,
    /// Its crafting recipes (_Template_CraftingRecipes ids).
    #[serde(default)]
    pub recipes: Vec<String>,
    /// Where a locked book comes from: its origin's icon (ui/icon_origin_*).
    #[serde(default)]
    pub locked_icon: String,
}

/// A node of a shop's category tree (_Template_ShopCategory).
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct ShopCategory {
    pub shop: String,
    pub id: String,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub priority: i32,
    /// The offers' categories it lists (OfferCategory).
    #[serde(default)]
    pub offers: Vec<String>,
    /// The premium shop's offer kinds it lists (TabCategory).
    #[serde(default)]
    pub tabs: Vec<String>,
    pub title: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct ShopBanner {
    pub texture: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub offer: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct SkinDef {
    #[serde(default)]
    pub title: String,
    /// The inventory equipment slots the skin fits (tools/inventory_catalog.py SLOTS).
    #[serde(default)]
    pub slots: Vec<i8>,
    #[serde(default)]
    pub classes: Vec<String>,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub price_rc: u32,
    #[serde(default)]
    pub price_vc: u32,
    /// Removing it: gold (scaled by level, as the apply price).
    #[serde(default)]
    pub remove_vc: u32,
    /// PvE or PvP equipment.
    #[serde(default)]
    pub kind: String,
    /// It carries an effect; the shop sells it (its offer); the item that shows it.
    #[serde(default)]
    pub effect: bool,
    #[serde(default)]
    pub offer: String,
    #[serde(default)]
    pub item: String,
    #[serde(default)]
    pub priority: i32,
}

/// A dye (_Template_EquipmentColor): the dye item that pays for it, its icon,
/// prices (Andermant when no dye item is carried), the removal's gold.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct DyeDef {
    #[serde(default)]
    pub item: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub price_rc: u32,
    #[serde(default)]
    pub price_vc: u32,
    #[serde(default)]
    pub remove_vc: u32,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub priority: i32,
}

#[derive(Asset, TypePath, Deserialize, Default)]
pub struct SystemsCatalog {
    #[serde(default)]
    pub achievements: HashMap<String, AchievementDef>,
    #[serde(default)]
    pub achievement_categories: Vec<AchievementCategory>,
    #[serde(default)]
    pub events: HashMap<String, EventDef>,
    #[serde(default)]
    pub challenges: HashMap<String, ChallengeDef>,
    /// By mission id (WantedThiefInfo's numbers).
    #[serde(default)]
    pub wanted: HashMap<String, WantedDef>,
    #[serde(default)]
    pub stickerbooks: Vec<StickerBook>,
    #[serde(default)]
    pub stickers: HashMap<String, StickerDef>,
    #[serde(default)]
    pub recipes: HashMap<String, RecipeDef>,
    #[serde(default)]
    pub recipe_books: HashMap<String, RecipeBookDef>,
    #[serde(default)]
    pub workbench_tabs: Vec<WorkbenchTab>,
    #[serde(default)]
    pub workbench_categories: Vec<WorkbenchCategory>,
    #[serde(default)]
    pub shop_categories: Vec<ShopCategory>,
    #[serde(default)]
    pub shop_banners: Vec<ShopBanner>,
    #[serde(default)]
    pub skins: HashMap<String, SkinDef>,
    /// "socket_<n>" -> rarity name -> Andermant (_Template_SocketExtensionCosts).
    #[serde(default)]
    pub socket_costs: HashMap<String, HashMap<String, u32>>,
    /// The PvP rank titles by honour level (0: no rank).
    #[serde(default)]
    pub pvp_ranks: Vec<String>,
    /// The matchmaking window's modes, by tab.
    #[serde(default)]
    pub pvp_modes: Vec<PvpMode>,
    /// The overall PvP board's rewards by place.
    #[serde(default)]
    pub pvp_rewards: Vec<PlaceReward>,
    /// Offer category codes (ShopOffer.category) by name.
    #[serde(default)]
    pub offer_categories: HashMap<String, u16>,
    /// _Globals' daily and Wanted figures: daily_reshuffle, wanted_base,
    /// wanted_vip, wanted_single_refresh, wanted_all_refresh, wanted_vip_buy.
    #[serde(default)]
    pub figures: HashMap<String, u32>,
    /// The album front page's banners (texture references).
    #[serde(default)]
    pub album_banners: Vec<String>,
    #[serde(default)]
    pub dyes: HashMap<String, DyeDef>,
    /// The gold prices' factor by level (index level - 1).
    #[serde(default)]
    pub level_costs: Vec<f32>,
    /// The currency bag's rows after the Glyphs of Power, in its order
    /// (crate::ui::controls::currency).
    #[serde(default)]
    pub currencies: Vec<CurrencyDef>,
}

/// A currency of the currency bag: its item (the keyed wallet's resource id)
/// and its icon (a texture reference).
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct CurrencyDef {
    pub id: String,
    #[serde(default)]
    pub icon: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct PvpMode {
    /// JoinMatchQueue's mode.
    pub mode: i8,
    pub title: String,
    #[serde(default)]
    pub artwork: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub reward: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct PlaceReward {
    pub first: u32,
    pub last: u32,
    #[serde(default)]
    pub items: Vec<String>,
}

/// A texture reference of the catalogue ("ui/x") as the asset path the
/// renderer loads; empty stays empty.
pub fn texture_path(reference: &str) -> String {
    let reference = reference.trim().trim_start_matches('/');
    if reference.is_empty() { String::new() } else { format!("textures/{reference}.png") }
}

impl SystemsCatalog {
    /// The Andermant the next socket (1-based) costs on an item of this rarity
    /// (ItemInfo.kind: 0 common .. 5 unique).
    /// A trinket's sockets cost the TrinketSocketCosts column, whatever its rarity.
    pub fn socket_price(&self, socket: usize, rarity: u8, trinket: bool) -> u32 {
        const RARITIES: [&str; 6] = ["Common", "Uncommon", "Magic", "Rare", "Epic", "Unique"];
        let name = if trinket { "TrinketSocketCosts" } else { RARITIES.get(rarity as usize).copied().unwrap_or("Unique") };
        self.socket_costs.get(&format!("socket_{socket}")).and_then(|c| c.get(name)).copied().unwrap_or(0)
    }
}

#[derive(Default, TypePath)]
pub struct SystemsCatalogLoader;
impl AssetLoader for SystemsCatalogLoader {
    type Asset = SystemsCatalog;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<SystemsCatalog, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] { &["systems.catalog.json"] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_reads_the_exported_shape_and_prices_sockets_by_rarity() {
        let catalog: SystemsCatalog = serde_json::from_str(r#"{
            "achievements": {"a": {"title": "Regicide", "category": "lore", "points": 10, "title_text": "Regicide"}},
            "achievement_categories": [{"id": "lore", "title": "Lore"}],
            "events": {"e": {"title": "Ghost Festival", "teleport": "a0200_kingscity:a0200_fairground"}},
            "stickers": {"s": {"name": "Cloak", "book": "b", "items": ["costume_cloak"]}},
            "skins": {"k": {"slots": [7], "classes": ["mage"], "price_rc": 199}},
            "socket_costs": {"socket_1": {"Common": 5, "Epic": 75}, "socket_2": {"Common": 10}}
        }"#).unwrap();
        assert_eq!(catalog.achievements["a"].amount, 1);
        assert_eq!(catalog.achievements["a"].title_text, "Regicide");
        assert_eq!(catalog.events["e"].teleport.split_once(':'), Some(("a0200_kingscity", "a0200_fairground")));
        assert_eq!(catalog.skins["k"].slots, [7]);
        assert_eq!((catalog.socket_price(1, 0, false), catalog.socket_price(1, 4, false), catalog.socket_price(2, 0, false), catalog.socket_price(3, 0, false)), (5, 75, 10, 0));
        assert!(catalog.recipes.is_empty() && catalog.wanted.is_empty());
    }
}
