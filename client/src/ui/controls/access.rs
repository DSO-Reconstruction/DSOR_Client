//! The map access window (mapaccesswindow): a travel item whose destination
//! has several difficulties (its _Template_MapAccessGroup rows sharing one
//! AccessGroup: pw003_05/07/09_grimmag_01_boss) asks for one on the
//! difficulty slider first. The slider's snap marks are the tiers; a click on
//! it picks the nearest; Enter travels: the item's use (0x13) arms it, then
//! UseTravelItem names the chosen row (DSOR_Server map_transition.py accepts
//! any row of the armed target's access group). Close drops it.
//! FAILURE: "j'ai pas le menu pour choisir quand j'utilise un voyage pour un
//!   boss, je devrais pouvoir choisir la difficulte avec le slider".
//! The row's BackgroundImage replaces the layout's default artwork; its
//! descriptions, possible rewards (RewardItems, the player's class variant),
//! the items its teleport takes (TeleportKeyId) and the group size
//! (_Template_MapTemplate.MaxPlayer) fill the panes.
//! FAILURE: "j'ai ni les boutons ni l'image du bon boss ni le texte".
//! The difficulty modifications, for every map: the icon
//! lines of MapAccessWidget::SetDifficultyInfoText (dro_client64 strings:
//! icons/difficulties_damage ... _items_epic; values _Template_DifficultyFactors,
//! matching the retail Grimmag Bloodshed screenshot: 1100%, 2500%, 5 - 6, 45,
//! 1100%, 0/14/54/10%, 600%), else the text list (retail Heredur screenshot).
//! A row whose UnlockingQuest is not closed in our log names it under the
//! entry requirements and refuses Enter with the original's
//! enterDungeonDisabledTooltip (DSOR_Server refuses that travel anyway,
//! map_unlocks._map_quest_accessible).
//! FAILURE: "tp bearach ... ca me dit pas pourquoi ca marche pas, juste ca me
//!   tp pas" (pw012 needs pw010-unlock-pw011-n).
//! UNVERIFIED: the rule choosing the icon pane (a Parallel World difficulty)
//!   and hiding the recommended level with it is inferred from two
//!   screenshots; the key order (KeyId first) likewise; no tooltips; the
//!   leaderboards tab is left out.
use super::*;
use dsor_proto::commands::ClientCommand;

pub(in crate::ui) const WINDOW: &str = "mapaccesswindow";
const ART: &str = "Canvas/Skillwindow/BackGroundLabel/WindowTitleLabel/Frame/Frame2/LabelArtwork2/LabelArtwork";
/// The layout's own artwork, swapped for the row's BackgroundImage.
const DEFAULT_ART: &str = "textures/ui/mapaccess_e_bos_02_mortis_01_crunch.png";
const CLASSES: [&str; 4] = ["Warrior", "Mage", "Ranger", "Dwarf"];
const RARITIES: [&str; 8] = ["Common", "Uncommon", "Magic", "Rare", "Epic", "Unique", "Set", "Mythic"];
/// The slider's track, reference pixels (k 0.5).
const TRACK: [f32; 4] = [327.69, 379.86, 692.5, 409.95];

pub(in crate::ui) fn prepare(layout: &Layout) -> Layout {
    let mut out = layout.clone();
    out.widgets.retain(|w| !w.id.contains("/TabButton") && !w.id.contains("/BottomLeaderboardsinfo") && !w.id.contains("/DifficultySnapLine")
        && !w.id.contains("/slider/Thumb") && !w.id.contains("/TextList/Scroll")
        && !w.id.contains("/DifficultyModificationsRight/TemplateListEntry") && !w.id.contains("/DifficultyModificationsRight/ScrollButton")
        && !w.id.contains("/ExtraModificationsHeader") && !w.id.contains("/ItemsTop3/Item3") && !w.id.ends_with("/DeniedGroupMapInfo")
        && !w.id.contains("/ResistancesInfo"));
    for w in &mut out.widgets {
        if w.id.contains("/ButtonBuyEnter") { w.hidden = false; }
        // Drawn by `dialog` where used: the slot frames, the filled part of the
        // track (DifficultySlider/progress).
        if is_slot(&w.id) || w.id.ends_with("/DifficultySlider/progress") { w.art = None; }
        let filled = ["/Headline/WindowTitle", "/TextList/TextLabel", "/DifficultyInfo/background2/TextLabel", "/GroupsizeText", "/ItemCount"];
        if filled.iter().any(|f| w.id.ends_with(f)) || w.id.contains("/DifficultyModificationsIcons/TextLabel") { w.text = None; }
    }
    out
}

