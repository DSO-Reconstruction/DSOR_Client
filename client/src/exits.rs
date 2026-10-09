//! Map exits: the glowing arrows on the ground that lead to another map.
//!
//! The 2018 client places them itself, from the level's `_Instance_InteractExit`
//! rows (tools/export_exits.py -> `maps/<map>.exits.json`): the server never
//! announces them. A click on one walks the player to it; once within its
//! PickingRange the client asks for the map with UnlockMapCommand (104), and the
//! server answers with a hand-off to the destination's map server
//! (dsor_proto Session::hand_off), which loads the new map (net.rs MapAssigned).
//! EVIDENCE: the real client's request, captured (session-maps1.jsonl frame 1074):
//!   8b 68 00 | 10 00 "a0302_wildforest" | 10 00 "a0302_wildforest" |
//!   0f 00 "exit_general_01" | 0 -- map id and exit URL are the row's ExitURL, the
//!   exit id its template Id. It came again every few ticks, between movement
//!   records, until the hand-off (frames 1084, 1090): the server ignores repeats.
//! UNVERIFIED: an exit whose EventSetRow is not -1 belongs to a map event (Kingshill
//!   stacks several on one spot); which events are running is not known here yet,
//!   so only the always-present exits are drawn. The click area (PICK_RADIUS) is
//!   not the client's own picking shape.

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::world_serialization::WorldAssetRoot;
use dsor_proto::commands::{player::UnlockMap, ClientCommand};
use serde::Deserialize;

use crate::map::{model_path, CurrentMap};
use crate::nav::{CurrentNav, NavMesh};
use crate::net::{LocalPlayer, Net, NetSystems};

#[derive(Deserialize, Debug, Clone)]
pub struct ExitRow {
    pub id: String,
    pub name: String,
    pub url: String,
    pub graphics: String,
    pub range: f32,
    pub event: i32,
    /// DirectX row-major 4x4 (the last row is the position): glam's column-major
    /// layout read as is, like the map's NPC placements.
    pub m: [f32; 16],
}

#[derive(Asset, TypePath, Deserialize, Debug)]
#[serde(transparent)]
pub struct MapExits(pub Vec<ExitRow>);

#[derive(Default, TypePath)]
pub struct MapExitsLoader;

impl AssetLoader for MapExitsLoader {
    type Asset = MapExits;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<MapExits, Self::Error> {
        let mut b = Vec::new();
        r.read_to_end(&mut b).await?;
        serde_json::from_slice(&b).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] {
        &["exits.json"]
    }
}

/// One exit drawn on the map.
#[derive(Component, Clone)]
pub struct Exit {
    pub row: ExitRow,
    pub at: Vec3,
}

/// A click on the cursor's ground point this close to an exit's centre is a click
/// on that exit.
const PICK_RADIUS: f32 = 2.0;
/// How often the request is repeated while the player stands in range.
const RESEND: f32 = 0.5;

#[derive(Resource, Default)]
struct ExitState {
    /// The map these exits belong to, and its exit file.
    map: Option<(String, Handle<MapExits>)>,
    spawned: bool,
    /// The exit the player clicked, until the map changes.
    chosen: Option<Exit>,
    /// Seconds until the request may go again.
    resend: f32,
}

pub struct ExitsPlugin;

impl Plugin for ExitsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<MapExits>()
            .register_asset_loader(MapExitsLoader)
            .init_resource::<ExitState>()
            .add_systems(Update, (load_exits, spawn_exits).chain().run_if(resource_exists::<CurrentMap>))
            .add_systems(Update, (click_exit, use_exit).chain().after(NetSystems));
    }
}

fn load_exits(mut state: ResMut<ExitState>, current: Res<CurrentMap>, assets: Res<AssetServer>) {
    if state.map.as_ref().is_some_and(|(m, _)| *m == current.name) {
        return;
    }
    state.map = Some((current.name.clone(), assets.load(format!("maps/{}.exits.json", current.name))));
    state.spawned = false;
    state.chosen = None;
}

