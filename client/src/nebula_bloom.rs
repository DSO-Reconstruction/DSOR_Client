//! The 2018 client's bloom, in place of bevy's.
//!
//! EVIDENCE: export_win32/frame/default.xml: `DownscaleScene` (pe_downsample2x2 of
//!   the ColorBuffer) runs right after the opaque passes, BEFORE the `ColorAlpha`
//!   pass draws the transparent world (effects, particles, additive surfaces);
//!   `BrightPassFilter` and the four `Bloom` blurs work on that SceneScaled, and
//!   pe_compose adds the result to the final colour x BloomScale. Effects never
//!   bloom in the 2018 client. bevy's bloom took the whole final image, effects
//!   included: they glared ("toujours trop d'emissive").
//! The kernels: shaders/nebula_bloom.wgsl, from the disassembled
//!   pe_brightpassfilter, pe_bloomvert / pe_bloomhori (Weight0..2, Offset1..2)
//!   and pe_compose. Intermediate targets 8-bit, as the client's.
//! The level's look (crate::lighting) sets NebulaBloom on the camera.

use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::core_pipeline::FullscreenShader;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer_sized};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::ViewTarget;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

/// The level's bloom: BrightPassThreshold, BloomScale, BloomColor.
#[derive(Component, Clone, Copy, ExtractComponent, Debug)]
pub struct NebulaBloom {
    pub threshold: f32,
    pub scale: f32,
    pub color: Vec4,
}

pub struct NebulaBloomPlugin;

impl Plugin for NebulaBloomPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/nebula_bloom.wgsl");
        app.add_plugins(ExtractComponentPlugin::<NebulaBloom>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_resource::<SpecializedRenderPipelines<BloomPipeline>>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Render, (prepare_textures.in_set(RenderSystems::PrepareResources), prepare_pipelines.in_set(RenderSystems::Prepare)))
            .add_systems(Core3d, bloom_scene.after(main_opaque_pass_3d).before(main_transparent_pass_3d).in_set(Core3dSystems::MainPass))
            .add_systems(
                Core3d,
                bloom_compose
                    .after(crate::clamp::clamp)
                    .before(bevy::core_pipeline::tonemapping::tonemapping)
                    .in_set(Core3dSystems::PostProcess),
            );
    }
}

const TARGET: TextureFormat = TextureFormat::Rgba8Unorm;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Stage {
    Bright,
    Down,
    Blur,
    Compose,
}

#[derive(Resource)]
struct BloomPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen: FullscreenShader,
    shader: Handle<Shader>,
}

fn init_pipeline(mut commands: Commands, device: Res<RenderDevice>, fullscreen: Res<FullscreenShader>, assets: Res<AssetServer>) {
    let layout = BindGroupLayoutDescriptor::new(
        "nebula_bloom_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer_sized(false, None),
                texture_2d(TextureSampleType::Float { filterable: true }),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        ..default()
    });
    commands.insert_resource(BloomPipeline {
        layout,
        sampler,
        fullscreen: fullscreen.clone(),
        shader: assets.load("embedded://dsor_client/shaders/nebula_bloom.wgsl"),
    });
}

impl SpecializedRenderPipeline for BloomPipeline {
    type Key = (Stage, TextureFormat);

    fn specialize(&self, (stage, format): Self::Key) -> RenderPipelineDescriptor {
        let entry = match stage {
            Stage::Bright => "fs_bright",
            Stage::Down => "fs_down",
            Stage::Blur => "fs_blur",
            Stage::Compose => "fs_compose",
        };
        RenderPipelineDescriptor {
            label: Some("nebula bloom".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL })],
                ..default()
            }),
            ..default()
        }
    }
}

#[derive(Component)]
struct BloomTargets {
    half: CachedTexture,
    a: CachedTexture,
    b: CachedTexture,
    quarter: UVec2,
}

#[derive(Component)]
struct BloomPipelines {
    bright: CachedRenderPipelineId,
    down: CachedRenderPipelineId,
    blur: CachedRenderPipelineId,
    compose: CachedRenderPipelineId,
}

