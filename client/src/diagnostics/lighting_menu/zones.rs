//! The menu's zone editing: the Regions page picks the target (the map's
//! global ambience or one of its local bubbles), and the Lighting and
//! Atmosphere pages' field groups edit that target's overrides
//! (presets::BubbleOverrides), saved with the presets.
//!
//! The zone being edited is not the zone in effect: the latter follows the
//! player's feet or the free camera's own position, in 3D, and only uses
//! always-on bubbles (event -1). "Preview selected zone" shows the target's
//! look wherever the observer is, event bubbles included, so a zone can be
//! edited without standing inside its box.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;

use super::presets::{AreaLightingOverride, BubbleOverrides, LightingMenuStore};
use super::window::{menu_text, MenuRoot};
use crate::rendering::lighting::{Ambience, Bubble, Level, LocalBubble};
use crate::world::map::CurrentMap;

/// The direction a bubble without one gets (Kingshill's sun).
const DEFAULT_DIR: [f32; 3] = [-0.136, 0.721, -0.679];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    LightIntensity, BackLightFactor, ShadowIntensity, DirectionX, DirectionY, DirectionZ,
    LightColorR, LightColorG, LightColorB, BackColorR, BackColorG, BackColorB,
    AmbientColorR, AmbientColorG, AmbientColorB,
    RimColorR, RimColorG, RimColorB, RimIntensity,
    FogColorR, FogColorG, FogColorB, FogIntensity, FogNear, FogFar,
    HeightFogR, HeightFogG, HeightFogB, FogNearHeight, FogFarHeight,
    PlayerColorR, PlayerColorG, PlayerColorB, PlayerLightIntensity, PlayerLightRange,
    Saturation, BloomScale, BrightPassThreshold, VignetteIntensity, VignetteSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorTarget { Key, Back, Ambient, Rim, Fog, HeightFog, Player }

impl ColorTarget {
    fn color(self, b: &Bubble) -> [f32; 4] {
        match self {
            Self::Key => b.light_color,
            Self::Back => b.light_opposite_color,
            Self::Ambient => b.light_ambient,
            Self::Rim => b.rim_light_color,
            Self::Fog => b.fog_color,
            Self::HeightFog => b.height_fog_color,
            Self::Player => b.local_light_color,
        }
    }

    fn slot(self, o: &mut BubbleOverrides) -> &mut Option<[f32; 4]> {
        match self {
            Self::Key => &mut o.light_color,
            Self::Back => &mut o.light_opposite_color,
            Self::Ambient => &mut o.light_ambient,
            Self::Rim => &mut o.rim_light_color,
            Self::Fog => &mut o.fog_color,
            Self::HeightFog => &mut o.height_fog_color,
            Self::Player => &mut o.local_light_color,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Group { Lighting, Colors, Rim, PlayerLight, Fog, PostProcessing }

const GROUPS: usize = 6;

impl Group {
    fn index(self) -> usize { self as usize }

    fn label(self) -> &'static str {
        match self {
            Self::Lighting => "Lighting & Direction",
            Self::Colors => "Light Colors",
            Self::Rim => "Rim Light (characters)",
            Self::PlayerLight => "Player Light",
            Self::Fog => "Fog & Height Fog",
            Self::PostProcessing => "Post Processing",
        }
    }

    fn fields(self) -> &'static [Field] {
        use Field::*;
        match self {
            Self::Lighting => &[LightIntensity, BackLightFactor, ShadowIntensity, DirectionX, DirectionY, DirectionZ],
            Self::Colors => &[LightColorR, LightColorG, LightColorB, BackColorR, BackColorG, BackColorB, AmbientColorR, AmbientColorG, AmbientColorB],
            Self::Rim => &[RimColorR, RimColorG, RimColorB, RimIntensity],
            Self::PlayerLight => &[PlayerColorR, PlayerColorG, PlayerColorB, PlayerLightIntensity, PlayerLightRange],
            Self::Fog => &[FogColorR, FogColorG, FogColorB, FogIntensity, FogNear, FogFar, HeightFogR, HeightFogG, HeightFogB, FogNearHeight, FogFarHeight],
            Self::PostProcessing => &[Saturation, BloomScale, BrightPassThreshold, VignetteIntensity, VignetteSize],
        }
    }
}

impl Field {
    fn label(self) -> &'static str {
        use Field::*;
        match self {
            LightIntensity => "Key light intensity",
            BackLightFactor => "Back light factor",
            ShadowIntensity => "Shadow intensity",
            DirectionX => "Light direction X",
            DirectionY => "Light direction Y",
            DirectionZ => "Light direction Z",
            LightColorR => "Key light R",
            LightColorG => "Key light G",
            LightColorB => "Key light B",
            BackColorR => "Back light R",
            BackColorG => "Back light G",
            BackColorB => "Back light B",
            AmbientColorR => "Ambient R",
            AmbientColorG => "Ambient G",
            AmbientColorB => "Ambient B",
            RimColorR => "Rim light R",
            RimColorG => "Rim light G",
            RimColorB => "Rim light B",
            RimIntensity => "Rim light intensity",
            FogColorR => "Fog R",
            FogColorG => "Fog G",
            FogColorB => "Fog B",
            FogIntensity => "Fog intensity",
            FogNear => "Fog near distance",
            FogFar => "Fog far distance",
            HeightFogR => "Height fog R",
            HeightFogG => "Height fog G",
            HeightFogB => "Height fog B",
            FogNearHeight => "Height fog near height",
            FogFarHeight => "Height fog far height",
            PlayerColorR => "Player light R",
            PlayerColorG => "Player light G",
            PlayerColorB => "Player light B",
            PlayerLightIntensity => "Player light intensity",
            PlayerLightRange => "Player light range",
            Saturation => "Saturation",
            BloomScale => "Bloom scale",
            BrightPassThreshold => "Bright-pass threshold",
            VignetteIntensity => "Vignette intensity",
            VignetteSize => "Vignette size",
        }
    }

