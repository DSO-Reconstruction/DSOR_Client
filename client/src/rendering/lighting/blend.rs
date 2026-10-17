//! Cross-fade radiance from fixed preset directions using one map-wide shadow map.
//! A small shared view uniform also works with WebGL's one-directional-light limit.
//! All forward materials (including cutouts) keep their own PBR specialization.
use super::{linear, Bubble, Lighting, UNIT};
use std::{any::Any, sync::Arc};
use bevy::prelude::*;
use bevy::material::{key::ErasedMaterialPipelineKey, MaterialProperties};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialVertexShader, MaterialFragmentShader, MeshPipeline,
    MeshPipelineKey, MeshViewBindGroup, PreparedMaterial, prepare_mesh_view_bind_groups};
use bevy::render::erased_render_asset::ErasedRenderAssets;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::{*, binding_types::uniform_buffer};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::shader::{ShaderDefVal, Source};

const MAX_TERMS: usize = 64;

pub(super) fn direction(value: Option<[f32; 3]>) -> Vec3 {
    Vec3::from_array(value.unwrap_or([-0.136, 0.721, -0.679])).normalize_or(Vec3::Y)
}

#[derive(Clone, Debug, PartialEq)]
struct LightTerm {
    dir: Vec3,
    front: Vec3,
    shadowed: Vec3,
    back: Vec3,
    /// BackLightFactor: the back light covers sat(offset - N.L), not only the
    /// surfaces facing away from the light.
    back_offset: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct LightBlend(Vec<LightTerm>, f32);

impl Default for LightBlend {
    fn default() -> Self { Self::from(&Bubble::default()) }
}

impl From<&Bubble> for LightBlend {
    fn from(b: &Bubble) -> Self {
        Self::with_reference(b, b.sun_luminance())
    }
}

impl LightBlend {
    pub(super) fn with_reference(b: &Bubble, reference: f32) -> Self {
        let color = |c: [f32; 4]| {
            let c = linear([c[0], c[1], c[2]], b.light_intensity);
            Vec3::new(c.red, c.green, c.blue)
        };
        let front = color(b.light_color);
        // Preserve outdoor contrast, but dim local presets must also lose
        // shadow strength. Merely dimming radiance keeps harsh black shadows
        // when an interior reduces ambient light by a similar amount.
        let strength = b.light_shadow_intensity.clamp(0., 1.)
            * (b.sun_luminance() / reference.max(1e-6)).clamp(0., 1.);
        Self(vec![LightTerm { dir: direction(b.dir), front,
            shadowed: front * strength,
            back: color(b.light_opposite_color), back_offset: b.back_light_factor }], reference)
    }
    pub(super) fn mix(&self, to: &Bubble, t: f32) -> Self {
        if t <= 0. { return self.clone(); }
        if t >= 1. { return Self::with_reference(to, self.1); }
        // Retarget from the actual mixture, including every outgoing direction.
        // Repeated doorway crossings never collapse it into a rotating light.
        let mut result = self.clone();
        for term in &mut result.0 {
            term.front *= 1. - t;
            term.shadowed *= 1. - t;
            term.back *= 1. - t;
        }
        let target = Self::with_reference(to, self.1).0.remove(0);
        if let Some(term) = result.0.iter_mut().find(|term| term.dir.distance_squared(target.dir) < 1e-10) {
            term.front += target.front * t;
            term.shadowed += target.shadowed * t;
            term.back += target.back * t;
            term.back_offset += (target.back_offset - term.back_offset) * t;
        } else {
            result.0.push(LightTerm { dir: target.dir, front: target.front * t,
                shadowed: target.shadowed * t, back: target.back * t, back_offset: target.back_offset });
        }
        // Discard only sub-visible residue after many interrupted transitions.
        result.0.retain(|term| term.front.max_element().max(term.back.max_element()) > 1e-7);
        result
    }

