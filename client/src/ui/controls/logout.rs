//! The game menu's Switch characters: logoutdialog asks "Are you sure?"; confirmed,
//! outside towns the server answers with its countdown (PlayerLogoutInfo) and the
//! dialog counts it down ("Please wait {0:i} seconds.", confirm greyed). Only
//! its cancel or a hit (the server's PlayerLogoutInfo 0) end it; the player
//! cannot walk meanwhile (Net::logout_pending). In a town the server switches at once.
//! SOURCE: language/en/gui.window.xml logoutdialogConfirmText / logoutdialogCancelText;
//!   the user's release261_192 recording (confirm, countdown 10 -> 7, cancel).
//! FAILURE (2026-10-24): the switch went out unasked, only a chat line said so.
//! FAILURE (2026-10-30): walking away called it off; in the original the player
//!   cannot move during the countdown (the user), so a stray click no longer
//!   cancels a switch.
use super::*;
use crate::network::chat::Kind;
use dsor_proto::commands::ClientCommand;
use dsor_proto::commands::player::Logout;

pub(in crate::ui) const WINDOW: &str = "logoutdialog";
const MESSAGE: &str = "Canvas/Label_blur/Label_bg/Label_empty/Message";
const CONFIRM_TEXT: &str = "Are you sure?";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(in crate::ui) enum Stage {
    #[default]
    Closed,
    Asking,
    /// Confirmed, the server's answer still to come.
    Sent,
    /// Counting down until (real seconds).
    Waiting(f64),
}

impl Stage {
    /// The message the dialog shows.
    pub(in crate::ui) fn message(self, now: f64) -> String {
        match self {
            Stage::Waiting(until) => format!("Please wait {} seconds.", (until - now).ceil().max(0.0) as i64),
            _ => CONFIRM_TEXT.to_owned(),
        }
    }

    /// Confirmed: the request is out (the player is held in place).
    pub(in crate::ui) fn pending(self) -> bool {
        matches!(self, Stage::Sent | Stage::Waiting(_))
    }

    /// What the next frame brings: `answer` the dialog's button, `seconds` the
    /// server's countdowns since the last frame. Returns the new stage and the
    /// command to send.
    pub(in crate::ui) fn step(self, answer: Option<bool>, seconds: &[u32], now: f64) -> (Stage, Option<Logout>) {
        let mut stage = self;
        let mut send = None;
        match (stage, answer) {
            (Stage::Asking, Some(true)) => { stage = Stage::Sent; send = Some(Logout { cancel: false, plain: false }); }
            (Stage::Asking, Some(false)) => stage = Stage::Closed,
            (Stage::Sent | Stage::Waiting(_), Some(false)) => { stage = Stage::Closed; send = Some(Logout { cancel: true, plain: false }); }
            _ => {}
        }
        for &s in seconds {
            stage = match (stage, s) {
                (Stage::Closed, _) => Stage::Closed,
                (_, 0) => Stage::Closed,
                (_, s) => Stage::Waiting(now + s as f64),
            };
        }
        (stage, send)
    }
}

/// The dialog's message is drawn here (its text changes each second).
pub(in crate::ui) fn prepare(layout: &Layout) -> Layout {
    let mut out = layout.clone();
    for w in &mut out.widgets { if w.id == MESSAGE { w.text = None; } }
    out
}

