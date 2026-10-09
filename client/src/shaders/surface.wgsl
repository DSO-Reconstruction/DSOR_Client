// Nebula's environment, simplelayer, water and volumefog shaders over bevy's PBR,
// from the shaders_sm30 effects of those names (disassembled). SEE: surfaces.rs.
//
//   environment  lit + cube(reflect(eye ray, N)) x Reflectivity x spec
//   simplelayer  colour = lerp(DiffMap0(uv0), DiffMap2(uv1 x tiling), DiffMap3(uv1).r)
//   water        N = n(uv x s0 + (0.7, t x speed)) + n(uv x s1 + (0, t x speed x k2))
//                colour = lit(DiffMap0 x transparency) + cube(reflect) x reflectivity x vcol
//                alpha  = max(max3(reflection), transparency) x DiffMap0.a x vcol.a
//                         x sat(|depth behind| / softBorder)
//   particle     colour = tex x vcol (emissive already in vcol), alpha x sat(behind / (density + 0.05));
//                additive: rgb x that x the fog's transmittance
//   glow         x = N.V, g = x P / ((2x - 1)(P - 1) + x) / 2 (P: FresnelPower),
//                colour = g x Amplitude x MatDiffuse, added, faded by the fog
//   volumefog    colour = tex x vcol x (1 + emissive)
//                alpha  = sat(max(vcol.a, tex.a) x depth behind x density)
//                         x (1 + sharpness x (sat(N.V)^2 - 1)) x alphaModulate
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_bindings,
    pbr_types,
    mesh_view_bindings::{globals, view},
    view_transformations::{depth_ndc_to_view_z, position_world_to_view},
}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif
#ifdef DISTANCE_FOG
#import bevy_pbr::{mesh_view_bindings as view_bindings, mesh_view_types}
#endif

struct Surface {
    p0: vec4<f32>,
    p1: vec4<f32>,
    p2: vec4<f32>,
    p3: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> surface: Surface;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var cube_texture: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var cube_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var layer_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var layer_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var mask_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var mask_sampler: sampler;

const ENVIRONMENT: i32 = 0;
const LAYER: i32 = 1;
const WATER: i32 = 2;
const VOLUME_FOG: i32 = 3;
const PARTICLE: i32 = 4;
const GLOW: i32 = 5;
const SCROLL: i32 = 6;

// How far behind this fragment the opaque scene is, in view units (large when
// the depth prepass is off).
fn depth_behind(frag_coord: vec4<f32>, world: vec3<f32>) -> f32 {
#ifdef DEPTH_PREPASS
    // Reverse Z: 0 is the far plane at infinity (nothing behind). Kept finite: an
    // infinite distance times a zero density is NaN, and bloom spreads a NaN pixel
    // over the whole frame.
    let ndc = prepass_depth(frag_coord, 0u);
    if ndc <= 0.0 {
        return 1.0e4;
    }
    let scene = -depth_ndc_to_view_z(ndc);
    let here = -position_world_to_view(world).z;
    return clamp(scene - here, -1.0e4, 1.0e4);
#else
    return 1.0e4;
#endif
}

// Nebula scrolls with its time wrapped (time mod 10 000 s for water and
// refraction, 1 000 s for volume fog), so the offsets keep their precision.
fn wrapped_time(period: f32) -> f32 {
    return globals.time - floor(globals.time / period) * period;
}

fn vertex_color(in: VertexOutput) -> vec4<f32> {
#ifdef VERTEX_COLORS
    return in.color;
#else
    return vec4<f32>(1.0);
#endif
}

fn uv0(in: VertexOutput) -> vec2<f32> {
#ifdef VERTEX_UVS_A
    return in.uv;
#else
    return vec2<f32>(0.0);
#endif
}

// What of an additive colour the fog lets through (Nebula multiplies additive
// colours by the fog factor; bevy's fog would add its colour instead).
fn fog_clear(in: VertexOutput) -> f32 {
    var clear = 1.0;
#ifdef DISTANCE_FOG
    let d = distance(in.world_position.xyz, view.world_position.xyz);
    let fog = view_bindings::fog;
    if fog.mode == mesh_view_types::FOG_MODE_LINEAR {
        clear = 1.0 - fog.base_color.a * (1.0 - clamp((fog.be.y - d) / (fog.be.y - fog.be.x), 0.0, 1.0));
    }
#endif
    return clear;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let kind = i32(surface.p0.x);
    var moved = in;
#ifdef VERTEX_UVS_A
    if kind == SCROLL {
        moved.uv = in.uv + surface.p3.xy * wrapped_time(10000.0);
    }
#endif
    var pbr_input = pbr_input_from_standard_material(moved, is_front);
    var out: FragmentOutput;

    if kind == PARTICLE {
        var c = textureSample(pbr_bindings::base_color_texture, pbr_bindings::base_color_sampler, uv0(in)) * vertex_color(in);
        let soft = clamp(depth_behind(in.position, in.world_position.xyz) / (surface.p2.x + 0.05), 0.0, 1.0);
        if surface.p2.y > 0.5 {
            c = vec4<f32>(c.rgb * soft * fog_clear(in), c.a);
        } else {
            c.a = c.a * soft;
        }
        out.color = main_pass_post_lighting_processing(pbr_input, c);
        return out;
    }

    if kind == GLOW {
        let x = dot(normalize(in.world_normal), pbr_input.V) * surface.p2.y;
        let p = surface.p2.x;
        let d = (2.0 * x - 1.0) * (p - 1.0) + x;
        var g = 0.0;
        if d > 0.0 && x > 0.0 {
            g = x * p / d * 0.5;
        }
        out.color = vec4<f32>(surface.p1.rgb * surface.p0.y * g * fog_clear(in), 1.0);
        return out;
    }

    if kind == VOLUME_FOG {
        let tex = textureSample(pbr_bindings::base_color_texture, pbr_bindings::base_color_sampler,
                                uv0(in) + surface.p3.xy * wrapped_time(1000.0));
        let vcol = vertex_color(in);
        let color = tex.rgb * vcol.rgb * (1.0 + surface.p2.w);
        var a = clamp(max(vcol.a, tex.a) * depth_behind(in.position, in.world_position.xyz) * surface.p2.x, 0.0, 1.0);
        let facing = clamp(dot(normalize(in.world_normal), pbr_input.V), 0.0, 1.0);
        a = a * (1.0 + surface.p2.z * (facing * facing - 1.0)) * surface.p2.y;
        out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(color, clamp(a, 0.0, 1.0)));
        return out;
    }

