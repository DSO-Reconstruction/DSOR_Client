//! The look of a level, from the 2018 data: its global light, ambient light, fog,
//! bloom, colour grading and vignette, and the lights placed in it
//! (tools/export_ambience.py -> `maps/<map>.ambience.json`, the level's default
//! `_Instance_AmbienceBubble` and its `_Instance_Light` rows).
//!
//! Nebula lights in gamma space and shows the result without tonemapping:
//!   colour = albedo x (sat(N.L) x LightColor x LightIntensity + LightAmbient
//!            + sat(BackLightFactor - N.L) x LightOppositeColor x LightIntensity)
//!            + point lights, each LightColor x LightIntensity x sat(N.L) x sat(1 - d / range)
//! EVIDENCE: shaders_sm30 "lightsources" GlobalLight / PointLight ps_3_0, the
//!   "standard" Solid ps_3_0 (lit colour = light buffer x DiffMap0) and "pe_compose"
//!   (no curve, only saturation, balance, bloom, vignette and fade).
//! Bevy lights linearly and divides the diffuse by pi, so each term becomes its
//! linear value in Bevy's units (see `UNIT`): what the 2018 renderer shows as `v`
//! Bevy shows as `v` too once the camera is not tonemapped.
//!
//! F2 opens a panel that scales each term (Shift: ten steps at once); 1 everywhere
//! is the level's own look. The factors are kept in the browser (localStorage) or,
//! natively, in `look.json` in the working directory.

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{DirectionalLight, GlobalAmbientLight, PointLight};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::post_process::effect_stack::Vignette;
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal};
use serde::{Deserialize, Serialize};

use crate::map::{CurrentMap, CELL};

/// One Nebula light unit (a light buffer value of 1) in Bevy's units: the inverse
/// of the camera's default exposure (EV100 9.7, `Exposure::BLENDER`).
const UNIT: f32 = 1.2 * 831.746; // 1.2 x 2^9.7
/// Bevy's diffuse ambient is `EnvBRDFApprox(albedo, F_AB(1, NdotV))` x ambient:
/// 0.452 x albedo at roughness 1 (Karis' fit); Nebula's is albedo x ambient.
const AMBIENT_BRDF: f32 = 0.452;
/// The light that follows the player (LocalLightColor x LocalLightIntensity):
/// `_Template_RemotePlayer.PlayerLightOffset` (1, 1, -0.5) from the player.
/// UNVERIFIED: its range (the data has none).
const PLAYER_LIGHT_OFFSET: Vec3 = Vec3::new(1.0, 1.0, -0.5);
const PLAYER_LIGHT_RANGE: f32 = 8.0;

/// Gamma value -> linear, per channel (the sRGB curve, also above 1).
fn linear(c: [f32; 3], k: f32) -> LinearRgba {
    let l = Color::srgb(c[0] * k, c[1] * k, c[2] * k).to_linear();
    LinearRgba::rgb(l.red, l.green, l.blue)
}

/// A Nebula point light: gamma colour x intensity, falling off linearly to its
/// range. Bevy's falloff is replaced by Nebula's (`NebulaFalloff`: sat(1 - d/r),
/// ^2.2 as the light is added in gamma space), so the light is its colour x
/// intensity at the centre and nothing at its range, with no inverse-square hot
/// spot near it: matched at half range with bevy's 1/d^2, a lamp lit the wall
/// beside it ~9 times too bright ("trop emissive les lights").
/// E / pi x exposure = linear(colour x intensity) x f: intensity / (4 pi) = pi x UNIT.
pub fn point_light(color: [f32; 3], intensity: f32, range: f32) -> PointLight {
    PointLight {
        color: Color::LinearRgba(linear(color, intensity)),
        intensity: 4.0 * std::f32::consts::PI.powi(2) * UNIT,
        range,
        shadow_maps_enabled: false,
        ..default()
    }
}

