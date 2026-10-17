//! The Lighting page's global render controls: the lighting panel's multipliers
//! (crate::rendering::lighting::FIELDS, saved to look.json), each with - / +
//! (Shift: x10) and a value to click and type (Enter applies, Esc cancels).
//! While a value takes text the game's keys and the free camera are held
//! (crate::diagnostics::lighting_menu).

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;

use super::window::menu_text;
use crate::rendering::lighting::{save, Lighting, FIELDS};

#[derive(Component)]
struct ValueText(usize);
#[derive(Component)]
struct NumericField(usize);
#[derive(Component)]
struct StepButton(usize, f32);

/// The value being typed, if any.
#[derive(Resource, Default)]
pub(super) struct NumericEditor {
    field: Option<usize>,
    draft: String,
    replace_on_next_input: bool,
}

impl NumericEditor {
    pub(super) fn typing(&self) -> bool { self.field.is_some() }

    pub(super) fn cancel(&mut self) {
        self.field = None;
        self.draft.clear();
        self.replace_on_next_input = false;
    }
}

/// The GLOBAL RENDER CONTROLS card.
pub(super) fn spawn(page: &mut ChildSpawnerCommands) {
    page.spawn((
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            padding: UiRect::all(Val::Px(12.0)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
    ))
    .with_children(|card| {
        card.spawn(menu_text("GLOBAL RENDER CONTROLS", 12.0, Color::srgb(0.91, 0.73, 0.43)));
        card.spawn(menu_text(
            "Click a value to type; Enter applies; Esc cancels. Shift adjusts by x10.",
            11.0,
            Color::srgb(0.64, 0.70, 0.79),
        ));
        for (i, field) in FIELDS.iter().enumerate() {
            card.spawn(Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(30.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(5.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Text::new(field.label),
                    TextFont { font_size: bevy::text::FontSize::Px(12.0), ..default() },
                    TextColor(Color::srgb(0.83, 0.86, 0.91)),
                    Node { width: Val::Px(140.0), ..default() },
                ));
                for (sign, label) in [(-1.0, "-"), (1.0, "+")] {
                    row.spawn((
                        Button,
                        StepButton(i, sign),
                        Node {
                            width: Val::Px(24.0),
                            min_height: Val::Px(25.0),
                            flex_shrink: 0.0,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Text::new(label),
                            TextFont { font_size: bevy::text::FontSize::Px(13.0), ..default() },
                            TextColor(Color::srgb(0.94, 0.95, 0.97)),
                        ));
                    });
                }
                row.spawn((
                    Button,
                    NumericField(i),
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(58.0),
                        max_width: Val::Px(100.0),
                        min_height: Val::Px(25.0),
                        flex_shrink: 0.0,
                        padding: UiRect::axes(Val::Px(5.0), Val::Px(3.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.07, 0.09, 0.13)),
                ))
                .with_children(|value| {
                    value.spawn((
                        ValueText(i),
                        Text::new(""),
                        TextFont { font_size: bevy::text::FontSize::Px(12.0), ..default() },
                        TextColor(Color::srgb(0.94, 0.95, 0.97)),
                    ));
                });
            });
        }
    });
}

/// The page systems, run by the menu after the editors are blurred.
pub(super) fn systems() -> impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, ()> {
    (press_buttons, edit_numeric_fields, refresh_values).chain()
}

fn press_buttons(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<(&Interaction, &StepButton), Changed<Interaction>>,
    mut lighting: ResMut<Lighting>,
    mut editor: ResMut<NumericEditor>,
) {
    let mul = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 10.0 } else { 1.0 };
    let mut changed = false;
    for (interaction, StepButton(i, sign)) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let f = &FIELDS[*i];
        let v = (f.get)(&lighting) + f.step * sign * mul;
        (f.set)(&mut lighting, v);
        changed = true;
    }
    if changed {
        editor.cancel();
        save(&lighting);
    }
}

pub(super) fn format_edit_value(value: f32, step: f32) -> String {
    if step >= 1.0 {
        return format!("{value:.0}");
    }
    let mut result = format!("{value:.5}");
    while result.contains('.') && result.ends_with('0') {
        result.pop();
    }
    if result.ends_with('.') {
        result.pop();
    }
    result
}

fn edit_numeric_fields(
    mut events: MessageReader<KeyboardInput>,
    fields: Query<(&Interaction, &NumericField), Changed<Interaction>>,
    mut lighting: ResMut<Lighting>,
    mut editor: ResMut<NumericEditor>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    for (interaction, NumericField(index)) in &fields {
        if *interaction == Interaction::Pressed {
            editor.field = Some(*index);
            editor.draft = format_edit_value((FIELDS[*index].get)(&lighting), FIELDS[*index].step);
            editor.replace_on_next_input = true;
        }
    }
    for event in events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        let Some(index) = editor.field else { continue };
        match &event.logical_key {
            Key::Enter => {
                if let Ok(value) = editor.draft.trim().parse::<f32>() {
                    if value.is_finite() {
                        (FIELDS[index].set)(&mut lighting, value);
                        save(&lighting);
                        editor.cancel();
                    }
                }
            }
            Key::Escape => editor.cancel(),
            Key::Backspace => {
                editor.draft.pop();
                editor.replace_on_next_input = false;
            }
            Key::Character(chars)
                if chars.as_str().eq_ignore_ascii_case("a")
                    && (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)) =>
            {
                editor.draft.clear();
                editor.replace_on_next_input = false;
            }
            Key::Character(chars) => {
                let allowed = chars.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'));
                if allowed {
                    if editor.replace_on_next_input {
                        editor.draft.clear();
                        editor.replace_on_next_input = false;
                    }
                    if editor.draft.len() + chars.len() <= 24 {
                        editor.draft.push_str(chars);
                    }
                }
            }
            _ => {}
        }
    }
}

fn refresh_values(lighting: Res<Lighting>, editor: Res<NumericEditor>, mut texts: Query<(&ValueText, &mut Text)>) {
    if !lighting.is_changed() && !editor.is_changed() {
        return;
    }
    for (ValueText(i), mut t) in &mut texts {
        t.0 = if editor.field == Some(*i) {
            format!("{}_", editor.draft)
        } else {
            format_edit_value((FIELDS[*i].get)(&lighting), FIELDS[*i].step)
        };
    }
}
