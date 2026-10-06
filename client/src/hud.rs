//! On-screen readout: FPS, frame time, entity count, map name and load progress.

use bevy::diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::map::CurrentMap;

#[derive(Component)]
struct HudText;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud).add_systems(Update, update_hud);
    }
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
) {
    // What the server last said about our health and skill resource (132).
    let vitals = net
        .as_ref()
        .map(|n| match (n.health, n.resource) {
            (Some(h), Some(r)) => format!("\nvie {h:.0}, ressource {r:.1}"),
            _ => "\nvie/ressource: pas encore recues du serveur".to_owned(),
        })
        .unwrap_or_default();
    // Surfaces drawn: what the renderer prepares, one by one, on the CPU. Counted
    // once a second (a full pass over every mesh each frame is a cost of its own).
    if time.elapsed_secs_f64() - counted.0 > 1.0 {
        *counted = (time.elapsed_secs_f64(), meshes.iter().filter(|v| v.get()).count(), meshes.iter().count());
    }
    let (_, drawn, total) = *counted;
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
        "{fps:.0} FPS ({ms:.2} ms)\nentities {entities:.0}, surfaces drawn {drawn}/{total}{vitals}\nmap {name}: {placed} placements, models {loaded}/{models}\nWASD/QE move, hold right mouse to look, shift run, wheel speed"
    );
    if let Ok(mut t) = text.single_mut() {
        t.0 = line;
    }
    // Also log it every 5 s, so native runs leave numbers in the terminal.
    let now = time.elapsed_secs_f64();
    if now - *last_log > 5.0 {
        *last_log = now;
        info!("{fps:.0} FPS ({ms:.2} ms), {entities:.0} entities, {drawn}/{total} surfaces drawn, {name}: {placed} placements, models {loaded}/{models}");
    }
}
