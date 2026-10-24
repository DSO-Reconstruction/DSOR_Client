//! Notifications, the group invitation, the booking receipt, the daily login
//! bonus, and the menu a middle click opens on another player (release261).
//!
//! The column: UI::NotificationWidget (2018 client 0x6CAA6B Load) clones the
//! notifications window's one button, Notification1, per pending notification and
//! gives each the icon of its type (0x6CAE52: "ui/icon_notifications_<type>").
//! Each button falls in from above the screen (`animate`, traced below) and stacks
//! upwards from Notification1's place.
//! A click opens the notification's own dialog: confirmgroupdialog's InviteDialog
//! for a group invitation (ConfirmGroupDialog, its message built by 0x53E46E), the
//! booking receipt (UI::BookingConfirmationWidget 0x620D1C) for items credited,
//! loginreward2 for the daily login bonus (UI::LoginRewardDialogWidget::SetupDialog
//! 0x6BD219, opened by 0x53D683 with dailyLoginCount and rewardIndex).
//! Its answer goes back as NotificationCommand type 2 with the entry echoed.
//! The menu: cmdmodecontextmenu's four buttons, handled by 0x64937A
//! (ButtonInviteToParty -> ClientSocialManager::GroupInvitePlayer 0x56E982,
//! ButtonWhisper -> the chat line "<whisper prefix> <name> ").
//! UNVERIFIED: the middle click as the menu's trigger is the user's account of the
//!   game (2026-10-17), not traced.

use super::*;
use crate::network::chat::Kind;
use dsor_proto::commands::social::{Foundation, GroupInvitationResponse, GroupInvite, Leave, Notification, NotificationEntry};
use dsor_proto::commands::ClientCommand;
use bevy::text::Justify;

pub(in crate::ui) const GROUP_DIALOG: &str = "confirmgroupdialog";
pub(in crate::ui) const BOOKING_DIALOG: &str = "notificationbookingconfirmation";
pub(in crate::ui) const LOGIN_DIALOG: &str = "loginreward2";
pub(in crate::ui) const CONTEXT_MENU: &str = "cmdmodecontextmenu";
/// The player trade window (tools/compose_trade_ui.py; crate::ui::controls::trade).
pub(in crate::ui) const TRADE_WINDOW: &str = "tradewindow";
const DIALOGS: [&str; 3] = [GROUP_DIALOG, BOOKING_DIALOG, LOGIN_DIALOG];

/// Notification1's rectangles (notifications.ui.json): drawn 40 x 39, 45 x 45
/// under the cursor (its mouseover state), against the left edge.
const BUTTON: [f32; 4] = [3.0, 293.0, 43.0, 332.0];
const BUTTON_HOVER: [f32; 4] = [0.0, 290.0, 45.0, 335.0];
/// The column's motion (UI::NotificationWidget, 2018 client):
/// - a new button starts with its centre one button width above the screen
///   (0x6CADDC: y = -(right - left));
/// - moving (state 1, 0x6CB66E), its target is Notification1's centre less one
///   button height per place, so later ones stack upwards; its start time is kept;
/// - each frame (0x6CA4C4) y += (now - start) x 2.5 (0xE4B588) x frame time,
///   in screen heights;
/// - it stops on its place once there (0x6CB7C3: state 2);
/// - the next waiting button starts 0.15 s (0xE79928) after the last one did;
/// - nothing moves until 10 s (0xD79C60) after the widget loaded (0x6CAD14);
/// - when one is answered, the ones after it wait again and fall to their new
///   places (0x6CA7E5).
const ACCELERATION: f32 = 2.5 * BASE_H;
const START_GAP: f64 = 0.15;
const LOAD_DELAY: f64 = 10.0;
/// Over the HUD and the chat, under the menus (crate::ui depths).
const COLUMN_Z: f32 = 50.0;
/// Inside a dialog's band, over its artwork (crate::ui::WINDOW_ART_DEPTH).
const CONTENT_Z: f32 = 7.5;
/// NotificationType: groupinvite and its join-request twin, the login reward, the
/// bookings (_Template_Notifications order; 0x6CAE52's switch).
const GROUP_INVITE: [u8; 2] = [1, 2];
/// A guild invitation (DSOR_Server services/social.py: kind 3).
const GUILD_INVITE: u8 = 3;
const LOGIN_REWARD: u8 = 6;
const BOOKING: [u8; 2] = [16, 18];
/// Game::NotificationResponse::Code (crate's Notification doc).
const CONFIRM: u8 = 1;
const CANCEL: u8 = 2;
/// The CMS grants' guid prefix (DSOR_Server services/admin_live.py
/// BOOKING_GUID_PREFIX): a moderator's credit, not a shop order.
const CMS_GUID: &[u8] = b"DSOCMS01";
/// A player's pick cylinder, metres: the character capsule's.
const PICK_RADIUS: f32 = 0.6;
const PICK_HEIGHT: f32 = 2.0;
/// loginreward2's columns: Label5Columns (Label6Columns only when a
/// "login_reward_special*" group is offered, 0x6BD219), days 0 to 4.
const LOGIN_DAYS: usize = 5;
/// One row of cmdmodecontextmenu (its buttons are 22.5 apart).
const MENU_ROW: f32 = 22.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Social { Confirm, Cancel, Whisper, Befriend, InviteToParty, Ignore, Trade, TradeMine(usize), TradeReady,
    /// The social window's group and guild buttons.
    LeaveGroup, FoundGroup, LeaveGuild, FoundGuild }

#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion { Waiting, Moving, Placed }

/// One button of the column.
struct Slot {
    guid: Vec<u8>,
    kind: u8,
    motion: Motion,
    /// Its centre, reference pixels from the top.
    y: f32,
    /// When it started moving, seconds.
    start: f64,
    entity: Option<Entity>,
    hovered: bool,
}

