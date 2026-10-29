use super::*;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
use bevy::ecs::system::RunSystemOnce;
use std::time::Duration;

fn sequence_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<Gltf>().init_asset::<bevy::world_serialization::WorldAsset>()
        .init_resource::<CameraShake>().init_resource::<crate::actors::monsters::ShaderColors>()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)))
        .add_systems(Update, play_sequences);
    app
}

fn cast_sequence(app: &mut App, actor: Entity) -> Entity {
    let sequence = Sequence {
        length: 25, repeat: false,
        tracks: vec![
            Track::Anim { name: "Cast".into(), speed: 1.0, start: 0, end: 25 },
            Track::Fx { graphics: "tests/cast_effect".into(), joint: String::new(),
                at: [0., 0., -2., 0., 0., 0., 1., 1., 1.], start: 0, end: 25, curves: default() },
            // A later animation must not reclaim the actor after movement starts.
            Track::Anim { name: "FollowThrough".into(), speed: 1.0, start: 15, end: 25 },
        ],
    };
    app.world_mut().run_system_once(move |mut commands: Commands|
        play(&mut commands, sequence.clone(), Some(actor), actor, false)).unwrap()
}

#[test]
fn cast_effect_survives_local_and_remote_movement_until_its_track_ends() {
    for class in 0..4 {
        for remote in [false, true] {
            let mut app = sequence_app();
            let actor = app.world_mut().run_system_once(move |mut commands: Commands|
                crate::actors::character::spawn_character(&mut commands,
                    crate::actors::character::CharacterDesc { class, ..default() }, Transform::default())).unwrap();
            if remote {
                app.world_mut().entity_mut(actor).insert(RemotePlayer { still_for: 1.0, ..default() });
            } else {
                app.world_mut().entity_mut(actor).insert(LocalPlayer::default());
            }
            let sequence = cast_sequence(&mut app, actor);
            for _ in 0..4 { app.update(); }
            let effect = app.world().get::<SequencePlayer>(sequence).unwrap().spawned[1].unwrap();
            let placement = *app.world().get::<Transform>(effect).unwrap();
            assert!(app.world().get::<ChildOf>(effect).is_none());
            app.world_mut().get_mut::<Transform>(actor).unwrap().translation = Vec3::X * 5.0;
            app.world_mut().get_mut::<CharacterAnim>(actor).unwrap().state = AnimState::Run;
            if remote {
                app.world_mut().get_mut::<RemotePlayer>(actor).unwrap().still_for = 0.0;
            } else {
                app.world_mut().get_mut::<LocalPlayer>(actor).unwrap().target = Some(Vec3::X * 10.0);
            }
            for _ in 0..5 {
                app.update();
                assert!(app.world().get_entity(effect).is_ok(), "class {class}, remote {remote}: walking removed the effect");
                assert_eq!(*app.world().get::<Transform>(effect).unwrap(), placement);
                assert_eq!(app.world().get::<CharacterAnim>(actor).unwrap().state, AnimState::Run);
                assert!(!app.world().get::<SequencePlayer>(sequence).unwrap().has_pending_animation());
            }
            for _ in 0..5 { app.update(); }
            assert!(app.world().get_entity(effect).is_err(), "effects must still expire");
            assert!(app.world().get_entity(sequence).is_err());
            assert_eq!(app.world().get::<CharacterAnim>(actor).unwrap().state, AnimState::Run);
        }
    }
}

#[test]
fn superseded_cast_effect_plays_out_without_resetting_the_new_cast_animation() {
    let mut app = sequence_app();
    let actor = app.world_mut().spawn((GlobalTransform::default(), CharacterAnim::default())).id();
    let first = cast_sequence(&mut app, actor);
    for _ in 0..4 { app.update(); }
    let first_effect = app.world().get::<SequencePlayer>(first).unwrap().spawned[1].unwrap();
    let second = cast_sequence(&mut app, actor);
    for _ in 0..5 { app.update(); }
    assert!(app.world().get_entity(first_effect).is_ok());
    assert_eq!(app.world().get::<CharacterAnim>(actor).unwrap().state, AnimState::Named("Cast".into()));
    for _ in 0..3 { app.update(); }
    assert!(app.world().get_entity(first_effect).is_err());
    assert!(app.world().get_entity(second).is_ok());
    assert_eq!(app.world().get::<CharacterAnim>(actor).unwrap().state, AnimState::Named("FollowThrough".into()));
}

