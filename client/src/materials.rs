//! Nebula render states that glTF cannot carry, applied while a model loads.
//!
//! tools/fix_materials.py marks each converted material with `extras.dsor_state`:
//! - `Decal`: ground decals (stones, sand, paths). The mesh is a projection box;
//!   colour and mask are already merged into one RGBA texture, drawn
//!   alpha-blended over the ground the box covers (crate::decals).
//! - `Additive`: glows (windows, torches, magic). Added to the frame, unlit.
//! - `Hidden`: volume fog, refraction and particle emitter surfaces. Not artwork:
//!   particles are rebuilt from the `.fx.json` sidecars (crate::fx).
//!
//! Done in a glTF extension handler, so the states are part of the loaded scene:
//! nothing runs per entity once the map is up. Every surface that is not opaque
//! also stops casting shadows, as in the game's shadow pass.

use bevy::gltf::extensions::{ErasedGltfExtensionHandler, GltfExtensionHandler, GltfExtensionHandlers};
use bevy::gltf::{gltf, GltfMaterial};
use bevy::asset::LoadContext;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

/// A Nebula decal: its mesh is the unit cube the texture is projected through
/// (straight down its local Y) onto the ground inside it.
#[derive(Component, Reflect, Default)]
#[reflect(Component, Default)]
pub struct DecalVolume;

#[derive(Clone, Copy, PartialEq)]
enum State {
    Decal,
    Additive,
    Hidden,
}

fn state_of(material: &gltf::Material) -> Option<State> {
    let extras = material.extras().as_ref()?.get();
    if !extras.contains("dsor_state") {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(extras).ok()?;
    match v.get("dsor_state")?.as_str()? {
        "Decal" => Some(State::Decal),
        "Additive" => Some(State::Additive),
        "Hidden" => Some(State::Hidden),
        _ => None,
    }
}

fn dsor_label(material_label: &str) -> String {
    format!("{material_label}/dsor")
}

/// bevy_pbr's own conversion is private; these are the fields our materials use.
fn standard_material(m: &GltfMaterial) -> StandardMaterial {
    StandardMaterial {
        base_color: m.base_color,
        base_color_channel: m.base_color_channel.clone(),
        base_color_texture: m.base_color_texture.clone(),
        emissive: m.emissive,
        emissive_texture: m.emissive_texture.clone(),
        perceptual_roughness: m.perceptual_roughness,
        metallic: m.metallic,
        normal_map_texture: m.normal_map_texture.clone(),
        double_sided: m.double_sided,
        cull_mode: m.cull_mode,
        unlit: m.unlit,
        alpha_mode: m.alpha_mode,
        uv_transform: m.uv_transform,
        ..default()
    }
}

#[derive(Default, Clone)]
struct NebulaStates;

impl GltfExtensionHandler for NebulaStates {
    fn dyn_clone(&self) -> Box<dyn ErasedGltfExtensionHandler> {
        Box::new(self.clone())
    }

    fn on_material(
        &mut self,
        load_context: &mut LoadContext<'_>,
        gltf_material: &gltf::Material,
        _material: Handle<GltfMaterial>,
        material_asset: &GltfMaterial,
        material_label: &str,
    ) {
        let Some(state) = state_of(gltf_material) else { return };
        let mut m = standard_material(material_asset);
        match state {
            State::Decal => {
                m.alpha_mode = AlphaMode::Blend;
                m.depth_bias = 50.0;
            }
            State::Additive => {
                m.alpha_mode = AlphaMode::Add;
                m.unlit = true;
                m.cull_mode = None;
            }
            State::Hidden => return,
        }
        load_context.add_labeled_asset(dsor_label(material_label), m);
    }

    fn on_spawn_mesh_and_material(
        &mut self,
        load_context: &mut LoadContext<'_>,
        _primitive: &gltf::Primitive,
        _mesh: &gltf::Mesh,
        material: &gltf::Material,
        entity: &mut EntityWorldMut,
        material_label: &str,
    ) {
        let state = state_of(material);
        if state.is_some() || format!("{:?}", material.alpha_mode()) != "Opaque" {
            entity.insert(NotShadowCaster);
        }
        match state {
            Some(State::Hidden) => {
                entity.insert(Visibility::Hidden);
            }
            Some(state) => {
                if state == State::Decal {
                    // A projection box, not a surface: crate::decals draws what it covers.
                    entity.insert((DecalVolume, Visibility::Hidden));
                }
                let handle = load_context.get_label_handle::<StandardMaterial>(dsor_label(material_label));
                entity.insert(MeshMaterial3d(handle));
            }
            None => {}
        }
    }
}

pub struct MaterialsPlugin;

impl Plugin for MaterialsPlugin {
    fn build(&self, app: &mut App) {
        // Scene components must be reflected to be instanced.
        app.register_type::<DecalVolume>();
        // After bevy_pbr's own handler, so the material it set is replaced.
        let handlers = app.world().resource::<GltfExtensionHandlers>().0.clone();
        #[cfg(target_family = "wasm")]
        bevy::tasks::block_on(async { handlers.write().await.push(Box::new(NebulaStates)) });
        #[cfg(not(target_family = "wasm"))]
        handlers.write_blocking().push(Box::new(NebulaStates));
    }
}
