//! Map placement manifests (`maps/<name>.map.json`, written by DSO_Godot's
//! `export_maps.py`) and the system that spawns one as a Bevy scene.
//!
//! The manifest is loaded through the `AssetServer`, never `std::fs`, so the same
//! code reads it from disk natively and over HTTP in the browser.
//!
//! Performance contract: every distinct model of the map is loaded ONCE
//! (`models[i]` -> one `Handle<WorldAsset>`), and every placement that uses it
//! spawns that same handle. The glTF scene therefore shares one `Handle<Mesh>`
//! and one `Handle<StandardMaterial>` per primitive across all of its instances,
//! which is what lets Bevy batch them, and no placement triggers a load of its own.
//! Frustum culling is Bevy's default (every mesh gets an `Aabb` and is tested
//! against the camera frustum); nothing here disables it.

use bevy::asset::{io::Reader, AssetLoader, LoadContext, LoadState, RecursiveDependencyLoadState};
use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::camera::primitives::MeshAabb;
use bevy::prelude::*;
use serde::Deserialize;

use crate::merge::MapMaterial;

/// Game map position -> Bevy world position.
///
/// The game's map frame is the one the `.map` placements are written in, which is
/// also the server's "description frame" (`dsor/mapdata.py`: positions are
/// `(x, elevation, y)`): Nebula3's right-handed, Y-up frame in metres, with the
/// two ground axes on X and Z and the elevation on Y. glTF and Bevy are both
/// right-handed and Y-up in metres, and DSO_Godot writes the `.glb` models in
/// that frame unconverted (its docs: "Nebula3 and glTF are both right-handed Y-up,
/// so nothing is converted"). The mapping is therefore the identity; keep every
/// game -> Bevy conversion going through this one function so a correction, if
/// one is ever proven, lands in one place.
///
/// The same holds for rotations (quaternions are used as-is) and scales.
#[inline]
pub fn game_to_bevy(pos: [f32; 3]) -> Vec3 {
    Vec3::new(pos[0], pos[1], pos[2])
}

/// One placement: `models[m]` at position `p`, rotation `r` (quaternion x,y,z,w),
/// scale `s`.
#[derive(Debug, Deserialize)]
pub struct Placement {
    pub m: usize,
    pub p: [f32; 3],
    pub r: [f32; 4],
    pub s: [f32; 3],
}

/// A whole `.map.json` manifest. Fields the renderer does not use yet (groups,
/// nav blockers) are skipped by serde.
#[derive(Asset, TypePath, Debug, Deserialize)]
pub struct MapManifest {
    pub map: String,
    /// The map's bounding box centre and half-extents, game frame.
    pub center: [f32; 3],
    pub extents: [f32; 3],
    /// Model references, relative to the asset root without extension
    /// (`t001_hub/tile_wall_corner_02` -> `t001_hub/tile_wall_corner_02.glb`).
    pub models: Vec<String>,
    pub instances: Vec<Placement>,
}

#[derive(Default, TypePath)]
pub struct MapManifestLoader;

impl AssetLoader for MapManifestLoader {
    type Asset = MapManifest;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _ctx: &mut LoadContext<'_>,
    ) -> Result<MapManifest, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)
    }

    fn extensions(&self) -> &[&str] {
        &["map.json"]
    }
}

/// How many model files may be loading at once.
///
/// CONTRACT: keep this bounded. Bevy 0.19's glTF loader waits on a
/// `TaskPool::scope` for its images, and while it waits that thread runs other
/// queued load tasks *nested on the same stack*. Requesting all of a map's
/// models at once (433 for Kingshill) nested ~90 glTF loads deep and overflowed
/// the 2 MB IO-thread stack ("thread 'IO Task Pool (0)' has overflowed its
/// stack", gdb: 92 nested `GltfLoader::load_gltf` frames). The nesting depth is
/// bounded by the number of loads in flight, so streaming the models in keeps it
/// shallow, natively and in the single-threaded browser build alike.
pub const MAX_MODELS_IN_FLIGHT: usize = 24;

