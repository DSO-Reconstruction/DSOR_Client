//! Nebula3 particle emitters (chimney smoke, torch flames, lantern glows...).
//!
//! tools/embed_emitters.py copies each emitter of a model's `.fx.json` sidecar
//! into the extras of the glTF node holding its emitter mesh. While the model
//! loads, that becomes an [`Emitter`] on the node; the mesh under it (drawn
//! hidden, see crate::materials) gives the emission points and normals, and its
//! texture the particle texture.
//!
//! Simulated on the CPU and drawn as camera-facing quads: every emitter sharing a
//! texture and blend mode goes into one mesh rebuilt each frame, so the whole map
//! is a handful of draw calls and runs on WebGL2.
//!
//! The model is Nebula3's particle system, from the parameters' own names and
//! values (no engine source in the workspace):
//! - emitter curves (frequency, lifetime, spread, start velocity) are sampled
//!   over the emission duration, particle curves (size, colour, mass, rotation,
//!   time manipulator, velocity factor) over each particle's age;
//! - acceleration = (wind * air resistance + gravity up) * mass. Torch flames
//!   (gravity 10, mass 0.45, life 0.65 s) rise about a metre that way; chimney
//!   smoke (gravity 1.14, mass 0.09, air resistance up to 18.5) only leaves the
//!   chimney when the wind carries it;
//! - spread angles are degrees around the emission normal; rotations radians;
//! - `texture_tile` n: the texture is n frames stacked vertically, one per
//!   particle, picked at birth (p_smoke_01 is two puffs in 128x256).
//! UNVERIFIED against the 2018 client: quad half-size = size, the tile layout,
//! the wind (Nebula3's default, +X, 1 unit).

use std::collections::HashMap;

use bevy::asset::{LoadContext, RenderAssetUsages};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::gltf::extensions::{ErasedGltfExtensionHandler, GltfExtensionHandler, GltfExtensionHandlers};
use bevy::gltf::gltf;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

/// Envelope curve: 4 values, 2 key positions, then sine modulation (freq, amp, mode).
type Envelope = [f32; 9];

const FREQUENCY: usize = 0;
const LIFETIME: usize = 1;
const SPREAD_MIN: usize = 2;
const SPREAD_MAX: usize = 3;
const START_VELOCITY: usize = 4;
const ROTATION_VELOCITY: usize = 5;
const SIZE: usize = 6;
const MASS: usize = 7;
const TIME_MANIPULATOR: usize = 8;
const VELOCITY_FACTOR: usize = 9;
const AIR_RESISTANCE: usize = 10;
const RED: usize = 11;
const GREEN: usize = 12;
const BLUE: usize = 13;
const ALPHA: usize = 14;
const ENVELOPE_KEYS: [&str; 15] = [
    "emission_frequency", "lifetime", "spread_min", "spread_max", "start_velocity",
    "rotation_velocity", "particle_size", "particle_mass", "time_manipulator",
    "velocity_factor", "air_resistance", "color_red", "color_green", "color_blue",
    "color_alpha",
];

/// The scene wind. UNVERIFIED: Nebula3's default; calibrate against the game.
const WIND: Vec3 = Vec3::new(1.0, 0.0, 0.0);
/// Simulation step for the warm-up (`precalc_time`).
const PRECALC_STEP: f32 = 1.0 / 30.0;
/// Longest frame simulated in one go; a stall is not replayed.
const MAX_DT: f32 = 0.1;
/// Live particles per emitter, a guard against bad data.
const MAX_PARTICLES: usize = 512;

fn sample(e: &Envelope, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let (k0, k1) = (e[4].clamp(0.0, 1.0), e[5].clamp(0.0, 1.0));
    let lerp = |a: f32, b: f32, f: f32| a + (b - a) * f.clamp(0.0, 1.0);
    let mut v = if t < k0 {
        lerp(e[0], e[1], if k0 > 0.0 { t / k0 } else { 1.0 })
    } else if t < k1 {
        lerp(e[1], e[2], (t - k0) / (k1 - k0).max(1e-6))
    } else {
        lerp(e[2], e[3], (t - k1) / (1.0 - k1).max(1e-6))
    };
    if e[7] != 0.0 {
        let phase = t * e[6] * std::f32::consts::TAU;
        v += e[7] * if e[8] == 0.0 { phase.sin() } else { phase.cos() };
    }
    v
}