    if kind == WATER {
        let uv = uv0(in);
        let t = wrapped_time(10000.0) * surface.p1.z;
        let n0 = textureSample(pbr_bindings::normal_map_texture, pbr_bindings::normal_map_sampler,
                               vec2<f32>(uv.x * surface.p1.x + 0.7, uv.y * surface.p1.x + t)).rgb * 2.0 - 1.0;
        let n1 = textureSample(pbr_bindings::normal_map_texture, pbr_bindings::normal_map_sampler,
                               vec2<f32>(uv.x * surface.p1.y, uv.y * surface.p1.y + t * surface.p1.w)).rgb * 2.0 - 1.0;
        let nt = normalize(n0 + n1);
        let ng = normalize(in.world_normal);
        var n = ng;
#ifdef VERTEX_TANGENTS
        let tangent = normalize(in.world_tangent.xyz);
        let bitangent = cross(ng, tangent) * in.world_tangent.w;
        n = normalize(nt.x * tangent + nt.y * bitangent + nt.z * ng);
#endif
        if !is_front {
            n = -n;
        }
        let diff = textureSample(pbr_bindings::base_color_texture, pbr_bindings::base_color_sampler,
                                 vec2<f32>(uv.x, uv.y + t * surface.p1.w));
        let vcol = vertex_color(in);
        let reflection = textureSample(cube_texture, cube_sampler, reflect(-pbr_input.V, n)).rgb * surface.p0.y * vcol.rgb;
        pbr_input.N = n;
        pbr_input.material.base_color = vec4<f32>(diff.rgb * surface.p2.x, 1.0);
        let lit = apply_pbr_lighting(pbr_input).rgb;
        var a = max(max(max(reflection.r, reflection.g), reflection.b), surface.p2.x) * diff.a * vcol.a;
        if surface.p2.y > 0.0 {
            a = a * clamp(abs(depth_behind(in.position, in.world_position.xyz)) / surface.p2.y, 0.0, 1.0);
        }
        pbr_input.material.base_color.a = a;
        out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(lit + reflection, clamp(a, 0.0, 1.0)));
        return out;
    }

    if kind == LAYER {
#ifdef VERTEX_UVS_B
        let mask = textureSample(mask_texture, mask_sampler, in.uv_b).r;
        let layer = textureSample(layer_texture, layer_sampler, in.uv_b * surface.p0.z);
        pbr_input.material.base_color = vec4<f32>(mix(pbr_input.material.base_color.rgb, layer.rgb, mask), pbr_input.material.base_color.a);
#endif
    }

    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    if (pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    if kind == ENVIRONMENT {
        // Nebula's SpecMap0 red masks the reflection; the conversion kept only a
        // roughness map from it (1 - gloss), and system/white for none.
        var mask = 1.0;
        if surface.p0.w > 0.5 {
            mask = 1.0 - pbr_input.material.perceptual_roughness;
        }
        let r = textureSample(cube_texture, cube_sampler, reflect(-pbr_input.V, pbr_input.N)).rgb;
        out.color = vec4<f32>(out.color.rgb + r * surface.p0.y * mask, out.color.a);
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
