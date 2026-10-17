//! Shared pointer ownership for Bevy panels and the mesh-rendered game UI.
//! UI keeps the raw input; world actions consult this state after UI hit testing.
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::window::PrimaryWindow;
use std::collections::HashSet;

/// Put this on a panel to include its background, padding and text in hit tests.
#[derive(Component, Default)]
#[require(RelativeCursorPosition)]
pub struct BlocksWorldInput;

/// Rectangles in the original interface's 1024 x 768 coordinate system.
/// Each rectangle follows extra screen width by its authored horizontal anchor.
#[derive(Component, Default)]
pub struct UiRegions(pub Vec<(f32, [f32; 4])>);

impl UiRegions {
    fn contains(&self, cursor: Vec2, window_size: Vec2) -> bool {
        if window_size.x <= 0.0 || window_size.y <= 0.0 { return false; }
        let scale = 768.0 / window_size.y;
        let point = cursor * scale;
        let extra_width = window_size.x * scale - 1024.0;
        self.0.iter().any(|(anchor, [left, top, right, bottom])| {
            let x = point.x - anchor * extra_width;
            x >= *left && x <= *right && point.y >= *top && point.y <= *bottom
        })
    }
}

#[derive(Resource, Default)]
pub struct UiInput {
    pub over_ui: bool,
    pub typing: bool,
    /// A tool window (crate::diagnostics::lighting_menu) is taking text: kept
    /// in `typing` until it lets go, whatever the chat's focus, and the chat
    /// leaves the keyboard alone (including the Enter that ends the text).
    pub hold_keys: bool,
    captured: HashSet<MouseButton>,
    /// A world-origin left hold keeps moving even when the cursor crosses UI.
    world_left: bool,
}

impl UiInput {
    pub fn blocks(&self, button: MouseButton) -> bool {
        self.captured.contains(&button) || (self.over_ui && !(button == MouseButton::Left && self.world_left))
    }

    fn update(&mut self, over_ui: bool, buttons: &ButtonInput<MouseButton>) {
        self.over_ui = over_ui;
        if buttons.just_pressed(MouseButton::Left) {
            self.world_left = !over_ui;
        } else if !buttons.pressed(MouseButton::Left) && !buttons.just_released(MouseButton::Left) {
            self.world_left = false;
        }
        self.captured.retain(|button| {
            !buttons.just_pressed(*button) && (buttons.pressed(*button) || buttons.just_released(*button))
        });
        // Keep UI-origin presses captured through release, including title-bar
        // drags off a panel. A world-origin left hold must never be recaptured.
        if over_ui {
            self.captured.extend(buttons.get_pressed().chain(buttons.get_just_pressed()).copied()
                .filter(|b| *b != MouseButton::Left || !self.world_left));
        }
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CaptureUiInput;

pub struct UiInputPlugin;

impl Plugin for UiInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiInput>()
            .add_systems(PreUpdate, capture.in_set(CaptureUiInput).after(bevy::ui::UiSystems::Focus));
    }
}

fn capture(
    mut input: ResMut<UiInput>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    regions: Query<(&UiRegions, &Visibility)>,
    panels: Query<(&RelativeCursorPosition, &Visibility, &InheritedVisibility), With<BlocksWorldInput>>,
    controls: Query<(&Interaction, &InheritedVisibility)>,
) {
    let mesh_ui = windows.single().ok().is_some_and(|window| {
        window.cursor_position().is_some_and(|cursor| {
            regions.iter().any(|(regions, visibility)| {
                *visibility != Visibility::Hidden && regions.contains(cursor, window.size())
            })
        })
    });
    let panel_ui = panels.iter().any(|(cursor, visibility, inherited)| {
        *visibility != Visibility::Hidden && inherited.get() && cursor.cursor_over()
    });
    let control_ui = controls.iter().any(|(interaction, visibility)| visibility.get() && *interaction != Interaction::None);
    input.update(mesh_ui || panel_ui || control_ui, &buttons);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_press_stays_captured_after_dragging_off_until_release() {
        let mut input = UiInput::default();
        let mut buttons = ButtonInput::default();
        buttons.press(MouseButton::Left);
        input.update(true, &buttons);
        assert!(input.blocks(MouseButton::Left));
        buttons.clear();
        input.update(false, &buttons);
        assert!(input.blocks(MouseButton::Left));
        assert!(!input.blocks(MouseButton::Right));
        buttons.release(MouseButton::Left);
        input.update(false, &buttons);
        assert!(input.blocks(MouseButton::Left));
        buttons.clear();
        buttons.press(MouseButton::Left);
        input.update(false, &buttons);
        assert!(!input.blocks(MouseButton::Left));
    }

    #[test]
    fn world_left_hold_crosses_ui_and_right_clicks_are_independent() {
        let mut input = UiInput::default();
        let mut buttons = ButtonInput::default();
        buttons.press(MouseButton::Left);
        input.update(false, &buttons);
        assert!(!input.blocks(MouseButton::Left));
        buttons.clear();
        input.update(true, &buttons);
        assert!(input.over_ui, "scroll still sees the UI under the cursor");
        assert!(!input.blocks(MouseButton::Left));
        buttons.press(MouseButton::Right);
        input.update(true, &buttons);
        assert!(!input.blocks(MouseButton::Left));
        assert!(input.blocks(MouseButton::Right));
        buttons.clear();
        input.update(false, &buttons);
        assert!(!input.blocks(MouseButton::Left));
        assert!(input.blocks(MouseButton::Right));
        buttons.release(MouseButton::Left);
        input.update(true, &buttons);
        assert!(!input.blocks(MouseButton::Left));
        buttons.clear();
        input.update(true, &buttons);
        assert!(input.blocks(MouseButton::Left));
        buttons.press(MouseButton::Left);
        input.update(true, &buttons);
        assert!(input.blocks(MouseButton::Left), "a fresh UI press must be captured");
    }

    #[test]
    fn legacy_regions_follow_window_scale_and_horizontal_anchors() {
        let region = |anchor| UiRegions(vec![(anchor, [400.0, 680.0, 600.0, 750.0])]);
        for size in [Vec2::new(1024.0, 768.0), Vec2::new(1920.0, 1080.0), Vec2::new(3440.0, 1440.0)] {
            for anchor in [0.0, 0.5, 1.0] {
                let scale = size.y / 768.0;
                let x = 500.0 * scale + anchor * (size.x - 1024.0 * scale);
                assert!(region(anchor).contains(Vec2::new(x, 710.0 * scale), size));
                assert!(!region(anchor).contains(Vec2::new(x, 500.0 * scale), size));
            }
        }
    }
}
