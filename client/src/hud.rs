//! On-screen readout: FPS, frame time, entity count, map name and load progress.

use bevy::diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::map::CurrentMap;

#[derive(Component)]
struct HudText;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_systems(Update, update_hud)
            .add_systems(First, cpu_frame_start)
            .add_systems(Last, cpu_main_end);
        if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render.add_systems(bevy::render::Render, cpu_render_end.in_set(bevy::render::RenderSystems::Cleanup));
        }
    }
}

/// CPU time of a frame, without the wait for the display: from the main
/// schedule's start to the end of rendering (one thread in the browser, and
/// natively with DSOR_LIKE_WEB). Milliseconds as f64 bits.
mod cpu {
    use std::sync::atomic::{AtomicU64, Ordering};
    static START: AtomicU64 = AtomicU64::new(0);
    pub static MAIN: AtomicU64 = AtomicU64::new(0);
    pub static TOTAL: AtomicU64 = AtomicU64::new(0);
    pub fn now() -> f64 {
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::window().and_then(|w| w.performance()).map(|p| p.now()).unwrap_or(0.0)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::sync::OnceLock;
            static T0: OnceLock<std::time::Instant> = OnceLock::new();
            T0.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
        }
    }
    pub fn start() {
        START.store(now().to_bits(), Ordering::Relaxed);
    }
    pub fn since_start() -> f64 {
        now() - f64::from_bits(START.load(Ordering::Relaxed))
    }
    pub fn store(slot: &AtomicU64, ms: f64) {
        // Smoothed: 90 % old, 10 % new.
        let old = f64::from_bits(slot.load(Ordering::Relaxed));
        slot.store((old * 0.9 + ms * 0.1).to_bits(), Ordering::Relaxed);
    }
    pub fn get(slot: &AtomicU64) -> f64 {
        f64::from_bits(slot.load(Ordering::Relaxed))
    }
}

fn cpu_frame_start() {
    cpu::start();
}
fn cpu_main_end() {
    cpu::store(&cpu::MAIN, cpu::since_start());
}
fn cpu_render_end() {
    cpu::store(&cpu::TOTAL, cpu::since_start());
}

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        HudText,
        Text::new("loading..."),
        TextFont { font_size: FontSize::Px(16.0), ..default() },
        TextColor(Color::srgb(0.9, 1.0, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(8),
            ..default()
        },
    ));
}

fn update_hud(
    diagnostics: Res<DiagnosticsStore>,
    current: Option<Res<CurrentMap>>,
    asset_server: Res<AssetServer>,
    mut text: Query<&mut Text, With<HudText>>,
    mut last_log: Local<f64>,
    time: Res<Time>,
    meshes: Query<&ViewVisibility, With<Mesh3d>>,
    mut counted: Local<(f64, usize, usize)>,
    net: Option<NonSend<crate::net::Net>>,
    anims: Query<&crate::character::CharacterAnim>,
) {
    let animating = anims.iter().filter(|a| a.is_animating()).count();
    let characters = anims.iter().count();
    // What the server last said about our health and skill resource (132).
    let vitals = net
        .as_ref()
        .map(|n| match (n.health, n.resource) {
            (Some(h), Some(r)) => format!("\nhealth {h:.0}, resource {r:.1}"),
            _ => "\nhealth/resource: not received from the server yet".to_owned(),
        })
        .unwrap_or_default();
    // Surfaces drawn: what the renderer prepares, one by one, on the CPU. Counted
    // once a second (a full pass over every mesh each frame is a cost of its own).
    if time.elapsed_secs_f64() - counted.0 > 1.0 {
        *counted = (time.elapsed_secs_f64(), meshes.iter().filter(|v| v.get()).count(), meshes.iter().count());
    }
    let (_, drawn, total) = *counted;
    let (cpu_main, cpu_total) = (cpu::get(&cpu::MAIN), cpu::get(&cpu::TOTAL));
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let entities = diagnostics
        .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
        .and_then(|d| d.value())
        .unwrap_or(0.0);
    let (name, placed, loaded, models) = match &current {
        Some(c) => {
            let (done, total) = c.progress(&asset_server);
            (c.name.as_str(), c.instances, done, total)
        }
        None => ("-", 0, 0, 0),
    };
    let line = format!(
        "{fps:.0} FPS ({ms:.2} ms), cpu {cpu_total:.1} ms (logic {cpu_main:.1})\nentities {entities:.0}, surfaces drawn {drawn}/{total}{vitals}\nmap {name}: {placed} placements, models {loaded}/{models}"
    );
    if let Ok(mut t) = text.single_mut() {
        t.0 = line;
    }
    // Also log it every 5 s, so native runs leave numbers in the terminal.
    let now = time.elapsed_secs_f64();
    if now - *last_log > 5.0 {
        *last_log = now;
        info!("{fps:.0} FPS ({ms:.2} ms), cpu {cpu_total:.1} ms (logic {cpu_main:.1}), {entities:.0} entities, {drawn}/{total} surfaces drawn, {animating}/{characters} animated, {name}: {placed} placements, models {loaded}/{models}");
    }
}
