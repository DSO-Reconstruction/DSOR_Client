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
//! Camera colour, bloom and vignette come directly from the active bubble.
//! F2 scales scene lighting and fog (Shift: ten steps at once). Those factors
//! are saved in localStorage or, natively, `look.json` in the working directory.

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{DirectionalLight, GlobalAmbientLight, PointLight};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::post_process::effect_stack::Vignette;
use bevy::prelude::*;
use bevy::render::view::ColorGrading;
use crate::rendering::nebula_bloom::NebulaPostProcess;
use serde::{Deserialize, Serialize};

use crate::world::map::{CurrentMap, CELL};

mod blend;
#[cfg(test)]
mod color_tests;
use blend::{LightBlend, BlendPlugin, AmbienceView, GpuAmbience};

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
    let new = "fn getDistanceAttenuation(distanceSquare: f32, inverseRangeSquared: f32) -> f32 {\n    // Nebula: linear to the range, in gamma space (crate::rendering::lighting).\n    return pow(saturate(1.0 - sqrt(distanceSquare * inverseRangeSquared)), 2.2);\n}";
    let patched = src.replace(old, new);
    shader.source = bevy::shader::Source::Wgsl(patched.into());
    info!("point lights: Nebula's linear falloff");
}

/// An ambience bubble's settings (the fields the client uses; names as in the
/// level database).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Bubble {
    pub light_color: [f32; 4],
    pub light_opposite_color: [f32; 4],
    pub light_intensity: f32,
    pub light_shadow_intensity: f32,
    pub back_light_factor: f32,
    pub light_ambient: [f32; 4],
    pub saturation: f32,
    pub balance: [f32; 4],
    pub bloom_scale: f32,
    pub bloom_color: [f32; 4],
    pub bright_pass_threshold: f32,
    pub fog_color: [f32; 4],
    pub fog_intensity: f32,
    pub fog_near_dist: f32,
    pub fog_far_dist: f32,
    /// Height fog, Nebula's second fog layer: by world height, from none at
    /// `FogNearHeight` to all of `HeightFogColor` at `FogFarHeight`.
    pub height_fog_color: [f32; 4],
    pub fog_near_height: f32,
    pub fog_far_height: f32,
    /// The characters' rim light (shd:character, characterfalloff only): the
    /// shader receives `RimLightColor x RimLightIntensity`.
    pub rim_light_color: [f32; 4],
    pub rim_light_intensity: f32,
    pub local_light_color: [f32; 4],
    pub local_light_intensity: f32,
    /// The player light's reach. Not in the level data (always PLAYER_LIGHT_RANGE
    /// there); the Lighting Menu can override it.
    pub local_light_range: f32,
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
    /// Linear direct-light energy, before the user's sun/exposure controls.
    pub fn sun_luminance(&self) -> f32 {
        let c = linear([self.light_color[0], self.light_color[1], self.light_color[2]], self.light_intensity);
        Vec3::new(c.red, c.green, c.blue).dot(Vec3::new(0.2126, 0.7152, 0.0722))
    }

    /// `self` faded `t` (0..1) of the way to `to`.
    fn mix(&self, to: &Bubble, t: f32) -> Bubble {
        if t <= 0. { return self.clone(); }
        if t >= 1. { return to.clone(); }
        let f = |a: f32, b: f32| a + (b - a) * t;
        Bubble {
            light_color: mix4(self.light_color, to.light_color, t),
            light_opposite_color: mix4(self.light_opposite_color, to.light_opposite_color, t),
            light_intensity: f(self.light_intensity, to.light_intensity),
            light_shadow_intensity: f(self.light_shadow_intensity, to.light_shadow_intensity),
            back_light_factor: f(self.back_light_factor, to.back_light_factor),
            light_ambient: mix4(self.light_ambient, to.light_ambient, t),
            saturation: f(self.saturation, to.saturation),
            balance: mix4(self.balance, to.balance, t),
            bloom_scale: f(self.bloom_scale, to.bloom_scale),
            bloom_color: mix4(self.bloom_color, to.bloom_color, t),
            bright_pass_threshold: f(self.bright_pass_threshold, to.bright_pass_threshold),
            fog_color: mix4(self.fog_color, to.fog_color, t),
            fog_intensity: f(self.fog_intensity, to.fog_intensity),
            fog_near_dist: f(self.fog_near_dist, to.fog_near_dist),
            fog_far_dist: f(self.fog_far_dist, to.fog_far_dist),
            height_fog_color: mix4(self.height_fog_color, to.height_fog_color, t),
            fog_near_height: f(self.fog_near_height, to.fog_near_height),
            fog_far_height: f(self.fog_far_height, to.fog_far_height),
            rim_light_color: mix4(self.rim_light_color, to.rim_light_color, t),
            rim_light_intensity: f(self.rim_light_intensity, to.rim_light_intensity),
            local_light_color: mix4(self.local_light_color, to.local_light_color, t),
            local_light_intensity: f(self.local_light_intensity, to.local_light_intensity),
            local_light_range: f(self.local_light_range, to.local_light_range),
            vignette_intensity: f(self.vignette_intensity, to.vignette_intensity),
            vignette_size: f(self.vignette_size, to.vignette_size),
            vignette_color: mix4(self.vignette_color, to.vignette_color, t),
            fade_time: to.fade_time,
            // Direction is preset metadata. LightBlend fades the contributions
            // of fixed directions; neither lighting nor shadows rotate.
            dir: to.dir.or(self.dir),
        }
    }
}