/// Bevy's `getDistanceAttenuation` (bevy_pbr::lighting) replaced, once the module
/// is loaded, by Nebula's linear point light falloff. Point and spot lights only.
/// EVIDENCE: the frame's Lights batch (b_empty, default.xml) and docs/fx.md
///   ("Nebula's linear falloff").
fn nebula_falloff(mut shaders: ResMut<Assets<bevy::shader::Shader>>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let target = bevy::shader::ShaderImport::Custom("bevy_pbr::lighting".into());
    let Some(id) = shaders.iter().find(|(_, s)| s.import_path == target).map(|(id, _)| id) else { return };
    *done = true;
    let Some(mut shader) = shaders.get_mut(id) else { return };
    let bevy::shader::Source::Wgsl(src) = &shader.source else { return };
    let old = "fn getDistanceAttenuation(distanceSquare: f32, inverseRangeSquared: f32) -> f32 {\n    return getRangeFalloff(distanceSquare, inverseRangeSquared) * 1.0 / max(distanceSquare, 0.0001);\n}";
    if !src.contains(old) {
        warn!("bevy_pbr::lighting: getDistanceAttenuation not as expected; point lights keep bevy's falloff");
        return;
    }
    let new = "fn getDistanceAttenuation(distanceSquare: f32, inverseRangeSquared: f32) -> f32 {\n    // Nebula: linear to the range, in gamma space (crate::lighting).\n    return pow(saturate(1.0 - sqrt(distanceSquare * inverseRangeSquared)), 2.2);\n}";
    let patched = src.replace(old, new);
    shader.source = bevy::shader::Source::Wgsl(patched.into());
    info!("point lights: Nebula's linear falloff");
}

/// An ambience bubble's settings (the fields the client uses; names as in the
/// level database).
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "PascalCase", default)]
pub struct Bubble {
    pub light_color: [f32; 4],
    pub light_opposite_color: [f32; 4],
    pub light_intensity: f32,
    pub back_light_factor: f32,
    pub light_ambient: [f32; 4],
    pub saturation: f32,
    pub bloom_scale: f32,
    pub bloom_color: [f32; 4],
    pub bright_pass_threshold: f32,
    pub fog_color: [f32; 4],
    pub fog_intensity: f32,
    pub fog_near_dist: f32,
    pub fog_far_dist: f32,
    pub local_light_color: [f32; 4],
    pub local_light_intensity: f32,
    pub vignette_intensity: f32,
    pub vignette_size: f32,
    pub vignette_color: [f32; 4],
    /// Seconds to fade into this bubble's look.
    #[serde(rename = "PEFadeTime")]
    pub fade_time: f32,
    /// The light's transform z axis: towards the light.
    #[serde(rename = "dir")]
    pub dir: Option<[f32; 3]>,
}

fn mix4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

impl Bubble {
    /// `self` faded `t` (0..1) of the way to `to`.
    fn mix(&self, to: &Bubble, t: f32) -> Bubble {
        let f = |a: f32, b: f32| a + (b - a) * t;
        let dir = match (self.dir, to.dir) {
            (Some(a), Some(b)) => Some(Vec3::from_array(a).lerp(Vec3::from_array(b), t).normalize_or(Vec3::Y).to_array()),
            (a, b) => b.or(a),
        };
        Bubble {
            light_color: mix4(self.light_color, to.light_color, t),
            light_opposite_color: mix4(self.light_opposite_color, to.light_opposite_color, t),
            light_intensity: f(self.light_intensity, to.light_intensity),
            back_light_factor: f(self.back_light_factor, to.back_light_factor),
            light_ambient: mix4(self.light_ambient, to.light_ambient, t),
            saturation: f(self.saturation, to.saturation),
            bloom_scale: f(self.bloom_scale, to.bloom_scale),
            bloom_color: mix4(self.bloom_color, to.bloom_color, t),
            bright_pass_threshold: f(self.bright_pass_threshold, to.bright_pass_threshold),
            fog_color: mix4(self.fog_color, to.fog_color, t),
            fog_intensity: f(self.fog_intensity, to.fog_intensity),
            fog_near_dist: f(self.fog_near_dist, to.fog_near_dist),
            fog_far_dist: f(self.fog_far_dist, to.fog_far_dist),
            local_light_color: mix4(self.local_light_color, to.local_light_color, t),
            local_light_intensity: f(self.local_light_intensity, to.local_light_intensity),
            vignette_intensity: f(self.vignette_intensity, to.vignette_intensity),
            vignette_size: f(self.vignette_size, to.vignette_size),
            vignette_color: mix4(self.vignette_color, to.vignette_color, t),
            fade_time: to.fade_time,
            dir,
        }
    }
}

