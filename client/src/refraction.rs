//! Nebula's refraction surfaces (shd:refraction: heat haze, shock waves, the
//! "blur" of many skills), drawn as the 2018 client does: the scene behind them,
//! displaced by their DuDv map. SEE: shaders/refraction.wgsl for the formula and
//! its source; crate::materials creates these materials while a model loads.
//!
//! The base StandardMaterial is made transmissive only so that bevy prepares its
//! copy of the scene (view_transmission_texture) and draws these surfaces after
//! the opaque ones; the shader replaces all of its lighting.

use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct Refraction {
    /// x: strength (pixels), y: alphaBlendFactor, zw: scroll per second.
    #[uniform(100)]
    pub params: Vec4,
    #[texture(101)]
    #[sampler(102)]
    pub dudv: Option<Handle<Image>>,
}

impl MaterialExtension for Refraction {
    fn fragment_shader() -> ShaderRef {
        "embedded://dsor_client/shaders/refraction.wgsl".into()
    }

    fn specialize(
        _: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // A veil over the scene: it hides nothing drawn after it.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

pub type RefractionMaterial = ExtendedMaterial<StandardMaterial, Refraction>;

/// The base half: transmissive (so bevy copies the scene and sorts it after the
/// opaque pass), unlit, no shadow.
pub fn base() -> StandardMaterial {
    StandardMaterial { specular_transmission: 1.0, unlit: true, ..default() }
}

pub struct RefractionPlugin;

impl Plugin for RefractionPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/refraction.wgsl");
        app.add_plugins(MaterialPlugin::<RefractionMaterial>::default());
    }
}
