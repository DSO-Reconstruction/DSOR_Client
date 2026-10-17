//! Lighting Menu (developer only, `DSOR_STUDIO_PREVIEW=1`): live edits of the
//! level's ambience, named presets and the global F2 multipliers.
//!
//! Kept out of crate::rendering::lighting, which only resolves each bubble
//! through the working overrides (presets::resolve_bubble) before fading to it.
//! With the menu on, F2 opens it instead of the lighting panel.

mod global;
pub(crate) mod presets;
mod presets_page;
mod window;
mod zones;

use bevy::camera_controller::free_camera::FreeCameraState;
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};

/// The menu's switch, read once.
pub fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("DSOR_STUDIO_PREVIEW").is_ok_and(|v| v == "1"))
}

pub struct LightingMenuPlugin;

impl Plugin for LightingMenuPlugin {
    fn build(&self, app: &mut App) {
        if !enabled() {
            return;
        }
        app.insert_resource(presets::LightingMenuStore::load())
            .init_resource::<global::NumericEditor>()
            .add_plugins((window::WindowPlugin, zones::ZonesPlugin, presets_page::PresetsPagePlugin))
            .add_systems(Update, (toggle, keep_cursor, blur_editors, global::systems(), zones::systems(), presets_page::systems(), hold_keys).chain());
    }
}

fn toggle(keys: Res<ButtonInput<KeyCode>>, mut roots: Query<&mut Visibility, With<window::MenuRoot>>) {
    if keys.just_pressed(KeyCode::F2) {
        for mut v in &mut roots {
            *v = if *v == Visibility::Hidden { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

/// The cursor stays visible while the menu is open (the free camera hides it).
fn keep_cursor(roots: Query<&Visibility, With<window::MenuRoot>>, mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    if roots.iter().any(|v| *v != Visibility::Hidden) {
        for mut cursor in &mut cursors {
            if !cursor.visible {
                cursor.visible = true;
            }
        }
    }
}

/// A press on any button ends the value being typed; a value's own press then
/// starts it again.
fn blur_editors(
    buttons: Query<&Interaction, (Changed<Interaction>, With<Button>)>,
    mut global: ResMut<global::NumericEditor>,
    mut zones: ResMut<zones::ZoneEditor>,
    mut name: ResMut<presets_page::NameEditor>,
) {
    if buttons.iter().any(|i| *i == Interaction::Pressed) {
        if global.typing() { global.cancel(); }
        if zones.typing() { zones.cancel(); }
        if name.typing() { name.cancel(); }
    }
}

/// While a value takes text: the game's keys (UiInput.typing) and the free
/// camera's WASD are held; both come back as they were afterwards. The hold
/// lasts one frame past the end, so the Enter or Esc that ends the text is not
/// also the game's (the chat would open on it).
fn hold_keys(
    global: Res<global::NumericEditor>,
    zones: Res<zones::ZoneEditor>,
    name: Res<presets_page::NameEditor>,
    mut input: ResMut<crate::ui::input::UiInput>,
    mut saved_enabled: Local<Option<bool>>,
    mut was_typing: Local<bool>,
    mut cameras: Query<&mut FreeCameraState>,
) {
    let editing = global.typing() || zones.typing() || name.typing();
    let typing = editing || *was_typing;
    *was_typing = editing;
    if input.hold_keys != typing {
        input.hold_keys = typing;
    }
    if typing {
        input.typing = true;
    }
    for mut camera in &mut cameras {
        if typing {
            if saved_enabled.is_none() {
                *saved_enabled = Some(camera.enabled);
            }
            camera.enabled = false;
            camera.velocity = Vec3::ZERO;
        } else if let Some(enabled) = saved_enabled.take() {
            camera.enabled = enabled;
            camera.velocity = Vec3::ZERO;
        }
    }
}