/// A bubble with a place of its own: inside its volume (a unit box or a sphere
/// of diameter 1 through its transform), the level takes its look, the highest
/// priority winning, faded in over its PEFadeTime.
/// UNVERIFIED: the sphere's unit radius (0.5, as the box's half side).
#[derive(Deserialize, Clone, Debug)]
pub struct LocalBubble {
    #[serde(flatten)]
    pub look: Bubble,
    pub m: Option<[f32; 16]>,
    #[serde(default)]
    pub priority: i32,
    /// The event set row it belongs to; -1 when always there.
    #[serde(default)]
    pub event: i32,
    #[serde(default)]
    pub shape: String,
}

impl LocalBubble {
    fn contains(&self, p: Vec3) -> bool {
        let Some(m) = self.m else { return false };
        let local = Mat4::from_cols_array(&m).inverse().transform_point3(p);
        if self.shape == "Sphere" {
            local.length() <= 0.5
        } else {
            local.abs().max_element() <= 0.5
        }
    }
}

impl Default for Bubble {
    /// Kingshill's (a0200_kingscity), for a level without data.
    fn default() -> Self {
        Self {
            light_color: [0.851, 0.757, 0.682, 1.0],
            light_opposite_color: [0.176, 0.231, 0.349, 1.0],
            light_intensity: 1.3,
            back_light_factor: 0.3,
            light_ambient: [0.855, 0.831, 0.8, 0.98],
            saturation: 1.0,
            bloom_scale: 1.0,
            bloom_color: [1.0, 0.71, 0.502, 1.0],
            bright_pass_threshold: 0.7,
            fog_color: [0.486, 0.745, 0.78, 1.0],
            fog_intensity: 1.0,
            fog_near_dist: 25.0,
            fog_far_dist: 50.0,
            local_light_color: [1.0, 0.71, 0.502, 1.0],
            local_light_intensity: 0.25,
            vignette_intensity: 1.0,
            vignette_size: 2.0,
            vignette_color: [0.0, 0.0, 0.0, 1.0],
            fade_time: 2.0,
            dir: Some([-0.136, 0.721, -0.679]),
        }
    }
}

/// A light placed in the level (`_Instance_Light`, always-on rows).
#[derive(Deserialize, Clone, Debug)]
pub struct LevelLight {
    /// 0 point, 1 projected spot (not drawn: see crate docs/fx.md).
    pub t: u8,
    pub p: [f32; 3],
    pub color: [f32; 3],
    pub i: f32,
    pub r: f32,
    /// [frequency, intensity] when it flickers.
    pub flicker: Option<[f32; 2]>,
}

#[derive(Asset, TypePath, Deserialize, Debug, Default)]
pub struct Ambience {
    pub global: Option<Bubble>,
    #[serde(default)]
    pub bubbles: Vec<LocalBubble>,
    #[serde(default)]
    pub lights: Vec<LevelLight>,
}

#[derive(Default, TypePath)]
pub struct AmbienceLoader;

impl AssetLoader for AmbienceLoader {
    type Asset = Ambience;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<Ambience, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)
    }

    fn extensions(&self) -> &[&str] {
        &["ambience.json"]
    }
}

/// The current level's ambience and what of it has been applied.
#[derive(Resource, Default)]
struct Level {
    map: String,
    ambience: Handle<Ambience>,
    /// The look in force (the data's, or the default until it loads), and the
    /// fade it is in: from, to, which local bubble (None: the level's own), done.
    bubble: Bubble,
    fade: Option<(Bubble, Bubble, f32)>,
    inside: Option<usize>,
    bubble_loaded: bool,
    lights_placed: bool,
}

/// The main directional light (spawned by main.rs): the level's global light.
#[derive(Component)]
pub struct Sun;

