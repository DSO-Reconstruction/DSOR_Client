//! DSOR client: renders one Drakensang Online map from the converted assets.
//!
//! Native:  `cargo run -p dsor-client --release -- [map] [--screenshot] [--cam x,y,z,tx,ty,tz]`
//! Web:     `index.html?map=a0302_wildforest`
//!
//! Assets come from the `assets/` folder at the workspace root (a symlink to the
//! DSO_Godot export, see `docs/assets.md`), always through Bevy's `AssetServer`
//! so the browser build fetches them over HTTP.

mod actors;
mod audio;
mod diagnostics;
mod gameplay;
#[cfg(not(target_arch = "wasm32"))]
mod level_editor;
mod network;
mod rendering;
mod screens;
mod ui;
mod world;

use actors::{appearance, character, monsters, nameplate, npc, picking};
use diagnostics::{debug, dump, hud};
use gameplay::{skills, stats};
use network::{chat, net};
use rendering::{
    anim_cull, clamp, decals, draw_mode, highlight, lighting, materials, model_variant,
    nebula_bloom, occlusion, particles, refraction, shadow_views, surfaces,
};
#[cfg(not(target_arch = "wasm32"))]
use screens::login;
use screens::{loading, selection};
use ui::{combat_text, input as ui_input};
use world::{drops, exits, map, nav, props};

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
    login_screen: bool,
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
    /// Play offline (class, gender): a character of one's own without a server
    /// (crate::network::net::start_offline). The debug map starts one by itself.
    play: Option<(u8, u8)>,
}

/// Present while the in-client account screen owns input and the viewport.
#[derive(Resource)]
pub struct LoginGate;

/// Offline play: `--play` / `?play=`, or the debug map with nobody else to play.
fn offline_play(world: &mut World) {
    let opts = world.resource::<Options>().clone();
    if opts.net.is_some() || opts.login_screen {
        return;
    }
    let play = opts.play.or((opts.map == "debug" && opts.character.is_none()).then_some((1, 0)));
    if let Some((class, gender)) = play {
        info!("offline play: class {class}, gender {gender}");
        net::start_offline(world, class, gender);
    }
}

fn parse_cam(s: &str) -> Option<([f32; 3], [f32; 3])> {
    let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (v.len() == 6).then(|| ([v[0], v[1], v[2]], [v[3], v[4], v[5]]))
}

#[cfg(not(target_arch = "wasm32"))]
fn options() -> Options {
    let mut o = Options { login_screen: true, map: DEFAULT_MAP.into(), screenshot: None, cam: None, shadows: true, npcs: false, character: None, anim: None, net: None, play: None };
    let mut force_login = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--login" => force_login = true,
            "--offline" => o.login_screen = false,
            "--screenshot" => o.screenshot = Some(SCREENSHOT_PATH.into()),
            "--screenshot-to" => o.screenshot = args.next(),
            "--cam" => o.cam = args.next().as_deref().and_then(parse_cam),
            "--no-shadows" => o.shadows = false,
            "--npcs" => o.npcs = true,
            "--character" => {
                o.login_screen = false;
                let class = args.next().and_then(|c| c.parse().ok()).unwrap_or(0);
                let gender = args.next().and_then(|g| g.parse().ok()).unwrap_or(0);
                o.character = Some((class, gender));
            }
            "--anim" => o.anim = args.next(),
            "--play" => {
                o.login_screen = false;
                let class = args.next().and_then(|c| c.parse().ok()).unwrap_or(1);
                let gender = args.next().and_then(|g| g.parse().ok()).unwrap_or(0);
                o.play = Some((class, gender));
            }
            "--server" => {
                let login = args.next().unwrap_or_else(|| "127.0.0.1:2190".into());
                o.net = Some(net::NetConfig {
                    login,
                    account: String::new(),
                    session: String::new(),
                    user: None,
                    password: None,
                    account_api: None,
                    relay: None,
                    character: None,
                    chat: None,
                });
            }
            "--account" => {
                if let (Some(n), Some(a)) = (o.net.as_mut(), args.next()) {
                    n.account = a;
                }
            }
            // Log in at the account service (release261): --user NAME --password PW
            // [--account-api host:port].
            "--user" => {
                if let Some(n) = o.net.as_mut() {
                    n.user = args.next();
                }
            }
            "--password" => {
                if let Some(n) = o.net.as_mut() {
                    n.password = args.next();
                }
            }
            "--account-api" => {
                if let Some(n) = o.net.as_mut() {
                    n.account_api = args.next();
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
            m if !m.starts_with('-') => {
                o.map = m.trim_end_matches(".map.json").into();
                o.login_screen = false;
            },
            other => eprintln!("unknown argument {other}"),
        }
    }
    if let Some(n) = &o.net {
        o.login_screen = n.user.is_none() && (n.account.is_empty() || n.session.is_empty());
    }
    o.login_screen |= force_login;
    o
}

