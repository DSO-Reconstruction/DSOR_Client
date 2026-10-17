use super::*;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<Ambience>()
        .init_resource::<Level>()
        .init_resource::<GpuAmbience>()
        .init_resource::<GlobalAmbientLight>()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(100)))
        // Existing native/browser preferences must not override bubble colour.
        .insert_resource(serde_json::from_str::<Lighting>(r#"{"saturation":3,"bloom":0,"vignette":0,"exposure":4,"tonemapping":true,"sun":0.8}"#).unwrap());
    app
}

fn look() -> Bubble {
    serde_json::from_str(r#"{"Saturation":0.25,"Balance":[0.5,0.75,1,1],"BloomScale":0.4,"BloomColor":[1,0.5,0.2,1],"BrightPassThreshold":0.8,"VignetteIntensity":0.7,"VignetteSize":3,"VignetteColor":[0.1,0.2,0.3,1]}"#).unwrap()
}

fn assert_look(app: &App, camera: Entity, bubble: &Bubble) {
    let post = app.world().get::<NebulaPostProcess>(camera).unwrap();
    assert_eq!(post.saturation, bubble.saturation);
    assert_eq!(post.balance.to_array(), bubble.balance);
    assert_eq!(post.scale, bubble.bloom_scale);
    assert_eq!(post.color.to_array(), bubble.bloom_color);
    assert_eq!(post.threshold, bubble.bright_pass_threshold);
    assert_eq!(post.vignette_intensity, bubble.vignette_intensity);
    assert_eq!(post.vignette_size, bubble.vignette_size);
    assert_eq!(post.vignette_color.to_array(), bubble.vignette_color);
    assert_eq!(app.world().get::<Tonemapping>(camera), Some(&Tonemapping::None));
    let grading = app.world().get::<ColorGrading>(camera).unwrap();
    assert_eq!(grading.global.exposure, 0.);
    assert_eq!(grading.global.post_saturation, 1.);
    assert!(app.world().get::<Bloom>(camera).is_none());
    assert!(app.world().get::<Vignette>(camera).is_none());
}

#[test]
fn bubble_controls_camera_colour_despite_legacy_preferences_and_zero_bloom() {
    let mut app = app();
    app.add_systems(Update, apply_look);
    let camera = app.world_mut().spawn((Camera3d::default(), Bloom::default(), Vignette::default())).id();
    let thumbnail = app.world_mut().spawn((Camera3d::default(), NoLevelLook)).id();
    let highlight = app.world_mut().spawn((Camera3d::default(), crate::rendering::highlight::HighlightCamera)).id();
    let bubble = look();
    app.world_mut().resource_mut::<Level>().bubble = bubble.clone();
    app.update();
    assert_look(&app, camera, &bubble);
    assert!(app.world().get::<NebulaPostProcess>(thumbnail).is_none());
    assert!(app.world().get::<NebulaPostProcess>(highlight).is_none());
    assert_eq!(app.world().resource::<Lighting>().sun, 0.8);
    // Added camera monitors inherit the active look even without a zone change.
    let monitor = app.world_mut().spawn(Camera3d::default()).id();
    app.update();
    assert_look(&app, monitor, &bubble);
    let mut no_bloom = bubble;
    no_bloom.bloom_scale = 0.;
    app.world_mut().resource_mut::<Level>().bubble = no_bloom.clone();
    app.update();
    assert_look(&app, camera, &no_bloom);
    assert_look(&app, monitor, &no_bloom);
    let old_asset: Bubble = serde_json::from_str(r#"{"Saturation":0.6}"#).unwrap();
    assert_eq!(old_asset.balance, [1.; 4]);
}

#[test]
fn camera_colours_follow_zone_entry_interrupted_fades_and_exit() {
    let mut app = app();
    app.add_systems(Update, (follow_level, apply_look).chain());
    let global = Bubble { saturation: 1., balance: [1.; 4], fade_time: 1., ..look() };
    let local = Bubble { saturation: 0., balance: [0.4, 0.6, 1., 1.], bloom_scale: 0., fade_time: 1., ..look() };
    let handle = app.world_mut().resource_mut::<Assets<Ambience>>().add(Ambience {
        global: Some(global.clone()),
        bubbles: vec![LocalBubble { look: local.clone(), m: Some(Mat4::from_scale(Vec3::splat(8.)).to_cols_array()),
            priority: 1, event: -1, shape: "Box".into(), enabled: true }], ..default()
    });
    app.insert_resource(CurrentMap::new("test".into(), Handle::default()))
        .insert_resource(Level { map: "test".into(), ambience: handle, ..default() });
    let outside = GlobalTransform::from_translation(Vec3::new(0., 0., 10.));
    let camera = app.world_mut().spawn((Camera3d::default(), outside)).id();
    app.update();
    assert_look(&app, camera, &global);
    app.world_mut().entity_mut(camera).insert(GlobalTransform::IDENTITY);
    for _ in 0..4 { app.update(); }
    let halfway = app.world().get::<NebulaPostProcess>(camera).unwrap().saturation;
    assert!(halfway > 0. && halfway < 1.);
    let balance = app.world().get::<NebulaPostProcess>(camera).unwrap().balance;
    assert!(balance.x > 0.4 && balance.x < 1.);
    // Reverse during entry: continue from the visible colour, with no snap.
    app.world_mut().entity_mut(camera).insert(outside);
    app.update();
    let reversed = app.world().get::<NebulaPostProcess>(camera).unwrap().saturation;
    assert!(reversed > halfway && reversed < 1.);
    for _ in 0..12 { app.update(); }
    assert_look(&app, camera, &global);
    app.world_mut().entity_mut(camera).insert(GlobalTransform::IDENTITY);
    for _ in 0..12 { app.update(); }
    assert_look(&app, camera, &local);
    app.world_mut().entity_mut(camera).insert(outside);
    for _ in 0..12 { app.update(); }
    assert_look(&app, camera, &global);
}

#[test]
fn editor_live_colour_changes_reach_scene_and_monitor() {
    let mut app = app();
    app.init_resource::<EditorLook>()
        .add_systems(Update, (copy_editor_look, apply_look).chain());
    let scene = app.world_mut().spawn(Camera3d::default()).id();
    let monitor = app.world_mut().spawn(Camera3d::default()).id();
    for bubble in [look(), Bubble { saturation: 0., balance: [1., 0.2, 0.3, 1.], ..look() }] {
        app.world_mut().resource_mut::<EditorLook>().bubble = bubble.clone();
        app.update();
        assert_look(&app, scene, &bubble);
        assert_look(&app, monitor, &bubble);
    }
}

#[test]
fn death_and_revival_preserve_the_bubble_and_update_new_cameras() {
    let mut app = app();
    app.add_systems(Update, (apply_look, death_look).chain());
    crate::network::net::start_offline(app.world_mut(), 1, 0);
    let camera = app.world_mut().spawn(Camera3d::default()).id();
    app.world_mut().resource_mut::<Level>().bubble = look();
    app.update();
    app.world_mut().non_send_mut::<crate::network::net::Net>().death.dead = true;
    for _ in 0..14 { app.update(); }
    assert_eq!(app.world().get::<NebulaPostProcess>(camera).unwrap().death_fade, 1.);
    let monitor = app.world_mut().spawn(Camera3d::default()).id();
    app.update();
    assert_eq!(app.world().get::<NebulaPostProcess>(monitor).unwrap().death_fade, 1.);
    app.world_mut().resource_mut::<Level>().bubble.saturation = 0.8;
    app.update();
    assert_eq!(app.world().get::<NebulaPostProcess>(camera).unwrap().saturation, 0.8);
    assert_eq!(app.world().get::<NebulaPostProcess>(camera).unwrap().death_fade, 1.);
    app.world_mut().non_send_mut::<crate::network::net::Net>().death.dead = false;
    for _ in 0..14 { app.update(); }
    assert_eq!(app.world().get::<NebulaPostProcess>(camera).unwrap().death_fade, 0.);
    assert_eq!(app.world().get::<NebulaPostProcess>(camera).unwrap().saturation, 0.8);
}

#[test]
fn lighting_menu_edits_resolve_bubbles_at_once_and_keep_zone_fades() {
    use crate::diagnostics::lighting_menu::presets::{AreaLightingOverride, BubbleOverrides, LightingMenuStore, MapLightingOverrides};
    let mut app = app();
    app.add_systems(Update, (follow_level, apply_look).chain());
    let global = Bubble { saturation: 1., fade_time: 1., ..look() };
    let local = Bubble { saturation: 0., fade_time: 1., ..look() };
    let m = Some(Mat4::from_scale(Vec3::splat(8.)).to_cols_array());
    let handle = app.world_mut().resource_mut::<Assets<Ambience>>().add(Ambience {
        global: Some(global.clone()),
        bubbles: vec![LocalBubble { look: local.clone(), m, priority: 1, event: -1, shape: "Box".into(), enabled: true }], ..default()
    });
    app.insert_resource(CurrentMap::new("test".into(), Handle::default()))
        .insert_resource(Level { map: "test".into(), ambience: handle, ..default() })
        .insert_resource(LightingMenuStore::default());
    let outside = GlobalTransform::from_translation(Vec3::new(0., 0., 10.));
    let camera = app.world_mut().spawn((Camera3d::default(), outside)).id();
    app.update();
    // No overrides: the level's own look.
    assert_look(&app, camera, &global);
    // A map-wide edit shows at once, without a fade.
    app.world_mut().resource_mut::<LightingMenuStore>().working_map_overrides.insert("test".into(), MapLightingOverrides {
        global: BubbleOverrides { saturation: Some(0.5), ..default() },
        areas: vec![AreaLightingOverride { bubble_index: 0, event: -1, priority: 1, shape: "Box".into(), transform: m,
            values: BubbleOverrides { saturation: Some(0.25), ..default() } }],
    });
    app.update();
    assert_look(&app, camera, &Bubble { saturation: 0.5, ..global.clone() });
    // Entering the zone still fades, to its edited look.
    app.world_mut().entity_mut(camera).insert(GlobalTransform::IDENTITY);
    for _ in 0..4 { app.update(); }
    let halfway = app.world().get::<NebulaPostProcess>(camera).unwrap().saturation;
    assert!(halfway > 0.25 && halfway < 0.5, "{halfway}");
    for _ in 0..12 { app.update(); }
    assert_eq!(app.world().get::<NebulaPostProcess>(camera).unwrap().saturation, 0.25);
}

#[test]
fn lighting_menu_preview_shows_a_zone_from_outside_event_zones_included() {
    use crate::diagnostics::lighting_menu::presets::LightingMenuStore;
    let mut app = app();
    app.add_systems(Update, (follow_level, apply_look).chain());
    let global = Bubble { saturation: 1., fade_time: 0.1, ..look() };
    let local = Bubble { saturation: 0., fade_time: 0.1, ..look() };
    let event = Bubble { saturation: 0.5, fade_time: 0.1, ..look() };
    let m = Some(Mat4::from_scale(Vec3::splat(8.)).to_cols_array());
    let handle = app.world_mut().resource_mut::<Assets<Ambience>>().add(Ambience {
        global: Some(global.clone()),
        bubbles: vec![
            LocalBubble { look: local.clone(), m, priority: 1, event: -1, shape: "Box".into(), enabled: true },
            LocalBubble { look: event.clone(), m, priority: 1, event: 83, shape: "Box".into(), enabled: true },
        ], ..default()
    });
    app.insert_resource(CurrentMap::new("test".into(), Handle::default()))
        .insert_resource(Level { map: "test".into(), ambience: handle, ..default() })
        .insert_resource(LightingMenuStore::default());
    let camera = app.world_mut().spawn((Camera3d::default(), GlobalTransform::from_translation(Vec3::new(0., 0., 10.)))).id();
    app.update();
    assert_look(&app, camera, &global);
    for (target, expected) in [(Some(0), &local), (Some(1), &event), (None, &global)] {
        app.world_mut().resource_mut::<LightingMenuStore>().preview_target = Some(target);
        for _ in 0..4 { app.update(); }
        assert_eq!(app.world().get::<NebulaPostProcess>(camera).unwrap().saturation, expected.saturation, "{target:?}");
    }
    // Preview off: back to the zone the observer is in (none here).
    app.world_mut().resource_mut::<LightingMenuStore>().preview_target = Some(Some(0));
    for _ in 0..4 { app.update(); }
    app.world_mut().resource_mut::<LightingMenuStore>().preview_target = None;
    for _ in 0..4 { app.update(); }
    assert_look(&app, camera, &global);
}