/// One emitter's parameters, from `extras.dsor_emitter`.
#[derive(Component, Reflect, Default, Clone)]
#[reflect(Component, Default)]
pub struct Emitter {
    envelopes: Vec<Envelope>,
    duration: f32,
    looping: bool,
    activity_distance: f32,
    billboard: bool,
    start_rotation_min: f32,
    start_rotation_max: f32,
    gravity: f32,
    texture_tile: u32,
    velocity_randomize: f32,
    rotation_randomize: f32,
    size_randomize: f32,
    precalc_time: f32,
    randomize_rotation: bool,
    start_delay: f32,
    additive: bool,
}

impl Emitter {
    fn from_json(v: &serde_json::Value) -> Option<Self> {
        let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        let envelopes = ENVELOPE_KEYS
            .iter()
            .map(|k| {
                let a = v.get(*k)?.as_array()?;
                let mut e = [0.0; 9];
                for (i, x) in a.iter().take(9).enumerate() {
                    e[i] = x.as_f64().unwrap_or(0.0) as f32;
                }
                Some(e)
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            envelopes,
            duration: num("emission_duration").max(0.01),
            looping: num("looping") != 0.0,
            activity_distance: if num("activity_distance") > 0.0 { num("activity_distance") } else { 100.0 },
            billboard: num("billboard") != 0.0,
            start_rotation_min: num("start_rotation_min"),
            start_rotation_max: num("start_rotation_max"),
            gravity: num("gravity"),
            texture_tile: (num("texture_tile") as u32).max(1),
            velocity_randomize: num("velocity_randomize"),
            rotation_randomize: num("rotation_randomize"),
            size_randomize: num("size_randomize"),
            precalc_time: num("precalc_time"),
            randomize_rotation: num("randomize_rotation") != 0.0,
            start_delay: num("start_delay"),
            additive: v.get("additive").and_then(|x| x.as_bool()).unwrap_or(false),
        })
    }
}

#[derive(Default, Clone)]
struct NodeEmitters;

impl GltfExtensionHandler for NodeEmitters {
    fn dyn_clone(&self) -> Box<dyn ErasedGltfExtensionHandler> {
        Box::new(self.clone())
    }

    fn on_gltf_node(&mut self, _: &mut LoadContext<'_>, gltf_node: &::gltf::Node, entity: &mut EntityWorldMut) {
        let Some(extras) = gltf_node.extras() else { return };
        let raw = extras.get();
        if !raw.contains("dsor_emitter") {
            return;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else { return };
        if let Some(e) = v.get("dsor_emitter").and_then(Emitter::from_json) {
            entity.insert(e);
        }
    }
}

struct Particle {
    pos: Vec3,
    vel: Vec3,
    age: f32,
    inv_life: f32,
    size_var: f32,
    rot: f32,
    rot_var: f32,
    frame: u32,
}

/// Runtime state, added once the emitter mesh is loaded.
#[derive(Component)]
struct EmitterState {
    /// Emission points and normals, in the emitter node's space.
    points: Vec<(Vec3, Vec3)>,
    group: usize,
    particles: Vec<Particle>,
    time: f32,
    pending: f32,
    active: bool,
    rng: u32,
}

impl EmitterState {
    fn rand(&mut self) -> f32 {
        // xorshift32
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// One draw: every particle with the same texture and blend mode.
struct Group {
    mesh: Handle<Mesh>,
    tile: u32,
}

#[derive(Resource, Default)]
struct Groups {
    by_key: HashMap<(Option<AssetId<Image>>, bool, u32), usize>,
    list: Vec<Group>,
}

pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Emitter>();
        let handlers = app.world().resource::<GltfExtensionHandlers>().0.clone();
        #[cfg(target_family = "wasm")]
        bevy::tasks::block_on(async { handlers.write().await.push(Box::new(NodeEmitters)) });
        #[cfg(not(target_family = "wasm"))]
        handlers.write_blocking().push(Box::new(NodeEmitters));
        app.init_resource::<Groups>()
            .add_systems(Update, (start_emitters, simulate).chain())
            .add_systems(PostUpdate, draw.after(TransformSystems::Propagate));
    }
}

#[allow(clippy::type_complexity)]
fn start_emitters(
    mut commands: Commands,
    mut groups: ResMut<Groups>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new: Query<(Entity, &Emitter, &Children), Without<EmitterState>>,
    surfaces: Query<(&Mesh3d, &MeshMaterial3d<StandardMaterial>, &Transform)>,
    mut seed: Local<u32>,
) {
    for (entity, emitter, children) in &new {
        let Some((mesh, material, local)) = children.iter().find_map(|c| surfaces.get(c).ok()) else { continue };
        let Some(m) = meshes.get(&mesh.0) else { continue };
        let Ok(VertexAttributeValues::Float32x3(pos)) = m.try_attribute(Mesh::ATTRIBUTE_POSITION) else { continue };
        let normals: Vec<[f32; 3]> = match m.try_attribute(Mesh::ATTRIBUTE_NORMAL) {
            Ok(VertexAttributeValues::Float32x3(n)) => n.clone(),
            _ => vec![[0.0, 1.0, 0.0]; pos.len()],
        };
        let points: Vec<(Vec3, Vec3)> = pos
            .iter()
            .zip(normals.iter())
            .map(|(p, n)| {
                (local.transform_point(Vec3::from(*p)), (local.rotation * Vec3::from(*n)).normalize_or(Vec3::Y))
            })
            .collect();
        if points.is_empty() {
            continue;
        }
        let texture = materials.get(&material.0).and_then(|m| m.base_color_texture.clone());
        let key = (texture.as_ref().map(|t| t.id()), emitter.additive, emitter.texture_tile);
        let group = match groups.by_key.get(&key) {
            Some(&g) => g,
            None => {
                let mat = materials.add(StandardMaterial {
                    base_color_texture: texture,
                    alpha_mode: if emitter.additive { AlphaMode::Add } else { AlphaMode::Blend },
                    unlit: true,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                });
                let mesh = meshes.add(empty_mesh());
                commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(mat),
                    NotShadowCaster,
                    NoFrustumCulling,
                    Transform::IDENTITY,
                    Name::new("particles"),
                ));
                groups.list.push(Group { mesh, tile: emitter.texture_tile });
                let g = groups.list.len() - 1;
                groups.by_key.insert(key, g);
                g
            }
        };
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        commands.entity(entity).insert(EmitterState {
            points,
            group,
            particles: Vec::new(),
            time: 0.0,
            pending: 0.0,
            active: false,
            rng: *seed | 1,
        });
    }
}

