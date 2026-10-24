//! Item cells share the authored window's hit regions, anchors and visibility.
//! Requests never invent item IDs or amounts: server snapshots drive the view.
use super::*;
use crate::gameplay::inventory::{InventoryState, Place, BAG_SLOTS, OVERFLOW_PAGE_SLOTS};
use dsor_proto::commands::inventory::ItemInfo;

#[derive(Deserialize, Clone)]
pub struct Definition {
    pub name: String, pub description: String, pub category: String, pub icon: String,
    pub max_stack: u32, pub required_level: u32, pub slots: Vec<i8>, pub class_name: String, pub rarity: String,
    #[serde(default)] pub icon_config: String,
    /// The raw _Template_Item ItemCategory ("Mount", "Armor"...).
    #[serde(default)] pub item_category: String,
    #[serde(default)] pub unique_values: Vec<String>,
    #[serde(default)] pub unique_effects: Vec<String>,
}
/// A rolled appearance's icon and icon configuration (_Template_ItemAppearance).
#[derive(Deserialize)]
pub struct Appearance { pub icon: String, pub config: String, #[serde(default)] pub name: String }
/// _Template_IconConfig: the background follows the item's rarity, or is fixed.
#[derive(Deserialize)]
pub struct IconConfig { pub rarity: bool, pub background: String, #[serde(default)] pub frame: String }
/// The rarities in ItemInfo.kind's order, as the catalog names them
/// (rarity_backgrounds follows it).
const RARITIES: [&str; 8] = ["Common", "Uncommon", "Magic", "Rare", "Epic", "Unique", "Set", "Mythic"];
#[derive(Deserialize)]
pub struct Ingredient { pub template: String, pub tab: String, pub rarity: usize, pub slot: usize, pub class_name: String, pub unlock: u32 }
#[derive(Deserialize)]
pub struct Enchantment { pub name: String }
#[derive(Asset, TypePath, Deserialize)]
pub struct Catalog { pub items: HashMap<String, Definition>, pub ingredients: Vec<Ingredient>, pub colors: HashMap<String, [f32;4]>,
    #[serde(default)] pub appearances: HashMap<String, Appearance>, #[serde(default)] pub icon_configs: HashMap<String, IconConfig>,
    #[serde(default)] pub rarity_backgrounds: Vec<String>,
    #[serde(default)] pub enchantments: HashMap<String, Enchantment>,
    #[serde(default)] pub values: values::Data }
impl Catalog {
    /// What a cell shows: the rolled appearance's icon (a generated item's look;
    /// its template's icon is the base model's, the same for every roll), else the
    /// template's; and the background its icon configuration gives it -- the
    /// rarity's (icons_toolkit/bg_rarity_*) when RarityControlled.
    /// SOURCE: tools/inventory_catalog.py (_Template_ItemAppearance, _Template_IconConfig).
    /// FAILURE (2026-10-16): every glove showed warrior_gloves_01a, with no rarity
    ///   background ("les icones des items sont tous les memes").
    pub fn icon(&self, template: &str, item: Option<&ItemInfo>) -> (Option<&str>, Option<&str>) {
        let (icon, background, _) = self.icon_parts(template, item);
        (icon, background)
    }
    /// `icon`, and the frame drawn over it (_Template_IconConfig.FrameTexture:
    /// icons_toolkit/frame_slim_basic, _unique, _set, _mythic).
    /// FAILURE (2026-10-16): without it the icon covered the whole cell, border
    ///   included ("les images sont pas bien dans les slots").
    /// What an item is called: its rolled appearance's name (all_amulet_02
    /// "Medallion"), else its template's. A generated piece's template name is
    /// the generic one ("Headband" for every hat).
    /// FAILURE (2026-10-20): "j'ai recupere un item qui n'est pas un headband mais
    ///   un autre chapeau et c'est ecrit headband meme si il a le bon skin".
    pub fn name<'a>(&'a self, template: &'a str, item: Option<&ItemInfo>) -> &'a str {
        item.and_then(|i| self.appearances.get(&i.appearance)).map(|a| a.name.as_str()).filter(|n| !n.is_empty())
            .or_else(|| self.items.get(template).map(|d| d.name.as_str()).filter(|n| !n.is_empty()))
            .unwrap_or(template)
    }
    /// A reward's icon (no carried item): its background the rarity the reward
    /// names when the icon configuration follows the rarity; an empty or unknown
    /// rarity keeps the template's own.
    /// EVIDENCE: release261 UI::QuestWidget2::CreateRewardElement (0x1404CF838)
    ///   takes the reward's rarity unless it is -1 or past the last one.
    pub fn reward_icon_parts(&self, template: &str, rarity: &str) -> (Option<&str>, Option<&str>, Option<&str>) {
        let (icon, background, frame) = self.icon_parts(template, None);
        let follows = self.items.get(template).and_then(|d| self.icon_configs.get(&d.icon_config)).is_some_and(|c| c.rarity);
        let ranked = RARITIES.iter().position(|r| r.eq_ignore_ascii_case(rarity)).filter(|_| follows)
            .and_then(|n| self.rarity_backgrounds.get(n)).map(String::as_str).filter(|b| !b.is_empty());
        (icon, ranked.or(background), frame)
    }
    pub fn icon_parts(&self, template: &str, item: Option<&ItemInfo>) -> (Option<&str>, Option<&str>, Option<&str>) {
        let def = self.items.get(template);
        let look = item.and_then(|i| self.appearances.get(if i.fourth_name.is_empty() { &i.appearance } else { &i.fourth_name }));
        let icon = look.map(|a| a.icon.as_str()).filter(|i| !i.is_empty()).or_else(|| def.map(|d| d.icon.as_str()).filter(|i| !i.is_empty()));
        let config = look.map(|a| a.config.as_str()).filter(|c| !c.is_empty()).or_else(|| def.map(|d| d.icon_config.as_str()));
        let config = config.and_then(|c| self.icon_configs.get(c));
        // Without an item (the album, shops, rewards) the template's own rarity.
        // FAILURE (2026-10-23): every pet sticker had the Common background.
        let background = config.and_then(|c| if c.rarity {
            let rarity = item.map(|i| i.kind as usize)
                .or_else(|| def.and_then(|d| RARITIES.iter().position(|r| *r == d.rarity))).unwrap_or(0);
            self.rarity_backgrounds.get(rarity).map(String::as_str)
        } else { Some(c.background.as_str()) }).filter(|b| !b.is_empty());
        let frame = config.map(|c| c.frame.as_str()).filter(|f| !f.is_empty());
        (icon, background, frame)
    }
}
#[derive(Default, TypePath)]
pub struct CatalogLoader;
impl AssetLoader for CatalogLoader {
    type Asset = Catalog; type Settings = (); type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<Catalog, Self::Error> {
        let mut b=Vec::new(); r.read_to_end(&mut b).await?; serde_json::from_slice(&b).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] { &["items.json"] }
}
#[derive(Resource)]
pub struct Data { pub(in crate::ui) catalog: Handle<Catalog>, pub(super) tooltip: Handle<Layout> }
pub fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Data { catalog: assets.load("interface/inventory.items.json"), tooltip: assets.load("interface/tooltip.ui.json") });
}

pub fn sync(net:Option<NonSend<crate::network::net::Net>>,mut windows:Query<&mut WindowControls>) {
    if let Some(net)=net {if net.inventory.ready {for mut c in &mut windows {
        c.bag_pages=(net.inventory.allocation.storage_sizes[2] as usize).div_ceil(BAG_SLOTS as usize).clamp(1,8);
        c.premium_bag=net.inventory.writable(Place::Extra(0));
        c.overflow_pages=net.inventory.allocation.overflow.len().div_ceil(OVERFLOW_PAGE_SLOTS as usize).max(1);
        c.overflow_page=c.overflow_page.min(c.overflow_pages-1);
        if c.bag==EXTRA && !c.premium_bag {c.bag=0;}
        if c.bag<EXTRA {c.bag=c.bag.min(c.bag_pages-1);}
    }}}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell { Bag(u32), Equipment(i8), Ingredient(usize) }
pub(super) fn cell(w: &Widget) -> Option<Cell> {
    if w.kind != "Slot" { return None; }
    let leaf=w.id.rsplit('/').next()?;
    if let Some(n)=leaf.strip_prefix("TemplateSlot#") {
        let (r,c)=n.split_once('_')?; let index=r.parse::<u32>().ok()?*7+c.parse::<u32>().ok()?;
        return Some(if w.id.contains("/IngredientBag/") { Cell::Ingredient(index as usize) } else { Cell::Bag(index) });
    }
    let slot=match leaf {
        "Helmet"=>0,"Shoulder"=>1,"Torso"=>2,"Gloves"=>3,"Boots"=>4,"RightHand"=>5,"LeftHand"=>6,
        "Back"=>7,"RingLeftHand"=>8,"RingRightHand"=>9,"Neck"=>10,"Waist"=>11,"WeaponMount"=>12,
        "RuneTrinket1"=>13,"RuneTrinket2"=>14,"RuneTrinket3"=>15,"RuneTrinket4"=>16,"RuneTrinket5"=>17,"RuneTrinket6"=>18,"RuneTrinket7"=>19,
        "JewelTrinket1"=>20,"JewelTrinket2"=>21,"JewelTrinket3"=>22,"Ammo"=>35,"AncientTrinket1"=>24,
        "MercenaryNeck"=>25,"MercenaryBack"=>26,"MercenaryWaist"=>27,"AmmoAlt1"=>35,"AmmoAlt2"=>34,
        _=>return None,
    }; Some(Cell::Equipment(slot))
}

const TABS: [&str;9]=["Basics","Boss","BossEvents","CraftingCores","Ammunition","Event_01","Keys","Cooking","Stain"];
pub const EXTRA: usize = usize::MAX-1;
pub const OVERFLOW: usize = usize::MAX;
fn place(c: Cell, w: &WindowControls) -> Option<Place> {
    Some(match c { Cell::Bag(n)=>match w.bag { EXTRA=>Place::Extra(n), OVERFLOW=>Place::Overflow(w.overflow_page as u32*OVERFLOW_PAGE_SLOTS+n), page=>Place::Bag(page as u32*BAG_SLOTS+n) },
        Cell::Equipment(n)=>Place::Equipment(n), Cell::Ingredient(_)=>return None })
}

/// The action bar's essence sockets are real equipment slots, not shortcuts.
fn item_cell(window: &str, action: &Action) -> Option<Cell> {
    match (window, action) {
        ("inventory", Action::Item(cell)) => Some(*cell),
        ("bottombar", Action::Skill(hotbar::PRIMARY_ESSENCE)) => Some(Cell::Equipment(35)),
        ("bottombar", Action::Skill(hotbar::BACKUP_ESSENCE)) => Some(Cell::Equipment(34)),
        _ => None,
    }
}

/// An item a consumable quick slot takes: a potion, an elixir.
pub(super) fn shortcut(catalog: &Catalog, template: &str) -> bool {
    catalog.items.get(template).is_some_and(|m| matches!(m.item_category.as_str(), "Consumable" | "InstantConsumable"))
}

/// The Universal Map (TravelMapItem): it opens the travel map.
pub(super) fn travel_map(catalog: &Catalog, template: &str) -> bool {
    catalog.items.get(template).is_some_and(|m| m.item_category == "TravelMapItem")
}

fn use_consumable(state: &InventoryState, catalog: &Catalog, id: u32) -> Option<dsor_proto::commands::ClientCommand> {
    let item = state.items.get(&id)?;
    let meta = catalog.items.get(&item.template)?;
    // Mounts, pets, costumes and shadow souls are used too, by one right click as
    // in the original (the user, 2026-10-23: "it has to be right click"); the
    // server summons or wears one and sends it away again (equip_toggle).
    if item.third == 0 || !state.movable(id) || !matches!(state.location(id), Some(Place::Bag(_) | Place::Extra(_)))
        || !matches!(meta.item_category.as_str(), "Consumable" | "InstantConsumable" | "SkinConsumable" | "RecipeConsumable" | "ConsumablePaymentItem"
            | "Mount" | "Pet" | "Mercenary" | "Switchable" | "Costume" | "TempCostume") { return None; }
    Some(crate::gameplay::inventory::command(id, dsor_proto::commands::inventory::Inventory::USE, vec![]))
}
fn ingredient<'a>(catalog: &'a Catalog, w: &WindowControls, slot: usize) -> Option<&'a Ingredient> {
    let class=["warrior","mage","ranger","dwarf"].get(w.spells.class as usize).copied().unwrap_or("mage");
    catalog.ingredients.iter().find(|i| i.tab==TABS[w.ingredient.min(8)] && (i.rarity==w.rarity || i.rarity==0)
        && i.slot==slot && (i.class_name=="any" || i.class_name.is_empty() || i.class_name==class))
}