/// A reward or entry item slot's frame (PossibleRewardsIcons/Item<i>,
/// ItemsTop3/Item<i> and its count plate Label<n>): retail draws only the used ones.
fn is_slot(id: &str) -> bool {
    let last = id.rsplit('/').next().unwrap_or("");
    let parent = id.rsplit('/').nth(1).unwrap_or("");
    (last.starts_with("Item") && (parent == "PossibleRewardsIcons" || parent == "ItemsTop3")) || (last.starts_with("Label") && parent.starts_with("Item") && id.contains("/ItemsTop3/"))
}

pub(in crate::ui) fn action(w: &Widget) -> Action {
    match w.event.as_str() { "Buy" => Action::AccessAnswer(true), "Close" => Action::AccessAnswer(false), _ => Action::Unavailable }
}

#[derive(Component)] pub struct AccessText;

/// What is asked: the travel item, its tiers (access row, difficulty, key) and
/// the one picked.
#[derive(Default)]
pub(in crate::ui) struct Asked { item: u32, tiers: Vec<(String, u32, Option<String>)>, picked: usize }

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::ui) fn dialog(
    mut commands: Commands,
    net: Option<NonSendMut<crate::network::net::Net>>,
    mut windows: Windows,
    mut pointer: ResMut<Pointer>,
    (layouts, loaded, names): (Res<Assets<Layout>>, Res<crate::ui::Loaded>, Res<Assets<crate::ui::local_map::MapNames>>),
    (assets, screen, mouse): (Res<AssetServer>, Res<Screen>, Res<ButtonInput<MouseButton>>),
    (data, catalogs, mut materials, mut meshes): (Res<items::Data>, Res<Assets<items::Catalog>>, ResMut<Assets<UiMaterial>>, ResMut<Assets<Mesh>>),
    (elements, players): (Query<(&Element, &MeshMaterial2d<UiMaterial>)>, Query<&crate::actors::character::Character, With<crate::network::net::LocalPlayer>>),
    (presentation, dialogue_catalogs, mut refused): (Res<crate::ui::dialogue::Presentation>, Res<Assets<crate::ui::dialogue::Catalog>>, Local<bool>),
    primary: Query<&Window, With<bevy::window::PrimaryWindow>>,
    old: Query<Entity, With<AccessText>>,
    (mut asked, mut shown, mut art): (Local<Option<Asked>>, Local<Option<(usize, usize, u32, u32, bool, u64)>>, Local<Option<Handle<Image>>>),
) {
    let Some(mut net) = net else { return };
    let names = names.get(&loaded.map_names);
    // DSOR_TEST_ACCESS=<access row>: offline, the window asks for that row (screenshots).
    if net.offline && asked.is_none() && shown.is_none() && names.is_some() {
        if let Ok(row) = std::env::var("DSOR_TEST_ACCESS") { if !row.is_empty() && net.access_ask.is_none() { net.access_ask = Some((0, row)); } }
    }
    if let Some((item, row)) = net.access_ask.take() {
        let tiers = names.map(|n| n.access_tiers(&row)).unwrap_or_default();
        let picked = tiers.iter().position(|t| t.0 == row).unwrap_or(0);
        *asked = Some(Asked { item, tiers, picked });
        *refused = false;
    }
    // The picked row's unlocking quests while none of them is closed.
    let locked = |a: &Asked, net: &crate::network::net::Net| -> Vec<String> {
        let Some((row, _, _)) = a.tiers.get(a.picked) else { return Vec::new() };
        let quests: Vec<String> = names.map(|n| n.access("quest", row)).unwrap_or_default()
            .split([';', ',']).map(str::trim).filter(|q| !q.is_empty()).map(str::to_owned).collect();
        if quests.iter().any(|q| net.map_markers.quest_state(q) == Some(2)) { Vec::new() } else { quests }
    };
    if let Some(yes) = pointer.access_answer.take() {
        if yes && asked.as_ref().is_some_and(|a| !locked(a, &net).is_empty()) {
            // Refused: the window stays and says why.
            *refused = true;
            *shown = None;
        } else {
            if let Some(a) = asked.take().filter(|_| yes) {
                if let Some((row, _, _)) = a.tiers.get(a.picked) {
                    net.send(&crate::gameplay::inventory::command(a.item, dsor_proto::commands::inventory::Inventory::USE, vec![]));
                    net.send(&ClientCommand::UseTravelItem(dsor_proto::commands::inventory::UseTravelItem { item: a.item, map: row.clone() }));
                }
            }
            *asked = None;
        }
    }
    let Some((root, _, _, mut v, tf)) = windows.iter_mut().find(|(_, n, ..)| n.0 == WINDOW) else { return };
    if asked.is_some() && shown.is_some() && *v == Visibility::Hidden { *asked = None; }
    v.set_if_neq(if asked.is_some() { Visibility::Inherited } else { Visibility::Hidden });
    // A click on the slider: the nearest tier.
    if let (Some(a), true, Ok(window)) = (asked.as_mut(), mouse.just_pressed(MouseButton::Left), primary.single()) {
        if let Some(cursor) = window.cursor_position() {
            let height = window.height().max(1.0);
            let point = cursor * (BASE_H / height);
            let width = BASE_H * window.width() / height;
            let x = point.x - 0.5 * (width - BASE_W) - tf.translation.x;
            let y = point.y + tf.translation.y;
            let n = a.tiers.len();
            if n > 1 && (TRACK[1] - 8.0..=TRACK[3] + 8.0).contains(&y) && (TRACK[0] - 10.0..=TRACK[2] + 10.0).contains(&x) {
                let at = ((x - TRACK[0]) / (TRACK[2] - TRACK[0])).clamp(0.0, 1.0);
                a.picked = (at * (n - 1) as f32).round() as usize;
            }
        }
    }
    let row = asked.as_ref().and_then(|a| a.tiers.get(a.picked)).map(|t| t.0.clone()).unwrap_or_default();
    // The row's artwork on every run of the layout's default one (rebuilt
    // windows come back with the default).
    if asked.is_some() {
        let bg = names.map(|n| n.access("bg", &row)).filter(|b| !b.is_empty()).map(|b| assets.load::<Image>(format!("textures/ui/{b}.png")));
        let default = assets.load::<Image>(DEFAULT_ART);
        let want = bg.unwrap_or_else(|| default.clone());
        for (element, m) in &elements {
            if element.root != root { continue }
            let Some(id) = materials.get(&m.0).and_then(|m| m.atlas.as_ref()).map(|h| h.id()) else { continue };
            if id != want.id() && (id == default.id() || art.as_ref().is_some_and(|a| a.id() == id)) {
                if let Some(mut mat) = materials.get_mut(&m.0) { mat.atlas = Some(want.clone()); }
            }
        }
        *art = Some(want);
    }
    let catalog = catalogs.get(&data.catalog);
    let key = asked.as_ref().map(|a| (a.tiers.len(), a.picked, screen.width.to_bits(), screen.scale.to_bits(), catalog.is_some(), net.inventory.revision));
    if *shown == key { return; }
    *shown = key;
    for e in &old { commands.entity(e).despawn(); }
    let (Some(a), Some(layout)) = (asked.as_ref(), loaded.layouts.iter().filter_map(|(h, _, _)| layouts.get(h)).find(|l| l.window == WINDOW)) else { return };
    let group = commands.spawn((AccessText, Transform::from_xyz(0.0, 0.0, WINDOW_ITEMS_DEPTH), Visibility::Inherited, ChildOf(root))).id();
    let font = |size: f32| TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(size * FONT_SCALE * screen.scale), ..default() };
    let put = |commands: &mut Commands, text: String, at: Vec2, width: f32, size: f32, color: Color, centered: bool| {
        let (justify, anchor) = if centered { (bevy::text::Justify::Center, Anchor::CENTER) } else { (bevy::text::Justify::Left, Anchor::TOP_LEFT) };
        commands.spawn((Text2d::new(text), font(size), TextColor(color), bevy::text::TextLayout::justify(justify), anchor,
            bevy::text::TextBounds::new_horizontal(width * screen.scale),
            Transform::from_translation(screen.world(0.5, at.x, at.y).extend(0.5)).with_scale(Vec3::splat(1.0 / screen.scale)),
            RenderLayers::layer(LAYER), ChildOf(group)));
    };
    let widget = |id: &str| layout.widgets.iter().find(|w| w.id.ends_with(id));
    let rect = |id: &str| widget(id).map(|w| w.rect);
    // A layout artwork, moved `dx` and cut at `upto` (x, reference pixels),
    // under an artwork parent as spawn_windows makes them (y flipped).
    let art_parent = commands.spawn((Follows { k: 0.5, flip: true }, follow_transform(&screen, 0.5, true), Visibility::Inherited, ChildOf(group))).id();
    let mut draw = |commands: &mut Commands, w: &Widget, dx: f32, upto: Option<(f32, f32)>, z: f32| {
        let Some(mut art) = w.art.clone() else { return };
        for x in art.pos.iter_mut().step_by(2) { *x += dx; }
        if let Some((left, right)) = upto {
            let cut = |x: f32| x.min(right);
            for i in 0..art.pos.len() / 2 {
                let x = art.pos[2 * i];
                if x > right { art.pos[2 * i] = cut(x); if let Some(u) = art.uv.get_mut(2 * i) { *u = ((right - left) / (x - left)).clamp(0.0, 1.0) * *u; } }
            }
        }
        let mut run = Run { k: w.k, ..default() };
        run.push(&art, 0.0);
        let image = art.tex.as_ref().map(|t| assets.load(t.clone()));
        commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(materials.add(material(image))), Transform::from_xyz(0.0, 0.0, z), RenderLayers::layer(LAYER), ChildOf(art_parent)));
    };
    let (gold, pale) = (Color::srgb(1.0, 0.87, 0.6), Color::srgb(0.92, 0.87, 0.76));
    let (_, difficulty, _) = a.tiers.get(a.picked).cloned().unwrap_or_default();
    let text = |what: &str, row: &str| names.map(|n| n.access(what, row).to_owned()).unwrap_or_default();
    let group_id = names.map(|n| n.access_group(&row)).unwrap_or_default();
    let title = names.map_or_else(|| group_id.clone(), |n| n.caption(&group_id));
    let mods = names.map(|n| n.difficulty_mods(difficulty)).unwrap_or_default();
    let m = |i: usize| mods.get(i).copied().unwrap_or(0);
    if let Some([l, t, r, b]) = rect("/Headline/WindowTitle") { put(&mut commands, title, Vec2::new((l + r) * 0.5, (t + b) * 0.5), r - l, 12.0, gold, true); }
    let pw = names.is_some_and(|n| n.difficulty_name(difficulty).starts_with("Parallel World"));
    // The description: the row's text, its boss note and, off the Parallel
    // Worlds, the level; the loca's colour tags (<cRRGGBBAA>...</>) kept.
    if let Some([l, t, r, _]) = rect(&format!("{ART}/TextList")) {
        let mut body = text("desc", &row);
        let note = text("desc2", &row);
        if !note.is_empty() { body.push_str(&format!("\n\n{note}")); }
        let level = text("level", &group_id).parse::<i64>().unwrap_or(0) + m(0);
        if level > 0 && !pw { body.push_str(&format!("\n\n{}: {level}", text("text", "recommendedLevel"))); }
        let spans = colored(&body, pale);
        let first = spans.first().cloned().unwrap_or_default();
        commands.spawn((Text2d::new(first.0), font(11.0), TextColor(first.1), bevy::text::TextLayout::justify(bevy::text::Justify::Left), Anchor::TOP_LEFT,
            bevy::text::TextBounds::new_horizontal((r - l - 16.0) * screen.scale),
            Transform::from_translation(screen.world(0.5, l, t).extend(0.5)).with_scale(Vec3::splat(1.0 / screen.scale)),
            RenderLayers::layer(LAYER), ChildOf(group))).with_children(|text| {
            for (part, color) in spans.into_iter().skip(1) { text.spawn((bevy::text::TextSpan::new(part), font(11.0), TextColor(color))); }
        });
    }
    let tier_name = names.map(|n| n.difficulty_name(difficulty)).unwrap_or_default();
    if let Some([l, t, r, b]) = rect("/DifficultyInfo/background2/TextLabel") { put(&mut commands, tier_name, Vec2::new((l + r) * 0.5, (t + b) * 0.5), r - l + 80.0, 11.0, gold, true); }
    // The snap marks and the thumb.
    let n = a.tiers.len().max(1);
    let at = |i: usize| if n > 1 { TRACK[0] + (TRACK[2] - TRACK[0]) * i as f32 / (n - 1) as f32 } else { (TRACK[0] + TRACK[2]) * 0.5 };
    let y = (TRACK[1] + TRACK[3]) * 0.5;
    if let Some(w) = widget("/DifficultySlider/progress") { let [l, _, _, _] = w.rect; draw(&mut commands, w, 0.0, Some((l, at(a.picked))), 0.3); }
    for i in (0..a.tiers.len()).filter(|i| *i != a.picked) { put(&mut commands, "|".into(), Vec2::new(at(i), y), 20.0, 11.0, Color::srgb(0.75, 0.68, 0.55), true); }
    // The layout's thumb (slider/Thumb/normal/Handle) on the picked tier.
    if let Some(w) = widget("/slider/Thumb/normal/Handle") { let [l, _, r, _] = w.rect; draw(&mut commands, w, at(a.picked) - (l + r) * 0.5, None, 0.6); }
    // The difficulty modifications: icon lines (Icon0-4 left, 5-9 right), or
    // the text list.
    let lines: Vec<(Option<&str>, String)> = {
        let f = names.map(|n| n.difficulty_factors(difficulty)).unwrap_or_default();
        let f = |i: usize| f.get(i).copied().unwrap_or(0.0);
        let pct = |v: f32| format!("{}%", (v * 100.0).round() as i64);
        vec![(Some("damage"), pct(f(0))), (Some("healthbonus"), pct(f(1))), (Some("champ_enchantments"), if f(2) == f(3) { (f(2) as i64).to_string() } else { format!("{} - {}", f(2) as i64, f(3) as i64) }),
            (Some("strength"), m(0).to_string()), (Some("xpbonus"), pct(f(4))), (Some("items_common"), format!("{}%", m(4))), (Some("items_magic"), format!("{}%", m(1))),
            (Some("items_rare"), format!("{}%", m(2))), (Some("items_epic"), format!("{}%", m(3))), (Some("stacksizebonus"), pct(f(5)))]
    };
    for (i, (icon, line)) in lines.into_iter().enumerate() {
        let (Some([il, it, ir, ib]), Some([_, t, r, b])) = (rect(&format!("/DifficultyModificationsIcons/Icon{i}")), rect(&format!("/DifficultyModificationsIcons/TextLabel{i}"))) else { continue };
        match icon {
            Some(icon) => {
                commands.spawn((Sprite { image: assets.load(format!("textures/icons/difficulties_{icon}.png")), custom_size: Some(Vec2::new(ir - il, ib - it)), ..default() },
                    Anchor::TOP_LEFT, Transform::from_translation(screen.world(0.5, il, it).extend(0.4)), RenderLayers::layer(LAYER), ChildOf(group)));
                put(&mut commands, line, Vec2::new(ir + 5.0, t + (b - t) * 0.5 - 8.0), r - ir, 11.0, pale, false);
            }
            None => put(&mut commands, line, Vec2::new(il, t + (b - t) * 0.5 - 7.0), r - il, 10.0, pale, false),
        }
    }
    if let Some([l, t, r, _]) = rect("/EntryRequirements/GroupsizeText") {
        let players = text("players", &group_id);
        // An unlocking quest still open: named in red, and Enter's refusal.
        let quests = locked(a, &net);
        if !quests.is_empty() {
            let catalog = dialogue_catalogs.get(presentation.catalog());
            let titles: Vec<String> = quests.iter().map(|q| catalog.map_or_else(|| q.clone(), |c| c.title(q))).collect();
            let mut line = format!("Requires quest: {}", titles.join(", "));
            if *refused { line.push_str(&format!("\n{}", text("text", "enterDungeonDisabledTooltip"))); }
            // Under the description (the requirements pane has no room left).
            if let Some([dl, _, dr, db]) = rect(&format!("{ART}/TextList")) {
                put(&mut commands, line, Vec2::new(dl, db - 40.0), dr - dl - 16.0, 11.0, Color::srgb(1.0, 0.35, 0.3), false);
            }
        }
        if !players.is_empty() && players != "0" { put(&mut commands, text("text", "maxGroupSize").replace("{0:i}", &players), Vec2::new((l + r) * 0.5, t + 32.0), r - l, 12.0, pale, true); }
    }
    let Some(catalog) = catalog else { return };
    let rarity = |template: &str| catalog.items.get(template).and_then(|d| RARITIES.iter().position(|r| r.eq_ignore_ascii_case(&d.rarity))).unwrap_or(0) as u8;
    let class = players.iter().next().and_then(|c| CLASSES.get(c.desc.class as usize)).copied().unwrap_or("Warrior");
    // The possible rewards: one item, or one per class ("Warrior|x,Mage|y").
    let rewards: Vec<String> = text("rewards", &row).split(';').filter(|r| !r.is_empty()).filter_map(|entry| {
        if !entry.contains('|') { return Some(entry.to_owned()) }
        let options: Vec<(&str, &str)> = entry.split(',').filter_map(|o| o.split_once('|')).collect();
        options.iter().find(|(c, _)| c.eq_ignore_ascii_case(class)).or(options.first()).map(|(_, item)| (*item).to_owned())
    }).collect();
    for (i, template) in rewards.iter().take(12).enumerate() {
        let Some(slot) = widget(&format!("/PossibleRewardsIcons/Item{i}")) else { continue };
        draw(&mut commands, slot, 0.0, None, 0.1);
        social::item_icon(&mut commands, &assets, &screen, group, catalog, template, rarity(template), slot.k, slot.rect, None);
    }
    // What the teleport takes ("item:count;item:count"), the KeyId items first:
    // the count asked on the icon, the count carried under it.
    let mut needs: Vec<(String, u32)> = Vec::new();
    for need in text("key", &row).split(';').chain(text("needs", &row).split(';')).filter(|n| !n.is_empty()) {
        let (template, count) = need.split_once(':').unwrap_or((need, "1"));
        if !needs.iter().any(|(t, _)| t == template) { needs.push((template.to_owned(), count.parse().unwrap_or(1))); }
    }
    for (i, (template, count)) in needs.iter().take(3).enumerate() {
        for w in layout.widgets.iter().filter(|w| is_slot(&w.id) && (w.id.ends_with(&format!("/ItemsTop3/Item{i}")) || w.id.contains(&format!("/ItemsTop3/Item{i}/")))) {
            draw(&mut commands, w, 0.0, None, 0.1);
        }
        let Some(slot) = widget(&format!("/ItemsTop3/Item{i}/Icon0")) else { continue };
        let [l, t, r, b] = slot.rect;
        social::item_icon(&mut commands, &assets, &screen, group, catalog, template, rarity(template), slot.k, [l - 2.5, t - 2.5, r + 2.5, b + 2.5], None);
        if *count > 1 {
            commands.spawn((Text2d::new(count.to_string()), font(13.0), TextColor(Color::WHITE), bevy::text::TextLayout::justify(bevy::text::Justify::Right), Anchor::BOTTOM_RIGHT,
                Transform::from_translation(screen.world(0.5, r - 2.0, b).extend(0.7)).with_scale(Vec3::splat(1.0 / screen.scale)), RenderLayers::layer(LAYER), ChildOf(group)));
        }
        let owned: u32 = net.inventory.items.values().filter(|x| &x.template == template).map(|x| x.third.max(1)).sum();
        let owned = if owned > 999 { "999+".to_owned() } else { owned.to_string() };
        if let Some([l, t, r, b]) = rect(&format!("/ItemsTop3/Item{i}/ItemCount")) { put(&mut commands, owned, Vec2::new((l + r) * 0.5, (t + b) * 0.5), r - l, 12.0, Color::WHITE, true); }
    }
}