/// The global light's back light: from the opposite direction, no shadow.
#[derive(Component)]
struct BackLight;

#[derive(Component)]
struct PlayerLight;

/// A level light: its intensity at factor 1, and its flicker (frequency, depth,
/// phase) when it has one.
#[derive(Component)]
struct LevelLightPower {
    base: f32,
    flicker: Option<(f32, f32, f32)>,
}

/// Scale factors over the level's own look, tunable in game (F2).
#[derive(Resource, Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Lighting {
    pub sun: f32,
    pub back: f32,
    pub ambient: f32,
    pub lights: f32,
    /// Exposure compensation, stops.
    pub exposure: f32,
    pub fog: f32,
    pub bloom: f32,
    pub saturation: f32,
    pub vignette: f32,
    /// The 2018 renderer clips; Bevy's TonyMcMapface rolls highlights off.
    pub tonemapping: bool,
    pub shadows: bool,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            sun: 1.0,
            back: 1.0,
            ambient: 1.0,
            lights: 1.0,
            exposure: 0.0,
            fog: 1.0,
            bloom: 1.0,
            saturation: 1.0,
            vignette: 1.0,
            tonemapping: false,
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
    Field { label: "Sun x", step: 0.05, get: |l| l.sun, set: |l, v| l.sun = v.max(0.0) },
    Field { label: "Back light x", step: 0.05, get: |l| l.back, set: |l, v| l.back = v.max(0.0) },
    Field { label: "Ambient x", step: 0.05, get: |l| l.ambient, set: |l, v| l.ambient = v.max(0.0) },
    Field { label: "Level lights x", step: 0.05, get: |l| l.lights, set: |l, v| l.lights = v.max(0.0) },
    Field { label: "Exposure", step: 0.05, get: |l| l.exposure, set: |l, v| l.exposure = v.clamp(-5.0, 5.0) },
    Field { label: "Fog x", step: 0.05, get: |l| l.fog, set: |l, v| l.fog = v.clamp(0.0, 2.0) },
    Field { label: "Bloom x", step: 0.05, get: |l| l.bloom, set: |l, v| l.bloom = v.clamp(0.0, 4.0) },
    Field { label: "Saturation x", step: 0.02, get: |l| l.saturation, set: |l, v| l.saturation = v.clamp(0.0, 3.0) },
    Field { label: "Vignette x", step: 0.05, get: |l| l.vignette, set: |l, v| l.vignette = v.clamp(0.0, 2.0) },
    Field {
        label: "Tonemapping (0/1)",
        step: 1.0,
        get: |l| if l.tonemapping { 1.0 } else { 0.0 },
        set: |l, v| l.tonemapping = v >= 0.5,
    },
    Field {
        label: "Shadows (0/1)",
        step: 1.0,
        get: |l| if l.shadows { 1.0 } else { 0.0 },
        set: |l, v| l.shadows = v >= 0.5,
    },
];

const STORAGE_KEY: &str = "dsor.look";

fn load_saved() -> Option<Lighting> {
    #[cfg(target_arch = "wasm32")]
    {
        let raw = web_sys::window()?.local_storage().ok()??.get_item(STORAGE_KEY).ok()??;
        serde_json::from_str(&raw).ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = STORAGE_KEY;
        serde_json::from_slice(&std::fs::read("look.json").ok()?).ok()
    }
}

fn save(l: &Lighting) {
    let Ok(raw) = serde_json::to_string(l) else { return };
    #[cfg(target_arch = "wasm32")]
    if let Some(Ok(Some(store))) = web_sys::window().map(|w| w.local_storage()) {
        let _ = store.set_item(STORAGE_KEY, &raw);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = std::fs::write("look.json", &raw);
    info!("look: {raw}");
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
        app.init_asset::<Ambience>()
            .init_asset_loader::<AmbienceLoader>()
            .init_resource::<Level>()
            .insert_resource(load_saved().unwrap_or_default())
            .add_systems(Update, nebula_falloff)
            .add_systems(Startup, (spawn_panel, spawn_back_light))
            .add_systems(
                Update,
                (
                    (toggle_panel, press_buttons, refresh_values).chain(),
                    (follow_level, apply_look, place_level_lights.run_if(resource_exists::<CurrentMap>)).chain(),
                    flicker,
                    player_light,
                ),
            );
    }
}

