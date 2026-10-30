use super::*;
use std::time::Duration;

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<Time>().init_resource::<Time<Real>>()
        .init_resource::<Assets<NavMesh>>()
        .add_systems(Update, (apply_remotes, move_remotes).chain());
    start_offline(app.world_mut(), 1, 0);
    app
}

fn actor(app: &mut App, at: Vec3) -> Entity {
    app.world_mut().spawn((Transform::from_translation(at), CharacterAnim::default(),
        crate::actors::monsters::tests::monster(41, 100),
        RemotePlayer { actor: 41, target: at, ..default() })).id()
}

fn step(app: &mut App) {
    app.world_mut().resource_mut::<Time>().advance_by(Duration::from_secs_f32(0.04));
    app.update();
}

fn ground(app: &mut App) {
    let triangles = [0.0, 10.0].into_iter().flat_map(|y| {
        let [a, b, c, d] = [Vec3::new(-10., y, -10.), Vec3::new(10., y, -10.),
            Vec3::new(10., y, 10.), Vec3::new(-10., y, 10.)];
        [[a, b, c], [a, c, d]]
    }).collect();
    let mesh = app.world_mut().resource_mut::<Assets<NavMesh>>().add(NavMesh::new(triangles));
    app.insert_resource(CurrentNav(mesh));
}

#[test]
fn older_records_cannot_reverse_motion_or_poison_speed() {
    let mut app = app();
    let e = actor(&mut app, Vec3::ZERO);
    app.world_mut().non_send_mut::<Net>().remote_moves.extend([
        (41, Vec3::ZERO, 0., 100, None), (41, Vec3::X, 0.5, 104, None),
        (41, -Vec3::X * 3., 2., 102, None),
    ]);
    step(&mut app);
    let r = app.world().get::<RemotePlayer>(e).unwrap();
    assert_eq!(r.target, Vec3::X);
    assert_eq!(r.last_record, Some((Vec3::X, 104)));
    assert_eq!(r.facing, 0.5);
    assert!((r.speed - 6.25).abs() < 1e-5);
    // A reliable stop at the same tick can refine the walking sample.
    app.world_mut().non_send_mut::<Net>().remote_moves.push((41, Vec3::X * 1.1, 0.6, 104, None));
    step(&mut app);
    assert_eq!(app.world().get::<RemotePlayer>(e).unwrap().target.x, 1.1);
}

#[test]
fn record_ticks_wrap_without_accepting_pre_wrap_packets() {
    let mut app = app();
    let e = actor(&mut app, Vec3::ZERO);
    app.world_mut().non_send_mut::<Net>().remote_moves.extend([
        (41, Vec3::ZERO, 0., u32::MAX - 1, None), (41, Vec3::X, 0., 2, None),
        (41, -Vec3::X, 0., u32::MAX, None),
    ]);
    step(&mut app);
    assert_eq!(app.world().get::<RemotePlayer>(e).unwrap().last_record, Some((Vec3::X, 2)));
}

#[test]
fn stationary_monsters_ground_when_nav_arrives_even_during_a_cast() {
    let mut app = app();
    let lower = actor(&mut app, Vec3::new(0., -2.58, 0.));
    let bridge = actor(&mut app, Vec3::new(2., 9., 0.));
    app.world_mut().get_mut::<RemotePlayer>(lower).unwrap().held = 3.;
    step(&mut app);
    assert_eq!(app.world().get::<Transform>(lower).unwrap().translation.y, -2.58);
    ground(&mut app);
    step(&mut app);
    for (e, y) in [(lower, 0.), (bridge, 10.)] {
        assert_eq!(app.world().get::<Transform>(e).unwrap().translation.y, y);
        assert_eq!(app.world().get::<RemotePlayer>(e).unwrap().target.y, y);
    }
}

#[test]
fn vertical_only_stop_corrections_are_applied() {
    let mut app = app();
    let e = actor(&mut app, Vec3::ZERO);
    app.world_mut().non_send_mut::<Net>().remote_moves.push((41, Vec3::Y, 0., 100, None));
    step(&mut app);
    assert_eq!(app.world().get::<Transform>(e).unwrap().translation, Vec3::Y);
}