#[test]
fn stopped_dead_or_removed_caster_still_cleans_up_its_sequence() {
    for reason in ["stop", "death", "removed"] {
        let mut app = sequence_app();
        let actor = app.world_mut().spawn((GlobalTransform::default(), CharacterAnim::default())).id();
        let sequence = cast_sequence(&mut app, actor);
        for _ in 0..4 { app.update(); }
        let effect = app.world().get::<SequencePlayer>(sequence).unwrap().spawned[1].unwrap();
        match reason {
            "stop" => app.world_mut().get_mut::<SequencePlayer>(sequence).unwrap().stop = true,
            "death" => app.world_mut().get_mut::<CharacterAnim>(actor).unwrap().dead = true,
            _ => { app.world_mut().despawn(actor); }
        }
        app.update();
        assert!(app.world().get_entity(effect).is_err(), "{reason}");
        assert!(app.world().get_entity(sequence).is_err(), "{reason}");
    }
}

fn casting_app(class: u8) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(crate::ui::input::UiInputPlugin)
        .init_resource::<ButtonInput<MouseButton>>().init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Assets<NavMesh>>().init_resource::<Assets<SkillTable>>()
        .init_resource::<Assets<SequenceTable>>().init_resource::<Time<Real>>()
        .init_resource::<crate::actors::monsters::Hovered>().init_resource::<crate::actors::monsters::DebugMonsters>()
        .init_resource::<crate::ui::SkillClicks>().init_resource::<crate::ui::Settings>()
        .init_resource::<crate::gameplay::stats::CharacterStats>()
        .init_resource::<crate::gameplay::skills::SkillCooldowns>()
        // Costs are a fraction of this maximum (default 100): 0.1 below is 10.
        .init_resource::<crate::gameplay::stats::ResourceMax>()
        .add_systems(Update, cast_input);
    crate::net::start_offline(app.world_mut(), class, 0);
    let skill: SkillDef = serde_json::from_value(serde_json::json!({
        "id": "repeat_attack", "class": crate::actors::character::CLASSES[class as usize],
        "type": "Ranged", "hit_frame": 1, "loop_start": 0, "unblock": 5,
        "motion_unblock": 5, "range": 20.0, "cooldown": 0.4, "resource_cost": 0.1,
        "execute": {}, "impact": "",
        "bullet": { "loop": "", "impact": "", "death": "", "motion": "Straight",
            "velocity": 10.0, "lifetime": 2.0, "radius": 0.2, "start": [0.0, 1.0, -1.0] }
    })).unwrap();
    let skills = app.world_mut().resource_mut::<Assets<SkillTable>>().add(SkillTable([(1, skill)].into_iter().collect()));
    let sequences = app.world_mut().resource_mut::<Assets<SequenceTable>>().add(SequenceTable(default()));
    app.insert_resource(SkillData { skills, sequences, status: default(), emotes: default(), by_id: [("repeat_attack".into(), 1)].into_iter().collect() });
    {
        let mut net = app.world_mut().non_send_mut::<Net>();
        net.bar[1] = Some("repeat_attack".into());
        net.resource = Some(100.0);
    }
    let mut window = Window { resolution: (1024, 768).into(), ..default() };
    window.set_cursor_position(Some(Vec2::new(400.0, 400.0)));
    app.world_mut().spawn((window, PrimaryWindow));
    app.world_mut().spawn((Camera3d::default(), Camera {
        computed: ComputedCameraValues {
            clip_from_view: Mat4::perspective_infinite_reverse_rh(1.0, 1024.0 / 768.0, 0.1),
            target_info: Some(RenderTargetInfo { physical_size: UVec2::new(1024, 768), scale_factor: 1.0 }),
            ..default()
        }, ..default()
    }, GlobalTransform::from(Transform::from_xyz(0., 10., 10.).looking_at(Vec3::ZERO, Vec3::Y))));
    let actor = app.world_mut().run_system_once(move |mut commands: Commands|
        crate::actors::character::spawn_character(&mut commands,
            crate::actors::character::CharacterDesc { class, ..default() }, Transform::default())).unwrap();
    app.world_mut().entity_mut(actor).insert(LocalPlayer::default());
    (app, actor)
}

fn advance_casting(app: &mut App, actor: Entity, seconds: f32) {
    app.world_mut().resource_mut::<Time<Real>>().advance_by(Duration::from_secs_f32(seconds));
    app.world_mut().get_mut::<LocalPlayer>(actor).unwrap().casting -= seconds;
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.update();
}

