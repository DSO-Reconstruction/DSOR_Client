//! The Presets page: a name to type, Save As / Overwrite / Rename / Delete
//! (Delete asks for a second click) / Deselect, and the saved presets;
//! clicking one loads its global controls (into look.json) and its map
//! overrides. Saved in lighting-presets.json (presets::LightingMenuStore).
//!
//! Rename with a new name typed renames at once; otherwise it opens the name
//! with the cursor at its end, and Enter renames. Typing adds to the name
//! (Ctrl+A clears it).

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;

use super::presets::{LightingControls, LightingMenuStore};
use super::window::menu_text;
use crate::rendering::lighting::{save, Lighting};

#[derive(Component)]
struct NameField;
#[derive(Component)]
struct NameText;
#[derive(Component)]
struct StatusText;
#[derive(Component)]
struct PresetList;
#[derive(Component)]
struct PresetRow;
#[derive(Component)]
struct PresetChoice(String);

#[derive(Component, Clone, Copy)]
enum PresetAction { SaveAs, Overwrite, Rename, Delete, Deselect }

/// The preset name being typed.
#[derive(Resource, Default)]
pub(super) struct NameEditor {
    focused: bool,
    draft: String,
    /// Enter renames the selected preset (opened by Rename).
    renaming: bool,
}

impl NameEditor {
    pub(super) fn typing(&self) -> bool { self.focused }

    /// Stop typing; the name typed so far stays for the actions.
    pub(super) fn cancel(&mut self) {
        self.focused = false;
        self.renaming = false;
    }
}

fn active_name(store: &LightingMenuStore) -> Option<String> {
    store.active_preset_id.as_deref().and_then(|id| store.presets.get(id)).map(|p| p.name.clone())
}

fn rename(store: &mut LightingMenuStore, editor: &mut NameEditor, ui: &mut PresetUi) {
    let Some(id) = store.active_preset_id.clone() else {
        ui.status = "Select a preset before renaming.".into();
        return;
    };
    let name = editor.draft.trim().to_string();
    match store.rename_preset(&id, &name) {
        Ok(()) => {
            editor.draft = name.clone();
            ui.status = format!("Renamed active preset to '{name}'.");
            persist(store, ui);
        }
        Err(error) => ui.status = format!("Rename failed: {error}"),
    }
}

#[derive(Resource, Default)]
struct PresetUi {
    status: String,
    pending_delete_id: Option<String>,
}

fn action_button(parent: &mut ChildSpawnerCommands, action: PresetAction, width: f32, label: &str, background: Color) {
    parent.spawn((
        Button,
        action,
        Node {
            width: Val::Px(width),
            min_width: Val::Px(width),
            max_width: Val::Px(width),
            flex_shrink: 0.0,
            min_height: Val::Px(32.0),
            padding: UiRect::axes(Val::Px(5.0), Val::Px(5.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(background),
    ))
    .with_children(|button| {
        button.spawn((Text::new(label), TextFont { font_size: bevy::text::FontSize::Px(12.0), ..default() }, TextColor(Color::srgb(0.94, 0.95, 0.97))));
    });
}

pub(super) fn spawn(page: &mut ChildSpawnerCommands) {
    page.spawn(menu_text("Lighting presets", 25.0, Color::srgb(0.94, 0.95, 0.97)));
    page.spawn(menu_text("Save named looks, load them later, and manage your saved profiles.", 13.0, Color::srgb(0.64, 0.70, 0.79)));
    let card = Node {
        width: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(10.0),
        padding: UiRect::all(Val::Px(12.0)),
        ..default()
    };
    page.spawn((card.clone(), BackgroundColor(Color::srgb(0.12, 0.145, 0.19)))).with_children(|card| {
        card.spawn(menu_text("PRESET NAME", 12.0, Color::srgb(0.91, 0.73, 0.43)));
        card.spawn((
            Button,
            NameField,
            Node { width: Val::Percent(100.0), min_height: Val::Px(36.0), padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)), align_items: AlignItems::Center, ..default() },
            BackgroundColor(Color::srgb(0.07, 0.09, 0.13)),
        ))
        .with_children(|field| {
            field.spawn((NameText, Text::new("Click to enter a preset name"), TextFont { font_size: bevy::text::FontSize::Px(13.0), ..default() }, TextColor(Color::srgb(0.94, 0.95, 0.97))));
        });
        card.spawn(menu_text(
            "Click the name field and type (Ctrl+A clears it), then choose an action. Rename opens the selected preset's name: Enter renames, Esc cancels.",
            11.0,
            Color::srgb(0.64, 0.70, 0.79),
        ));
        for actions in [[(PresetAction::SaveAs, "Save As"), (PresetAction::Overwrite, "Overwrite")], [(PresetAction::Rename, "Rename"), (PresetAction::Delete, "Delete")]] {
            card.spawn(Node { width: Val::Px(254.0), flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), ..default() }).with_children(|row| {
                for (action, label) in actions {
                    action_button(row, action, 124.0, label, Color::srgb(0.20, 0.23, 0.29));
                }
            });
        }
        action_button(card, PresetAction::Deselect, 254.0, "Deselect", Color::srgb(0.16, 0.20, 0.25));
        card.spawn((StatusText, Text::new("No preset selected"), TextFont { font_size: bevy::text::FontSize::Px(12.0), ..default() }, TextColor(Color::srgb(0.96, 0.80, 0.49))));
    });
    page.spawn((card, BackgroundColor(Color::srgb(0.12, 0.145, 0.19)))).with_children(|card| {
        card.spawn(menu_text("SAVED PRESETS", 12.0, Color::srgb(0.91, 0.73, 0.43)));
        card.spawn(menu_text(
            "Select a saved preset to load it. The selected preset can then be overwritten, renamed, or deleted.",
            11.0,
            Color::srgb(0.64, 0.70, 0.79),
        ));
        card.spawn((PresetList, Node { width: Val::Percent(100.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), ..default() }));
    });
}