    fn step(self) -> f32 {
        use Field::*;
        match self {
            FogNear | FogFar | PlayerLightRange | FogNearHeight | FogFarHeight => 0.5,
            Saturation => 0.02,
            _ => 0.05,
        }
    }

    fn color_channel(self) -> Option<(ColorTarget, usize)> {
        use ColorTarget as C;
        use Field::*;
        Some(match self {
            LightColorR => (C::Key, 0), LightColorG => (C::Key, 1), LightColorB => (C::Key, 2),
            BackColorR => (C::Back, 0), BackColorG => (C::Back, 1), BackColorB => (C::Back, 2),
            AmbientColorR => (C::Ambient, 0), AmbientColorG => (C::Ambient, 1), AmbientColorB => (C::Ambient, 2),
            RimColorR => (C::Rim, 0), RimColorG => (C::Rim, 1), RimColorB => (C::Rim, 2),
            FogColorR => (C::Fog, 0), FogColorG => (C::Fog, 1), FogColorB => (C::Fog, 2),
            HeightFogR => (C::HeightFog, 0), HeightFogG => (C::HeightFog, 1), HeightFogB => (C::HeightFog, 2),
            PlayerColorR => (C::Player, 0), PlayerColorG => (C::Player, 1), PlayerColorB => (C::Player, 2),
            _ => return None,
        })
    }

    fn value(self, b: &Bubble) -> f32 {
        if let Some((target, channel)) = self.color_channel() {
            return target.color(b)[channel];
        }
        use Field::*;
        match self {
            LightIntensity => b.light_intensity,
            BackLightFactor => b.back_light_factor,
            ShadowIntensity => b.light_shadow_intensity,
            DirectionX => b.dir.unwrap_or(DEFAULT_DIR)[0],
            DirectionY => b.dir.unwrap_or(DEFAULT_DIR)[1],
            DirectionZ => b.dir.unwrap_or(DEFAULT_DIR)[2],
            RimIntensity => b.rim_light_intensity,
            FogIntensity => b.fog_intensity,
            FogNear => b.fog_near_dist,
            FogFar => b.fog_far_dist,
            FogNearHeight => b.fog_near_height,
            FogFarHeight => b.fog_far_height,
            PlayerLightIntensity => b.local_light_intensity,
            PlayerLightRange => b.local_light_range,
            Saturation => b.saturation,
            BloomScale => b.bloom_scale,
            BrightPassThreshold => b.bright_pass_threshold,
            VignetteIntensity => b.vignette_intensity,
            VignetteSize => b.vignette_size,
            _ => unreachable!("colour fields are handled above"),
        }
    }

