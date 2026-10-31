use super::*;
use crate::network::net::{Net, start_offline};
use dsor_proto::commands::inventory::{Inventory, InventoryInfo};

fn catalog() -> Catalog {
    let mut entries = serde_json::Map::new();
    for (name, category, slots) in [
        ("essence", "Ammunition", vec![35, 34]), ("helmet", "Armor", vec![0]),
        ("potion", "Consumable", vec![37]), ("elixir", "Consumable", vec![]),
        ("instant", "InstantConsumable", vec![]), ("mount", "Mount", vec![]),
    ] {
        entries.insert(name.into(), serde_json::json!({"name":name,"description":"","category":category,
            "item_category":category,"icon":"","max_stack":999,"required_level":1,"slots":slots,"class_name":"any","rarity":""}));
    }
    serde_json::from_value(serde_json::json!({"items":entries,"ingredients":[],"colors":{}})).unwrap()
}

fn controls(name: &str, ids: &[(&str, &str)]) -> WindowControls {
    let widgets: Vec<_> = ids.iter().enumerate().map(|(i,(id,kind))| serde_json::json!({
        "id":id,"type":kind,"k":0.0,"rect":[i as f32 * 40.,0.,i as f32 * 40.+30.,30.],"hidden":false
    })).collect();
    WindowControls::new(&serde_json::from_value(serde_json::json!({"window":name,"widgets":widgets})).unwrap())
}

fn app() -> (App, Entity, Entity, Entity) {
    let mut app = App::new();
    app.init_resource::<Interaction>().init_resource::<Pointer>().init_resource::<Time<Real>>()
        .init_resource::<Assets<Catalog>>().init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>().init_resource::<super::super::trade::TradeUi>().init_resource::<super::super::hotbar::ItemCooldowns>()
        .init_resource::<crate::ui::input::UiInput>()
        .add_systems(Update, input);
    let handle = app.world_mut().resource_mut::<Assets<Catalog>>().add(catalog());
    app.insert_resource(Data { catalog: handle, tooltip: default() });
    start_offline(app.world_mut(), 1, 0);
    app.world_mut().non_send_mut::<Net>().offline = false;
    let window = app.world_mut().spawn((Window { focused:true, resolution:(1024,768).into(), ..default() }, PrimaryWindow)).id();
    let inventory = app.world_mut().spawn((Window2018("inventory".into()),
        controls("inventory", &[("Canvas/TemplateSlot#0_0","Slot"),("Canvas/TemplateSlot#0_1","Slot")]),
        Visibility::Inherited,Transform::default())).id();
    let bar = app.world_mut().spawn((Window2018("bottombar".into()),
        controls("bottombar", &[("Canvas/Ammo","SlotButton"),("Canvas/AmmoInactive","SlotButton")]),
        Visibility::Inherited,Transform::default())).id();
    (app, window, inventory, bar)
}

fn snapshot(app: &mut App, template: &str, place: Place) {
    let mut allocation = InventoryInfo { items:vec![ItemInfo { id:1, template:template.into(), third:20, ..default() }],
        storage_sizes:[0,0,28,0,0,0], ..default() };
    match place {
        Place::Bag(cell) => allocation.placements.push((1,cell)),
        Place::Equipment(slot) => allocation.slots.push((1,vec![slot])),
        _ => unreachable!(),
    }
    app.world_mut().non_send_mut::<Net>().inventory.apply(allocation);
}

fn hover(app: &mut App, window: Entity, root: Entity, index: usize, x: f32) {
    let mut windows = app.world_mut().query::<&mut WindowControls>();
    for mut c in windows.iter_mut(app.world_mut()) { c.hovered = None; }
    app.world_mut().get_mut::<WindowControls>(root).unwrap().hovered = Some(index);
    app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(Vec2::new(x,10.)));
}

#[test]
fn essence_drags_round_trip_through_both_action_bar_sockets_without_double_clicks() {
    for (index, slot) in [(0,35),(1,34)] {
        let (mut app, window, inventory, bar) = app();
        snapshot(&mut app,"essence",Place::Equipment(slot));
        for (from, to, from_index, to_index, destination) in [
            (bar,inventory,index,0,Place::Bag(0)),
            (inventory,bar,0,index,Place::Equipment(slot)),
            (bar,inventory,index,1,Place::Bag(1)),
        ] {
            hover(&mut app,window,from,from_index,10.);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            app.update();
            assert!(app.world().resource::<Interaction>().drag.is_some(), "slot {slot}: drag must start even immediately after a previous drag");
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            hover(&mut app,window,to,to_index,100.);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            app.update();
            assert!(app.world().resource::<Interaction>().pending.is_some(), "slot {slot}: drop must send a move");
            assert!(app.world().resource::<Interaction>().drag.is_none());
            assert!(app.world().non_send::<Net>().inventory.at(destination).is_none(), "wait for the server snapshot");
            snapshot(&mut app,"essence",destination);
            assert_eq!(app.world().non_send::<Net>().inventory.location(1),Some(destination));
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        }
    }
}

