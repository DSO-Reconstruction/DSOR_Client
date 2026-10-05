//! The game server, inside Bevy: the session, the transport, and what the world
//! messages do to the scene.
//!
//! Natively the transport is UDP straight to the server; in the browser it is the
//! WebSocket relay (crates/relay). Either way the RakNet connection is the client's
//! own (crates/raknet), and `dsor_proto::session::Session` walks login -> character
//! service -> map.
//!
//! Frames: positions the server DESCRIBES (NewPlayer, NewMonster, ...) are in the
//! map's frame, the one the placement manifests use. Movement records are on the wire
//! frame: world units * 128 relative to the map header's centre, which is the
//! manifest's `center` (experimental dsor/mapheader.frame_origin reads the same
//! header field). SEE: wire_to_game, game_to_wire.

use std::f32::consts::TAU;

use bevy::input::mouse::MouseButton;
use bevy::picking::mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use dsor_proto::commands::{decode_message, movement, ClientCommand, ServerCommand};
use dsor_proto::identity::Credentials;
use dsor_proto::session::{Session, SessionEvent};
use dsor_raknet::Reliability;
use dsor_transport::Transport;

use crate::character::{redress, spawn_character, AnimState, Character, CharacterAnim, CharacterDesc, CharacterLibrary, ItemSkins};
use crate::map::{CurrentMap, MapManifest, MapRoot};
use crate::nav::{CurrentNav, NavMesh};

/// How far up or down one step may go (stairs, slopes).
const MAX_STEP: f32 = 1.2;
/// Seconds ANOTHER player keeps running after they last moved: just over the 40 ms
/// between two of their records. SEE: move_remotes. The player's own run stops the
/// moment they stop ("elle devrait s'arreter quand j'arrete de marcher").
const RUN_GRACE: f32 = 0.12;
/// A click this close to the player is no destination: holding the button on
/// yourself must not make the run flicker.
const CLICK_DEAD_ZONE: f32 = 0.4;

/// Game tick, as the 2018 client counts it.
const TICK_SECONDS: f32 = 0.04;
/// _Template_Player.RunSpeed, the same 5.0 for every class (static.db4).
/// UNKNOWN: the unit -- tuned by eye against the map until confirmed.
pub const RUN_SPEED: f32 = 5.0;
/// The duration every captured client MoveCommand carries (session-walk4: 20).
const MOVE_DURATION: u16 = 20;
/// The game camera, from _Globals (static.db4): CamRotX -45, CamRotY -45,
/// DefCamFov 34, Min/Max/DefCamHeight 12/25/25, CamOffsetZ 0.5.
/// INFERRED: the height is the camera's altitude above the player and the FOV is
/// vertical -- the client's ObserverCameraProperty (0x4E9B84) was not traced to the
/// end; checked by eye against the 2018 client.
pub const CAM_PITCH_DEG: f32 = -45.0;
pub const CAM_YAW_DEG: f32 = -45.0;
pub const CAM_FOV_DEG: f32 = 34.0;
pub const CAM_HEIGHT_MIN: f32 = 12.0;
pub const CAM_HEIGHT_MAX: f32 = 25.0;
pub const CAM_HEIGHT_DEFAULT: f32 = 25.0;
pub const CAM_OFFSET_Z: f32 = 0.5;

/// The camera's current zoom (its height above the player).
#[derive(Resource)]
pub struct CameraZoom(pub f32);

/// Where and as whom to connect. Absent: the client is a map viewer.
#[derive(Resource, Clone, Debug)]
pub struct NetConfig {
    /// The login server, "host:port".
    pub login: String,
    pub account: String,
    pub session: String,
    /// The WebSocket relay, required in the browser ("ws://host:2290").
    pub relay: Option<String>,
    /// Which character to pick (its id); the first of the roster when None.
    pub character: Option<u32>,
}

