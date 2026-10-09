//! The 2018 client's colour range, before the bloom.
//!
//! Nebula renders the scene into an 8-bit buffer encoded x 0.5 (every lit shader
//! ends `mul oC0, colour, 0.5`; pe_compose decodes x 2): nothing it shows exceeds
//! 2.0, and its bright pass (pe_brightpassfilter: 2c x BloomColor x
//! lum(max(2c - threshold, 0))) only ever sees that range. Bevy's HDR scene is
//! unbounded: a window's emission or a stack of additive particles reached tens,
//! and the bloom spread them over the screen ("les fenetres et les fx sont hyper
//! brillants"). This pass clamps the scene to 2.0 shown (2^2.2 linear) just
//! before bevy's bloom.
//! EVIDENCE: shaders_sm30 standard / monster Solid ps (`mul oC0, r1, c10.xxxy`,
//!   c10.x = 0.5), pe_brightpassfilter ps (`mad r1.xyz, r0, 2, -threshold`).

use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::core_pipeline::FullscreenShader;
use bevy::prelude::*;
use bevy::render::render_resource::binding_types::{sampler, texture_2d};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::camera::ExtractedCamera;
use bevy::render::view::ViewTarget;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

pub struct ClampPlugin;

impl Plugin for ClampPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/clamp.wgsl");
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_resource::<SpecializedRenderPipelines<ClampPipeline>>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Render, prepare.in_set(RenderSystems::Prepare))
            .add_systems(Core3d, clamp.before(bevy::post_process::bloom::bloom).in_set(Core3dSystems::PostProcess));
    }
}

#[derive(Resource)]
struct ClampPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen: FullscreenShader,
    shader: Handle<Shader>,
}

fn init_pipeline(mut commands: Commands, device: Res<RenderDevice>, fullscreen: Res<FullscreenShader>, assets: Res<AssetServer>) {
    let layout = BindGroupLayoutDescriptor::new(
        "nebula_clamp_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (texture_2d(TextureSampleType::Float { filterable: false }), sampler(SamplerBindingType::NonFiltering)),
        ),
    );
    commands.insert_resource(ClampPipeline {
        layout,
        sampler: device.create_sampler(&SamplerDescriptor::default()),
        fullscreen: fullscreen.clone(),
        shader: assets.load("embedded://dsor_client/shaders/clamp.wgsl"),
    });
}

impl SpecializedRenderPipeline for ClampPipeline {
    type Key = TextureFormat;

    fn specialize(&self, format: TextureFormat) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("nebula clamp".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                targets: vec![Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL })],
                ..default()
            }),
            ..default()
        }
    }
}

#[derive(Component)]
struct ClampPipelineId(CachedRenderPipelineId);

fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<ClampPipeline>>,
    pipeline: Option<Res<ClampPipeline>>,
    views: Query<(Entity, &ViewTarget, &ExtractedCamera)>,
) {
    let Some(pipeline) = pipeline else { return };
    for (e, target, camera) in &views {
        // Only the HDR scene (the interface camera draws on top later).
        if !camera.hdr {
            continue;
        }
        let id = pipelines.specialize(&cache, &pipeline, target.main_texture_format());
        commands.entity(e).insert(ClampPipelineId(id));
    }
}

fn clamp(view: ViewQuery<(&ViewTarget, &ClampPipelineId)>, pipeline: Res<ClampPipeline>, cache: Res<PipelineCache>, mut ctx: RenderContext) {
    let (target, id) = view.into_inner();
    let Some(render_pipeline) = cache.get_render_pipeline(id.0) else { return };
    let post = target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        "nebula_clamp",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((post.source, &pipeline.sampler)),
    );
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("nebula_clamp"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post.destination,
            depth_slice: None,
            resolve_target: None,
            ops: Operations { load: LoadOp::Clear(Default::default()), store: StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(render_pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.draw(0..3, 0..1);
}