#[derive(Component)] pub struct LogoutText;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::ui) fn dialog(
    mut commands: Commands,
    net: Option<NonSendMut<crate::network::net::Net>>,
    mut windows: Windows,
    mut pointer: ResMut<Pointer>,
    mut chat: ResMut<crate::network::chat::ChatLog>,
    (layouts, loaded): (Res<Assets<Layout>>, Res<crate::ui::Loaded>),
    (assets, screen, time): (Res<AssetServer>, Res<Screen>, Res<Time<Real>>),
    old: Query<Entity, With<LogoutText>>,
    (mut state, mut shown, mut entries): (Local<Stage>, Local<Option<(String, u32, u32)>>, Local<u32>),
) {
    let Some(mut net) = net else { return };
    let now = time.elapsed_secs_f64();
    // A map entered (the switch went through, or a map exit): nothing is
    // pending. Counted, not by name: the character picked again may stand on
    // the same map.
    // FAILURE (2026-10-30): switching to a character in the same town left the
    //   dialog on the screen (the user's shot).
    if net.map_entries != *entries {
        *entries = net.map_entries;
        *state = Stage::Closed;
    }
    if std::mem::take(&mut pointer.logout) && *state == Stage::Closed { *state = Stage::Asking; }
    let seconds = std::mem::take(&mut net.logout_info);
    let before = *state;
    let (stage, send) = state.step(pointer.logout_answer.take(), &seconds, now);
    if let Some(command) = send { net.send(&ClientCommand::Logout(command)); }
    // Called off by us (cancel) or by the server (a hit).
    if stage == Stage::Closed && before.pending() { chat.push(Kind::System, "The switch of characters was cancelled.".into()); }
    *state = stage;
    if net.logout_pending != stage.pending() { net.logout_pending = stage.pending(); }
    let Some((root, _, mut c, mut v, _)) = windows.iter_mut().find(|(_, n, ..)| n.0 == WINDOW) else { return };
    let open = stage != Stage::Closed;
    v.set_if_neq(if open { Visibility::Inherited } else { Visibility::Hidden });
    let waiting = stage.pending();
    if c.waiting != waiting { c.waiting = waiting; }
    let key = open.then(|| (stage.message(now), screen.width.to_bits(), screen.scale.to_bits()));
    if *shown == key { return; }
    *shown = key.clone();
    for e in &old { commands.entity(e).despawn(); }
    let (Some((text, _, _)), Some(layout)) = (key, loaded.layouts.iter().filter_map(|(h, _, _)| layouts.get(h)).find(|l| l.window == WINDOW)) else { return };
    let Some(w) = layout.widgets.iter().find(|w| w.id == MESSAGE) else { return };
    let [l, t, r, b] = w.rect;
    let font = TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(10.0 * FONT_SCALE * screen.scale), ..default() };
    commands.spawn((Text2d::new(text), font, TextColor(Color::srgb(0.9, 0.78, 0.54)),
        bevy::text::TextLayout::justify(bevy::text::Justify::Center), Anchor::CENTER, bevy::text::TextBounds::new_horizontal((r - l) * screen.scale),
        Transform::from_translation(screen.world(w.k, (l + r) * 0.5, (t + b) * 0.5).extend(WINDOW_ITEMS_DEPTH)).with_scale(Vec3::splat(1.0 / screen.scale)),
        RenderLayers::layer(LAYER), LogoutText, ChildOf(root)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_characters_asks_counts_down_and_is_called_off_by_cancel_or_a_hit() {
        // Asked; cancel closes without telling the server.
        assert_eq!(Stage::Asking.step(Some(false), &[], 0.0), (Stage::Closed, None));
        assert!(!Stage::Asking.pending());
        // Confirmed: the request goes out, the server answers 10 s, the dialog counts down.
        let (sent, cmd) = Stage::Asking.step(Some(true), &[], 0.0);
        assert_eq!((sent, cmd), (Stage::Sent, Some(Logout { cancel: false, plain: false })));
        assert!(sent.pending());
        assert_eq!(sent.message(0.0), "Are you sure?");
        let (waiting, _) = sent.step(None, &[10], 1.0);
        assert_eq!(waiting, Stage::Waiting(11.0));
        assert!(waiting.pending());
        assert_eq!(waiting.message(1.0), "Please wait 10 seconds.");
        assert_eq!(waiting.message(3.5), "Please wait 8 seconds.");
        // Nothing else happening keeps it counting; its cancel tells the server;
        // a hit (0) just closes.
        assert_eq!(waiting.step(None, &[], 2.0), (waiting, None));
        assert_eq!(waiting.step(Some(false), &[], 2.0), (Stage::Closed, Some(Logout { cancel: true, plain: false })));
        assert_eq!(waiting.step(None, &[0], 2.0), (Stage::Closed, None));
        // A countdown nobody asked for (another client's?) opens nothing.
        assert_eq!(Stage::Closed.step(None, &[10], 0.0), (Stage::Closed, None));
    }
}