/// CONTRACT: a NON-SEND resource: in the browser the transport holds a WebSocket
///   and JS closures, which cannot cross threads; natively it costs nothing.
pub struct Net {
    session: Session,
    transport: Option<Transport>,
    clock_offset: Option<i64>,
    relay: Option<String>,
    wanted_character: Option<u32>,
    pub local_actor: Option<u32>,
    /// The map frame's centre (wire = (game - centre) * 128).
    pub centre: Option<Vec3>,
    pending_local: Option<(CharacterDesc, Vec3, f32)>,
    /// Actors already asked about (ActorRequest), so each is asked once.
    requested: std::collections::HashSet<u32>,
    /// Other players to spawn, despawn, or move, applied by `apply_remotes`.
    remote_spawns: Vec<(u32, CharacterDesc, Vec3, f32, String)>,
    remote_gone: Vec<u32>,
    remote_moves: Vec<(u32, Vec3, f32, bool)>,
    /// What this player wears, as the inventory names it: (slots, item template).
    local_worn: Option<Vec<(Vec<i8>, String)>>,
    /// Another player re-dressed (RemotePlayerInfo): actor, skins, armament.
    remote_redress: Vec<(u32, Vec<(u8, Vec<String>)>, i8)>,
}

/// Another player, drawn from NewRemotePlayer and moved by their MoveCommands.
#[derive(Component)]
pub struct RemotePlayer {
    pub actor: u32,
    pub name: String,
    /// Where their last record says they are heading.
    pub target: Vec3,
    pub facing: f32,
    /// Seconds since this player last moved, so the run cycle is not cut between
    /// two of their records.
    pub still_for: f32,
}

/// The player this client controls.
#[derive(Component)]
pub struct LocalPlayer {
    pub actor: u32,
    /// Where the player is walking to, game frame.
    pub target: Option<Vec3>,
    /// Body facing, radians around +Y.
    pub facing: f32,
    last_sent_tick: u32,
}

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        // DSOR_ZOOM=<height>: start at another zoom (debugging screenshots).
        let zoom = std::env::var("DSOR_ZOOM").ok().and_then(|z| z.parse().ok()).unwrap_or(CAM_HEIGHT_DEFAULT);
        app.insert_resource(CameraZoom(zoom))
            .add_systems(Startup, connect.run_if(resource_exists::<NetConfig>))
            .add_systems(
                PreUpdate,
                pump.run_if(net_exists),
            )
            .add_systems(
                Update,
                (debug_hierarchy, spawn_local, dress, apply_remotes, move_remotes, click_to_move, walk_local, zoom_camera, follow_camera, hide_occluders, send_moves)
                    .chain()
                    .run_if(net_exists),
            );
    }
}

fn now_ms(time: &Time<Real>) -> u64 {
    (time.elapsed_secs_f64() * 1000.0) as u64
}

fn net_exists(net: Option<NonSend<Net>>) -> bool {
    net.is_some()
}

/// Exclusive: a non-send resource can only be inserted through the World.
fn connect(world: &mut World) {
    let Some(config) = world.get_resource::<NetConfig>().cloned() else { return };
    let now = now_ms(world.resource::<Time<Real>>());
    let Some(credentials) = Credentials::parse(&config.account, &config.session) else {
        error!("bad credentials: account {:?}, session {:?}", config.account, config.session);
        return;
    };
    let Some(login) = dsor_proto::session::resolve(&config.login, 2190) else {
        error!("cannot resolve the login server {}", config.login);
        return;
    };
    // One RakNet GUID per run, as the real client keeps one for all its connections.
    let guid = 0x0660_0000_0000_0000 | (credentials.account as u64) << 8 | 0x42;
    let session = Session::new(login, credentials, guid, now);
    world.insert_non_send(Net {
        session,
        transport: None,
        clock_offset: None,
        relay: config.relay.clone(),
        wanted_character: config.character,
        local_actor: None,
        centre: None,
        pending_local: None,
        requested: Default::default(),
        remote_spawns: Vec::new(),
        remote_gone: Vec::new(),
        remote_moves: Vec::new(),
        local_worn: None,
        remote_redress: Vec::new(),
    });
}

/// CONTRACT: leaving says goodbye. FAILURE: closing the window left the player in
///   the world for the server's 60 s silence timeout, so the next login found a
///   double of itself standing on the arrival point (2026-10-06).
impl Drop for Net {
    fn drop(&mut self) {
        // Any time will do: the goodbye is flushed at once.
        self.session.disconnect(u64::MAX / 2);
        let mut out = Vec::new();
        self.session.drain_outgoing(&mut out);
        if let Some(t) = self.transport.as_mut() {
            for d in &out {
                let _ = t.send(d);
            }
        }
    }
}