/// CONTRACT: not on WebGL2. bevy there takes one directional light; with a
///   second one it kept the back light and dropped the sun ("The amount of
///   directional lights of 2 is exceeding the supported limit of 1": no shadows,
///   flat light in the browser).
fn spawn_back_light(mut commands: Commands) {
    if crate::WEBGL2 {
        return;
    }
    commands.spawn((BackLight, DirectionalLight { shadow_maps_enabled: false, ..default() }, Transform::default()));
}

/// A new level: load its ambience.
/// A new level: load its ambience. Then follow the bubble the player (or the
/// camera's focus) stands in.
#[allow(clippy::too_many_arguments)]
fn follow_level(
    current: Option<Res<CurrentMap>>,
    mut level: ResMut<Level>,
    assets: Res<AssetServer>,
    ambiences: Res<Assets<Ambience>>,
    time: Res<Time>,
    manifests: Res<Assets<crate::map::MapManifest>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    players: Query<&GlobalTransform, With<crate::net::LocalPlayer>>,
) {
    let Some(current) = current else { return };
    if level.map != current.name {
        level.map = current.name.clone();
        level.ambience = assets.load(format!("maps/{}.ambience.json", current.name));
        level.bubble = Bubble::default();
        level.fade = None;
        level.inside = None;
        level.bubble_loaded = false;
        level.lights_placed = false;
    }
    let Some(ambience) = ambiences.get(&level.ambience) else { return };
    let global = ambience.global.clone().unwrap_or_default();
    if !level.bubble_loaded {
        level.bubble = global.clone();
        level.bubble_loaded = true;
    }
    if !ambience.bubbles.is_empty() {
        let ground = manifests.get(&current.manifest).map(|m| m.center[1]).unwrap_or(0.0);
        let focus = cameras.iter().next().map(|cam| crate::map::camera_focus(cam, players.iter().next().map(|p| p.translation()), ground));
        let inside = focus.and_then(|p| {
            ambience
                .bubbles
                .iter()
                .enumerate()
                .filter(|(_, b)| b.event == -1 && b.contains(p))
                .max_by_key(|(_, b)| b.priority)
                .map(|(i, _)| i)
        });
        if inside != level.inside {
            level.inside = inside;
            let to = inside.map(|i| ambience.bubbles[i].look.clone()).unwrap_or(global);
            let from = level.bubble.clone();
            level.fade = Some((from, to, 0.0));
        }
    }
    // Only while fading: touching `level` marks it changed, which re-applies the look.
    if level.fade.is_some() {
        let Some((from, to, t)) = level.fade.take() else { return };
        let t = t + time.delta_secs() / to.fade_time.max(0.01);
        level.bubble = from.mix(&to, t.min(1.0));
        if t < 1.0 {
            level.fade = Some((from, to, t));
        }
    }
}