struct DisplayItem<'a> { item: Option<&'a ItemInfo>, template: &'a str, amount: u32, unlock: u32, read_only: bool, waiting: bool }
fn displayed<'a>(cell: Cell, c: &WindowControls, state: &'a InventoryState, catalog: &'a Catalog) -> Option<DisplayItem<'a>> {
    if let Cell::Ingredient(slot)=cell {
        let i=ingredient(catalog,c,slot)?;
        let items:Vec<_>=state.items.values().filter(|x| x.template==i.template && state.allocation.words_180.contains(&x.id)).collect();
        return Some(DisplayItem { template:&i.template, amount:items.iter().map(|x| x.third).sum(), item:items.into_iter().min_by_key(|x|x.id), unlock:i.unlock, read_only:true, waiting:false });
    }
    let item=state.at(place(cell,c)?)?;
    let waiting=matches!(place(cell,c),Some(Place::Overflow(_)));
    Some(DisplayItem { item:Some(item), template:&item.template, amount:item.third, unlock:0, read_only:waiting, waiting })
}

struct DragItem { id:u32, revision:u64, origin:Vec2, active:bool, root:Entity }
struct Split { id:u32, revision:u64, target:Place, total:u32 }
#[derive(Resource,Default)]
pub struct Interaction { drag:Option<DragItem>, split:Option<Split>, pending:Option<(u64,f64)>,
    /// The item DESTROY_DIALOG asks about.
    destroy:Option<u32> }
impl Interaction {
    pub(super) fn cancel_drag(&mut self)->bool { self.drag.take().is_some() }
    /// The item being dragged, once the drag is under way.
    fn lifted(&self)->Option<u32> { self.drag.as_ref().filter(|d|d.active).map(|d|d.id) }
}

/// Whether a cell draws its item: not the one being dragged out of it, which
/// the cursor carries meanwhile (the ingredient tab's totals stay), as the
/// original (its dragged ring's slot shows the empty ring) and the hotbar.
/// FAILURE (2026-10-23): a worn ring dragged off showed twice, in its slot and
///   under the cursor (the user's screenshot).
fn drawn(cell:Cell, item:Option<&ItemInfo>, lifted:Option<u32>)->bool {
    matches!(cell,Cell::Ingredient(_)) || lifted.is_none_or(|id|item.is_none_or(|i|i.id!=id))
}

/// An item dropped on the world, out of every window, is destroyed once this
/// dialog is confirmed: confirmdialog's DeleteItem variant (the item in its
/// slot beside the question), centred.
/// UNVERIFIED: the question's runtime text (the layout's is a placeholder) and
///   whether the "do not show again" variant is ever chosen.
pub(in crate::ui) const DESTROY_DIALOG: &str = "confirmdialog";
const DELETE_ITEM: &str = "Canvas/DialogCenter/Content/DeleteItem";
/// gui.window.general_confirmText.
const DESTROY_QUESTION: &str = "Do you really wish to destroy this item?";

pub(in crate::ui) fn prepare_destroy(layout:&Layout)->Layout {
    let mut out=layout.clone();
    // DialogBottom is the same dialog placed lower.
    out.widgets.retain(|w|!w.id.starts_with("Canvas/DialogBottom"));
    for w in &mut out.widgets {
        let Some(rest)=w.id.strip_prefix("Canvas/DialogCenter/Content/") else{continue};
        // The question and the item are drawn by destroy_dialog.
        w.hidden=(rest!="DeleteItem" && !rest.starts_with("DeleteItem/")) || rest.ends_with("/Question");
    }
    out
}

/// Only what the bags carry: the server's discard (operation 0) leaves an
/// equipped item's look and stats as they were (DSOR_Server
/// services/inventory_map.py, "Discard is predicted by the client").
fn destroyable(state:&InventoryState, id:u32)->bool {
    state.movable(id) && matches!(state.location(id),Some(Place::Bag(_)|Place::Extra(_)))
}
/// Opens DESTROY_DIALOG on the item; false while its layout is not loaded.
fn ask_destroy(ui:&mut Interaction, windows:&mut Windows, id:u32)->bool {
    let mut found=false;
    for (_,name,mut c,mut v,_) in windows.iter_mut() { if name.0==DESTROY_DIALOG {
        c.hovered=None;c.pressed=None;*v=Visibility::Inherited;ui.destroy=Some(id);found=true;
    }}
    found
}

fn set_amount(c:&mut WindowControls, amount:u32) {
    let Some(id)=c.buttons.iter().find(|b| matches!(b.action,Action::Edit(_))).map(|b|b.id.clone()) else{return};
    let value=amount.to_string(); c.entries.insert(id,TextEntry { cursor:value.len(),text:value,selected:true });
}
fn amount(c:&WindowControls)->Option<u32> { c.entries.values().next()?.text.trim().parse().ok() }
fn open_split(ui:&mut Interaction, windows:&mut Windows, state:&InventoryState, id:u32) {
    let Some(item)=state.items.get(&id) else{return};
    if item.third<=1 {return;}
    let Some(base)=state.location(id).filter(|p|!matches!(p,Place::Equipment(_))) else{return};
    let Some(target)=state.free_slot(base) else{return};
    for (_,name,mut c,mut v,mut tf) in windows.iter_mut() { if name.0=="unstackwindow" {
        set_amount(&mut c,(item.third/2).max(1)); *v=Visibility::Inherited; tf.translation.z=600.0;
        ui.split=Some(Split{id,revision:state.revision,target,total:item.third});
    }}
}

/// How far an equipment slot's icon_slot_bg frame reaches past the Slot itself,
/// reference pixels (inventory.ui.json: Helmet 949.5..999.7 in 945.9..1003.4).
const EQUIPMENT_FRAME_MARGIN: f32 = 3.6;

/// Two clicks on the same item within this many seconds.
const DOUBLE_CLICK: f64 = 0.4;

/// The move a double click makes: an item in the bags to its equipment slot (a
/// free one of its slots, else the first, swapping), an equipped one to the
/// first free bag cell. None when it cannot be worn (class, level) or go back.
fn equip_toggle(state:&InventoryState, catalog:&Catalog, id:u32, level:u32, class:&str) -> Option<dsor_proto::commands::ClientCommand> {
    let item=state.items.get(&id)?; let meta=catalog.items.get(&item.template)?;
    // A mount is used, not worn: InventoryItemAction 0x13 starts its cast, and
    // again takes it away (DSOR_Server services/inventory_map.py, "use operation").
    // Pets, costumes and shadow souls the same: the server summons or wears one
    // and sends it away again, nothing consumed ("Costumes and pets are
    // ``Switchable`` items", "Shadow souls are reusable companions"). None has a slot.
    // FAILURE (2026-10-23): a Mortis Doll in the bag did nothing on any click.
    if matches!(meta.item_category.as_str(), "Mount" | "Pet" | "Mercenary" | "Switchable" | "Costume" | "TempCostume") {
        return Some(crate::gameplay::inventory::command(id,dsor_proto::commands::inventory::Inventory::USE,vec![]));
    }
    if let Some(Place::Equipment(_))=state.location(id) {
        let free=state.free_slot(Place::Bag(0))?;
        return state.drop_on(id,free,meta.max_stack);
    }
    // Unidentified equipment is not worn until identified (a right click).
    if item.flag_0 || meta.slots.is_empty() || meta.required_level>level || !(meta.class_name.is_empty() || meta.class_name=="any" || meta.class_name==class) {return None;}
    let slot=meta.slots.iter().copied().find(|s|state.at(Place::Equipment(*s)).is_none()).unwrap_or(meta.slots[0]);
    state.drop_on(id,Place::Equipment(slot),meta.max_stack)
}

/// Whether an item may be worn in equipment `slot`: one of its slots, identified,
/// its level reached, its class ours (or any).
fn wearable(item:Option<&ItemInfo>, meta:Option<&Definition>, slot:i8, level:u32, class:u8)->bool {
    !item.is_some_and(|i|i.flag_0) && meta.is_some_and(|m|m.slots.contains(&slot) && m.required_level<=level
        && (m.class_name.is_empty() || m.class_name=="any" || m.class_name==["warrior","mage","ranger","dwarf"][class.min(3) as usize]))
}