/// A bubble with a place of its own: inside its volume (a unit box or a sphere
/// of diameter 1 through its transform), the level takes its look, the highest
/// priority winning, faded in over its PEFadeTime.
/// UNVERIFIED: the sphere's unit radius (0.5, as the box's half side).
#[derive(Deserialize, Clone, Debug, PartialEq)]
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
    #[serde(default = "enabled")]
    pub enabled: bool,
}
fn enabled() -> bool { true }

#[cfg(test)]
mod bubble_tests {
    use super::*;

    fn volume(at: Mat4, shape: &str, priority: i32) -> LocalBubble {
        LocalBubble { look: default(), m: Some(at.to_cols_array()), priority, event: -1, shape: shape.into(), enabled: true }
    }

    #[test]
    fn boundaries_follow_rotated_scaled_volume_and_reject_invalid_transforms() {
        let at = Mat4::from_scale_rotation_translation(Vec3::new(12.0, 6.0, 8.0), Quat::from_rotation_y(0.7), Vec3::new(100.0, 14.0, -162.0));
        for shape in ["Box", "Sphere"] {
            let bubble = volume(at, shape, 0);
            assert!(bubble.contains(at.transform_point3(Vec3::new(0.499, 0.0, 0.0))));
            assert!(!bubble.contains(at.transform_point3(Vec3::new(0.501, 0.0, 0.0))));
            assert!(!bubble.contains(at.transform_point3(Vec3::new(0.0, 0.501, 0.0))));
        }
        assert!(!volume(Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0)), "Box", 0).contains(Vec3::ZERO));
        assert!(!volume(Mat4::IDENTITY, "Box", 0).contains(Vec3::splat(f32::NAN)));
    }

    #[test]
    fn disabled_and_event_bubbles_do_not_override_player_volume() {
        let mut ambience = Ambience { bubbles: vec![volume(Mat4::IDENTITY, "Box", 1), volume(Mat4::IDENTITY, "Box", 2)], ..default() };
        assert_eq!(ambience.bubble_at(Vec3::ZERO), Some(1));
        ambience.bubbles[1].enabled = false;
        assert_eq!(ambience.bubble_at(Vec3::ZERO), Some(0));
        ambience.bubbles[1].enabled = true;
        ambience.bubbles[1].event = 83;
        assert_eq!(ambience.bubble_at(Vec3::ZERO), Some(0));
        assert_eq!(ambience.bubble_at(Vec3::X), None);
    }

    #[test]
    fn height_fog_is_read_from_the_level_and_fades_between_bubbles() {
        // a0302_wildforest's default bubble.
        let wild: Bubble = serde_json::from_str(r#"{"HeightFogColor":[0.2275,0.6627,0.6902,1.0],"FogNearHeight":-3.0,"FogFarHeight":-20.0}"#).unwrap();
        assert_eq!((wild.height_fog_color, wild.fog_near_height, wild.fog_far_height), ([0.2275, 0.6627, 0.6902, 1.0], -3.0, -20.0));
        // Assets without the columns keep Kingshill's, like every other field.
        let legacy: Bubble = serde_json::from_str("{}").unwrap();
        assert_eq!((legacy.fog_near_height, legacy.fog_far_height), (Bubble::default().fog_near_height, Bubble::default().fog_far_height));
        let half = Bubble::default().mix(&wild, 0.5);
        assert!((half.fog_near_height - (-2.0)).abs() < 1e-5 && (half.fog_far_height - (-25.0)).abs() < 1e-5);
        assert!((half.height_fog_color[0] - (1.0 + 0.2275) / 2.0).abs() < 1e-5);
    }

    #[test]
    fn rim_light_is_read_from_the_level_and_fades_between_bubbles() {
        // a0302_wildforest's default bubble.
        let wild: Bubble = serde_json::from_str(r#"{"RimLightColor":[0.2275,0.6627,0.6902,1.0],"RimLightIntensity":0.5}"#).unwrap();
        assert_eq!((wild.rim_light_color, wild.rim_light_intensity), ([0.2275, 0.6627, 0.6902, 1.0], 0.5));
        let dark: Bubble = serde_json::from_str(r#"{"RimLightIntensity":0.0}"#).unwrap();
        let half = wild.mix(&dark, 0.5);
        assert!((half.rim_light_intensity - 0.25).abs() < 1e-5);
    }

    #[test]
    fn light_blend_is_independent_of_volume_scale() {
        let from = Bubble { dir: Some(Vec3::X.to_array()), ..default() };
        let to = Bubble { dir: Some((Vec3::Z * 42.603).to_array()), ..default() };
        let unscaled = Bubble { dir: Some(Vec3::Z.to_array()), ..to.clone() };
        assert_eq!(LightBlend::from(&from).mix(&to, 0.5), LightBlend::from(&from).mix(&unscaled, 0.5));
    }

    #[test]
    fn player_position_wins_over_camera_position_rotation_and_availability() {
        let outside = Vec3::new(20., 0., 0.);
        let camera = Camera::default();
        let at = GlobalTransform::from(Transform::IDENTITY);
        let ambience = Ambience { bubbles: vec![volume(Mat4::IDENTITY, "Box", 1)], ..default() };
        for cameras in [vec![(&camera, &at)], vec![]] {
            assert_eq!(ambience.bubble_at(observer_position(Some(outside), cameras.into_iter()).unwrap()), None);
        }
        let far_camera = GlobalTransform::from(Transform::from_translation(outside).looking_at(Vec3::ZERO, Vec3::Y));
        assert_eq!(ambience.bubble_at(observer_position(Some(Vec3::ZERO), [(&camera, &far_camera)].into_iter()).unwrap()), Some(0));
    }

    #[test]
    fn free_camera_rotation_cannot_enter_a_bubble_but_movement_fades_in_and_out() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Ambience>()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(100)))
            .add_systems(Update, follow_level);
        let mut local = volume(Mat4::from_scale(Vec3::splat(10.)), "Box", 1);
        local.look.light_intensity = 4.;
        local.look.fade_time = 1.;
        local.look.dir = Some(Vec3::Y.to_array());
        let global = Bubble { light_intensity: 1., fade_time: 1., dir: Some(Vec3::Y.to_array()), ..default() };
        let handle = app.world_mut().resource_mut::<Assets<Ambience>>().add(Ambience {
            global: Some(global.clone()), bubbles: vec![local], ..default()
        });
        app.insert_resource(CurrentMap::new("test".into(), Handle::default()))
            .insert_resource(Level { map: "test".into(), ambience: handle, ..default() });
        let outside = Vec3::new(0., 10., 20.);
        let camera = app.world_mut().spawn((Camera3d::default(),
            GlobalTransform::from(Transform::from_translation(outside).looking_at(Vec3::ZERO, Vec3::Y)))).id();
        // Thumbnail and inactive cameras must not prevent the main camera from
        // being evaluated, or accidentally activate a volume of their own.
        app.world_mut().spawn((Camera3d::default(), NoLevelLook, GlobalTransform::IDENTITY));
        app.world_mut().spawn((Camera3d::default(), Camera { is_active: false, ..default() }, GlobalTransform::IDENTITY));
        app.update();
        for target in [Vec3::ZERO, Vec3::new(0., 0., 40.), Vec3::ZERO] {
            app.world_mut().entity_mut(camera).insert(GlobalTransform::from(Transform::from_translation(outside).looking_at(target, Vec3::Y)));
            app.update();
            let level = app.world().resource::<Level>();
            assert_eq!(level.inside, None);
            assert_eq!(level.bubble, global);
            assert!(level.fade.is_none());
        }
        app.world_mut().entity_mut(camera).insert(GlobalTransform::from_translation(Vec3::ZERO));
        app.update();
        let level = app.world().resource::<Level>();
        assert_eq!(level.inside, Some(0));
        assert!(level.bubble.light_intensity > 1. && level.bubble.light_intensity < 4.);
        assert!(level.fade.is_some());
        for _ in 0..12 { app.update(); }
        assert_eq!(app.world().resource::<Level>().bubble.light_intensity, 4.);
        app.world_mut().entity_mut(camera).insert(GlobalTransform::from_translation(outside));
        app.update();
        let level = app.world().resource::<Level>();
        assert_eq!(level.inside, None);
        assert!(level.bubble.light_intensity > 1. && level.bubble.light_intensity < 4.);
        for _ in 0..12 { app.update(); }
        assert_eq!(app.world().resource::<Level>().bubble, global);
    }

    #[test]
    #[ignore = "requires the converted Kingscity assets with authored overrides applied"]
    fn kingscity_entrances_stay_outdoors_until_the_doorway() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/maps/a0200_kingscity.ambience.json");
        let ambience: Ambience = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        // Church: the porch begins at x=137, the open doors are at x=140.
        assert_eq!(ambience.bubble_at(Vec3::new(139.9, 1.0, -53.0)), None);
        assert!(ambience.bubble_at(Vec3::new(140.1, 1.0, -53.0)).is_some());
        // Palace: entrance arch at z=-140, room-side entrance caps at z=-143.
        assert_eq!(ambience.bubble_at(Vec3::new(100.0, 11.0, -142.9)), None);
        assert!(ambience.bubble_at(Vec3::new(100.0, 11.0, -143.1)).is_some());
    }
}