    pub(super) fn gpu(&self, factors: &Lighting) -> GpuAmbience {
        let mut value = GpuAmbience::default();
        let mut terms: Vec<_> = self.0.iter().collect();
        if terms.len() > MAX_TERMS {
            // A pathological sequence of >64 distinct zones within their fade
            // times: retain the visible contributions, never rotate directions.
            terms.sort_by(|a, b| b.front.max_element().max(b.back.max_element())
                .total_cmp(&a.front.max_element().max(a.back.max_element())));
            warn_once!("Ambience blend exceeds {MAX_TERMS} directions; dropping faintest contributions");
            terms.truncate(MAX_TERMS);
        }
        value.counts = UVec4::new(1, terms.len() as u32, 0, 0);
        let unit = std::f32::consts::PI * UNIT;
        for (out, term) in value.terms.iter_mut().zip(terms) {
            *out = GpuTerm { direction: term.dir.extend(0.),
                front: (term.front * unit * factors.sun).extend(0.),
                shadowed: (term.shadowed * unit * factors.sun).extend(0.),
                back: (term.back * unit * factors.back).extend(term.back_offset) };
        }
        value
    }
}

#[derive(Clone, Copy, Default, ShaderType)]
struct GpuTerm { direction: Vec4, front: Vec4, shadowed: Vec4, back: Vec4 }

#[derive(Resource, Clone, ExtractResource, ShaderType)]
pub(super) struct GpuAmbience {
    counts: UVec4,
    terms: [GpuTerm; MAX_TERMS],
    /// Linear rgb; w 1 enables height fog (0 in the inactive buffer).
    height_fog_color: Vec4,
    /// x FogNearHeight, y FogFarHeight.
    fog_heights: Vec4,
    /// Gamma rgb: RimLightColor x RimLightIntensity, Nebula's rimLightColor
    /// (0 in the inactive buffer: no rim).
    rim_color: Vec4,
    /// View-space direction (xyz) and exponent (w), Nebula's rimLightDirAndSpec.
    rim_dir_spec: Vec4,
}

impl Default for GpuAmbience {
    fn default() -> Self {
        Self { counts: UVec4::ZERO, terms: [GpuTerm::default(); MAX_TERMS], height_fog_color: Vec4::ZERO, fog_heights: Vec4::ZERO,
            rim_color: Vec4::ZERO, rim_dir_spec: Vec4::ZERO }
    }
}

impl GpuAmbience {
    /// The bubble's height fog. Like the 2018 shaders, it has no strength of its
    /// own: the colour and the two heights are the whole of it.
    pub(super) fn with_height_fog(mut self, b: &Bubble) -> Self {
        let c = linear([b.height_fog_color[0], b.height_fog_color[1], b.height_fog_color[2]], 1.0);
        self.height_fog_color = Vec4::new(c.red, c.green, c.blue, 1.0);
        self.fog_heights = Vec4::new(b.fog_near_height, b.fog_far_height, 0.0, 0.0);
        self
    }

    /// The characters' rim light. The colour is the bubble's; the engine sets the
    /// direction and exponent itself, the same in every map.
    /// EVIDENCE: the "character" and "characterfalloff" ps_3_0 (shaders_sm30) add
    ///   rimLightColor (c8) x pow(max(dot(n_view, c9.xyz), 0), c9.w) to the lit
    ///   colour, before the hit colour and the fogs. Read from the running
    ///   release261_192 client (D3D9 SetPixelShaderConstantF, Wild Forest):
    ///   c8 = (0.1137, 0.3314, 0.3451, 0.5), its RimLightColor x RimLightIntensity
    ///   0.5; c9 = (0.6963, 0.6963, 0.1741, 4), (4, 4, 1) normalised.
    pub(super) fn with_rim_light(mut self, b: &Bubble) -> Self {
        let c = b.rim_light_color;
        // DSOR_FLAGS=norim leaves it out, for comparisons.
        let scale = if crate::flag("norim") { 0.0 } else { 1.0 };
        self.rim_color = Vec4::new(c[0], c[1], c[2], 1.0) * b.rim_light_intensity * scale;
        self.rim_dir_spec = Vec3::new(4.0, 4.0, 1.0).normalize().extend(4.0);
        self
    }
}

#[derive(Component, Clone, ExtractComponent)]
pub(super) struct AmbienceView;

pub(super) struct BlendPlugin;
impl Plugin for BlendPlugin {
    fn build(&self, app: &mut App) {
        bevy::shader::load_shader_library!(app, "ambience.wgsl");
        app.init_resource::<GpuAmbience>()
            .add_plugins((ExtractResourcePlugin::<GpuAmbience>::default(), ExtractComponentPlugin::<AmbienceView>::default()))
            .add_systems(Update, prepare_shaders);
        let Some(render) = app.get_sub_app_mut(RenderApp) else { return };
        render.init_resource::<SpecializedMeshPipelines<BlendPipeline>>()
            .init_resource::<BlendBuffers>()
            .add_systems(Render, adapt_materials.after(RenderSystems::PrepareAssets).before(RenderSystems::Specialize))
            .add_systems(Render, prepare_bind_groups.in_set(RenderSystems::PrepareBindGroups).after(prepare_mesh_view_bind_groups));
    }
}

fn blend_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new("ambience blend", &BindGroupLayoutEntries::single(
        ShaderStages::FRAGMENT, uniform_buffer::<GpuAmbience>(false)))
}

