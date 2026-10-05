//! The map's walkable ground: PathEngine's navigation mesh, the one the 2018 client
//! walks on and the server spawns on (tools/export_navmesh.py writes
//! `maps/<map>.nav.bin` from `navigation/<map>/<map>.tok`).
//!
//! It gives the player their height (the server's elevations are approximate: the
//! Kingshill arrival is 1.2 units under the ground it stands on) and their collisions:
//! a step that would leave the mesh is not taken.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::prelude::*;

/// Grid cell size of the lookup index, world units.
const BUCKET: f32 = 4.0;

#[derive(Asset, TypePath, Debug)]
pub struct NavMesh {
    pub triangles: Vec<[Vec3; 3]>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

fn cell(v: f32) -> i32 {
    (v / BUCKET).floor() as i32
}

impl NavMesh {
    pub fn new(triangles: Vec<[Vec3; 3]>) -> Self {
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, t) in triangles.iter().enumerate() {
            let (min_x, max_x) = (t[0].x.min(t[1].x).min(t[2].x), t[0].x.max(t[1].x).max(t[2].x));
            let (min_z, max_z) = (t[0].z.min(t[1].z).min(t[2].z), t[0].z.max(t[1].z).max(t[2].z));
            for cx in cell(min_x)..=cell(max_x) {
                for cz in cell(min_z)..=cell(max_z) {
                    grid.entry((cx, cz)).or_default().push(i as u32);
                }
            }
        }
        Self { triangles, grid }
    }

    /// Every ground height under (x, z), from every overlapping level.
    pub fn heights(&self, x: f32, z: f32) -> impl Iterator<Item = f32> + '_ {
        self.grid
            .get(&(cell(x), cell(z)))
            .into_iter()
            .flatten()
            .filter_map(move |&i| height_in(&self.triangles[i as usize], x, z))
    }

    /// The ground at (x, z) on the level nearest `near` and within `max_step` of it.
    pub fn ground(&self, x: f32, z: f32, near: f32, max_step: f32) -> Option<f32> {
        self.heights(x, z)
            .filter(|h| (h - near).abs() <= max_step)
            .min_by(|a, b| (a - near).abs().total_cmp(&(b - near).abs()))
    }

    /// The nearest point on the mesh to `p` (horizontally) within `radius`.
    pub fn nearest(&self, p: Vec3, radius: f32) -> Option<Vec3> {
        if let Some(h) = self.ground(p.x, p.z, p.y, f32::MAX) {
            return Some(Vec3::new(p.x, h, p.z));
        }
        let mut best: Option<(f32, Vec3)> = None;
        for cx in cell(p.x - radius)..=cell(p.x + radius) {
            for cz in cell(p.z - radius)..=cell(p.z + radius) {
                for &i in self.grid.get(&(cx, cz)).into_iter().flatten() {
                    let t = &self.triangles[i as usize];
                    for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                        let q = closest_on_segment(a, b, p);
                        let d = Vec2::new(q.x - p.x, q.z - p.z).length_squared() + (q.y - p.y).abs() * 0.01;
                        if d <= radius * radius && best.is_none_or(|(bd, _)| d < bd) {
                            best = Some((d, q));
                        }
                    }
                }
            }
        }
        best.map(|(_, q)| q)
    }

    /// Where a ray first meets the ground.
    pub fn raycast(&self, ray: Ray3d) -> Option<Vec3> {
        let mut best: Option<f32> = None;
        for t in &self.triangles {
            if let Some(d) = ray_triangle(ray, t) {
                if best.is_none_or(|b| d < b) {
                    best = Some(d);
                }
            }
        }
        best.map(|d| ray.get_point(d))
    }
}

fn closest_on_segment(a: Vec3, b: Vec3, p: Vec3) -> Vec3 {
    let ab = Vec2::new(b.x - a.x, b.z - a.z);
    let len = ab.length_squared();
    let t = if len == 0.0 { 0.0 } else { (Vec2::new(p.x - a.x, p.z - a.z).dot(ab) / len).clamp(0.0, 1.0) };
    a + (b - a) * t
}

/// The height of triangle `t` at (x, z), when (x, z) is inside it.
fn height_in(t: &[Vec3; 3], x: f32, z: f32) -> Option<f32> {
    let (a, b, c) = (t[0], t[1], t[2]);
    let d = (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z);
    if d.abs() < 1e-9 {
        return None;
    }
    let w1 = ((b.z - c.z) * (x - c.x) + (c.x - b.x) * (z - c.z)) / d;
    let w2 = ((c.z - a.z) * (x - c.x) + (a.x - c.x) * (z - c.z)) / d;
    let w3 = 1.0 - w1 - w2;
    const E: f32 = -1e-4;
    (w1 >= E && w2 >= E && w3 >= E).then(|| w1 * a.y + w2 * b.y + w3 * c.y)
}

/// Möller-Trumbore, both faces.
fn ray_triangle(ray: Ray3d, t: &[Vec3; 3]) -> Option<f32> {
    let dir: Vec3 = *ray.direction;
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv = 1.0 / det;
    let s = ray.origin - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let d = e2.dot(q) * inv;
    (d > 0.0).then_some(d)
}

#[derive(Default, TypePath)]
pub struct NavMeshLoader;

impl AssetLoader for NavMeshLoader {
    type Asset = NavMesh;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<NavMesh, Self::Error> {
        let mut b = Vec::new();
        r.read_to_end(&mut b).await?;
        if b.len() < 4 {
            return Err(std::io::Error::other("navigation mesh too short"));
        }
        let n = u32::from_le_bytes(b[0..4].try_into().unwrap()) as usize;
        if b.len() < 4 + n * 36 {
            return Err(std::io::Error::other("navigation mesh truncated"));
        }
        let f = |at: usize| f32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        let triangles = (0..n)
            .map(|i| {
                let at = 4 + i * 36;
                [0, 1, 2].map(|k| Vec3::new(f(at + k * 12), f(at + k * 12 + 4), f(at + k * 12 + 8)))
            })
            .collect();
        Ok(NavMesh::new(triangles))
    }
    fn extensions(&self) -> &[&str] {
        &["nav.bin"]
    }
}

/// The current map's mesh, once the map is known.
#[derive(Resource)]
pub struct CurrentNav(pub Handle<NavMesh>);

pub struct NavPlugin;

impl Plugin for NavPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<NavMesh>()
            .register_asset_loader(NavMeshLoader)
            .add_systems(Update, follow_map.run_if(resource_exists::<crate::map::CurrentMap>));
    }
}

fn follow_map(
    mut commands: Commands,
    current: Res<crate::map::CurrentMap>,
    nav: Option<Res<CurrentNav>>,
    assets: Res<AssetServer>,
    mut loaded_for: Local<String>,
) {
    if *loaded_for == current.name && nav.is_some() {
        return;
    }
    *loaded_for = current.name.clone();
    commands.insert_resource(CurrentNav(assets.load(format!("maps/{}.nav.bin", current.name))));
}
