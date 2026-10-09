//! Nebula render states that glTF cannot carry, applied while a model loads.
//!
//! tools/fix_materials.py marks each converted material with `extras.dsor_state`:
//! - `Decal`: ground decals (stones, sand, paths). The mesh is a projection box;
//!   colour and mask are already merged into one RGBA texture, drawn
//!   alpha-blended over the ground the box covers (crate::decals).
//! - `Additive`: glows (windows, torches, magic). Added to the frame, unlit.
//! - `Hidden`: particle emitter surfaces, not artwork: the particles are rebuilt
//!   from the emitters (crate::particles).
//!
//! Before any state, the Nebula shaders a StandardMaterial cannot draw get a
//! material of ours: refraction (crate::refraction), and environment, simplelayer,
//! water, volumefog, glow and scrolling uvanimated surfaces (crate::surfaces).
//! The node's static alpha factor (extras.dsor_alpha) is applied to the rest.
//!
//! Done in a glTF extension handler, so the states are part of the loaded scene:
//! nothing runs per entity once the map is up. Every surface that is not opaque
//! also stops casting shadows, as in the game's shadow pass.

use bevy::gltf::extensions::{ErasedGltfExtensionHandler, GltfExtensionHandler, GltfExtensionHandlers};
use bevy::gltf::{gltf, GltfMaterial};
use bevy::asset::LoadContext;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

/// A Nebula decal: its mesh is the unit cube the texture is projected through
/// (straight down its local Y) onto the ground inside it.
///
/// With `extras.dsor_decal` (tools/fix_materials.py --n3) it carries the colour,
/// repeated over the ground at `scale`, and the mask cut over the box; without
/// it the box material's merged texture is stretched over the box once.
#[derive(Component, Reflect, Default)]
#[reflect(Component, Default)]
pub struct DecalVolume {
    pub tiling: Option<DecalTiling>,
}

#[derive(Reflect, Default, Clone)]
pub struct DecalTiling {
    pub color: Handle<Image>,
    pub mask: Handle<Image>,
    /// The n3 node's `Scale`: colour repeats per 1/scale world units.
    pub scale: f32,
}