impl Net {
    /// The server's game tick now.
    pub fn server_tick(&self, now: u64) -> u32 {
        let local = self.session.local_tick(now) as i64;
        (local + self.clock_offset.unwrap_or(0)).max(0) as u32
    }

    pub fn wire_to_game(&self, x: i16, elevation: i16, y: i16) -> Vec3 {
        let c = self.centre.unwrap_or(Vec3::ZERO);
        Vec3::new(x as f32 / 128.0, elevation as f32 / 128.0, y as f32 / 128.0) + c
    }

    pub fn game_to_wire(&self, p: Vec3) -> (i16, i16, i16) {
        let c = self.centre.unwrap_or(Vec3::ZERO);
        let w = (p - c) * 128.0;
        let clamp = |v: f32| v.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        (clamp(w.x), clamp(w.y), clamp(w.z))
    }

    /// A game command to the map server, reliable ordered.
    pub fn send(&mut self, command: &ClientCommand) {
        let (bytes, bits) = command.encode();
        self.session.send_command(&bytes, bits);
    }
}

#[allow(clippy::too_many_arguments)]
fn pump(
    mut commands: Commands,
    mut net: NonSendMut<Net>,
    time: Res<Time<Real>>,
    current: Option<Res<CurrentMap>>,
    roots: Query<Entity, With<MapRoot>>,
    assets: Res<AssetServer>,
    manifests: Res<Assets<MapManifest>>,
) {
    let now = now_ms(&time);
    let net = &mut *net;
    if let Some(t) = net.transport.as_mut() {
        let mut inbox = Vec::new();
        t.poll(&mut inbox);
        for d in inbox {
            net.session.receive(&d, now);
        }
    }
    net.session.update(now);
    while let Some(event) = net.session.poll_event() {
        match event {
            SessionEvent::Connect { target, farewell } => {
                if let Some(t) = net.transport.as_mut() {
                    for d in &farewell {
                        let _ = t.send(d);
                    }
                }
                match Transport::connect(&target.to_string(), net.relay.as_deref()) {
                    Ok(t) => net.transport = Some(t),
                    Err(e) => error!("cannot reach {target}: {e}"),
                }
                info!("connecting to {target}");
            }
            SessionEvent::Service(name) => info!("service: {name}"),
            SessionEvent::Roster(roster) => {
                let pick = net.wanted_character.or_else(|| roster.first().map(|e| e.character));
                info!("characters: {:?}", roster.iter().map(|e| (&e.name, e.character, e.level)).collect::<Vec<_>>());
                if let Some(c) = pick {
                    net.session.select_character(c, now);
                }
            }
            SessionEvent::MapAssigned { name, rule_set } => {
                info!("map {name} (rule set {rule_set})");
                if current.as_ref().is_some_and(|c| c.name == name) {
                    continue;
                }
                for root in &roots {
                    commands.entity(root).despawn();
                }
                net.centre = None;
                commands.insert_resource(CurrentMap::new(name.clone(), assets.load(format!("maps/{name}.map.json"))));
            }
            SessionEvent::Clock(tick) => {
                let local = net.session.local_tick(now) as i64;
                net.clock_offset = Some(tick as i64 - local);
            }
            SessionEvent::Commands { data, bits } => match decode_message(&data, bits) {
                Ok(list) => {
                    for (command, actor) in list {
                        on_command(net, command, actor);
                    }
                }
                Err(e) => warn!("undecodable {:#04x} message ({} bytes): {e:?}", data.first().copied().unwrap_or(0), data.len()),
            },
            SessionEvent::Disconnected(reason) => warn!("disconnected: {reason:?}"),
        }
    }
    if net.centre.is_none() {
        if let Some(manifest) = current.as_ref().and_then(|c| manifests.get(&c.manifest)) {
            net.centre = Some(Vec3::from(manifest.center));
        }
    }
    let mut out = Vec::new();
    net.session.drain_outgoing(&mut out);
    if let Some(t) = net.transport.as_mut() {
        for d in &out {
            let _ = t.send(d);
        }
    }
}