#[allow(clippy::type_complexity)]
fn apply_look(
    mut commands: Commands,
    lighting: Res<Lighting>,
    level: Res<Level>,
    mut suns: Query<(&mut DirectionalLight, &mut Transform), (With<Sun>, Without<BackLight>)>,
    mut backs: Query<(&mut DirectionalLight, &mut Transform), (With<BackLight>, Without<Sun>)>,
    mut ambient: ResMut<GlobalAmbientLight>,
    cameras: Query<(Entity, Has<Bloom>), With<Camera3d>>,
    mut fresh: Local<Vec<Entity>>,
) {
    let new_camera = cameras.iter().any(|(e, _)| !fresh.contains(&e));
    if !lighting.is_changed() && !level.is_changed() && !new_camera {
        return;
    }
    fresh.clear();
    fresh.extend(cameras.iter().map(|(e, _)| e));
    let (l, b) = (&*lighting, &level.bubble);
    let rgb = |c: [f32; 4]| [c[0], c[1], c[2]];
    let toward = Vec3::from_array(b.dir.unwrap_or([-0.136, 0.721, -0.679])).normalize_or(Vec3::Y);
    let lit = |c: [f32; 4], k: f32| Color::LinearRgba(linear(rgb(c), k));
    for (mut sun, mut tf) in &mut suns {
        sun.color = lit(b.light_color, b.light_intensity);
        sun.illuminance = std::f32::consts::PI * UNIT * l.sun;
        sun.shadow_maps_enabled = l.shadows && !crate::flag("noshadows");
        *tf = Transform::default().looking_to(-toward, Vec3::Y);
    }
    for (mut back, mut tf) in &mut backs {
        // sat(BackLightFactor - N.L) is approximated by a light from -L.
        back.color = lit(b.light_opposite_color, b.light_intensity);
        back.illuminance = std::f32::consts::PI * UNIT * l.back;
        *tf = Transform::default().looking_to(toward, Vec3::Y);
    }
    let a = linear(rgb(b.light_ambient), 1.0);
    ambient.color = Color::LinearRgba(a);
    ambient.brightness = UNIT / AMBIENT_BRDF * l.ambient;

    // Fog: Nebula's factor is clamp((far - z) / (far - near), FogColor.w, 1).
    // UNVERIFIED: FogIntensity as the fog's opacity.
    let fog = DistanceFog {
        color: Color::srgba(b.fog_color[0], b.fog_color[1], b.fog_color[2], (b.fog_intensity * l.fog).clamp(0.0, 1.0)),
        falloff: FogFalloff::Linear { start: b.fog_near_dist, end: b.fog_far_dist.max(b.fog_near_dist + 0.1) },
        ..default()
    };
    // Bloom: Nebula's bright pass keeps lum(max(2c - threshold, 0)) of the scene,
    // added back blurred x BloomScale (pe_brightpassfilter, pe_compose).
    // Bevy's bloom has no tint (BloomColor) and thresholds linear values.
    let threshold = Color::srgb(b.bright_pass_threshold * 0.5, 0.0, 0.0).to_linear().red;
    let bloom = Bloom {
        intensity: 0.15 * b.bloom_scale * l.bloom,
        prefilter: BloomPrefilter { threshold, threshold_softness: 0.5 },
        composite_mode: BloomCompositeMode::Additive,
        ..Bloom::NATURAL
    };
    // Vignette: lerp(colour, VignetteColor, sat(d^VignetteSize x VignetteIntensity)),
    // d the distance to the screen centre in uv units. Bevy's with smoothness 1 and
    // full edge compensation is lerp(colour, color, (d / radius)^2).
    let vignette = Vignette {
        intensity: if b.vignette_intensity > 0.0 { l.vignette.min(1.0) } else { 0.0 },
        radius: 1.0 / (b.vignette_intensity * l.vignette.max(1.0)).max(1e-3).sqrt(),
        smoothness: 1.0,
        roundness: 1.0,
        edge_compensation: 1.0,
        color: Color::srgb(b.vignette_color[0], b.vignette_color[1], b.vignette_color[2]),
        ..default()
    };
    let grading = ColorGrading {
        global: ColorGradingGlobal { exposure: l.exposure, post_saturation: b.saturation * l.saturation, ..default() },
        ..default()
    };
    let tonemapping = if l.tonemapping { Tonemapping::TonyMcMapface } else { Tonemapping::None };
    for (e, _) in &cameras {
        let mut cam = commands.entity(e);
        cam.insert((fog.clone(), grading.clone(), vignette.clone(), tonemapping));
        // The 2018 client's own bloom (crate::nebula_bloom): from the opaque scene
        // only, as its frame shader does; bevy's took the effects too.
        cam.remove::<Bloom>();
        if crate::flag("nobloom") || b.bloom_scale * l.bloom <= 0.0 {
            cam.remove::<crate::nebula_bloom::NebulaBloom>();
        } else {
            cam.insert(crate::nebula_bloom::NebulaBloom {
                threshold: b.bright_pass_threshold,
                scale: b.bloom_scale * l.bloom,
                color: Vec4::from_array(b.bloom_color),
            });
        }
        let _ = &bloom;
    }
}