/// Which map to show, and its loading progress.
#[derive(Resource)]
pub struct CurrentMap {
    pub name: String,
    pub manifest: Handle<MapManifest>,
    /// One glTF handle per distinct model requested so far (index = manifest
    /// model index). Models are requested in order, so `models.len()` is also
    /// the number requested.
    pub models: Vec<Handle<Gltf>>,
    /// Requested models whose file has not finished parsing yet.
    pub in_flight: Vec<usize>,
    /// Placements per model, built once the manifest arrives.
    pub by_model: Vec<Vec<usize>>,
    pub root: Option<Entity>,
    /// Culling cells (CELL units square) by grid key; every placement is a child
    /// of its cell. SEE: cull_cells.
    pub cells: std::collections::HashMap<(i32, i32), Entity>,
    /// Static surfaces waiting to be merged (crate::merge), baked once every
    /// model is placed.
    pub batches: crate::merge::Batches,
    /// Materials by what they look like (textures, colours, states): the many
    /// identical materials of different models share one, so their surfaces merge.
    pub canonical: std::collections::HashMap<String, Handle<StandardMaterial>>,
    /// The same for the Nebula surfaces and refractions (crate::surfaces,
    /// crate::refraction): one per look across all the map's models.
    pub canonical_surfaces: std::collections::HashMap<String, Handle<crate::surfaces::NebulaMaterial>>,
    pub canonical_refractions: std::collections::HashMap<String, Handle<crate::refraction::RefractionMaterial>>,
    /// True once every model has been requested and every placement spawned.
    pub spawned: bool,
    pub instances: usize,
}

impl CurrentMap {
    pub fn new(name: String, manifest: Handle<MapManifest>) -> Self {
        Self {
            name,
            manifest,
            models: Vec::new(),
            in_flight: Vec::new(),
            by_model: Vec::new(),
            root: None,
            cells: Default::default(),
            batches: Default::default(),
            canonical: Default::default(),
            canonical_surfaces: Default::default(),
            canonical_refractions: Default::default(),
            spawned: false,
            instances: 0,
        }
    }

    /// (models fully loaded including textures, or failed; models in the map)
    pub fn progress(&self, asset_server: &AssetServer) -> (usize, usize) {
        let done = self
            .models
            .iter()
            .filter(|h| {
                matches!(
                    asset_server.get_recursive_dependency_load_state(*h),
                    Some(RecursiveDependencyLoadState::Loaded | RecursiveDependencyLoadState::Failed(_))
                )
            })
            .count();
        (done, self.by_model.len())
    }
}

/// Root entity of the spawned map; every placement is its child.
#[derive(Component)]
pub struct MapRoot;

/// Asset path of a model reference from a manifest.
pub fn model_path(reference: &str) -> String {
    // Older manifests may keep a resource prefix such as `mdl:`.
    let rel = reference.rsplit(':').next().unwrap_or(reference);
    format!("{rel}.glb")
}

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<MapManifest>()
            .init_asset_loader::<MapManifestLoader>()
            .add_systems(Update, (start_map, stream_models, cull_cells).chain().run_if(resource_exists::<CurrentMap>));
    }
}

/// Once the manifest is in: create the map root, index placements by model and
/// frame the camera on the map.
fn start_map(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    manifests: Res<Assets<MapManifest>>,
    mut current: ResMut<CurrentMap>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
) {
    if current.root.is_some() || current.spawned {
        return;
    }
    let Some(manifest) = manifests.get(&current.manifest) else {
        if let Some(LoadState::Failed(err)) = asset_server.get_load_state(&current.manifest) {
            error!("map manifest {} failed to load: {err}", current.name);
            current.spawned = true;
        }
        return;
    };

    let mut by_model = vec![Vec::new(); manifest.models.len()];
    for (i, inst) in manifest.instances.iter().enumerate() {
        if let Some(list) = by_model.get_mut(inst.m) {
            list.push(i);
        }
    }
    current.by_model = by_model;
    current.root = Some(
        commands
            .spawn((MapRoot, Name::new(manifest.map.clone()), Transform::default(), Visibility::default()))
            .id(),
    );

    // Frame the map: look down on its centre from the south, roughly the game's
    // own three-quarter view, far enough out to see most of it.
    let center = game_to_bevy(manifest.center);
    let reach = Vec3::from_array(manifest.extents).xz().length().clamp(20.0, 110.0);
    for mut t in &mut cameras {
        if t.translation == Vec3::ZERO {
            *t = Transform::from_translation(center + Vec3::new(0.0, reach * 0.55, reach * 0.55))
                .looking_at(center, Vec3::Y);
        }
    }
    info!(
        "map {}: {} placements of {} distinct models",
        manifest.map,
        manifest.instances.len(),
        manifest.models.len()
    );
}