fn on_command(net: &mut Net, command: ServerCommand, actor: Option<u32>) {
    match command {
        ServerCommand::NewPlayer(p) => {
            let desc = CharacterDesc {
                class: p.parts[0],
                gender: p.parts[1],
                hair: p.parts[2],
                beard: p.parts[3],
                body: p.parts[4],
                variation: p.parts[5],
                equipment: Vec::new(),
                // The client tracks its own armament from the inventory; 0 is what
                // the server sends (dsor/newplayer.py DEFAULT_ARMAMENT).
                armament: p.rank.max(0),
            };
            info!("I am {} ({}), actor {:?}, at {:?}", p.name, desc.animation_set(), actor, p.position);
            net.local_actor = actor;
            net.pending_local = Some((desc, Vec3::from(p.position), p.heading));
        }
        // The real client asks about every actor the vicinity names
        // (session-walk4: a burst of 8b/34 after each 0x85/125).
        ServerCommand::ActorsEnterVicinity(v) => {
            debug!("vicinity +{:x?}", v.actors);
            for a in v.actors {
                if Some(a) != net.local_actor && net.requested.insert(a) {
                    net.send(&ClientCommand::ActorRequest(dsor_proto::commands::player::ActorRequest { actor: a }));
                }
            }
        }
        ServerCommand::ActorsLeftVicinity(v) => {
            for a in v.actors {
                net.requested.remove(&a);
                net.remote_gone.push(a);
            }
        }
        ServerCommand::NewRemotePlayer(p) => {
            let Some(actor) = actor else { return };
            let desc = CharacterDesc {
                class: p.parts[0],
                gender: p.parts[1],
                hair: p.parts[2],
                beard: p.parts[3],
                body: p.parts[4],
                variation: p.parts[5],
                equipment: p.equipment.iter().map(|w| (w.slot, w.skin_parts.clone())).collect(),
                armament: p.armament.max(0),
            };
            info!("{} ({}) is here, actor {actor:#x}, {} worn slot(s)", p.name, desc.animation_set(), desc.equipment.len());
            net.remote_spawns.push((actor, desc, Vec3::from(p.position), p.heading, p.name));
        }
        ServerCommand::InventoryInfo(inv) => {
            let worn = inv
                .slots
                .iter()
                .filter_map(|(item, slots)| {
                    inv.items.iter().find(|i| i.id == *item).map(|i| (slots.clone(), i.template.clone()))
                })
                .collect::<Vec<_>>();
            info!("wearing {} item(s)", worn.len());
            net.local_worn = Some(worn);
        }
        ServerCommand::RemotePlayerInfo(p) => {
            if let Some(a) = actor {
                let equipment = p.equipment.iter().map(|w| (w.slot, w.skin_parts.clone())).collect();
                net.remote_redress.push((a, equipment, p.armament.max(0)));
            }
        }
        ServerCommand::DiscardPlayer(_) => {
            if let Some(a) = actor {
                net.requested.remove(&a);
                net.remote_gone.push(a);
            }
        }
        ServerCommand::Move(m) => {
            if let Some(a) = actor {
                if Some(a) != net.local_actor {
                    let at = net.wire_to_game(m.x, m.elevation, m.y);
                    let facing = m.facing as f32 / 256.0 * TAU;
                    net.remote_moves.push((a, at, facing, m.duration > 0));
                }
            }
        }
        _ => {}
    }
}

