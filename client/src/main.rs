//! DSOR client: renders one Drakensang Online map from the converted assets.
//!
//! Native:  `cargo run -p dsor-client --release -- [map] [--screenshot] [--cam x,y,z,tx,ty,tz]`
//! Web:     `index.html?map=a0302_wildforest`
//!
//! Assets come from the `assets/` folder at the workspace root (a symlink to the
//! DSO_Godot export, see `docs/assets.md`), always through Bevy's `AssetServer`
//! so the browser build fetches them over HTTP.

mod hud;
mod map;

use bevy::camera_controller::free_camera::{FreeCamera, FreeCameraPlugin};
use bevy::diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy::light::GlobalAmbientLight;
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
}

fn parse_cam(s: &str) -> Option<([f32; 3], [f32; 3])> {
    let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (v.len() == 6).then(|| ([v[0], v[1], v[2]], [v[3], v[4], v[5]]))
}

#[cfg(not(target_arch = "wasm32"))]
fn options() -> Options {
    let mut o = Options { map: DEFAULT_MAP.into(), screenshot: None, cam: None, shadows: false };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => o.screenshot = Some(SCREENSHOT_PATH.into()),
            "--screenshot-to" => o.screenshot = args.next(),
            "--cam" => o.cam = args.next().as_deref().and_then(parse_cam),
            "--shadows" => o.shadows = true,
            m if !m.starts_with('-') => o.map = m.trim_end_matches(".map.json").into(),
            other => eprintln!("unknown argument {other}"),
        }
    }
    o
}

#[cfg(target_arch = "wasm32")]
fn options() -> Options {
    let mut o = Options { map: DEFAULT_MAP.into(), screenshot: None, cam: None, shadows: false };
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    for pair in search.trim_start_matches('?').split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "map" if !v.is_empty() => o.map = v.into(),
            "cam" => o.cam = parse_cam(&v.replace("%2C", ",")),
            "shadows" => o.shadows = true,
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

fn main() {
    let opts = options();
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin { file_path: asset_root(), ..default() })
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
            MapPlugin,
            hud::HudPlugin,
        ))
        .insert_resource(ClearColor(Color::srgb(0.05, 0.06, 0.08)))
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 900.0,
            affects_lightmapped_meshes: true,
        })
        .insert_resource(opts)
        .add_systems(Startup, setup)
        .add_systems(Update, screenshot_when_loaded)
        .run();
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>, opts: Res<Options>) {
    commands.insert_resource(CurrentMap::new(
        opts.map.clone(),
        asset_server.load(format!("maps/{}.map.json", opts.map)),
    ));

    // Camera: left at the origin until the map frames it, unless --cam says where.
    let cam = match opts.cam {
        Some((p, t)) => Transform::from_translation(map::game_to_bevy(p))
            .looking_at(map::game_to_bevy(t), Vec3::Y),
        None => Transform::default(),
    };
    commands.spawn((
        Camera3d::default(),
        cam,
        FreeCamera { walk_speed: 15.0, run_speed: 60.0, ..default() },
    ));

    // Sun: high and from the side, so walls and props read in relief.
    commands.spawn((
        DirectionalLight { illuminance: 6_000.0, shadow_maps_enabled: opts.shadows, ..default() },
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.3), Vec3::Y),
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
    let (done, total) = current.progress(&asset_server);
    let loading = total - done;
    if loading > 0 && *frames < 6000 {
        return;
    }
    *settled += 1;
    if *settled < 60 {
        return;
    }
    info!("screenshot -> {path} (frame {}, {loading} models still loading)", *frames);
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
    *taken = Some(*frames);
}
