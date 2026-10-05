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
use bevy::gltf::{Gltf, GltfMaterialName};
use bevy::prelude::*;
use serde::Deserialize;

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
            .add_systems(Update, (start_map, stream_models).chain())
            .add_observer(hide_effect_surfaces);
    }
}

/// Nebula shaders whose surfaces are not meant to be drawn as plain lit meshes.
///
/// DSO_Godot names every material `<node>_<shader>` (e.g. `static_0_1_volumefog`).
/// These three come out of glTF as lit, alpha-blended sheets that show as milky
/// white cards over the map:
/// - `particle`: the emitter's spawn surface; the particles themselves are only
///   in the `.fx.json` sidecar (DSO_Godot: "the emitter mesh itself stops being
///   drawn" once the effect is rebuilt);
/// - `refraction`: a screen-space DuDv warp of what is behind, not a colour;
/// - `volumefog`: fog cards drawn with depth-based fade;
/// - `decal`: ground decals. The exporter puts the decal's alpha mask
///   (`EmsvMap0`, e.g. `decal_patch_02_mask`) in the glTF *emissive* slot over a
///   tiled ground colour (`DiffMap0`, with a `Scale` UV factor), so in a stock
///   PBR material they draw as glowing white squares. Needs a small custom
///   material (colour at uv*Scale, alpha = mask at uv).
/// Hidden until the client has real implementations of them.
const HIDDEN_SHADER_SUFFIXES: [&str; 4] = ["_particle", "_refraction", "_volumefog", "_decal"];

fn hide_effect_surfaces(
    add: On<Add, GltfMaterialName>,
    names: Query<&GltfMaterialName>,
    mut commands: Commands,
) {
    if let Ok(name) = names.get(add.entity) {
        if HIDDEN_SHADER_SUFFIXES.iter().any(|s| name.0.ends_with(s)) {
            commands.entity(add.entity).insert(Visibility::Hidden);
        }
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
fn stream_models(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    manifests: Res<Assets<MapManifest>>,
    mut current: ResMut<CurrentMap>,
) {
    let Some(root) = current.root else { return };
    if current.spawned {
        return;
    }
    let Some(manifest) = manifests.get(&current.manifest) else { return };

    // Drop the loads whose glTF has been parsed (or failed).
    let models = &current.models;
    let still: Vec<usize> = current
        .in_flight
        .iter()
        .copied()
        .filter(|&m| {
            !matches!(
                asset_server.get_load_state(&models[m]),
                Some(LoadState::Loaded | LoadState::Failed(_))
            )
        })
        .collect();
    current.in_flight = still;

    while current.in_flight.len() < MAX_MODELS_IN_FLIGHT && current.models.len() < manifest.models.len() {
        let m = current.models.len();
        let path = model_path(&manifest.models[m]);
        let gltf: Handle<Gltf> = asset_server.load(path.clone());
        // Loading the labelled scene of the same file costs no second load.
        let scene: Handle<WorldAsset> = asset_server.load(GltfAssetLabel::Scene(0).from_asset(path));
        current.models.push(gltf);
        current.in_flight.push(m);

        let placements = &current.by_model[m];
        commands.entity(root).with_children(|parent| {
            for &i in placements {
                let inst = &manifest.instances[i];
                let rotation = Quat::from_xyzw(inst.r[0], inst.r[1], inst.r[2], inst.r[3]).normalize();
                parent.spawn((
                    WorldAssetRoot(scene.clone()),
                    Transform {
                        translation: game_to_bevy(inst.p),
                        rotation,
                        scale: Vec3::from_array(inst.s),
                    },
                ));
            }
        });
        current.instances += placements.len();
    }
    if current.models.len() == manifest.models.len() {
        current.spawned = true;
        info!("map {}: all {} models requested", manifest.map, manifest.models.len());
    }
}
