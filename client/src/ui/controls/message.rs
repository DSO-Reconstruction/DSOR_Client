//! The notice under the top navigation when an item is received (a quest's
//! reward, the daily login chest, a chest opened...): the item's icon and
//! "Daily Login Chest received" (messagewindow's ItemMessageLine), one notice
//! at a time, each for _Globals InfoTextDuration (4 s).
//!
//! EVIDENCE: release261 dro_client64, UI::MessageWidget (messagewidget.cc):
//!   - Load (0x1404860A0) takes messagewindow's seven lines; ItemMessageLine
//!     (msgElement7) starts hidden, in its authored colour and size (the
//!     info lines take InfoTextColor and InfoTextSize); every line stays
//!     InfoTextDuration (0x1413765A0, registered at 0x1400776E0; 3.5 s until
//!     read).
//!   - AddIconMessage (0x140485104; the message manager's channel 1 through
//!     DrasaUIManager 0x1403EBDC8) appends {text, icon} to a queue
//!     (Array<IconMessageEntry>), never capped.
//!   - Refresh (0x140486D00), each frame: while the line is empty, the first
//!     entry's text goes on ItemMessageLine, its icon on ItemLabel (the Icon in
//!     ItemSlot), the line shows and the entry leaves the queue; once
//!     InfoTextDuration has passed, the line empties and hides.
//!   - The text (0x1405E53EC, 0x1405E4B74): gui.window lootReceived "{0:s}
//!     received", lootReceivedAmount "{0:i} {0:s} received" when more than
//!     one; the carried item's name and icon, else its template's.
//! The message itself: crate::gameplay::inventory::Receipt.
//! DEVIATION: while the UI manager's flag +0x329 is set (not identified), the
//!   original puts the shown entry back at the front of the queue, hidden;
//!   here a notice always runs its time. The next notice shows on the frame the
//!   last one ends, not the frame after.
//! INFERRED: the notice takes no clicks (the world under it stays clickable);
//!   the original's hit testing of the line was not traced.
use std::collections::VecDeque;

use bevy::prelude::*;

use super::items;
use super::template::{icon_layers, set_text, Icon};
use crate::gameplay::inventory::Receipt;
use crate::network::net::Net;
use crate::ui::combat_text::{CombatTextTable, Texts};
use crate::ui::{Layout, Loaded, Window2018};

pub(in crate::ui) const WINDOW: &str = "messagewindow";
const LINE: &str = "Canvas/Label/InfoMessages/ItemMessageLine";
const ICON: &str = "Canvas/Label/InfoMessages/ItemMessageLine/ItemSlot/ItemLabel";
/// _Globals InfoTextDuration, seconds.
const DURATION: f64 = 4.0;
/// gui.window lootReceived and lootReceivedAmount (tools/export_combat_text.py).
const TEXTS: &str = "interface/combat_text.json";

/// What one notice shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Notice {
    pub text: String,
    pub icon: Icon,
}

impl Notice {
    /// None for a template the client does not know (the original's template
    /// lookup fails and it shows nothing).
    fn of(receipt: &Receipt, catalog: &items::Catalog, texts: &Texts) -> Option<Self> {
        let item = receipt.item.as_ref();
        if item.is_none() && !catalog.items.contains_key(&receipt.template) { return None; }
        let text = if receipt.amount > 1 { &texts.loot_amount } else { &texts.loot };
        let text = text.replace("{0:i}", &receipt.amount.to_string()).replace("{0:s}", catalog.name(&receipt.template, item));
        let (image, background, frame) = catalog.icon_parts(&receipt.template, item);
        let part = |p: Option<&str>| p.unwrap_or_default().to_owned();
        Some(Self { text, icon: Icon { background: part(background), image: part(image), frame: part(frame), ..Default::default() } })
    }
}

/// The notices waiting, and the one shown since when (real seconds).
#[derive(Resource, Default)]
pub(in crate::ui) struct ItemMessages {
    texts: Handle<CombatTextTable>,
    queue: VecDeque<Notice>,
    shown: Option<(Notice, f64)>,
}