/// The equipment slots the dragged item may go to (bit n: slot n), worn or empty.
fn drop_targets(ui:&Interaction, state:&InventoryState, catalog:&Catalog, level:u32, class:u8)->u64 {
    let Some(item)=ui.drag.as_ref().filter(|d|d.active).and_then(|d|state.items.get(&d.id)) else{return 0};
    let meta=catalog.items.get(&item.template);
    meta.map_or(0,|m|m.slots.iter().filter(|s|(0..64).contains(*s) && wearable(Some(item),meta,**s,level,class)).fold(0,|bits,s|bits|1<<*s))
}

/// While an item is dragged, the equipment slots it may go to glow, taken or not,
/// as the original's do: the inventory layout's own slot_highlight (a pale blue
/// line 15 % inside its rect, fading out to the rect's edge, a faint white fill
/// within), stretched over the slot's frame, drawn over the slot's background,
/// under its empty-slot icon and the item's icon.
/// Measured on the user's release261_192 shot (rings dragged, Xerines): the
/// line's square is 0.68-0.70 of the frame, (179, 219, 240) at its peak; the
/// empty ring stays dark over the fill; a worn ring covers the line, its fade
/// showing between the frame and the icon.
/// FAILURE (2026-10-23): nothing showed where a dragged item could go.
/// FAILURE (2026-10-23): fx_icon_glow tinted pale blue spread a teal haze far
///   past the frames ("still looks off"; the user's rings, side by side).
const DROP_HIGHLIGHT: &str = "Canvas/slot_highlight";

/// The layout's slot_highlight as a mesh over the unit square (x to 1, y down
/// to -1), to be scaled over a slot's frame, and its texture.
fn drop_highlight(layout:&Layout)->Option<(Mesh,Option<String>)> {
    unit_art(layout.widgets.iter().find(|w|w.id==DROP_HIGHLIGHT)?)
}
/// A widget's artwork as a mesh over the unit square (x to 1, y down to -1),
/// to be scaled over a rect, and its texture.
fn unit_art(w:&Widget)->Option<(Mesh,Option<String>)> {
    let mut art=w.art.clone()?; let [l,t,r,b]=w.rect;
    for p in art.pos.chunks_exact_mut(2) { p[0]=(p[0]-l)/(r-l); p[1]=-(p[1]-t)/(b-t); }
    let mut run=Run::default(); run.push(&art,0.0);
    Some((run.mesh(),art.tex))
}

/// An equipped item's level, in the original's small dark box: the layout's
/// UpgradeButtonTemplate_Green, its normal face, white text with a black glow.
/// SOURCE: dro_client64 (2026) UI::InventoryWidget::UpdateItemUpgradeButton
///   (0x1404636d4) -> 0x140451340: only slot types 0-10 and 12 (no belt,
///   trinkets, mercenary or essences); centred 0.00625 of the screen (6.4 x
///   4.8 reference pixels) in from the Slot's top right corner, its top left
///   for 5 and 7-10 and 12 (mask 0x17a0: the left column); the text "%d".
///   Its one caller always passes Green: the Red "outdated" face is unused.
/// UNVERIFIED: the original shows the sum of two GameItem fields (+0x40,
///   +0x48); the server sends one level, ItemInfo.durability (the tooltip's).
/// NOT DONE: its tooltip and left click ("Left-click to upgrade the item.",
///   the item upgrade window).
const LEVEL_LABEL: &str = "/UpgradeButtonTemplate_Green/normal";
const LEVEL_INSET: Vec2 = Vec2::new(6.4, 4.8);
/// The label's TextEffectSize (TextEffect 2, black).
const LEVEL_OUTLINE: f32 = 1.7;

/// Where an equipped item's level is centred over its Slot `rect`; None for
/// the slots that show none.
fn level_centre(slot:i8, rect:[f32;4])->Option<Vec2> {
    let left=match slot { 0..=4|6=>false, 5|7..=10|12=>true, _=>return None };
    let [l,t,r,_]=rect;
    Some(Vec2::new(if left {l+LEVEL_INSET.x} else {r-LEVEL_INSET.x}, t+LEVEL_INSET.y))
}

/// The level label's face over the unit square, its texture, its size and its
/// font size (reference pixels).
fn level_face(layout:&Layout)->Option<(Mesh,Option<String>,Vec2,f32)> {
    let w=layout.widgets.iter().find(|w|w.id.ends_with(LEVEL_LABEL))?;
    let (mesh,tex)=unit_art(w)?;
    Some((mesh,tex,Vec2::new(w.rect[2]-w.rect[0],w.rect[3]-w.rect[1]),w.text.as_ref().map_or(9.0,|t|t.size)*FONT_SCALE))
}

/// White text centred at `at`, outlined in black (crate::ui::outline_copies).
fn outlined(commands:&mut Commands,assets:&AssetServer,screen:&Screen,parent:Entity,value:String,at:Vec3,size:f32) {
    let font=TextFont{font:assets.load("fonts/tahoma.ttf").into(),font_size:bevy::text::FontSize::Px(size*screen.scale),..default()};
    let text=commands.spawn((Text2d::new(value.clone()),font.clone(),TextColor(Color::WHITE),Anchor::CENTER,
        Transform::from_translation(at).with_scale(Vec3::splat(1.0/screen.scale)),RenderLayers::layer(LAYER),ChildOf(parent))).id();
    for (offset,shadow) in outline_copies((0.5+0.35*LEVEL_OUTLINE).min(1.5)) {
        let copy=OutlineCopy{offset,shadow,dz:-0.01,color:Color::BLACK};
        commands.spawn((Text2d::new(value.clone()),font.clone(),TextColor(Color::BLACK),Anchor::CENTER,copy.transform(screen.scale),
            bevy::sprite::Text2dShadow{offset:shadow*screen.scale,color:Color::BLACK},RenderLayers::layer(LAYER),ChildOf(text),Visibility::Inherited,copy));
    }
}

/// DSOR_TEST_DROP_SLOTS=8,9, offline: those equipment slots glow as under a
/// drag (screenshots; offline drags are refused).
fn test_drop_slots(offline:bool)->u64 {
    static SLOTS:std::sync::OnceLock<u64>=std::sync::OnceLock::new();
    if !offline {return 0;}
    *SLOTS.get_or_init(||std::env::var("DSOR_TEST_DROP_SLOTS").map_or(0,|v|v.split(',').filter_map(|s|s.trim().parse::<u32>().ok()).filter(|s|*s<64).fold(0,|bits,s|bits|1<<s)))
}

/// What a right click looks up: the open shop (selling) and the travel items.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Lookups<'w> {
    shop: Option<Res<'w, crate::ui::dialogue::Presentation>>,
    quest_data: Option<Res<'w, Assets<crate::gameplay::quests::QuestCatalog>>>,
    quest_ui: Option<Res<'w, crate::gameplay::quests::QuestUi>>,
    /// The NPC desks' picks (super::systems::pick).
    desks: Option<ResMut<'w, super::systems::State>>,
}