#[test]
fn equipment_cannot_be_dropped_into_an_essence_socket() {
    let (mut app,window,inventory,bar)=app();
    snapshot(&mut app,"helmet",Place::Bag(0));
    hover(&mut app,window,inventory,0,10.);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    app.update();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    hover(&mut app,window,bar,0,100.);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    app.update();
    assert!(app.world().resource::<Interaction>().pending.is_none());
}

#[test]
fn inventory_right_click_only_acts_on_usable_items_even_while_trading() {
    for trading in [false,true] {
        // A mount is used by a right click too, as in the original (the user, 2026-10-23).
        for (template,usable) in [("potion",true),("elixir",true),("instant",true),("essence",false),("helmet",false),("mount",true)] {
            let (mut app,window,inventory,_)=app();
            snapshot(&mut app,template,Place::Bag(0));
            if trading {
                app.world_mut().spawn((Window2018(super::super::trade::WINDOW.into()),controls(super::super::trade::WINDOW,&[]),Visibility::Inherited,Transform::default()));
            }
            hover(&mut app,window,inventory,0,10.);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
            app.update();
            assert_eq!(app.world().resource::<Interaction>().pending.is_some(), usable && !trading, "{template}, trading={trading}");
            assert_eq!(!app.world().resource::<super::super::trade::TradeUi>().add.is_empty(), usable && trading);
            assert!(app.world().non_send::<Net>().inventory.at(Place::Bag(0)).is_some());
        }
    }
}

#[test]
fn consumable_use_sends_use_instead_of_equipping_and_rejects_non_carried_items() {
    let catalog=catalog();
    let mut state=InventoryState::default();
    state.apply(InventoryInfo { items:vec![ItemInfo { id:1,template:"potion".into(),third:2,..default() }],
        placements:vec![(1,0)],storage_sizes:[0,0,28,0,0,0],..default() });
    assert_eq!(use_consumable(&state,&catalog,1),Some(crate::gameplay::inventory::command(1,Inventory::USE,vec![])));
    state.allocation.placements.clear();
    state.allocation.overflow.push(1);
    assert!(use_consumable(&state,&catalog,1).is_none());
    state.allocation.overflow.clear();
    state.allocation.slots.push((1,vec![37]));
    assert!(use_consumable(&state,&catalog,1).is_none());
}

/// Drags the hovered cell of `from` and lets go away from every item cell, over
/// a window or over the world.
fn drop_outside(app: &mut App, window: Entity, from: Entity, index: usize, over_ui: bool) {
    hover(app,window,from,index,10.);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    app.update();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    let mut windows = app.world_mut().query::<&mut WindowControls>();
    for mut c in windows.iter_mut(app.world_mut()) { c.hovered = None; }
    app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(Vec2::new(500.,400.)));
    app.world_mut().resource_mut::<crate::ui::input::UiInput>().over_ui = over_ui;
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    app.update();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
}

fn destroy_dialog(app: &mut App) -> Entity {
    app.world_mut().spawn((Window2018(DESTROY_DIALOG.into()),controls(DESTROY_DIALOG,&[]),Visibility::Hidden,Transform::default())).id()
}

#[test]
fn an_item_dropped_on_the_world_is_destroyed_only_once_confirmed_and_by_the_server() {
    for confirm in [true,false] {
        let (mut app,window,inventory,_)=app();
        let dialog=destroy_dialog(&mut app);
        snapshot(&mut app,"helmet",Place::Bag(0));
        drop_outside(&mut app,window,inventory,0,false);
        assert_eq!(*app.world().get::<Visibility>(dialog).unwrap(),Visibility::Inherited,"dropped on the world, the dialog asks");
        assert_eq!(app.world().resource::<Interaction>().destroy,Some(1));
        assert!(app.world().resource::<Interaction>().pending.is_none(),"nothing is sent before the answer");
        app.world_mut().resource_mut::<Pointer>().destroy=Some(confirm);
        app.update();
        assert_eq!(*app.world().get::<Visibility>(dialog).unwrap(),Visibility::Hidden);
        assert_eq!(app.world().resource::<Interaction>().pending.is_some(),confirm,"confirm={confirm}");
        assert!(app.world().resource::<Interaction>().destroy.is_none());
        assert!(app.world().non_send::<Net>().inventory.at(Place::Bag(0)).is_some(),"wait for the server's inventory");
    }
}