/// The level's own lights, each in the culling cell it stands in, so they go out
/// with the cell (crate::map::cull_cells) and with the map.
fn place_level_lights(
    mut commands: Commands,
    mut level: ResMut<Level>,
    mut current: ResMut<CurrentMap>,
    ambiences: Res<Assets<Ambience>>,
    lighting: Res<Lighting>,
) {
    if level.lights_placed || crate::flag("nolights") {
        return;
    }
    let (Some(root), Some(ambience)) = (current.root, ambiences.get(&level.ambience)) else { return };
    let mut placed = 0;
    for (n, light) in ambience.lights.iter().enumerate() {
        // Projected spots (LightType 1) need their texture and shadow: left out.
        if light.t != 0 || light.r <= 0.0 || light.i <= 0.0 {
            continue;
        }
        let at = crate::map::game_to_bevy(light.p);
        let key = ((at.x / CELL).floor() as i32, (at.z / CELL).floor() as i32);
        let cell = *current.cells.entry(key).or_insert_with(|| {
            commands.spawn((Name::new(format!("cell {key:?}")), Transform::default(), Visibility::default(), ChildOf(root))).id()
        });
        let pl = point_light(light.color, light.i, light.r);
        let power = LevelLightPower {
            base: pl.intensity,
            flicker: light.flicker.map(|[frequency, amount]| (frequency, amount.clamp(0.0, 1.0), n as f32 * 1.618)),
        };
        commands.spawn((PointLight { intensity: pl.intensity * lighting.lights, ..pl }, power, Transform::from_translation(at), ChildOf(cell)));
        placed += 1;
    }
    info!("level {}: {placed} lights", level.map);
    level.lights_placed = true;
}

/// Level lights at the panel's factor; flickering ones wander under their base,
/// `depth` deep. UNVERIFIED: the 2018 client's flicker curve.
fn flicker(time: Res<Time>, lighting: Res<Lighting>, mut lights: Query<(&mut PointLight, &LevelLightPower, &InheritedVisibility)>) {
    let t = time.elapsed_secs();
    for (mut l, power, shown) in &mut lights {
        let Some((frequency, depth, phase)) = power.flicker else {
            if lighting.is_changed() {
                l.intensity = power.base * lighting.lights;
            }
            continue;
        };
        if !shown.get() {
            continue;
        }
        let a = t * frequency + phase;
        let wander = 0.5 + 0.3 * a.sin() + 0.2 * (2.3 * a + 1.7).sin();
        l.intensity = power.base * lighting.lights * (1.0 - depth * 0.5 * wander);
    }
}

/// The light that follows the local player (the level's LocalLight).
fn player_light(
    mut commands: Commands,
    level: Res<Level>,
    lighting: Res<Lighting>,
    players: Query<Entity, Added<crate::net::LocalPlayer>>,
    mut lights: Query<&mut PointLight, With<PlayerLight>>,
) {
    let b = &level.bubble;
    let make = || {
        let mut l = point_light([b.local_light_color[0], b.local_light_color[1], b.local_light_color[2]], b.local_light_intensity, PLAYER_LIGHT_RANGE);
        l.intensity *= lighting.lights;
        l
    };
    for p in &players {
        commands.spawn((PlayerLight, make(), Transform::from_translation(PLAYER_LIGHT_OFFSET), ChildOf(p)));
    }
    if level.is_changed() || lighting.is_changed() {
        for mut l in &mut lights {
            *l = make();
        }
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
            panel.spawn((Text::new("Look (F2): the level's x factor - Shift: x10"), font.clone(), TextColor(Color::srgb(1.0, 0.85, 0.4))));
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

fn refresh_values(lighting: Res<Lighting>, mut texts: Query<(&ValueText, &mut Text)>) {
    if !lighting.is_changed() {
        return;
    }
    for (ValueText(i), mut t) in &mut texts {
        let v = (FIELDS[*i].get)(&lighting);
        t.0 = if FIELDS[*i].step >= 1.0 { format!("{v:.0}") } else { format!("{v:.2}") };
    }
}