/// Request the map's models a few at a time and spawn each model's placements
/// as soon as it is requested (a `WorldAssetRoot` whose scene is still loading
/// simply spawns its children when the scene arrives).
/// Map models stream in, MAX_MODELS_IN_FLIGHT at a time; each one's placements
/// are spawned once its glTF is parsed.
///
/// CONTRACT: a static model (no animation, skin, particle emitter or decal) is
///   spawned FLAT: one entity per visible surface, its node chain folded into its
///   transform, under MapRoot. As a scene it cost every node of its hierarchy
///   too: Kingshill was 37 534 node entities for 16 311 visible surfaces, and the
///   browser (one thread) paid for each entity every frame (20-30 FPS).
///   Effect and helper surfaces (dsor_state Hidden) are not spawned at all.
/// SEE: crate::materials for the states a scene gets from its extension handler,
///   which flat surfaces replicate here.
#[allow(clippy::too_many_arguments)]
fn stream_models(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    manifests: Res<Assets<MapManifest>>,
    mut current: ResMut<CurrentMap>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    meshes: Res<Assets<GltfMesh>>,
    materials: Res<Assets<GltfMaterial>>,
    mut mesh_assets_mut: ResMut<Assets<Mesh>>,
    std_materials: Res<Assets<StandardMaterial>>,
    surface_materials: Res<Assets<crate::surfaces::NebulaMaterial>>,
    refraction_materials: Res<Assets<crate::refraction::RefractionMaterial>>,
) {
    let mesh_assets = &*mesh_assets_mut;
    let Some(root) = current.root else { return };
    if current.spawned {
        return;
    }
    let Some(manifest) = manifests.get(&current.manifest) else { return };

    // Place the models whose glTF has been parsed; forget the failed ones.
    let mut still = Vec::new();
    for m in std::mem::take(&mut current.in_flight) {
        match asset_server.get_load_state(&current.models[m]) {
            Some(LoadState::Loaded) => {
                let Some(gltf) = gltfs.get(&current.models[m]) else {
                    still.push(m);
                    continue;
                };
                let placements = current.by_model[m].clone();
                let flat = flat_surfaces(gltf, &nodes, &meshes, &materials, mesh_assets, &asset_server);
                for &i in &placements {
                    let inst = &manifest.instances[i];
                    let at = Transform {
                        translation: game_to_bevy(inst.p),
                        rotation: Quat::from_xyzw(inst.r[0], inst.r[1], inst.r[2], inst.r[3]).normalize(),
                        scale: Vec3::from_array(inst.s),
                    };
                    let key = cell_of(at.translation);
                    let cell = *current.cells.entry(key).or_insert_with(|| {
                        commands
                            .spawn((Name::new(format!("cell {key:?}")), Transform::default(), Visibility::default(), ChildOf(root)))
                            .id()
                    });
                    match &flat {
                        Some(surfaces) => {
                            let inst_affine = at.compute_affine();
                            for s in surfaces {
                                let world = inst_affine * s.local;
                                let size = s.size * at.scale.abs().max_element();
                                let casts_shadow = !(s.no_shadow || size < MIN_SHADOW_CASTER);
                                // Merged with its cell's like surfaces (crate::merge);
                                // alone when its layout cannot be.
                                let material = match &s.material {
                                    MapMaterial::Standard(h) => match std_materials.get(h) {
                                        Some(m) => {
                                            let sig = material_signature(m);
                                            MapMaterial::Standard(current.canonical.entry(sig).or_insert_with(|| h.clone()).clone())
                                        }
                                        None => s.material.clone(),
                                    },
                                    MapMaterial::Surface(h) => match surface_materials.get(h) {
                                        Some(m) => {
                                            let e = &m.extension;
                                            let t = |h: &Option<Handle<Image>>| h.as_ref().map(|h| format!("{:?}", h.id())).unwrap_or_default();
                                            let sig = format!("{}|{:?}|{}|{}|{}", material_signature(&m.base), e.params, t(&e.cube), t(&e.layer), t(&e.mask));
                                            MapMaterial::Surface(current.canonical_surfaces.entry(sig).or_insert_with(|| h.clone()).clone())
                                        }
                                        None => s.material.clone(),
                                    },
                                    MapMaterial::Refraction(h) => match refraction_materials.get(h) {
                                        Some(m) => {
                                            let dudv = m.extension.dudv.as_ref().map(|h| format!("{:?}", h.id())).unwrap_or_default();
                                            let sig = format!("{}|{:?}|{dudv}", material_signature(&m.base), m.extension.params);
                                            MapMaterial::Refraction(current.canonical_refractions.entry(sig).or_insert_with(|| h.clone()).clone())
                                        }
                                        None => s.material.clone(),
                                    },
                                };
                                let merged = mesh_assets.get(&s.mesh).and_then(|mesh| {
                                    let layout = crate::merge::layout_of(mesh)?;
                                    let key = crate::merge::BatchKey { cell: key, material: material.id(), casts_shadow, layout };
                                    current.batches.batch(key, &material).push(mesh, &world, layout).then_some(())
                                });
                                if merged.is_none() {
                                    let tf = Transform::from_matrix(Mat4::from(world));
                                    let mut e = commands.spawn((Mesh3d(s.mesh.clone()), tf, ChildOf(cell)));
                                    s.material.insert(&mut e);
                                    if !casts_shadow {
                                        e.insert(bevy::light::NotShadowCaster);
                                    }
                                }
                            }
                        }
                        None => {
                            let scene = gltf.scenes.first().cloned().unwrap_or_default();
                            commands.spawn((WorldAssetRoot(scene), at, ChildOf(cell)));
                        }
                    }
                }
                current.instances += placements.len();
            }
            Some(LoadState::Failed(_)) => {}
            _ => still.push(m),
        }
    }
    current.in_flight = still;

    while current.in_flight.len() < MAX_MODELS_IN_FLIGHT && current.models.len() < manifest.models.len() {
        let m = current.models.len();
        let gltf: Handle<Gltf> = asset_server.load(model_path(&manifest.models[m]));
        current.models.push(gltf);
        current.in_flight.push(m);
    }
    if current.models.len() == manifest.models.len() && current.in_flight.is_empty() {
        // Bake the merged surfaces into their cells.
        let batches = std::mem::take(&mut current.batches);
        let mut draws = 0;
        for (key, batch) in batches.open.into_iter().chain(batches.ready) {
            let Some(&cell) = current.cells.get(&key.cell) else { continue };
            let material = batch.material.clone();
            let Some(mesh) = batch.build() else { continue };
            let mut e = commands.spawn((Mesh3d(mesh_assets_mut.add(mesh)), Transform::IDENTITY, ChildOf(cell)));
            material.insert(&mut e);
            if !key.casts_shadow {
                e.insert(bevy::light::NotShadowCaster);
            }
            draws += 1;
        }
        info!("map {}: static surfaces merged into {draws} meshes", manifest.map);
        current.spawned = true;
        info!("map {}: all {} models placed", manifest.map, manifest.models.len());
    }
}