    /// Write one field into an override record; `b` is the target's resolved look
    /// (the other channels of a colour, the other axes of the direction).
    fn write(self, o: &mut BubbleOverrides, b: &Bubble, value: f32) {
        if let Some((target, channel)) = self.color_channel() {
            let mut color = target.color(b);
            color[channel] = value.clamp(0.0, 4.0);
            *target.slot(o) = Some(color);
            return;
        }
        use Field::*;
        match self {
            LightIntensity => o.light_intensity = Some(value.clamp(0.0, 20.0)),
            BackLightFactor => o.back_light_factor = Some(value.clamp(-5.0, 10.0)),
            ShadowIntensity => o.light_shadow_intensity = Some(value.clamp(0.0, 1.0)),
            DirectionX | DirectionY | DirectionZ => {
                let mut direction = b.dir.unwrap_or(DEFAULT_DIR);
                direction[match self { DirectionX => 0, DirectionY => 1, _ => 2 }] = value.clamp(-1.0, 1.0);
                o.dir = Some(Some(direction));
            }
            RimIntensity => o.rim_light_intensity = Some(value.clamp(0.0, 10.0)),
            FogIntensity => o.fog_intensity = Some(value.clamp(0.0, 1.0)),
            FogNear => o.fog_near_dist = Some(value.clamp(0.0, (b.fog_far_dist - 0.1).max(0.0))),
            FogFar => o.fog_far_dist = Some(value.max(b.fog_near_dist + 0.1).min(100_000.0)),
            FogNearHeight => o.fog_near_height = Some(value.clamp(-10_000.0, 10_000.0)),
            FogFarHeight => o.fog_far_height = Some(value.clamp(-10_000.0, 10_000.0)),
            PlayerLightIntensity => o.local_light_intensity = Some(value.clamp(0.0, 100.0)),
            PlayerLightRange => o.local_light_range = Some(value.clamp(0.1, 1000.0)),
            Saturation => o.saturation = Some(value.clamp(0.0, 4.0)),
            BloomScale => o.bloom_scale = Some(value.clamp(0.0, 10.0)),
            BrightPassThreshold => o.bright_pass_threshold = Some(value.clamp(0.0, 4.0)),
            VignetteIntensity => o.vignette_intensity = Some(value.clamp(0.0, 10.0)),
            VignetteSize => o.vignette_size = Some(value.clamp(0.0, 10.0)),
            _ => unreachable!("colour fields are handled above"),
        }
    }
}

#[derive(Component)]
struct ZoneNumericField(Field);
#[derive(Component)]
struct ZoneValueText(Field);
#[derive(Component, Clone, Copy)]
struct ZoneStepButton(Field, f32);
#[derive(Component)]
struct TargetLabel;
#[derive(Component)]
struct StatusText;
#[derive(Component, Clone, Copy)]
struct TargetStep(i8);
#[derive(Component)]
struct PreviewToggle;
#[derive(Component)]
struct PreviewToggleText;
#[derive(Component)]
struct GroupToggle(Group);
#[derive(Component)]
struct GroupTitle(Group);
#[derive(Component)]
struct GroupBody(Group);

/// The zone value being typed, if any.
#[derive(Resource, Default)]
pub(super) struct ZoneEditor {
    field: Option<Field>,
    draft: String,
    replace_on_next_input: bool,
}

impl ZoneEditor {
    pub(super) fn typing(&self) -> bool { self.field.is_some() }

    pub(super) fn cancel(&mut self) {
        self.field = None;
        self.draft.clear();
        self.replace_on_next_input = false;
    }
}

#[derive(Resource, Default)]
struct ZoneUi {
    groups: [bool; GROUPS],
    /// None: the map's global ambience.
    selected: Option<usize>,
    map_name: Option<String>,
    status: String,
}

/// The map's ambience, once its level is current.
fn level_ambience<'a>(current: Option<&CurrentMap>, level: &Level, ambiences: &'a Assets<Ambience>) -> Option<(&'a Ambience, String)> {
    let current = current?;
    (level.map == current.name).then_some(())?;
    Some((ambiences.get(&level.ambience)?, current.name.clone()))
}

/// The selected target's look with the working overrides.
fn resolved_target(ambience: &Ambience, map: &str, store: &LightingMenuStore, selected: Option<usize>) -> Option<Bubble> {
    let overrides = store.working_map_overrides.get(map);
    match selected {
        None => Some(super::presets::resolve_bubble(&ambience.global.clone().unwrap_or_default(), overrides, None)),
        Some(i) => {
            let area = ambience.bubbles.get(i)?;
            Some(super::presets::resolve_bubble(&area.look, overrides, Some((i, area))))
        }
    }
}