impl LocalBubble {
    pub fn contains(&self, p: Vec3) -> bool {
        let Some(m) = self.m else { return false };
        let transform = Mat4::from_cols_array(&m);
        if !p.is_finite() || !transform.is_finite() || transform.determinant().abs() < 1e-8 {
            return false;
        }
        let local = transform.inverse().transform_point3(p);
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
            light_shadow_intensity: 1.0,
            back_light_factor: 0.3,
            light_ambient: [0.855, 0.831, 0.8, 0.98],
            saturation: 1.0,
            balance: [1.0; 4],
            bloom_scale: 1.0,
            bloom_color: [1.0, 0.71, 0.502, 1.0],
            bright_pass_threshold: 0.7,
            fog_color: [0.486, 0.745, 0.78, 1.0],
            fog_intensity: 1.0,
            fog_near_dist: 25.0,
            fog_far_dist: 50.0,
            height_fog_color: [1.0, 0.961, 0.902, 1.0],
            fog_near_height: -1.0,
            fog_far_height: -30.0,
            rim_light_color: [1.0, 1.0, 0.902, 1.0],
            rim_light_intensity: 0.5,
            local_light_color: [1.0, 0.71, 0.502, 1.0],
            local_light_intensity: 0.25,
            local_light_range: PLAYER_LIGHT_RANGE,
            vignette_intensity: 1.0,
            vignette_size: 2.0,
            vignette_color: [0.0, 0.0, 0.0, 1.0],
            fade_time: 2.0,
            dir: Some([-0.136, 0.721, -0.679]),
        }
    }
}