#[allow(clippy::too_many_arguments)]
pub fn input(mut ui:ResMut<Interaction>,mut pointer:ResMut<Pointer>,mut windows:Windows,
    primary:Query<&Window,With<PrimaryWindow>>,mouse:Res<ButtonInput<MouseButton>>,keys:Res<ButtonInput<KeyCode>>,
    net:Option<NonSendMut<crate::network::net::Net>>,data:Res<Data>,catalogs:Res<Assets<Catalog>>,time:Res<Time<Real>>,
    mut last_click:Local<Option<(u32,f64)>>,mut trade:ResMut<super::trade::TradeUi>,capture:Res<crate::ui::input::UiInput>,mut tested:Local<bool>,
    mut lookups:Lookups) {
    let Some(mut net)=net else{return}; let Some(catalog)=catalogs.get(&data.catalog) else{return};
    let Ok(window)=primary.single() else{return};
    let point=window.cursor_position().map(|p|p*(BASE_H/window.height().max(1.0)));
    if !window.focused {ui.drag=None;}
    if ui.pending.is_some_and(|(r,t)|r!=net.inventory.revision || time.elapsed_secs_f64()-t>2.0) {ui.pending=None;}
    let busy=ui.pending.is_some() || net.offline || !net.inventory.ready;
    let dialog=windows.iter().find(|(_,n,_,v,_)|n.0=="unstackwindow" && **v!=Visibility::Hidden).map(|(e,_,_,_,_)|e);
    if let Some(e)=dialog {
        let valid=ui.split.as_ref().is_some_and(|s|s.revision==net.inventory.revision && net.inventory.items.contains_key(&s.id));
        if !valid {windows.get_mut(e).unwrap().3.set_if_neq(Visibility::Hidden); ui.split=None;return;}
        let split=ui.split.as_ref().unwrap();
        let (_,_,mut c,_,tf)=windows.get_mut(e).unwrap();
        let change=pointer.item_command.take();
        if let Some(delta)=change.filter(|d|*d!=0) {let n=(amount(&c).unwrap_or(1) as i64+delta as i64).clamp(1,split.total as i64-1) as u32;set_amount(&mut c,n);}
        if window.focused && mouse.pressed(MouseButton::Left) {
            if let (Some(b),Some(p))=(c.pressed.map(|i|&c.buttons[i]),point) {if b.action==Action::SplitSlider {
                let x=p.x-window_offset(&tf).x-b.k*(BASE_H*window.width()/window.height().max(1.0)-BASE_W);
                let f=((x-b.rect[0])/(b.rect[2]-b.rect[0])).clamp(0.0,1.0);
                let n=1+(f*(split.total-2) as f32).round() as u32; set_amount(&mut c,n);
            }}
        }
        if change==Some(0) || keys.just_pressed(KeyCode::Enter) {
            if let Some(command)=amount(&c).and_then(|n|net.inventory.split(split.id,n,split.target)) {
                net.send(&command);ui.pending=Some((net.inventory.revision,time.elapsed_secs_f64()));ui.split=None;
                windows.get_mut(e).unwrap().3.set_if_neq(Visibility::Hidden);pointer.focus=None;
            }
        }
        return;
    }
    ui.split=None;pointer.item_command=None;
    // DSOR_TEST_DESTROY=1, offline (DSOR_INVENTORY_PREVIEW): the dialog asks about
    // the first carried item (screenshots).
    if !*tested && net.offline && net.inventory.ready && std::env::var_os("DSOR_TEST_DESTROY").is_some() {
        let first=net.inventory.allocation.placements.iter().map(|(id,_)|*id).filter(|id|destroyable(&net.inventory,*id)).min();
        *tested=first.is_none_or(|id|ask_destroy(&mut ui,&mut windows,id));
    }
    // Confirmed, the server destroys the item (InventoryCommand operation 0) and
    // sends the inventory back: nothing is removed here before that.
    let asking=windows.iter().find(|(_,n,_,v,_)|n.0==DESTROY_DIALOG && **v!=Visibility::Hidden).map(|(e,_,_,_,_)|e);
    if let Some(e)=asking {
        let answer=pointer.destroy.take();
        let item=ui.destroy.filter(|id|destroyable(&net.inventory,*id));
        if item.is_none() || answer.is_some() {
            if let (Some(id),Some(true),false)=(item,answer,busy) {
                net.send(&crate::gameplay::inventory::command(id,dsor_proto::commands::inventory::Inventory::DELETE,vec![]));
                ui.pending=Some((net.inventory.revision,time.elapsed_secs_f64()));
            }
            windows.get_mut(e).unwrap().3.set_if_neq(Visibility::Hidden);ui.destroy=None;
        }
        return;
    }
    ui.destroy=None;pointer.destroy=None;
    let hovered=windows.iter().filter(|(_,_,_,v,_)|**v!=Visibility::Hidden)
        .find_map(|(e,n,c,_,_)|c.hovered.and_then(|b|item_cell(&n.0,&c.buttons[b].action).map(|cell|(e,cell))));
    // Right-click sells a carried stack while a shop that buys is open (the
    // server keeps it in the buyback list), else uses carried consumables, mounts,
    // pets and costumes. Equipment and essences keep their drag/double-click controls.
    let trading=windows.iter().any(|(_,n,_,v,_)|n.0==super::trade::WINDOW && *v!=Visibility::Hidden);
    if mouse.just_pressed(MouseButton::Right) && !busy {
        if let Some((root,cell))=hovered {
            let (_,_,c,_,_)=windows.get(root).unwrap();
            if let Some(id)=place(cell,c).and_then(|p|net.inventory.at(p)).map(|i|i.id) {
                let selling=lookups.shop.as_ref().and_then(|p|p.open_shop()).filter(|(_,can_sell)|*can_sell).map(|(shop,_)|shop.to_owned());
                let carried=matches!(net.inventory.location(id),Some(Place::Bag(_)|Place::Extra(_)));
                let unidentified=net.inventory.items.get(&id).is_some_and(|i|i.flag_0);
                let travel=net.inventory.items.get(&id).and_then(|i|lookups.quest_data.as_ref().zip(lookups.quest_ui.as_ref()).and_then(|(d,u)|d.get(&u.catalog)).and_then(|q|q.travel_items.get(&i.template).cloned()));
                // An open NPC desk takes the item: picked for melting, laid on the
                // gem or transmog desk, a gem set into the desk's item.
                let desk=windows.iter().find(|(_,n,_,v,_)|super::systems::DESKS.iter().any(|(d,_)|*d==n.0.as_str()) && **v!=Visibility::Hidden).map(|(_,n,_,_,_)|n.0.clone());
                if let (Some(desk),Some(state))=(desk,lookups.desks.as_mut()) {
                    if let Some(item)=net.inventory.items.get(&id).cloned() {
                        let category=catalog.items.get(&item.template).map(|d|d.item_category.clone()).unwrap_or_default();
                        if let Some(cmd)=super::systems::pick(state,&desk,&net,&item,&category) { net.send(&cmd);ui.pending=Some((net.inventory.revision,time.elapsed_secs_f64())); }
                    }
                } else if unidentified && carried {
                    // Unidentified: a right click identifies it (InventoryCommand operation
                    // 0x12; the server answers with the item, DSOR_Server services/inventory_map.py).
                    net.send(&crate::gameplay::inventory::command(id,0x12,vec![]));ui.pending=Some((net.inventory.revision,time.elapsed_secs_f64()));
                } else if let Some(map)=travel.filter(|_|carried && net.inventory.movable(id)) {
                    // A travel item: its use (0x13) arms it on the server for 15 s, then the
                    // travel names the item's map (_Template_Item.TravelItemTarget).
                    net.send(&crate::gameplay::inventory::command(id,dsor_proto::commands::inventory::Inventory::USE,vec![]));
                    net.send(&dsor_proto::commands::ClientCommand::UseTravelItem(dsor_proto::commands::inventory::UseTravelItem { item:id, map }));
                    ui.pending=Some((net.inventory.revision,time.elapsed_secs_f64()));
                } else if let Some(shop)=selling {
                    if net.inventory.movable(id) && matches!(net.inventory.location(id),Some(Place::Bag(_)|Place::Extra(_))) {
                        net.send(&dsor_proto::commands::ClientCommand::Transaction(dsor_proto::commands::inventory::Transaction::sell(&shop,id)));
                        ui.pending=Some((net.inventory.revision,time.elapsed_secs_f64()));
                    }
                } else if travel_map(catalog,net.inventory.items.get(&id).map_or("",|i|i.template.as_str())) {
                    // The Universal Map: "(right click to open the travel map)" (gui.window
                    // howToUseTravelMapItem); the server has no use for it.
                    pointer.open_window=Some("travelmap");
                } else if let Some(cmd)=use_consumable(&net.inventory,catalog,id) {
                    if trading { trade.add.push(id); }
                    else { net.send(&cmd);ui.pending=Some((net.inventory.revision,time.elapsed_secs_f64())); }
                }
            }
        }
    }
    if mouse.just_pressed(MouseButton::Left) {
        ui.drag=None;
        if let (false,Some((root,cell)),Some(p))=(busy,hovered,point) {
            let (_,_,c,_,_)=windows.get(root).unwrap();
            if let Some(id)=place(cell,c).and_then(|p|net.inventory.at(p)).map(|i|i.id).filter(|id|net.inventory.movable(*id)) {
                let now=time.elapsed_secs_f64();
                let double=last_click.take().is_some_and(|(last,at)|last==id && now-at<=DOUBLE_CLICK);
                if keys.any_pressed([KeyCode::ControlLeft,KeyCode::ControlRight]) {open_split(&mut ui,&mut windows,&net.inventory,id);pointer.down=None;}
                else if double {
                    // A double click equips a piece from the bags, or takes an
                    // equipped one off into the first free bag cell.
                    let class=["warrior","mage","ranger","dwarf"][c.spells.class.min(3) as usize];
                    if let Some(cmd)=equip_toggle(&net.inventory,catalog,id,net.level.unwrap_or(1),class) {
                        net.send(&cmd);ui.pending=Some((net.inventory.revision,now));
                    }
                    pointer.down=None;
                }
                else {ui.drag=Some(DragItem{id,revision:net.inventory.revision,origin:p,active:false,root});*last_click=Some((id,now));}
            }
        }
    }
    if ui.drag.as_ref().is_some_and(|d|d.revision!=net.inventory.revision || !windows.get(d.root).is_ok_and(|(_,_,_,v,_)|*v!=Visibility::Hidden)) {ui.drag=None;}
    if let Some(d)=ui.drag.as_mut() {if point.is_some_and(|p|p.distance(d.origin)>=4.0) {d.active=true;*last_click=None;}}
    if mouse.just_released(MouseButton::Left) {
        // Dropped on our side of the trade window: offered.
        let on_trade=windows.iter().filter(|(_,n,_,v,_)|n.0==super::trade::WINDOW && **v!=Visibility::Hidden)
            .any(|(_,_,c,_,_)|c.hovered.is_some_and(|b|matches!(c.buttons[b].action,Action::Social(super::social::Social::TradeMine(_)))));
        if on_trade {if let Some(d)=ui.drag.take().filter(|d|d.active) {trade.add.push(d.id);}}
        // Dropped on the bar: a shortcut to it there (super::stickers::apply_drop),
        // the item staying in the bag. The Universal Map on a skill slot (the user
        // kept it on 2 to travel fast), potions and elixirs on the consumable slots.
        // FAILURE (2026-10-24): items could not be put on the bar at all ("the map
        //   I can't put in the slots"; "I meant the skill slots").
        let on_bar=windows.iter().filter(|(_,n,_,v,_)|n.0=="bottombar" && **v!=Visibility::Hidden)
            .find_map(|(_,_,c,_,_)|c.hovered.and_then(|b|match c.buttons[b].action {Action::Skill(s)=>Some(s),_=>None}));
        // Only an item that fits takes the drop (an essence on its socket goes on
        // to the normal equipment move below).
        let fits=|i:&ItemInfo,slot:usize| if travel_map(catalog,&i.template) {slot<super::stickers::SKILL_SLOTS} else {shortcut(catalog,&i.template) && super::stickers::CONSUMABLE_SLOTS.contains(&slot)};
        let shortcut_drop=on_bar.zip(ui.drag.as_ref().filter(|d|d.active).and_then(|d|net.inventory.items.get(&d.id)))
            .filter(|(slot,item)|fits(item,*slot)).map(|(slot,item)|(item.template.clone(),slot));
        if let Some((template,slot))=shortcut_drop {ui.drag=None;pointer.sticker_drop=Some((1,template,slot));}
        if let Some(d)=ui.drag.take().filter(|d|d.active && !busy) {
            if let Some((root,cell))=hovered {
                let (_,_,c,_,_)=windows.get(root).unwrap();
                if let Some(target)=place(cell,c) {
                    let source=net.inventory.items.get(&d.id);
                    let meta=source.and_then(|i|catalog.items.get(&i.template));
                    let valid=if let Place::Equipment(slot)=target {wearable(source,meta,slot,net.level.unwrap_or(1),c.spells.class)} else{true};
                    if let Some(cmd)=valid.then(||net.inventory.drop_on(d.id,target,meta.map_or(1,|m|m.max_stack))).flatten() {
                        net.send(&cmd);ui.pending=Some((net.inventory.revision,time.elapsed_secs_f64()));
                    }
                }
            } else if point.is_some() && !capture.over_ui && !trading && destroyable(&net.inventory,d.id) {
                // Out of every window, on the world: ask before destroying it.
                ask_destroy(&mut ui,&mut windows,d.id);
            }
        }
    }
}