#[derive(Resource, Default)]
pub(crate) struct SocialUi {
    pub(super) clicked: Option<Social>,
    /// The notification the open dialog answers.
    open: Option<NotificationEntry>,
    /// The player the menu was opened on.
    pub(super) target: Option<String>,
    /// A social window row asked for the menu on `target`, where the cursor is.
    pub(super) menu_at_cursor: bool,
    pressed: Option<usize>,
    slots: Vec<Slot>,
    /// When the column loaded (the first frame in the game), seconds.
    loaded: Option<f64>,
}

/// The icon 0x6CAE52 gives a notification type.
/// The event announcements' kinds (_Template_Notifications row order).
const EVENT_ONCE: u8 = 7;
const EVENT_KEPT: u8 = 8;

fn icon(kind: u8) -> &'static str {
    match kind {
        1 | 2 => "groupinvite",
        3 => "guildinvite",
        4 | 5 => "specialoffer",
        6 => "loginreward",
        7 | 8 => "event",
        9 | 10 | 33 => "firstlogin",
        11 => "talentup",
        12 => "newskill",
        13 => "inviteprompt",
        14 => "invitationreward",
        16 | 18 => "bookingconfirmation",
        17 => "negativebooking",
        19 | 20 => "dailyachievements",
        22 => "addon",
        24 => "achievements_title",
        25..=27 => "crown",
        28..=30 => "leaderboards",
        32 => "claim1",
        trade::TRADE_INVITE => "invitationreward",
        _ => "maprework",
    }
}

/// The dialog a notification type opens, if any.
fn dialog(kind: u8) -> Option<&'static str> {
    if GROUP_INVITE.contains(&kind) || kind == GUILD_INVITE || kind == trade::TRADE_INVITE { Some(GROUP_DIALOG) }
    else if BOOKING.contains(&kind) { Some(BOOKING_DIALOG) }
    else if kind == LOGIN_REWARD { Some(LOGIN_DIALOG) }
    else { None }
}

pub(super) fn section(window: &str, parts: &[&str]) -> Option<Section> {
    let leaf = *parts.last()?;
    match window {
        // The invitation, not the join request; its text is written by `dialogs`.
        GROUP_DIALOG if parts.contains(&"JoinDialog") || leaf == "Invitetext" => Some(Section::Hidden),
        BOOKING_DIALOG => {
            // The other bookings' artwork and buttons (character edit, cinema,
            // one-time event); the text list's row template and scroll buttons
            // (its text is written by `dialogs`).
            if parts.iter().any(|p| matches!(*p, "ArtworkCharStyleEdit" | "ArtworkCinema" | "ArtworkOneTimeNotification"
                | "ConfirmCharStyleEditLabel" | "ConfirmCinemaLabel" | "TextList")) { return Some(Section::Hidden); }
            // Decline only where the booking allows it (playerCanRejectItem).
            if parts.contains(&"Deny") { return Some(Section::Page("booking_reject", 0b10)); }
            // A slot per item credited.
            let slot = parts.iter().find_map(|p| p.strip_prefix("ItemSlot").and_then(|n| n.parse::<u32>().ok()))?;
            Some(Section::Page("booking_items", !((2u32 << slot) - 1)))
        }
        LOGIN_DIALOG => {
            // The six-column variant, the icon set templates (drawn per day by
            // `dialogs`) and the description (written by `dialogs`).
            if parts.iter().any(|p| matches!(*p, "Label6Columns" | "IconSets")) || leaf == "Textlabel" { return Some(Section::Hidden); }
            // A day up to today is active and checked; a later one is locked.
            let day = parts.iter().find_map(|p| p.strip_prefix("LabelDay").and_then(|n| n.parse::<u32>().ok()))?;
            if parts.iter().any(|p| matches!(*p, "LabelActive" | "Check")) { return Some(Section::Page("login_day", !((1u32 << day) - 1))); }
            if parts.iter().any(|p| p.starts_with("Lock")) { return Some(Section::Page("login_day", (1u32 << day) - 1)); }
            None
        }
        _ => None,
    }
}

pub(super) fn action(window: &str, w: &Widget) -> Option<Action> {
    let social = match (window, w.event.as_str()) {
        (GROUP_DIALOG | BOOKING_DIALOG | LOGIN_DIALOG, "confirm") => Social::Confirm,
        (GROUP_DIALOG, "cancel") | (BOOKING_DIALOG, "deny") => Social::Cancel,
        (CONTEXT_MENU, "ButtonWhisper") => Social::Whisper,
        (CONTEXT_MENU, "ButtonBefriend") => Social::Befriend,
        (CONTEXT_MENU, "ButtonInviteToParty") => Social::InviteToParty,
        (CONTEXT_MENU, "ButtonIgnore") => Social::Ignore,
        (CONTEXT_MENU, "ButtonTrade") => Social::Trade,
        (GROUP_DIALOG | BOOKING_DIALOG | LOGIN_DIALOG | CONTEXT_MENU, _) => return Some(Action::Unavailable),
        _ => return None,
    };
    Some(Action::Social(social))
}

#[derive(Component)]
pub(in crate::ui) struct Column;

/// Notification1's centre, and its rectangle around a centre at `y`.
fn centre() -> Vec2 {
    Vec2::new((BUTTON[0] + BUTTON[2]) * 0.5, (BUTTON[1] + BUTTON[3]) * 0.5)
}

fn rect_at(y: f32, hover: bool) -> [f32; 4] {
    let [l, t, r, b] = if hover { BUTTON_HOVER } else { BUTTON };
    let dy = y - centre().y;
    [l, t + dy, r, b + dy]
}

/// The place of the button at `index`: Notification1's centre, one button height
/// higher per button before it.
fn target(index: usize) -> f32 {
    centre().y - (BUTTON[3] - BUTTON[1]) * index as f32
}