/// A light placed in the level (`_Instance_Light`).
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct LevelLight {
    /// 0 point, 1 spot. Legacy projected lights with a zero cone stay omitted.
    pub t: u8,
    pub p: [f32; 3],
    pub color: [f32; 3],
    pub i: f32,
    pub r: f32,
    /// [frequency, intensity] when it flickers.
    pub flicker: Option<[f32; 2]>,
    #[serde(default)] pub dir: Option<[f32; 3]>,
    #[serde(default)] pub cone: f32,
    #[serde(default)] pub shadows: bool,
    #[serde(default = "no_event")] pub event: i32,
}
fn no_event() -> i32 { -1 }

/// Shared by the game and editor: identical light units, cone and shadow rules.
pub fn spawn_level_light(commands: &mut Commands, light: &LevelLight, parent: Entity, at: Transform, phase: f32, factor: f32) {
    if light.r <= 0. || light.i <= 0. || light.t > 1 || (light.t == 1 && light.cone <= 0.) { return; }
    let mut point = point_light(light.color, light.i, light.r);
    point.shadow_maps_enabled = light.shadows;
    let power = LevelLightPower { base: point.intensity,
        flicker: light.flicker.map(|[frequency, amount]| (frequency, amount.clamp(0., 1.), phase)) };
    point.intensity *= factor;
    if light.t == 0 {
        commands.spawn((point, power, at, ChildOf(parent)));
    } else {
        let outer = (light.cone.to_radians() * 0.5).clamp(0.001, std::f32::consts::FRAC_PI_2 - 0.001);
        commands.spawn((SpotLight { color: point.color, intensity: point.intensity, range: point.range,
            shadow_maps_enabled: light.shadows, inner_angle: outer * 0.8, outer_angle: outer, ..default() }, power, at, ChildOf(parent)));
    }
}