fn spawn_exits(
    mut commands: Commands,
    mut state: ResMut<ExitState>,
    current: Res<CurrentMap>,
    exits: Res<Assets<MapExits>>,
    assets: Res<AssetServer>,
) {
    if state.spawned {
        return;
    }
    // Children of the map root: they go with the map.
    let Some(root) = current.root else { return };
    let Some((_, handle)) = &state.map else { return };
    let Some(list) = exits.get(handle) else {
        // A map without exits has no file.
        if assets.load_state(handle).is_failed() {
            state.spawned = true;
        }
        return;
    };
    let mut drawn = 0;
    for row in list.0.iter().filter(|r| r.event == -1) {
        let tf = Transform::from_matrix(Mat4::from_cols_array(&row.m));
        let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(model_path(&row.graphics)));
        commands.spawn((
            Name::new(format!("exit {} -> {}", row.name, row.url)),
            Exit { row: row.clone(), at: tf.translation },
            WorldAssetRoot(scene),
            tf,
            ChildOf(root),
        ));
        drawn += 1;
    }
    info!("{} exits on {} ({} drawn)", list.0.len(), current.name, drawn);
    state.spawned = true;
}

/// A left click on an exit: walk to it, and remember it for use_exit.
#[allow(clippy::too_many_arguments)]
fn click_exit(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    nav: Option<Res<CurrentNav>>,
    meshes: Res<Assets<NavMesh>>,
    exits: Query<&Exit>,
    mut players: Query<(&Transform, &mut LocalPlayer)>,
    mut state: ResMut<ExitState>,
) {
    if !buttons.just_pressed(MouseButton::Left) || keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        return;
    }
    let Ok((tf, mut player)) = players.single_mut() else { return };
    let (Ok(window), Ok((camera, cam_tf))) = (windows.single(), cameras.single()) else { return };
    let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
    let Some(point) = crate::net::cursor_ground(window, camera, cam_tf, mesh, tf.translation) else { return };
    let flat = |a: Vec3, b: Vec3| Vec2::new(a.x - b.x, a.z - b.z).length();
    let hit = exits
        .iter()
        .filter(|e| flat(e.at, point) <= PICK_RADIUS && (e.at.y - point.y).abs() < 3.0)
        .min_by(|a, b| flat(a.at, point).total_cmp(&flat(b.at, point)));
    state.chosen = hit.cloned();
    if let Some(exit) = hit {
        let target = mesh.and_then(|m| m.nearest(exit.at, 3.0)).unwrap_or(exit.at);
        player.target = Some(target);
        state.resend = 0.0;
    }
}

/// The clicked exit, once the player is within its PickingRange: ask the server
/// for its map, again every RESEND seconds until it moves us.
fn use_exit(
    time: Res<Time>,
    net: Option<NonSendMut<Net>>,
    players: Query<&Transform, With<LocalPlayer>>,
    exits: Query<&Exit>,
    mut state: ResMut<ExitState>,
) {
    // DSOR_TEST_EXIT=<map>: use this map's exit at once, wherever the player is
    // (testing the map switch without walking there).
    let test = std::env::var("DSOR_TEST_EXIT").ok();
    if state.chosen.is_none() && players.single().is_ok() {
        if let Some(url) = &test {
            state.chosen = exits.iter().find(|e| e.row.url == *url).cloned();
        }
    }
    let Some(exit) = state.chosen.clone() else { return };
    let Some(mut net) = net else { return };
    let Ok(tf) = players.single() else { return };
    state.resend -= time.delta_secs();
    let reach = if test.is_some() { f32::INFINITY } else { exit.row.range.max(1.0) };
    if Vec2::new(tf.translation.x - exit.at.x, tf.translation.z - exit.at.z).length() > reach || state.resend > 0.0 {
        return;
    }
    state.resend = RESEND;
    info!("exit {} -> {} ({})", exit.row.id, exit.row.url, exit.row.name);
    net.send(&ClientCommand::UnlockMap(UnlockMap {
        map_id: exit.row.url.clone(),
        exit_url: exit.row.url.clone(),
        exit_id: exit.row.id.clone(),
        unknown_flag_3: false,
    }));
}