/// Record an edit against the target: the map's global overrides, or the
/// bubble's identity (index, event, priority, shape, transform).
fn write_target(store: &mut LightingMenuStore, ambience: &Ambience, map: &str, selected: Option<usize>, field: Field, value: f32) -> Result<(), String> {
    let resolved = resolved_target(ambience, map, store, selected).ok_or("the selected zone no longer exists")?;
    let overrides = store.working_map_overrides.entry(map.to_string()).or_default();
    let record = match selected {
        None => &mut overrides.global,
        Some(i) => {
            let area: &LocalBubble = &ambience.bubbles[i];
            let existing = overrides.areas.iter().position(|c| {
                c.bubble_index == i && c.event == area.event && c.priority == area.priority && c.shape == area.shape && c.transform == area.m
            });
            let index = existing.unwrap_or_else(|| {
                overrides.areas.push(AreaLightingOverride {
                    bubble_index: i,
                    event: area.event,
                    priority: area.priority,
                    shape: area.shape.clone(),
                    transform: area.m,
                    values: BubbleOverrides::default(),
                });
                overrides.areas.len() - 1
            });
            &mut overrides.areas[index].values
        }
    };
    field.write(record, &resolved, value);
    Ok(())
}

/// Whether the target's look is the one shown: previewed, or the zone in effect.
fn shown(store: &LightingMenuStore, level: &Level, selected: Option<usize>) -> bool {
    match store.preview_target {
        Some(target) => target == selected,
        // Map-wide edits reach every zone; a zone's only its own.
        None => selected.is_none() || level.inside == selected,
    }
}

/// The status after an edit, with a hint when the edit cannot be seen.
fn edited(what: &str, store: &LightingMenuStore, level: &Level, selected: Option<usize>) -> String {
    if shown(store, level, selected) {
        what.to_string()
    } else {
        format!("{what} Not visible here: you are not in this zone. Turn on Preview selected zone to see it.")
    }
}

fn persist(store: &mut LightingMenuStore, ui: &mut ZoneUi) {
    if let Err(error) = store.save() {
        ui.status = format!("Not saved: {error}");
    }
}

fn target_name(ambience: &Ambience, selected: Option<usize>) -> String {
    match selected.and_then(|i| Some((i, ambience.bubbles.get(i)?))) {
        None => "Global ambience".into(),
        Some((i, area)) => {
            let event = if area.event == -1 { "always on".to_string() } else { format!("event {}, not active in game", area.event) };
            format!("Area #{} | {} | priority {} | {}", i + 1, area.shape, area.priority, event)
        }
    }
}

// ---------------------------------------------------------------- spawning