#[derive(Asset, TypePath, Deserialize, Debug, Default)]
pub struct Ambience {
    pub global: Option<Bubble>,
    /// Authored map-wide shadow projection, independent of local light directions.
    pub shadow: Option<[f32; 3]>,
    #[serde(default)]
    pub bubbles: Vec<LocalBubble>,
    #[serde(default)]
    pub lights: Vec<LevelLight>,
}

impl Ambience {
    fn bubble_at(&self, position: Vec3) -> Option<usize> {
        self.bubbles.iter().enumerate()
            .filter(|(_, b)| b.enabled && b.event == -1 && b.contains(position))
            .max_by_key(|(_, b)| b.priority)
            .map(|(i, _)| i)
    }
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
pub(crate) struct Level {
    pub(crate) map: String,
    pub(crate) ambience: Handle<Ambience>,
    /// The current post effects, radiance mixture and fixed map shadow direction.
    bubble: Bubble,
    blend: LightBlend,
    shadow_dir: Vec3,
    fade: Option<(Bubble, Bubble, LightBlend, f32)>,
    /// The local bubble in effect (None: the global one).
    pub(crate) inside: Option<usize>,
    bubble_loaded: bool,
    lights_placed: bool,
}

/// The map's fixed shadow caster (spawned by main.rs). Preset lighting is
/// evaluated separately by the ambience shader using this light's shadow map.
#[derive(Component)]
pub struct Sun;

#[derive(Component)]
struct PlayerLight;

/// A level light: its intensity at factor 1, and its flicker (frequency, depth,
/// phase) when it has one.
#[derive(Component)]
struct LevelLightPower {
    base: f32,
    flicker: Option<(f32, f32, f32)>,
}

/// Scene light and fog factors, tunable in game (F2). Camera colour is authored
/// by ambience bubbles; legacy colour overrides in saved JSON are ignored.
#[derive(Resource, Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Lighting {
    pub sun: f32,
    pub back: f32,
    pub ambient: f32,
    pub lights: f32,
    pub fog: f32,
    /// Scale the authored fog distances beyond the camera's focus.
    pub fog_range: f32,
    pub shadows: bool,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            sun: 1.0,
            back: 0.9,
            ambient: 0.9,
            lights: 1.75,
            fog: 0.55,
            fog_range: 0.5,
            shadows: true,
        }
    }
}

/// One tunable line: label, step, read, write. Shared with the Lighting Menu.
pub(crate) struct Field {
    pub(crate) label: &'static str,
    pub(crate) step: f32,
    pub(crate) get: fn(&Lighting) -> f32,
    pub(crate) set: fn(&mut Lighting, f32),
}

pub(crate) const FIELDS: &[Field] = &[
    Field { label: "Sun x", step: 0.05, get: |l| l.sun, set: |l, v| l.sun = v.max(0.0) },
    Field { label: "Back light x", step: 0.05, get: |l| l.back, set: |l, v| l.back = v.max(0.0) },
    Field { label: "Ambient x", step: 0.05, get: |l| l.ambient, set: |l, v| l.ambient = v.max(0.0) },
    Field { label: "Level lights x", step: 0.05, get: |l| l.lights, set: |l, v| l.lights = v.max(0.0) },
    Field { label: "Fog x", step: 0.05, get: |l| l.fog, set: |l, v| l.fog = v.clamp(0.0, 2.0) },
    Field { label: "Fog range x", step: 0.25, get: |l| l.fog_range, set: |l, v| l.fog_range = v.clamp(0.25, 4.0) },
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

pub(crate) fn save(l: &Lighting) {
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

/// Thumbnail cameras retain their studio lighting and omit map post effects.
#[derive(Component)] pub struct NoLevelLook;
#[derive(Resource, Default)] pub struct EditorLook { pub bubble: Bubble, pub shadow_dir: Option<[f32; 3]>, pub shadow_reference: f32 }
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)] pub struct ApplyEditorLighting;
pub struct EditorLightingPlugin;
impl Plugin for EditorLightingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(BlendPlugin).init_resource::<Level>().init_resource::<EditorLook>()
            .insert_resource(load_saved().unwrap_or_default())
            .add_systems(Update, nebula_falloff)
            .add_systems(Update, (copy_editor_look, apply_look, flicker).chain().in_set(ApplyEditorLighting))
            .add_systems(PostUpdate, shift_fog.after(bevy::transform::TransformSystems::Propagate));
    }
}
fn copy_editor_look(look: Res<EditorLook>, mut level: ResMut<Level>) {
    if look.is_changed() {
        level.bubble = look.bubble.clone();
        level.blend = LightBlend::with_reference(&look.bubble, look.shadow_reference);
        level.shadow_dir = blend::direction(look.shadow_dir);
    }
}

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(BlendPlugin).init_asset::<Ambience>()
            .init_asset_loader::<AmbienceLoader>()
            .init_resource::<Level>()
            .insert_resource(load_saved().unwrap_or_default())
            .add_systems(Update, nebula_falloff)
            // The Lighting Menu replaces this panel when it is on (crate::diagnostics::lighting_menu).
            .add_systems(Startup, spawn_panel.run_if(|| !crate::diagnostics::lighting_menu::enabled()))
            .add_systems(
                Update,
                (
                    (toggle_panel, press_buttons, refresh_values).chain(),
                    flicker,
                    player_light,
                ),
            )
            .add_systems(PostUpdate,
                (follow_level, apply_look, death_look, place_level_lights.run_if(resource_exists::<CurrentMap>), shift_fog)
                    .chain().after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::light::cascade::build_directional_light_cascades));
    }
}