/// Map surfaces smaller than this (largest extent, world units) cast no shadow.
/// 0 since the static surfaces are merged (crate::merge): every object casts one
/// again, as in the game, for a few hundred shadow draws.
/// Before the merge:
/// the shadow pass drew ~1 900 surfaces in Kingshill, 5 ms a frame in the
/// browser, mostly barrels, crates and clutter whose shadows hardly show.
const MIN_SHADOW_CASTER: f32 = 0.0;

/// Culling cell size, world units.
pub const CELL: f32 = 24.0;
/// Cells farther than this from the point the camera looks at are hidden, from
/// the view and from the shadow pass alike. The game camera at full zoom-out sees
/// about 45 units around that point.
pub const CULL_DISTANCE: f32 = 35.0;

fn cell_of(p: Vec3) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32)
}

/// Where the camera looks: the local player when there is one, else where the
/// camera's axis meets the ground plane of the map (or a point ahead of it).
pub fn camera_focus(cam: &GlobalTransform, player: Option<Vec3>, ground: f32) -> Vec3 {
    if let Some(p) = player {
        return p;
    }
    let (o, d) = (cam.translation(), cam.forward().as_vec3());
    if d.y < -0.05 {
        let t = (ground - o.y) / d.y;
        if t > 0.0 {
            return o + d * t.min(200.0);
        }
    }
    o + d * 30.0
}