/// The client doesn't use view environment probes/clustered decals: its cube
/// maps belong to Nebula materials. Group 1 is free, keeping the four bind-group
/// limit on WebGL. Shadow and prepass pipelines retain Bevy's empty group 1.
struct BlendPipeline { pipeline: MaterialPipeline, properties: Arc<MaterialProperties> }
impl SpecializedMeshPipeline for BlendPipeline {
    type Key = ErasedMaterialPipelineKey;
    fn specialize(&self, key: Self::Key, layout: &MeshVertexBufferLayoutRef)
        -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        let mesh_key: MeshPipelineKey = key.mesh_key.downcast();
        let mut desc = self.pipeline.mesh_pipeline.specialize(mesh_key, layout)?;
        assert!(desc.layout[1].entries.is_empty(), "Ambience view binding conflicts with view environment probes");
        desc.layout[1] = blend_layout();
        desc.layout.insert(3, self.properties.material_layout.as_ref().unwrap().clone());
        let material_group = ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 3);
        desc.vertex.shader_defs.push(material_group.clone());
        if let Some(shader) = self.properties.get_shader(MaterialVertexShader) { desc.vertex.shader = shader; }
        if let Some(fragment) = &mut desc.fragment {
            fragment.shader_defs.extend([material_group, "DSOR_AMBIENCE".into()]);
            if let Some(shader) = self.properties.get_shader(MaterialFragmentShader) { fragment.shader = shader; }
        }
        if let Some(specialize) = self.properties.user_specialize {
            specialize(&self.pipeline as &dyn Any, &mut desc, layout, key)?;
        }
        if self.properties.bindless {
            desc.vertex.shader_defs.push("BINDLESS".into());
            if let Some(fragment) = &mut desc.fragment { fragment.shader_defs.push("BINDLESS".into()); }
        }
        Ok(desc)
    }
}

fn specialize(world: &mut World, key: ErasedMaterialPipelineKey, layout: &MeshVertexBufferLayoutRef,
    properties: &Arc<MaterialProperties>) -> Result<CachedRenderPipelineId, SpecializedMeshPipelineError> {
    world.resource_scope(|world, mut cache: Mut<SpecializedMeshPipelines<BlendPipeline>>| {
        let specializer = BlendPipeline { pipeline: MaterialPipeline { mesh_pipeline: world.resource::<MeshPipeline>().clone() },
            properties: properties.clone() };
        cache.specialize(world.resource::<PipelineCache>(), &specializer, key, layout)
    })
}

fn adapt_materials(mut materials: ResMut<ErasedRenderAssets<PreparedMaterial>>) {
    for (_, material) in materials.iter_mut() {
        // Newly prepared properties are unique until specialization shares them.
        if let Some(properties) = Arc::get_mut(&mut material.properties) {
            if properties.base_specialize.is_some() { properties.base_specialize = Some(specialize); }
        }
    }
}

#[derive(Resource, Default)]
struct BlendBuffers { active: UniformBuffer<GpuAmbience>, inactive: UniformBuffer<GpuAmbience>, groups: Option<(BindGroup, BindGroup)> }

fn prepare_bind_groups(device: Res<RenderDevice>, queue: Res<RenderQueue>, cache: Res<PipelineCache>,
    ambience: Res<GpuAmbience>, mut buffers: ResMut<BlendBuffers>, mut views: Query<(&mut MeshViewBindGroup, Has<AmbienceView>)>) {
    if ambience.is_changed() || buffers.groups.is_none() {
        buffers.active.set(ambience.clone());
        buffers.active.write_buffer(&device, &queue);
    }
    if buffers.groups.is_none() {
        buffers.inactive.write_buffer(&device, &queue);
        let layout = cache.get_bind_group_layout(&blend_layout());
        buffers.groups = Some((device.create_bind_group("ambience active", &layout, &BindGroupEntries::single(&buffers.active)),
            device.create_bind_group("ambience inactive", &layout, &BindGroupEntries::single(&buffers.inactive))));
    }
    let (active, inactive) = buffers.groups.as_ref().unwrap();
    for (mut view, enabled) in &mut views { view.binding_array = if enabled { active.clone() } else { inactive.clone() }; }
}