fn decal_tiling(textures: &[Option<Handle<Image>>], material: &gltf::Material) -> Option<DecalTiling> {
    let extras = material.extras().as_ref()?.get();
    let v: serde_json::Value = serde_json::from_str(extras).ok()?;
    let d = v.get("dsor_decal")?;
    let index = |k: &str| d.get(k).and_then(|x| x.as_u64());
    Some(DecalTiling {
        color: textures.get(index("color")? as usize)?.clone()?,
        mask: textures.get(index("mask")? as usize)?.clone()?,
        scale: d.get("scale").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Decal,
    Additive,
    Hidden,
}

fn state_of(material: &gltf::Material) -> Option<State> {
    let extras = material.extras().as_ref()?.get();
    if !extras.contains("dsor_state") {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(extras).ok()?;
    match v.get("dsor_state")?.as_str()? {
        "Decal" => Some(State::Decal),
        "Additive" => Some(State::Additive),
        "Hidden" => Some(State::Hidden),
        _ => None,
    }
}

fn dsor_label(material_label: &str) -> String {
    format!("{material_label}/dsor")
}

/// bevy_pbr's own conversion is private; these are the fields our materials use.
fn standard_material(m: &GltfMaterial) -> StandardMaterial {
    StandardMaterial {
        base_color: m.base_color,
        base_color_channel: m.base_color_channel.clone(),
        base_color_texture: m.base_color_texture.clone(),
        emissive: m.emissive,
        emissive_texture: m.emissive_texture.clone(),
        perceptual_roughness: m.perceptual_roughness,
        metallic: m.metallic,
        normal_map_texture: m.normal_map_texture.clone(),
        double_sided: m.double_sided,
        cull_mode: m.cull_mode,
        unlit: m.unlit,
        alpha_mode: m.alpha_mode,
        uv_transform: m.uv_transform,
        ..default()
    }
}

/// A node's animated shader variables (n3 animator nodes, tools/embed_animators.py).
#[derive(Component, Reflect, Default, Clone)]
#[reflect(Component, Default)]
pub struct ShaderAnim {
    tracks: Vec<VarTrack>,
    /// Seconds since the model was spawned.
    t: f32,
}

#[derive(Reflect, Default, Clone)]
struct VarTrack {
    var: String,
    looping: bool,
    keys: Vec<[f32; 2]>,
}

impl VarTrack {
    fn at(&self, t: f32) -> f32 {
        let (Some(first), Some(last)) = (self.keys.first(), self.keys.last()) else { return 1.0 };
        let t = if self.looping && last[0] > 0.0 { t % last[0] } else { t.clamp(first[0], last[0]) };
        for w in self.keys.windows(2) {
            if t <= w[1][0] {
                let span = (w[1][0] - w[0][0]).max(1e-6);
                return w[0][1] + (w[1][1] - w[0][1]) * ((t - w[0][0]) / span).clamp(0.0, 1.0);
            }
        }
        last[1]
    }
}

/// A Nebula sprite node (n3 "RPSS"): turned to face the camera every frame, its
/// children placed in that frame (tools/embed_animators.py).
#[derive(Component, Reflect, Default, Clone)]
#[reflect(Component, Default)]
pub struct FacesViewer;

/// After the animation pose: each sprite node's world rotation becomes the
/// camera's (its translation and scale stay the animation's).
fn face_viewer(
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut sprites: Query<(&mut Transform, &ChildOf), With<FacesViewer>>,
    parents: Query<&GlobalTransform>,
) {
    let Some(cam) = cameras.iter().next() else { return };
    let cam_rot = cam.compute_transform().rotation;
    for (mut tf, parent) in &mut sprites {
        let Ok(p) = parents.get(parent.parent()) else { continue };
        let parent_rot = p.compute_transform().rotation;
        tf.rotation = parent_rot.inverse() * cam_rot;
    }
}

/// A node's texture animation: n3 UV animator keys (offset, scale, rotation of the
/// texture layer) and/or the uvanimated shader's Velocity scroll. Applied as the
/// material's uv_transform, per instance.
/// EVIDENCE: uvanimated vs (shaders_sm30): uv = uv + c6 (an offset the engine
///   advances); uvanimated2 vs: uv = (u, v, 1) . uvTransform rows.
/// UNVERIFIED: the rotation's pivot (taken at 0.5, 0.5) and units (degrees).
#[derive(Component, Reflect, Default, Clone)]
#[reflect(Component, Default)]
pub struct UvAnim {
    looping: bool,
    pos: Vec<[f32; 4]>,
    scale: Vec<[f32; 4]>,
    rot: Vec<[f32; 4]>,
    velocity: Vec2,
    t: f32,
}

fn keys_at(keys: &[[f32; 4]], t: f32, looping: bool, default: Vec3) -> Vec3 {
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else { return default };
    let t = if looping && last[0] > 0.0 { t % last[0] } else { t.clamp(first[0], last[0]) };
    for w in keys.windows(2) {
        if t <= w[1][0] {
            let k = ((t - w[0][0]) / (w[1][0] - w[0][0]).max(1e-6)).clamp(0.0, 1.0);
            return Vec3::new(w[0][1], w[0][2], w[0][3]).lerp(Vec3::new(w[1][1], w[1][2], w[1][3]), k);
        }
    }
    Vec3::new(last[1], last[2], last[3])
}

impl UvAnim {
    fn from_extras(v: &serde_json::Value) -> Option<Self> {
        let keys = |k: &str| -> Vec<[f32; 4]> {
            v.get("dsor_uvanim")
                .and_then(|u| u.get(k))
                .and_then(|a| a.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|k| {
                            let k = k.as_array()?;
                            Some([0, 1, 2, 3].map(|i| k.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let velocity = v
            .get("dsor_vector")
            .and_then(|d| d.get("Velocity"))
            .and_then(|a| a.as_array())
            .map(|a| Vec2::new(a.first().and_then(|x| x.as_f64()).unwrap_or(0.0) as f32, a.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32))
            .unwrap_or(Vec2::ZERO);
        let uv = Self {
            looping: v.get("dsor_uvanim").and_then(|u| u.get("loop")).and_then(|l| l.as_str()) != Some("clamp"),
            pos: keys("pos"),
            scale: keys("scale"),
            rot: keys("rot"),
            velocity,
            t: 0.0,
        };
        (!uv.pos.is_empty() || !uv.scale.is_empty() || !uv.rot.is_empty() || uv.velocity != Vec2::ZERO).then_some(uv)
    }

    fn transform(&self) -> bevy::math::Affine2 {
        let pos = keys_at(&self.pos, self.t, self.looping, Vec3::ZERO);
        let scale = keys_at(&self.scale, self.t, self.looping, Vec3::ONE);
        let rot = keys_at(&self.rot, self.t, self.looping, Vec3::ZERO);
        let offset = Vec2::new(pos.x, pos.y) + self.velocity * self.t;
        let offset = offset - offset.floor();
        let pivot = Vec2::splat(0.5);
        bevy::math::Affine2::from_translation(offset + pivot)
            * bevy::math::Affine2::from_angle(rot.z.to_radians())
            * bevy::math::Affine2::from_scale(Vec2::new(scale.x, scale.y).max(Vec2::splat(1e-4)))
            * bevy::math::Affine2::from_translation(-pivot)
    }
}

fn animate_uvs(
    mut commands: Commands,
    time: Res<Time>,
    mut nodes: Query<(&mut UvAnim, &Children, &InheritedVisibility)>,
    mut prims: Query<(Entity, &mut MeshMaterial3d<StandardMaterial>, Option<&AnimatedMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (mut uv, children, shown) in &mut nodes {
        uv.t += time.delta_secs();
        if !shown.get() {
            continue;
        }
        let tf = uv.transform();
        for c in children.iter() {
            let Ok((e, mut handle, own)) = prims.get_mut(c) else { continue };
            if own.is_none() {
                let Some(m) = materials.get(&handle.0) else { continue };
                let (base, emissive) = (m.base_color.to_linear(), m.emissive);
                let copy = m.clone();
                handle.0 = materials.add(copy);
                commands.entity(e).insert(AnimatedMaterial { base, emissive });
            }
            if let Some(mut m) = materials.get_mut(&handle.0) {
                m.uv_transform = tf;
            }
        }
    }
}

/// A primitive whose material an animator drives: its own copy, and the values
/// the animation scales.
#[derive(Component)]
struct AnimatedMaterial {
    base: LinearRgba,
    emissive: LinearRgba,
}

/// n3 animators: per instance, MatEmissiveIntensity scales a glow (an unlit
/// additive surface's colour, or a lit surface's emission); Intensity0/1/3 scale
/// opacity (decals, blended cards) -- glows that fade, scorches that appear and go.
/// UNVERIFIED: Intensity0..3 as opacity; Amplitude and Fresnel* are not applied.
fn animate_shader_vars(
    mut commands: Commands,
    time: Res<Time>,
    mut nodes: Query<(&mut ShaderAnim, &Children, &InheritedVisibility)>,
    mut prims: Query<(Entity, &mut MeshMaterial3d<StandardMaterial>, Option<&AnimatedMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (mut anim, children, shown) in &mut nodes {
        anim.t += time.delta_secs();
        // Not seen: its materials are left alone (every material change makes bevy
        // re-check every mesh entity).
        if !shown.get() {
            continue;
        }
        let t = anim.t;
        // The intensity itself (the material holds the unscaled colour, see
        // NodeParams::glow_animated), as a linear factor (gamma_factor).
        let glow = anim.tracks.iter().find(|k| k.var == "MatEmissiveIntensity").map(|k| gamma_factor(k.at(t)));
        let fade = anim.tracks.iter().find(|k| k.var.starts_with("Intensity")).map(|k| k.at(t));
        if glow.is_none() && fade.is_none() {
            continue;
        }
        for c in children.iter() {
            let Ok((e, mut handle, own)) = prims.get_mut(c) else { continue };
            let Some(m) = materials.get(&handle.0) else { continue };
            let (base, emissive) = match own {
                Some(a) => (a.base, a.emissive),
                None => {
                    // This instance's own copy, so instances animate independently.
                    let (base, emissive) = (m.base_color.to_linear(), m.emissive);
                    let copy = m.clone();
                    handle.0 = materials.add(copy);
                    commands.entity(e).insert(AnimatedMaterial { base, emissive });
                    (base, emissive)
                }
            };
            let Some(mut m) = materials.get_mut(&handle.0) else { continue };
            let mut b = base;
            let mut em = emissive;
            if let Some(g) = glow {
                if m.unlit {
                    b = LinearRgba::new(b.red * g, b.green * g, b.blue * g, b.alpha);
                } else {
                    em = em * g;
                }
            }
            if let Some(f) = fade {
                if matches!(m.alpha_mode, AlphaMode::Add) {
                    b = LinearRgba::new(b.red * f, b.green * f, b.blue * f, b.alpha);
                } else {
                    b.alpha *= f;
                }
            }
            m.base_color = b.into();
            m.emissive = em;
        }
    }
}

/// A node's n3 shader parameters (tools/embed_animators.py): its floats
/// (extras.dsor_shader: MatEmissiveIntensity, Intensity0..3, Amplitude, Scale,
/// BumpScale...) and vectors (extras.dsor_vector: Velocity, MatDiffuse...).
#[derive(Default, Clone)]
struct NodeParams {
    floats: std::collections::HashMap<String, f32>,
    vectors: std::collections::HashMap<String, Vec4>,
    velocity: Vec2,
    /// An animator drives its MatEmissiveIntensity (ShaderAnim): the material keeps
    /// its unscaled colour and the animator applies the intensity.
    glow_animated: bool,
}

impl NodeParams {
    fn get(&self, k: &str) -> Option<f32> {
        self.floats.get(k).copied()
    }
}

#[derive(Default, Clone)]
struct NebulaStates {
    /// This file's textures by glTF index (external images load by path, not as
    /// labelled sub-assets, so on_texture is the only way to their handles).
    textures: Vec<Option<Handle<Image>>>,
    /// Node name -> its n3 shader parameters.
    params: std::collections::HashMap<String, NodeParams>,
    /// Material labels given a refraction material ("<label>/refr").
    refractions: std::collections::HashSet<String>,
    /// Material labels given a surface material ("<label>/neb", crate::surfaces).
    surfaces: std::collections::HashSet<String>,
    /// Whether lit surfaces of this file get their emission scaled (effects and
    /// the map; characters keep theirs).
    scale_lit: bool,
    /// Material labels given a material of ours ("<label>/dsor").
    ours: std::collections::HashSet<String>,
}


/// The node a material belongs to: DSO_Godot names materials "<node>_<shader>".
fn material_node<'a>(material: &'a gltf::Material<'a>) -> Option<&'a str> {
    material.name().and_then(|n| n.rsplit_once('_')).map(|(n, _)| n)
}

/// The asset path in a material's extras.dsor_cube (tools/embed_textures.py).
fn cube_path(extras: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(extras).ok()?;
    Some(v.get("dsor_cube")?.as_str()?.to_owned())
}

/// A character surface's dye mask (extras.dsor_dye, tools/embed_character_masks.py).
fn dye_mask(textures: &[Option<Handle<Image>>], extras: &str) -> Option<Handle<Image>> {
    let v: serde_json::Value = serde_json::from_str(extras).ok()?;
    textures.get(v.get("dsor_dye")?.get("mask")?.as_u64()? as usize)?.clone()
}

/// The second layer's textures and tiling (extras.dsor_layer).
fn layer_of(textures: &[Option<Handle<Image>>], extras: &str) -> Option<(Handle<Image>, Handle<Image>, f32)> {
    let v: serde_json::Value = serde_json::from_str(extras).ok()?;
    let l = v.get("dsor_layer")?;
    let tex = |k: &str| textures.get(l.get(k)?.as_u64()? as usize)?.clone();
    Some((tex("color")?, tex("mask")?, l.get("tiling").and_then(|t| t.as_f64()).unwrap_or(1.0) as f32))
}

impl GltfExtensionHandler for NebulaStates {
    fn dyn_clone(&self) -> Box<dyn ErasedGltfExtensionHandler> {
        Box::new(self.clone())
    }

    fn on_root(&mut self, load_context: &mut LoadContext<'_>, gltf: &::gltf::Gltf, _: &bevy::gltf::GltfLoaderSettings) {
        self.scale_lit = !load_context.path().path().to_string_lossy().starts_with("characters");
        for node in gltf.nodes() {
            let (Some(name), Some(extras)) = (node.name(), node.extras().as_ref()) else { continue };
            let raw = extras.get();
            if !raw.contains("dsor_shader") && !raw.contains("dsor_vector") && !raw.contains("dsor_anim") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else { continue };
            let floats = v
                .get("dsor_shader")
                .and_then(|s| s.as_object())
                .map(|o| o.iter().filter_map(|(k, x)| Some((k.clone(), x.as_f64()? as f32))).collect())
                .unwrap_or_default();
            let vectors: std::collections::HashMap<String, Vec4> = v
                .get("dsor_vector")
                .and_then(|s| s.as_object())
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, x)| {
                            let a = x.as_array()?;
                            let c = |i: usize| a.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
                            Some((k.clone(), Vec4::new(c(0), c(1), c(2), c(3))))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let velocity = vectors.get("Velocity").map(|v| v.truncate().truncate()).unwrap_or(Vec2::ZERO);
            let glow_animated = v
                .get("dsor_anim")
                .and_then(|a| a.as_array())
                .is_some_and(|l| l.iter().any(|t| t.get("var").and_then(|x| x.as_str()) == Some("MatEmissiveIntensity")));
            self.params.insert(name.to_owned(), NodeParams { floats, vectors, velocity, glow_animated });
        }
    }

    fn on_gltf_node(&mut self, _: &mut LoadContext<'_>, gltf_node: &::gltf::Node, entity: &mut EntityWorldMut) {
        let Some(extras) = gltf_node.extras() else { return };
        let raw = extras.get();
        if raw.contains("\"dsor_sprite\":true") {
            entity.insert(FacesViewer);
        }
        if raw.contains("dsor_uvanim") || raw.contains("dsor_vector") {
            if let Some(uv) = serde_json::from_str::<serde_json::Value>(raw).ok().and_then(|v| UvAnim::from_extras(&v)) {
                entity.insert(uv);
            }
        }
        if !raw.contains("dsor_anim") {
            return;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else { return };
        let Some(list) = v.get("dsor_anim").and_then(|a| a.as_array()) else { return };
        let tracks = list
            .iter()
            .filter_map(|a| {
                Some(VarTrack {
                    var: a.get("var")?.as_str()?.to_owned(),
                    looping: a.get("loop").and_then(|l| l.as_str()) == Some("loop"),
                    keys: a
                        .get("keys")?
                        .as_array()?
                        .iter()
                        .filter_map(|k| Some([k.get(0)?.as_f64()? as f32, k.get(1)?.as_f64()? as f32]))
                        .collect(),
                })
            })
            .collect();
        entity.insert(ShaderAnim { tracks, t: 0.0 });
    }

    fn on_texture(&mut self, gltf_texture: &gltf::Texture, texture: Handle<Image>) {
        let i = gltf_texture.index();
        if self.textures.len() <= i {
            self.textures.resize(i + 1, None);
        }
        self.textures[i] = Some(texture);
    }

    fn on_material(
        &mut self,
        load_context: &mut LoadContext<'_>,
        gltf_material: &gltf::Material,
        _material: Handle<GltfMaterial>,
        material_asset: &GltfMaterial,
        material_label: &str,
    ) {
        let extras = gltf_material.extras().as_ref().map(|e| e.get().to_owned()).unwrap_or_default();
        let node = material_node(gltf_material).and_then(|n| self.params.get(n)).cloned().unwrap_or_default();
        // An animated glow keeps its colour unscaled: the animator applies it.
        // FAILURE: a static 0 baked in (warrior_mightyswing's swoosh: 0, then up to
        //   3 for a tenth of a second) left the colour black, and the animator only
        //   scaled that black ("mighty swing ne marche pas").
        let intensity = if node.glow_animated { None } else { node.get("MatEmissiveIntensity") };
        // Refraction (shd:refraction): its own material, crate::refraction.
        // EVIDENCE: shaders_sm30 "refraction": the distortion is displacementFactor
        //   (semantic Intensity1) x 10 pixels (ps preshader), the alpha
        //   AlphaBlendFactor (the engine's fade, 1) x vertex alpha, the DuDv map
        //   scrolled by uvVelocity (Velocity) x time.
        if extras.contains("shd:refraction") {
            let m = crate::refraction::RefractionMaterial {
                base: crate::refraction::base(),
                extension: crate::refraction::Refraction {
                    params: Vec4::new(10.0 * node.get("Intensity1").unwrap_or(1.0), 1.0, node.velocity.x, node.velocity.y),
                    dudv: material_asset.base_color_texture.clone(),
                },
            };
            load_context.add_labeled_asset(format!("{material_label}/refr"), m);
            self.refractions.insert(material_label.to_owned());
            return;
        }
        if let Some(kind) = crate::surfaces::kind_of(&extras).filter(|k| *k != crate::surfaces::Kind::Scroll) {
            use crate::surfaces::{Kind, NebulaMaterial, NebulaSurface, SurfaceParams};
            let mut base = standard_material(material_asset);
            let mut params = SurfaceParams { p0: Vec4::new(kind as u32 as f32, 0.0, 1.0, 0.0), ..default() };
            let (mut layer, mut mask) = (None, None);
            let f = |k: &str, d: f32| node.get(k).unwrap_or(d);
            match kind {
                Kind::Environment | Kind::Layer => {
                    params.p0.y = f("Amplitude", 0.5);
                    params.p0.w = material_asset.metallic_roughness_texture.is_some() as u32 as f32;
                    if let Some((c, m, tiling)) = layer_of(&self.textures, &extras) {
                        (layer, mask) = (Some(c), Some(m));
                        params.p0.z = tiling;
                    }
                    if self.scale_lit && material_asset.emissive_texture.is_some() {
                        base.emissive = base.emissive * (gamma_factor(intensity.unwrap_or(1.0)) * EMISSIVE_NITS);
                    }
                }
                Kind::Water => {
                    params.p0.y = f("Intensity1", 0.0);
                    params.p1 = Vec4::new(f("Intensity0", 1.0), f("Scale", 1.0), f("Intensity3", 0.0), f("Intensity2", 0.0));
                    params.p2 = Vec4::new(f("Amplitude", 0.0), f("BumpScale", 0.0), 0.0, 0.0);
                    base.perceptual_roughness = 0.3;
                    base.metallic = 0.0;
                }
                Kind::VolumeFog => {
                    params.p2 = Vec4::new(f("Intensity0", 1.0), f("Intensity1", 0.0), f("Intensity2", 0.0), intensity.unwrap_or(0.0));
                    params.p3 = node.velocity.extend(0.0).extend(0.0);
                    base.unlit = true;
                    base.alpha_mode = AlphaMode::Blend;
                }
                Kind::Glow => {
                    let c = node.vectors.get("MatDiffuse").copied().unwrap_or(Vec4::ZERO);
                    let lin = Color::srgb(c.x, c.y, c.z).to_linear();
                    params.p0.y = f("Amplitude", 0.0);
                    params.p1 = Vec4::new(lin.red, lin.green, lin.blue, 0.0);
                    params.p2 = Vec4::new(f("FresnelPower", 0.0), 1.0, 0.0, 0.0);
                    base.unlit = true;
                    base.alpha_mode = AlphaMode::Add;
                    base.fog_enabled = false;
                    base.base_color = Color::WHITE;
                }
                Kind::Character => {
                    mask = dye_mask(&self.textures, &extras);
                }
                // Never here: crate::particles builds its own; Scroll is below.
                Kind::Particle | Kind::Scroll => {}
            }
            let cube = cube_path(&extras).map(|p| load_context.load::<Image>(p));
            let m = NebulaMaterial { base, extension: NebulaSurface { params, cube, layer, mask } };
            load_context.add_labeled_asset(format!("{material_label}/neb"), m);
            self.surfaces.insert(material_label.to_owned());
            return;
        }
        let state = state_of(gltf_material);
        let mut m = standard_material(material_asset);
        match state {
            Some(State::Decal) => {
                m.alpha_mode = AlphaMode::Blend;
                m.depth_bias = crate::decals::DECAL_DEPTH_BIAS;
            }
            Some(State::Additive) => {
                m.alpha_mode = AlphaMode::Add;
                m.unlit = true;
                m.cull_mode = None;
                // The glow is EmsvMap0 x MatEmissiveIntensity (the fireball's big
                // halo is 0.1: a faint red haze, not a white disc).
                if let Some(i) = intensity {
                    let i = gamma_factor(i);
                    let c = m.base_color.to_linear();
                    m.base_color = LinearRgba::new(c.red * i, c.green * i, c.blue * i, c.alpha).into();
                }
            }
            Some(State::Hidden) => return,
            None => {
                let unlit = gltf_material.extras().as_ref().is_some_and(|e| e.get().contains("\"dsor_unlit\":true"));
                let emissive = self.scale_lit && material_asset.emissive_texture.is_some();
                if !unlit && !emissive && !extras.contains("\"dsor_alpha\"") && !extras.contains("\"dsor_scroll\"") {
                    return;
                }
                // Drawn without lighting, as its Nebula state or shader says
                // (AlphaUnlit, PostAlphaUnlit, shd:unlit...): the sun no longer
                // tints it.
                if unlit {
                    m.unlit = true;
                }
                // A lit surface with an emissive map, outside the characters: its
                // emission at the node's intensity, in Bevy's luminance units.
                if emissive {
                    m.emissive = m.emissive * (gamma_factor(intensity.unwrap_or(1.0)) * EMISSIVE_NITS);
                }
            }
        }
        // The node's static alpha factor (Intensity0): opacity, or brightness for
        // an additive surface.
        if let Some(a) = crate::surfaces::extras_number(&extras, "dsor_alpha") {
            let c = m.base_color.to_linear();
            m.base_color = if matches!(m.alpha_mode, AlphaMode::Add) {
                let a = gamma_factor(a);
                LinearRgba::new(c.red * a, c.green * a, c.blue * a, c.alpha)
            } else {
                LinearRgba::new(c.red, c.green, c.blue, c.alpha * a)
            }
            .into();
        }
        if let Some(v) = crate::surfaces::extras_vec2(&extras, "dsor_scroll") {
            use crate::surfaces::{Kind, NebulaMaterial, NebulaSurface, SurfaceParams};
            let params = SurfaceParams { p0: Vec4::new(Kind::Scroll as u32 as f32, 0.0, 1.0, 0.0), p3: v.extend(0.0).extend(0.0), ..default() };
            let surface = NebulaMaterial { base: m, extension: NebulaSurface { params, cube: None, layer: None, mask: None } };
            load_context.add_labeled_asset(format!("{material_label}/neb"), surface);
            self.surfaces.insert(material_label.to_owned());
            return;
        }
        load_context.add_labeled_asset(dsor_label(material_label), m);
        self.ours.insert(material_label.to_owned());
    }

    fn on_spawn_mesh_and_material(
        &mut self,
        load_context: &mut LoadContext<'_>,
        _primitive: &gltf::Primitive,
        _mesh: &gltf::Mesh,
        material: &gltf::Material,
        entity: &mut EntityWorldMut,
        material_label: &str,
    ) {
        let state = state_of(material);
        // Skill effects cast no shadow (the fireball's rock threw one on the ground;
        // in the game its flight leaves none).
        let effect = load_context.path().path().to_string_lossy().starts_with("effects");
        if effect || state.is_some() || format!("{:?}", material.alpha_mode()) != "Opaque" {
            entity.insert(NotShadowCaster);
        }
        if self.refractions.contains(material_label) {
            let handle = load_context.get_label_handle::<crate::refraction::RefractionMaterial>(format!("{material_label}/refr"));
            entity.remove::<MeshMaterial3d<StandardMaterial>>();
            entity.insert((MeshMaterial3d(handle), Visibility::Inherited, NotShadowCaster));
            return;
        }
        if self.surfaces.contains(material_label) {
            let handle = load_context.get_label_handle::<crate::surfaces::NebulaMaterial>(format!("{material_label}/neb"));
            entity.remove::<MeshMaterial3d<StandardMaterial>>();
            entity.insert((MeshMaterial3d(handle), Visibility::Inherited));
            return;
        }
        match state {
            Some(State::Hidden) => {
                entity.insert(Visibility::Hidden);
            }
            Some(state) => {
                if state == State::Decal {
                    // A projection box, not a surface: crate::decals draws what it covers.
                    let tiling = decal_tiling(&self.textures, material);
                    entity.insert((DecalVolume { tiling }, Visibility::Hidden));
                }
                let handle = load_context.get_label_handle::<StandardMaterial>(dsor_label(material_label));
                entity.insert(MeshMaterial3d(handle));
            }
            None => {
                if self.ours.contains(material_label) {
                    let handle = load_context.get_label_handle::<StandardMaterial>(dsor_label(material_label));
                    entity.insert(MeshMaterial3d(handle));
                }
            }
        }
    }
}

/// Nebula adds EmsvMap0 x MatEmissiveIntensity to the lit colour at display
/// brightness; Bevy's emissive is a luminance, and its default exposure (EV100
/// 9.7: 1 / (1.2 x 2^9.7) ~ 1/1000) shows ~1000 nits as 1.0.
/// EVIDENCE: shaders_sm30 "particle" ps_3_0: colour x (1 + MatEmissiveIntensity);
///   the intensities themselves are the n3 nodes' (tools/embed_animators.py).
const EMISSIVE_NITS: f32 = 1000.0;

/// A factor Nebula applies to a gamma-space colour, as the factor to apply to
/// bevy's linear one: (k x c)^2.2 = k^2.2 x c^2.2. Nebula adds emission and
/// additive colours in gamma space (D3D9, no sRGB writes); applied as is in linear
/// space, a 0.3 emission was drawn at 0.58 ("les emissives sont trop puissants").
fn gamma_factor(k: f32) -> f32 {
    k.max(0.0).powf(2.2)
}

pub struct MaterialsPlugin;

impl Plugin for MaterialsPlugin {
    fn build(&self, app: &mut App) {
        // Scene components must be reflected to be instanced.
        app.register_type::<DecalVolume>().register_type::<DecalTiling>();
        app.register_type::<ShaderAnim>().register_type::<VarTrack>().register_type::<FacesViewer>();
        app.add_systems(
            PostUpdate,
            face_viewer.after(bevy::app::AnimationSystems).before(TransformSystems::Propagate),
        );
        app.register_type::<UvAnim>();
        // Chained: the first to copy a material marks it, the second reuses the copy.
        app.add_systems(Update, (animate_shader_vars, animate_uvs).chain());
        // After bevy_pbr's own handler, so the material it set is replaced.
        let handlers = app.world().resource::<GltfExtensionHandlers>().0.clone();
        #[cfg(target_family = "wasm")]
        bevy::tasks::block_on(async { handlers.write().await.push(Box::new(NebulaStates::default())) });
        #[cfg(not(target_family = "wasm"))]
        handlers.write_blocking().push(Box::new(NebulaStates::default()));
    }
}