#[cfg(target_arch = "wasm32")]
fn options() -> Options {
    let mut o = Options { login_screen: false, map: DEFAULT_MAP.into(), screenshot: None, cam: None, shadows: true, npcs: false, character: None, anim: None, net: None, play: None };
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
            // ?play=<class>[,<gender>]: an offline character of one's own.
            "play" => {
                let mut it = v.split("%2C").flat_map(|x| x.split(',')).filter_map(|x| x.parse::<u8>().ok());
                o.play = Some((it.next().unwrap_or(1), it.next().unwrap_or(0)));
            }
            // Diagnostics: ?flag=name switches one cost off (crate::flag).
            "flag" => {
                let _ = FLAGS.set(v.split(',').map(str::to_owned).collect());
            }
            "server" | "account" | "sid" | "relay" | "char" | "user" | "pass" | "api" => {
                let n = o.net.get_or_insert_with(|| net::NetConfig {
                    login: "127.0.0.1:2190".into(),
                    account: String::new(),
                    session: String::new(),
                    user: None,
                    password: None,
                    account_api: None,
                    relay: None,
                    character: None,
                    chat: None,
                });
                let v = percent_decode(v);
                match k {
                    "server" => n.login = v,
                    "account" => n.account = v,
                    "sid" => n.session = v,
                    "relay" => n.relay = Some(v),
                    "user" => n.user = Some(v),
                    "pass" => n.password = Some(v),
                    "api" => n.account_api = Some(v),
                    _ => n.character = v.parse().ok(),
                }
            }
            _ => {}
        }
    }
    o
}

/// A URL query value, %XX-decoded ('+' is a space).
#[cfg(target_arch = "wasm32")]
fn percent_decode(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 3 <= b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(x) => out.push(x),
                    None => out.extend_from_slice(&b[i..i + 3]),
                }
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The folder the client keeps its data in (`assets/`, `game-data/`; the
/// release's `converter/`). A release (a `converter/` beside the executable)
/// uses its own folder wherever it is; a build run from inside the workspace
/// it was compiled in (`target/...`) uses that workspace, as before; any other
/// executable (a copy) uses its own folder (docs/release.md).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn install_root() -> std::path::PathBuf {
    static ROOT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| install_root_for(concat!(env!("CARGO_MANIFEST_DIR"), "/.."), std::env::current_exe().ok())).clone()
}

/// [`install_root`] for a given workspace and executable. Paths are compared
/// resolved but returned as given: a resolved Windows path (`\\?\D:\...`)
/// takes no forward slashes, and the asset paths are joined with them.
#[cfg(not(target_arch = "wasm32"))]
fn install_root_for(workspace: &str, exe: Option<std::path::PathBuf>) -> std::path::PathBuf {
    let resolved = |p: &std::path::Path| std::fs::canonicalize(p).ok();
    let folder = exe.as_deref().and_then(std::path::Path::parent).map(std::path::Path::to_path_buf);
    match (resolved(std::path::Path::new(workspace)), folder) {
        (_, Some(folder)) if folder.join("converter").is_dir() => folder,
        (Some(ws), Some(folder)) if resolved(&folder).is_some_and(|f| f.starts_with(&ws)) => workspace.into(),
        (_, Some(folder)) => folder,
        (_, None) => workspace.into(),
    }
}

/// Where the asset folder is. Natively it is `assets/` in [`install_root`]
/// (overridable with `DSOR_ASSETS`); in the browser it is `assets/` next to the page.
pub(crate) fn asset_root() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("DSOR_ASSETS").unwrap_or_else(|_| install_root().join("assets").to_string_lossy().into_owned())
    }
    #[cfg(target_arch = "wasm32")]
    {
        "assets".to_string()
    }
}

