//! DSOR client: renders one Drakensang Online map from the converted assets.
//!
//! Native:  `cargo run -p dsor-client --release -- [map] [--screenshot] [--cam x,y,z,tx,ty,tz]`
//! Web:     `index.html?map=a0302_wildforest`
//!
//! Assets come from the `assets/` folder at the workspace root (a symlink to the
//! DSO_Godot export, see `docs/assets.md`), always through Bevy's `AssetServer`
//! so the browser build fetches them over HTTP.

mod character;
mod decals;
mod dump;
mod exits;
mod hud;
mod anim_cull;
mod refraction;
mod lighting;
mod map;
mod materials;
mod merge;
mod nav;
mod nameplate;
mod net;
mod npc;
mod particles;
mod skills;
mod surfaces;
mod ui;

use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use map::{CurrentMap, MapPlugin};

const DEFAULT_MAP: &str = "a0200_kingscity";
#[cfg(not(target_arch = "wasm32"))]
const SCREENSHOT_PATH: &str = "/tmp/claude-1000/dsor_shot.png";

/// Command-line (native) or URL query (web) options.
#[derive(Resource, Clone, Debug)]
struct Options {
    map: String,
    /// Save one screenshot once the map has loaded, then exit.
    screenshot: Option<String>,
    /// Camera position and look-at target, game frame.
    cam: Option<([f32; 3], [f32; 3])>,
    shadows: bool,
    /// Map viewer: place every NPC the level has.
    npcs: bool,
    /// Demo: spawn a dressed character (class, gender) at the map centre.
    character: Option<(u8, u8)>,
    /// Demo: the animation state to show it in.
    anim: Option<String>,
    /// Play online: the login server and the launcher's identity.
    net: Option<net::NetConfig>,
}

fn parse_cam(s: &str) -> Option<([f32; 3], [f32; 3])> {
    let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (v.len() == 6).then(|| ([v[0], v[1], v[2]], [v[3], v[4], v[5]]))
}

#[cfg(not(target_arch = "wasm32"))]
fn options() -> Options {
    let mut o = Options { map: DEFAULT_MAP.into(), screenshot: None, cam: None, shadows: true, npcs: false, character: None, anim: None, net: None };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => o.screenshot = Some(SCREENSHOT_PATH.into()),
            "--screenshot-to" => o.screenshot = args.next(),
            "--cam" => o.cam = args.next().as_deref().and_then(parse_cam),
            "--no-shadows" => o.shadows = false,
            "--npcs" => o.npcs = true,
            "--character" => {
                let class = args.next().and_then(|c| c.parse().ok()).unwrap_or(0);
                let gender = args.next().and_then(|g| g.parse().ok()).unwrap_or(0);
                o.character = Some((class, gender));
            }
            "--anim" => o.anim = args.next(),
            "--server" => {
                let login = args.next().unwrap_or_else(|| "127.0.0.1:2190".into());
                o.net = Some(net::NetConfig {
                    login,
                    account: "107909469".into(),
                    session: "f7d06d94-af66-4ed7-bdae-b98f8e049ceb".into(),
                    relay: None,
                    character: None,
                });
            }
            "--account" => {
                if let (Some(n), Some(a)) = (o.net.as_mut(), args.next()) {
                    n.account = a;
                }
            }
            "--sid" => {
                if let (Some(n), Some(sid)) = (o.net.as_mut(), args.next()) {
                    n.session = sid;
                }
            }
            "--char" => {
                if let Some(n) = o.net.as_mut() {
                    n.character = args.next().and_then(|c| c.parse().ok());
                }
            }
            m if !m.starts_with('-') => o.map = m.trim_end_matches(".map.json").into(),
            other => eprintln!("unknown argument {other}"),
        }
    }
    o
}

#[cfg(target_arch = "wasm32")]
fn options() -> Options {
    let mut o = Options { map: DEFAULT_MAP.into(), screenshot: None, cam: None, shadows: true, npcs: false, character: None, anim: None, net: None };
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    for pair in search.trim_start_matches('?').split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "map" if !v.is_empty() => o.map = v.into(),
            "cam" => o.cam = parse_cam(&v.replace("%2C", ",")),
            "noshadows" => o.shadows = false,
            "npcs" => o.npcs = true,
            // Diagnostics: ?flag=name switches one cost off (crate::flag).
            "flag" => {
                let _ = FLAGS.set(v.split(',').map(str::to_owned).collect());
            }
            "server" | "account" | "sid" | "relay" | "char" => {
                let n = o.net.get_or_insert_with(|| net::NetConfig {
                    login: "127.0.0.1:2190".into(),
                    account: String::new(),
                    session: String::new(),
                    relay: None,
                    character: None,
                });
                let v = v.replace("%3A", ":").replace("%2F", "/");
                match k {
                    "server" => n.login = v,
                    "account" => n.account = v,
                    "sid" => n.session = v,
                    "relay" => n.relay = Some(v),
                    _ => n.character = v.parse().ok(),
                }
            }
            _ => {}
        }
    }
    o
}

