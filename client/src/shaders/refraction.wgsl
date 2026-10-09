// Nebula's shd:refraction, as the 2018 client draws it (shaders_sm30 "refraction"
// ps_3_0, disassembled): the scene behind the surface, read at the pixel moved by
// the DuDv map --
//   offset = (dudv.xy * 2 - 1) * pixelSize * displacementFactor * 10   (ps preshader)
//   colour = scene(screen + offset) * vertex colour, alpha = alphaBlendFactor * vertex alpha
//   dudv uv = uv + uvVelocity * time                                     (vs preshader)
// drawn over that same scene. bevy's copy of the scene (view_transmission_texture)
// stands in for the client's light buffer; the alpha blend is done here, against
// the unmoved scene, so the pass needs no blending state.
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals, view_transmission_texture, view_transmission_sampler},
}

struct Refraction {
    // x: distortion strength (pixels), y: alphaBlendFactor, zw: texture scroll per second.
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> refraction: Refraction;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var dudv_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var dudv_sampler: sampler;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var tint = vec4<f32>(1.0);
#ifdef VERTEX_COLORS
    tint = in.color;
#endif
    var uv = vec2<f32>(0.0);
#ifdef VERTEX_UVS_A
    uv = in.uv;
#endif
    uv = uv + refraction.params.zw * globals.time;
    let d = textureSample(dudv_texture, dudv_sampler, uv).xy * 2.0 - 1.0;
    let pixel = 1.0 / view.viewport.zw;
    let screen = (in.position.xy - view.viewport.xy) * pixel;
    let moved = textureSample(view_transmission_texture, view_transmission_sampler, screen + d * pixel * refraction.params.x);
    let behind = textureSample(view_transmission_texture, view_transmission_sampler, screen);
    let a = clamp(refraction.params.y * tint.a, 0.0, 1.0);
    var out: FragmentOutput;
    out.color = vec4<f32>(mix(behind.rgb, moved.rgb * tint.rgb, a), 1.0);
    return out;
}