fn prepare_textures(
    mut commands: Commands,
    mut cache: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    views: Query<(Entity, &ExtractedCamera), With<NebulaBloom>>,
) {
    for (e, camera) in &views {
        let Some(size) = camera.physical_viewport_size else { continue };
        let tex = |cache: &mut TextureCache, s: UVec2, label: &'static str| {
            cache.get(
                &device,
                TextureDescriptor {
                    label: Some(label),
                    size: Extent3d { width: s.x.max(1), height: s.y.max(1), depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TARGET,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let half = (size / 2).max(UVec2::ONE);
        let quarter = (size / 4).max(UVec2::ONE);
        commands.entity(e).insert(BloomTargets {
            half: tex(&mut cache, half, "nebula_bloom_half"),
            a: tex(&mut cache, quarter, "nebula_bloom_a"),
            b: tex(&mut cache, quarter, "nebula_bloom_b"),
            quarter,
        });
    }
}

fn prepare_pipelines(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<BloomPipeline>>,
    pipeline: Option<Res<BloomPipeline>>,
    views: Query<(Entity, &ViewTarget), With<NebulaBloom>>,
) {
    let Some(pipeline) = pipeline else { return };
    for (e, target) in &views {
        let mut get = |stage, format| pipelines.specialize(&cache, &pipeline, (stage, format));
        commands.entity(e).insert(BloomPipelines {
            bright: get(Stage::Bright, TARGET),
            down: get(Stage::Down, TARGET),
            blur: get(Stage::Blur, TARGET),
            compose: get(Stage::Compose, target.main_texture_format()),
        });
    }
}

/// One fullscreen pass from `source` (and `second`) into `dest`.
#[allow(clippy::too_many_arguments)]
fn pass(
    ctx: &mut RenderContext,
    cache: &PipelineCache,
    pipeline: &BloomPipeline,
    id: CachedRenderPipelineId,
    source: &TextureView,
    second: &TextureView,
    dest: &TextureView,
    params: [f32; 12],
) -> bool {
    let Some(render_pipeline) = cache.get_render_pipeline(id) else { return false };
    let buffer = ctx.render_device().create_buffer_with_data(&BufferInitDescriptor {
        label: Some("nebula_bloom_params"),
        contents: bytemuck_bytes(&params),
        usage: BufferUsages::UNIFORM,
    });
    let bind_group = ctx.render_device().create_bind_group(
        "nebula_bloom",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((source, &pipeline.sampler, buffer.as_entire_binding(), second)),
    );
    let mut p = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("nebula_bloom"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: dest,
            depth_slice: None,
            resolve_target: None,
            ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    p.set_pipeline(render_pipeline);
    p.set_bind_group(0, &bind_group, &[]);
    p.draw(0..3, 0..1);
    true
}

fn bytemuck_bytes(v: &[f32; 12]) -> &[u8] {
    // SAFETY: [f32; 12] is plain old data, 48 bytes.
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, 48) }
}

/// After the opaque pass, before the transparent one: the bright pass and its blur.
fn bloom_scene(
    view: ViewQuery<(&ViewTarget, &BloomTargets, &BloomPipelines, &NebulaBloom)>,
    pipeline: Res<BloomPipeline>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (target, t, ids, b) = view.into_inner();
    let scene = target.main_texture_view();
    let q = Vec2::new(1.0 / t.quarter.x as f32, 1.0 / t.quarter.y as f32);
    let c = b.color;
    let params = |dir: Vec2| [b.threshold, b.scale, q.x, q.y, c.x, c.y, c.z, c.w, dir.x, dir.y, 0.0, 0.0];
    let ok = pass(&mut ctx, &cache, &pipeline, ids.bright, scene, scene, &t.half.default_view, params(Vec2::ZERO))
        && pass(&mut ctx, &cache, &pipeline, ids.down, &t.half.default_view, &t.half.default_view, &t.a.default_view, params(Vec2::ZERO));
    if !ok {
        return;
    }
    for _ in 0..2 {
        pass(&mut ctx, &cache, &pipeline, ids.blur, &t.a.default_view, &t.a.default_view, &t.b.default_view, params(Vec2::Y));
        pass(&mut ctx, &cache, &pipeline, ids.blur, &t.b.default_view, &t.b.default_view, &t.a.default_view, params(Vec2::X));
    }
}

/// Before the tone mapping: the blurred bright pass added x BloomScale.
fn bloom_compose(
    view: ViewQuery<(&ViewTarget, &BloomTargets, &BloomPipelines, &NebulaBloom)>,
    pipeline: Res<BloomPipeline>,
    cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (target, t, ids, b) = view.into_inner();
    if cache.get_render_pipeline(ids.compose).is_none() || b.scale <= 0.0 {
        return;
    }
    let post = target.post_process_write();
    let params = [b.threshold, b.scale, 0.0, 0.0, b.color.x, b.color.y, b.color.z, b.color.w, 0.0, 0.0, 0.0, 0.0];
    pass(&mut ctx, &cache, &pipeline, ids.compose, post.source, &t.a.default_view, post.destination, params);
}