fn observer_position<'a>(player: Option<Vec3>, cameras: impl Iterator<Item = (&'a Camera, &'a GlobalTransform)>) -> Option<Vec3> {
    player.or_else(|| {
        let mut active = cameras.filter(|(camera, _)| camera.is_active);
        let (_, at) = active.next()?;
        active.next().is_none().then(|| at.translation())
    })
}

/// A new level: load its ambience. Then follow the bubble the player (or the
/// map viewer's camera) stands in, after this frame's movement has propagated.
#[allow(clippy::too_many_arguments)]
fn follow_level(
    current: Option<Res<CurrentMap>>,
    mut level: ResMut<Level>,
    assets: Res<AssetServer>,
    ambiences: Res<Assets<Ambience>>,
    time: Res<Time>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>, Without<NoLevelLook>)>,
    players: Query<&GlobalTransform, With<crate::network::net::LocalPlayer>>,
    menu: Option<Res<crate::diagnostics::lighting_menu::presets::LightingMenuStore>>,
) {
    let Some(current) = current else { return };
    if level.map != current.name {
        level.map = current.name.clone();
        level.ambience = assets.load(format!("maps/{}.ambience.json", current.name));
        level.bubble = Bubble::default();
        level.blend = LightBlend::from(&level.bubble);
        level.shadow_dir = blend::direction(level.bubble.dir);
        level.fade = None;
        level.inside = None;
        level.bubble_loaded = false;
        level.lights_placed = false;
    }
    let Some(ambience) = ambiences.get(&level.ambience) else { return };
    // The Lighting Menu's working overrides (none without DSOR_STUDIO_PREVIEW):
    // each bubble is resolved through them before it is faded to.
    use crate::diagnostics::lighting_menu::presets::resolve_bubble;
    let edits = menu.as_deref().and_then(|s| s.working_map_overrides.get(&level.map));
    let global = resolve_bubble(&ambience.global.clone().unwrap_or_default(), edits, None);
    let resolve = |i: usize| resolve_bubble(&ambience.bubbles[i].look, edits, Some((i, &ambience.bubbles[i])));
    if menu.as_ref().is_some_and(|s| s.is_changed() && !s.is_added()) && level.bubble_loaded {
        // A live edit: show it at once, without a fade.
        let mut now = level.inside.map(resolve).unwrap_or_else(|| global.clone());
        now.dir = global.dir;
        level.blend = LightBlend::from(&now);
        level.bubble = now;
        level.shadow_dir = blend::direction(ambience.shadow.or(global.dir));
        level.fade = None;
    }
    if !level.bubble_loaded {
        level.bubble = global.clone();
        level.blend = LightBlend::from(&global);
        level.shadow_dir = blend::direction(ambience.shadow.or(global.dir));
        level.bubble_loaded = true;
    }
    // The menu's "preview selected zone": that bubble (or the global one) in
    // effect wherever the observer is, event bubbles included.
    let preview = menu.as_deref().and_then(|s| s.preview_target);
    if !ambience.bubbles.is_empty() || preview.is_some() {
        // A spawned player's feet are authoritative, independent of camera
        // offsets, zoom, and whether a camera is available during map handoff.
        // A free camera must enter the volume too: its ground focus moves when
        // looking around and can cross a doorway while the camera is outside.
        let position = observer_position(players.single().ok().map(|p| p.translation()), cameras.iter());
        let inside = match preview {
            Some(target) => target.filter(|&i| i < ambience.bubbles.len()),
            None => position.and_then(|p| ambience.bubble_at(p)),
        };
        if inside != level.inside {
            level.inside = inside;
            // A local box never moves the sun: its Transform is its volume, not a
            // light. FAILURE: its z axis (horizontal, ~173 deg from the level's sun
            // on a0007_pastures_pve) was faded to at every border, swinging the sun
            // over the zenith ("le soleil fait des allers-retours").
            let mut to = inside.map(resolve).unwrap_or_else(|| global.clone());
            to.dir = global.dir;
            let from = level.bubble.clone();
            level.fade = Some((from, to, level.blend.clone(), 0.0));
        }
    }
    // Only while fading: touching `level` marks it changed, which re-applies the look.
    if level.fade.is_some() {
        let Some((from, to, from_blend, t)) = level.fade.take() else { return };
        let t = t + time.delta_secs() / to.fade_time.max(0.01);
        level.bubble = from.mix(&to, t.min(1.0));
        level.blend = from_blend.mix(&to, t.min(1.0));
        if t < 1.0 {
            level.fade = Some((from, to, from_blend, t));
        }
    }
}