/// Culling: hide whole cells away from the camera's focus.
/// CONTRACT: hidden for every view, the shadow cascade included -- frustum culling
///   alone left the shadow pass ~12 700 surfaces in Kingshill (the camera drew
///   1 840), each prepared on the CPU every frame: 30 FPS in the browser.
pub fn cull_cells(
    current: Res<CurrentMap>,
    manifests: Res<Assets<MapManifest>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    players: Query<&GlobalTransform, With<crate::net::LocalPlayer>>,
    mut cells: Query<&mut Visibility>,
) {
    let Ok(cam) = cameras.single() else { return };
    let ground = manifests.get(&current.manifest).map(|m| m.center[1]).unwrap_or(0.0);
    let focus = camera_focus(cam, players.iter().next().map(|p| p.translation()), ground);
    for (&(x, z), &e) in &current.cells {
        let min = Vec2::new(x as f32 * CELL, z as f32 * CELL);
        let nearest = Vec2::new(focus.x, focus.z).clamp(min, min + Vec2::splat(CELL));
        let near = nearest.distance(Vec2::new(focus.x, focus.z)) <= CULL_DISTANCE;
        let Ok(mut v) = cells.get_mut(e) else { continue };
        let wanted = if near { Visibility::Inherited } else { Visibility::Hidden };
        if *v != wanted {
            *v = wanted;
        }
    }
}

/// What a material looks like, as a key: two materials with the same signature
/// draw the same.
fn material_signature(m: &StandardMaterial) -> String {
    let t = |h: &Option<Handle<Image>>| h.as_ref().map(|h| format!("{:?}", h.id())).unwrap_or_default();
    let c = m.base_color.to_linear();
    format!(
        "{}|{}|{}|{}|{}|{:?}|{}|{}|{:?}|{:.3},{:.3},{:.3},{:.3}|{:.3},{:.3},{:.3}|{:.3}|{:.3}|{:?}|{:?}",
        t(&m.base_color_texture),
        t(&m.normal_map_texture),
        t(&m.metallic_roughness_texture),
        t(&m.emissive_texture),
        t(&m.occlusion_texture),
        m.alpha_mode,
        m.unlit,
        m.double_sided,
        m.cull_mode,
        c.red, c.green, c.blue, c.alpha,
        m.emissive.red, m.emissive.green, m.emissive.blue,
        m.perceptual_roughness,
        m.metallic,
        m.uv_transform,
        m.depth_bias,
    )
}

/// One visible surface of a static model, in the model's space.
struct FlatSurface {
    mesh: Handle<Mesh>,
    material: MapMaterial,
    local: bevy::math::Affine3A,
    no_shadow: bool,
    /// Largest extent of the surface in the model's space (its mesh bounds
    /// through `local`).
    size: f32,
}