#[test]
fn dropping_over_a_window_or_an_equipped_item_never_asks_to_destroy() {
    let (mut app,window,inventory,_)=app();
    let dialog=destroy_dialog(&mut app);
    snapshot(&mut app,"helmet",Place::Bag(0));
    drop_outside(&mut app,window,inventory,0,true);
    assert_eq!(*app.world().get::<Visibility>(dialog).unwrap(),Visibility::Hidden,"over a window");
    let (mut app,window,_,bar)=self::app();
    let dialog=destroy_dialog(&mut app);
    snapshot(&mut app,"essence",Place::Equipment(35));
    drop_outside(&mut app,window,bar,0,false);
    assert_eq!(*app.world().get::<Visibility>(dialog).unwrap(),Visibility::Hidden,"equipped");
    assert!(app.world().resource::<Interaction>().pending.is_none());
}

#[test]
fn the_destroy_dialog_closes_when_its_item_leaves_the_bags() {
    let (mut app,window,inventory,_)=app();
    let dialog=destroy_dialog(&mut app);
    snapshot(&mut app,"helmet",Place::Bag(0));
    drop_outside(&mut app,window,inventory,0,false);
    assert_eq!(*app.world().get::<Visibility>(dialog).unwrap(),Visibility::Inherited);
    snapshot(&mut app,"helmet",Place::Equipment(0));
    app.update();
    assert_eq!(*app.world().get::<Visibility>(dialog).unwrap(),Visibility::Hidden);
    assert!(app.world().resource::<Interaction>().pending.is_none());
}

#[test]
fn the_confirmation_dialog_shows_one_centred_layer_by_its_page() {
    use crate::ui::controls::consent::{DESTROY, PAGES, SPEND};
    let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/interface/confirmdialog.ui.json");
    let Ok(bytes)=std::fs::read(path) else { return };
    let layout=prepare_confirm(&serde_json::from_slice(&bytes).unwrap());
    let shown=|id:&str|layout.widgets.iter().any(|w|w.id==id && !w.hidden);
    let spend="Canvas/DialogCenter/Content/DefaultDoNotShowAgain";
    assert!(shown(&format!("{DELETE_ITEM}/ItemSlot")));
    assert!(shown(&format!("{spend}/CheckBox/pressed")) && shown(&format!("{spend}/CheckBoxText")));
    assert!(shown("Canvas/DialogCenter/Confirm") && shown("Canvas/DialogCenter/Cancel"));
    assert!(!shown(&format!("{DELETE_ITEM}/Question")) && !shown(&format!("{spend}/Question")),"the questions are written at runtime");
    assert!(!shown("Canvas/DialogCenter/Content/Default/Question"));
    assert!(!shown("Canvas/DialogCenter/Content/DeleteItemDoNotShowAgain/CheckBox"));
    assert!(layout.widgets.iter().all(|w|!w.id.starts_with("Canvas/DialogBottom")));
    // One layer at a time: the item to destroy, or the purchase.
    let mut c=WindowControls::new(&layout);
    let visible=|c:&WindowControls,id:&str|c.visible(&c.gate(DESTROY_DIALOG,layout.widgets.iter().find(|w|w.id==id).unwrap()));
    c.set_page(PAGES,DESTROY);
    assert!(visible(&c,&format!("{DELETE_ITEM}/ItemSlot")) && !visible(&c,&format!("{spend}/CheckBoxText")));
    c.set_page(PAGES,SPEND);
    assert!(!visible(&c,&format!("{DELETE_ITEM}/ItemSlot")) && visible(&c,&format!("{spend}/CheckBoxText")));
    assert!(visible(&c,"Canvas/DialogCenter/Confirm"));
}

#[test]
fn unidentified_equipment_is_not_worn_until_identified() {
    let (mut app,..)=app();
    snapshot(&mut app,"helmet",Place::Bag(0));
    let catalog=catalog();
    let mut net=app.world_mut().non_send_mut::<Net>();
    net.inventory.items.get_mut(&1).unwrap().flag_0=true;
    assert!(equip_toggle(&net.inventory,&catalog,1,10,"warrior").is_none());
    net.inventory.items.get_mut(&1).unwrap().flag_0=false;
    assert!(equip_toggle(&net.inventory,&catalog,1,10,"warrior").is_some());
}
