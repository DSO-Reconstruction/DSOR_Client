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
) {
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
        "{fps:.0} FPS ({ms:.2} ms)\nentities {entities:.0}\nmap {name}: {placed} placements, models {loaded}/{models}\nWASD/QE move, hold right mouse to look, shift run, wheel speed"
    );
    if let Ok(mut t) = text.single_mut() {
        t.0 = line;
    }
    // Also log it every 5 s, so native runs leave numbers in the terminal.
    let now = time.elapsed_secs_f64();
    if now - *last_log > 5.0 {
        *last_log = now;
        info!("{fps:.0} FPS ({ms:.2} ms), {entities:.0} entities, {name}: {placed} placements, models {loaded}/{models}");
    }
}