#[derive(Component)] pub struct DestroyContent;
/// DESTROY_DIALOG's item, drawn in its DeleteItem slot as a cell draws it, and
/// the question with the item's name.
#[allow(clippy::too_many_arguments,clippy::type_complexity)]
pub fn destroy_dialog(mut commands:Commands,ui:Res<Interaction>,windows:Query<(Entity,&Window2018,&Visibility)>,old:Query<Entity,With<DestroyContent>>,
    net:Option<NonSend<crate::network::net::Net>>,(data,catalogs):(Res<Data>,Res<Assets<Catalog>>),(layouts,loaded):(Res<Assets<Layout>>,Res<crate::ui::Loaded>),
    assets:Res<AssetServer>,screen:Res<Screen>,mut last:Local<Option<(Entity,u32,u64,u32,u32,bool)>>) {
    let catalog=catalogs.get(&data.catalog);
    let open=ui.destroy.zip(windows.iter().find(|(_,n,v)|n.0==DESTROY_DIALOG && **v!=Visibility::Hidden).map(|(e,_,_)|e));
    let key=open.map(|(id,e)|(e,id,net.as_ref().map_or(0,|n|n.inventory.revision),screen.width.to_bits(),screen.scale.to_bits(),catalog.is_some()));
    if *last==key {return;} *last=key; for e in &old {commands.entity(e).despawn();}
    let (Some((id,root)),Some(net),Some(catalog))=(open,net,catalog) else{return};
    let Some(item)=net.inventory.items.get(&id) else{return};
    let Some(layout)=loaded.layouts.iter().filter_map(|(h,_,_)|layouts.get(h)).find(|l|l.window==DESTROY_DIALOG) else{return};
    let widget=|leaf:&str|layout.widgets.iter().find(|w|w.id==format!("{DELETE_ITEM}/{leaf}"));
    let group=commands.spawn((DestroyContent,Transform::from_xyz(0.0,0.0,WINDOW_ITEMS_DEPTH),Visibility::Inherited,ChildOf(root))).id();
    if let Some(w)=widget("ItemSlot/IconLabel") {
        let [l,t,r,b]=w.rect;
        sprites(&mut commands,&assets,catalog,&item.template,Some(item),screen.world(w.k,l,t).extend(0.0),Vec2::new(r-l,b-t),Color::WHITE,group);
        if item.third>1 {
            super::social::label(&mut commands,&assets,&screen,group,item.third.to_string(),screen.world(w.k,l,b-16.0).extend(0.2),10.0,
                Color::srgb(1.0,0.865,0.677),r-l-4.0,bevy::text::Justify::Right);
        }
    }
    if let Some(w)=widget("Question") {
        let [l,t,r,b]=w.rect;
        let name=catalog.items.get(&item.template).map_or(item.template.as_str(),|m|m.name.as_str());
        let font=TextFont{font:assets.load("fonts/tahoma.ttf").into(),font_size:bevy::text::FontSize::Px(9.0*FONT_SCALE*screen.scale),..default()};
        commands.spawn((Text2d::new(format!("{DESTROY_QUESTION}\n\n")),font.clone(),TextColor(Color::srgb(0.9,0.78,0.54)),
            bevy::text::TextLayout::justify(bevy::text::Justify::Center),Anchor::CENTER,bevy::text::TextBounds::new_horizontal((r-l)*screen.scale),
            Transform::from_translation(screen.world(w.k,(l+r)*0.5,(t+b)*0.5).extend(0.2)).with_scale(Vec3::splat(1.0/screen.scale)),
            RenderLayers::layer(LAYER),ChildOf(group)))
            .with_child((TextSpan::new(name),font,TextColor(preview::rarity(catalog,&item.template,Some(item)).1)));
    }
}

#[derive(Component)] pub struct Contents;
#[derive(Component)] pub struct Overlay;
pub(super) fn text(commands:&mut Commands,assets:&AssetServer,screen:&Screen,parent:Entity,value:String,at:Vec3,size:f32,color:Color,width:f32) {
    commands.spawn((Text2d::new(value),TextFont{font:assets.load("fonts/tahoma.ttf").into(),font_size:bevy::text::FontSize::Px(size*TEXT_SCALE*screen.scale),..default()},
        TextColor(color),Anchor::TOP_LEFT,bevy::text::TextBounds::new_horizontal(width*screen.scale),
        Transform::from_translation(at).with_scale(Vec3::splat(1.0/screen.scale)),RenderLayers::layer(LAYER),ChildOf(parent)));
}
/// An item as a cell draws it, top-left at `at`: its rarity background, its icon,
/// and its frame over both (icons_toolkit/frame_slim_*). False without an icon.
/// FAILURE (2026-10-19): the dragged copy left out the frame ("dragged only with
///   their background").
#[allow(clippy::too_many_arguments)]
fn sprites(commands:&mut Commands,assets:&AssetServer,catalog:&Catalog,template:&str,item:Option<&ItemInfo>,at:Vec3,size:Vec2,tint:Color,parent:Entity)->bool {
    let (icon,background,frame)=catalog.icon_parts(template,item);
    for (z,image) in [(0.0,background),(0.05,icon),(0.1,frame)] {
        let Some(image)=image else{continue};
        commands.spawn((Sprite{image:assets.load(image.to_owned()),custom_size:Some(size),color:tint,..default()},Anchor::TOP_LEFT,
            Transform::from_translation(at+Vec3::Z*z),RenderLayers::layer(LAYER),ChildOf(parent)));
    }
    icon.is_some()
}
fn color(catalog:&Catalog,item:Option<&ItemInfo>)->Color {
    let rgba=item.and_then(|i|catalog.colors.get(&i.twenty_eighth.to_string())).copied().unwrap_or([0.85,0.8,0.67,1.0]);
    Color::srgba(rgba[0],rgba[1],rgba[2],rgba[3])
}

#[allow(clippy::too_many_arguments,clippy::type_complexity)]
pub fn render(mut commands:Commands,windows:Query<(Entity,&Window2018,&WindowControls,&Visibility)>,net:Option<NonSend<crate::network::net::Net>>,
    data:Res<Data>,catalogs:Res<Assets<Catalog>>,assets:Res<AssetServer>,screen:Res<Screen>,old:Query<Entity,With<Contents>>,ui:Res<Interaction>,
    mut last:Local<Option<(Entity,u64,usize,u8,usize,usize,u32,u32,u8,usize,u64,Option<u32>)>>,(layouts,loaded):(Res<Assets<Layout>>,Res<crate::ui::Loaded>),
    (mut meshes,mut materials):(ResMut<Assets<Mesh>>,ResMut<Assets<UiMaterial>>),mut highlight:Local<Option<(Handle<Mesh>,Handle<UiMaterial>)>>,
    mut level:Local<Option<(Handle<Mesh>,Handle<UiMaterial>,Vec2,f32)>>) {
    let Some(catalog)=catalogs.get(&data.catalog) else{return}; let Some(net)=net else{return};
    let window=windows.iter().find(|(_,n,_,v)|n.0=="inventory" && **v!=Visibility::Hidden);
    let targets=window.map_or(0,|(_,_,c,_)|drop_targets(&ui,&net.inventory,catalog,net.level.unwrap_or(1),c.spells.class)|test_drop_slots(net.offline));
    let lifted=ui.lifted();
    let key=window.map(|(e,_,c,_)|(e,net.inventory.revision,c.bag,c.inventory,c.ingredient,c.rarity,screen.width.to_bits(),screen.scale.to_bits(),c.spells.class,c.overflow_page,targets,lifted));
    if *last==key {return;} *last=key; for e in &old {commands.entity(e).despawn();}
    let Some((root,_,c,_))=window else{return};
    if targets!=0 && highlight.is_none() {
        *highlight=loaded.layouts.iter().filter_map(|(h,_,_)|layouts.get(h)).find(|l|l.window=="inventory").and_then(drop_highlight)
            .map(|(mesh,tex)|(meshes.add(mesh),materials.add(material(tex.map(|t|assets.load(t))))));
    }
    if level.is_none() {
        *level=loaded.layouts.iter().filter_map(|(h,_,_)|layouts.get(h)).find(|l|l.window=="inventory").and_then(level_face)
            .map(|(mesh,tex,size,font)|(meshes.add(mesh),materials.add(material(tex.map(|t|assets.load(t)))),size,font));
    }
    let group=commands.spawn((Contents,Transform::from_xyz(0.0,0.0,WINDOW_ITEMS_DEPTH),Visibility::Inherited,ChildOf(root))).id();
    for b in &c.buttons {
        let Action::Item(cell)=b.action else{continue}; if !c.section_visible(b.section) {continue;}
        // An equipment Slot (50 px) sits inside its icon_slot_bg frame (57.5 px); a
        // bag cell is its frame. Drawn over the frame, an equipped icon fills its
        // slot as a bag one does.
        // FAILURE (2026-10-16): drawn in the Slot, equipped icons stood 7 px inside
        //   their frames ("ceux equipes prennent pas la taille entiere du slot").
        let grow=if matches!(cell,Cell::Equipment(_)) {EQUIPMENT_FRAME_MARGIN} else {0.0};
        let [l,t,r,bt]=[b.rect[0]-grow,b.rect[1]-grow,b.rect[2]+grow,b.rect[3]+grow];let pos=screen.world(b.k,l,t);let size=Vec2::new(r-l,bt-t);
        if let (Cell::Equipment(slot),Some((mesh,glow)))=(cell,highlight.as_ref()) { if (0..64).contains(&slot) && targets & (1<<slot)!=0 {
            // At its slot widget's depth; a slot without art (the essences' AmmoAlt) has none: over the window.
            let z=if b.depth>0.0 {b.depth-WINDOW_ITEMS_DEPTH} else {0.02};
            commands.spawn((Mesh2d(mesh.clone()),MeshMaterial2d(glow.clone()),Transform::from_translation(pos.extend(z)).with_scale(size.extend(1.0)),
                RenderLayers::layer(LAYER),ChildOf(group)));
        }}
        let locked=place(cell,c).is_some_and(|p|!matches!(p,Place::Equipment(_)) && p.cell()>=net.inventory.capacity(p));
        if locked {commands.spawn((Sprite::from_color(Color::srgba(0.01,0.01,0.01,0.8),size),Anchor::TOP_LEFT,Transform::from_translation(pos.extend(0.0)),RenderLayers::layer(LAYER),ChildOf(group)));continue;}
        let Some(d)=displayed(cell,c,&net.inventory,catalog).filter(|d|drawn(cell,d.item,lifted)) else{continue};
        // Unidentified (ItemInfo flag_0, DSOR_Server world_loot_protocol NeedsIdentify):
        // greyed under a question mark until a right click identifies it.
        let unidentified=d.item.is_some_and(|i|i.flag_0);
        let tint=if d.amount==0 {Color::srgba(0.38,0.38,0.38,0.65)} else if unidentified {Color::srgb(0.45,0.45,0.45)} else{Color::WHITE};
        if !sprites(&mut commands,&assets,catalog,d.template,d.item,Vec3::new(pos.x+2.5,pos.y-2.5,0.05),size-Vec2::splat(5.0),tint,group) {
            text(&mut commands,&assets,&screen,group,"?".into(),pos.extend(0.2)+Vec3::new(15.0,-10.0,0.0),22.0,Color::WHITE,size.x);
        }
        if unidentified {
            let mark=size*0.62;
            commands.spawn((Sprite{image:assets.load("textures/ui/icon_questionmark.png"),custom_size:Some(mark),..default()},Anchor::TOP_LEFT,
                Transform::from_translation(Vec3::new(pos.x+(size.x-mark.x)/2.0,pos.y-(size.y-mark.y)/2.0,0.25)),RenderLayers::layer(LAYER),ChildOf(group)));
        }
        if d.amount>1 || matches!(cell,Cell::Ingredient(_)) {
            let at=Vec3::new(pos.x+3.0,pos.y-size.y+17.0,0.3);
            text(&mut commands,&assets,&screen,group,d.amount.to_string(),at+Vec3::new(1.0,-1.0,-0.01),13.0,Color::BLACK,size.x-4.0);
            text(&mut commands,&assets,&screen,group,d.amount.to_string(),at,13.0,Color::WHITE,size.x-4.0);
        }
        if let (Cell::Equipment(slot),Some(item),Some((mesh,face,size,font)))=(cell,d.item,level.as_ref()) { if let Some(c)=level_centre(slot,b.rect) {
            commands.spawn((Mesh2d(mesh.clone()),MeshMaterial2d(face.clone()),Transform::from_translation(screen.world(b.k,c.x-size.x/2.0,c.y-size.y/2.0).extend(0.35))
                .with_scale(size.extend(1.0)),RenderLayers::layer(LAYER),ChildOf(group)));
            outlined(&mut commands,&assets,&screen,group,item.durability.to_string(),screen.world(b.k,c.x,c.y).extend(0.4),*font);
        }}
    }
}