pub(super) struct PresetsPagePlugin;

impl Plugin for PresetsPagePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NameEditor>().init_resource::<PresetUi>().add_systems(Startup, start_with_active_name);
    }
}

/// The page systems, run by the menu after the editors are blurred.
pub(super) fn systems() -> impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, ()> {
    (edit_name, handle_actions, select_from_list, refresh_name, refresh_status, rebuild_list).chain()
}

fn start_with_active_name(store: Res<LightingMenuStore>, mut editor: ResMut<NameEditor>) {
    if let Some(name) = active_name(&store) {
        editor.draft = name;
    }
}

fn persist(store: &mut LightingMenuStore, ui: &mut PresetUi) {
    // Tests must not write lighting-presets.json into the working directory.
    if cfg!(test) {
        return;
    }
    if let Err(error) = store.save() {
        warn!("lighting presets: could not persist changes: {error}");
        ui.status = format!("Could not save lighting-presets.json: {error}");
    }
}

fn edit_name(
    mut events: MessageReader<KeyboardInput>,
    fields: Query<&Interaction, (Changed<Interaction>, With<NameField>)>,
    mut store: ResMut<LightingMenuStore>,
    mut editor: ResMut<NameEditor>,
    mut ui: ResMut<PresetUi>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if fields.iter().any(|i| *i == Interaction::Pressed) {
        editor.focused = true;
    }
    for event in events.read() {
        if event.state != ButtonState::Pressed || !editor.focused {
            continue;
        }
        match &event.logical_key {
            Key::Enter => {
                if editor.renaming {
                    rename(&mut store, &mut editor, &mut ui);
                }
                editor.cancel();
            }
            Key::Escape => {
                if editor.renaming {
                    ui.status = "Rename cancelled.".into();
                }
                editor.cancel();
                editor.draft = active_name(&store).unwrap_or_default();
            }
            Key::Backspace => {
                editor.draft.pop();
            }
            Key::Space => {
                if editor.draft.chars().count() < 64 {
                    editor.draft.push(' ');
                }
            }
            Key::Character(chars)
                if chars.as_str().eq_ignore_ascii_case("a")
                    && (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)) =>
            {
                editor.draft.clear();
            }
            Key::Character(chars) => {
                if chars.chars().any(char::is_control) {
                    continue;
                }
                if editor.draft.chars().count().saturating_add(chars.chars().count()) <= 64 {
                    editor.draft.push_str(chars);
                }
            }
            _ => {}
        }
    }
}