#[test]
fn casting_monster_settles_at_server_stop_without_losing_attack_pose() {
    let mut app = app();
    let e = actor(&mut app, Vec3::ZERO);
    let pose = AnimState::Named("Attack".into());
    app.world_mut().get_mut::<CharacterAnim>(e).unwrap().state = pose.clone();
    {
        let mut r = app.world_mut().get_mut::<RemotePlayer>(e).unwrap();
        r.target = Vec3::X;
        r.held = 3.;
        r.held_until_tick = Some(175);
    }
    for _ in 0..8 { step(&mut app); }
    assert_eq!(app.world().get::<Transform>(e).unwrap().translation, Vec3::X);
    assert_eq!(app.world().get::<CharacterAnim>(e).unwrap().state, pose);
    app.world_mut().non_send_mut::<Net>().remote_moves.push((41, Vec3::X * 2., 1., 176, None));
    step(&mut app);
    let r = app.world().get::<RemotePlayer>(e).unwrap();
    assert_eq!(r.held, 0.);
    assert_eq!(r.held_until_tick, None);
    assert!(app.world().get::<Transform>(e).unwrap().translation.x > 1.);
}

#[test]
fn grounding_never_changes_authoritative_horizontal_position() {
    let mut app = app();
    ground(&mut app);
    let nav = app.world().resource::<CurrentNav>();
    let mesh = app.world().resource::<Assets<NavMesh>>().get(&nav.0).unwrap();
    let outside = Vec3::new(20., -2., 0.);
    assert_eq!(remote_ground_position(Some(mesh), outside), outside);
    let inside = Vec3::new(2., -2., 3.);
    assert_eq!(remote_ground_position(Some(mesh), inside), Vec3::new(2., 0., 3.));
}

fn monster_snapshot(health: u64) -> ServerCommand {
    use dsor_proto::commands::actors::{Faction, NewMonster, HOSTILE_FACTION_TYPE, MONSTER_FACTION};
    ServerCommand::NewMonster(NewMonster {
        blueprint: "test".into(), health, max_health: 100, level: 1,
        faction: Faction { faction_id: MONSTER_FACTION, faction_type: HOSTILE_FACTION_TYPE, ..default() }, ..default()
    })
}

/// A Shadow Soul's NewMonster batch as the release261 server sends it to `viewer`
/// (tools/summon_vectors.py: soul_owner, soul_peer, soul_hostile).
fn soul_batch(viewer: &str) -> Vec<(ServerCommand, Option<u32>)> {
    let vectors: Vec<serde_json::Value> = serde_json::from_str(include_str!("../../../../crates/proto/tests/data/summon_release261.json")).unwrap();
    let v = vectors.into_iter().find(|v| v["name"] == viewer).unwrap();
    let hex = v["hex"].as_str().unwrap();
    let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
    dsor_proto::commands::decode_message(&bytes, v["bits"].as_u64().unwrap() as usize).unwrap()
}

#[test]
fn a_shadow_soul_comes_as_a_summon_and_leaves_through_its_despawn() {
    for (viewer, friendly) in [("soul_owner", true), ("soul_peer", true), ("soul_hostile", false)] {
        let mut app = app();
        let mut net = app.world_mut().non_send_mut::<Net>();
        for (command, actor) in soul_batch(viewer) {
            on_command(&mut net, command, actor);
        }
        let spawn = &net.monster_spawns[0];
        assert_eq!((spawn.actor, spawn.blueprint.as_str(), spawn.health, spawn.friendly), (0x10600, "mercenary_ancient_watcher", 5000, friendly), "{viewer}");
        // Its owner (us: actor 1), stamped when it appeared, turned as its owner
        // was (yaw 0.5).
        assert_eq!((spawn.summoner, spawn.category, spawn.spawn_tick), (if viewer == "soul_owner" { 1 } else { 0x2000001 }, 1, 123456));
        assert!((spawn.facing - (0.5 + std::f32::consts::PI)).abs() < 1e-6);
        assert!(net.summons.contains(&0x10600));
        on_command(&mut net, ServerCommand::DiscardMonster(default()), Some(0x10600));
        assert_eq!((net.summons_gone.as_slice(), net.remote_gone.as_slice()), (&[0x10600][..], &[][..]));
        // A monster of the map leaves at once, as before.
        on_command(&mut net, monster_snapshot(100), Some(41));
        assert!(net.monster_spawns.iter().any(|s| s.actor == 41 && s.summoner == 0 && !s.friendly));
        on_command(&mut net, ServerCommand::DiscardMonster(default()), Some(41));
        assert_eq!(net.remote_gone, [41]);
    }
}