pub(in crate::ui) fn setup(mut messages: ResMut<ItemMessages>, assets: Res<AssetServer>) {
    messages.texts = assets.load(TEXTS);
}

/// Takes the items received into the queue and puts the next notice up when
/// the line is free; rebuilds the window only when its notice changes.
#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn sync(
    mut commands: Commands,
    net: Option<NonSendMut<Net>>,
    time: Res<Time<Real>>,
    data: Res<items::Data>,
    catalogs: Res<Assets<items::Catalog>>,
    tables: Res<Assets<CombatTextTable>>,
    mut messages: ResMut<ItemMessages>,
    mut loaded: ResMut<Loaded>,
    layouts: Res<Assets<Layout>>,
    windows: Query<(Entity, &Window2018)>,
) {
    // Kept until the names can be given (the catalogs load at start).
    if let Some(mut net) = net.filter(|n| !n.receipts.is_empty()) {
        if let (Some(catalog), Some(table)) = (catalogs.get(&data.catalog), tables.get(&messages.texts)) {
            let notices: Vec<Notice> = net.receipts.drain(..).filter_map(|r| Notice::of(&r, catalog, &table.texts)).collect();
            messages.queue.extend(notices);
        }
    }
    let now = time.elapsed_secs_f64();
    let ended = messages.shown.as_ref().is_some_and(|(_, since)| now - since > DURATION);
    let free = ended || messages.shown.is_none();
    if !free || (!ended && messages.queue.is_empty()) { return; }
    let next = messages.queue.pop_front().map(|notice| (notice, now));
    messages.shown = next;
    for (entity, window) in &windows {
        if window.0 == WINDOW { commands.entity(entity).despawn_related::<Children>(); }
    }
    for i in 0..loaded.layouts.len() {
        if layouts.get(&loaded.layouts[i].0).is_some_and(|l| l.window == WINDOW) { loaded.spawned[i] = false; }
    }
}

