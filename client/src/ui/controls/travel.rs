//! The travel confirmation (confirmtraveldialog): a destination picked on the
//! travel map asks first, "Travel to <destination>", with "The journey will
//! cost you:" and its gold price when it costs something (a travel stone's
//! journey: _Template_TravelCosts, local_map::MapNames travel_vc). Confirm
//! travels; Cancel or Close drop it.
//! FAILURE: the map travelled at once ("quand je click il doit avoir le popup
//!   Travel to destination, The journey will cost you").
//! UNVERIFIED: the Andermant button (ButtonRC) and the privileged free button's
//!   Premium/Deluxe icons are not offered.
use super::*;
use dsor_proto::commands::ClientCommand;

pub(in crate::ui) const WINDOW: &str = "confirmtraveldialog";
const BORDER: &str = "Canvas/Label_blur/Label_struktur/Border";
/// The page group: 1 free (the centre button), 2 paid (the gold button).
pub(in crate::ui) const PAGES: &str = "travel_cost";

/// The layout as drawn: its texts written by `dialog`, the gold and the free
/// buttons confirming, the Andermant button and the price templates left out.
pub(in crate::ui) fn prepare(layout: &Layout) -> Layout {
    let mut out = layout.clone();
    out.widgets.retain(|w| !w.id.contains("/ButtonRC") && !w.id.contains("/VCDisplay") && !w.id.contains("/IconDeluxe") && !w.id.contains("/IconPremium"));
    for w in &mut out.widgets {
        if w.id.ends_with("/TextLabelDestinationName") || w.id.ends_with("/TextLabelCurrencyExplanation") || w.id.ends_with("/ButtonLabel") { w.text = None; }
        if w.id.contains("/ButtonVC") || w.id.contains("/ButtonCenterPrivileged") { w.hidden = false; }
        if w.id == format!("{BORDER}/ButtonVC") || w.id == format!("{BORDER}/ButtonCenterPrivileged") { super::template::make_button(w, "confirmTravel"); }
    }
    out
}

pub(in crate::ui) fn section(parts: &[&str]) -> Option<Section> {
    if parts.contains(&"ButtonVC") { return Some(Section::Page(PAGES, 0b10)); }
    if parts.contains(&"ButtonCenterPrivileged") { return Some(Section::Page(PAGES, 0b01)); }
    None
}

pub(in crate::ui) fn action(w: &Widget) -> Action {
    match w.event.as_str() { "confirmTravel" => Action::TravelAnswer(true), "Cancel" | "Close" => Action::TravelAnswer(false), _ => Action::Unavailable }
}

/// "{g} Gold {s} Silver {c} Copper" (gui.window travelmap travelCostValueVC).
fn gold(copper: u64) -> String {
    format!("{} Gold {} Silver {} Copper", copper / 10_000, copper / 100 % 100, copper % 100)
}

/// The travel map's locked destinations, greyed as the original's (their
/// layout's "disabled" art: desaturated, the travel stone grey) and not
/// clickable: those whose bit NewPlayer's travel map access bits lack
/// (local_map::MapNames::travel_unlocked). Redone when the bits, the names
/// or the window change.
/// EVIDENCE: the user's original clip 20261107-1534-28 (Atlantis: the main
///   island grey, Helios and the hub in colour).
/// FAILURE (to 2026-11-07): every area in colour; Hagastove Grotto offered a
///   journey the server then refused ("rejected locked map travel").
pub(in crate::ui) fn locks(
    net: Option<NonSend<crate::network::net::Net>>,
    mut windows: Windows,
    (loaded, names): (Res<crate::ui::Loaded>, Res<Assets<crate::ui::local_map::MapNames>>),
    mut done: Local<Option<(Vec<u32>, bool, Entity)>>,
) {
    let Some(net) = net else { return };
    let names = names.get(&loaded.map_names);
    let Some((root, _, mut c, _, _)) = windows.iter_mut().find(|(_, n, ..)| n.0 == "travelmap") else { return };
    let key = (net.travel_unlocks.clone(), names.is_some(), root);
    if done.as_ref() == Some(&key) { return; }
    let Some(names) = names else { return };
    c.lock_travel(|map| names.travel_unlocked(map, &net.travel_unlocks));
    *done = Some(key);
}