/// The windows rebuild their views when ui_revision moves (docs/ui.md,
/// "Per-frame cost"): combat and movement, many a second, leave it alone;
/// what the windows show moves it.
#[test]
fn only_commands_the_windows_show_move_the_ui_revision() {
    let mut app = app();
    let mut net = app.world_mut().non_send_mut::<Net>();
    let before = net.ui_revision;
    for command in [
        ServerCommand::Move(default()),
        ServerCommand::MonsterUpdate(default()),
        ServerCommand::Hit(default()),
        ServerCommand::Kill(default()),
        monster_snapshot(100),
    ] {
        on_command(&mut net, command, Some(41));
    }
    assert_eq!(net.ui_revision, before, "the actor stream changes no window");
    for command in [
        ServerCommand::CurrencyChanged(default()),
        ServerCommand::XpChanged(default()),
        ServerCommand::AchievementInfo(default()),
        ServerCommand::RecipeBookInfo(default()),
    ] {
        let before = net.ui_revision;
        on_command(&mut net, command, None);
        assert_ne!(net.ui_revision, before);
    }
}

#[test]
fn monster_vicinity_health_survives_until_its_spawn_description_arrives() {
    let mut app = app();
    let mut net = app.world_mut().non_send_mut::<Net>();
    on_command(&mut net, ServerCommand::MonsterUpdate(dsor_proto::commands::actors::MonsterUpdate {
        health: 25, max_health: 100, ..default()
    }), Some(41));
    // The visual consumer used to discard this update because no entity exists.
    net.monster_health.clear();
    on_command(&mut net, monster_snapshot(100), Some(41));
    assert_eq!(net.monster_spawns[0].health, 25);
    assert_eq!(net.monster_vitals[&41], (25, 100));
}

#[test]
fn dead_monster_cannot_be_recreated_by_late_spawn_or_health_packets() {
    for death in [
        ServerCommand::Kill(default()),
        ServerCommand::Hit(dsor_proto::commands::combat::Hit { victim_health: 0, victim_max_health: 100, ..default() }),
        ServerCommand::MonsterUpdate(dsor_proto::commands::actors::MonsterUpdate { health: 0, max_health: 100, ..default() }),
        monster_snapshot(0),
    ] {
        let mut app = app();
        let mut net = app.world_mut().non_send_mut::<Net>();
        on_command(&mut net, monster_snapshot(100), Some(41));
        net.monster_spawns.clear();
        on_command(&mut net, death, Some(41));
        // Local corpse disposal must retain the server's death state.
        net.forget_actor(41);
        on_command(&mut net, ServerCommand::MonsterUpdate(dsor_proto::commands::actors::MonsterUpdate {
            health: 100, max_health: 100, ..default()
        }), Some(41));
        on_command(&mut net, monster_snapshot(100), Some(41));
        assert_eq!(net.monster_vitals[&41].0, 0);
        assert!(net.monster_spawns.is_empty());
        // An explicit discard ends that lifetime and permits actor-id reuse.
        net.discard_actor(41);
        on_command(&mut net, monster_snapshot(100), Some(41));
        assert_eq!(net.monster_spawns.len(), 1);
        assert_eq!(net.monster_vitals[&41], (100, 100));
    }
}