fn apply_remotes(
    mut commands: Commands,
    mut net: NonSendMut<Net>,
    mut remotes: Query<(Entity, &mut RemotePlayer)>,
    nav: Option<Res<CurrentNav>>,
    meshes: Res<Assets<NavMesh>>,
) {
    let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
    for actor in std::mem::take(&mut net.remote_gone) {
        for (e, r) in &remotes {
            if r.actor == actor {
                commands.entity(e).despawn();
            }
        }
    }
    for (actor, desc, at, heading, name) in std::mem::take(&mut net.remote_spawns) {
        for (e, r) in &remotes {
            if r.actor == actor {
                commands.entity(e).despawn();
            }
        }
        let at = mesh.and_then(|m| m.nearest(at, 4.0)).unwrap_or(at);
        let e = spawn_character(&mut commands, desc, Transform::from_translation(at).with_rotation(Quat::from_rotation_y(heading)));
        commands.entity(e).insert(RemotePlayer { actor, name, target: at, facing: heading, still_for: 1.0 });
    }
    for (actor, at, facing, _moving) in std::mem::take(&mut net.remote_moves) {
        for (_, mut r) in &mut remotes {
            if r.actor == actor {
                let at = mesh.and_then(|m| m.ground(at.x, at.z, at.y, 3.0).map(|h| Vec3::new(at.x, h, at.z))).unwrap_or(at);
                r.target = at;
                r.facing = facing;
            }
        }
    }
}

/// Other players run toward their last reported position at the run speed, and
/// snap when they are far off (a teleport, a map entry).
fn move_remotes(time: Res<Time>, mut remotes: Query<(&mut Transform, &mut RemotePlayer, &mut CharacterAnim)>) {
    for (mut tf, mut r, mut anim) in &mut remotes {
        let to = r.target - tf.translation;
        let flat = Vec2::new(to.x, to.z).length();
        let step = RUN_SPEED * 1.25 * time.delta_secs();
        let moving = flat > 0.05;
        if flat > 8.0 {
            tf.translation = r.target;
        } else if moving {
            tf.translation += to * (step / flat).min(1.0);
            tf.rotation = Quat::from_rotation_y(to.x.atan2(to.z));
        } else {
            tf.rotation = Quat::from_rotation_y(r.facing);
        }
        // CONTRACT: a short grace before standing: records arrive 25 a second and a
        //   remote player reaches each one a little early, so without it the run
        //   restarted at every record ("les animations ne se jouent pas entierement").
        r.still_for = if moving { 0.0 } else { r.still_for + time.delta_secs() };
        let wanted = if r.still_for < RUN_GRACE { AnimState::Run } else { AnimState::Idle };
        if anim.state != wanted {
            anim.state = wanted;
        }
    }
}

fn spawn_local(
    mut commands: Commands,
    mut net: NonSendMut<Net>,
    existing: Query<Entity, With<LocalPlayer>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some((desc, at, heading)) = net.pending_local.take() else { return };
    for e in &existing {
        commands.entity(e).despawn();
    }
    let entity = spawn_character(&mut commands, desc, Transform::from_translation(at).with_rotation(Quat::from_rotation_y(heading)));
    // DSOR_MARKER=1: a bright pillar on the player, to find them while debugging.
    if std::env::var("DSOR_MARKER").is_ok() {
        let marker = commands
            .spawn((
                Mesh3d(meshes.add(Cylinder::new(0.3, 30.0))),
                MeshMaterial3d(materials.add(StandardMaterial { emissive: LinearRgba::rgb(20.0, 0.0, 0.0), ..default() })),
                Transform::from_xyz(0.0, 15.0, 0.0),
            ))
            .id();
        commands.entity(entity).add_child(marker);
    }
    commands.entity(entity).insert(LocalPlayer {
        actor: net.local_actor.unwrap_or(0),
        target: None,
        facing: heading,
        last_sent_tick: 0,
    });
}

/// Left click on the ground: walk there. The ground is the navigation mesh -- stairs
/// and upper floors included -- or, before it has loaded, the plane at the player's feet.
#[allow(clippy::too_many_arguments)]
fn click_to_move(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    nav: Option<Res<CurrentNav>>,
    meshes: Res<Assets<NavMesh>>,
    mut players: Query<(&Transform, &mut LocalPlayer)>,
) {
    if !buttons.pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    let Ok((camera, cam_tf)) = cameras.single() else { return };
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else { return };
    let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
    for (tf, mut player) in &mut players {
        let hit = mesh.and_then(|m| m.raycast(ray)).or_else(|| {
            ray.intersect_plane(tf.translation, InfinitePlane3d::new(Vec3::Y)).map(|d| ray.get_point(d))
        });
        if let Some(point) = hit {
            let flat = Vec2::new(point.x - tf.translation.x, point.z - tf.translation.z).length();
            if flat > CLICK_DEAD_ZONE {
                player.target = Some(point);
            }
        }
    }
}