#[allow(clippy::type_complexity)]
fn apply_look(
    mut commands: Commands,
    lighting: Res<Lighting>,
    level: Res<Level>,
    mut suns: Query<(&mut DirectionalLight, &mut Transform, &mut GlobalTransform), With<Sun>>,
    mut gpu_ambience: ResMut<GpuAmbience>,
    mut ambient: ResMut<GlobalAmbientLight>,
    cameras: Query<Entity, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>, Without<NoLevelLook>)>,
    mut fresh: Local<Vec<Entity>>,
) {
    let new_camera = cameras.iter().any(|e| !fresh.contains(&e));
    if !lighting.is_changed() && !level.is_changed() && !new_camera {
        return;
    }
    fresh.clear();
    fresh.extend(cameras.iter());
    let (l, b) = (&*lighting, &level.bubble);
    let rgb = |c: [f32; 4]| [c[0], c[1], c[2]];
    *gpu_ambience = level.blend.gpu(l).with_height_fog(b).with_rim_light(b);
    let toward = level.shadow_dir.normalize_or(blend::direction(None));
    for (mut sun, mut tf, mut global_tf) in &mut suns {
        // The single real sun supplies the fixed shadow map. The fragment
        // shader evaluates the preset light contributions using that map.
        sun.color = Color::WHITE;
        sun.illuminance = std::f32::consts::PI * UNIT;
        sun.shadow_maps_enabled = l.shadows && !crate::flag("noshadows");
        *tf = Transform::default().looking_to(-toward, Vec3::Y);
        // apply_look follows propagation so player membership is current.
        // This unparented light must reach cascade construction this frame.
        *global_tf = GlobalTransform::from(*tf);
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
    for e in &cameras {
        let mut cam = commands.entity(e);
        cam.insert(AmbienceView);
        // Compose bloom, saturation, balance and vignette together in display
        // space, as pe_compose does. Zero bloom still needs colour composition.
        cam.insert((fog.clone(), ColorGrading::default(), Tonemapping::None,
            NebulaPostProcess {
                threshold: b.bright_pass_threshold,
                scale: if crate::flag("nobloom") { 0.0 } else { b.bloom_scale },
                color: Vec4::from_array(b.bloom_color),
                saturation: b.saturation,
                balance: Vec4::from_array(b.balance),
                vignette_color: Vec4::from_array(b.vignette_color),
                vignette_intensity: b.vignette_intensity,
                vignette_size: b.vignette_size,
                death_fade: 0.0,
            }));
        cam.remove::<(Bloom, Vignette)>();
    }
}

/// The level's own lights, each in the culling cell it stands in, so they go out
/// with the cell (crate::world::map::cull_cells) and with the map.
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
        if light.event != -1 || light.t > 1 || (light.t == 1 && light.cone <= 0.) || light.r <= 0.0 || light.i <= 0.0 {
            continue;
        }
        let at = crate::world::map::game_to_bevy(light.p);
        let key = ((at.x / CELL).floor() as i32, (at.z / CELL).floor() as i32);
        let cell = *current.cells.entry(key).or_insert_with(|| {
            commands.spawn((Name::new(format!("cell {key:?}")), Transform::default(), Visibility::default(), ChildOf(root))).id()
        });
        let direction = -Vec3::from_array(light.dir.unwrap_or([0., 1., 0.])).normalize_or(Vec3::Y);
        let transform = Transform::from_translation(at).looking_to(direction, Vec3::Y);
        spawn_level_light(&mut commands, light, cell, transform, n as f32 * 1.618, lighting.lights);
        placed += 1;
    }
    info!("level {}: {placed} lights", level.map);
    level.lights_placed = true;
}