/// Reuse Bevy's current BRDF for each fixed direction, including normal maps,
/// specular and material extensions. Only the contribution blend is custom.
fn light_function(source: &str) -> Option<String> {
    let start = source.find("fn directional_light(")?;
    let end = source[start..].find("\n}")? + start + 2;
    let original = &source[start..end];
    let start_color = original.find("    var texture_sample = 1f;")?;
    let mut function = original[..start_color].to_owned();
    function = function.replace("fn directional_light(", "fn ambience_directional_light(")
        .replace("    light_id: u32,", "    direction: vec3<f32>,\n    light_color: vec3<f32>,")
        .replace("    let light = &view_bindings::lights.directional_lights[light_id];", "")
        .replace("let L = (*light).direction_to_light.xyz;", "let L = direction;");
    if function.contains("(*light)") || !function.contains("let L = direction;") { return None; }
    function.push_str("    return color * light_color;\n}\n");
    Some(format!("{source}\n{function}"))
}

fn blend_function(source: &str) -> Option<String> {
    let start = source.find("        var light_contrib = lighting::directional_light(")?;
    let end_anchor = "        direct_light += light_contrib * shadow;";
    let end = source[start..].find(end_anchor)? + start + end_anchor.len();
    let original = &source[start..end];
    let mut patched = source.replace(original, &format!("#ifdef DSOR_AMBIENCE\n        if ambience.counts.x != 0u && i == 0u {{\n            direct_light += apply_ambience(&lighting_input, enable_diffuse, shadow);\n        }} else {{\n{original}\n        }}\n#else\n{original}\n#endif"));
    // Transmission uses the same radiance blend on the opposite material lobe.
    let start = patched.find("        let transmitted_light_contrib =\n            lighting::directional_light(")?;
    let anchor = "        transmitted_light += transmitted_light_contrib * transmitted_shadow;";
    let end = patched[start..].find(anchor)? + start + anchor.len();
    let original = patched[start..end].to_owned();
    patched = patched.replace(&original, &format!("#ifdef DSOR_AMBIENCE\n        if ambience.counts.x != 0u && i == 0u {{\n            transmitted_light += apply_ambience(&transmissive_lighting_input, enable_diffuse, transmitted_shadow);\n        }} else {{\n{original}\n        }}\n#else\n{original}\n#endif"));
    // Height fog follows distance fog on the same fogged materials, as the 2018
    // pixel shaders lerp heightFogColor over the distance-fogged colour.
    let fog_end = "            pbr_input.frag_coord.xy,\n        );\n    }\n#endif  // DISTANCE_FOG";
    if !patched.contains(fog_end) { return None; }
    patched = patched.replace(fog_end, "            pbr_input.frag_coord.xy,\n        );\n#ifdef DSOR_AMBIENCE\n        output_color = apply_height_fog(output_color, pbr_input.world_position.xyz);\n#endif\n    }\n#endif  // DISTANCE_FOG");
    Some(format!("#ifdef DSOR_AMBIENCE\n#import dsor::ambience::{{ambience, apply_ambience, apply_height_fog}}\n#endif\n{patched}"))
}

fn prepare_shaders(mut shaders: ResMut<Assets<Shader>>, mut done: Local<bool>) {
    if *done { return; }
    let find = |name: &str| shaders.iter().find_map(|(id, s)| {
        (s.import_path == bevy::shader::ShaderImport::Custom(name.into())).then_some(id)
    });
    let (Some(lighting), Some(pbr)) = (find("bevy_pbr::lighting"), find("bevy_pbr::pbr_functions")) else { return };
    for (id, patch) in [(lighting, light_function as fn(&str) -> Option<String>), (pbr, blend_function)] {
        let mut shader = shaders.get_mut(id).unwrap();
        let Source::Wgsl(source) = &shader.source else { panic!("Ambience requires Bevy's WGSL lighting shaders") };
        let patched = Shader::from_wgsl(patch(source).expect("Bevy lighting shader changed; update ambience blend adapter"), shader.path.clone());
        // Refresh the dependency metadata too: pbr_functions now imports our
        // module. Preserve the original shader's definitions and strong handles.
        shader.source = patched.source;
        shader.imports = patched.imports;
    }
    *done = true;
    info!("ambience: fixed shadow projection and radiance cross-fades");
}

#[cfg(test)]
#[path = "blend_tests.rs"]
mod tests;