fn handle_actions(
    buttons: Query<(&Interaction, &PresetAction), Changed<Interaction>>,
    lighting: Res<Lighting>,
    mut store: ResMut<LightingMenuStore>,
    mut editor: ResMut<NameEditor>,
    mut ui: ResMut<PresetUi>,
) {
    let controls = LightingControls::from(&*lighting);
    for (interaction, action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let name_of = |store: &LightingMenuStore, id: &str| store.presets.get(id).map(|p| p.name.clone()).unwrap_or_else(|| id.to_string());
        if !matches!(action, PresetAction::Delete) {
            ui.pending_delete_id = None;
        }
        match action {
            PresetAction::SaveAs => {
                let requested = editor.draft.trim().to_string();
                match store.save_as(&requested, &controls) {
                    Ok(id) => {
                        let saved = name_of(&store, &id);
                        editor.draft = saved.clone();
                        ui.status = format!("Saved new preset '{saved}'.");
                        persist(&mut store, &mut ui);
                    }
                    Err(error) => ui.status = format!("Save As failed: {error}"),
                }
            }
            PresetAction::Overwrite => {
                let Some(id) = store.active_preset_id.clone() else {
                    ui.status = "Select a preset before overwriting.".into();
                    continue;
                };
                let name = name_of(&store, &id);
                match store.overwrite_active(&controls) {
                    Ok(()) => {
                        ui.status = format!("Overwrote '{name}'.");
                        persist(&mut store, &mut ui);
                    }
                    Err(error) => ui.status = format!("Overwrite failed: {error}"),
                }
            }
            PresetAction::Rename => {
                let Some(current) = active_name(&store) else {
                    ui.status = "Select a preset before renaming.".into();
                    continue;
                };
                let typed = editor.draft.trim().to_string();
                if !typed.is_empty() && typed != current {
                    // A new name typed first: rename now.
                    rename(&mut store, &mut editor, &mut ui);
                } else {
                    editor.draft = current;
                    editor.focused = true;
                    editor.renaming = true;
                    ui.status = "Type the new name, then press Enter to rename (Esc cancels).".into();
                }
            }
            PresetAction::Deselect => {
                store.active_preset_id = None;
                editor.draft.clear();
                ui.status = "No preset selected. The current look stays as it is; Overwrite, Rename and Delete need a selected preset.".into();
                persist(&mut store, &mut ui);
            }
            PresetAction::Delete => {
                let Some(id) = store.active_preset_id.clone() else {
                    ui.pending_delete_id = None;
                    ui.status = "Select a preset before deleting.".into();
                    continue;
                };
                let name = name_of(&store, &id);
                if ui.pending_delete_id.as_deref() == Some(id.as_str()) {
                    ui.pending_delete_id = None;
                    if store.delete_preset(&id) {
                        editor.draft.clear();
                        ui.status = format!("Deleted '{name}'. Working settings were kept.");
                        persist(&mut store, &mut ui);
                    } else {
                        ui.status = "The selected preset no longer exists.".into();
                    }
                } else {
                    ui.pending_delete_id = Some(id);
                    ui.status = format!("Click Delete again to confirm deleting '{name}'.");
                }
            }
        }
    }
}

/// A row loads its preset: the global controls (saved to look.json) and the
/// map overrides; zones show the loaded look at once.
fn select_from_list(
    choices: Query<(&Interaction, &PresetChoice), Changed<Interaction>>,
    mut store: ResMut<LightingMenuStore>,
    mut lighting: ResMut<Lighting>,
    mut editor: ResMut<NameEditor>,
    mut ui: ResMut<PresetUi>,
) {
    for (interaction, choice) in &choices {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(preset) = store.select_preset(&choice.0) else {
            ui.status = "That preset could not be found.".into();
            continue;
        };
        preset.controls.apply(&mut lighting);
        save(&lighting);
        editor.draft = preset.name.clone();
        editor.cancel();
        ui.pending_delete_id = None;
        ui.status = format!("Loaded '{}'.", preset.name);
        persist(&mut store, &mut ui);
    }
}

fn refresh_name(editor: Res<NameEditor>, mut texts: Query<&mut Text, With<NameText>>) {
    if !editor.is_changed() {
        return;
    }
    let shown = if editor.focused {
        format!("{}_", editor.draft)
    } else if editor.draft.is_empty() {
        "Click to enter a preset name".to_string()
    } else {
        editor.draft.clone()
    };
    for mut text in &mut texts {
        text.0 = shown.clone();
    }
}