#[test]
fn held_right_click_repeats_across_ui_for_every_class_and_respects_cast_limits() {
    for class in 0..4 {
        let (mut app, actor) = casting_app(class);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        app.update();
        assert_eq!(app.world().non_send::<Net>().resource, Some(90.0));
        let panel = app.world_mut().spawn((crate::ui::input::UiRegions(vec![(0., [350., 300., 500., 500.])]), Visibility::Inherited)).id();
        advance_casting(&mut app, actor, 0.1);
        assert_eq!(app.world().non_send::<Net>().resource, Some(90.0), "cast lock must prevent repeats");
        advance_casting(&mut app, actor, 0.15);
        assert_eq!(app.world().non_send::<Net>().resource, Some(90.0), "cooldown must prevent repeats");
        advance_casting(&mut app, actor, 0.2);
        assert_eq!(app.world().non_send::<Net>().resource, Some(80.0), "class {class}: UI interrupted the held cast");
        app.world_mut().despawn(panel);
        advance_casting(&mut app, actor, 0.5);
        assert_eq!(app.world().non_send::<Net>().resource, Some(70.0));
        assert_eq!(app.world_mut().query::<&Pending>().iter(app.world()).count(), 3);

        app.world_mut().non_send_mut::<Net>().resource = Some(5.0);
        advance_casting(&mut app, actor, 0.5);
        assert_eq!(app.world().non_send::<Net>().resource, Some(5.0), "insufficient resource must stop casts");
        app.world_mut().non_send_mut::<Net>().resource = Some(10.0);
        advance_casting(&mut app, actor, 0.1);
        assert_eq!(app.world().non_send::<Net>().resource, Some(0.0), "holding retries when resource returns");

        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
        app.world_mut().non_send_mut::<Net>().resource = Some(100.0);
        advance_casting(&mut app, actor, 0.5);
        assert_eq!(app.world().non_send::<Net>().resource, Some(100.0), "release must stop repeats");
    }
}

#[test]
fn right_click_started_on_ui_cannot_cast_when_dragged_into_world() {
    let (mut app, actor) = casting_app(1);
    let panel = app.world_mut().spawn((crate::ui::input::UiRegions(vec![(0., [350., 300., 500., 500.])]), Visibility::Inherited)).id();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
    app.update();
    assert_eq!(app.world().non_send::<Net>().resource, Some(100.0));
    app.world_mut().despawn(panel);
    advance_casting(&mut app, actor, 0.5);
    assert_eq!(app.world().non_send::<Net>().resource, Some(100.0));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
    advance_casting(&mut app, actor, 0.5);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
    app.update();
    assert_eq!(app.world().non_send::<Net>().resource, Some(90.0));
}

#[test]
fn a_skill_without_a_motion_hold_keeps_the_walk_going() {
    let (mut app, actor) = casting_app(0);
    let handle = app.world().resource::<SkillData>().skills.clone();
    app.world_mut().resource_mut::<Assets<SkillTable>>().get_mut(&handle).unwrap().0.get_mut(&1).unwrap().motion_unblock = 0;
    app.world_mut().get_mut::<LocalPlayer>(actor).unwrap().target = Some(Vec3::new(5.0, 0.0, 5.0));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
    app.update();
    assert_eq!(app.world().non_send::<Net>().resource, Some(90.0), "cast");
    let player = app.world().get::<LocalPlayer>(actor).unwrap();
    assert_eq!(player.target, Some(Vec3::new(5.0, 0.0, 5.0)), "the walk goes on");
    assert_eq!(player.casting, 0.0, "no movement hold");
}

#[test]
fn an_effect_animators_run_at_its_sequence_rate() {
    // Rageful Swing at 6.25x: its slash's MatEmissiveIntensity animator ran at
    // 1x and the track ended before it lit.
    let mut app = App::new();
    let fx = app.world_mut().spawn(FxModel { gltf: Handle::default(), started: true, looping: false, speed: 6.25 }).id();
    let glow = app.world_mut().spawn((crate::rendering::materials::ShaderAnim::default(), ChildOf(fx))).id();
    let scroll = app.world_mut().spawn((crate::rendering::materials::UvAnim::default(), ChildOf(glow))).id();
    let plain = app.world_mut().spawn(ChildOf(fx)).id();
    app.world_mut().run_system_once(fx_particle_speed).unwrap();
    app.world_mut().flush();
    for node in [glow, scroll] {
        assert_eq!(app.world().get::<crate::rendering::materials::AnimTimeScale>(node).map(|r| r.0), Some(6.25));
    }
    assert!(app.world().get::<crate::rendering::materials::AnimTimeScale>(plain).is_none());
    // At 1x nothing is set.
    let slow = app.world_mut().spawn(FxModel { gltf: Handle::default(), started: true, looping: false, speed: 1.0 }).id();
    let node = app.world_mut().spawn((crate::rendering::materials::ShaderAnim::default(), ChildOf(slow))).id();
    app.world_mut().run_system_once(fx_particle_speed).unwrap();
    assert!(app.world().get::<crate::rendering::materials::AnimTimeScale>(node).is_none());
}