fn button(parent: &mut ChildSpawnerCommands, marker: impl Bundle, width: Val, label: &str) {
    parent.spawn((
        Button,
        marker,
        Node {
            width,
            min_height: Val::Px(32.0),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
    ))
    .with_children(|b| {
        b.spawn((Text::new(label), TextFont { font_size: bevy::text::FontSize::Px(13.0), ..default() }, TextColor(Color::srgb(0.94, 0.95, 0.97))));
    });
}

fn card(page: &mut ChildSpawnerCommands, fill: impl FnOnce(&mut ChildSpawnerCommands)) {
    page.spawn((
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(10.0),
            padding: UiRect::all(Val::Px(14.0)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
    ))
    .with_children(fill);
}

/// The Regions page: the target, its preview toggle and the last status.
pub(super) fn spawn_regions(page: &mut ChildSpawnerCommands) {
    page.spawn(menu_text("Ambience regions", 25.0, Color::srgb(0.94, 0.95, 0.97)));
    page.spawn(menu_text(
        "Select the map-wide ambience or cycle through local ambience regions.",
        13.0,
        Color::srgb(0.64, 0.70, 0.79),
    ));
    card(page, |card| {
        card.spawn(menu_text("AMBIENCE TARGET", 12.0, Color::srgb(0.91, 0.73, 0.43)));
        card.spawn(menu_text(
            "Choose which ambience the Lighting and Atmosphere settings edit.",
            12.0,
            Color::srgb(0.72, 0.77, 0.84),
        ));
        card.spawn(Node { width: Val::Percent(100.0), flex_direction: FlexDirection::Row, column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() })
            .with_children(|row| {
                button(row, TargetStep(-1), Val::Px(64.0), "Prev");
                row.spawn((
                    TargetLabel,
                    Text::new("Target: loading map ambience..."),
                    TextFont { font_size: bevy::text::FontSize::Px(13.0), ..default() },
                    TextColor(Color::srgb(0.96, 0.80, 0.49)),
                    Node { flex_grow: 1.0, min_width: Val::Px(0.0), ..default() },
                ));
                button(row, TargetStep(1), Val::Px(64.0), "Next");
            });
        card.spawn(Node { width: Val::Percent(100.0), flex_direction: FlexDirection::Row, column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() })
            .with_children(|row| {
                row.spawn((
                    Button,
                    PreviewToggle,
                    Node { min_height: Val::Px(32.0), padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)), align_items: AlignItems::Center, ..default() },
                    BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                ))
                .with_children(|b| {
                    b.spawn((PreviewToggleText, Text::new("[ ] Preview selected zone"), TextFont { font_size: bevy::text::FontSize::Px(13.0), ..default() }, TextColor(Color::srgb(0.94, 0.95, 0.97))));
                });
            });
        card.spawn(menu_text(
            "Preview shows the target's look wherever you are, event zones included. Off: the zone you stand in applies (player's feet, or the free camera's own position, inside the box in 3D). The selected zone is outlined in the world.",
            12.0,
            Color::srgb(0.64, 0.70, 0.79),
        ));
        card.spawn((StatusText, Text::new(""), TextFont { font_size: bevy::text::FontSize::Px(12.0), ..default() }, TextColor(Color::srgb(0.72, 0.77, 0.84))));
    });
}

/// Collapsible groups of the target's fields (the Lighting and Atmosphere pages).
pub(super) fn spawn_groups(page: &mut ChildSpawnerCommands, groups: &[Group]) {
    for &group in groups {
        page.spawn((
            Node { width: Val::Percent(100.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), padding: UiRect::all(Val::Px(10.0)), ..default() },
            BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
        ))
        .with_children(|section| {
            section.spawn((
                Button,
                GroupToggle(group),
                Node { width: Val::Percent(100.0), min_height: Val::Px(32.0), padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)), align_items: AlignItems::Center, ..default() },
                BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
            ))
            .with_children(|b| {
                b.spawn((GroupTitle(group), Text::new(format!("[+]  {}", group.label())), TextFont { font_size: bevy::text::FontSize::Px(13.0), ..default() }, TextColor(Color::srgb(0.94, 0.95, 0.97))));
            });
            section.spawn((
                GroupBody(group),
                Node { display: Display::None, width: Val::Percent(100.0), flex_direction: FlexDirection::Column, row_gap: Val::Px(7.0), padding: UiRect::all(Val::Px(5.0)), ..default() },
            ))
            .with_children(|body| {
                for &field in group.fields() {
                    body.spawn(Node { width: Val::Percent(100.0), min_height: Val::Px(30.0), flex_direction: FlexDirection::Row, column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() })
                        .with_children(|row| {
                            row.spawn((
                                Text::new(field.label()),
                                TextFont { font_size: bevy::text::FontSize::Px(12.0), ..default() },
                                TextColor(Color::srgb(0.83, 0.86, 0.91)),
                                Node { flex_grow: 1.0, min_width: Val::Px(0.0), ..default() },
                            ));
                            button(row, ZoneStepButton(field, -1.0), Val::Px(27.0), "-");
                            row.spawn((
                                Button,
                                ZoneNumericField(field),
                                Node {
                                    width: Val::Px(100.0),
                                    min_height: Val::Px(29.0),
                                    flex_shrink: 0.0,
                                    padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.07, 0.09, 0.13)),
                            ))
                            .with_children(|value| {
                                value.spawn((ZoneValueText(field), Text::new("-"), TextFont { font_size: bevy::text::FontSize::Px(12.0), ..default() }, TextColor(Color::srgb(0.96, 0.80, 0.49))));
                            });
                            button(row, ZoneStepButton(field, 1.0), Val::Px(27.0), "+");
                        });
                }
            });
        });
    }
}

// ---------------------------------------------------------------- systems

pub(super) struct ZonesPlugin;

impl Plugin for ZonesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ZoneEditor>()
            .init_resource::<ZoneUi>()
            .add_systems(Update, draw_selected_zone);
    }
}