#[derive(Component)] pub struct TravelText;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::ui) fn dialog(
    mut commands: Commands,
    net: Option<NonSendMut<crate::network::net::Net>>,
    mut windows: Windows,
    mut pointer: ResMut<Pointer>,
    mut chat: ResMut<crate::network::chat::ChatLog>,
    (layouts, loaded, names): (Res<Assets<Layout>>, Res<crate::ui::Loaded>, Res<Assets<crate::ui::local_map::MapNames>>),
    (assets, screen): (Res<AssetServer>, Res<Screen>),
    old: Query<Entity, With<TravelText>>,
    (mut asked, mut shown): (Local<Option<(String, String, u64)>>, Local<Option<(String, u64, u32, u32)>>),
) {
    let Some(mut net) = net else { return };
    let names = names.get(&loaded.map_names);
    if let Some(map) = pointer.travel.take() {
        let source = net.travel_source.take().unwrap_or_default();
        // Only a stone's journey is paid (DSOR_Server map_transition.py).
        let price = if source.is_empty() { 0 } else { names.map_or(0, |n| n.travel_price(&map, net.level.unwrap_or(1))) };
        *asked = Some((map, source, price));
    }
    if let Some(yes) = pointer.travel_answer.take() {
        if let Some((map, source, _)) = asked.take().filter(|_| yes) {
            let name = names.map_or_else(|| map.clone(), |n| n.caption(&map));
            net.send(&ClientCommand::Travel(dsor_proto::commands::player::Travel { source, ..dsor_proto::commands::player::Travel::to_map(&map) }));
            chat.push(crate::network::chat::Kind::System, format!("Travelling to {name}."));
        }
        *asked = None;
    }
    let Some((root, _, mut c, mut v, _)) = windows.iter_mut().find(|(_, n, ..)| n.0 == WINDOW) else { return };
    // Closed another way (Escape): no journey.
    if asked.is_some() && shown.is_some() && *v == Visibility::Hidden { *asked = None; }
    v.set_if_neq(if asked.is_some() { Visibility::Inherited } else { Visibility::Hidden });
    let page = if asked.as_ref().is_some_and(|a| a.2 > 0) { 1 } else { 0 };
    if c.page(PAGES) != page { c.set_page(PAGES, page); }
    let key = asked.as_ref().map(|(map, _, price)| (map.clone(), *price, screen.width.to_bits(), screen.scale.to_bits()));
    if *shown == key { return; }
    *shown = key.clone();
    for e in &old { commands.entity(e).despawn(); }
    let (Some((map, price, _, _)), Some(layout)) = (key, loaded.layouts.iter().filter_map(|(h, _, _)| layouts.get(h)).find(|l| l.window == WINDOW)) else { return };
    let name = names.map_or_else(|| map.clone(), |n| n.caption(&map));
    let font = |size: f32| TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(size * FONT_SCALE * screen.scale), ..default() };
    let mut write = |id: &str, text: String, size: f32| {
        let Some(w) = layout.widgets.iter().find(|w| w.id.ends_with(id)) else { return };
        let [l, t, r, b] = w.rect;
        commands.spawn((Text2d::new(text), font(size), TextColor(Color::srgb(1.0, 0.87, 0.6)),
            bevy::text::TextLayout::justify(bevy::text::Justify::Center), Anchor::CENTER, bevy::text::TextBounds::new_horizontal((r - l) * screen.scale),
            Transform::from_translation(screen.world(w.k, (l + r) * 0.5, (t + b) * 0.5).extend(WINDOW_ITEMS_DEPTH + 0.2)).with_scale(Vec3::splat(1.0 / screen.scale)),
            RenderLayers::layer(LAYER), TravelText, ChildOf(root)));
    };
    write("/TextLabelDestinationName", format!("Travel to {name}"), 12.0);
    if price > 0 {
        write("/TextLabelCurrencyExplanation", "The journey will cost you:".into(), 11.0);
        write("/ButtonVC/normal/relief/rahmen", gold(price), 10.0);
    } else {
        write("/ButtonCenterPrivileged/normal/relief1/rahmen", "Travel".into(), 11.0);
    }
}