#[test]
fn discard_cancels_monsters_still_waiting_for_visual_assets() {
    for leaves_vicinity in [false, true] {
        let mut app = app();
        let mut net = app.world_mut().non_send_mut::<Net>();
        on_command(&mut net, monster_snapshot(100), Some(41));
        on_command(&mut net, ServerCommand::Kill(default()), Some(41));
        if leaves_vicinity {
            on_command(&mut net, ServerCommand::ActorsLeftVicinity(movement::ActorsLeftVicinity { actors: vec![41] }), None);
        } else {
            net.discard_actor(41);
        }
        assert!(net.monster_spawns.is_empty());
        assert!(net.monster_vitals.is_empty());
        assert!(net.monster_health.is_empty());
        assert!(net.kills.is_empty());
    }
}

#[test]
fn a_moving_record_is_walked_along_its_heading_for_its_duration() {
    let mut app = app();
    let e = actor(&mut app, Vec3::ZERO);
    // 5 units a second along +z, for one second.
    app.world_mut().non_send_mut::<Net>().remote_moves.push((41, Vec3::ZERO, 0., 100, Some((Vec3::Z * 5., 1.))));
    for _ in 0..10 { step(&mut app); }
    let z = app.world().get::<Transform>(e).unwrap().translation.z;
    assert!((z - 2.).abs() < 0.05, "0.4 s into its stretch, not standing at its start: {z}");
    for _ in 0..40 { step(&mut app); }
    assert_eq!(app.world().get::<RemotePlayer>(e).unwrap().target, Vec3::Z * 5., "the stretch ends with its duration");
    assert!((app.world().get::<Transform>(e).unwrap().translation.z - 5.).abs() < 1e-4);
}

#[test]
fn move_records_with_a_speed_carry_their_travel() {
    let mut app = app();
    let mut net = app.world_mut().non_send_mut::<Net>();
    net.remote_moves.clear();
    // A run (0x40) a quarter turn round (+x), for 25 ticks.
    let run = movement::Move { x: 128, speed: 0x40, heading: 64, facing: 64, start_tick: 7, duration: 25, ..default() };
    on_command(&mut net, ServerCommand::Move(run), Some(41));
    on_command(&mut net, ServerCommand::Move(movement::Move { speed: 0, start_tick: 8, ..run }), Some(41));
    let (actor, at, _, tick, travel) = net.remote_moves[0];
    assert_eq!((actor, at, tick), (41, net.wire_to_game(128, 0, 0), 7));
    let (velocity, seconds) = travel.unwrap();
    assert!((velocity - Vec3::X * RUN_SPEED).length() < 1e-4, "{velocity}");
    assert!((seconds - 1.).abs() < 1e-6);
    assert_eq!(net.remote_moves[1].4, None, "a standing record is a position only");
}

/// A status effect past its end tick goes, ours and another actor's, with the
/// removals its visuals and transformations follow; a permanent one stays.
/// FAILURE: "j'ai un slow ca me l'enleve jamais".
#[test]
fn status_effects_that_ran_out_are_removed() {
    use dsor_proto::commands::combat::{StatusEffectElement, StatusEffectTail};
    let mut app = app();
    let mut net = app.world_mut().non_send_mut::<Net>();
    let effect = |instance, end| StatusEffectElement { index: 5, instance, end, tail: Some(StatusEffectTail::default()), ..Default::default() };
    net.stat_effects.insert(1, effect(1, 100));
    net.stat_effects.insert(2, effect(2, 0));
    net.apply_actor_effects(41, false, &[effect(3, 100)]);
    net.status_events.clear();
    net.expire_effects(105);
    assert_eq!(net.stat_effects.len(), 2, "not yet: half a second of grace");
    net.expire_effects(200);
    assert!(!net.stat_effects.contains_key(&1) && net.stat_effects.contains_key(&2));
    assert!(!net.actor_effects.contains_key(&41));
    assert_eq!(net.status_events.iter().filter(|e| e.removed).count(), 2);
}