/// Walk toward the target on the navigation mesh: every step takes the ground's
/// height, and a step that would leave the mesh slides along its edge or stops.
fn walk_local(
    time: Res<Time>,
    nav: Option<Res<CurrentNav>>,
    meshes: Res<Assets<NavMesh>>,
    mut players: Query<(&mut Transform, &mut LocalPlayer, &mut CharacterAnim)>,
    mut snapped: Local<bool>,
    mut autowalked: Local<bool>,
) {
    let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
    for (mut tf, mut player, mut anim) in &mut players {
        // On arrival, stand on the ground nearest where the server put us.
        if let (Some(mesh), false) = (mesh, *snapped) {
            if let Some(p) = mesh.nearest(tf.translation, 4.0) {
                tf.translation = p;
            }
            *snapped = true;
        }
        // DSOR_AUTOWALK=dx,dz: walk that far once, on arrival (testing without a mouse).
        if *snapped && player.target.is_none() && player.last_sent_tick > 0 {
            if let Some((dx, dz)) = std::env::var("DSOR_AUTOWALK").ok().and_then(|v| {
                let mut it = v.split(',').filter_map(|x| x.trim().parse::<f32>().ok());
                Some((it.next()?, it.next()?))
            }) {
                if !*autowalked {
                    *autowalked = true;
                    player.target = Some(tf.translation + Vec3::new(dx, 0.0, dz));
                }
            }
        }
        let mut moving = false;
        if let Some(target) = player.target {
            let here = tf.translation;
            let mut to = target - here;
            to.y = 0.0;
            let step = RUN_SPEED * time.delta_secs();
            let dir = to.normalize_or_zero();
            let wish = if to.length() <= step { Vec3::new(target.x, here.y, target.z) } else { here + dir * step };
            let next = match mesh {
                None => Some(wish),
                Some(m) => {
                    let on = |p: Vec3| m.ground(p.x, p.z, here.y, MAX_STEP).map(|h| Vec3::new(p.x, h, p.z));
                    on(wish)
                        .or_else(|| on(Vec3::new(wish.x, here.y, here.z)))
                        .or_else(|| on(Vec3::new(here.x, here.y, wish.z)))
                }
            };
            match next {
                Some(p) if (p - here).length_squared() > 1e-8 => {
                    tf.translation = p;
                    if dir != Vec3::ZERO {
                        player.facing = dir.x.atan2(dir.z);
                    }
                    moving = true;
                    if Vec2::new(target.x - p.x, target.z - p.z).length() < 0.05 {
                        player.target = None;
                    }
                }
                // Blocked: a wall, a cliff, the edge of the world.
                _ => player.target = None,
            }
        }
        tf.rotation = Quat::from_rotation_y(player.facing);
        let wanted = if moving { AnimState::Run } else { AnimState::Idle };
        if anim.state != wanted {
            anim.state = wanted;
        }
    }
}

fn zoom_camera(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, mut zoom: ResMut<CameraZoom>) {
    for w in wheel.read() {
        let step = if w.unit == bevy::input::mouse::MouseScrollUnit::Line { w.y } else { w.y / 40.0 };
        zoom.0 = (zoom.0 - step).clamp(CAM_HEIGHT_MIN, CAM_HEIGHT_MAX);
    }
}

/// The DSO camera: fixed pitch and yaw, looking at the player from `zoom` above.
fn follow_camera(
    zoom: Res<CameraZoom>,
    players: Query<&Transform, With<LocalPlayer>>,
    mut cameras: Query<(&mut Transform, &mut Projection), (With<Camera3d>, Without<LocalPlayer>)>,
) {
    let Ok(player) = players.single() else { return };
    let rotation = Quat::from_euler(EulerRot::YXZ, CAM_YAW_DEG.to_radians(), CAM_PITCH_DEG.to_radians(), 0.0);
    let back = rotation * Vec3::Z;
    let target = player.translation + Vec3::new(0.0, 0.0, CAM_OFFSET_Z);
    // The zoom is the camera's distance to the player along its view.
    // FAILURE (2026-10-06): read as an altitude, the camera sat 35 units away and the
    // player was a speck ("je vois pas le perso que je joue").
    let distance = zoom.0;
    for (mut cam, mut projection) in &mut cameras {
        *cam = Transform::from_translation(target + back * distance).with_rotation(rotation);
        if let Projection::Perspective(p) = &mut *projection {
            p.fov = CAM_FOV_DEG.to_radians();
        }
    }
}