/// Where the asset folder is. Natively it is `<workspace>/assets` (overridable
/// with `DSOR_ASSETS`); in the browser it is `assets/` next to the page.
fn asset_root() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("DSOR_ASSETS")
            .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../assets").to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        "assets".to_string()
    }
}

/// Diagnostic switches: the web page's `flag=a,b` or DSOR_FLAGS=a,b natively.
/// noanim (no character animation), noshadows, nonpcs-shadow, noparticles.
pub static FLAGS: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();

pub fn flag(name: &str) -> bool {
    FLAGS
        .get_or_init(|| std::env::var("DSOR_FLAGS").map(|v| v.split(',').map(str::to_owned).collect()).unwrap_or_default())
        .contains(name)
}

/// The browser build on WebGL2 (the `webgl2` feature), whose limits some choices
/// follow; natively and on WebGPU they do not apply.
pub const WEBGL2: bool = cfg!(all(target_arch = "wasm32", not(feature = "webgpu")));

/// Whether the camera keeps a depth pre-pass (see setup).
fn depth_prepass() -> bool {
    if WEBGL2 { flag("depth") } else { !flag("nodepth") }
}

fn main() {
    // CONTRACT: the IO pool is created here, before TaskPoolPlugin (which keeps an
    //   existing pool), with a large stack. bevy_gltf's loader waits in a
    //   TaskPool::scope, and a thread waiting there runs other queued loads on top
    //   of its own stack: loads nest one inside another.
    // FAILURE (2026-10-06): with Kingshill's NPCs (~100 part files requested at
    //   once) "IO Task Pool has overflowed its stack" -- the core showed six nested
    //   load_gltf frames. The stack is only reserved, not committed.
    #[cfg(not(target_arch = "wasm32"))]
    bevy::tasks::IoTaskPool::get_or_init(|| {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        bevy::tasks::TaskPoolBuilder::new()
            .thread_name("IO Task Pool".into())
            .num_threads((cores / 4).clamp(1, 4))
            .stack_size(256 << 20)
            .build()
    });
    let opts = options();
    let mut app = App::new();
    if let Some(config) = &opts.net {
        app.insert_resource(config.clone());
    }
    if opts.npcs && opts.net.is_none() {
        app.insert_resource(npc::OfflineNpcs::default());
    }
    app
        .add_plugins(
            {
                let plugins = DefaultPlugins.build();
                // DSOR_LIKE_WEB=1 (native): run as the browser does -- one thread,
                // logic and rendering one after the other -- to measure a frame's
                // cost where it matters.
                #[cfg(not(target_arch = "wasm32"))]
                let plugins = if std::env::var("DSOR_LIKE_WEB").is_ok() {
                    plugins
                        .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
                        .set(bevy::app::TaskPoolPlugin {
                            task_pool_options: bevy::app::TaskPoolOptions::with_num_threads(1),
                        })
                } else {
                    plugins
                };
                plugins
            }
                .set(AssetPlugin {
                    file_path: asset_root(),
                    // The converted tree has no .meta files: asking for one per model
                    // was thousands of 404s in the browser.
                    meta_check: bevy::asset::AssetMetaCheck::Never,
                    ..default()
                })
                .set(bevy::log::LogPlugin { custom_layer: dump::log_layer, ..default() })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: format!("DSOR - {}", opts.map),
                        // Uncapped frame rate so the FPS readout means something.
                        present_mode: bevy::window::PresentMode::AutoNoVsync,
                        fit_canvas_to_parent: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin::default(),
            FreeCameraPlugin,
            materials::MaterialsPlugin,
            decals::DecalPlugin,
            particles::ParticlePlugin,
            npc::NpcPlugin,
            skills::SkillsPlugin,
            nameplate::NameplatePlugin,
            lighting::LightingPlugin,
            MapPlugin,
            character::CharacterPlugin,
            net::NetPlugin,
            nav::NavPlugin,
            hud::HudPlugin,
        ))
        .add_plugins(dump::DumpPlugin)
        .add_plugins(ui::UiPlugin)
        .add_plugins((anim_cull::AnimCullPlugin, refraction::RefractionPlugin, surfaces::SurfacesPlugin, exits::ExitsPlugin))
        .insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.08)))
        // The sun's one shadow cascade covers 60 units: 1 024 texels in the
        // browser (half bevy's default) is ~6 cm a texel.
        .insert_resource(bevy::light::DirectionalLightShadowMap {
            size: if flag("lowshadows") { 512 } else if WEBGL2 { 1024 } else { 2048 },
        })
        .insert_resource(opts.clone())
        .add_systems(Startup, setup);
    // CONTRACT: vertex and index slabs stay small in the browser. bevy's defaults
    //   grow a slab x1.5 up to 512 MiB, copying it each time; with Kingshill's
    //   ~140 MiB of static vertices ANGLE/Metal failed the allocation
    //   ("GL_OUT_OF_MEMORY ... Failed to allocate host memory", then
    //   CONTEXT_LOST) in Chrome on a Mac, the original build included.
    if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
        if cfg!(target_arch = "wasm32") {
            render.insert_resource(bevy::render::mesh::allocator::MeshAllocatorSettings {
                slab_allocator_settings: bevy::render::slab_allocator::SlabAllocatorSettings {
                    min_slab_size: 1 << 20,
                    max_slab_size: 64 << 20,
                    large_threshold: 32 << 20,
                    growth_factor: 1.5,
                },
                ..default()
            });
        }
    }
    app
        .add_systems(Update, (screenshot_when_loaded, demo_character))
        .run();
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>, opts: Res<Options>) {
    // Online, the server says which map; offline, the command line does.
    if opts.net.is_some() {
        // NetConfig is inserted before the app runs: net::connect reads it at Startup.
    } else {
        commands.insert_resource(CurrentMap::new(
            opts.map.clone(),
            asset_server.load(format!("maps/{}.map.json", opts.map)),
        ));
    }

    // Camera: left at the origin until the map frames it, unless --cam says where.
    let cam = match opts.cam {
        Some((p, t)) => Transform::from_translation(map::game_to_bevy(p))
            .looking_at(map::game_to_bevy(t), Vec3::Y),
        None => Transform::default(),
    };
    // Lights are clustered for a top-down view: they all lie within a short depth
    // of the camera, so few depth slices (bevy's advice for such games); the
    // default 4 096 clusters in 24 slices cost CPU for nothing.
    let clusters = bevy::light::cluster::ClusterConfig::FixedZ {
        total: 512,
        z_slices: 2,
        z_config: default(),
        dynamic_resizing: true,
    };
    let camera = commands.spawn((Camera3d::default(), clusters, cam)).id();
    // The depth of the opaque scene, for the 2018 shaders' soft edges: particles
    // (depthDensity), volume fog, water borders (crate::surfaces). Natively and on
    // WebGPU always; on WebGL2 on request (?flag=depth): it binds the depth
    // texture only without MSAA, and the pre-pass draws every opaque surface again.
    // WebGL2: one hardware 2x2 comparison per pixel. bevy's default Gaussian
    // filter (13 taps) on ANGLE/Metal took Kingshill from ~35 to ~1 FPS in
    // Chrome on an M1 Pro (GPU-bound, the CPU at 4 ms).
    if WEBGL2 {
        commands.entity(camera).insert(bevy::light::ShadowFilteringMethod::Hardware2x2);
    }
    if depth_prepass() {
        commands.entity(camera).insert(bevy::core_pipeline::prepass::DepthPrepass);
        if WEBGL2 {
            commands.entity(camera).insert(Msaa::Off);
        }
    }
    // Online the game camera follows the player (net::follow_camera); the free
    // fly camera is the map viewer's.
    if opts.net.is_none() {
        commands.entity(camera).insert(FreeCamera { walk_speed: 15.0, run_speed: 60.0, ..default() });
    }

    // The level's global light (crate::lighting sets its colour, strength and
    // direction from the level's data).
    commands.spawn((
        lighting::Sun,
        DirectionalLight { illuminance: 7_500.0, shadow_maps_enabled: opts.shadows, ..default() },
        // One cascade: the boundary of a second one sat at the camera's own distance
        // at full zoom-out and cut the view in two ("la vision est coupee en 2").
        bevy::light::CascadeShadowConfigBuilder {
            num_cascades: 1,
            maximum_distance: 60.0,
            ..default()
        }
        .build(),
        Transform::default().looking_to(Vec3::new(-0.6, -1.0, -0.45), Vec3::Y),
    ));
}

