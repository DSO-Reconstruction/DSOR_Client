//! Ground decals: Nebula's `decal` shader projects its texture straight down a
//! unit box (scaled thin in Y: 0.16 to 2.2 units in Kingshill) onto whatever lies
//! inside it -- stone patches, sand, paths. Drawing the box itself gave flat
//! squares; projecting needs the depth buffer, which WebGL2 cannot sample.
//!
//! So the projection is done once, on the CPU: once the map is in, every opaque
//! ground triangle inside a box is clipped to it. The mask is laid over the box
//! (uv_b = box x + 0.5, z + 0.5); the colour repeats over the ground at the n3
//! node's `Scale` (uv = world x, z * Scale; 0.2 in Kingshill, so the stones keep
//! their size however big the box). All decals sharing textures end up in one
//! mesh, which also removes their ~350 box entities from the frame.
//! UNVERIFIED against the 2018 client: the colour in world (not box) space.

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::light::NotShadowCaster;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::map::CurrentMap;
use crate::materials::DecalVolume;

/// The decal's mask, cut over its box (uv_b); see shaders/decal.wgsl.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct DecalMask {
    #[texture(100)]
    #[sampler(101)]
    mask: Handle<Image>,
}

impl MaterialExtension for DecalMask {
    fn fragment_shader() -> ShaderRef {
        "embedded://dsor_client/shaders/decal.wgsl".into()
    }
}

pub type DecalMaterial = ExtendedMaterial<StandardMaterial, DecalMask>;

/// Frames to wait after every model has loaded, for scenes to spawn and their
/// transforms to propagate.
const SETTLE_FRAMES: u32 = 10;
/// Grid cell of the ground index, world units.
const CELL: f32 = 8.0;
/// Triangles steeper than this (|normal.y| below it) are walls: left out, as a
/// straight-down projection would only smear them.
const MIN_UP: f32 = 0.5;
/// Lift over the ground, on top of the material's depth bias.
const LIFT: f32 = 0.02;

/// A decal box already drawn.
#[derive(Component)]
struct Projected;

pub struct DecalPlugin;

impl Plugin for DecalPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/decal.wgsl");
        app.add_plugins(MaterialPlugin::<DecalMaterial>::default())
            .add_systems(Update, project_decals.run_if(resource_exists::<CurrentMap>));
    }
}

#[derive(Default)]
struct Batch {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    uvs_b: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

/// What a batch is drawn with.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Look {
    /// Colour repeated at a scale, mask over the box.
    Tiled(AssetId<Image>, AssetId<Image>, u32),
    /// The box material's merged texture over the box (no dsor_decal extras).
    Stretched(AssetId<StandardMaterial>),
}

fn world_aabb(affine: &bevy::math::Affine3A, min: Vec3, max: Vec3) -> (Vec3, Vec3) {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for i in 0..8 {
        let c = Vec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        );
        let w = affine.transform_point3(c);
        lo = lo.min(w);
        hi = hi.max(w);
    }
    (lo, hi)
}

fn cells(lo: Vec3, hi: Vec3) -> impl Iterator<Item = (i32, i32)> {
    let (x0, x1) = ((lo.x / CELL).floor() as i32, (hi.x / CELL).floor() as i32);
    let (z0, z1) = ((lo.z / CELL).floor() as i32, (hi.z / CELL).floor() as i32);
    (x0..=x1).flat_map(move |x| (z0..=z1).map(move |z| (x, z)))
}

/// Sutherland-Hodgman against the unit box, in box space.
fn clip_to_box(mut poly: Vec<Vec3>) -> Vec<Vec3> {
    for axis in 0..3 {
        for sign in [-1.0f32, 1.0] {
            if poly.is_empty() {
                return poly;
            }
            // inside: sign * p[axis] <= 0.5
            let d = |p: Vec3| 0.5 - sign * p[axis];
            let mut out = Vec::with_capacity(poly.len() + 2);
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                let (da, db) = (d(a), d(b));
                if da >= 0.0 {
                    out.push(a);
                }
                if (da >= 0.0) != (db >= 0.0) {
                    out.push(a + (b - a) * (da / (da - db)));
                }
            }
            poly = out;
        }
    }
    poly
}

