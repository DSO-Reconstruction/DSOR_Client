//! Nebula render states that glTF cannot carry, applied while a model loads.
//!
//! tools/fix_materials.py marks each converted material with `extras.dsor_state`:
//! - `Decal`: ground decals (stones, sand, paths). The mesh is a projection box;
//!   colour and mask are already merged into one RGBA texture, drawn
//!   alpha-blended over the ground the box covers (crate::decals).
//! - `Additive`: glows (windows, torches, magic). Added to the frame, unlit.
//! - `Hidden`: volume fog, refraction and particle emitter surfaces. Not artwork:
//!   particles are rebuilt from the `.fx.json` sidecars (crate::fx).
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
    mut nodes: Query<(&mut ShaderAnim, &Children)>,
    mut prims: Query<(Entity, &mut MeshMaterial3d<StandardMaterial>, Option<&AnimatedMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (mut anim, children) in &mut nodes {
        anim.t += time.delta_secs();
        let t = anim.t;
        let glow = anim.tracks.iter().find(|k| k.var == "MatEmissiveIntensity").map(|k| k.at(t));
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

#[derive(Default, Clone)]
struct NebulaStates {
    /// This file's textures by glTF index (external images load by path, not as
    /// labelled sub-assets, so on_texture is the only way to their handles).
    textures: Vec<Option<Handle<Image>>>,
}

impl GltfExtensionHandler for NebulaStates {
    fn dyn_clone(&self) -> Box<dyn ErasedGltfExtensionHandler> {
        Box::new(self.clone())
    }

    fn on_gltf_node(&mut self, _: &mut LoadContext<'_>, gltf_node: &::gltf::Node, entity: &mut EntityWorldMut) {
        let Some(extras) = gltf_node.extras() else { return };
        let raw = extras.get();
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
        let Some(state) = state_of(gltf_material) else { return };
        let mut m = standard_material(material_asset);
        match state {
            State::Decal => {
                m.alpha_mode = AlphaMode::Blend;
                m.depth_bias = 50.0;
            }
            State::Additive => {
                m.alpha_mode = AlphaMode::Add;
                m.unlit = true;
                m.cull_mode = None;
            }
            State::Hidden => return,
        }
        load_context.add_labeled_asset(dsor_label(material_label), m);
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
            None => {}
        }
    }
}

/// Nebula adds its emissive map (EmsvMap0) to the lit colour at display brightness;
/// Bevy's emissive is a luminance in nits, and its default exposure (EV100 9.7)
/// shows ~1000 nits as full white. glTF carries the map with factor 1.0, which in
/// Bevy is nearly black: lit windows, lava cracks and the fireball's burning rock
/// all came out dull ("le fx est pas affiche pareil").
/// UNVERIFIED: Nebula's emissive intensity is 1.0 for these materials.
const EMISSIVE_NITS: f32 = 400.0;

/// Every effect material with an emissive map, once, as it is added.
fn scale_emissive(
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<AssetServer>,
    mut done: Local<std::collections::HashSet<AssetId<StandardMaterial>>>,
) {
    let added: Vec<AssetId<StandardMaterial>> = events
        .read()
        .filter_map(|e| match e {
            AssetEvent::Added { id } => Some(*id),
            _ => None,
        })
        .collect();
    for id in added {
        if !done.insert(id) {
            continue;
        }
        // Effects only: a character's or the map's emissive maps keep their look
        // (scaled, a mage's purple orb came out white -- "t'as change la couleur de
        // ma baguette").
        let effect = assets.get_path(id).is_some_and(|p| p.path().to_string_lossy().starts_with("effects"));
        if !effect {
            continue;
        }
        let Some(mut m) = materials.get_mut(id) else { continue };
        if m.emissive_texture.is_some() && !m.unlit {
            m.emissive = m.emissive * EMISSIVE_NITS;
        }
    }
}

pub struct MaterialsPlugin;

impl Plugin for MaterialsPlugin {
    fn build(&self, app: &mut App) {
        // Scene components must be reflected to be instanced.
        app.register_type::<DecalVolume>().register_type::<DecalTiling>();
        app.register_type::<ShaderAnim>().register_type::<VarTrack>();
        app.add_systems(PostUpdate, scale_emissive);
        app.add_systems(Update, animate_shader_vars);
        // After bevy_pbr's own handler, so the material it set is replaced.
        let handlers = app.world().resource::<GltfExtensionHandlers>().0.clone();
        #[cfg(target_family = "wasm")]
        bevy::tasks::block_on(async { handlers.write().await.push(Box::new(NebulaStates::default())) });
        #[cfg(not(target_family = "wasm"))]
        handlers.write_blocking().push(Box::new(NebulaStates::default()));
    }
}