/// One MoveCommand per game tick, as the 2018 client sends them: where the player
/// is, speed 0 (the 2018 client never fills it), heading and facing in 256ths of a
/// turn, the tick, and a 20-tick duration.
fn send_moves(mut net: NonSendMut<Net>, time: Res<Time<Real>>, mut players: Query<(&Transform, &mut LocalPlayer)>) {
    let Ok((tf, mut player)) = players.single_mut() else { return };
    if net.centre.is_none() {
        return;
    }
    let tick = net.server_tick(now_ms(&time));
    if tick == player.last_sent_tick {
        return;
    }
    player.last_sent_tick = tick;
    let (x, elevation, y) = net.game_to_wire(tf.translation);
    let turn = |r: f32| ((r.rem_euclid(TAU) / TAU) * 256.0).round() as u32 as u8;
    let command = ClientCommand::Move(movement::Move {
        x,
        elevation,
        y,
        speed: 0,
        heading: turn(player.facing),
        facing: turn(player.facing),
        start_tick: tick,
        duration: MOVE_DURATION,
    });
    let (bytes, bits) = command.encode();
    net.session.send_command_with(&bytes, bits, Reliability::UnreliableSequenced);
    let _ = TICK_SECONDS;
}

/// Decor between the camera and the player is hidden while it is in the way, as the
/// game does with roofs and walls (the player spawns under Kingshill's roofs).
/// INFERRED: the client's own occluder fading was not traced; this hides outright.
fn hide_occluders(
    mut ray_cast: MeshRayCast,
    players: Query<(Entity, &GlobalTransform), With<LocalPlayer>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    children: Query<&Children>,
    mut visibility: Query<&mut Visibility>,
    // Effect surfaces, decal boxes, particles: never occluders, and some are
    // hidden on purpose (crate::materials) -- unhiding them would show them.
    effects: Query<(), With<bevy::light::NotShadowCaster>>,
    mut hidden: Local<Vec<Entity>>,
) {
    if std::env::var("DSOR_NO_OCCLUDERS").is_ok() {
        return;
    }
    let Ok((player, at)) = players.single() else { return };
    let Ok(cam) = cameras.single() else { return };
    let own: std::collections::HashSet<Entity> = children.iter_descendants(player).collect();
    let filter = |e: Entity| !own.contains(&e) && !effects.contains(e);
    let settings = MeshRayCastSettings::default()
        .with_visibility(RayCastVisibility::Any)
        .with_filter(&filter);
    let feet = at.translation();
    let mut now_hidden = Vec::new();
    for offset in [Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.8, 0.0), Vec3::new(0.6, 1.0, 0.0), Vec3::new(-0.6, 1.0, 0.0), Vec3::new(0.0, 0.3, 0.0)] {
        let goal = feet + offset;
        let from = cam.translation();
        let Ok(dir) = Dir3::new(goal - from) else { continue };
        let reach = (goal - from).length() - 0.3;
        let ray = Ray3d::new(from, dir);
        for (entity, hit) in ray_cast.cast_ray(ray, &settings) {
            if hit.distance < reach && hit.point.y > feet.y + 0.4 {
                now_hidden.push(*entity);
            }
        }
    }
    for e in hidden.drain(..) {
        if !now_hidden.contains(&e) {
            if let Ok(mut v) = visibility.get_mut(e) {
                *v = Visibility::Inherited;
            }
        }
    }
    for &e in &now_hidden {
        if let Ok(mut v) = visibility.get_mut(e) {
            *v = Visibility::Hidden;
        }
    }
    *hidden = now_hidden;
}