fn refresh_status(store: Res<LightingMenuStore>, lighting: Res<Lighting>, ui: Res<PresetUi>, mut texts: Query<&mut Text, With<StatusText>>) {
    if !store.is_changed() && !lighting.is_changed() && !ui.is_changed() {
        return;
    }
    let heading = match store.active_preset_id.as_deref().and_then(|id| store.presets.get(id)) {
        Some(preset) if store.active_preset_is_modified(&LightingControls::from(&*lighting)) => format!("Active: {}  * UNSAVED CHANGES", preset.name),
        Some(preset) => format!("Active: {}", preset.name),
        None => "No preset selected".to_string(),
    };
    let shown = if ui.status.is_empty() { heading } else { format!("{heading}\n{}", ui.status) };
    for mut text in &mut texts {
        text.0 = shown.clone();
    }
}

/// The saved presets, rebuilt only when their names or the active one change
/// (not on every zone edit).
fn rebuild_list(
    mut commands: Commands,
    store: Res<LightingMenuStore>,
    lists: Query<Entity, With<PresetList>>,
    rows: Query<Entity, With<PresetRow>>,
    mut shown: Local<Option<(Vec<(String, String)>, Option<String>)>>,
) {
    let current = (
        store.presets.iter().map(|(id, p)| (id.clone(), p.name.clone())).collect::<Vec<_>>(),
        store.active_preset_id.clone(),
    );
    if shown.as_ref() == Some(&current) || lists.is_empty() {
        return;
    }
    for row in &rows {
        commands.entity(row).despawn();
    }
    for parent in &lists {
        commands.entity(parent).with_children(|list| {
            for (id, name) in &current.0 {
                let active = current.1.as_deref() == Some(id.as_str());
                list.spawn((
                    Button,
                    PresetChoice(id.clone()),
                    PresetRow,
                    Node { width: Val::Percent(100.0), min_height: Val::Px(24.0), padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)), align_items: AlignItems::Center, ..default() },
                    BackgroundColor(if active { Color::srgb(0.30, 0.24, 0.12) } else { Color::srgb(0.17, 0.17, 0.21) }),
                ))
                .with_children(|button| {
                    button.spawn((
                        Text::new(if active { format!(">  {name}") } else { name.clone() }),
                        TextFont { font_size: bevy::text::FontSize::Px(13.0), ..default() },
                        TextColor(Color::srgb(0.95, 0.95, 0.95)),
                    ));
                });
            }
        });
    }
    *shown = Some(current);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<KeyboardInput>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Lighting>()
            .init_resource::<NameEditor>()
            .init_resource::<PresetUi>()
            .add_systems(Update, (edit_name, handle_actions).chain());
        let mut store = LightingMenuStore::default();
        store.save_as("Kingshill", &LightingControls::default()).unwrap();
        app.insert_resource(store);
        let rename = app.world_mut().spawn((Button, PresetAction::Rename, Interaction::None)).id();
        (app, rename)
    }

    fn press(app: &mut App, button: Entity) {
        app.world_mut().entity_mut(button).insert(Interaction::Pressed);
        app.update();
        app.world_mut().entity_mut(button).insert(Interaction::None);
    }

    fn key(app: &mut App, logical_key: Key) {
        let window = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
            logical_key, state: ButtonState::Pressed, text: None, repeat: false, window,
        });
        app.update();
    }

    fn name(app: &App) -> String {
        let store = app.world().resource::<LightingMenuStore>();
        active_name(store).unwrap()
    }

    #[test]
    fn rename_opens_the_name_to_extend_and_enter_renames() {
        let (mut app, rename) = app();
        press(&mut app, rename);
        let editor = app.world().resource::<NameEditor>();
        assert!(editor.focused && editor.renaming);
        assert_eq!(editor.draft, "Kingshill", "the current name, to add to");
        key(&mut app, Key::Space);
        key(&mut app, Key::Character("dusk".into()));
        assert_eq!(name(&app), "Kingshill", "nothing renamed before Enter");
        key(&mut app, Key::Enter);
        assert_eq!(name(&app), "Kingshill dusk");
        assert!(!app.world().resource::<NameEditor>().focused);
    }

    #[test]
    fn escape_cancels_a_rename_and_a_typed_name_renames_at_once() {
        let (mut app, rename) = app();
        press(&mut app, rename);
        key(&mut app, Key::Character("x".into()));
        key(&mut app, Key::Escape);
        assert_eq!(name(&app), "Kingshill");
        assert_eq!(app.world().resource::<NameEditor>().draft, "Kingshill");
        // A new name typed in the box first: Rename applies it directly.
        app.world_mut().resource_mut::<NameEditor>().draft = "Night".into();
        press(&mut app, rename);
        assert_eq!(name(&app), "Night");
    }
}
