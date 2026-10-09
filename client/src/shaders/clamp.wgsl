// The 2018 client's colour range (crate::clamp): what its 8-bit scene buffer,
// encoded x 0.5, can hold -- 2.0 shown, 2^2.2 in linear -- before its bloom.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var in_texture: texture_2d<f32>;
@group(0) @binding(1) var in_sampler: sampler;

const TOP: f32 = 4.5948; // 2^2.2

@fragment
fn fs_main(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(in_texture, in_sampler, in.uv);
    return vec4<f32>(min(c.rgb, vec3<f32>(TOP)), c.a);
}
