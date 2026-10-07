//! The look of the scene, tunable in game: sun, ambient light and colour grading.
//!
//! F2 opens a panel; each line has - / + buttons (Shift: ten steps at once). The
//! values are kept in the browser (localStorage) or, natively, in
//! `lighting.json` next to the binary's working directory, and printed to the log
//! on every change so they can be made the defaults.
//! UNVERIFIED: the defaults; the game's own per-map light (the .map files) is not
//! read yet.

use bevy::light::{DirectionalLight, GlobalAmbientLight};
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};
use serde::{Deserialize, Serialize};

#[derive(Resource, Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Lighting {
    /// Sun direction: degrees around the vertical (0 = from +Z) and above the horizon.
    pub sun_azimuth: f32,
    pub sun_elevation: f32,
    /// Lux.
    pub sun_illuminance: f32,
    pub sun_r: f32,
    pub sun_g: f32,
    pub sun_b: f32,
    pub ambient: f32,
    pub ambient_r: f32,
    pub ambient_g: f32,
    pub ambient_b: f32,
    /// Exposure compensation (stops), white-balance temperature and tint.
    pub exposure: f32,
    pub temperature: f32,
    pub tint: f32,
    /// Saturation after tonemapping, and contrast/saturation of the midtones.
    pub saturation: f32,
    pub contrast: f32,
    pub midtone_saturation: f32,
    pub gamma: f32,
    pub shadows: bool,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            sun_azimuth: 211.0,
            sun_elevation: 54.0,
            sun_illuminance: 7_500.0,
            sun_r: 1.0,
            sun_g: 1.0,
            sun_b: 1.0,
            ambient: 600.0,
            ambient_r: 1.0,
            ambient_g: 1.0,
            ambient_b: 1.0,
            exposure: 0.0,
            temperature: 0.0,
            tint: 0.0,
            saturation: 1.0,
            contrast: 1.0,
            midtone_saturation: 1.0,
            gamma: 1.0,
            shadows: true,
        }
    }
}

/// One tunable line: label, step, read, write.
struct Field {
    label: &'static str,
    step: f32,
    get: fn(&Lighting) -> f32,
    set: fn(&mut Lighting, f32),
}

const FIELDS: &[Field] = &[
    Field { label: "Sun azimuth", step: 5.0, get: |l| l.sun_azimuth, set: |l, v| l.sun_azimuth = v.rem_euclid(360.0) },
    Field { label: "Sun elevation", step: 2.0, get: |l| l.sun_elevation, set: |l, v| l.sun_elevation = v.clamp(5.0, 90.0) },
    Field { label: "Sun strength", step: 250.0, get: |l| l.sun_illuminance, set: |l, v| l.sun_illuminance = v.max(0.0) },
    Field { label: "Sun red", step: 0.02, get: |l| l.sun_r, set: |l, v| l.sun_r = v.clamp(0.0, 2.0) },
    Field { label: "Sun green", step: 0.02, get: |l| l.sun_g, set: |l, v| l.sun_g = v.clamp(0.0, 2.0) },
    Field { label: "Sun blue", step: 0.02, get: |l| l.sun_b, set: |l, v| l.sun_b = v.clamp(0.0, 2.0) },
    Field { label: "Ambient", step: 25.0, get: |l| l.ambient, set: |l, v| l.ambient = v.max(0.0) },
    Field { label: "Ambient red", step: 0.02, get: |l| l.ambient_r, set: |l, v| l.ambient_r = v.clamp(0.0, 2.0) },
    Field { label: "Ambient green", step: 0.02, get: |l| l.ambient_g, set: |l, v| l.ambient_g = v.clamp(0.0, 2.0) },
    Field { label: "Ambient blue", step: 0.02, get: |l| l.ambient_b, set: |l, v| l.ambient_b = v.clamp(0.0, 2.0) },
    Field { label: "Exposure", step: 0.05, get: |l| l.exposure, set: |l, v| l.exposure = v.clamp(-5.0, 5.0) },
    Field { label: "Temperature", step: 0.02, get: |l| l.temperature, set: |l, v| l.temperature = v.clamp(-1.0, 1.0) },
    Field { label: "Tint", step: 0.02, get: |l| l.tint, set: |l, v| l.tint = v.clamp(-1.0, 1.0) },
    Field { label: "Saturation", step: 0.02, get: |l| l.saturation, set: |l, v| l.saturation = v.clamp(0.0, 3.0) },
    Field { label: "Contrast", step: 0.02, get: |l| l.contrast, set: |l, v| l.contrast = v.clamp(0.2, 3.0) },
    Field { label: "Midtone saturation", step: 0.02, get: |l| l.midtone_saturation, set: |l, v| l.midtone_saturation = v.clamp(0.0, 3.0) },
    Field { label: "Gamma", step: 0.02, get: |l| l.gamma, set: |l, v| l.gamma = v.clamp(0.2, 3.0) },
    Field {
        label: "Shadows (0/1)",
        step: 1.0,
        get: |l| if l.shadows { 1.0 } else { 0.0 },
        set: |l, v| l.shadows = v >= 0.5,
    },
];

const STORAGE_KEY: &str = "dsor.lighting";

