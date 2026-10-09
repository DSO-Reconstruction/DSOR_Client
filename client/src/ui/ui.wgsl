// The 2018 interface's artwork: atlas texture x vertex colour, as Nebula's gui
// shader draws it (in gamma space), cut to a clip rectangle (the globes' and the
// XP bar's fill). SEE: client/src/ui/mod.rs.
#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import bevy_render::color_operations::{linear_to_srgb, srgb_to_linear}

struct UiMaterial {
    // World-space rectangle kept: min x, min y, max x, max y.
    clip: vec4<f32>,
    // x: 1 when textured.
    flags: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: UiMaterial;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var atlas: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var atlas_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sampled first: textureSample must stay in uniform control flow.
    let t = textureSample(atlas, atlas_sampler, in.uv);
    let p = in.world_position.xy;
    if p.x < material.clip.x || p.y < material.clip.y || p.x > material.clip.z || p.y > material.clip.w {
        discard;
    }
    // The atlas back to its stored (gamma) values, times the gamma vertex colour.
    var c = vec4<f32>(1.0);
    if material.flags.x > 0.5 {
        c = vec4<f32>(linear_to_srgb(t.rgb), t.a);
    }
#ifdef VERTEX_COLORS
    c = c * in.color;
#endif
#ifndef SRGB_OUTPUT
    // A linear target: hand it linear values.
    c = vec4<f32>(srgb_to_linear(c.rgb), c.a);
#endif
    return c;
}