/// Original tooltip corner/edge meshes keep their border thickness as content grows.
pub(super) fn frame(commands:&mut Commands,assets:&AssetServer,meshes:&mut Assets<Mesh>,materials:&mut Assets<UiMaterial>,root:Entity,layout:&Layout,w:f32,h:f32,fill:Color) {
    // The edge meshes include a transparent outer fringe. The fill begins at
    // their opaque vertices, underneath the gold rail, rather than at (0, 0).
    let edge = |name: &str| layout.widgets.iter().find(|w| w.id == format!("Canvas/BGLabel/{name}"));
    let (Some(left), Some(right), Some(top), Some(bottom)) = (edge("leftedge"),edge("rightedge"),edge("topedge"),edge("bottomedge")) else {
        tooltips::card::background(commands,root,w,h,[4.0;4],fill); return;
    };
    let x0=left.rect[0]; let y0=top.rect[1];
    let sw=right.rect[2]-x0; let sh=bottom.rect[3]-y0;
    let map = |v:f32, size:f32, source:f32, low:f32, high:f32| {
        if v<=low {v} else if v>=high {size-(source-v)} else {low+(v-low)*(size-low-(source-high))/(high-low)}
    };
    let opaque = |widget:&Widget,axis:usize,maximum:bool| widget.art.as_ref().map(|a|
        a.pos.chunks_exact(2).zip(a.color.chunks_exact(4)).filter(|(_,c)|c[3]>0.99)
            .map(|(p,_)|p[axis]).reduce(|a,b|if maximum {a.max(b)} else {a.min(b)})).flatten();
    let mut inset=[opaque(left,0,false).unwrap_or(x0+4.0)-x0,opaque(top,1,false).unwrap_or(y0+4.0)-y0,
        x0+sw-opaque(right,0,true).unwrap_or(x0+sw-4.0), y0+sh-opaque(bottom,1,true).unwrap_or(y0+sh-4.0)];
    // The horizontal rail texture still has transparent texels at its opaque
    // mesh vertices. Three reference pixels cover that strip and its filtering.
    inset[1]+=3.0; inset[3]+=3.0;
    tooltips::card::background(commands,root,w,h,inset,fill);
    for widget in &layout.widgets {
        if !widget.id.starts_with("Canvas/BGLabel/") {continue;}
        let leaf=widget.id.rsplit('/').next().unwrap_or("");
        if !leaf.ends_with("edge") && !leaf.ends_with("corner") {continue;}
        let Some(mut art)=widget.art.clone() else{continue};
        for p in art.pos.chunks_exact_mut(2) {
            p[0]=map(p[0]-x0,w,sw,top.rect[0]-x0,top.rect[2]-x0);
            p[1]=-map(p[1]-y0,h,sh,left.rect[1]-y0,left.rect[3]-y0);
        }
        let atlas=art.tex.as_ref().map(|t|assets.load(t.clone()));let mut run=Run::default();run.push(&art,0.0);
        commands.spawn((Mesh2d(meshes.add(run.mesh())),MeshMaterial2d(materials.add(material(atlas))),Transform::from_xyz(0.0,0.0,0.1),RenderLayers::layer(LAYER),ChildOf(root)));
    }
}

mod preview;
mod values;
pub(in crate::ui) mod pins;

#[cfg(test)]
mod interaction_tests;

pub(super) fn sticker_card(catalog:&Catalog, layout:Option<&Layout>, template:&str)->tooltips::card::Card {
    preview::sticker(catalog,layout,template)
}

pub(super) fn preview_card(catalog:&Catalog, layout:Option<&Layout>, template:&str, item:Option<&ItemInfo>, amount:u32, advanced:bool)->tooltips::card::Card {
    preview::card(&DisplayItem { item,template,amount,unlock:0,read_only:false,waiting:false },catalog,layout,advanced)
}

