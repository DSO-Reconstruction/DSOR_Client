//! Animation culling for every animated model that is not a dressed character
//! (crate::character manages its own): whole-model NPCs (barrel dealers,
//! gamblers...), animated map decor, effect models.
//!
//! bevy's animate_targets visits every bone hooked to a player (AnimatedBy), each
//! frame, whether or not anything is seen: in the browser it was the largest
//! single cost with only 3 of 84 characters animated (Chromium CPU profile of the
//! wasm build: QueryParIter<AnimationTargetId...> 6.5 %). A player away from the
//! camera's focus or off screen has its bones unhooked, and hooked again when it
//! comes back; its clip time keeps running (the player itself is untouched).

use std::collections::HashMap;

use bevy::animation::AnimatedBy;
use bevy::prelude::*;

/// Players crate::character drives itself.
#[derive(Component)]
pub struct ManagedAnimation;

/// Bones unhooked from a player while it is culled.
#[derive(Component, Default)]
struct Parked(Vec<Entity>);

/// Re-decided every this many frames (a model crossing the edge of the screen a
/// few frames late is not seen).
const EVERY: u32 = 8;

pub struct AnimCullPlugin;

impl Plugin for AnimCullPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, cull_animations.before(bevy::app::AnimationSystems));
    }
}

#[allow(clippy::type_complexity)]
fn cull_animations(
    mut commands: Commands,
    mut frame: Local<u32>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    players: Query<(Entity, &GlobalTransform, &InheritedVisibility, Option<&Parked>), (With<AnimationPlayer>, Without<ManagedAnimation>)>,
    targets: Query<(Entity, &AnimatedBy)>,
) {
    *frame = frame.wrapping_add(1);
    if *frame % EVERY != 0 {
        return;
    }
    let Some((cam, cam_at)) = cameras.iter().next() else { return };
    let mut decided: HashMap<Entity, bool> = HashMap::new();
    for (e, at, shown, parked) in &players {
        let p = at.translation();
        let near = cam_at.translation().distance(p) <= crate::map::CULL_DISTANCE + 25.0;
        let on_screen = cam.world_to_ndc(cam_at, p).is_some_and(|n| n.x.abs() < 1.5 && n.y.abs() < 1.5 && n.z > 0.0);
        let live = shown.get() && near && on_screen;
        match (live, parked) {
            (true, Some(Parked(bones))) => {
                for &b in bones {
                    commands.entity(b).try_insert(AnimatedBy(e));
                }
                commands.entity(e).remove::<Parked>();
            }
            (false, None) => {
                decided.insert(e, false);
            }
            _ => {}
        }
    }
    if decided.is_empty() {
        return;
    }
    let mut parked: HashMap<Entity, Vec<Entity>> = HashMap::new();
    for (bone, AnimatedBy(player)) in &targets {
        if decided.contains_key(player) {
            parked.entry(*player).or_default().push(bone);
        }
    }
    for (player, bones) in parked {
        for &b in &bones {
            commands.entity(b).remove::<AnimatedBy>();
        }
        commands.entity(player).insert(Parked(bones));
    }
}
