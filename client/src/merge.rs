//! Static map surfaces merged into a few large meshes.
//!
//! In the browser (WebGL2) every mesh entity is one draw call prepared on the CPU,
//! with no GPU-side batching: Kingshill's ~2 500 visible surfaces cost ~7 ms a
//! frame of the 17.5 a single-threaded frame took (bevy trace_chrome,
//! DSOR_LIKE_WEB). Every static surface of a culling cell sharing a material,
//! a shadow choice and a vertex layout is baked, in world space, into one mesh:
//! a few hundred draws instead.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::asset::UntypedAssetId;
use bevy::prelude::*;

/// A static surface's material: a StandardMaterial, or one of the Nebula shaders
/// drawn by a material of ours (crate::surfaces, crate::refraction).
#[derive(Clone, Debug)]
pub enum MapMaterial {
    Standard(Handle<StandardMaterial>),
    Surface(Handle<crate::surfaces::NebulaMaterial>),
    Refraction(Handle<crate::refraction::RefractionMaterial>),
}

impl Default for MapMaterial {
    fn default() -> Self {
        Self::Standard(Handle::default())
    }
}

impl MapMaterial {
    pub fn id(&self) -> UntypedAssetId {
        match self {
            Self::Standard(h) => h.id().untyped(),
            Self::Surface(h) => h.id().untyped(),
            Self::Refraction(h) => h.id().untyped(),
        }
    }

    pub fn insert(&self, e: &mut EntityCommands) {
        match self {
            Self::Standard(h) => e.insert(MeshMaterial3d(h.clone())),
            Self::Surface(h) => e.insert(MeshMaterial3d(h.clone())),
            Self::Refraction(h) => e.insert(MeshMaterial3d(h.clone())),
        };
    }
}

/// The attributes a merged mesh carries, as bits: 1 normal, 2 uv0, 4 uv1, 8 tangent,
/// 16 colour. Surfaces are merged only with others of the same layout.
pub type Layout = u8;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BatchKey {
    pub cell: (i32, i32),
    pub material: UntypedAssetId,
    pub casts_shadow: bool,
    pub layout: Layout,
}

#[derive(Default)]
pub struct Batch {
    pub material: MapMaterial,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uv0: Vec<[f32; 2]>,
    uv1: Vec<[f32; 2]>,
    tangents: Vec<[f32; 4]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

/// A merged mesh is split before it would pass this many vertices.
const MAX_VERTICES: usize = 1 << 20;

pub fn layout_of(mesh: &Mesh) -> Option<Layout> {
    mesh.try_attribute(Mesh::ATTRIBUTE_POSITION).ok()?;
    let has = |a| mesh.try_attribute(a).is_ok();
    Some(
        (has(Mesh::ATTRIBUTE_NORMAL) as u8)
            | (has(Mesh::ATTRIBUTE_UV_0) as u8) << 1
            | (has(Mesh::ATTRIBUTE_UV_1) as u8) << 2
            | (has(Mesh::ATTRIBUTE_TANGENT) as u8) << 3
            | (has(Mesh::ATTRIBUTE_COLOR) as u8) << 4,
    )
}

fn f3(v: &VertexAttributeValues) -> Option<&Vec<[f32; 3]>> {
    if let VertexAttributeValues::Float32x3(x) = v { Some(x) } else { None }
}
fn f2(v: &VertexAttributeValues) -> Option<&Vec<[f32; 2]>> {
    if let VertexAttributeValues::Float32x2(x) = v { Some(x) } else { None }
}
fn f4(v: &VertexAttributeValues) -> Option<&Vec<[f32; 4]>> {
    if let VertexAttributeValues::Float32x4(x) = v { Some(x) } else { None }
}

impl Batch {
    pub fn is_full(&self) -> bool {
        self.positions.len() >= MAX_VERTICES
    }