/// The item line with the notice shown, its icon in ItemSlot; nothing when
/// none is (the other lines are other messages').
pub(in crate::ui) fn prepare(source: &Layout, messages: &ItemMessages) -> Layout {
    let mut layout = Layout { window: source.window.clone(), widgets: Vec::new() };
    let Some((notice, _)) = &messages.shown else { return layout };
    for widget in &source.widgets {
        let holds_line = LINE.strip_prefix(widget.id.as_str()).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
        let in_line = widget.id.strip_prefix(LINE).is_some_and(|rest| rest.starts_with('/'));
        if !holds_line && !in_line { continue; }
        let mut w = widget.clone();
        w.hidden = false;
        if w.id == LINE { set_text(&mut w, &notice.text); }
        if w.id == ICON {
            let layers = icon_layers(&mut w, &notice.icon);
            layout.widgets.push(w);
            layout.widgets.extend(layers);
            continue;
        }
        layout.widgets.push(w);
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::controls::template::fixture::{find, text_of};
    use crate::ui::UiWork;
    use bevy::time::TimeUpdateStrategy;
    use dsor_proto::commands::inventory::ItemInfo;
    use std::time::Duration;

    fn catalog() -> items::Catalog {
        let item = |name: &str, icon: &str, config: &str| serde_json::json!({ "name": name, "description": "", "category": "", "icon": icon,
            "max_stack": 1, "required_level": 0, "slots": [], "class_name": "", "rarity": "Common", "icon_config": config });
        serde_json::from_value(serde_json::json!({
            "items": { "dailylogin_lootbox_day_01": item("Daily Login Chest", "textures/icons/all_chest_giftbox_01.png", "item_lootgen"),
                "money_rc_no_auto_use": item("Andermant", "textures/icons/money_rc_no_auto_use.png", "item_currency") },
            "ingredients": [], "colors": {},
            "icon_configs": { "item_lootgen": { "rarity": true, "background": "", "frame": "textures/icons_toolkit/frame_slim_basic.png" },
                "item_currency": { "rarity": false, "background": "textures/icons_toolkit/bg_currency.png", "frame": "textures/icons_toolkit/frame_slim_basic.png" } },
            "rarity_backgrounds": ["textures/icons_toolkit/bg_rarity_common.png", "textures/icons_toolkit/bg_rarity_uncommon.png"],
        })).unwrap()
    }

    fn texts() -> CombatTextTable {
        serde_json::from_value(serde_json::json!({ "types": {}, "texts": { "block": "", "critical": "", "immunity": "", "low_health": "",
            "loot": "{0:s} received", "loot_amount": "{0:i} {0:s} received" } })).unwrap()
    }

    /// messagewindow as exported: the containers, the seven lines, the slot and
    /// its Icon (no artwork of their own).
    fn layout() -> Layout {
        let widget = |id: &str, rect: [f32; 4], text: bool| serde_json::json!({ "id": id, "type": "TextLabel", "k": 0.5, "rect": rect, "hidden": false,
            "text": if text { serde_json::json!({"text": "", "key": "", "size": 10.0, "color": [1.0, 0.79, 0.5, 1.0], "align": [0, 1], "word_break": true}) } else { serde_json::Value::Null } });
        let lines = ["FirstMessageLine", "SecondMessageLine", "ThirdMessageLine", "StatusMessageLine", "SystemMessageLine", "InfoMessageLine"];
        let mut widgets = vec![widget("Canvas", [314.5, 62.7, 709.5, 419.9], false), widget("Canvas/Label", [314.5, 62.7, 709.5, 419.9], false),
            widget("Canvas/Label/InfoMessages", [314.5, 62.7, 709.5, 419.9], false)];
        widgets.extend(lines.iter().map(|l| widget(&format!("Canvas/Label/InfoMessages/{l}"), [318.0, 149.0, 706.0, 173.0], true)));
        widgets.extend([widget(LINE, [427.7, 92.4, 671.8, 138.7], true), widget(&format!("{LINE}/ItemSlot"), [369.0, 89.5, 421.7, 142.2], false),
            widget(ICON, [369.0, 89.5, 421.7, 142.2], true)]);
        serde_json::from_value(serde_json::json!({ "window": WINDOW, "widgets": widgets })).unwrap()
    }

    /// The carried item's name and look; "N name received" past one; a
    /// currency without an item by its template; an unknown template nothing.
    #[test]
    fn notices_name_the_item_and_its_amount() {
        let (catalog, texts) = (catalog(), texts().texts);
        let chest = ItemInfo { id: 41, template: "dailylogin_lootbox_day_01".into(), kind: 1, ..Default::default() };
        let notice = Notice::of(&Receipt { amount: 1, template: chest.template.clone(), item: Some(chest) }, &catalog, &texts).unwrap();
        assert_eq!(notice.text, "Daily Login Chest received");
        assert_eq!(notice.icon, Icon { background: "textures/icons_toolkit/bg_rarity_uncommon.png".into(),
            image: "textures/icons/all_chest_giftbox_01.png".into(), frame: "textures/icons_toolkit/frame_slim_basic.png".into(), ..Default::default() });
        let andermant = Notice::of(&Receipt { amount: 25, template: "money_rc_no_auto_use".into(), item: None }, &catalog, &texts).unwrap();
        assert_eq!(andermant.text, "25 Andermant received");
        assert_eq!(andermant.icon.background, "textures/icons_toolkit/bg_currency.png");
        assert_eq!(Notice::of(&Receipt { amount: 1, template: "gone".into(), item: None }, &catalog, &texts), None);
    }

    /// Only the item line, its text set, the icon's layers in the slot; nothing
    /// without a notice.
    #[test]
    fn the_window_holds_the_item_line_alone() {
        let mut messages = ItemMessages::default();
        assert!(prepare(&layout(), &messages).widgets.is_empty());
        let icon = Icon { background: "bg.png".into(), image: "chest.png".into(), frame: "frame.png".into(), ..Default::default() };
        messages.shown = Some((Notice { text: "Daily Login Chest received".into(), icon }, 0.0));
        let prepared = prepare(&layout(), &messages);
        assert_eq!(prepared.widgets.iter().map(|w| w.id.rsplit('/').next().unwrap()).collect::<Vec<_>>(),
            ["Canvas", "Label", "InfoMessages", "ItemMessageLine", "ItemSlot", "ItemLabel", "Background", "Image", "Frame"]);
        assert_eq!(text_of(&prepared, LINE), "Daily Login Chest received");
        let image = find(&prepared, &format!("{ICON}/Image"));
        assert_eq!((image.art.as_ref().unwrap().tex.as_deref(), image.rect), (Some("chest.png"), [369.0, 89.5, 421.7, 142.2]));
        assert!(find(&prepared, ICON).art.is_none());
    }

    /// One notice at a time, each for InfoTextDuration, in the order received;
    /// the window is built for each change and never on idle frames.
    #[test]
    fn notices_follow_one_another_and_idle_frames_build_nothing() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(500)))
            .init_asset::<Layout>().init_asset::<Mesh>().init_asset::<crate::ui::UiMaterial>().init_asset::<Image>().init_asset::<Font>()
            .init_asset::<crate::ui::dialogue::Catalog>().init_asset::<items::Catalog>().init_asset::<CombatTextTable>()
            .init_resource::<Loaded>().init_resource::<crate::ui::Screen>().init_resource::<crate::ui::dialogue::Presentation>()
            .init_resource::<super::super::party::Party>().init_resource::<super::super::quests::Quests>().init_resource::<crate::gameplay::quests::QuestUi>()
            .init_resource::<super::super::systems::Views>().init_resource::<super::super::currency::Shown>().init_resource::<UiWork>()
            .init_resource::<ItemMessages>()
            .add_systems(Update, (sync, crate::ui::spawn_windows).chain());
        crate::network::net::start_offline(app.world_mut(), 1, 0);
        let catalog = app.world_mut().resource_mut::<Assets<items::Catalog>>().add(catalog());
        app.insert_resource(items::Data { catalog, tooltip: default() });
        app.world_mut().resource_mut::<ItemMessages>().texts = app.world_mut().resource_mut::<Assets<CombatTextTable>>().add(texts());
        let handle = app.world_mut().resource_mut::<Assets<Layout>>().add(layout());
        app.world_mut().resource_mut::<Loaded>().layouts.push((handle, 62.0, None));
        app.world_mut().resource_mut::<Loaded>().spawned.push(false);
        let prepared = |app: &App| app.world().resource::<UiWork>().layouts_prepared;
        let shown = |app: &App| app.world().resource::<ItemMessages>().shown.as_ref().map(|(n, _)| n.text.clone());
        app.update();
        assert_eq!((prepared(&app), shown(&app)), (1, None), "nothing to show: an empty window");
        for _ in 0..5 { app.update(); }
        assert_eq!(prepared(&app), 1, "idle frames build nothing");

        let receipt = |amount, template: &str| Receipt { amount, template: template.into(), item: None };
        app.world_mut().non_send_mut::<Net>().receipts.extend([receipt(1, "dailylogin_lootbox_day_01"), receipt(1, "unknown"), receipt(3, "money_rc_no_auto_use")]);
        app.update();
        assert!(app.world().non_send::<Net>().receipts.is_empty());
        assert_eq!((prepared(&app), shown(&app).as_deref()), (2, Some("Daily Login Chest received")));
        let texts: Vec<String> = app.world_mut().query::<&Text2d>().iter(app.world()).map(|t| t.0.clone()).collect();
        assert!(texts.iter().any(|t| t == "Daily Login Chest received"), "the line is drawn: {texts:?}");
        // Shown for 4 s (a frame is 0.5 s here), then the next one at once.
        for _ in 0..8 { app.update(); }
        assert_eq!((prepared(&app), shown(&app).as_deref()), (2, Some("Daily Login Chest received")));
        app.update();
        assert_eq!((prepared(&app), shown(&app).as_deref()), (3, Some("3 Andermant received")));
        for _ in 0..9 { app.update(); }
        assert_eq!((prepared(&app), shown(&app)), (4, None), "the last one ends: the window empties");
        let texts: Vec<String> = app.world_mut().query::<&Text2d>().iter(app.world()).map(|t| t.0.clone()).collect();
        assert!(texts.is_empty(), "nothing left on screen: {texts:?}");
        for _ in 0..5 { app.update(); }
        assert_eq!(prepared(&app), 4, "idle frames build nothing");
    }
}