fn load_saved() -> Option<Lighting> {
    #[cfg(target_arch = "wasm32")]
    {
        let raw = web_sys::window()?.local_storage().ok()??.get_item(STORAGE_KEY).ok()??;
        serde_json::from_str(&raw).ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = STORAGE_KEY;
        serde_json::from_slice(&std::fs::read("lighting.json").ok()?).ok()
    }
}

fn save(l: &Lighting) {
    let Ok(raw) = serde_json::to_string(l) else { return };
    #[cfg(target_arch = "wasm32")]
    if let Some(Ok(Some(store))) = web_sys::window().map(|w| w.local_storage()) {
        let _ = store.set_item(STORAGE_KEY, &raw);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::fs::write("lighting.json", &raw);
    info!("lighting: {raw}");
}

#[derive(Component)]
struct Panel;
#[derive(Component)]
struct ValueText(usize);
#[derive(Component)]
struct StepButton(usize, f32);

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load_saved().unwrap_or_default())
            .add_systems(Startup, spawn_panel)
            .add_systems(Update, (toggle_panel, press_buttons, apply_lighting, refresh_values).chain());
    }
}

fn spawn_panel(mut commands: Commands) {
    let font = TextFont { font_size: bevy::text::FontSize::Px(14.0), ..default() };
    commands
        .spawn((
            Panel,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(8.0),
                top: Val::Px(8.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            // DSOR_PANEL=1 (native): open from the start, for screenshots.
            if std::env::var("DSOR_PANEL").is_ok() { Visibility::Inherited } else { Visibility::Hidden },
        ))
        .with_children(|panel| {
            panel.spawn((Text::new("Lighting (F2) - Shift: x10"), font.clone(), TextColor(Color::srgb(1.0, 0.85, 0.4))));
            for (i, f) in FIELDS.iter().enumerate() {
                panel
                    .spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() })
                    .with_children(|row| {
                        row.spawn((Text::new(f.label), font.clone(), Node { width: Val::Px(150.0), ..default() }));
                        for (sign, label) in [(-1.0, "-"), (1.0, "+")] {
                            row.spawn((
                                Button,
                                StepButton(i, sign),
                                Node { width: Val::Px(24.0), justify_content: JustifyContent::Center, ..default() },
                                BackgroundColor(Color::srgb(0.25, 0.25, 0.3)),
                            ))
                            .with_children(|b| {
                                b.spawn((Text::new(label), font.clone()));
                            });
                        }
                        row.spawn((ValueText(i), Text::new(""), font.clone(), Node { width: Val::Px(70.0), ..default() }));
                    });
            }
        });
}

fn toggle_panel(keys: Res<ButtonInput<KeyCode>>, mut panel: Query<&mut Visibility, With<Panel>>) {
    if keys.just_pressed(KeyCode::F2) {
        for mut v in &mut panel {
            *v = if *v == Visibility::Hidden { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

fn press_buttons(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<(&Interaction, &StepButton), Changed<Interaction>>,
    mut lighting: ResMut<Lighting>,
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
        save(&lighting);
    }
}

fn apply_lighting(
    lighting: Res<Lighting>,
    mut suns: Query<(&mut DirectionalLight, &mut Transform)>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut cameras: Query<(Entity, Option<&mut ColorGrading>), With<Camera3d>>,
    mut commands: Commands,
) {
    if !lighting.is_changed() && !cameras.iter().any(|(_, g)| g.is_none()) {
        return;
    }
    let l = &*lighting;
    let az = l.sun_azimuth.to_radians();
    let el = l.sun_elevation.to_radians();
    // The light travels from the sun towards the ground.
    let from_sun = -Vec3::new(az.sin() * el.cos(), el.sin(), az.cos() * el.cos());
    for (mut sun, mut tf) in &mut suns {
        sun.illuminance = l.sun_illuminance;
        sun.color = Color::linear_rgb(l.sun_r, l.sun_g, l.sun_b);
        sun.shadow_maps_enabled = l.shadows;
        *tf = Transform::default().looking_to(from_sun, Vec3::Y);
    }
    ambient.brightness = l.ambient;
    ambient.color = Color::linear_rgb(l.ambient_r, l.ambient_g, l.ambient_b);
    let grading = ColorGrading {
        global: ColorGradingGlobal {
            exposure: l.exposure,
            temperature: l.temperature,
            tint: l.tint,
            post_saturation: l.saturation,
            ..default()
        },
        midtones: ColorGradingSection { saturation: l.midtone_saturation, contrast: l.contrast, gamma: l.gamma, ..default() },
        shadows: ColorGradingSection { contrast: l.contrast, gamma: l.gamma, ..default() },
        highlights: ColorGradingSection { contrast: l.contrast, gamma: l.gamma, ..default() },
    };
    for (e, g) in &mut cameras {
        match g {
            Some(mut g) => *g = grading.clone(),
            None => {
                commands.entity(e).insert(grading.clone());
            }
        }
    }
}

fn refresh_values(lighting: Res<Lighting>, mut texts: Query<(&ValueText, &mut Text)>) {
    if !lighting.is_changed() {
        return;
    }
    for (ValueText(i), mut t) in &mut texts {
        let v = (FIELDS[*i].get)(&lighting);
        t.0 = if FIELDS[*i].step >= 1.0 { format!("{v:.0}") } else { format!("{v:.2}") };
    }
}