/// The page systems, run by the menu after the editors are blurred.
pub(super) fn systems() -> impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, ()> {
    (toggle_groups, cycle_target, toggle_preview, step_fields, edit_fields, refresh_target_label, refresh_values).chain()
}

fn toggle_groups(
    buttons: Query<(&Interaction, &GroupToggle), Changed<Interaction>>,
    mut ui: ResMut<ZoneUi>,
    mut bodies: Query<(&GroupBody, &mut Node)>,
    mut titles: Query<(&GroupTitle, &mut Text)>,
) {
    let mut toggled = false;
    for (interaction, group) in &buttons {
        if *interaction == Interaction::Pressed {
            ui.groups[group.0.index()] ^= true;
            toggled = true;
        }
    }
    if !toggled {
        return;
    }
    for (group, mut node) in &mut bodies {
        node.display = if ui.groups[group.0.index()] { Display::Flex } else { Display::None };
    }
    for (group, mut text) in &mut titles {
        text.0 = format!("{}  {}", if ui.groups[group.0.index()] { "[-]" } else { "[+]" }, group.0.label());
    }
}

/// Prev / Next: the global ambience, then every local bubble with a volume
/// (event bubbles too: they can be previewed and edited, not entered).
fn cycle_target(
    buttons: Query<(&Interaction, &TargetStep), Changed<Interaction>>,
    current: Option<Res<CurrentMap>>,
    level: Res<Level>,
    ambiences: Res<Assets<Ambience>>,
    mut ui: ResMut<ZoneUi>,
    mut editor: ResMut<ZoneEditor>,
    mut store: ResMut<LightingMenuStore>,
) {
    let steps: Vec<i8> = buttons.iter().filter_map(|(i, s)| (*i == Interaction::Pressed).then_some(s.0)).collect();
    if steps.is_empty() {
        return;
    }
    let Some((ambience, map)) = level_ambience(current.as_deref(), &level, &ambiences) else {
        ui.status = "Map ambience is still loading.".into();
        return;
    };
    let targets: Vec<Option<usize>> = std::iter::once(None)
        .chain(ambience.bubbles.iter().enumerate().filter(|(_, a)| a.m.is_some()).map(|(i, _)| Some(i)))
        .collect();
    for step in steps {
        let at = targets.iter().position(|t| *t == ui.selected).unwrap_or(0);
        ui.selected = targets[(at as isize + step as isize).rem_euclid(targets.len() as isize) as usize];
    }
    editor.cancel();
    if store.preview_target.is_some() {
        store.preview_target = Some(ui.selected);
    }
    ui.status = format!("Editing {} in {map}.", target_name(ambience, ui.selected));
}

fn toggle_preview(
    buttons: Query<&Interaction, (Changed<Interaction>, With<PreviewToggle>)>,
    ui: Res<ZoneUi>,
    mut store: ResMut<LightingMenuStore>,
    mut labels: Query<&mut Text, With<PreviewToggleText>>,
) {
    if buttons.iter().any(|i| *i == Interaction::Pressed) {
        store.preview_target = if store.preview_target.is_some() { None } else { Some(ui.selected) };
    }
    if store.is_changed() {
        let label = if store.preview_target.is_some() { "[x] Preview selected zone" } else { "[ ] Preview selected zone" };
        for mut text in &mut labels {
            if text.0 != label {
                text.0 = label.into();
            }
        }
    }
}

fn step_fields(
    buttons: Query<(&Interaction, &ZoneStepButton), Changed<Interaction>>,
    keys: Res<ButtonInput<KeyCode>>,
    current: Option<Res<CurrentMap>>,
    level: Res<Level>,
    ambiences: Res<Assets<Ambience>>,
    mut ui: ResMut<ZoneUi>,
    mut store: ResMut<LightingMenuStore>,
) {
    let steps: Vec<(Field, f32)> = buttons.iter().filter_map(|(i, s)| (*i == Interaction::Pressed).then_some((s.0, s.1))).collect();
    if steps.is_empty() {
        return;
    }
    let Some((ambience, map)) = level_ambience(current.as_deref(), &level, &ambiences) else {
        ui.status = "Map ambience is still loading.".into();
        return;
    };
    let mul = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 10.0 } else { 1.0 };
    for (field, sign) in steps {
        let Some(look) = resolved_target(ambience, &map, &store, ui.selected) else { continue };
        let value = field.value(&look) + field.step() * sign * mul;
        match write_target(&mut store, ambience, &map, ui.selected, field, value) {
            Ok(()) => ui.status = edited(&format!("Adjusted {} of {}.", field.label(), target_name(ambience, ui.selected)), &store, &level, ui.selected),
            Err(error) => ui.status = error,
        }
    }
    persist(&mut store, &mut ui);
}