#[allow(clippy::too_many_arguments)]
pub fn overlay(mut commands:Commands,ui:Res<Interaction>,windows:Query<(&Window2018,&WindowControls,&Visibility,&Transform),Without<Overlay>>,primary:Query<&Window,With<PrimaryWindow>>,
    net:Option<NonSend<crate::network::net::Net>>,(data,catalogs,layouts):(Res<Data>,Res<Assets<Catalog>>,Res<Assets<Layout>>),assets:Res<AssetServer>,screen:Res<Screen>,
    (old,mut transforms):(Query<Entity,With<Overlay>>,Query<&mut Transform,With<Overlay>>),
    (mut meshes,mut materials):(ResMut<Assets<Mesh>>,ResMut<Assets<UiMaterial>>),
    mut last:Local<Option<(String,u64,bool,bool,u32,bool)>>, settings:Res<settings::Settings>,keys:Res<ButtonInput<KeyCode>>,pointer:Res<Pointer>) {
    let catalog=catalogs.get(&data.catalog);
    let point=primary.single().ok().filter(|w|w.focused).and_then(|w|w.cursor_position().map(|p|p*(BASE_H/w.height().max(1.0))));
    let dragged=net.as_ref().and_then(|n|ui.drag.as_ref().filter(|d|d.active).and_then(|d|n.inventory.items.get(&d.id)));
    let mut anchor=None;
    let display=dragged.map(|i|DisplayItem{item:Some(i),template:&i.template,amount:i.third,unlock:0,read_only:false,waiting:false}).or_else(|| {
        let (Some(net),Some(catalog))=(&net,catalog) else{return None;};
        windows.iter().filter(|(n,_,v,_)|n.0=="inventory" && **v!=Visibility::Hidden)
            .find_map(|(_,c,_,tf)|c.hovered.and_then(|n|match c.buttons[n].action {
                Action::Item(cell)=>{
                    let b=&c.buttons[n]; let offset=window_offset(tf)+Vec2::new(b.k*(screen.width-BASE_W),0.0);
                    anchor=Some([b.rect[0]+offset.x,b.rect[1]+offset.y,b.rect[2]+offset.x,b.rect[3]+offset.y]);
                    displayed(cell,c,&net.inventory,catalog)
                },_=>None
            }))
    });
    let advanced=settings.current.advanced_tooltips(&keys,pointer.focus.is_some());
    let layout=layouts.get(&data.tooltip);
    let enabled=point.is_some() && ui.split.is_none() && (dragged.is_some() || (settings.current.tooltips && pointer.down.is_none() && pointer.drag.is_none() && pointer.spell_drag.is_none()));
    let key=display.as_ref().filter(|_|enabled).map(|d|(format!("{}:{}:{}",d.template,d.item.map_or(0,|i|i.id),d.amount),net.as_ref().map_or(0,|n|n.inventory.revision),dragged.is_some(),advanced,screen.scale.to_bits(),layout.is_some()));
    let card=display.as_ref().zip(catalog).filter(|_|dragged.is_none()).map(|(d,c)|preview::card(d,c,layout,advanced));
    let at=if dragged.is_some(){point.map(|p|(p-Vec2::splat(22.0),1.0))}else{card.as_ref().zip(anchor).map(|(c,a)|tooltips::card::placement(a,Vec2::new(c.width,c.height()),Vec2::new(screen.width,BASE_H),false))};
    if *last==key && !catalogs.is_changed() && !layouts.is_changed() {if let Some((p,scale))=at {for mut tf in &mut transforms {tf.translation=screen.world(0.0,p.x,p.y).extend(800.0);tf.scale=Vec3::splat(scale);}}return;}
    *last=key.clone();for e in &old {commands.entity(e).despawn();}if key.is_none(){return;}
    let (Some(d),Some((p,scale)),Some(catalog))=(display,at,catalog) else{return;};
    let root=commands.spawn((Overlay,Transform::from_translation(screen.world(0.0,p.x,p.y).extend(800.0)).with_scale(Vec3::splat(scale)),Visibility::Inherited)).id();
    if dragged.is_some() {
        sprites(&mut commands,&assets,catalog,d.template,d.item,Vec3::new(0.0,0.0,-0.01),Vec2::splat(44.0),Color::WHITE,root);
    } else if let Some(card)=card {card.draw(&mut commands,&assets,&screen,&mut meshes,&mut materials,root,layout);}
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsor_proto::commands::inventory::InventoryInfo;

    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn rendered_windows_keep_items_and_labels_in_order_and_start_with_zero_wisdom() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default(), TransformPlugin))
            .init_asset::<Layout>().init_asset::<Mesh>().init_asset::<UiMaterial>()
            .init_asset::<Image>().init_asset::<Font>().init_asset::<Catalog>()
            .init_resource::<Loaded>().init_resource::<Screen>().init_resource::<Figures>()
            .init_resource::<settings::Settings>().init_resource::<hotbar::Hotbar>()
            .init_resource::<dialogue::Presentation>().init_asset::<dialogue::Catalog>()
            .init_resource::<crate::gameplay::dialogue::Dialogue>()
            .init_resource::<crate::gameplay::stats::Sheet>().init_resource::<crate::actors::monsters::HoverInfo>()
            .insert_resource(crate::gameplay::stats::ResourceMax(100.0))
            .init_resource::<super::party::Party>().init_resource::<super::quests::Quests>().init_resource::<super::systems::Views>().init_resource::<super::currency::Shown>().init_resource::<super::message::ItemMessages>().init_resource::<crate::ui::UiWork>()
            .init_resource::<Pointer>().init_resource::<Interaction>().init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<bevy::sprite_render::EntitiesNeedingSpecialization<UiMaterial>>()
            .add_systems(PostUpdate, (refresh_mesh_order, bevy::sprite_render::check_entities_needing_specialization::<UiMaterial>)
                .chain().after(TransformSystems::Propagate).after(bevy::asset::AssetEventSystems))
            .add_systems(Update, (spawn_windows, sync, keyboard, order_windows, read_figures, update_fills, update_values, paint, render).chain());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/interface");
        for (name, z) in HUD_WINDOWS.into_iter().chain(MENU_WINDOWS.map(|(name, _, z)| (name, z))) {
            let layout: Layout = serde_json::from_slice(&std::fs::read(dir.join(format!("{name}.ui.json"))).unwrap()).unwrap();
            let handle = app.world_mut().resource_mut::<Assets<Layout>>().add(layout);
            let mut loaded = app.world_mut().resource_mut::<Loaded>();
            loaded.layouts.push((handle, z, None));
            loaded.spawned.push(false);
        }
        let catalog: Catalog = serde_json::from_slice(&std::fs::read(dir.join("inventory.items.json")).unwrap()).unwrap();
        let template = catalog.ingredients[0].template.clone();
        let catalog = app.world_mut().resource_mut::<Assets<Catalog>>().add(catalog);
        app.insert_resource(Data { catalog, tooltip: default() });
        crate::network::net::start_offline(app.world_mut(), 1, 0);
        app.world_mut().non_send_mut::<crate::network::net::Net>().inventory.apply(InventoryInfo {
            items: vec![ItemInfo { id: 1, third: 23, template, ..default() }],
            placements: vec![(1, 0)], storage_sizes: [0, 0, 224, 0, 0, 0], ..default()
        });
        app.update();
        let roots: HashMap<_, _> = app.world_mut().query::<(Entity, &Window2018)>().iter(app.world())
            .map(|(e, w)| (w.0.clone(), e)).collect();
        let inventory = roots["inventory"];
        let skills = roots["skilltalentswindow"];
        let press = |app: &mut App, key| {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
            app.update();
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset(key);
        };
        press(&mut app, KeyCode::KeyK);
        assert_eq!(*app.world().get::<Visibility>(skills).unwrap(), Visibility::Hidden);
        press(&mut app, KeyCode::KeyI);
        press(&mut app, KeyCode::KeyS);
        assert_eq!(*app.world().get::<Visibility>(skills).unwrap(), Visibility::Inherited);

        // Measure the actual rendered descendants, including runtime item icons
        // and stack labels, rather than just checking the two root transforms.
        fn depths(world: &World, root: Entity) -> (f32, f32) {
            let mut bounds = (f32::INFINITY, f32::NEG_INFINITY);
            let mut queue = vec![root];
            while let Some(e) = queue.pop() {
                if let Some(children) = world.get::<Children>(e) { queue.extend(children.iter()); }
                if world.get::<Mesh2d>(e).is_some() || world.get::<Sprite>(e).is_some() || world.get::<Text2d>(e).is_some() {
                    let z = world.get::<GlobalTransform>(e).unwrap().translation().z;
                    bounds = (bounds.0.min(z), bounds.1.max(z));
                }
            }
            bounds
        }
        assert!(app.world_mut().query::<&Sprite>().iter(app.world()).count() > 0);
        assert!(depths(app.world(), inventory).1 < depths(app.world(), skills).0);
        for _ in 0..20 {
            // External visibility changes must raise the whole window too.
            *app.world_mut().get_mut::<Visibility>(inventory).unwrap() = Visibility::Hidden;
            app.update();
            *app.world_mut().get_mut::<Visibility>(inventory).unwrap() = Visibility::Inherited;
            app.update();
            assert!(depths(app.world(), skills).1 < depths(app.world(), inventory).0);
            press(&mut app, KeyCode::KeyS);
            press(&mut app, KeyCode::KeyS);
            assert!(depths(app.world(), inventory).1 < depths(app.world(), skills).0);
            assert!(depths(app.world(), skills).1 < 500.0, "focus changes must stay below modal windows");
            let queued = &app.world().resource::<bevy::sprite_render::EntitiesNeedingSpecialization<UiMaterial>>().changed;
            let meshes: Vec<_> = app.world().get::<Children>(inventory).unwrap().iter()
                .chain(app.world().get::<Children>(skills).unwrap().iter())
                .flat_map(|parent| app.world().get::<Children>(parent).into_iter().flat_map(|c| c.iter()))
                .filter(|e| app.world().get::<Mesh2d>(*e).is_some()).collect();
            assert!(!meshes.is_empty());
            assert!(meshes.iter().all(|e| queued.contains(e)), "the renderer must refresh retained mesh sort keys after focus changes");
        }
        app.update();
        assert!(app.world().resource::<bevy::sprite_render::EntitiesNeedingSpecialization<UiMaterial>>().changed.is_empty(),
            "stationary UI must keep its cached render data");
        app.world_mut().resource_mut::<Pointer>().focus = Some((skills, "search".into()));
        press(&mut app, KeyCode::KeyS);
        assert_eq!(*app.world().get::<Visibility>(skills).unwrap(), Visibility::Inherited, "typing must consume shortcuts");

        let mut values = app.world_mut().query::<(&Value, &Text2d)>();
        for (kind, expected, count) in [(ValueKind::WisdomLevel, "0", 1), (ValueKind::BarWisdomLevel, "Wisdom Level 0", 1), (ValueKind::WisdomProgress, "", 1), (ValueKind::WisdomPoints, "Available Points: 0", 1)] {
            let actual: Vec<_> = values.iter(app.world()).filter(|(v, _)| v.0 == kind).map(|(_, t)| t.0.as_str()).collect();
            assert_eq!(actual, vec![expected; count]);
        }
        let mut gauges = app.world_mut().query::<(&Fill, &MeshMaterial2d<UiMaterial>)>();
        let wisdom: Vec<_> = gauges.iter(app.world()).filter(|(f, _)| f.which == Gauge::Wisdom).collect();
        assert!(wisdom.len() >= 2, "both the HUD and talents wisdom bars must be dynamic");
        for (_, handle) in wisdom {
            let clip = app.world().resource::<Assets<UiMaterial>>().get(&handle.0).unwrap().clip;
            assert_eq!(clip.x, clip.z, "zero wisdom must have an empty progress bar");
        }
        // Late-arriving layouts must receive defaults without a stats update.
        let late = app.world_mut().spawn((Value(ValueKind::WisdomPoints), Text2d::new("Available Points: 19"))).id();
        app.update();
        assert_eq!(app.world().get::<Text2d>(late).unwrap().0, "Available Points: 0");
    }

    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn inventory_pointer_split_cancellation_and_collection_placeholders() {
        let (mut app, primary, roots, layouts) = super::super::tests::exported_app();
        app.init_resource::<Time<Real>>()
            .init_resource::<Assets<Catalog>>()
            .add_systems(Update, (sync, input).chain().after(edit_text).before(paint));
        crate::network::net::start_offline(app.world_mut(), 1, 0);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/interface/inventory.items.json");
        let catalog: Catalog = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let ingredient_template = catalog.ingredients.iter().find(|i| i.tab == "Basics" && i.rarity == 1 && i.slot == 0).unwrap().template.clone();
        let handle = app.world_mut().resource_mut::<Assets<Catalog>>().add(catalog);
        app.insert_resource(Data { catalog: handle.clone(), tooltip: default() });
        let mut net = app.world_mut().non_send_mut::<crate::network::net::Net>();
        net.offline = false;
        net.inventory.apply(InventoryInfo {
            items: vec![ItemInfo { id: 1, third: 10, template: ingredient_template.clone(), ..default() },
                ItemInfo { id: 2, third: 23, template: ingredient_template, ..default() }],
            placements: vec![(1, 0)], words_180: vec![2], storage_sizes: [0, 0, 224, 0, 0, 0], ..default()
        });
        let inventory = roots["inventory"];
        let dialog = roots["unstackwindow"];
        *app.world_mut().get_mut::<Visibility>(inventory).unwrap() = Visibility::Inherited;
        app.update();
        assert_eq!(app.world().get::<WindowControls>(inventory).unwrap().bag_pages, 8);
        let cursor = |app: &mut App, cell| {
            let c = app.world().get::<WindowControls>(inventory).unwrap();
            let b = c.buttons.iter().find(|b| b.action == Action::Item(Cell::Bag(cell))).unwrap();
            let p = Vec2::new((b.rect[0] + b.rect[2]) * 0.5, (b.rect[1] + b.rect[3]) * 0.5);
            app.world_mut().get_mut::<Window>(primary).unwrap().set_cursor_position(Some(p));
        };
        cursor(&mut app, 0);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert_eq!(*app.world().get::<Visibility>(dialog).unwrap(), Visibility::Inherited);
        assert_eq!(amount(app.world().get::<WindowControls>(dialog).unwrap()), Some(5));
        assert_eq!(app.world().resource::<Interaction>().split.as_ref().unwrap().target, Place::Bag(1));
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
        app.update();
        assert_eq!(*app.world().get::<Visibility>(dialog).unwrap(), Visibility::Hidden);
        assert!(app.world().resource::<Interaction>().pending.is_some());
        assert_eq!(app.world().non_send::<crate::network::net::Net>().inventory.at(Place::Bag(0)).unwrap().third, 10,
            "wait for authoritative IDs and amounts, never fabricate split stacks");
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        app.world_mut().non_send_mut::<crate::network::net::Net>().inventory.revision += 1;
        cursor(&mut app, 0);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        cursor(&mut app, 2);
        app.update();
        assert!(app.world().resource::<Interaction>().drag.as_ref().is_some_and(|d| d.active));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<Interaction>().drag.is_none());
        assert_eq!(*app.world().get::<Visibility>(inventory).unwrap(), Visibility::Inherited);
        let c = app.world().get::<WindowControls>(inventory).unwrap();
        let catalog = app.world().resource::<Assets<Catalog>>().get(&handle).unwrap();
        let state = &app.world().non_send::<crate::network::net::Net>().inventory;
        assert_eq!(displayed(Cell::Ingredient(0), c, state, catalog).unwrap().amount, 23);
        let empty = displayed(Cell::Ingredient(1), c, state, catalog).unwrap();
        assert_eq!(empty.amount, 0);
        assert!(empty.item.is_none());
        assert!(preview::card(&empty, catalog, None, false).height() > 32.0);

        let layout = &layouts["inventory"];
        assert_eq!(layout.widgets.iter().filter(|w| matches!(cell(w), Some(Cell::Bag(_)))).count(), 28);
        let background = layout.widgets.iter().position(|w| w.id.ends_with("/IngredientBag/Label_bg/Artwork")).unwrap();
        for tier in 1..=5 {
            let n = layout.widgets.iter().position(|w| w.id.ends_with(&format!("/Button_Rarity{tier}/normal"))).unwrap();
            assert!(background < n, "ingredient background must be painted before tier selectors");
            assert!(!layout.widgets[n].hidden && layout.widgets[n].art.is_some());
        }
        assert!(layout.widgets.iter().filter(|w| w.id.contains("/icon_area_cores/") || w.id.ends_with("/RecipesInfotextArea")).all(|w|w.hidden));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
        {
            let mut c=app.world_mut().get_mut::<WindowControls>(inventory).unwrap();
            c.bag=7;
            assert_eq!(place(Cell::Bag(27),&c),Some(Place::Bag(223)));
            assert!(c.section_visible(Section::BagFourthRow));
            assert!(!c.section_visible(Section::OverflowFooter));
            assert!(!c.enabled(c.buttons.iter().find(|b|b.action==Action::Bag(EXTRA)).unwrap()));
        }
        {
            let mut net=app.world_mut().non_send_mut::<crate::network::net::Net>();
            net.inventory.membership(2,true);
            net.inventory.allocation.storage_sizes[5]=28;
        }
        app.update();
        {
            let mut c=app.world_mut().get_mut::<WindowControls>(inventory).unwrap();
            assert!(c.enabled(c.buttons.iter().find(|b|b.action==Action::Bag(EXTRA)).unwrap()));
            c.bag=EXTRA;
            assert_eq!(place(Cell::Bag(27),&c),Some(Place::Extra(27)));
        }
        app.world_mut().non_send_mut::<crate::network::net::Net>().inventory.membership(2,false);
        app.update();
        assert_eq!(app.world().get::<WindowControls>(inventory).unwrap().bag,0);
        {
            let mut net=app.world_mut().non_send_mut::<crate::network::net::Net>();
            net.inventory.allocation.placements.clear();
            net.inventory.allocation.overflow=(1..=28).collect();
            net.inventory.revision+=1;
        }
        app.world_mut().get_mut::<WindowControls>(inventory).unwrap().bag=OVERFLOW;
        cursor(&mut app,0);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<Interaction>().drag.is_none());
        assert!(app.world().resource::<Interaction>().split.is_none());
        let mut c=app.world_mut().get_mut::<WindowControls>(inventory).unwrap();
        assert_eq!(c.overflow_pages,2);
        assert!(c.section_visible(Section::OverflowFooter));
        assert!(!c.section_visible(Section::BagFourthRow));
        c.overflow_page=1;
        assert_eq!(place(Cell::Bag(0),&c),Some(Place::Overflow(21)));
    }

    #[test]
    fn pets_are_used_by_a_click_and_worn_slots_still_glow_for_a_dragged_piece() {
        let catalog: Catalog = serde_json::from_value(serde_json::json!({
            "items": {
                "pet_epic": {"name":"Mortis Doll","description":"","category":"Pet","icon":"p.png","max_stack":1,"required_level":1,"slots":[],
                    "class_name":"","rarity":"Epic","icon_config":"item_pet","item_category":"Pet"},
                "gloves": {"name":"Gloves","description":"","category":"Gloves","icon":"g.png","max_stack":1,"required_level":1,"slots":[3],
                    "class_name":"warrior","rarity":"Common","item_category":"Armor"}
            },
            "ingredients": [], "colors": {},
            "icon_configs": {"item_pet": {"rarity": true, "background": "", "frame": "f.png"}},
            "rarity_backgrounds": ["c.png", "u.png", "m.png", "r.png", "e.png"]
        })).unwrap();
        // Without an item (the album, shops) the template's rarity; a carried one its own.
        assert_eq!(catalog.icon_parts("pet_epic", None).1, Some("e.png"));
        assert_eq!(catalog.icon_parts("pet_epic", Some(&ItemInfo { kind: 2, template: "pet_epic".into(), ..default() })).1, Some("m.png"));
        let mut state = InventoryState::default();
        state.apply(InventoryInfo {
            items: vec![ItemInfo { id: 1, third: 1, template: "pet_epic".into(), ..default() }, ItemInfo { id: 2, third: 1, template: "gloves".into(), ..default() },
                ItemInfo { id: 3, third: 1, template: "gloves".into(), ..default() }],
            placements: vec![(1, 0), (2, 1)], storage_sizes: [0, 0, 224, 0, 0, 0], ..default()
        });
        state.allocation.slots.push((3, vec![3]));
        // A right click, or a double click, uses (summons) a pet; gloves are not used.
        let used = Some(crate::gameplay::inventory::command(1, dsor_proto::commands::inventory::Inventory::USE, vec![]));
        assert_eq!(format!("{:?}", equip_toggle(&state, &catalog, 1, 1, "warrior")), format!("{used:?}"));
        assert_eq!(format!("{:?}", use_consumable(&state, &catalog, 1)), format!("{used:?}"));
        assert!(use_consumable(&state, &catalog, 2).is_none());
        // Dragging the second gloves: their slot glows though gloves are worn; not for a mage, not before the drag starts.
        let drag = |active| Interaction { drag: Some(DragItem { id: 2, revision: 0, origin: Vec2::ZERO, active, root: Entity::PLACEHOLDER }), ..default() };
        assert_eq!(drop_targets(&drag(true), &state, &catalog, 1, 0), 1 << 3);
        assert_eq!(drop_targets(&drag(true), &state, &catalog, 1, 1), 0);
        assert_eq!(drop_targets(&drag(false), &state, &catalog, 1, 0), 0);
        // The gloves dragged leave their cell empty once the drag is under way; the others stay.
        let (gloves, other) = (state.items.get(&2), state.items.get(&3));
        assert_eq!((drag(true).lifted(), drag(false).lifted()), (Some(2), None));
        assert!(!drawn(Cell::Bag(1), gloves, drag(true).lifted()));
        assert!(drawn(Cell::Bag(1), gloves, drag(false).lifted()));
        assert!(drawn(Cell::Equipment(3), other, drag(true).lifted()));
        assert!(drawn(Cell::Ingredient(0), gloves, drag(true).lifted()));
    }

    #[test]
    fn the_drop_highlight_is_the_layout_s_own_spread_over_the_unit_square() {
        let mut layout = crate::ui::controls::template::fixture::layout("inventory", &[(DROP_HIGHLIGHT, "Slot", false)]);
        let w = &mut layout.widgets[0];
        w.rect = [100.0, 200.0, 170.0, 270.0];
        // Its corners and a vertex of its line (15 % inside), as the layout's.
        w.art = Some(Art { tex: Some("textures/ui/deco_elemente.png".into()), pos: vec![100.0, 200.0, 170.0, 270.0, 110.5, 210.5],
            uv: vec![0.9; 6], color: vec![1.0; 12], idx: vec![0, 1, 2] });
        let (mesh, tex) = drop_highlight(&layout).unwrap();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        assert_eq!(pos, &vec![[0.0, 0.0, 0.0], [1.0, -1.0, 0.0], [0.15, -0.15, 0.0]]);
        assert_eq!(tex.as_deref(), Some("textures/ui/deco_elemente.png"));
        assert!(drop_highlight(&crate::ui::controls::template::fixture::layout("inventory", &[])).is_none());
    }

    #[test]
    fn equipped_levels_sit_in_the_outer_top_corner_of_the_slots_that_show_one() {
        let rect = [100.0, 200.0, 150.0, 250.0];
        let near = |slot: i8, at: Vec2| level_centre(slot, rect).is_some_and(|c| c.distance(at) < 1e-4);
        // The right column (helmet to boots, the off hand): top right.
        for slot in [0, 1, 2, 3, 4, 6] { assert!(near(slot, Vec2::new(143.6, 204.8)), "slot {slot}"); }
        // The left column (weapon, cloak, rings, amulet, weapon mod): top left.
        for slot in [5, 7, 8, 9, 10, 12] { assert!(near(slot, Vec2::new(106.4, 204.8)), "slot {slot}"); }
        // No belt, trinkets, mercenary pieces or essences.
        for slot in [11, 13, 20, 24, 25, 27, 34, 35] { assert_eq!(level_centre(slot, rect), None, "slot {slot}"); }
        // The face: the Green template's normal state, at its size, with its font size.
        let id = "Canvas/UpgradeButtonLabel/UpgradeButtonLabel/UpgradeButtonTemplate_Green/normal";
        let mut layout = crate::ui::controls::template::fixture::layout("inventory",
            &[("Canvas/UpgradeButtonLabel/UpgradeButtonLabel/UpgradeButtonTemplate_Red/normal", "TextLabel", true), (id, "TextLabel", true)]);
        layout.widgets[0].rect = [0.0, 0.0, 1.0, 1.0];
        let (_, _, size, font) = level_face(&layout).unwrap();
        assert_eq!((size, font), (Vec2::new(90.0, 46.0), 10.0 * FONT_SCALE));
        assert!(level_face(&crate::ui::controls::template::fixture::layout("inventory", &[])).is_none());
    }
}