fn step(e: &Emitter, s: &mut EmitterState, at: &GlobalTransform, dt: f32) {
    let env = &e.envelopes;
    // Emission, sampled over the emitter's duration.
    s.time += dt;
    let t = s.time - e.start_delay;
    if t >= 0.0 && (e.looping || t <= e.duration) {
        let rel = if e.looping { (t % e.duration) / e.duration } else { t / e.duration };
        s.pending += sample(&env[FREQUENCY], rel).max(0.0) * dt;
        let rot = at.rotation();
        while s.pending >= 1.0 {
            s.pending -= 1.0;
            if s.particles.len() >= MAX_PARTICLES {
                continue;
            }
            let i = ((s.rand() * s.points.len() as f32) as usize).min(s.points.len() - 1);
            let (p, n) = s.points[i];
            let normal = (rot * n).normalize_or(Vec3::Y);
            let theta = sample(&env[SPREAD_MIN], rel)
                + (sample(&env[SPREAD_MAX], rel) - sample(&env[SPREAD_MIN], rel)) * s.rand();
            let rho = std::f32::consts::TAU * s.rand();
            let side = normal.any_orthonormal_vector();
            let dir = Quat::from_axis_angle(normal, rho) * (Quat::from_axis_angle(side, theta.to_radians()) * normal);
            let speed = sample(&env[START_VELOCITY], rel) * (1.0 - s.rand() * e.velocity_randomize);
            let life = sample(&env[LIFETIME], rel).max(0.01);
            let mut rot_var = 1.0 - s.rand() * e.rotation_randomize;
            if e.randomize_rotation && s.rand() < 0.5 {
                rot_var = -rot_var;
            }
            let start_rot = e.start_rotation_min + (e.start_rotation_max - e.start_rotation_min) * s.rand();
            let size_var = 1.0 - s.rand() * e.size_randomize;
            let frame = ((s.rand() * e.texture_tile as f32) as u32).min(e.texture_tile - 1);
            s.particles.push(Particle {
                pos: at.transform_point(p),
                vel: dir * speed,
                age: 0.0,
                inv_life: 1.0 / life,
                size_var,
                rot: start_rot,
                rot_var,
                frame,
            });
        }
    }
    // Particles, sampled over their own age.
    s.particles.retain_mut(|p| {
        let a = p.age;
        let dt = dt * sample(&env[TIME_MANIPULATOR], a);
        let acc = (WIND * sample(&env[AIR_RESISTANCE], a) + Vec3::Y * e.gravity) * sample(&env[MASS], a);
        p.vel += acc * dt;
        p.pos += p.vel * dt * sample(&env[VELOCITY_FACTOR], a);
        p.rot += p.rot_var * sample(&env[ROTATION_VELOCITY], a) * dt;
        p.age += p.inv_life * dt;
        p.age < 1.0
    });
}

