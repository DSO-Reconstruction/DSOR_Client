//! The question the original asks before a sticker's next rarity tier is
//! bought: the confirmation dialog (items::DESTROY_DIALOG) with its Default
//! layer, centred: "Do you want to upgrade <tier> for <n> Altered
//! Crescerite(s)". Confirm sends the upgrade (UpgradeStickerbookItem); Cancel
//! or Escape drop it.
//!
//! EVIDENCE: gui.window.messages stickerbookUpgradeDialogue (the question's
//!   text); the user's recording of the original (20261031-1506-03): the
//!   plain question with the tick and the cross, "Do you want to upgrade Blue
//!   Dragonspawn for 5 Altered Crescerite(s)?", then 25, then 50.
use super::*;
use dsor_proto::commands::systems::UpgradeStickerbookItem;
use dsor_proto::commands::ClientCommand;

/// The dialog's page for this question (consent::PAGES).
pub(in crate::ui) const UPGRADE: u8 = 2;
const QUESTION: &str = "Canvas/DialogCenter/Content/Default/Question";
/// gui.window.messages stickerbookUpgradeDialogue.
const TEXT: &str = "Do you want to upgrade {0:s} for {0:l} Altered Crescerite(s)?";

/// A sticker tier to buy, waiting for the player's word: the name the server
/// owns the sticker under, the next tier's name, its price.
#[derive(Clone, PartialEq, Debug)]
pub(in crate::ui) struct Ask { pub key: String, pub name: String, pub cost: u32 }

/// The question written on the dialog.
#[derive(Component)]
pub(in crate::ui) struct UpgradeText;

/// The question's words.
fn question(ask: &Ask) -> String { TEXT.replacen("{0:s}", &ask.name, 1).replacen("{0:l}", &ask.cost.to_string(), 1) }

/// Opens the dialog for an asked upgrade, writes its question, sends the
/// upgrade on Confirm and closes it once answered. Nothing to do while
/// nothing is asked.
#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn sync(mut commands: Commands, mut windows: Windows, mut pointer: ResMut<Pointer>, net: Option<NonSendMut<crate::network::net::Net>>,
    (layouts, loaded, assets, screen): (Res<Assets<Layout>>, Res<Loaded>, Res<AssetServer>, Res<Screen>),
    old: Query<Entity, With<UpgradeText>>, mut asked: Local<Option<Ask>>, mut written: Local<Option<(Entity, u32, u32)>>) {
    let new = pointer.upgrade_ask.take();
    let answer = pointer.upgrade_answer.take();
    if new.is_none() && asked.is_none() && written.is_none() { return; }
    let Some((root, mut c, mut v)) = windows.iter_mut().find(|(_, n, _, _, _)| n.0 == items::DESTROY_DIALOG).map(|(e, _, c, v, _)| (e, c, v)) else { return };
    let open = *v != Visibility::Hidden && c.page(consent::PAGES) == UPGRADE;
    if let Some(ask) = new {
        // Another question on the dialog is answered first.
        if *v == Visibility::Hidden || open {
            c.set_page(consent::PAGES, UPGRADE);
            c.hovered = None;
            c.pressed = None;
            *v = Visibility::Inherited;
            *asked = Some(ask);
        }
    } else if let Some(yes) = answer.filter(|_| open) {
        if let (true, Some(ask), Some(mut net)) = (yes, asked.as_ref(), net) {
            if !net.offline { net.send(&ClientCommand::UpgradeStickerbookItem(UpgradeStickerbookItem { id: ask.key.clone() })); }
        }
        *asked = None;
        *v = Visibility::Hidden;
    } else if asked.is_some() && !open {
        // Closed another way (Escape): no upgrade.
        *asked = None;
    }
    let key = asked.as_ref().map(|_| (root, screen.width.to_bits(), screen.scale.to_bits()));
    if *written == key { return; }
    for e in &old { commands.entity(e).despawn(); }
    *written = key;
    let Some(ask) = asked.as_ref() else { return };
    let Some(w) = loaded.layouts.iter().filter_map(|(h, _, _)| layouts.get(h)).find(|l| l.window == items::DESTROY_DIALOG)
        .and_then(|l| l.widgets.iter().find(|w| w.id == QUESTION)) else { return };
    let [l, t, r, b] = w.rect;
    // As the destroy question (items::destroy_dialog) is written.
    let font = TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(9.0 * FONT_SCALE * screen.scale), ..default() };
    commands.spawn((UpgradeText, Text2d::new(question(ask)), font, TextColor(Color::srgb(0.9, 0.78, 0.54)),
        bevy::text::TextLayout::justify(bevy::text::Justify::Center), Anchor::CENTER, bevy::text::TextBounds::new_horizontal((r - l) * screen.scale),
        Transform::from_translation(screen.world(w.k, (l + r) * 0.5, (t + b) * 0.5).extend(WINDOW_ITEMS_DEPTH + 0.2)).with_scale(Vec3::splat(1.0 / screen.scale)),
        RenderLayers::layer(LAYER), ChildOf(root)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_names_the_tier_and_its_price() {
        let ask = Ask { key: "pet_karabossa_uncommon".into(), name: "Karabossa Doll".into(), cost: 5 };
        assert_eq!(question(&ask), "Do you want to upgrade Karabossa Doll for 5 Altered Crescerite(s)?");
    }
}