/// The visible surfaces of a model that can be spawned flat, or None when it needs
/// its scene (animations, skins, emitters, decal boxes).
fn flat_surfaces(
    gltf: &Gltf,
    nodes: &Assets<GltfNode>,
    meshes: &Assets<GltfMesh>,
    materials: &Assets<GltfMaterial>,
    mesh_assets: &Assets<Mesh>,
    asset_server: &AssetServer,
) -> Option<Vec<FlatSurface>> {
    if !gltf.animations.is_empty() || !gltf.skins.is_empty() {
        return None;
    }
    let all: Vec<&GltfNode> = gltf.nodes.iter().map(|h| nodes.get(h)).collect::<Option<_>>()?;
    if all.iter().any(|n| n.extras.as_ref().is_some_and(|e| e.value.contains("dsor_emitter"))) {
        return None;
    }
    let children: std::collections::HashSet<usize> =
        all.iter().flat_map(|n| n.children.iter().filter_map(|c| nodes.get(c).map(|c| c.index))).collect();
    let mut out = Vec::new();
    let mut stack: Vec<(usize, bevy::math::Affine3A)> = all
        .iter()
        .filter(|n| !children.contains(&n.index))
        .map(|n| (n.index, bevy::math::Affine3A::IDENTITY))
        .collect();
    let by_index: std::collections::HashMap<usize, &GltfNode> = all.iter().map(|n| (n.index, *n)).collect();
    while let Some((i, parent)) = stack.pop() {
        let node = by_index.get(&i)?;
        let local = parent * node.transform.compute_affine();
        for c in &node.children {
            stack.push((nodes.get(c)?.index, local));
        }
        let Some(mesh) = node.mesh.as_ref().and_then(|h| meshes.get(h)) else { continue };
        for prim in &mesh.primitives {
            let state = prim.material_extras.as_ref().map(|e| e.value.as_str()).unwrap_or("");
            // Nebula shaders of ours come first: refraction and volume fog are
            // marked Hidden for the scene path's StandardMaterial.
            let ours = if state.contains("shd:refraction") {
                Some("refr")
            } else {
                crate::surfaces::kind_of(state).map(|_| "neb")
            };
            if ours.is_none() && state.contains("\"dsor_state\":\"Hidden\"") {
                continue;
            }
            if state.contains("\"dsor_state\":\"Decal\"") {
                return None;
            }
            let additive = state.contains("\"dsor_state\":\"Additive\"");
            let Some(gm) = prim.material.as_ref() else { return None };
            let Some(path) = gm.path() else { return None };
            let label = path.label()?.to_owned();
            // Mirroring is undone in the merged mesh itself (crate::merge reverses
            // its winding), so the plain material, never the "(inverted)" one.
            let label = label.trim_end_matches(" (inverted)").to_owned();
            // Ours (crate::materials) for glows, unlit surfaces, surfaces with a
            // static alpha factor, and lit surfaces with an emissive map, whose
            // emission is scaled to the node's intensity.
            let emissive = materials.get(gm).is_some_and(|m| m.emissive_texture.is_some());
            let adjusted = state.contains("\"dsor_unlit\":true") || state.contains("\"dsor_alpha\"");
            let material = match ours {
                Some("refr") => MapMaterial::Refraction(asset_server.load(path.clone().with_label(format!("{label}/refr")))),
                Some(_) => MapMaterial::Surface(asset_server.load(path.clone().with_label(format!("{label}/neb")))),
                None => {
                    let suffix = if additive || emissive || adjusted { "dsor" } else { "std" };
                    MapMaterial::Standard(asset_server.load(path.clone().with_label(format!("{label}/{suffix}"))))
                }
            };
            let opaque = materials.get(gm).is_some_and(|m| matches!(m.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)));
            let size = mesh_assets
                .get(&prim.mesh)
                .and_then(|m| m.compute_aabb())
                .map(|b| (local.matrix3 * Vec3A::from(b.half_extents * 2.0)).abs().max_element())
                .unwrap_or(f32::MAX);
            out.push(FlatSurface { mesh: prim.mesh.clone(), material, local, no_shadow: additive || !opaque, size });
        }
    }
    Some(out)
}
