//! One click-through hover layer for navigation, skills and populated HUD slots.
use super::*;
pub(super) mod card;
use card::{Card, WHITE, BLUE};

#[derive(Component)]
pub(in crate::ui) struct Tooltip;

fn title(id: &str) -> String {
    let mut out = String::new();
    for (i,c) in id.chars().enumerate() {
        if c == '_' { out.push(' '); }
        else { if i > 0 && c.is_uppercase() { out.push(' '); } out.push(c); }
    }
    out.trim().to_owned()
}

fn window_title(name: &str) -> &str {
    match name { "skilltalentswindow" => "Skills", "charactersheet" => "Character", "collectorsalbum" => "Collector's album",
        "questlog" => "Quests", "locationmap" => "Map overlay", "travelmap" => "World map", "socialscreen" => "Social",
        "shopwindow" => "Shop", "matchmakingwindow" => "Battlegrounds", "gamemenu" => "Game menu", "leaderboards" => "Rankings",
        "eventwindow" => "Events", "inventory" => "Inventory", "currencybag" => "Currency Bag", _ => name }
}

/// The social window's tabs and icon buttons (gui.window.social_*TabHeading,
/// *Button, guildChangeName).
fn social_button(window: &str, leaf: &str) -> Option<&'static str> {
    if window != "socialscreen" { return None; }
    Some(match leaf {
        "TabButtonSearch" => "Player Search", "TabButtonFriends" => "Friends", "TabButtonGroup" => "Group Overview",
        "TabButtonGuild" => "Guild", "TabButtonIgnore" => "Ignore", "TabButtonInvite" => "Invite friends",
        "ButtonLeaveGroup" => "Leave group", "ButtonInviteNearby" => "Invite nearby players", "ButtonInviteFriends" => "Invite friends to the group",
        "ButtonShowGroupfinder" => "Group finder", "ButtonEditGroupName" => "Rename the group", "ButtonFoundGroup" => "Found a group",
        "ButtonLeaveGuild" => "Leave guild", "ButtonEditGuildName" => "Change the guild settings", "ButtonFoundGuild" => "Found a Guild",
        _ => return None,
    })
}

/// Icon buttons need names; text buttons already explain their destination.
fn repeats_label(window: &str, button: &Button, layout: Option<&Layout>) -> bool {
    if window == "gamemenu" { return true; }
    match &button.action {
        Action::Skill(_) | Action::Spell | Action::Item(_) => false,
        // The quest screen's Accept and Back are icons: their tooltips name them
        // (crate::ui::dialogue::quest).
        Action::NpcChoice(_) => window != "questscreen2",
        Action::Edit(_) | Action::ChatChannel(_) => true,
        Action::Settings(event) => event.starts_with("row") || matches!(event.as_str(), "previous" | "next"),
        _ => layout.is_some_and(|layout| layout.widgets.iter().any(|w|
            !w.hidden && (w.id == button.id || w.id.starts_with(&format!("{}/", button.id)))
                && w.text.as_ref().is_some_and(|t| t.color[3] > 0.0 && t.text != "Text"
                    && t.text.chars().filter(|c| c.is_alphabetic()).count() > 1))),
    }
}

pub(super) fn clean_text(raw: &str) -> String {
    card::plain(raw)
}

#[allow(clippy::too_many_arguments)]
/// One-line tooltips (a bar's "58832 / 58832 Health Points", a button's name)
/// at 0.62 of the card's text size: measured on the user's release261_192 shot
/// beside ours (against the bar's width), 2026-10-24 ("its so big").
const COMPACT_TEXT: f32 = 0.62;