/// With `--screenshot`: wait until every model of the map is loaded, give the
/// renderer a few frames to upload, save one screenshot, then exit.
fn screenshot_when_loaded(
    mut commands: Commands,
    opts: Res<Options>,
    current: Option<Res<CurrentMap>>,
    asset_server: Res<AssetServer>,
    mut settled: Local<u32>,
    mut frames: Local<u32>,
    mut taken: Local<Option<u32>>,
    demo: Query<&character::CharacterAnim>,
) {
    let Some(path) = &opts.screenshot else { return };
    *frames += 1;
    if let Some(at) = *taken {
        // save_to_disk completes a few frames after the request; then quit.
        if *frames > at + 30 {
            commands.write_message(AppExit::Success);
        }
        return;
    }
    let Some(current) = current else { return };
    if !current.spawned {
        return;
    }
    if (opts.character.is_some() || opts.net.is_some()) && (demo.is_empty() || !demo.iter().all(|a| a.is_ready())) {
        return;
    }
    let (done, total) = current.progress(&asset_server);
    let loading = total - done;
    if loading > 0 && *frames < 6000 {
        return;
    }
    *settled += 1;
    // DSOR_SHOT_WAIT=<frames>: settle longer (two clients that must meet first).
    let wait = std::env::var("DSOR_SHOT_WAIT").ok().and_then(|w| w.parse().ok()).unwrap_or(60);
    if *settled < wait {
        return;
    }
    info!("screenshot -> {path} (frame {}, {loading} models still loading)", *frames);
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
    *taken = Some(*frames);
}

