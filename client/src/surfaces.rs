//! Nebula surface shaders a StandardMaterial cannot express, as one material
//! extension (shaders/surface.wgsl) with a kind per 2018 shader:
//!
//! - `Environment` (shd:environment, 3 415 surfaces: windows, metal, crystals):
//!   the lit surface plus the cube map seen in the reflected view ray, x the
//!   node's Amplitude (`Reflectivity`) x the spec map.
//! - `Layer` (shd:simplelayer, 438 ground surfaces): a second colour (DiffMap2,
//!   tiled by Intensity1 on the second UV set) laid over the first through a mask
//!   (DiffMap3).
//! - `Water` (shd:water, coast, ocean: rivers, swamps, harbours): two scrolling
//!   normal maps, the cube map reflected in them, the lit colour map faded by
//!   Amplitude (`colorTransparency`), its edge softened where the ground behind is
//!   near (BumpScale, `softBorderLineRange`).
//! - `VolumeFog` (shd:volumefog: light shafts in windows, mist patches): unlit,
//!   scrolling, faded by the depth behind it (Intensity0, `depthDensity`) and at
//!   its silhouette (Intensity2), x Intensity1 (`alphaModulate`).
//! - `Glow` (shd:glow, 174 surfaces: halos, portals, totems): no texture, an
//!   additive shell lit by N.V through FresnelPower's curve, x MatDiffuse x
//!   Amplitude. UNVERIFIED: the shell is not pushed out by FresnelBias x 0.1
//!   (the vs does it) and an animated Amplitude is not applied.
//! - `Particle` (shd:particle, crate::particles): texture x vertex colour, soft
//!   against the scene behind (Intensity0); additive ones fade out in the fog
//!   (the "particle" Additive ps multiplies by the fog factor, bevy's fog would
//!   add its colour).
//!
//! EVIDENCE: the shaders_sm30 effects of those names, disassembled (Solid /
//!   AlphaLit / AlphaUnlit techniques and their preshaders); the parameter <->
//!   n3 variable mapping is each effect parameter's semantic.
//! The depth-based fades need the depth prepass (crate::main's quality switch);
//! without it they are left out.

use bevy::asset::{io::Reader, AssetLoader, LoadContext, RenderAssetUsages};
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType, TextureViewDescriptor, TextureViewDimension};
use bevy::shader::ShaderRef;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Environment = 0,
    Layer = 1,
    Water = 2,
    VolumeFog = 3,
    /// The particle batches of crate::particles (shd:particle).
    Particle = 4,
    Glow = 5,
    /// shd:uvanimated / uvanimated2 surfaces whose texture scrolls (waterfalls,
    /// flowing lava, banners): the StandardMaterial they would have, its uv moved
    /// by the node's Velocity x time in the shader, so merged map surfaces scroll
    /// too and no material changes per frame.
    Scroll = 6,
}

/// The kind of surface a glTF material's extras describe, if it is one of ours.
/// Shared by the scene path (crate::materials) and the flat map path (crate::map).
pub fn kind_of(extras: &str) -> Option<Kind> {
    let shader = |s: &str| extras.contains(&format!("\"nebula_shader\":\"shd:{s}\""));
    let cube = extras.contains("\"dsor_cube\"");
    if extras.contains("\"dsor_scroll\"") {
        Some(Kind::Scroll)
    } else if shader("environment") && cube {
        Some(Kind::Environment)
    } else if shader("simplelayer") && extras.contains("\"dsor_layer\"") {
        Some(Kind::Layer)
    } else if (shader("water") || shader("coast") || shader("ocean")) && cube {
        Some(Kind::Water)
    } else if shader("volumefog") {
        Some(Kind::VolumeFog)
    } else if shader("glow") {
        Some(Kind::Glow)
    } else {
        None
    }
}

/// A number or a 2-vector from a material's extras (tools/embed_animators.py:
/// dsor_alpha, the node's static Intensity0; dsor_scroll, its Velocity).
pub fn extras_number(extras: &str, key: &str) -> Option<f32> {
    if !extras.contains(key) {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(extras).ok()?;
    Some(v.get(key)?.as_f64()? as f32)
}

pub fn extras_vec2(extras: &str, key: &str) -> Option<Vec2> {
    if !extras.contains(key) {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(extras).ok()?;
    let a = v.get(key)?.as_array()?;
    Some(Vec2::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))
}

#[derive(ShaderType, Reflect, Debug, Clone, Copy, Default)]
pub struct SurfaceParams {
    /// x kind, y reflectivity (environment, water), z layer tiling, w 1 when the
    /// surface has a spec map (else Nebula's system/white: a full reflection).
    pub p0: Vec4,
    /// Water: bump scaling 0 / 1, river speed, second bump speed factor.
    pub p1: Vec4,
    /// Water: colour transparency, soft border range. Volume fog: depth density,
    /// alpha modulate, silhouette sharpness, emissive intensity. Particle: depth
    /// density, 1 when additive.
    pub p2: Vec4,
    /// Volume fog, scroll: texture scroll per second (xy).
    pub p3: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct NebulaSurface {
    #[uniform(100)]
    pub params: SurfaceParams,
    #[texture(101, dimension = "cube")]
    #[sampler(102)]
    pub cube: Option<Handle<Image>>,
    #[texture(103)]
    #[sampler(104)]
    pub layer: Option<Handle<Image>>,
    #[texture(105)]
    #[sampler(106)]
    pub mask: Option<Handle<Image>>,
}

impl MaterialExtension for NebulaSurface {
    fn fragment_shader() -> ShaderRef {
        "embedded://dsor_client/shaders/surface.wgsl".into()
    }
}

pub type NebulaMaterial = ExtendedMaterial<StandardMaterial, NebulaSurface>;

/// `textures/<path>.cube.png` (tools/embed_textures.py): six faces stacked top to
/// bottom, +X -X +Y -Y +Z -Z, loaded as one cube texture.
#[derive(Default, TypePath)]
pub struct CubeLoader;

impl AssetLoader for CubeLoader {
    type Asset = Image;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<Image, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let mut image = Image::from_buffer(
            &bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::Default,
            RenderAssetUsages::RENDER_WORLD,
        )
        .map_err(std::io::Error::other)?;
        image.reinterpret_stacked_2d_as_array(6).map_err(|e| std::io::Error::other(format!("{e:?}")))?;
        image.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
        Ok(image)
    }

    fn extensions(&self) -> &[&str] {
        &["cube.png"]
    }
}

pub struct SurfacesPlugin;

impl Plugin for SurfacesPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/surface.wgsl");
        app.register_asset_loader(CubeLoader)
            .add_plugins(MaterialPlugin::<NebulaMaterial>::default());
    }
}