pub(in crate::ui) fn overlay(mut commands: Commands, windows: Query<(&Window2018, &WindowControls, &Visibility, &Transform), Without<Tooltip>>,
    primary: Query<&Window, With<PrimaryWindow>>, screen: Res<Screen>, settings: Res<settings::Settings>,
    (pointer,keys,figures,shown):(Res<Pointer>,Res<ButtonInput<KeyCode>>,Res<Figures>,Res<currency::Shown>),
    net: Option<NonSend<crate::network::net::Net>>, hotbar: Res<hotbar::Hotbar>,
    (data, catalogs, dialogue, views): (Res<items::Data>, Res<Assets<items::Catalog>>, Res<dialogue::Presentation>, Res<super::systems::Views>), layouts: Res<Assets<Layout>>, loaded: Res<Loaded>,
    (skill_tables, status_bar, status_catalogs, status_slots): (Res<Assets<crate::gameplay::skills::SkillTable>>,
        Res<effects::StatusBar>, Res<Assets<effects::Catalog>>, Query<&effects::EffectSlot>), assets: Res<AssetServer>,
    (old, mut transforms): (Query<Entity, With<Tooltip>>, Query<&mut Transform, With<Tooltip>>),
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<UiMaterial>>),
    mut last: Local<Option<(Card, u32, bool)>>) {
    let point = primary.single().ok().filter(|w| w.focused).and_then(|w| w.cursor_position().map(|p| p*(BASE_H/w.height().max(1.0))));
    let enabled = settings.current.tooltips && pointer.down.is_none() && pointer.drag.is_none() && pointer.spell_drag.is_none();
    let catalog = catalogs.get(&data.catalog);
    let mut content: Option<(String,String,Option<String>)> = None;
    let mut anchor = None;
    let mut detail = None;
    // Cards over status effects open above or below them, clear of their bar
    // (the placement's vertical-first order).
    let mut above = false;
    let template = layouts.get(&data.tooltip);
    if enabled && point.is_some() {
        let hovered = windows.iter().filter(|(_,_,v,_)| **v != Visibility::Hidden)
            .find_map(|(w,c,_,tf)| c.hovered.filter(|i| !matches!(c.buttons[*i].action, Action::Item(_))).map(|i| (w,c,&c.buttons[i],tf)));
        if let Some((w,c,b,tf)) = hovered.filter(|(w,_,b,_)| !repeats_label(&w.0,b,
            loaded.layouts.iter().filter_map(|(h,_,_)| layouts.get(h)).find(|l| l.window == w.0))) {
            let [l,t,r,bottom] = b.rect;
            let offset = window_offset(tf) + Vec2::new(b.k*(screen.width-BASE_W),0.0);
            anchor = Some([l+offset.x,t+offset.y,r+offset.x,bottom+offset.y]);
            let mut heading = String::new(); let mut body = String::new(); let mut icon = None;
            let mut spell = b.spell.as_ref();
            match &b.action {
                Action::Skill(index) => {
                    heading = match *index { 17 => "Primary essence".into(), 18 => "Backup essence".into(),
                        _ => settings::shortcuts().iter().find(|s| s.0 == format!("slot{index}")).map_or_else(|| "Action slot".into(), |s| s.1.clone()) };
                    if *index < 8 {
                        let id = c.spells.bar.get(*index).and_then(|s| s.as_ref());
                        spell = windows.iter().flat_map(|(_,c,_,_)| &c.buttons).filter_map(|b| b.spell.as_ref())
                            .find(|s| Some(&s.id) == id && s.class_id == c.spells.class);
                    }
                    if let Some(net) = &net {
                        if *index >= hotbar::PRIMARY_ESSENCE {
                            if let Some(item) = hotbar::essence(&net.inventory,*index) {
                                if let Some(catalog) = catalog {
                                    let name = catalog.name(&item.template, Some(item));
                                    body = format!("{name}\nQuantity: {}",item.third);
                                    icon = catalog.icon(&item.template,Some(item)).0.map(str::to_owned);
                                    detail = Some(items::preview_card(catalog,template,&item.template,Some(item),item.third,settings.current.advanced_tooltips(&keys,pointer.focus.is_some())));
                                }
                            } else { body = "Empty. Equip an essence from your inventory.".into(); }
                            body.push_str(&format!("\nSwitch essences: {}",settings.current.label("essence")));
                        } else if let Some(slot) = hotbar::slot(net, hotbar.active,*index) {
                            if slot.kind == 0 {
                                spell = windows.iter().flat_map(|(_,c,_,_)| &c.buttons).filter_map(|b| b.spell.as_ref())
                                    .find(|s| slot.id.as_ref() == Some(&s.id));
                            } else if let (Some(id),Some(catalog)) = (&slot.id,catalog) {
                                let item = hotbar::item(slot,&net.inventory);
                                let template = item.map_or(id.as_str(),|i| i.template.as_str());
                                if let Some(meta) = catalog.items.get(template) {
                                    heading = catalog.name(template, item).to_owned(); body = clean_text(&meta.description);
                                    icon = catalog.icon(template,item).0.map(str::to_owned);
                                    if slot.kind == 1 { body.push_str(&format!("\nQuantity: {}",item.map_or(0,|i| i.third))); }
                                    detail = Some(items::preview_card(catalog,layouts.get(&data.tooltip),template,item,item.map_or(0,|i|i.third),settings.current.advanced_tooltips(&keys,pointer.focus.is_some())));
                                }
                            } else { body = "Empty slot".into(); }
                        }
                    }
                    if spell.is_none() && *index < 17 { body.push_str(&format!("\nPress {}", settings.current.slot_label(*index))); }
                }
                Action::Spell => {}
                Action::NpcChoice(index) => { heading = dialogue.choice_label(*index).unwrap_or_default().into(); }
                Action::Item(_) => return, // Inventory owns its detailed item tooltip.
                // The bar's dropdown: the picked currency's whole amount.
                Action::Window(currency::BAG) if b.id.ends_with("/ButtonDropdown") => {
                    heading = currency::describe(&shown.id, figures.wallet.balance(&shown.id), catalog);
                }
                Action::Window(name) => { heading = window_title(name).into(); let key = if *name == "gamemenu" { "Esc".into() } else { settings.current.label(name) }; if key != "Unbound" { body = format!("(Press {key})"); } }
                Action::Currency(id) => heading = currency::describe(id, figures.wallet.balance(id), catalog),
                Action::Settings(event) => {
                    heading = match event.as_str() { "open" => "Game Settings", "controls" => "Hotkeys", "general" => "General", "graphics" => "Graphics", "audio" => "Audio", "cursor" => "Mouse", "combatSettings" => "Combat", "apply" => "Apply and save", "cancel" => "Discard changes", "revert" => "Restore defaults", _ => "Settings" }.into();
                    if let Some(row) = event.strip_prefix("row").and_then(|s| s.parse::<usize>().ok()) {
                        if let Some((_,label,value)) = settings.rows().get(settings.page*settings::ROWS+row) { heading = label.clone(); body = value.clone(); }
                    }
                    if event == "revert" { body = "Restore default preferences and hotkeys. Apply saves them; Cancel keeps your current settings.".into(); }
                }
                Action::Lock => { heading = if c.locked { "Unlock action bar" } else { "Lock action bar" }.into(); body = "Lock prevents dragging skills out of their slots.".into(); }
                Action::Essence => { heading = "Switch primary and backup essence".into(); body = format!("Press {}",settings.current.label("essence")); }
                Action::Bar(delta) => { heading = if *delta<0 { "Previous action bar" } else { "Next action bar" }.into(); body = format!("Press {}",settings.current.label(if *delta<0 { "previous_bar" } else { "next_bar" })); }
                Action::Fullscreen => heading = "Toggle fullscreen".into(),
                Action::Membership => { heading = "Membership".into(); body = if net.as_ref().is_some_and(|n| n.inventory.premium) { "Membership active. Open the membership page for details." } else { "Open the membership page." }.into(); }
                Action::Close => { heading = "Close".into(); body = "Press Esc".into(); }
                // A row is its own description; the systems' buttons say what they do.
                // A 3D preview's turning area is the figure itself.
                Action::QuestSelect(_) | Action::QuestGroup(_) | Action::DragRotate => return,
                Action::QuestCancel(_) => { heading = "Stop quest".into(); body = "The quest leaves your log; you can take it again.".into(); }
                Action::QuestCompact(compact) => heading = if *compact { "Hide the objectives" } else { "Show the objectives" }.into(),
                Action::PartyCompact(compact) => heading = if *compact { "Hide the group" } else { "Show the group" }.into(),
                Action::QuestTrack(delta) => heading = if *delta < 0 { "Previous quest" } else { "Next quest" }.into(),
                // A sticker: its item's card, as the original's.
                Action::System(super::systems::Request::Pick(id, _)) if catalog.is_some() && views.album.sticker(id).is_some_and(|s| !s.template.is_empty()) => {
                    let template = &views.album.sticker(id).unwrap().template;
                    detail = Some(items::sticker_card(catalog.unwrap(), layouts.get(&data.tooltip), template));
                }
                Action::System(request) => match super::systems::tooltip(request) {
                    Some((title, text)) => { heading = title.into(); body = text; }
                    None => return,
                },
                _ => {
                    // Prefer the source's localized button text, then a readable ID.
                    heading = if w.0 == "tradingscreen" {
                        match b.id.rsplit('/').next().unwrap_or("") {
                            "BuyTabBtn" => "Buy", "RestoreTabBtn" => "Buy back",
                            "PrevPageBtn" => "Previous page", "NextPageBtn" => "Next page", _ => "",
                        }.into()
                    // gui.window.exclusiveactivatoncode_title / recalledsignin_title.
                    } else if w.0 == "gamebar" && b.id.ends_with("/ExclsiveactiveCodeBtn") { "Return Link".into() }
                    else if w.0 == "gamebar" && b.id.ends_with("/RecalledSignInBtn") { "Return Check-in".into() }
                    else if let Some(name) = social_button(&w.0, b.id.rsplit('/').next().unwrap_or("")) { name.into() }
                    else { loaded.layouts.iter().filter_map(|(h,_,_)| layouts.get(h)).find(|l| l.window == w.0)
                        .and_then(|l| l.widgets.iter().find(|x| x.id.starts_with(&format!("{}/",b.id)) && x.text.as_ref().is_some_and(|t| !t.text.is_empty() && t.text != "Text")))
                        .and_then(|w| w.text.as_ref()).map_or_else(|| title(b.id.rsplit('/').next().unwrap_or("")), |t| t.text.clone()) };
                    if !c.enabled(b) { body = "Currently unavailable".into(); }
                }
            }
            if let Some(s) = spell {
                let mut card = Card::new(380.0, template);
                card.title(&s.name, Some(&s.icon), WHITE);
                card.rule();
                let description = if s.preview_description.is_empty() { &s.description } else { &s.preview_description };
                card.text(description, WHITE);
                if !s.growth.is_empty() { card.gap(10.0); card.text(&s.growth, BLUE); }
                card.gap(6.0); card.rule();
                if !s.target.is_empty() { card.text(&format!("Target: {}",s.target), WHITE); }
                if let Some(def) = skill_tables.iter().find_map(|(_,t)| s.wire.and_then(|id| t.0.get(&id)).filter(|d| d.id == s.id)) {
                    let resource = ["Rage", "Mana", "Concentration", "Steam"].get(s.class_id as usize).unwrap_or(&"Resource");
                    card.text(&format!("Base cost: <GreenStart>{:.0}%</> maximum {resource}",def.resource_cost*100.0), WHITE);
                    card.text(&format!("Essence Costs: {}",def.ammo_cost), WHITE);
                    card.text(&format!("Cool-down: {} s",def.cooldown), WHITE);
                }
                if !c.spells.available(s) && s.wire.is_some() { card.text(&format!("Required level: {}",s.unlock), Color::srgb(0.9,0.65,0.3)); }
                if s.talent_cost > 0 { card.text(&format!("Cost: {} Talent Points", s.talent_cost), WHITE); }
                if let Action::Skill(index) = b.action { card.hint(&format!("Press {}",settings.current.slot_label(index))); }
                else if s.wire.is_some() { card.hint("Drag to the action bar"); }
                else { card.hint("Passive talent"); }
                if description.contains('{') || s.growth.contains('{') { card.hint("—: value unavailable"); }
                card.gap(3.0); card.hint("Base skill values");
                detail = Some(card);
            }
            if !heading.is_empty() { content = Some((heading,body.trim().to_owned(),icon)); }
        } else if let Some(p) = point {
            // The status effects (above the action bar, under a monster's panel):
            // click-through, so not buttons, but each explains itself.
            let offset_of = |row: usize| { let name = if row == effects::TARGET { HOVER_WINDOW } else { "bottombar" };
                windows.iter().find(|(w,_,v,_)| w.0 == name && **v != Visibility::Hidden).map(|(_,_,_,tf)| window_offset(tf)) };
            let effect = effects::card(p, offset_of, screen.width, &status_bar,
                status_catalogs.get(&status_bar.catalog), &status_slots, net.as_deref(), template);
            if let Some((card, rect)) = effect {
                detail = Some(card);
                anchor = Some(rect);
                above = true;
            // Non-button gauges and currency amounts also explain themselves.
            } else if let Some((w,wc,_,tf)) = windows.iter().filter(|(_,c,v,tf)| **v != Visibility::Hidden && c.covers(p-window_offset(tf),screen.width))
                .max_by(|a,b|a.3.translation.z.total_cmp(&b.3.translation.z)) {
                if let Some(layout) = loaded.layouts.iter().filter_map(|(h,_,_)| layouts.get(h)).find(|l|l.window==w.0) {
                    // The currency bar as drawn in game (its layout keeps other money displays).
                    let drawn = |x: &Widget| if w.0 == currency::BAR { currency::bar_part(&x.id).unwrap_or(!x.hidden) } else { !x.hidden };
                    for widget in layout.widgets.iter().rev().filter(|x| drawn(x) && contains(x.k,x.rect,p-window_offset(tf),screen.width)) {
                        let tip = if w.0 == currency::BAR { currency::tip(&widget.id, &figures.wallet) }
                        else if widget.id.contains("andermant") { Some("Andermant".into()) } else if widget.id.contains("VirtualCurrency") { Some("Coins: 100 copper = 1 silver; 100 silver = 1 gold".into()) }
                        // The bars' own lines (gui.window bottomBarXp, bottomBarHealth,
                        // bottomBarRage / Mana / Stamina / MechanicResource).
                        // FAILURE (2026-10-24): "Health", "Experience: earn experience to
                        //   level up" without the numbers (the user's shots beside the original's).
                        else if widget.id.contains("bottombar_xp_bar") { Some(figures.xp_points.map_or_else(|| "Experience points".to_owned(), |(n, of)| format!("{n} / {of} Experience points"))) }
                        else if widget.id.contains("bottombar_wisdom_bar") { Some("Ancient Wisdom".into()) }
                        else { match gauge_of(&widget.id) {
                            Some(Gauge::Health | Gauge::HealthBar) => Some(format!("{:.0} / {:.0} Health Points", figures.health.0, figures.health.1)),
                            Some(Gauge::Resource | Gauge::ResourceBar) => Some(format!("{:.0} / {:.0} {}", figures.resource.0, figures.resource.1,
                                ["Rage points", "Mana points", "concentration", "Steam"][wc.spells.class.min(3) as usize])),
                            _ => None } };
                        if let Some(tip) = tip {
                            content=Some((tip,String::new(),None));
                            let [l,t,r,b] = widget.rect;
                            let offset = window_offset(tf) + Vec2::new(widget.k*(screen.width-BASE_W),0.0);
                            anchor = Some([l+offset.x,t+offset.y,r+offset.x,b+offset.y]);
                            break;
                        }
                    }
                }
            }
        }
    }
    let compact = detail.is_none() && content.as_ref().is_some_and(|(_,_,icon)| icon.is_none());
    let card = detail.or_else(|| content.map(|(heading,body,icon)| {
        let mut card = if compact {
            // Sized to its text at the size it is drawn.
            let mut card = Card::new(260.0,template);
            card.shrink(COMPACT_TEXT);
            let width = heading.lines().chain(body.lines()).map(|s| crate::network::chat::text_width(s,card.text_size()))
                .fold(0.0,f32::max).ceil()+28.0;
            card.width = width.clamp(48.0,260.0);
            card
        } else { Card::new(320.0,template) };
        if compact {
            card.y = 9.0;
            card.centered(&heading,WHITE,false);
            if !body.is_empty() { card.centered(&body,if body.starts_with("(Press ") || body.starts_with("Press ") { Color::srgb(1.0,0.9,0.0) } else { WHITE },false); }
            card.y -= 4.0;
        } else {
            card.title(&heading,icon.as_deref(),WHITE); card.rule(); card.text(&body,WHITE);
        }
        card
    }));
    let key = card.as_ref().map(|c| (c.clone(), screen.scale.to_bits(), template.is_some()));
    let at = card.as_ref().zip(anchor).map(|(c,a)| card::placement(a,Vec2::new(c.width,c.height()),Vec2::new(screen.width,BASE_H),compact || above));
    if *last == key {
        if let Some((at,scale))=at { for mut tf in &mut transforms { tf.translation=screen.world(0.0,at.x,at.y).extend(850.0); tf.scale=Vec3::splat(scale); } }
        return;
    }
    *last=key; for e in &old { commands.entity(e).despawn(); }
    let (Some(card),Some((at,scale)))=(card,at) else{return;};
    let root=commands.spawn((Tooltip,Transform::from_translation(screen.world(0.0,at.x,at.y).extend(850.0)).with_scale(Vec3::splat(scale)),Visibility::Inherited)).id();
    card.draw(&mut commands,&assets,&screen,&mut meshes,&mut materials,root,template);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires locally converted game UI assets"]
    fn hover_cards_anchor_to_elements_and_advanced_items_reflow() {
        let mut app=App::new();
        app.add_plugins((MinimalPlugins,bevy::asset::AssetPlugin::default(),TransformPlugin))
            .init_asset::<Layout>().init_asset::<items::Catalog>().init_asset::<Mesh>().init_asset::<UiMaterial>()
            .init_asset::<Image>().init_asset::<Font>().init_asset::<crate::gameplay::skills::SkillTable>()
            .init_resource::<Screen>().init_resource::<Loaded>().init_resource::<settings::Settings>()
            .init_resource::<Pointer>().init_resource::<hotbar::Hotbar>().init_resource::<dialogue::Presentation>()
            .init_resource::<items::Interaction>().init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<effects::StatusBar>().init_asset::<effects::Catalog>()
            .init_resource::<Figures>().init_resource::<currency::Shown>()
            .add_systems(Update,(overlay,items::overlay).chain());
        app.world_mut().resource_mut::<settings::Settings>().current.advanced_tooltips=false;
        let dir=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/interface");
        let load=|name:&str|serde_json::from_slice::<Layout>(&std::fs::read(dir.join(format!("{name}.ui.json"))).unwrap()).unwrap();
        let tooltip=app.world_mut().resource_mut::<Assets<Layout>>().add(load("tooltip"));
        let catalog:items::Catalog=serde_json::from_slice(&std::fs::read(dir.join("inventory.items.json")).unwrap()).unwrap();
        let template=catalog.items.iter().find(|(_,m)|m.item_category=="Weapon" && m.class_name=="dwarf").unwrap().0.clone();
        let catalog=app.world_mut().resource_mut::<Assets<items::Catalog>>().add(catalog);
        app.insert_resource(items::Data{catalog,tooltip});
        let primary=app.world_mut().spawn((Window {focused:true,resolution:(1024,768).into(),..default()},PrimaryWindow)).id();
        let layout=load("skilltalentswindow");
        let mut c=WindowControls::new(&layout);
        c.hovered=c.buttons.iter().position(|b|b.spell.as_ref().is_some_and(|s|s.id=="HeavyShot"));
        assert!(c.hovered.is_some());
        let owner=app.world_mut().spawn((Window2018(layout.window.clone()),c,Visibility::Inherited,Transform::default())).id();
        app.world_mut().get_mut::<Window>(primary).unwrap().set_cursor_position(Some(Vec2::new(500.0,300.0)));
        app.update();
        let tip=app.world_mut().query_filtered::<Entity,With<Tooltip>>().single(app.world()).unwrap();
        let before=*app.world().get::<Transform>(tip).unwrap();
        let text=app.world_mut().query::<&Text2d>().iter(app.world()).map(|t|t.0.as_str()).collect::<Vec<_>>().join(" ");
        assert!(text.contains("Heavy Shot") && text.contains("explosion"));
        assert!(text.contains("250.0%") && text.contains("2.5%"));
        app.world_mut().get_mut::<Window>(primary).unwrap().set_cursor_position(Some(Vec2::new(530.0,330.0)));
        app.update();
        assert_eq!(*app.world().get::<Transform>(tip).unwrap(),before,"cursor motion must not move the card");
        app.world_mut().get_mut::<Transform>(owner).unwrap().translation.x+=10.0;
        app.update();
        assert_ne!(*app.world().get::<Transform>(tip).unwrap(),before,"moving the owner moves the card");
        // The shared frame's fill stays inside the original opaque border.
        let fill=app.world().get::<Children>(tip).unwrap().iter().find_map(|e|app.world().get::<Sprite>(e).zip(app.world().get::<Transform>(e)));
        let (_,tf)=fill.unwrap(); assert!(tf.translation.x>3.0 && tf.translation.y < -3.0);
        app.world_mut().get_mut::<Visibility>(owner).unwrap().set_if_neq(Visibility::Hidden);
        crate::network::net::start_offline(app.world_mut(),3,0);
        app.world_mut().non_send_mut::<crate::network::net::Net>().inventory.apply(dsor_proto::commands::inventory::InventoryInfo {
            items:vec![dsor_proto::commands::inventory::ItemInfo{id:1,template,third:1,kind:5,durability:100,level:12345,fifty_second:26445,..default()}],
            placements:vec![(1,0)],storage_sizes:[0,0,224,0,0,0],..default()
        });
        let layout=load("inventory");let mut c=WindowControls::new(&layout);
        c.hovered=c.buttons.iter().position(|b|b.action==Action::Item(items::Cell::Bag(0)));
        app.world_mut().spawn((Window2018("inventory".into()),c,Visibility::Inherited,Transform::default()));
        app.update();
        let texts=|app:&mut App|app.world_mut().query::<&Text2d>().iter(app.world()).map(|t|t.0.as_str()).collect::<Vec<_>>().join(" ");
        assert!(texts(&mut app).contains("Hold Y"));
        assert!(texts(&mut app).contains("26445"));
        let tip=app.world_mut().query_filtered::<Entity,With<items::Overlay>>().single(app.world()).unwrap();
        let before=*app.world().get::<Transform>(tip).unwrap();
        app.world_mut().get_mut::<Window>(primary).unwrap().set_cursor_position(Some(Vec2::new(550.0,350.0)));
        app.update();assert_eq!(*app.world().get::<Transform>(tip).unwrap(),before);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyY);app.update();
        assert!(!texts(&mut app).contains("Hold Y"));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyY);app.update();
        assert!(texts(&mut app).contains("Hold Y"));
        app.world_mut().resource_mut::<settings::Settings>().current.advanced_tooltips=true;app.update();
        assert!(!texts(&mut app).contains("Hold Y"),"saved preference reflows an already visible card without Y");
        app.world_mut().get_mut::<Window>(primary).unwrap().focused=false;app.update();
        assert_eq!(app.world_mut().query_filtered::<Entity,With<items::Overlay>>().iter(app.world()).count(),0);
    }
}