    /// Append `mesh` placed by `to_world`. False if its attributes are not the
    /// plain float layouts merging expects (the caller then spawns it alone).
    pub fn push(&mut self, mesh: &Mesh, to_world: &bevy::math::Affine3A, layout: Layout) -> bool {
        let Ok(pos) = mesh.try_attribute(Mesh::ATTRIBUTE_POSITION) else { return false };
        let Some(pos) = f3(pos) else { return false };
        let get3 = |a| mesh.try_attribute(a).ok().and_then(f3);
        let get2 = |a| mesh.try_attribute(a).ok().and_then(f2);
        let get4 = |a| mesh.try_attribute(a).ok().and_then(f4);
        let normals = if layout & 1 != 0 { Some(get3(Mesh::ATTRIBUTE_NORMAL)) } else { None };
        let uv0 = if layout & 2 != 0 { Some(get2(Mesh::ATTRIBUTE_UV_0)) } else { None };
        let uv1 = if layout & 4 != 0 { Some(get2(Mesh::ATTRIBUTE_UV_1)) } else { None };
        let tangents = if layout & 8 != 0 { Some(get4(Mesh::ATTRIBUTE_TANGENT)) } else { None };
        let colors = if layout & 16 != 0 { Some(get4(Mesh::ATTRIBUTE_COLOR)) } else { None };
        if [normals.map(|x| x.is_none()), uv0.map(|x| x.is_none()), uv1.map(|x| x.is_none()), tangents.map(|x| x.is_none()), colors.map(|x| x.is_none())]
            .into_iter()
            .any(|missing| missing == Some(true))
        {
            return false;
        }
        let base = self.positions.len() as u32;
        let m3 = to_world.matrix3;
        let normal_m = m3.inverse().transpose();
        // A mirroring placement turns triangles inside out: their winding is
        // reversed so the merged surface faces the way the original did.
        let mirrored = m3.determinant() < 0.0;
        self.positions.extend(pos.iter().map(|p| to_world.transform_point3(Vec3::from(*p)).to_array()));
        if let Some(Some(n)) = normals {
            self.normals.extend(n.iter().map(|n| (normal_m * Vec3A::from(Vec3::from(*n))).normalize_or_zero().to_array()));
        }
        if let Some(Some(u)) = uv0 {
            self.uv0.extend_from_slice(u);
        }
        if let Some(Some(u)) = uv1 {
            self.uv1.extend_from_slice(u);
        }
        if let Some(Some(t)) = tangents {
            self.tangents.extend(t.iter().map(|t| {
                let v = (m3 * Vec3A::new(t[0], t[1], t[2])).normalize_or_zero();
                [v.x, v.y, v.z, if mirrored { -t[3] } else { t[3] }]
            }));
        }
        if let Some(Some(c)) = colors {
            self.colors.extend_from_slice(c);
        }
        let tri: Vec<u32> = match mesh.try_indices() {
            Ok(Indices::U16(v)) => v.iter().map(|&i| i as u32).collect(),
            Ok(Indices::U32(v)) => v.clone(),
            _ => (0..pos.len() as u32).collect(),
        };
        for t in tri.chunks_exact(3) {
            if mirrored {
                self.indices.extend([base + t[0], base + t[2], base + t[1]]);
            } else {
                self.indices.extend([base + t[0], base + t[1], base + t[2]]);
            }
        }
        true
    }

    pub fn build(self) -> Option<Mesh> {
        if self.indices.is_empty() {
            return None;
        }
        // Kept on the CPU too: the ground decals are projected onto these meshes.
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_indices(Indices::U32(self.indices));
        if !self.normals.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        }
        if !self.uv0.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv0);
        }
        if !self.uv1.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, self.uv1);
        }
        if !self.tangents.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, self.tangents);
        }
        if !self.colors.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        }
        Some(mesh)
    }
}

/// Batches being filled, by key; a full batch moves to `ready`.
#[derive(Default)]
pub struct Batches {
    pub open: HashMap<BatchKey, Batch>,
    pub ready: Vec<(BatchKey, Batch)>,
}

impl Batches {
    pub fn batch(&mut self, key: BatchKey, material: &MapMaterial) -> &mut Batch {
        if self.open.get(&key).is_some_and(|b| b.is_full()) {
            let full = self.open.remove(&key).unwrap();
            self.ready.push((key.clone(), full));
        }
        self.open.entry(key).or_insert_with(|| Batch { material: material.clone(), ..default() })
    }
}