/// Diagnostic switches: the web page's `flag=a,b` or DSOR_FLAGS=a,b natively.
/// noanim (no character animation), noshadows, nonpcs-shadow, pointshadows (level lights cast
/// shadows; the original's do not), noparticles, norim,
/// linearlight (the light summed in linear space instead of the original's gamma sum),
/// noshadowfloor (shadow removes the sun's share only, not half the whole light as the original),
/// noao (no ambient occlusion), aolinear / aosquared / ao13 (Bevy's GTAO instead of the original's pe_ssao),
/// linearscene (Bevy's linear scene target instead of the original's gamma blending),
/// nospecular (no sun specular), playerhole (the old dithered hole around the player behind walls),
/// hiddenglow (the player behind walls drawn as a glow; the original showed none behind the palace wall),
/// camfocus (offline viewer: the ambience zone at the game camera's focus, not the camera),
/// shadowcull (sun shadows from front faces only, as Bevy draws them by default).
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
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::args().any(|a| a == "--editor") {
        level_editor::desktop();
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(directory) = std::env::args().skip_while(|a| a != "--level-editor").nth(1) {
        level_editor::run(directory.into());
        return;
    }
    let opts = options();
    let mut app = App::new();
    // Outside debug builds a command on an entity already despawned (an effect
    // ending the frame its actor goes) is logged, not fatal: one such race
    // crashed the game (2026-10-18). Debug builds keep panicking to show them.
    #[cfg(not(debug_assertions))]
    app.set_error_handler(bevy::ecs::error::warn);
    rendering::graphics::configure_native_dlss(&mut app);
    app.register_asset_source(bevy::asset::io::AssetSourceId::Default, model_variant::source(asset_root()));
    if let Some(config) = opts.net.as_ref().filter(|_| !opts.login_screen) {
        app.insert_resource(config.clone());
    }
    if opts.login_screen {
        app.insert_resource(LoginGate);
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
                    // Opt-in diagnostic: test scheduler overhead independently
                    // of rendering pipelining and the asset-loading pools.
                    let mut pools = bevy::app::TaskPoolOptions::default();
                    if let Some(threads) = std::env::var("DSOR_COMPUTE_THREADS").ok()
                        .and_then(|value| value.parse::<usize>().ok())
                        .filter(|threads| *threads > 0)
                    {
                        let available = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
                        let threads = threads.min(available);
                        pools.compute.min_threads = threads;
                        pools.compute.max_threads = threads;
                    }
                    plugins.set(bevy::app::TaskPoolPlugin { task_pool_options: pools })
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
                        title: if opts.login_screen { "DSOR - Login".into() } else { format!("DSOR - {}", opts.map) },
                        // Uncapped frame rate so the FPS readout means something.
                        present_mode: bevy::window::PresentMode::AutoNoVsync,
                        fit_canvas_to_parent: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(model_variant::BlockTexturesPlugin)
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
        .add_plugins((dump::DumpPlugin, shadow_views::ShadowViewsPlugin, draw_mode::DrawModePlugin::default()))
        .add_plugins(diagnostics::lighting_menu::LightingMenuPlugin)
        .add_plugins(ui::UiPlugin)
        .add_plugins(rendering::graphics::GraphicsPlugin)
        .add_plugins(ui_input::UiInputPlugin)
        .add_plugins(loading::LoadingPlugin)
        .add_plugins(selection::SelectionPlugin)
        .add_plugins((anim_cull::AnimCullPlugin, refraction::RefractionPlugin, surfaces::SurfacesPlugin, exits::ExitsPlugin, monsters::MonstersPlugin, crate::actors::mounts::MountsPlugin, crate::actors::pets::PetsPlugin, combat_text::CombatTextPlugin, debug::DebugPlugin, stats::StatsPlugin, clamp::ClampPlugin, nebula_bloom::NebulaBloomPlugin, occlusion::OcclusionPlugin))
        .add_plugins((highlight::HighlightPlugin, picking::PickingPlugin, chat::ChatPlugin, props::PropsPlugin, drops::DropsPlugin, rendering::hidden_pixels::HiddenPixelsPlugin, audio::GameAudioPlugin))
        .insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.08)))
        // The sun's one shadow cascade covers 60 units: 1 024 texels in the
        // browser (half bevy's default) is ~6 cm a texel.
        .insert_resource(bevy::light::DirectionalLightShadowMap {
            size: if flag("lowshadows") { 512 } else if WEBGL2 { 1024 } else { 2048 },
        })
        .insert_resource(opts.clone())
        .add_systems(Startup, (setup, offline_play));
    #[cfg(not(target_arch = "wasm32"))]
    app.add_plugins(login::LoginPlugin);
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
    if opts.net.is_some() || opts.login_screen {
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
    let camera = commands.spawn((Camera3d::default(), rendering::graphics::GameGraphics, clusters, cam)).id();
    // The depth of the opaque scene, for the 2018 shaders' soft edges: particles
    // (depthDensity), volume fog, water borders (crate::rendering::surfaces). Natively and on
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
    // Player anti-aliasing, SSAO and sharpening are applied by GraphicsPlugin.
    // Online the game camera follows the player (net::follow_camera); the free
    // fly camera is the map viewer's.
    if opts.net.is_none() && !opts.login_screen {
        commands.entity(camera).insert(FreeCamera { walk_speed: 15.0, run_speed: 60.0, ..default() });
    }

    // The level's global light (crate::rendering::lighting sets its colour, strength and
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
    loading: Res<loading::LoadingScreen>,
    current: Option<Res<CurrentMap>>,
    asset_server: Res<AssetServer>,
    mut settled: Local<u32>,
    mut frames: Local<u32>,
    mut taken: Local<Option<u32>>,
    demo: Query<&character::CharacterAnim>,
    outfits: Query<&character::Character>,
) {
    let Some(path) = &opts.screenshot else { return };
    *frames += 1;
    if let Some(at) = *taken {
        // DSOR_SHOT_BURST=<n>: n more shots, one per frame (<path>.<i>.png), to
        // see what changes from frame to frame (a flicker).
        let burst = std::env::var("DSOR_SHOT_BURST").ok().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
        let i = *frames - at;
        if i <= burst {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(format!("{path}.{i}.png")));
        }
        // save_to_disk completes a few frames after the request; then quit.
        if *frames > at + 30 + burst {
            commands.write_message(AppExit::Success);
        }
        return;
    }
    if loading.active() { return; }
    // Also allow a capture of the native account form before a map is loaded.
    let login_wait = std::env::var("DSOR_SHOT_WAIT").ok().and_then(|w| w.parse().ok()).unwrap_or(120);
    if opts.login_screen && current.is_none() && *frames > login_wait {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
        *taken = Some(*frames);
        return;
    }
    let Some(current) = current else { return };
    if !current.spawned {
        return;
    }
    if (opts.character.is_some() || opts.net.is_some() || opts.play.is_some())
        && (demo.is_empty() || !demo.iter().all(|a| a.is_ready()) || !outfits.iter().all(character::Character::outfit_ready)) {
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
fn demo_equipment(class: u8) -> Vec<appearance::Equipment> {
    let skins: &[(u8, &str)] = match class {
        1 => &[(1, "unique_mage_shoulders_01_sargon"), (0, "unique_mage_helmet_01_sargon"), (2, "unique_mage_torso_01_sargon"),
               (4, "unique_mage_boots_02_kingshill"), (3, "mage_gloves_01"), (5, "unique_mage_2h_staff2h_01_heroic_bossdrop")],
        2 => &[(0, "unique_ranger_helmet_01_sargon"), (1, "unique_ranger_shoulders_01_sargon"), (2, "unique_ranger_torso_01_sargon"),
               (4, "unique_ranger_boots_02_lvl40"), (3, "ranger_gloves_05"), (5, "unique_ranger_rh_shortbow_sargon_01")],
        _ => &[(2, "unique_warrior_torso_01_sargon"), (1, "unique_warrior_shoulders_01_sargon"), (0, "unique_warrior_helmet_01_sargon"),
               (4, "warrior_boots_06"), (6, "unique_warrior_lh_shield_sargon_01"), (5, "unique_warrior_rh_mace_sargon_01")],
    };
    skins.iter().map(|(slot, s)| appearance::Equipment { slot: *slot, skin_parts: vec![s.to_string()], colour: None }).collect()
}

/// `--character <class> <gender> [--anim State]`: one dressed character at the map
/// centre, the camera three metres in front of it.
fn demo_character(
    mut commands: Commands,
    opts: Res<Options>,
    current: Option<Res<CurrentMap>>,
    manifests: Res<Assets<map::MapManifest>>,
    mut done: Local<bool>,
    mut cameras: Query<&mut Transform, (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::path::PathBuf;

    /// docs/release.md: a build inside its workspace keeps using the workspace's
    /// folders; a released or copied executable keeps its data beside itself.
    #[test]
    fn a_released_executable_keeps_its_data_beside_it() {
        let base = std::env::temp_dir().join(format!("dsor-install-root-{}", std::process::id()));
        let (workspace, release) = (base.join("ws"), base.join("DSOR"));
        for folder in [workspace.join("client"), workspace.join("target/release"), release.clone()] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let from = workspace.join("client/..").to_string_lossy().into_owned();
        let built = workspace.join("target/release/dsor-client.exe");
        assert_eq!(super::install_root_for(&from, Some(built)), PathBuf::from(&from));
        let shipped = release.join("dsor-client.exe");
        assert_eq!(super::install_root_for(&from, Some(shipped.clone())), release);
        let elsewhere = base.join("gone/client/..").to_string_lossy().into_owned();
        assert_eq!(super::install_root_for(&elsewhere, Some(shipped)), release, "the build machine's workspace is not there");
        // A release unpacked inside the workspace (to test it) is still a release.
        let inside = workspace.join(".local/DSOR");
        std::fs::create_dir_all(inside.join("converter")).unwrap();
        assert_eq!(super::install_root_for(&from, Some(inside.join("dsor-client.exe"))), inside);
        assert_eq!(super::install_root_for(&from, None), PathBuf::from(&from));
        let _ = std::fs::remove_dir_all(base);
    }
}