fn simulate(
    time: Res<Time>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut emitters: Query<(&Emitter, &mut EmitterState, &GlobalTransform, &InheritedVisibility)>,
) {
    let Ok(cam) = cameras.single() else { return };
    let eye = cam.translation();
    let dt = time.delta_secs().min(MAX_DT);
    for (e, mut s, at, shown) in &mut emitters {
        // Culled with its map cell: no simulation either.
        let near = shown.get() && at.translation().distance_squared(eye) <= e.activity_distance * e.activity_distance;
        if !near {
            if s.active {
                s.active = false;
                s.particles.clear();
            }
            continue;
        }
        if !s.active {
            // Entering range: start in the steady state, as precalc_time asks.
            s.active = true;
            s.time = 0.0;
            let mut warm = e.precalc_time.min(10.0);
            while warm > 0.0 {
                step(e, &mut s, at, PRECALC_STEP);
                warm -= PRECALC_STEP;
            }
        }
        step(e, &mut s, at, dt);
    }
}

fn empty_mesh() -> Mesh {
    // One degenerate triangle: an empty vertex buffer is not drawable.
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 3])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2]))
}

#[derive(Default)]
struct Buffers {
    pos: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    color: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

fn draw(
    groups: Res<Groups>,
    mut meshes: ResMut<Assets<Mesh>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    emitters: Query<(&Emitter, &EmitterState, &GlobalTransform, &InheritedVisibility)>,
    mut drawn: Local<Vec<bool>>,
) {
    let Ok(cam) = cameras.single() else { return };
    let (right, up) = (cam.right().as_vec3(), cam.up().as_vec3());
    let mut buffers: Vec<Buffers> = (0..groups.list.len()).map(|_| Buffers::default()).collect();
    for (e, s, at, vis) in &emitters {
        if !s.active || !vis.get() {
            continue;
        }
        let b = &mut buffers[s.group];
        let tile = groups.list[s.group].tile as f32;
        // Not billboarded: the quad lies in the emitter's own XY plane.
        let (r0, u0) = if e.billboard { (right, up) } else { (at.right().as_vec3(), at.up().as_vec3()) };
        for p in &s.particles {
            let a = p.age;
            let half = sample(&e.envelopes[SIZE], a) * p.size_var;
            let (sin, cos) = p.rot.sin_cos();
            let r = (r0 * cos + u0 * sin) * half;
            let u = (u0 * cos - r0 * sin) * half;
            let c = [
                sample(&e.envelopes[RED], a),
                sample(&e.envelopes[GREEN], a),
                sample(&e.envelopes[BLUE], a),
                sample(&e.envelopes[ALPHA], a).clamp(0.0, 1.0),
            ];
            let (v0, v1) = (p.frame as f32 / tile, (p.frame + 1) as f32 / tile);
            let base = b.pos.len() as u32;
            for (corner, uv) in [(-r - u, [0.0, v1]), (r - u, [1.0, v1]), (r + u, [1.0, v0]), (-r + u, [0.0, v0])] {
                b.pos.push((p.pos + corner).into());
                b.uv.push(uv);
                b.color.push(c);
            }
            b.idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    drawn.resize(groups.list.len(), true);
    for (i, b) in buffers.into_iter().enumerate() {
        if b.idx.is_empty() {
            // Clear once, then leave the empty mesh alone.
            if drawn[i] {
                drawn[i] = false;
                if let Some(mut m) = meshes.get_mut(&groups.list[i].mesh) {
                    *m = empty_mesh();
                }
            }
            continue;
        }
        drawn[i] = true;
        let n = b.pos.len();
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, b.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, b.uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, b.color)
            .with_inserted_indices(Indices::U32(b.idx));
        if let Some(mut m) = meshes.get_mut(&groups.list[i].mesh) {
            *m = mesh;
        }
    }
}