/// The ArmamentState the hands give (the 2018 rule the experimental server's
/// World.armament_of mirrors): a weapon in both hand slots is TwoHand (5); a weapon
/// with something in the off hand 4; a weapon alone 3; nothing 0.
/// INFERRED: Small/Large weapons are not told apart here (no item categories yet).
fn armament(worn: &[(Vec<i8>, String)]) -> i8 {
    let right = worn.iter().find(|(s, _)| s.contains(&5));
    let left = worn.iter().find(|(s, _)| s.contains(&6));
    match (right, left) {
        (Some((s, _)), _) if s.contains(&6) => 5,
        (Some(_), Some(_)) => 4,
        (Some(_), None) => 3,
        _ => 0,
    }
}

/// Dress the player from the inventory, and other players from RemotePlayerInfo.
#[allow(clippy::type_complexity)]
fn dress(
    mut commands: Commands,
    mut net: NonSendMut<Net>,
    library: Option<Res<CharacterLibrary>>,
    skins: Res<Assets<ItemSkins>>,
    mut locals: Query<(Entity, &mut Character, &mut CharacterAnim, Option<&Children>), (With<LocalPlayer>, Without<RemotePlayer>)>,
    mut remotes: Query<(Entity, &RemotePlayer, &mut Character, &mut CharacterAnim, Option<&Children>), Without<LocalPlayer>>,
) {
    for (actor, equipment, armament) in std::mem::take(&mut net.remote_redress) {
        for (e, r, mut c, mut a, kids) in &mut remotes {
            if r.actor == actor {
                redress(&mut commands, e, &mut c, &mut a, kids, equipment.clone(), armament);
            }
        }
    }
    let Some(table) = library.as_ref().and_then(|l| skins.get(&l.item_skins)) else { return };
    let Ok((e, mut c, mut a, kids)) = locals.single_mut() else { return };
    let Some(worn) = net.local_worn.take() else { return };
    let equipment = worn
        .iter()
        .filter_map(|(slots, template)| {
            table.0.get(template).map(|parts| (slots.first().copied().unwrap_or(0).max(0) as u8, parts.clone()))
        })
        .collect();
    redress(&mut commands, e, &mut c, &mut a, kids, equipment, armament(&worn));
}

/// DSOR_DEBUG_HIER=1: once, four seconds in, where the player's body really is.
pub fn debug_hierarchy(
    time: Res<Time>,
    mut done: Local<bool>,
    players: Query<(Entity, &GlobalTransform), With<LocalPlayer>>,
    children: Query<&Children>,
    names: Query<&Name>,
    globals: Query<&GlobalTransform>,
    skinned: Query<&bevy::mesh::skinning::SkinnedMesh>,
    parents: Query<&ChildOf>,
) {
    if *done || std::env::var("DSOR_DEBUG_HIER").is_err() || time.elapsed_secs() < 6.0 {
        return;
    }
    *done = true;
    let Ok((player, at)) = players.single() else { return };
    info!("HIER player {player:?} at {:?}", at.translation());
    for c in children.get(player).into_iter().flatten() {
        let n = names.get(*c).map(|n| n.as_str().to_owned()).unwrap_or_default();
        info!("HIER  child {c:?} {n:?} global {:?} has_global {}", globals.get(*c).map(|g| g.translation()).ok(), globals.contains(*c));
    }
    let mut shown = 0;
    for e in children.iter_descendants(player) {
        if let Ok(s) = skinned.get(e) {
            let j0 = s.joints.first().copied();
            let jp = j0.and_then(|j| parents.get(j).ok().map(|p| p.parent()));
            info!(
                "HIER  skinned {e:?} global {:?}; joint0 {:?} {:?} at {:?}; joint0 under player: {}",
                globals.get(e).map(|g| g.translation()).ok(),
                j0,
                j0.and_then(|j| names.get(j).ok().map(|n| n.as_str().to_owned())),
                j0.and_then(|j| globals.get(j).ok().map(|g| g.translation())),
                j0.map(|j| children.iter_descendants(player).any(|d| d == j)).unwrap_or(false),
            );
            let _ = jp;
            shown += 1;
            if shown > 4 {
                break;
            }
        }
    }
}