fn edit_fields(
    mut events: MessageReader<KeyboardInput>,
    fields: Query<(&Interaction, &ZoneNumericField), Changed<Interaction>>,
    keys: Res<ButtonInput<KeyCode>>,
    current: Option<Res<CurrentMap>>,
    level: Res<Level>,
    ambiences: Res<Assets<Ambience>>,
    mut ui: ResMut<ZoneUi>,
    mut store: ResMut<LightingMenuStore>,
    mut editor: ResMut<ZoneEditor>,
) {
    let target = level_ambience(current.as_deref(), &level, &ambiences);
    for (interaction, ZoneNumericField(field)) in &fields {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(look) = target.as_ref().and_then(|(a, map)| resolved_target(a, map, &store, ui.selected)) else {
            ui.status = "Map ambience is still loading.".into();
            continue;
        };
        editor.field = Some(*field);
        editor.draft = super::global::format_edit_value(field.value(&look), field.step());
        editor.replace_on_next_input = true;
    }
    for event in events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        let Some(field) = editor.field else { continue };
        match &event.logical_key {
            Key::Enter => {
                let Ok(value) = editor.draft.trim().parse::<f32>() else { continue };
                if !value.is_finite() {
                    continue;
                }
                let Some((ambience, map)) = target.as_ref() else {
                    ui.status = "Map ambience is still loading.".into();
                    continue;
                };
                match write_target(&mut store, ambience, map, ui.selected, field, value) {
                    Ok(()) => ui.status = edited(&format!("Updated {} of {}.", field.label(), target_name(ambience, ui.selected)), &store, &level, ui.selected),
                    Err(error) => ui.status = error,
                }
                persist(&mut store, &mut ui);
                editor.cancel();
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
                if chars.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')) {
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

/// The target line (with "you are here" when it is the zone in effect) and the
/// status line, kept current across map and ambience changes.
fn refresh_target_label(
    current: Option<Res<CurrentMap>>,
    level: Res<Level>,
    ambiences: Res<Assets<Ambience>>,
    store: Res<LightingMenuStore>,
    mut ui: ResMut<ZoneUi>,
    mut labels: Query<&mut Text, (With<TargetLabel>, Without<StatusText>)>,
    mut statuses: Query<&mut Text, (With<StatusText>, Without<TargetLabel>)>,
) {
    let map_name = current.as_ref().map(|m| m.name.clone());
    if ui.map_name != map_name {
        ui.map_name = map_name;
        ui.selected = None;
        ui.status.clear();
    }
    let label = match level_ambience(current.as_deref(), &level, &ambiences) {
        None => "Target: loading map ambience...".to_string(),
        Some((ambience, _)) => {
            if ui.selected.is_some_and(|i| ambience.bubbles.get(i).is_none_or(|a| a.m.is_none())) {
                ui.selected = None;
            }
            let zones = ambience.bubbles.iter().filter(|a| a.m.is_some()).count();
            let state = match store.preview_target {
                Some(target) if target == ui.selected => "  - previewing",
                Some(_) => "  - previewing another target",
                None if level.inside == ui.selected => "  - you are here",
                None if ui.selected.is_some() => "  - not in effect here (use Preview)",
                None => "",
            };
            format!("Target: {} ({zones} zones){state}", target_name(ambience, ui.selected))
        }
    };
    for mut text in &mut labels {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
    for mut text in &mut statuses {
        if text.0 != ui.status {
            text.0 = ui.status.clone();
        }
    }
}

fn refresh_values(
    current: Option<Res<CurrentMap>>,
    level: Res<Level>,
    ambiences: Res<Assets<Ambience>>,
    store: Res<LightingMenuStore>,
    editor: Res<ZoneEditor>,
    ui: Res<ZoneUi>,
    mut texts: Query<(&ZoneValueText, &mut Text)>,
) {
    let map_changed = current.as_ref().is_some_and(|m| m.is_changed());
    if !store.is_changed() && !level.is_changed() && !ambiences.is_changed() && !editor.is_changed() && !ui.is_changed() && !map_changed {
        return;
    }
    let look = level_ambience(current.as_deref(), &level, &ambiences).and_then(|(a, map)| resolved_target(a, &map, &store, ui.selected));
    for (ZoneValueText(field), mut text) in &mut texts {
        text.0 = if editor.field == Some(*field) {
            format!("{}_", editor.draft)
        } else if let Some(look) = look.as_ref() {
            super::global::format_edit_value(field.value(look), field.step())
        } else {
            "-".to_string()
        };
    }
}

/// While the menu is open: the selected zone's volume, its centre, and a line
/// from the camera to it so it can be found off screen.
fn draw_selected_zone(
    roots: Query<&Visibility, With<MenuRoot>>,
    current: Option<Res<CurrentMap>>,
    level: Res<Level>,
    ambiences: Res<Assets<Ambience>>,
    ui: Res<ZoneUi>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut gizmos: Gizmos,
) {
    if !roots.iter().any(|v| *v != Visibility::Hidden) {
        return;
    }
    let Some(index) = ui.selected else { return };
    let Some((ambience, _)) = level_ambience(current.as_deref(), &level, &ambiences) else { return };
    let Some(matrix) = ambience.bubbles.get(index).and_then(|a| a.m.map(|m| (a, m))) else { return };
    let (area, matrix) = matrix;
    // The same local-to-world matrix as LocalBubble::contains.
    let transform = Transform::from_matrix(Mat4::from_cols_array(&matrix));
    let color = if area.event == -1 { Color::srgb(1.0, 0.12, 0.85) } else { Color::srgb(0.6, 0.6, 0.6) };
    let center = transform.translation;
    if area.shape == "Sphere" {
        // The local sphere's radius is 0.5; the largest scale for non-uniform ones.
        gizmos.sphere(Isometry3d::new(center, transform.rotation), 0.5 * transform.scale.abs().max_element(), color);
    } else {
        gizmos.cube(transform, color);
    }
    gizmos.cross(center, 0.5, Color::srgb(1.0, 0.85, 0.15));
    if let Some(camera) = cameras.iter().next() {
        if camera.translation().distance(center) > 1.0 {
            gizmos.line(camera.translation(), center, Color::srgb(1.0, 0.5, 0.1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ambience() -> Ambience {
        let area = |event| LocalBubble {
            look: Bubble { fog_far_dist: 60.0, ..default() },
            m: Some(Mat4::from_scale(Vec3::splat(8.0)).to_cols_array()),
            priority: 1,
            event,
            shape: "Box".into(),
            enabled: true,
        };
        Ambience { global: Some(Bubble { fog_far_dist: 50.0, ..default() }), bubbles: vec![area(-1), area(83)], ..default() }
    }

    #[test]
    fn edits_go_to_the_global_record_or_the_bubbles_identity() {
        let ambience = ambience();
        let mut store = LightingMenuStore::default();
        write_target(&mut store, &ambience, "test", None, Field::FogFar, 70.0).unwrap();
        write_target(&mut store, &ambience, "test", Some(1), Field::RimIntensity, 0.8).unwrap();
        write_target(&mut store, &ambience, "test", Some(1), Field::FogFar, 90.0).unwrap();
        let map = &store.working_map_overrides["test"];
        assert_eq!(map.global.fog_far_dist, Some(70.0));
        assert_eq!(map.areas.len(), 1, "one record per bubble");
        assert_eq!((map.areas[0].bubble_index, map.areas[0].event), (1, 83));
        assert_eq!(map.areas[0].values.rim_light_intensity, Some(0.8));
        assert_eq!(resolved_target(&ambience, "test", &store, Some(1)).unwrap().fog_far_dist, 90.0);
        assert_eq!(resolved_target(&ambience, "test", &store, Some(0)).unwrap().fog_far_dist, 70.0, "map-wide edits reach other zones");
    }

    #[test]
    fn a_colour_channel_keeps_the_other_channels() {
        let ambience = ambience();
        let mut store = LightingMenuStore::default();
        let before = resolved_target(&ambience, "test", &store, None).unwrap().height_fog_color;
        write_target(&mut store, &ambience, "test", None, Field::HeightFogG, 0.25).unwrap();
        let after = resolved_target(&ambience, "test", &store, None).unwrap().height_fog_color;
        assert_eq!(after, [before[0], 0.25, before[2], before[3]]);
    }
}