/// The column follows Net::notifications: buttons arrive, fall into place, and
/// leave when answered.
fn animate(slots: &mut [Slot], now: f64, dt: f32) {
    for (i, slot) in slots.iter_mut().enumerate() {
        if slot.motion != Motion::Moving { continue; }
        slot.y += (now - slot.start) as f32 * ACCELERATION * dt;
        if slot.y >= target(i) {
            slot.y = target(i);
            slot.motion = Motion::Placed;
        }
    }
    if let Some(next) = slots.iter().position(|s| s.motion == Motion::Waiting) {
        let last = slots.iter().filter(|s| s.motion == Motion::Moving).map(|s| s.start).next_back().unwrap_or(0.0);
        if now - last > START_GAP {
            slots[next].motion = Motion::Moving;
            slots[next].start = now;
        }
    }
}

/// The column of notifications: an icon each, falling into place, its own hit
/// test, and a click opening its dialog.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::ui) fn column(
    mut commands: Commands,
    mut net: Option<NonSendMut<crate::network::net::Net>>,
    mut social: ResMut<SocialUi>,
    mut windows: Windows,
    primary: Query<&Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    screen: Res<Screen>,
    assets: Res<AssetServer>,
    mut columns: Query<(Entity, &mut crate::ui::input::UiRegions), With<Column>>,
    mut sprites: Query<(&mut Sprite, &mut Transform), (Without<Column>, Without<Window2018>)>,
    mut test: Local<bool>,
    mut systems: ResMut<super::systems::State>,
    catalogs: Res<Assets<crate::gameplay::systems::SystemsCatalog>>,
) {
    let now = time.elapsed_secs_f64();
    // DSOR_TEST_NOTIFICATIONS=group|booking|login: a group invitation, a credited
    // booking and the daily login bonus in the column, the named one open
    // (screenshots, offline; the column moves at once).
    let test_open = (!*test).then(|| std::env::var("DSOR_TEST_NOTIFICATIONS").ok()).flatten();
    *test = true;
    if let (Some(open), Some(net)) = (&test_open, net.as_mut()) {
        net.notifications.push(NotificationEntry { note_type: 1, guid: vec![1; 16], unknown_u32_0: 0, group_id: 7, inviter_id: 9,
            params: r#"{"invitation":"group","playerName":"Simo","playerLevel":32,"playerClass":2,"groupName":"Simo's group"}"#.into(), timestamp: String::new() });
        let mut guid = CMS_GUID.to_vec();
        guid.extend([0, 0, 0, 0, 0, 0, 0, 3]);
        net.notifications.push(NotificationEntry { note_type: 16, guid, unknown_u32_0: 0, group_id: 0x06A7EB00, inviter_id: 0x06A7EB00,
            params: r#"{"ingameTransactionId":3,"templates":"epicmount_death_knight_horse","amounts":"1","rarities":"Epic","addItems":true,"playerCanRejectItem":false}"#.into(), timestamp: String::new() });
        net.notifications.push(NotificationEntry { note_type: 6, guid: vec![6; 16], unknown_u32_0: 0x0DB05EC2, group_id: 0, inviter_id: 0,
            params: r#"{"reward":"login_reward_03_warrior_01","dailyLoginCount":3,"rewardIndex":2,"premiumReward":"premium_login_reward_03_warrior_01"}"#.into(), timestamp: String::new() });
        if net.login_rewards.is_empty() {
            net.login_rewards = (1..=6).map(|d| dsor_proto::commands::player::OfferGroup {
                resource_id: format!("login_reward_{:02}_warrior_01", d.min(5)), price_rc: 0,
                offers: vec![(dsor_proto::commands::player::OfferDetails { item: format!("dailylogin_lootbox_day_{:02}", d.min(5)), amount: 1, ..default() }, 1)],
            }).collect();
        }
        social.loaded = Some(now - LOAD_DELAY);
        social.pressed = Some(match open.as_str() { "booking" => 1, "login" => 2, _ => 0 });
    }
    let loaded = *social.loaded.get_or_insert(now);
    let notes: &[NotificationEntry] = net.as_ref().map_or(&[], |n| &n.notifications);

    // Answered ones leave; the ones after them fall again (0x6CA7E5). New ones
    // join at the end, above the screen (0x6CADDC).
    let SocialUi { slots, .. } = &mut *social;
    if let Some(gone) = slots.iter().position(|s| !notes.iter().any(|n| n.guid == s.guid)) {
        if let Some(e) = slots[gone].entity { commands.entity(e).despawn(); }
        slots.remove(gone);
        for s in &mut slots[gone..] { s.motion = Motion::Waiting; }
    }
    for note in notes {
        if !slots.iter().any(|s| s.guid == note.guid) {
            slots.push(Slot { guid: note.guid.clone(), kind: note.note_type, motion: Motion::Waiting, y: -(BUTTON[2] - BUTTON[0]), start: 0.0, entity: None, hovered: false });
        }
    }
    if now - loaded > LOAD_DELAY { animate(slots, now, time.delta_secs()); }

    let point = primary.single().ok().filter(|w| w.focused && w.height() > 0.0)
        .and_then(|w| w.cursor_position().map(|p| p * (BASE_H / w.height())));
    let over = point.and_then(|p| slots.iter().position(|s| {
        let [l, t, r, b] = rect_at(s.y, false);
        p.x >= l && p.x <= r && p.y >= t && p.y <= b
    }));

    // Draw: one sprite per button, moved every frame.
    let root = match columns.iter_mut().next() {
        Some((e, mut regions)) => {
            regions.0 = slots.iter().map(|s| (0.0, rect_at(s.y, false))).collect();
            e
        }
        None => commands.spawn((Column, Transform::from_xyz(0.0, 0.0, COLUMN_Z), Visibility::Inherited, crate::ui::input::UiRegions::default())).id(),
    };
    for (i, slot) in slots.iter_mut().enumerate() {
        slot.hovered = over == Some(i);
        let [l, t, r, b] = rect_at(slot.y, slot.hovered);
        let size = Vec2::new(r - l, b - t);
        let at = screen.world(0.0, l, t).extend(0.0);
        match slot.entity.and_then(|e| sprites.get_mut(e).ok()) {
            Some((mut sprite, mut tf)) => {
                if sprite.custom_size != Some(size) { sprite.custom_size = Some(size); }
                if tf.translation != at { tf.translation = at; }
            }
            None if slot.entity.is_none() => {
                slot.entity = Some(commands.spawn((
                    Sprite { image: assets.load(format!("textures/ui/icon_notifications_{}.png", icon(slot.kind))), custom_size: Some(size), ..default() },
                    Anchor::TOP_LEFT, Transform::from_translation(at), RenderLayers::layer(LAYER), ChildOf(root),
                )).id());
            }
            None => {}
        }
    }

    if mouse.just_pressed(MouseButton::Left) && test_open.is_none() { social.pressed = over; }
    let clicked = if test_open.is_some() { social.pressed.take() }
        else if mouse.just_released(MouseButton::Left) { social.pressed.take().filter(|i| Some(*i) == over) } else { None };
    let Some(index) = clicked else { return };
    let Some(guid) = social.slots.get(index).map(|s| s.guid.clone()) else { return };
    let Some(note) = notes.iter().find(|n| n.guid == guid).cloned() else { return };
    // An event's announcement (_Template_Notifications 7 eventnonpersistent, 8
    // eventpersistent; payload {"eventId", "templateId"}, DSOR_Server
    // services/events.py event_notification_entries): the event window, on
    // that event's tab. The one-time kind is answered and leaves the column
    // (the server acknowledges any reply naming an eventId).
    // UNVERIFIED: the retail click (not traced); before, it did nothing.
    if matches!(note.note_type, EVENT_ONCE | EVENT_KEPT) {
        let id = params(&note)["eventId"].as_u64().map(|v| v as u32);
        if let Some(net) = net.as_mut() {
            let fallback = crate::gameplay::systems::SystemsCatalog::default();
            let catalog = catalogs.get(&systems.catalog).unwrap_or(&fallback);
            let minute = super::systems::unix_seconds() / 60;
            if let Some(tab) = id.and_then(|id| super::systems::events::shown(net, catalog, minute).position(|e| e.identifier == id)) {
                systems.event = tab;
                systems.event_page = None;
            }
            if note.note_type == EVENT_ONCE {
                net.send(&ClientCommand::Notification(Notification::response(CONFIRM, note.clone())));
                net.notifications.retain(|n| n.guid != note.guid);
            }
        }
        for (_, w, mut c, mut v, _) in &mut windows {
            if w.0 == super::systems::EVENTS {
                *v = Visibility::Inherited;
                c.hovered = None;
                c.pressed = None;
            }
        }
        return;
    }
    let Some(dialog) = dialog(note.note_type) else {
        info!("notification type {} has no dialog here", note.note_type);
        return;
    };
    let p = params(&note);
    for (_, w, mut c, mut v, _) in &mut windows {
        if DIALOGS.contains(&w.0.as_str()) { *v = Visibility::Hidden; }
        if w.0 == dialog {
            if dialog == BOOKING_DIALOG {
                let items = split(&p, "templates").len().min(15) as u8;
                c.pages.insert("booking_items", items);
                c.pages.insert("booking_reject", u8::from(p["playerCanRejectItem"].as_bool().unwrap_or(false)));
            }
            if dialog == LOGIN_DIALOG {
                c.pages.insert("login_day", p["rewardIndex"].as_u64().unwrap_or(0).min(LOGIN_DAYS as u64 - 1) as u8);
            }
            *v = Visibility::Inherited;
        }
    }
    social.open = Some(note);
}

fn params(note: &NotificationEntry) -> serde_json::Value {
    serde_json::from_str(&note.params).unwrap_or(serde_json::Value::Null)
}

/// A booking's comma-separated list ("templates", "amounts", "rarities").
fn split(params: &serde_json::Value, key: &str) -> Vec<String> {
    match &params[key] {
        serde_json::Value::String(s) => s.split(',').map(|p| p.trim().to_owned()).filter(|p| !p.is_empty()).collect(),
        serde_json::Value::Array(a) => a.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).collect(),
        serde_json::Value::Null => Vec::new(),
        v => vec![v.to_string()],
    }
}

#[derive(Component)]
pub(in crate::ui) struct DialogContent;

/// The open dialog's own text and items, inside its window.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::ui) fn dialogs(
    mut commands: Commands,
    social: Res<SocialUi>,
    windows: Query<(Entity, &Window2018, &WindowControls, &Visibility)>,
    old: Query<Entity, With<DialogContent>>,
    data: Res<items::Data>,
    catalogs: Res<Assets<items::Catalog>>,
    layouts: Res<Assets<Layout>>,
    loaded: Res<crate::ui::Loaded>,
    screen: Res<Screen>,
    assets: Res<AssetServer>,
    net: Option<NonSend<crate::network::net::Net>>,
    mut last: Local<Option<(Entity, Vec<u8>, u32, bool)>>,
) {
    let open = social.open.as_ref().and_then(|note| windows.iter()
        .find(|(_, w, _, v)| **v != Visibility::Hidden && Some(w.0.as_str()) == dialog(note.note_type))
        .map(|(e, w, _, _)| (e, w.0.clone(), note)));
    let catalog = catalogs.get(&data.catalog);
    let key = open.as_ref().map(|(e, _, note)| (*e, note.guid.clone(), screen.scale.to_bits(), catalog.is_some()));
    if *last == key { return; }
    *last = key;
    for e in &old { commands.entity(e).despawn(); }
    let Some((root, window, note)) = open else { return };
    let layout = loaded.layouts.iter().filter_map(|(h, _, _)| layouts.get(h)).find(|l| l.window == window);
    let widget = |id: &str| layout.and_then(|l| l.widgets.iter().find(|w| w.id.ends_with(id)));
    let group = commands.spawn((DialogContent, Transform::from_xyz(0.0, 0.0, CONTENT_Z), Visibility::Inherited, ChildOf(root))).id();
    let p = params(note);
    if window == GROUP_DIALOG && note.note_type == trade::TRADE_INVITE {
        let text = format!("{}\nwants to trade with you.\n\nWould you like to trade?", p["playerName"].as_str().unwrap_or("?"));
        let Some(w) = widget("/InviteDialog/Invitetext") else { return };
        let [l, t, r, _] = w.rect;
        label(&mut commands, &assets, &screen, group, text, screen.world(w.k, l, t).extend(0.1), 12.0, Color::srgb(0.92, 0.87, 0.76), r - l, Justify::Center);
        return;
    }
    if window == GROUP_DIALOG && note.note_type == GUILD_INVITE {
        // gui.window.guild_confirmJoinGuild: the same dialog names the guild.
        let class = ["Warrior", "Mage", "Ranger", "Dwarf", "Niwalk"].get(p["playerClass"].as_u64().unwrap_or(9) as usize).copied().unwrap_or("");
        let text = format!("{}\n({} Level: {})\nhas invited you to join their guild:\n{}\n\nWould you like to join?",
            p["playerName"].as_str().unwrap_or("?"), class, p["playerLevel"].as_u64().unwrap_or(1), p["guildName"].as_str().unwrap_or(""));
        let Some(w) = widget("/InviteDialog/Invitetext") else { return };
        let [l, t, r, _] = w.rect;
        label(&mut commands, &assets, &screen, group, text, screen.world(w.k, l, t).extend(0.1), 12.0, Color::srgb(0.92, 0.87, 0.76), r - l, Justify::Center);
        return;
    }
    if window == GROUP_DIALOG {
        // gui.window.grouping_confirmJoinGroup, its rank line empty (titleType 0).
        let class = ["Warrior", "Mage", "Ranger", "Dwarf", "Niwalk"].get(p["playerClass"].as_u64().unwrap_or(9) as usize).copied().unwrap_or("");
        let text = format!("{}\n({} Level: {})\nhas invited you to join their group:\n{}\n\nWould you like to join?",
            p["playerName"].as_str().unwrap_or("?"), class, p["playerLevel"].as_u64().unwrap_or(1), p["groupName"].as_str().unwrap_or(""));
        let Some(w) = widget("/InviteDialog/Invitetext") else { return };
        let [l, t, r, _] = w.rect;
        label(&mut commands, &assets, &screen, group, text, screen.world(w.k, l, t).extend(0.1), 12.0, Color::srgb(0.92, 0.87, 0.76), r - l, Justify::Center);
        return;
    }
    if window == LOGIN_DIALOG {
        login(&mut commands, &assets, &screen, group, &p, layout, catalog, net.as_ref().map_or(&[], |n| &n.login_rewards));
        return;
    }
    // gui.window.booking: a moderator's credit or a shop order, then the items.
    let cms = note.guid.starts_with(CMS_GUID);
    let mut text = String::from(if cms { "A moderator has credited a pack to your account." } else { "We received an order." });
    text.push_str("\n\n");
    let (templates, amounts, rarities) = (split(&p, "templates"), split(&p, "amounts"), split(&p, "rarities"));
    for (i, template) in templates.iter().enumerate() {
        let name = catalog.and_then(|c| c.items.get(template)).map_or(template.as_str(), |d| d.name.as_str());
        text.push_str(&format!("{} x {}\n", amounts.get(i).map_or("1", String::as_str), name));
    }
    if let Some(w) = widget("/ContentLabel/TextList") {
        let [l, t, r, _] = w.rect;
        label(&mut commands, &assets, &screen, group, text, screen.world(w.k, l, t + 4.0).extend(0.1), 12.0, Color::srgb(0.92, 0.87, 0.76), r - l - 20.0, Justify::Left);
    }
    let Some(catalog) = catalog else { return };
    const RARITIES: [&str; 8] = ["Common", "Uncommon", "Magic", "Rare", "Epic", "Unique", "Set", "Mythic"];
    for (i, template) in templates.iter().take(15).enumerate() {
        let Some(slot) = widget(&format!("/ContentLabel/ItemSlot{i}")) else { continue };
        let rarity = rarities.get(i).and_then(|r| RARITIES.iter().position(|n| n.eq_ignore_ascii_case(r))).unwrap_or(0);
        item_icon(&mut commands, &assets, &screen, group, catalog, template, rarity as u8, slot.k, slot.rect, None);
    }
}

/// An item's background, icon and frame in a slot rectangle, and its amount.
#[allow(clippy::too_many_arguments)]
pub(super) fn item_icon(commands: &mut Commands, assets: &AssetServer, screen: &Screen, parent: Entity, catalog: &items::Catalog,
    template: &str, rarity: u8, k: f32, [l, t, r, b]: [f32; 4], amount: Option<u32>) {
    let item = dsor_proto::commands::inventory::ItemInfo { kind: rarity, ..default() };
    let (icon, background, frame) = catalog.icon_parts(template, Some(&item));
    let at = screen.world(k, l + 2.5, t + 2.5);
    let size = Vec2::new(r - l, b - t) - Vec2::splat(5.0);
    for (z, image) in [(0.2, background), (0.3, icon), (0.4, frame)] {
        let Some(image) = image else { continue };
        commands.spawn((Sprite { image: assets.load(image.to_owned()), custom_size: Some(size), ..default() }, Anchor::TOP_LEFT,
            Transform::from_translation(at.extend(z)), RenderLayers::layer(LAYER), ChildOf(parent)));
    }
    if let Some(n) = amount.filter(|n| *n > 1) {
        label(commands, assets, screen, parent, n.to_string(), screen.world(k, l, b - 16.0).extend(0.5), 10.0, Color::WHITE, r - l - 6.0, Justify::Right);
    }
}

/// loginreward2 (UI::LoginRewardDialogWidget::SetupDialog 0x6BD219): the
/// description with the days in a row, and per day column the items of that
/// day's offer group, laid out as IconSet<n-1> (n: up to 3 items) centred on the
/// column. The groups are SpecialOfferCommand's (Net::login_rewards).
/// UNVERIFIED: the premium row (ItemSlot%iPremiumFull, shown with a membership)
///   is not drawn; the future days' half-transparent bars (0x6BD219 sets their
///   colour alpha to 0.5) are drawn opaque.
#[allow(clippy::too_many_arguments)]
fn login(commands: &mut Commands, assets: &AssetServer, screen: &Screen, group: Entity, p: &serde_json::Value,
    layout: Option<&Layout>, catalog: Option<&items::Catalog>, rewards: &[dsor_proto::commands::player::OfferGroup]) {
    let Some(layout) = layout else { return };
    let widget = |id: &str| layout.widgets.iter().find(|w| w.id.ends_with(id));
    const COLUMNS: &str = "Canvas/LabelBlur/Label5Columns/BackGroundDarkLabel/ArtworkLabel";
    // gui.window.loginreward_descriptionLabel / descriptionLabelSingular.
    let count = p["dailyLoginCount"].as_u64().unwrap_or(1);
    let text = if count > 1 {
        format!("Congratulations, you were online {count} days in a row.\nCome back tomorrow to collect the next reward.\nThe first time you log in 5 days in a row, you will receive a unique reward!")
    } else {
        "Congratulations, you successfully logged in today!\nCome back tomorrow to collect the next reward.\nThe first time you log in 5 days in a row, you will receive a one-time reward!".to_owned()
    };
    if let Some(w) = widget(&format!("{COLUMNS}/TextlabelHeadline/Textlabel")) {
        let [l, t, r, _] = w.rect;
        label(commands, assets, screen, group, text, screen.world(w.k, l, t).extend(0.1), 11.0, Color::srgb(0.92, 0.87, 0.76), r - l, Justify::Center);
    }
    let Some(catalog) = catalog else { return };
    for (day, reward) in rewards.iter().take(LOGIN_DAYS).enumerate() {
        let Some(column) = widget(&format!("{COLUMNS}/LabelDay{day}")) else { continue };
        let n = reward.offers.len().min(3);
        if n == 0 { continue; }
        let set = format!("Canvas/LabelBlur/IconSets/IconSet{}", n - 1);
        let Some(set_rect) = layout.widgets.iter().find(|w| w.id == set).map(|w| w.rect) else { continue };
        let [cl, ct, cr, cb] = column.rect;
        let offset = Vec2::new((cl + cr - set_rect[0] - set_rect[2]) * 0.5, (ct + cb - set_rect[1] - set_rect[3]) * 0.5);
        for (i, (offer, amount)) in reward.offers.iter().take(n).enumerate() {
            let Some(slot) = layout.widgets.iter().find(|w| w.id == format!("{set}/ItemSlot{i}")) else { continue };
            let [l, t, r, b] = slot.rect;
            let rect = [l + offset.x, t + offset.y, r + offset.x, b + offset.y];
            item_icon(commands, assets, screen, group, catalog, &offer.item, offer.rarity as u8, column.k, rect, Some(offer.amount * amount));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn label(commands: &mut Commands, assets: &AssetServer, screen: &Screen, parent: Entity, value: String, at: Vec3, size: f32, color: Color, width: f32, justify: Justify) {
    commands.spawn((Text2d::new(value), TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(size * FONT_SCALE * screen.scale), ..default() },
        TextColor(color), bevy::text::TextLayout::justify(justify), Anchor::TOP_LEFT, bevy::text::TextBounds::new_horizontal(width * screen.scale),
        Transform::from_translation(at).with_scale(Vec3::splat(1.0 / screen.scale)), RenderLayers::layer(LAYER), ChildOf(parent)));
}

/// The clicked dialog and menu buttons: the answers and the menu's actions.
#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn answer(
    net: Option<NonSendMut<crate::network::net::Net>>,
    mut social: ResMut<SocialUi>,
    mut trade: ResMut<trade::TradeUi>,
    mut windows: Windows,
    mut pointer: ResMut<Pointer>,
    mut chat: ResMut<crate::network::chat::ChatLog>,
) {
    let Some(mut net) = net else { return };
    // The inviter's answer (handler 0x5709BB, gui.window.grouping texts).
    for (code, name) in std::mem::take(&mut net.group_answers) {
        chat.push(Kind::System, match code {
            0 => format!("{name} has joined your group."),
            1 => format!("{name} did not join your group."),
            2 => format!("{name} is already in a group."),
            3 => format!("{name} is not currently logged in."),
            4 => format!("{name} could not be found."),
            5 => "Only the group leader may use this command.".to_owned(),
            c => format!("{name}: group invitation answer {c}."),
        });
    }
    // The inviter's answer to a guild invitation (171; DSOR_Server services/social.py).
    for (code, name) in std::mem::take(&mut net.guild_answers) {
        chat.push(Kind::System, match code {
            0 => format!("{name} has joined your guild."),
            1 => format!("{name} declined the guild invitation."),
            2 => format!("{name} is already in a guild."),
            4 => format!("{name} could not be found."),
            5 => "Only the guild leader and its officers may invite.".to_owned(),
            6 => "You are not in a guild.".to_owned(),
            c => format!("{name}: guild invitation answer {c}."),
        });
    }
    for line in std::mem::take(&mut net.social_notes) { chat.push(Kind::System, line); }
    // The travel map's destination: the server moves us when the map is discovered.
    if let Some(map) = pointer.travel.take() {
        net.send(&ClientCommand::Travel(dsor_proto::commands::player::Travel::to_map(&map)));
        chat.push(Kind::System, format!("Travelling to {}.", map.replace('_', " ")));
    }
    let Some(clicked) = social.clicked.take() else { return };
    let me = net.local_name.clone().unwrap_or_default();
    match clicked {
        Social::TradeMine(n) => trade.remove = Some(n),
        Social::TradeReady => trade.ready = true,
        Social::Confirm | Social::Cancel => {
            let Some(note) = social.open.take() else { return };
            let confirm = clicked == Social::Confirm;
            if note.note_type == trade::TRADE_INVITE {
                // The answer to a trade invitation goes to the trade, not to the
                // notification system (the server never sent it as one).
                net.send(&ClientCommand::TradeAnswer(dsor_proto::commands::trade::TradeAnswer { accept: confirm }));
                net.notifications.retain(|n| n.guid != note.guid);
                for (_, w, _, mut v, _) in &mut windows {
                    if DIALOGS.contains(&w.0.as_str()) { *v = Visibility::Hidden; }
                }
                return;
            }
            if GROUP_INVITE.contains(&note.note_type) {
                // AcceptGroupInvite 0x56BB3D: code 0, the inviter, the group;
                // RefuseGroupInvite 0x574E80: code 1. The server checks the name
                // against our own (DSOR_Server services/social.py
                // _handle_invitation_response).
                net.send(&ClientCommand::GroupInvitationResponse(GroupInvitationResponse {
                    code: if confirm { 0 } else { 1 }, a: note.inviter_id, b: note.group_id, name: me.clone(),
                }));
            }
            if note.note_type == GUILD_INVITE {
                // The same answer, to the guild command (171): the server joins on 0,
                // declines on 1, and refuses every other code.
                net.send(&ClientCommand::GuildInvitationResponse(GroupInvitationResponse {
                    code: if confirm { 0 } else { 1 }, a: note.inviter_id, b: note.group_id, name: me.clone(),
                }));
            }
            net.send(&ClientCommand::Notification(Notification::response(if confirm { CONFIRM } else { CANCEL }, note.clone())));
            net.notifications.retain(|n| n.guid != note.guid);
            for (_, w, _, mut v, _) in &mut windows {
                if DIALOGS.contains(&w.0.as_str()) { *v = Visibility::Hidden; }
            }
        }
        // The social window's buttons (DSOR_Server services/social.py: a GroupStatus or
        // GuildStatus answers the leave, a foundation response the founding).
        Social::LeaveGroup => net.send(&ClientCommand::GroupLeave(Leave { unknown_u32_0: 0 })),
        Social::LeaveGuild => net.send(&ClientCommand::GuildLeave(Leave { unknown_u32_0: 0 })),
        Social::FoundGroup => net.send(&ClientCommand::GroupFoundation(Foundation { unknown_u32_0: 0, source_name: me.clone(), name: format!("{me}'s group"), unknown_bool_1: false })),
        Social::FoundGuild => {
            // The guild's name (3 to 32 letters): what the window's search field holds.
            let name = windows.iter().find(|(_, w, _, _, _)| w.0 == "socialscreen")
                .and_then(|(_, _, c, _, _)| c.entries.iter().find(|(id, _)| id.ends_with("/InputQuery")).map(|(_, e)| e.text.trim().to_owned()))
                .unwrap_or_default();
            if name.chars().count() < 3 { chat.push(Kind::Local, "Type the guild's name in the social window's search field, then press Found Guild.".into()); }
            else { net.send(&ClientCommand::GuildFoundation(Foundation { unknown_u32_0: 0, source_name: me.clone(), name, unknown_bool_1: false })); }
        }
        Social::InviteToParty | Social::Whisper | Social::Befriend | Social::Ignore | Social::Trade => {
            for (_, w, _, mut v, _) in &mut windows {
                if w.0 == CONTEXT_MENU { *v = Visibility::Hidden; }
            }
            let Some(target) = social.target.take() else { return };
            match clicked {
                // GroupInvitePlayer 0x56E982: our name, theirs, then 0.
                Social::InviteToParty => {
                    net.send(&ClientCommand::GroupInvite(GroupInvite { inviter_name: me, invitee_name: target.clone(), unknown_u32_0: 0 }));
                    chat.push(Kind::System, "Invitation was sent.".into());
                }
                Social::Trade => net.send(&ClientCommand::TradeRequest(dsor_proto::commands::trade::TradeRequest { target: target.clone() })),
                // The chat line, ready to write to them.
                Social::Whisper => {
                    set_chat_compact(false, &mut windows, &mut pointer);
                    if let Some((root, id)) = chat_entry(&windows) {
                        if let Ok((_, _, mut c, _, _)) = windows.get_mut(root) {
                            let text = format!("/w {target} ");
                            c.entries.insert(id.clone(), TextEntry { cursor: text.len(), text, selected: false });
                        }
                        pointer.focus = Some((root, id));
                    }
                }
                // BuddyListCommand / the ignore list: the server refuses every
                // change to them (DSOR_Server services/social.py _handle_buddy_list).
                _ => chat.push(Kind::Local, "The friend and ignore lists are not available on this server yet.".into()),
            }
        }
    }
}

/// A middle click on another player opens the menu where the cursor is; any
/// other click closes it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::ui) fn context_menu(
    ui: Res<crate::ui::input::UiInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    primary: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    players: Query<(&GlobalTransform, &crate::actors::nameplate::Nameplate, &InheritedVisibility),
        (With<crate::network::net::RemotePlayer>, Without<crate::actors::monsters::Monster>)>,
    mut social: ResMut<SocialUi>,
    mut windows: Windows,
    screen: Res<Screen>,
    mut test: Local<bool>,
) {
    // DSOR_TEST_MENU=1: the menu opened on "Simo" at (400, 300) (screenshots).
    let test_open = !*test && std::env::var_os("DSOR_TEST_MENU").is_some();
    *test = true;
    let open = windows.iter().any(|(_, w, _, v, _)| w.0 == CONTEXT_MENU && *v != Visibility::Hidden);
    if open && (mouse.just_released(MouseButton::Left) || mouse.just_pressed(MouseButton::Right)) {
        for (_, w, _, mut v, _) in &mut windows {
            if w.0 == CONTEXT_MENU { *v = Visibility::Hidden; }
        }
    }
    // A social window row (crate::ui::controls::interact): its player, at the cursor.
    let from_row = social.menu_at_cursor;
    if from_row { social.menu_at_cursor = false; }
    if !test_open && !from_row && (!mouse.just_pressed(MouseButton::Middle) || ui.over_ui) { return; }
    let Ok(window) = primary.single() else { return };
    let scale = window.height().max(1.0) / BASE_H;
    let picked = if test_open { Some(("Simo".to_owned(), Vec2::new(400.0, 300.0))) }
        else if from_row { social.target.clone().zip(window.cursor_position().map(|cursor| cursor / scale)) }
        else { window.cursor_position().and_then(|cursor| pick(cursor, &cameras, &players).map(|name| (name, cursor / scale))) };
    let Some((name, p)) = picked else { return };
    social.target = Some(name);
    // The menu's top-left corner at the cursor, kept on the screen. Its Canvas:
    // [512, 364, 730, 476], k 0 (prepare_menu).
    let (w, h) = (730.0 - 512.0, 476.0 + MENU_ROW - 364.0);
    let left = p.x.min(screen.width - w).max(0.0);
    let top = p.y.min(BASE_H - h).max(0.0);
    let anchor = 512.0;
    for (_, win, mut c, mut v, mut tf) in &mut windows {
        if win.0 != CONTEXT_MENU { continue; }
        tf.translation.x = left - anchor;
        tf.translation.y = -(top - 364.0);
        c.hovered = None;
        c.pressed = None;
        *v = Visibility::Inherited;
    }
}

/// The nearest other player under the cursor: a ray against their upright pick
/// cylinders (as crate::actors::monsters::hover does for a model without boxes).
#[allow(clippy::type_complexity)]
fn pick(
    cursor: Vec2,
    cameras: &Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    players: &Query<(&GlobalTransform, &crate::actors::nameplate::Nameplate, &InheritedVisibility),
        (With<crate::network::net::RemotePlayer>, Without<crate::actors::monsters::Monster>)>,
) -> Option<String> {
    let ray = cameras.single().ok().and_then(|(cam, at)| cam.viewport_to_world(at, cursor).ok())?;
    let (o, d) = (ray.origin, *ray.direction);
    let mut best: Option<(f32, &str)> = None;
    for (at, plate, shown) in players {
        if !shown.get() || plate.text.is_empty() { continue; }
        let base = at.translation();
        let (ox, oz) = (o.x - base.x, o.z - base.z);
        let (a, b) = (d.x * d.x + d.z * d.z, ox * d.x + oz * d.z);
        if a < 1e-6 { continue; }
        let t = (-b / a).max(0.0);
        let (px, pz) = (ox + d.x * t, oz + d.z * t);
        let y = o.y + d.y * t - base.y;
        if px * px + pz * pz > PICK_RADIUS * PICK_RADIUS || !(-0.2..=PICK_HEIGHT).contains(&y) { continue; }
        if best.is_none_or(|(bt, _)| t < bt) { best = Some((t, &plate.text)); }
    }
    best.map(|(_, name)| name.to_owned())
}

/// cmdmodecontextmenu with a fifth row, Trade (a DSOR addition: the game has no
/// trade): a copy of Ignore's button one row lower, and the menu's frame grown by
/// one row through its middle (its points below the buttons' middle move down).
pub(in crate::ui) fn prepare_menu(layout: &Layout) -> Layout {
    let mut out = layout.clone();
    // Placed at the cursor from the screen's left edge (context_menu).
    for w in &mut out.widgets { w.k = 0.0; }
    const SPLIT: f32 = 430.0;
    let stretch = |w: &mut Widget| {
        if w.rect[3] > SPLIT { w.rect[3] += MENU_ROW; }
        if w.rect[1] > SPLIT { w.rect[1] += MENU_ROW; }
        if let Some(art) = &mut w.art {
            for y in art.pos.iter_mut().skip(1).step_by(2) { if *y > SPLIT { *y += MENU_ROW; } }
        }
    };
    for w in out.widgets.iter_mut().filter(|w| w.id == "Canvas" || w.id == "Canvas/LabelDropDownMenu") { stretch(w); }
    const IGNORE: &str = "Canvas/LabelDropDownMenu/TextButtonIgnore";
    let trade: Vec<Widget> = layout.widgets.iter().filter(|w| w.id == IGNORE || w.id.starts_with(&format!("{IGNORE}/"))).map(|w| {
        let mut w = w.clone();
        w.k = 0.0;
        w.id = w.id.replacen("TextButtonIgnore", "TextButtonTrade", 1);
        if let Some(b) = &mut w.button { *b = b.replacen("TextButtonIgnore", "TextButtonTrade", 1); }
        if w.event == "ButtonIgnore" { w.event = "ButtonTrade".into(); }
        w.rect[1] += MENU_ROW;
        w.rect[3] += MENU_ROW;
        if let Some(art) = &mut w.art { for y in art.pos.iter_mut().skip(1).step_by(2) { *y += MENU_ROW; } }
        if let Some(t) = &mut w.text { t.text = "Trade".into(); t.key = "gui.window.cmdmode_buttonTrade".into(); }
        w
    }).collect();
    out.widgets.extend(trade);
    out
}
