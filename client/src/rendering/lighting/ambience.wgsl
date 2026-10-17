#define_import_path dsor::ambience

#import bevy_pbr::lighting::{LightingInput, ambience_directional_light, LAYER_BASE}

struct AmbienceTerm {
    direction: vec4<f32>,
    front: vec4<f32>,
    shadowed: vec4<f32>,
    back: vec4<f32>,
}
struct Ambience {
    counts: vec4<u32>,
    terms: array<AmbienceTerm, 64>,
    // rgb linear colour, w 1 when enabled.
    height_fog_color: vec4<f32>,
    // x FogNearHeight, y FogFarHeight.
    fog_heights: vec4<f32>,
    // Gamma rgb: RimLightColor x RimLightIntensity (0: no rim).
    rim_color: vec4<f32>,
    // View-space direction (xyz), exponent (w).
    rim_dir_spec: vec4<f32>,
}
@group(1) @binding(0) var<uniform> ambience: Ambience;

fn apply_ambience(input: ptr<function, LightingInput>, enable_diffuse: bool, shadow: f32) -> vec3<f32> {
    var result = vec3(0.0);
    for (var i = 0u; i < ambience.counts.y; i += 1u) {
        let term = ambience.terms[i];
        // The shadowed share fades with its source radiance. Dark target zones
        // lose shadows naturally; LightShadowIntensity controls their contrast.
        let front = max(vec3(0.0), term.front.rgb - term.shadowed.rgb * (1.0 - shadow));
        if any(front > vec3(0.0)) {
            result += ambience_directional_light(term.direction.xyz, front, input, enable_diffuse);
        }
        // The back light, as the 2018/261 sun shader adds it: LightOppositeColor
        // x sat(BackLightFactor - N.L), diffuse only. It also reaches the faces
        // still turned a little towards the light (Wild Forest's orange rims).
        // Scaled as Bevy's diffuse is (albedo / pi), like the front light.
        let back_weight = saturate(term.back.w - dot((*input).layers[LAYER_BASE].N, term.direction.xyz));
        if enable_diffuse && back_weight > 0.0 && any(term.back.rgb > vec3(0.0)) {
            result += (*input).diffuse_color * term.back.rgb * (back_weight * 0.31830988618);
        }
    }
    return result;
}

// Nebula's height fog, after distance fog. The 2018/261 vertex shaders compute
// saturate((far - y) / (far - near)) from the world height (heightFogColor.w is
// FogFarHeight, fogDistances.z FogNearHeight), and the pixel shaders lerp from
// heightFogColor to the fogged colour by it: none above near, all of it past far.
fn apply_height_fog(color: vec4<f32>, world_position: vec3<f32>) -> vec4<f32> {
    let span = ambience.fog_heights.y - ambience.fog_heights.x;
    if ambience.height_fog_color.w == 0.0 || abs(span) < 1e-4 {
        return color;
    }
    let clear = saturate((ambience.fog_heights.y - world_position.y) / span);
    // In gamma space, as the D3D9 shaders' lrp mixed non-sRGB colours: a linear
    // mix washes partial fog out (about 1.8x brighter at 30% over dark ground).
    let gamma = pow(max(color.rgb, vec3(0.0)), vec3(1.0 / 2.2));
    let fog = pow(ambience.height_fog_color.rgb, vec3(1.0 / 2.2));
    return vec4(pow(mix(fog, gamma, clear), vec3(2.2)), color.a);
}

// The characters' rim light, in gamma space, to add to their lit colour: the
// 2018/261 "character" and "characterfalloff" pixel shaders add
// rimLightColor x pow(max(dot(n_view, rimLightDirAndSpec.xyz), 0), .w) before the
// hit colour and the fogs. Their view-space normals face the camera with +z
// (the normal buffer's decode spans z -0.52..1), as Bevy's view space does.
fn rim_light(world_normal: vec3<f32>, view_from_world: mat4x4<f32>) -> vec3<f32> {
    if ambience.rim_color.w <= 0.0 {
        return vec3(0.0);
    }
    let n = normalize((view_from_world * vec4(world_normal, 0.0)).xyz);
    let rim = pow(max(dot(n, ambience.rim_dir_spec.xyz), 0.0), ambience.rim_dir_spec.w);
    return ambience.rim_color.rgb * rim;
}