/// The loca's colour tags: "<cRRGGBBAA>" opens a colour, "</>" closes the last
/// tag, "<i>" (italic) keeps the colour.
fn colored(raw: &str, base: Color) -> Vec<(String, Color)> {
    let mut stack = vec![base];
    let mut out: Vec<(String, Color)> = vec![(String::new(), base)];
    // Some descriptions carry the escape itself ("\\n" in the loca).
    let raw = raw.replace("\\n", "\n");
    let mut rest = raw.as_str();
    while let Some(at) = rest.find('<') {
        out.last_mut().unwrap().0.push_str(&rest[..at]);
        let Some(end) = rest[at..].find('>') else { rest = &rest[at..]; break };
        let tag = &rest[at + 1..at + end];
        rest = &rest[at + end + 1..];
        if tag.starts_with('/') { if stack.len() > 1 { stack.pop(); } }
        else {
            let color = tag.strip_prefix('c').filter(|h| h.len() == 8).and_then(|h| u32::from_str_radix(h, 16).ok())
                .map(|v| Color::srgba_u8((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8)).unwrap_or(*stack.last().unwrap());
            stack.push(color);
        }
        out.push((String::new(), *stack.last().unwrap()));
    }
    out.last_mut().unwrap().0.push_str(rest);
    out.retain(|(s, _)| !s.is_empty());
    out
}