#[allow(clippy::too_many_arguments)]
fn project_decals(
    mut commands: Commands,
    current: Res<CurrentMap>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut decal_materials: ResMut<Assets<DecalMaterial>>,
    pending: Query<(Entity, &GlobalTransform, &DecalVolume, &MeshMaterial3d<StandardMaterial>), Without<Projected>>,
    ground: Query<
        (&Mesh3d, &GlobalTransform, &Aabb),
        (Without<DecalVolume>, Without<NotShadowCaster>, Without<SkinnedMesh>),
    >,
    mut settled: Local<u32>,
    parents: Query<&ChildOf>,
    map_roots: Query<(), With<crate::map::MapRoot>>,
) {
    // Only the map's own decals are projected here, once. A skill effect's decal box
    // (a scorch under a fireball impact) must not rerun the whole projection, which
    // indexes every ground surface; it stays hidden.
    // UNVERIFIED: effect decals are not drawn yet.
    let mut map_pending = 0;
    for (e, ..) in &pending {
        if parents.iter_ancestors(e).any(|a| map_roots.contains(a)) {
            map_pending += 1;
        } else {
            commands.entity(e).insert(Projected);
        }
    }
    if map_pending == 0 {
        *settled = 0;
        return;
    }
    let (done, total) = current.progress(&asset_server);
    if !current.spawned || done < total {
        *settled = 0;
        return;
    }
    *settled += 1;
    if *settled < SETTLE_FRAMES {
        return;
    }
    *settled = 0;

    // Index the ground by world bounds.
    let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    let mut entries = Vec::new();
    for (mesh, gt, aabb) in &ground {
        let affine = gt.affine();
        let (lo, hi) = world_aabb(&affine, Vec3::from(aabb.min()), Vec3::from(aabb.max()));
        let i = entries.len() as u32;
        entries.push((mesh.0.id(), affine, lo, hi));
        for c in cells(lo, hi) {
            grid.entry(c).or_default().push(i);
        }
    }

    let mut batches: HashMap<Look, (Option<Handle<DecalMaterial>>, Option<Handle<StandardMaterial>>, Batch)> =
        HashMap::new();
    for (entity, gt, volume, material) in &pending {
        if !parents.iter_ancestors(entity).any(|a| map_roots.contains(a)) {
            continue;
        }
        commands.entity(entity).insert(Projected);
        let to_world = gt.affine();
        let to_box = to_world.inverse();
        let (lo, hi) = world_aabb(&to_world, Vec3::splat(-0.5), Vec3::splat(0.5));
        let mut seen = HashSet::new();
        let (look, scale) = match &volume.tiling {
            Some(t) => (Look::Tiled(t.color.id(), t.mask.id(), t.scale.to_bits()), Some(t.scale)),
            None => (Look::Stretched(material.0.id()), None),
        };
        let batch = &mut batches
            .entry(look)
            .or_insert_with(|| match &volume.tiling {
                Some(t) => {
                    let m = decal_materials.add(DecalMaterial {
                        base: StandardMaterial {
                            base_color_texture: Some(t.color.clone()),
                            alpha_mode: AlphaMode::Blend,
                            depth_bias: 50.0,
                            perceptual_roughness: 0.8,
                            metallic: 0.0,
                            ..default()
                        },
                        extension: DecalMask { mask: t.mask.clone() },
                    });
                    (Some(m), None, Batch::default())
                }
                None => (None, Some(material.0.clone()), Batch::default()),
            })
            .2;
        for c in cells(lo, hi) {
            for &i in grid.get(&c).into_iter().flatten() {
                if !seen.insert(i) {
                    continue;
                }
                let (mesh_id, affine, glo, ghi) = &entries[i as usize];
                if glo.cmpgt(hi).any() || ghi.cmplt(lo).any() {
                    continue;
                }
                let Some(mesh) = meshes.get(*mesh_id) else { continue };
                let Ok(VertexAttributeValues::Float32x3(pos)) = mesh.try_attribute(Mesh::ATTRIBUTE_POSITION) else {
                    continue;
                };
                let tri: Vec<usize> = match mesh.try_indices() {
                    Ok(Indices::U16(v)) => v.iter().map(|&x| x as usize).collect(),
                    Ok(Indices::U32(v)) => v.iter().map(|&x| x as usize).collect(),
                    _ => (0..pos.len()).collect(),
                };
                for t in tri.chunks_exact(3) {
                    let w = [0, 1, 2].map(|k| affine.transform_point3(Vec3::from(pos[t[k]])));
                    let tlo = w[0].min(w[1]).min(w[2]);
                    let thi = w[0].max(w[1]).max(w[2]);
                    if tlo.cmpgt(hi).any() || thi.cmplt(lo).any() {
                        continue;
                    }
                    let mut n = (w[1] - w[0]).cross(w[2] - w[0]).normalize_or_zero();
                    if n.y.abs() < MIN_UP {
                        continue;
                    }
                    if n.y < 0.0 {
                        n = -n;
                    }
                    let poly = clip_to_box(w.iter().map(|p| to_box.transform_point3(*p)).collect());
                    if poly.len() < 3 {
                        continue;
                    }
                    let base = batch.positions.len() as u32;
                    for p in &poly {
                        let wp = to_world.transform_point3(*p) + n * LIFT;
                        batch.positions.push(wp.into());
                        batch.normals.push(n.into());
                        match scale {
                            Some(k) => {
                                batch.uvs.push([wp.x * k, wp.z * k]);
                                batch.uvs_b.push([p.x + 0.5, p.z + 0.5]);
                            }
                            None => batch.uvs.push([p.x + 0.5, p.z + 0.5]),
                        }
                    }
                    // Fan, wound to face up.
                    let up = (Vec3::from(batch.positions[base as usize + 1]) - Vec3::from(batch.positions[base as usize]))
                        .cross(Vec3::from(batch.positions[base as usize + 2]) - Vec3::from(batch.positions[base as usize]))
                        .y
                        >= 0.0;
                    for k in 1..poly.len() as u32 - 1 {
                        if up {
                            batch.indices.extend([base, base + k, base + k + 1]);
                        } else {
                            batch.indices.extend([base, base + k + 1, base + k]);
                        }
                    }
                }
            }
        }
    }

    let parent = current.root;
    let mut triangles = 0;
    for (tiled, stretched, b) in batches.into_values() {
        if b.indices.is_empty() {
            continue;
        }
        triangles += b.indices.len() / 3;
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, b.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, b.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, b.uvs)
            .with_inserted_indices(Indices::U32(b.indices));
        if !b.uvs_b.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, b.uvs_b);
        }
        let mut e = commands.spawn((
            Mesh3d(meshes.add(mesh)),
            NotShadowCaster,
            Transform::IDENTITY,
            Name::new("decals"),
        ));
        match (tiled, stretched) {
            (Some(m), _) => e.insert(MeshMaterial3d(m)),
            (None, Some(m)) => e.insert(MeshMaterial3d(m)),
            _ => continue,
        };
        if let Some(p) = parent {
            e.insert(ChildOf(p));
        }
    }
    info!("decals: {} boxes projected, {} triangles", pending.iter().count(), triangles);
}