/// Level lights at the panel's factor; flickering ones wander under their base,
/// `depth` deep. UNVERIFIED: the 2018 client's flicker curve.
fn flicker(time: Res<Time>, lighting: Res<Lighting>, mut lights: Query<(&mut PointLight, &LevelLightPower, &InheritedVisibility)>,
    mut spots: Query<(&mut SpotLight, &LevelLightPower, &InheritedVisibility)>) {
    let t = time.elapsed_secs();
    let intensity = |power: &LevelLightPower, shown: &InheritedVisibility| {
        let Some((frequency, depth, phase)) = power.flicker else {
            return Some(power.base * lighting.lights);
        };
        if !shown.get() { return None; }
        let a = t * frequency + phase;
        let wander = 0.5 + 0.3 * a.sin() + 0.2 * (2.3 * a + 1.7).sin();
        Some(power.base * lighting.lights * (1.0 - depth * 0.5 * wander))
    };
    for (mut l, power, shown) in &mut lights {
        if let Some(value) = intensity(power, shown) { l.intensity = value; }
    }
    for (mut l, power, shown) in &mut spots {
        if let Some(value) = intensity(power, shown) { l.intensity = value; }
    }
}

/// The light that follows the local player (the level's LocalLight).
fn player_light(
    mut commands: Commands,
    level: Res<Level>,
    lighting: Res<Lighting>,
    players: Query<Entity, Added<crate::network::net::LocalPlayer>>,
    mut lights: Query<&mut PointLight, With<PlayerLight>>,
) {
    let b = &level.bubble;
    let make = || {
        let mut l = point_light([b.local_light_color[0], b.local_light_color[1], b.local_light_color[2]], b.local_light_intensity, b.local_light_range);
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
            crate::ui::input::BlocksWorldInput,
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
            panel.spawn((Text::new("Camera color: active ambience bubble"), font.clone()));
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

/// Bevy measures fog from the camera. Start the map's authored range beyond
/// its focus so the camera-to-player distance does not consume the clear range.
/// Previously `zoom - CAM_HEIGHT_MAX` left only 2 clear units behind the
/// player in Kingshill (near 27, camera distance 25), washing out the upper
/// half of the top-down view. Use the actual transform for both play and the
/// free-camera map viewer, after camera/player transforms have propagated.
fn shift_fog(
    level: Res<Level>,
    lighting: Res<Lighting>,
    current: Option<Res<CurrentMap>>,
    manifests: Res<Assets<crate::world::map::MapManifest>>,
    players: Query<&GlobalTransform, With<crate::network::net::LocalPlayer>>,
    mut fogs: Query<(&GlobalTransform, &mut DistanceFog), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>, Without<NoLevelLook>)>,
) {
    let b = &level.bubble;
    let ground = current.as_ref().and_then(|m| manifests.get(&m.manifest)).map(|m| m.center[1]).unwrap_or(0.0);
    let player = players.iter().next().map(|p| p.translation());
    for (camera, mut fog) in &mut fogs {
        let focus = crate::world::map::camera_focus(camera, player, ground);
        let shift = camera.translation().distance(focus);
        let start = shift + b.fog_near_dist.max(0.0) * lighting.fog_range;
        let end = (shift + b.fog_far_dist * lighting.fog_range).max(start + 0.1);
        if let FogFalloff::Linear { start: s, end: e } = fog.falloff {
            if (s - start).abs() > 1e-3 || (e - end).abs() > 1e-3 {
                fog.falloff = FogFalloff::Linear { start, end };
            }
        }
    }
}

/// Dead: the scene drains of colour and darkens over a second, and comes back on
/// revival. This transient effect leaves the bubble's authored values intact.
/// UNVERIFIED: the 2018 client's own death look (its post-effect values are not
///   traced): a near-greyscale, dimmer screen.
fn death_look(
    net: Option<NonSend<crate::network::net::Net>>,
    time: Res<Time>,
    mut fade: Local<f32>,
    mut cameras: Query<&mut NebulaPostProcess, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>, Without<NoLevelLook>)>,
) {
    let target = if net.as_ref().is_some_and(|n| n.death.dead) { 1.0 } else { 0.0 };
    let step = time.delta_secs() / 1.2;
    *fade = if target > *fade { (*fade + step).min(target) } else { (*fade - step).max(target) };
    for mut grading in &mut cameras {
        if grading.death_fade != *fade {
            grading.death_fade = *fade;
        }
    }
}