/// The skins each class wears in the demo: the converted char_sel_p1 outfits.
fn demo_equipment(class: u8) -> Vec<(u8, Vec<String>)> {
    let skins: &[&str] = match class {
        1 => &["unique_mage_shoulders_01_sargon", "unique_mage_helmet_01_sargon", "unique_mage_torso_01_sargon",
               "unique_mage_boots_02_kingshill", "mage_gloves_01", "unique_mage_2h_staff2h_01_heroic_bossdrop"],
        2 => &["unique_ranger_helmet_01_sargon", "unique_ranger_shoulders_01_sargon", "unique_ranger_torso_01_sargon",
               "unique_ranger_boots_02_lvl40", "ranger_gloves_05", "unique_ranger_rh_shortbow_sargon_01"],
        _ => &["unique_warrior_torso_01_sargon", "unique_warrior_shoulders_01_sargon", "unique_warrior_helmet_01_sargon",
               "warrior_boots_06", "unique_warrior_lh_shield_sargon_01", "unique_warrior_rh_mace_sargon_01"],
    };
    skins.iter().enumerate().map(|(i, s)| (i as u8, vec![s.to_string()])).collect()
}

/// `--character <class> <gender> [--anim State]`: one dressed character at the map
/// centre, the camera three metres in front of it.
fn demo_character(
    mut commands: Commands,
    opts: Res<Options>,
    current: Option<Res<CurrentMap>>,
    manifests: Res<Assets<map::MapManifest>>,
    mut done: Local<bool>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
    mut anims: Query<&mut character::CharacterAnim>,
) {
    let Some((class, gender)) = opts.character else { return };
    if let Some(state) = &opts.anim {
        for mut a in &mut anims {
            a.state = character::AnimState::Named(state.clone());
        }
    }
    if *done {
        return;
    }
    let Some(current) = current else { return };
    let Some(manifest) = manifests.get(&current.manifest) else { return };
    let at = map::game_to_bevy(manifest.center);
    let desc = character::CharacterDesc {
        class,
        gender,
        equipment: demo_equipment(class),
        armament: if class == 1 { 5 } else if class == 2 { 1 } else { 4 },
        ..Default::default()
    };
    character::spawn_character(&mut commands, desc, Transform::from_translation(at));
    if opts.cam.is_none() {
        for mut cam in &mut cameras {
            *cam = Transform::from_translation(at + Vec3::new(0.0, 1.6, 3.2)).looking_at(at + Vec3::Y * 1.0, Vec3::Y);
        }
    }
    *done = true;
}
